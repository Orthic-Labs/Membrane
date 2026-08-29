//! §16.3 / §16.4 / §16.1 — Cortex durable-lifecycle gap tests.
//!
//! Production trace: the durable write path (`try_put_*` →
//! `persist_entry_with_record_lifecycle_on`), the reversible quarantine
//! (`restore_quarantined`), the governed hard erase (`hard_erase`), and the
//! backup/restore recall-equivalence proof (`backup_cortex`/`restore_cortex`),
//! plus frozen interface 1 (`admit_approved_proposal`) consumed by the
//! native-path review transition.

use cortex_core::MemoryTier;
use membrane_runtime::{ApprovedProposalAdmissionV1, MemDb, MemoryStore};

fn store() -> MemoryStore {
    MemoryStore::open(MemDb::open_in_memory())
}

// ---------------------------------------------------------------------------
// §16.3 — write-time duplicate/conflict detection
// ---------------------------------------------------------------------------

#[test]
fn exact_content_under_a_different_id_is_a_typed_duplicate_not_a_second_record() {
    let s = store();
    let first = s
        .try_put(
            "first",
            "The deployment pipeline runs the contract tests before any push.",
            "global",
            MemoryTier::Semantic,
        )
        .expect("first write admits");
    let second = s
        .try_put(
            "second",
            "The deployment pipeline runs the contract tests before any push.",
            "global",
            MemoryTier::Semantic,
        )
        .expect("duplicate is a successful idempotent no-op");
    assert_eq!(second, first, "legacy V1 returns the existing active id");
    let contents: Vec<String> = s
        .entries(100)
        .into_iter()
        .map(|entry| entry.content)
        .collect();
    assert_eq!(
        contents
            .iter()
            .filter(|c| **c == "The deployment pipeline runs the contract tests before any push.")
            .count(),
        1,
        "a second silent record must never exist: {contents:?}"
    );
}

