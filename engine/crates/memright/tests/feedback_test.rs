//! Feedback rail end-to-end: persisted per-candidate feedback survives a restart, a verified
//! `contradicted` vetoes the entry, a cited verdict without a ref is rejected fail-closed, and the
//! upsert is idempotent by `(trace_id, candidate_id)`.

use memright::feedback::{FeedbackRecord, FeedbackSource};
use memright::memdb::MemDb;
use memright::store::{MemoryEventContext, MemoryStore};
use memright_core::{EffectivenessGate, Outcome};

fn temp_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("cr-feedback-{tag}-{}.db", std::process::id()))
}

fn observed(trace: &str, candidate: &str, outcome: Outcome) -> FeedbackRecord {
    FeedbackRecord {
        trace_id: trace.into(),
        candidate_id: candidate.into(),
        content_sha256: "sha-of-body".into(),
        outcome,
        source: FeedbackSource::ObservedAction,
        verdict_ref: None,
        scope_id: "global".into(),
    }
}

#[test]
fn verified_contradicted_persists_and_vetoes_across_restart() {
    let path = temp_path("veto");
    let _ = std::fs::remove_file(&path);
    let id;
    {
        let m = MemoryStore::open(MemDb::open(&path).unwrap());
        let entry = m.remember(
            "the box nginx is dockerized; diff confs before a rebuild",
            vec![],
        );
        id = entry.id.clone();
        // No feedback yet -> the gate allows the entry.
        assert!(EffectivenessGate::default().should_inject(&m.feedback_rows_from_db(), &id));
        // The agent recalled it and found it wrong -> an observed per-candidate contradiction.
        m.record_feedback(&observed("turn-1", &id, Outcome::Contradicted))
            .unwrap();
    }
    // Reopen == a serve restart. The in-RAM history is gone; the DB veto must survive.
    let reloaded = MemoryStore::open(MemDb::open(&path).unwrap());
    let rows = reloaded.feedback_rows_from_db();
    assert_eq!(rows.len(), 1, "feedback row must persist across restart");
    assert!(
        !EffectivenessGate::default().should_inject(&rows, &id),
        "a verified contradicted must veto the entry across a restart"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn cited_verdict_without_ref_is_rejected_and_not_persisted() {
    let path = temp_path("reject");
    let _ = std::fs::remove_file(&path);
    let m = MemoryStore::open(MemDb::open(&path).unwrap());
    let bad = FeedbackRecord {
        source: FeedbackSource::CitedVerdict,
        verdict_ref: None, // fail-closed: a ranking verdict must cite a resolvable ref
        ..observed("turn-2", "mem-x", Outcome::Contradicted)
    };
    assert!(
        m.record_feedback(&bad).is_err(),
        "cited verdict with no ref must be rejected"
    );
    assert!(
        m.feedback_rows_from_db().is_empty(),
        "a rejected feedback record must not persist"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn get_records_retrieval_but_never_fabricates_verified_use() {
    let m = MemoryStore::new();
    let e = m.remember("prefer edit-on-laptop then git pull on the box", vec![]);
    let ctx = MemoryEventContext::new("http").with_trace("turn-get");
    m.get_full_observed(&e.id, &ctx).unwrap();
    assert!(
        m.feedback_rows_from_db().is_empty(),
        "retrieval is not proof that the memory helped the downstream task"
    );
    let retrieved: i64 = m
        .db()
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM context_event_log
              WHERE artifact_id IS NOT NULL AND phase='candidate.retrieved'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retrieved, 1);
}

#[test]
fn explicit_feedback_links_to_the_canonical_retrieval_trace() {
    let m = MemoryStore::new();
    let e = m.remember("linked feedback memory", vec![]);
    let ctx = MemoryEventContext::new("http").with_trace("raw retrieval trace");
    m.get_full_observed(&e.id, &ctx).unwrap();
    m.record_feedback(&FeedbackRecord {
        trace_id: "raw retrieval trace".into(),
        candidate_id: e.id,
        content_sha256: "a".repeat(64),
        outcome: Outcome::Used,
        source: FeedbackSource::ObservedAction,
        verdict_ref: None,
        scope_id: "global".into(),
    })
    .unwrap();
    let conn = m.db().lock();
    let links: Vec<String> = conn
        .prepare("SELECT relation FROM context_event_link ORDER BY relation")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(links.iter().any(|relation| relation == "feedback_for"));
    assert!(links.iter().any(|relation| relation == "outcome_for"));
    let traces: Vec<String> = conn
        .prepare(
            "SELECT DISTINCT trace_id FROM context_event_log \
             WHERE phase IN ('candidate.retrieved', 'candidate.used')",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(traces.len(), 1);
    assert!(traces[0].starts_with("trace-") && traces[0].len() == 38);
}

#[test]
fn get_without_trace_records_no_feedback() {
    let m = MemoryStore::new();
    let e = m.remember("a memory with no recall trace", vec![]);
    m.get_full_observed(&e.id, &MemoryEventContext::default())
        .unwrap(); // trace_id: None
    assert!(
        m.feedback_rows_from_db().is_empty(),
        "a bare get (no trace) must not spuriously record feedback"
    );
}

#[test]
fn delete_auto_captures_verified_contradicted() {
    let m = MemoryStore::new();
    let e = m.remember("this memory turned out to be wrong", vec![]);
    let ctx = MemoryEventContext::new("http").with_trace("turn-del");
    assert!(m.try_delete_observed(&e.id, &ctx).unwrap());
    let rows = m.feedback_rows_from_db();
    assert_eq!(
        rows.len(),
        1,
        "deleting a recalled memory must record one contradiction"
    );
    assert!(rows[0].verified && matches!(rows[0].outcome, Some(Outcome::Contradicted)));
}

#[test]
fn feedback_rows_for_is_bounded_to_requested_candidates() {
    let m = MemoryStore::new();
    m.record_feedback(&observed("t", "mem-a", Outcome::Contradicted))
        .unwrap();
    m.record_feedback(&observed("t", "mem-b", Outcome::Used))
        .unwrap();
    m.record_feedback(&observed("t", "mem-c", Outcome::Used))
        .unwrap();
    // Ask for only two of the three — the scan must not return the third.
    let rows = m.feedback_rows_for(&["mem-a".to_string(), "mem-b".to_string()]);
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|r| r.entry_id == "mem-a" || r.entry_id == "mem-b"));
    // Empty request short-circuits.
    assert!(m.feedback_rows_for(&[]).is_empty());
    // The bounded read still surfaces the veto for the requested candidate.
    assert!(
        !EffectivenessGate::default().should_inject(&rows, "mem-a"),
        "bounded read must still carry the verified veto"
    );
}

#[test]
fn metrics_expose_feedback_block() {
    let m = MemoryStore::new();
    m.record_feedback(&observed("t", "mem-a", Outcome::Used))
        .unwrap();
    let vetoed = m
        .try_put(
            "mem-b",
            "current contradicted body",
            "global",
            memright_core::MemoryTier::Semantic,
        )
        .unwrap();
    let current_sha: String = m
        .db()
        .lock()
        .query_row(
            "SELECT content_hash FROM memories WHERE id=?1",
            [&vetoed],
            |row| row.get(0),
        )
        .unwrap();
    let mut contradiction = observed("t", &vetoed, Outcome::Contradicted);
    contradiction.content_sha256 = current_sha;
    m.record_feedback(&contradiction).unwrap();
    m.record_feedback(&FeedbackRecord {
        source: FeedbackSource::Advisory,
        ..observed("t", "mem-c", Outcome::Contradicted)
    })
    .unwrap();
    let fb = &m.metrics_json()["feedback"];
    assert_eq!(fb["verified_used"], 1);
    assert_eq!(fb["verified_contradicted"], 1);
    assert_eq!(fb["advisory_total"], 1);
    assert_eq!(fb["active_vetoes"], 1);
    assert_eq!(fb["distinct_candidates_with_feedback"], 3);
}

#[test]
fn upsert_is_idempotent_by_trace_and_candidate() {
    let path = temp_path("idem");
    let _ = std::fs::remove_file(&path);
    let m = MemoryStore::open(MemDb::open(&path).unwrap());
    m.record_feedback(&observed("turn-3", "mem-y", Outcome::Used))
        .unwrap();
    m.record_feedback(&observed("turn-3", "mem-y", Outcome::Used))
        .unwrap(); // same key again
    assert_eq!(
        m.feedback_rows_from_db().len(),
        1,
        "a re-run at compaction must upsert, not double-count"
    );
    let _ = std::fs::remove_file(&path);
}
