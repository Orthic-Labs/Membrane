use cortex_store::temporal::{
    TemporalFactStore, TemporalInstantV1, TemporalQueryOutcomeV1,
    TemporalValidityV1,
};
use cortex_store::MemDb;
use rusqlite::params;

const T0: &str = "2026-08-01T00:00:00Z";
const T1: &str = "2026-08-02T00:00:00Z";
const T2: &str = "2026-08-03T00:00:00Z";

fn record(
    id: &str,
    value: &str,
    valid_at: TemporalInstantV1,
    recorded_at: TemporalInstantV1,
    authority: &str,
    independently_verified: bool,
    revoked: bool,
) -> TemporalValidityV1 {
    TemporalValidityV1 {
        record_id: id.into(),
        subject: "repo".into(),
        predicate: "owner".into(),
        object: serde_json::json!(value),
        scope_id: "scope-a".into(),
        authority: authority.into(),
        valid_at,
        recorded_at,
        invalid_at: None,
        superseded_by: None,
        revoked,
        independently_verified,
        expires_at: None,
    }
}

fn store() -> TemporalFactStore {
    TemporalFactStore::new(MemDb::open_in_memory())
}

#[test]
fn valid_at_is_not_defaulted_to_ingest_and_unknown_is_typed_unavailable() {
    let store = store();
    let ingest = TemporalInstantV1::known(T2);
    store
        .record_validity(
            record(
                "unknown-authored",
                "owner-a",
                TemporalInstantV1::unavailable("authored_time_not_supplied"),
                ingest.clone(),
                "A1",
                false,
                false,
            ),
            false,
        )
        .unwrap();

    let outcome = store
        .query_validity(vec!["scope-a".into()], "repo".into(), "owner".into(), T2.into())
        .unwrap();
    let TemporalQueryOutcomeV1 { records, conflict } = outcome;
    assert!(conflict.is_none());
    assert_eq!(records[0].recorded_at, ingest);
    assert_eq!(
        records[0].valid_at,
        TemporalInstantV1::Unavailable {
            reason: "authored_time_not_supplied".into()
        }
    );
}

#[test]
fn recorded_at_and_valid_at_remain_distinct_across_round_trip() {
    let store = store();
    store
        .record_validity(
            record(
                "distinct-times",
                "owner-a",
                TemporalInstantV1::known(T0),
                TemporalInstantV1::known(T2),
                "A1",
                false,
                false,
            ),
            false,
        )
        .unwrap();
    let records = store
        .query_validity(vec!["scope-a".into()], "repo".into(), "owner".into(), T1.into())
        .unwrap()
        .records;
    assert_eq!(records[0].valid_at, TemporalInstantV1::known(T0));
    assert_eq!(records[0].recorded_at, TemporalInstantV1::known(T2));
}