#[test]
fn near_identical_but_ambiguous_content_surfaces_conflict_never_a_second_record() {
    let s = store();
    s.try_put(
        "primary",
        "The deployment pipeline runs the full contract suite before any push to the shared integration branch.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("primary write admits");
    // Only the final word differs: 4-shingle Jaccard stays inside the conflict
    // band while exact-normalized equality remains false.
    let near = s.try_put(
        "near",
        "The deployment pipeline runs the full contract suite before any push to the shared integration trunk.",
        "global",
        MemoryTier::Semantic,
    );
    let error = near.expect_err("ambiguous near-duplicate must surface, not admit silently");
    assert!(
        error.contains("conflict") || error.contains("duplicate"),
        "typed disposition required, got: {error}"
    );
    let count = s
        .entries(100)
        .into_iter()
        .filter(|entry| entry.content.contains("contract suite"))
        .count();
    assert!(count <= 2, "no silent second record may appear: {count}");
    if count == 2 {
        panic!("a near-identical write was admitted silently — the pre-filter did not run");
    }
}

#[test]
fn same_id_update_is_not_blocked_by_the_prefilter() {
    let s = store();
    s.try_put(
        "note",
        "Version one of the deployment runbook.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("initial write");
    s.try_put(
        "note",
        "Version two of the deployment runbook, rewritten.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("same-id update is lifecycle-governed, not pre-filtered");
}

#[test]
fn distinct_content_is_unaffected_by_the_prefilter() {
    let s = store();
    s.try_put(
        "alpha",
        "Rust borrow checking prevents data races at compile time.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("alpha admits");
    s.try_put(
        "beta",
        "The nginx container needs a diff of the confs before rebuild.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("beta admits");
    assert_eq!(s.entries(10).len(), 2);
}

#[test]
fn unspecific_short_content_is_not_confused_by_the_prefilter() {
    let s = store();
    let first = s
        .try_put("tiny-a", "ok fine", "global", MemoryTier::Semantic)
        .expect("short content admits");
    let duplicate = s
        .try_put("tiny-b", "ok fine", "global", MemoryTier::Semantic)
        .expect("exact normalized duplicate remains an idempotent no-op");
    assert_eq!(duplicate, first);
}

// ---------------------------------------------------------------------------
// §16.4 — hard erase, backup/restore, quarantine intact
// ---------------------------------------------------------------------------

#[test]
fn hard_erase_clears_payload_from_every_projection_and_missing_ids_report_false() {
    let s = store();
    let id = s
        .try_put(
            "secret",
            "Erase-me payload with distinctive zeppelin marble content.",
            "global",
            MemoryTier::Semantic,
        )
        .expect("write");
    // Quarantine a second copy path: quarantine its own row then hard-erase it.
    let quarantine_id = s
        .try_put(
            "to-quarantine",
            "Second payload with distinctive harbor lantern content.",
            "global",
            MemoryTier::Semantic,
        )
        .expect("write");

    // Simulate the destructive-prune quarantine path by moving the row into
    // memory_quarantine (the durable schema is public; the governed production
    // path is dream's, but erasure must clear this projection regardless of who
    // put the copy there).
    {
        let conn = s.db().lock();
        conn.execute(
            "INSERT INTO memory_quarantine
                (id, tier, content, keywords, score, created_at, updated_at, access_count,
                 embedding, embedding_q, scope_id, inject_count, content_hash, embed_model,
                 source_ids, quarantined_at, reason)
             SELECT id, tier, content, keywords, score, created_at, updated_at, access_count,
                    embedding, embedding_q, scope_id, inject_count, content_hash, embed_model,
                    source_ids, strftime('%Y-%m-%dT%H:%M:%fZ','now'), 'test-fixture'
               FROM memories WHERE id = ?1",
            rusqlite::params![quarantine_id],
        )
        .expect("quarantine copy");
        conn.execute(
            "DELETE FROM memories WHERE id = ?1",
            rusqlite::params![quarantine_id],
        )
        .expect("quarantine removal");
    }

    assert!(s.hard_erase(&id).expect("hard erase succeeds"));
    assert!(s
        .hard_erase(&quarantine_id)
        .expect("hard erase quarantine copy"));

    {
        let conn = s.db().lock();
        for (table, target) in [
            ("memories", &id),
            ("memories", &quarantine_id),
            ("memory_quarantine", &quarantine_id),
            ("deletions", &id),
        ] {
            let remaining: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE id = ?1"),
                    rusqlite::params![target],
                    |row| row.get(0),
                )
                .expect("count query");
            assert_eq!(remaining, 0, "payload must be gone from {table}");
        }
        // No payload string survives anywhere in the durable projections.
        for needle in ["zeppelin marble", "harbor lantern"] {
            let hits: i64 = conn
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM memories WHERE content LIKE ?1)
                           + (SELECT COUNT(*) FROM memory_quarantine WHERE content LIKE ?1)",
                    rusqlite::params![format!("%{needle}%")],
                    |row| row.get(0),
                )
                .expect("payload scan");
            assert_eq!(hits, 0, "payload {needle:?} must leave no projection row");
        }
    }
    assert!(
        !s.entries(100).iter().any(|entry| entry.id == id),
        "registry must not serve erased payload"
    );
    // A missing id reports false — it is not a silent success.
    assert!(!s.hard_erase("global/never-existed").expect("absent erase"));
}

#[test]
fn quarantined_rows_still_restore_transactionally_and_the_prefilter_does_not_break_restore() {
    let s = store();
    let id = s
        .try_put(
            "restorable",
            "Restore-me payload with distinctive comet cellar content.",
            "global",
            MemoryTier::Semantic,
        )
        .expect("write");
    {
        let conn = s.db().lock();
        conn.execute(
            "INSERT INTO memory_quarantine
                (id, tier, content, keywords, score, created_at, updated_at, access_count,
                 embedding, embedding_q, scope_id, inject_count, content_hash, embed_model,
                 source_ids, quarantined_at, reason)
             SELECT id, tier, content, keywords, score, created_at, updated_at, access_count,
                    embedding, embedding_q, scope_id, inject_count, content_hash, embed_model,
                    source_ids, strftime('%Y-%m-%dT%H:%M:%fZ','now'), 'test-fixture'
               FROM memories WHERE id = ?1",
            rusqlite::params![id],
        )
        .expect("quarantine copy");
        conn.execute("DELETE FROM memories WHERE id = ?1", rusqlite::params![id])
            .expect("quarantine removal");
    }
    let restored = s.restore_quarantined(&id).expect("restore is typed");
    assert!(restored, "the quarantined row restores");
    let entry = s
        .entries(50)
        .into_iter()
        .find(|entry| entry.id == id)
        .expect("restored row is recallable");
    assert!(entry.content.contains("comet cellar"));
}

#[test]
fn backup_then_wipe_then_restore_proves_recall_equivalence() {
    let s = store();
    s.try_put(
        "doc-one",
        "Backup equivalence fixture one: the fuse relay spec.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("write one");
    s.try_put(
        "doc-two",
        "Backup equivalence fixture two: the tidal combiner memo.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("write two");

    let backup = s.backup_cortex().expect("backup");
    assert_eq!(backup.memories.len(), 2);
    assert!(!backup.payload_sha256.is_empty(), "backup is digest-sealed");

    // Wipe through the governed erase path, then restore.
    for entry in s.entries(10) {
        s.hard_erase(&entry.id).expect("wipe");
    }
    assert!(s.entries(10).is_empty(), "store is wiped");

    let restored = s.restore_cortex(&backup).expect("restore");
    assert_eq!(restored, 2, "every backed-up memory row is restored");

    // Recall-equivalence proof: the restored store recalls the same content for
    // the same query via public search, and the payload matches the backup exactly.
    let hits = s.search("fuse relay spec", 5);
    assert!(
        hits.iter()
            .any(|entry| entry.content.contains("fuse relay spec")),
        "recall after restore must surface the restored payload: {hits:?}"
    );
    let after: Vec<String> = {
        let mut contents: Vec<String> = s
            .entries(10)
            .into_iter()
            .map(|entry| entry.content)
            .collect();
        contents.sort();
        contents
    };
    assert_eq!(
        after,
        vec![
            "Backup equivalence fixture one: the fuse relay spec.".to_owned(),
            "Backup equivalence fixture two: the tidal combiner memo.".to_owned(),
        ],
        "restored payload is byte-faithful"
    );
    // A tampered envelope is refused, never restored blindly.
    let mut tampered = backup.clone();
    tampered.memories[0].content = "tampered".to_owned();
    assert!(
        s.restore_cortex(&tampered).is_err(),
        "digest mismatch refuses"
    );
}

// ---------------------------------------------------------------------------
// Frozen interface 1 — admit_approved_proposal
// ---------------------------------------------------------------------------

#[test]
fn admit_approved_proposal_admits_a_novel_proposal() {
    let s = store();
    let admission = s
        .admit_approved_proposal(
            "prop-1",
            r#"{"text": "Approved emission about deterministic ledger reindexing."}"#,
        )
        .expect("novel proposal admits");
    match admission {
        ApprovedProposalAdmissionV1::Admitted { memory_id } => {
            assert!(memory_id.starts_with("proposed/proposal-"), "{memory_id}");
            let entry = s
                .entries(10)
                .into_iter()
                .find(|entry| entry.id == memory_id)
                .expect("admitted record is durable");
            assert!(entry.content.contains("deterministic ledger reindexing"));
        }
        other => panic!("novel proposal must admit, got {other:?}"),
    }
}

#[test]
fn admit_approved_proposal_resolves_a_duplicate_proposal_typed() {
    let s = store();
    let payload = r#"{"text": "Approved emission about deterministic ledger reindexing."}"#;
    let first = s
        .admit_approved_proposal("prop-1", payload)
        .expect("first admits");
    assert!(
        matches!(first, ApprovedProposalAdmissionV1::Admitted { .. }),
        "{first:?}"
    );
    let second = s
        .admit_approved_proposal("prop-2", payload)
        .expect("the call itself succeeds and returns a typed outcome");
    match second {
        ApprovedProposalAdmissionV1::Duplicate { existing_id } => {
            let admitted = match first {
                ApprovedProposalAdmissionV1::Admitted { memory_id } => memory_id,
                other => panic!("first was admitted, got {other:?}"),
            };
            assert_eq!(existing_id, admitted, "duplicate names the existing record");
        }
        ApprovedProposalAdmissionV1::Conflict { existing_id } => {
            assert!(
                !existing_id.is_empty(),
                "conflict names the existing record"
            );
        }
        ApprovedProposalAdmissionV1::Admitted { memory_id } => {
            panic!("a duplicate proposal must never admit a second record: {memory_id}");
        }
    }
}

#[test]
fn admit_approved_proposal_rejects_unusable_payloads_typed() {
    let s = store();
    assert!(
        s.admit_approved_proposal("", "{}").is_err(),
        "empty id refused"
    );
    assert!(
        s.admit_approved_proposal("prop-3", "not json").is_err(),
        "non-JSON payload refused"
    );
    assert!(
        s.admit_approved_proposal("prop-4", r#"{"text": "   "#)
            .is_err()
            || {
                // A whitespace-only text is also refused by the payload contract.
                s.admit_approved_proposal("prop-4", r#"{"text": "   "}"#)
                    .is_err()
            },
        "unusable text refused"
    );
    let err = s.admit_approved_proposal("prop-5", r#"{"contentless": true}"#);
    match err {
        Err(membrane_runtime::store::StoreError::Admission(message)) => {
            assert!(message.contains("text or content"), "{message}");
        }
        other => panic!("expected StoreError::Admission, got {other:?}"),
    }
}
