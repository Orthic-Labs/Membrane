//! Parity coverage for `blueprint/src/graph/confidence-migration.mjs`
//! (legacy test: `blueprint/tests/confidence-migration.test.mjs`).
//!
//! `engine/crates/membrane-blueprint/src/migrations.rs` is already a direct
//! Rust port of the store's schema migrations (its own module doc says so)
//! and its `make_nullable` helper implements the same nullable-confidence
//! rule the legacy `migrateNullableFactConfidence` does: schema-18->19 makes
//! `symbols.confidence` / `edges.confidence` nullable, schema-19->20 does
//! the same for `claim_code_edges.confidence`, in both cases preserving
//! rowids, indexes and triggers verbatim. This file proves that behavior
//! against fixtures shaped like the legacy JS test's fixture, without
//! touching `migrations.rs` itself.

use membrane_blueprint::migrations::{migrate, migrate_to, schema_version, MigrationError};
use rusqlite::Connection;

fn fixture_at_v18() -> Connection {
    let mut db = Connection::open_in_memory().unwrap();
    migrate_to(&mut db, 18).unwrap();
    db.execute_batch(
        "CREATE INDEX idx_symbols_confidence_fixture ON symbols(node_ordinal);
         CREATE INDEX idx_edges_confidence_fixture ON edges(id);
         CREATE TABLE audit (id TEXT);
         CREATE TRIGGER symbol_confidence_audit AFTER INSERT ON symbols BEGIN INSERT INTO audit VALUES (new.id); END;
         INSERT INTO symbols(rowid,id,kind,labels,name,qualified_name,path,confidence,evidence,generation_id,node_ordinal)
             VALUES (7,'legacy','Function','[]','f','f','a.ts',0.78,'[]','g',2);
         INSERT INTO edges(rowid,id,kind,source,target,confidence,evidence,generation_id)
             VALUES (11,'legacy-edge','CALLS','a','b',1,'[]','g');",
    )
    .unwrap();
    db
}

