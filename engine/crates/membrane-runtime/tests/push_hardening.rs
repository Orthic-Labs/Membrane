use membrane_runtime::push::recovery::{self, RecoveryError, RecoveryScope, RecoveryStore, Selector};
use rusqlite::{params, Connection};

#[test]
fn legacy_content_addressed_store_migrates_without_breaking_old_handles() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("push-artifacts.sqlite");
    let scope = RecoveryScope::new(temp.path(), "legacy-session").unwrap();
    let bytes = b"legacy exact bytes\r\n";
    let source_hash = recovery::digest(bytes);
    let legacy_handle = format!("mr://anchor/{source_hash}");

    {
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE push_store (id INTEGER PRIMARY KEY CHECK(id=1), identity TEXT NOT NULL);\n\
                 INSERT INTO push_store VALUES(1, 'legacy-store');\n\
                 CREATE TABLE push_originals (\n\
                   scope TEXT NOT NULL, digest TEXT NOT NULL, content BLOB NOT NULL,\n\
                   size INTEGER NOT NULL, created INTEGER NOT NULL, expires INTEGER NOT NULL,\n\
                   invalidated INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(scope,digest));\n\
                 CREATE INDEX push_expiry ON push_originals(expires);",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO push_originals(scope,digest,content,size,created,expires,invalidated) VALUES(?1,?2,?3,?4,100,200,0)",
                params![scope.binding(), source_hash, bytes, bytes.len() as i64],
            )
            .unwrap();
    }

    let store = RecoveryStore::at(temp.path());
    let restored = store
        .resolve(&scope, &legacy_handle, &Selector::Whole, 128, 150)
        .unwrap();
    assert_eq!(restored.bytes().unwrap(), bytes);
    assert_eq!(restored.reference.handle, legacy_handle);
    assert_eq!(restored.reference.source_digest, format!("sha256:{source_hash}"));

    // Active duplicate publication preserves the legacy handle and never renews.
    let duplicate = store.publish(&scope, bytes, 1_000, 150).unwrap();
    assert_eq!(duplicate.handle, legacy_handle);
    assert_eq!(duplicate.expires_at, 200);

    // Once terminal, the payload row is reclaimed and identical content gets a
    // fresh opaque handle. The historical handle remains terminal, never revived.
    let replacement = store.publish(&scope, bytes, 1_000, 201).unwrap();
    assert_ne!(replacement.handle, legacy_handle);
    assert_eq!(replacement.source_digest, format!("sha256:{source_hash}"));
    assert!(matches!(
        store.resolve(&scope, &legacy_handle, &Selector::Whole, 128, 201),
        Err(RecoveryError::Expired)
    ));
    assert_eq!(
        store
            .resolve(&scope, &replacement.handle, &Selector::Whole, 128, 201)
            .unwrap()
            .bytes()
            .unwrap(),
        bytes
    );
}

#[test]
fn expired_history_does_not_permanently_consume_the_active_object_quota() {
    let temp = tempfile::tempdir().unwrap();
    let scope = RecoveryScope::new(temp.path(), "quota-session").unwrap();
    let store = RecoveryStore::at(temp.path());
    store.identity().unwrap(); // initialize/migrate schema

    let db_path = temp.path().join("push-artifacts.sqlite");
    {
        let mut connection = Connection::open(&db_path).unwrap();
        let transaction = connection.transaction().unwrap();
        {
            let mut insert = transaction
                .prepare(
                    "INSERT INTO push_originals(scope,digest,handle_digest,content,size,created,expires,invalidated) VALUES(?1,?2,?3,x'',0,0,1,0)",
                )
                .unwrap();
            for index in 0u64..4096 {
                let source = format!("{index:064x}");
                let handle = format!("{:064x}", index + 4096);
                insert.execute(params![scope.binding(), source, handle]).unwrap();
            }
        }
        transaction.commit().unwrap();
    }

    // Publication compacts terminal rows before quota accounting, so historical
    // churn cannot leave the store permanently wedged at MAX_ARTIFACTS.
    let reference = store.publish(&scope, b"fresh payload", 1_000, 2).unwrap();
    assert_eq!(
        store
            .resolve(&scope, &reference.handle, &Selector::Whole, 128, 2)
            .unwrap()
            .bytes()
            .unwrap(),
        b"fresh payload"
    );

    let connection = Connection::open(&db_path).unwrap();
    let active: i64 = connection
        .query_row("SELECT COUNT(*) FROM push_originals", [], |row| row.get(0))
        .unwrap();
    let tombstones: i64 = connection
        .query_row("SELECT COUNT(*) FROM push_tombstones", [], |row| row.get(0))
        .unwrap();
    assert_eq!(active, 1);
    assert_eq!(tombstones, 4096);
}