#[test]
fn supersession_marks_loser_without_deleting_it() {
    let db = MemDb::open_in_memory();
    let store = TemporalFactStore::new(db.clone());
    store
        .record_validity(
            record("old", "owner-a", TemporalInstantV1::known(T0), TemporalInstantV1::known(T0), "A1", false, false),
            true,
        )
        .unwrap();
    store
        .record_validity(
            record("new", "owner-b", TemporalInstantV1::known(T1), TemporalInstantV1::known(T2), "A1", false, false),
            true,
        )
        .unwrap();

    let row: (i64, Option<String>, Option<String>) = db
        .lock()
        .query_row(
            "SELECT COUNT(*), invalid_at, superseded_by FROM membrane_temporal_fact WHERE fact_id='old'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, (1, Some(T1.into()), Some("new".into())));
}

#[test]
fn as_of_recall_keeps_the_historical_record_after_later_supersession() {
    let store = store();
    store
        .record_validity(
            record("old", "owner-a", TemporalInstantV1::known(T0), TemporalInstantV1::known(T0), "A1", false, false),
            true,
        )
        .unwrap();
    store
        .record_validity(
            record("new", "owner-b", TemporalInstantV1::known(T1), TemporalInstantV1::known(T2), "A1", false, false),
            true,
        )
        .unwrap();

    let before = store
        .query_validity(vec!["scope-a".into()], "repo".into(), "owner".into(), "2026-08-01T12:00:00Z".into())
        .unwrap();
    let after = store
        .query_validity(vec!["scope-a".into()], "repo".into(), "owner".into(), "2026-08-02T12:00:00Z".into())
        .unwrap();
    assert_eq!(before.records[0].record_id, "old");
    assert_eq!(after.records[0].record_id, "new");
}

#[test]
fn lineage_is_queryable_across_a_supersession_chain() {
    let store = store();
    for (id, valid_at) in [("a", T0), ("b", T1), ("c", T2)] {
        store
            .record_validity(
                record(id, id, TemporalInstantV1::known(valid_at), TemporalInstantV1::known(valid_at), "A1", false, false),
                true,
            )
            .unwrap();
    }
    let lineage = store.lineage("b").unwrap();
    assert_eq!(
        lineage.iter().map(|item| item.record_id.as_str()).collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
}

#[test]
fn read_order_is_revoked_then_valid_time_then_authority_then_verification() {
    let store = store();
    let inputs = [
        // If revoked were not step one, this would win on every later tie-breaker.
        record("revoked", "revoked-value", TemporalInstantV1::known(T0), TemporalInstantV1::known(T0), "A5", true, true),
        // If valid-time were not step two, this future record would win by authority.
        record("future", "future-value", TemporalInstantV1::known(T2), TemporalInstantV1::known(T0), "A5", true, false),
        // Authority must precede independent verification: this is the winner.
        record("authority", "authority-value", TemporalInstantV1::known(T0), TemporalInstantV1::known(T0), "A2", false, false),
        record("verified", "verified-value", TemporalInstantV1::known(T0), TemporalInstantV1::known(T0), "A1", true, false),
    ];
    for item in inputs {
        store.record_validity(item, false).unwrap();
    }
    let outcome = store
        .query_validity(vec!["scope-a".into()], "repo".into(), "owner".into(), T1.into())
        .unwrap();
    assert_eq!(outcome.records[0].record_id, "authority");
}

#[test]
fn unresolved_conflict_is_typed_and_never_blended() {
    let store = store();
    for id in ["left", "right"] {
        store
            .record_validity(
                record(id, if id == "left" { "owner-a" } else { "owner-b" }, TemporalInstantV1::known(T0), TemporalInstantV1::known(T0), "A2", true, false),
                false,
            )
            .unwrap();
    }
    let outcome = store
        .query_validity(vec!["scope-a".into()], "repo".into(), "owner".into(), T1.into())
        .unwrap();
    assert!(outcome.records.is_empty());
    let conflict = outcome.conflict.unwrap();
    assert_eq!(conflict.reason, "unresolved_temporal_conflict");
    assert_eq!(conflict.record_ids, vec!["left", "right"]);
}

#[test]
fn old_temporal_fact_and_effective_ms_rows_migrate_in_place() {
    let db = MemDb::open_in_memory();
    // Insert using only the old TemporalFact columns, before the V1 lane has
    // had a chance to add or populate canonical columns.
    db.lock()
        .execute(
            "INSERT INTO membrane_temporal_fact
             (fact_id,subject,predicate,object_json,scope_id,authority,veracity,observed_at,
              valid_from,valid_until,expires_at,supersedes,payload_sha256,transition_sha256)
             VALUES (?1,?2,?3,?4,?5,?6,'supported',?7,?8,NULL,NULL,NULL,'legacy-hash','')",
            params!["legacy-fact", "repo", "owner", "\"legacy-owner\"", "scope-a", "A1", T1, T0],
        )
        .unwrap();
    db.lock()
        .execute(
            "INSERT INTO memories (id,tier,content,keywords,score,created_at,access_count,scope_id,effective_from_ms,effective_until_ms,superseded_by)
             VALUES (?1,'Semantic',?2,'[]',1.0,?3,0,'scope-a',?4,NULL,NULL)",
            params!["legacy-memory", "legacy content", T2, 0_i64],
        )
        .unwrap();
    let store = TemporalFactStore::new(db.clone());

    let migrated_fact = store
        .query_validity(vec!["scope-a".into()], "repo".into(), "owner".into(), T1.into())
        .unwrap();
    assert_eq!(migrated_fact.records[0].record_id, "legacy-fact");
    let migrated_memory = store.memory_validity("legacy-memory").unwrap().unwrap();
    assert_eq!(migrated_memory.valid_at, TemporalInstantV1::known("1970-01-01T00:00:00.000Z"));
    assert_eq!(migrated_memory.recorded_at, TemporalInstantV1::known(T2));
}