#[test]
fn nullable_confidence_migration_preserves_data_rowids_indexes_and_triggers() {
    let mut db = fixture_at_v18();

    let before_symbols: Vec<(i64, String)> =
        db.prepare("SELECT rowid,id FROM symbols").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    let before_edges: Vec<(i64, String)> =
        db.prepare("SELECT rowid,id FROM edges").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();

    assert_eq!(migrate(&mut db).unwrap(), 20);
    assert_eq!(schema_version(&db).unwrap(), 20);

    let after_symbols: Vec<(i64, String)> =
        db.prepare("SELECT rowid,id FROM symbols").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    let after_edges: Vec<(i64, String)> =
        db.prepare("SELECT rowid,id FROM edges").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(before_symbols, after_symbols);
    assert_eq!(before_edges, after_edges);

    for table in ["symbols", "edges"] {
        let notnull: i64 = db
            .query_row(&format!("SELECT \"notnull\" FROM pragma_table_info('{table}') WHERE name='confidence'"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(notnull, 0, "{table}.confidence should be nullable after migration");
    }

    db.execute("INSERT INTO symbols(id,kind,labels,name,qualified_name,path,confidence,evidence,generation_id,node_ordinal) VALUES ('authoritative','Function','[]','a','a','a.ts',NULL,'[]','g',3)", []).unwrap();
    db.execute("INSERT INTO edges(id,kind,source,target,confidence,evidence,generation_id) VALUES ('authoritative-edge','CALLS','a','b',NULL,'[]','g')", []).unwrap();

    let confidence: Option<f64> = db.query_row("SELECT confidence FROM symbols WHERE id='authoritative'", [], |r| r.get(0)).unwrap();
    assert_eq!(confidence, None);

    let audit_count: i64 = db.query_row("SELECT COUNT(*) FROM audit WHERE id='authoritative'", [], |r| r.get(0)).unwrap();
    assert_eq!(audit_count, 1, "trigger must have survived the table rebuild");

    let index_count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('idx_symbols_confidence_fixture','idx_edges_confidence_fixture')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(index_count, 2, "fixture indexes must have survived the table rebuild");

    let fk_violations: i64 = db.query_row("PRAGMA foreign_key_check", [], |r| r.get(0)).unwrap_or(0);
    assert_eq!(fk_violations, 0);
}

#[test]
fn nullable_confidence_migration_is_idempotent() {
    let mut db = fixture_at_v18();
    migrate(&mut db).unwrap();
    let before: Vec<(String, String, Option<String>)> = db
        .prepare("SELECT type,name,sql FROM sqlite_master ORDER BY name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    // Re-running migrate on an already-current DB is a no-op (no pending
    // versions), matching the legacy migration's idempotence.
    assert_eq!(migrate(&mut db).unwrap(), 20);
    let after: Vec<(String, String, Option<String>)> = db
        .prepare("SELECT type,name,sql FROM sqlite_master ORDER BY name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(before, after);
}

#[test]
fn claim_code_edges_confidence_becomes_nullable_at_schema_20() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate_to(&mut db, 19).unwrap();
    db.execute_batch(
        "INSERT INTO documents(id,path,generation_id) VALUES ('d','doc.md','g');
         INSERT INTO claims(id,document_id,text,generation_id) VALUES ('c','d','claim text','g');
         INSERT INTO claim_code_edges(id,claim_id,kind,source,target,confidence,generation_id)
             VALUES ('cce','c','implements','doc.md','a.ts',0.9,'g');",
    )
    .unwrap();
    migrate(&mut db).unwrap();
    assert_eq!(schema_version(&db).unwrap(), 20);
    let notnull: i64 = db
        .query_row("SELECT \"notnull\" FROM pragma_table_info('claim_code_edges') WHERE name='confidence'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(notnull, 0);
    db.execute("UPDATE claim_code_edges SET confidence=NULL WHERE id='cce'", []).unwrap();
    let confidence: Option<f64> = db.query_row("SELECT confidence FROM claim_code_edges WHERE id='cce'", [], |r| r.get(0)).unwrap();
    assert_eq!(confidence, None);
}

#[test]
fn failure_on_the_second_table_rolls_back_the_first_table_and_its_indexes() {
    // Legacy test: "failure on the second table rolls back the first table
    // and its indexes" (blueprint/tests/confidence-migration.test.mjs).
    // `make_nullable("symbols", ...)` succeeds and creates its temp table,
    // then `make_nullable("edges", "edges_nullable_confidence_v19")`
    // collides with a pre-existing table of that name. Because both calls
    // happen inside the single per-version `rusqlite::Transaction` that
    // `migrate_to` opens for schema 18->19, the failure must roll back the
    // symbols rebuild too: schema stays at 18 and both tables are untouched.
    let mut db = fixture_at_v18();
    db.execute_batch("CREATE TABLE edges_nullable_confidence_v19 (collision TEXT)").unwrap();

    let before_notnull: i64 = db
        .query_row("SELECT \"notnull\" FROM pragma_table_info('symbols') WHERE name='confidence'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before_notnull, 1);

    let err = migrate(&mut db).unwrap_err();
    assert!(matches!(err, MigrationError::Sqlite(_)), "expected a raw sqlite collision error, got {err:?}");

    // Schema version must not have advanced past 18, and the symbols table
    // (the first table processed) must be exactly as it was before the
    // failed transaction, not left mid-rebuild.
    assert_eq!(schema_version(&db).unwrap(), 18);
    let notnull: i64 = db
        .query_row("SELECT \"notnull\" FROM pragma_table_info('symbols') WHERE name='confidence'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(notnull, 1, "symbols.confidence must still be NOT NULL after the rolled-back migration");
    let symbols_rowid: i64 = db.query_row("SELECT rowid FROM symbols WHERE id='legacy'", [], |r| r.get(0)).unwrap();
    assert_eq!(symbols_rowid, 7);
    // The collision placeholder table must still be there, untouched.
    let collision_count: i64 = db
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='edges_nullable_confidence_v19'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(collision_count, 1);
}

#[test]
fn unexpected_schema_aborts_with_a_typed_migration_failure() {
    // Legacy test: "unexpected schema aborts with a typed migration failure"
    // (blueprint/tests/confidence-migration.test.mjs): a table with no
    // `confidence` column is a schema mismatch, never a silent no-op.
    let mut db = fixture_at_v18();
    db.execute_batch(
        "CREATE TABLE edges_v18_backup AS SELECT * FROM edges;
         DROP TABLE edges;
         CREATE TABLE edges (id TEXT PRIMARY KEY, kind TEXT, source TEXT, target TEXT, evidence TEXT, generation_id TEXT);",
    )
    .unwrap();
    let err = migrate(&mut db).unwrap_err();
    match err {
        MigrationError::Failed { from, to, detail } => {
            assert_eq!((from, to), (19, 20));
            assert!(detail.contains("confidence_migration_schema_mismatch"), "{detail}");
        }
        other => panic!("expected typed schema mismatch, got {other:?}"),
    }
    assert!(schema_version(&db).unwrap() < 20);
}
