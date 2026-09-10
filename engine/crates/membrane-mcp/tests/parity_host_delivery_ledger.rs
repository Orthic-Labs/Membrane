//! Parity for legacy `mcp/host/host-delivery-ledger.test.mjs`.

use membrane_mcp::host_delivery_ledger_store::*;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn fresh_process_emits_once_omits_same_hash_and_emits_changed_ledger() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let key = "session:session-A";
    let hash = format!("sha256:{}", "a".repeat(64));

    let entries = vec![
        DeliveredEntry { id: "rules:AGENTS.md".into(), delivery_mode: "inline".into(), source_hash: hash.clone(), bytes: 16 },
        DeliveredEntry { id: "git:meta".into(), delivery_mode: "inline".into(), source_hash: hash.clone(), bytes: 16 },
    ];
    let first = persist(key, &entries, root);
    assert_eq!(first.written, 2);

    // "fresh child" hydrates and must observe both as already delivered.
    let hydrated = hydrate(
        key,
        &[
            Candidate { id: "rules:AGENTS.md".into(), source_hash: hash.clone() },
            Candidate { id: "git:meta".into(), source_hash: hash.clone() },
        ],
        root,
    );
    assert_eq!(hydrated.matched.len(), 2);

    // Changed source hash re-delivers (persists as a new record).
    let changed_hash = format!("sha256:{}", "b".repeat(64));
    let changed = persist(
        key,
        &[DeliveredEntry { id: "rules:AGENTS.md".into(), delivery_mode: "inline".into(), source_hash: changed_hash.clone(), bytes: 7 }],
        root,
    );
    assert_eq!(changed.written, 1);

    // A different ledger key never observes the first session's records.
    let other = hydrate(
        "other-ledger",
        &[Candidate { id: "rules:AGENTS.md".into(), source_hash: hash }],
        root,
    );
    assert_eq!(other.matched.len(), 0);
}

#[test]
fn twelve_parallel_first_writers_land_exactly_one_record_per_block() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let key = "session:session-C".to_string();
    let hash = format!("sha256:{}", "c".repeat(64));
    let expected = record_filename("rules:AGENTS.md", &hash);

    let handles: Vec<_> = (0..12)
        .map(|_| {
            let root = root.clone();
            let key = key.clone();
            let hash = hash.clone();
            std::thread::spawn(move || {
                persist(&key, &[DeliveredEntry { id: "rules:AGENTS.md".into(), delivery_mode: "inline".into(), source_hash: hash, bytes: 32 }], &root)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().map(|r| r.written).sum::<usize>(), 1);
    assert_eq!(results.iter().map(|r| r.existing).sum::<usize>(), 11);
    let entries: Vec<_> = std::fs::read_dir(session_dir(&key, &root)).unwrap().filter_map(Result::ok).collect();
    assert_eq!(entries.len(), 1);
    assert!(entries.iter().any(|e| e.file_name().to_string_lossy() == expected));
}

#[test]
fn malicious_neighbours_fail_open_without_suppressing_a_legitimate_write() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let key = "session:session-D";
    let hash = format!("sha256:{}", "d".repeat(64));
    let directory = session_dir(key, root);
    std::fs::create_dir_all(&directory).unwrap();
    let legit_name = record_filename("rules:AGENTS.md", &hash);
    std::fs::write(directory.join(&legit_name), "{ not valid json").unwrap();

    let probe = hydrate(key, &[Candidate { id: "rules:AGENTS.md".into(), source_hash: hash.clone() }], root);
    assert_eq!(probe.matched.len(), 0);
    assert!(probe.diagnostic.is_some());

    std::fs::remove_file(directory.join(&legit_name)).unwrap();
    let written = persist(key, &[DeliveredEntry { id: "rules:AGENTS.md".into(), delivery_mode: "inline".into(), source_hash: hash.clone(), bytes: 16 }], root);
    assert!(written.written >= 1);

    let probe2 = hydrate(key, &[Candidate { id: "rules:AGENTS.md".into(), source_hash: hash }], root);
    assert_eq!(probe2.matched.len(), 1);
}

#[test]
fn outbox_enqueue_is_idempotent_bounded_and_deadline_checked() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let now = 5_000i64;
    assert!(matches!(
        enqueue_outbox(root, EnqueueParams::new("mutation-1", json!({"op": "a"}), now + 60_000, now)),
        EnqueueOutcome::Enqueued(_)
    ));
    assert!(matches!(
        enqueue_outbox(root, EnqueueParams::new("mutation-1", json!({"op": "a"}), now + 60_000, now)),
        EnqueueOutcome::Duplicate
    ));
    assert!(matches!(
        enqueue_outbox(root, EnqueueParams::new("mutation-1", json!({"op": "changed"}), now + 60_000, now)),
        EnqueueOutcome::Conflict("outbox_event_id_payload_drift")
    ));
    let pending = std::fs::read_dir(outbox_root(root)).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".json")).count();
    assert_eq!(pending, 1);
    assert!(matches!(
        enqueue_outbox(root, EnqueueParams::new("mutation-expired", json!({"op": "b"}), now - 1, now)),
        EnqueueOutcome::Invalid("outbox_deadline_expired")
    ));
}

#[test]
fn outbox_crash_before_effect_retains_wal_record_until_later_replay() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    enqueue_outbox(root, EnqueueParams::new("crash-pre-effect", json!({"op": "a"}), 10_000, 0));
    let outcome = deliver_outbox(root, 0, 256, |_p, _id| EffectOutcome::Failed("simulated crash before effect".into()), None::<fn(&OutboxRecord)>);
    assert_eq!(outcome.acked, 0);
    assert_eq!(replay_outbox(root, 100, 10).len(), 1);
    let mut effects = 0;
    let recovered = deliver_outbox(root, 100, 256, |_p, _id| { effects += 1; EffectOutcome::Persisted }, None::<fn(&OutboxRecord)>);
    assert_eq!(recovered.acked, 1);
    assert_eq!(effects, 1);
    assert_eq!(replay_outbox(root, 100, 10).len(), 0);
}

#[test]
fn outbox_replay_cursor_is_stable_ordered_and_bounded_ack_is_idempotent() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let now = 1_000i64;
    enqueue_outbox(root, EnqueueParams::new("mutation-b", json!({}), 10_000, now));
    enqueue_outbox(root, EnqueueParams::new("mutation-a", json!({}), 10_000, now));
    let due = replay_outbox(root, now + 1, 10);
    assert_eq!(due.iter().map(|r| r.event_id.clone()).collect::<Vec<_>>(), vec!["mutation-a", "mutation-b"]);
    assert_eq!(due[0].payload, json!({}));
    assert_eq!(replay_outbox(root, now - 1, 10).len(), 0);
    assert_eq!(replay_outbox(root, now + 1, 1).len(), 1);
    assert!(matches!(ack_outbox(root, "mutation-a"), AckOutcome::Acked));
    assert!(matches!(ack_outbox(root, "mutation-a"), AckOutcome::Absent));
    assert_eq!(replay_outbox(root, now + 1, 10).iter().map(|r| r.event_id.clone()).collect::<Vec<_>>(), vec!["mutation-b"]);
}

#[test]
fn expired_replay_is_terminal_and_visible_in_the_dlq() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    enqueue_outbox(root, EnqueueParams::new("mutation-expiring", json!({"op": "d"}), 10, 0));
    assert_eq!(replay_outbox(root, 10, 10).len(), 0);
    assert_eq!(list_dead_letters(root)[0].event_id, "mutation-expiring");
}

#[test]
fn outbox_failure_backoff_is_bounded_and_exhausted_items_reach_the_dlq() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    enqueue_outbox(root, EnqueueParams::new("mutation-poison", json!({"op": "c"}), 100_000, 0));
    let f1 = mark_outbox_failed(root, "mutation-poison", MarkFailedParams { error: "boom", now_ms: 0, ..Default::default() });
    assert!(matches!(f1, MarkFailedOutcome::RetryScheduled { next_attempt_at_ms: 100, .. }));
    let f2 = mark_outbox_failed(root, "mutation-poison", MarkFailedParams { error: "boom", now_ms: 0, ..Default::default() });
    assert!(matches!(f2, MarkFailedOutcome::RetryScheduled { next_attempt_at_ms: 200, attempts: 2 }));
    let f3 = mark_outbox_failed(root, "mutation-poison", MarkFailedParams { error: "boom", now_ms: 0, max_attempts: 2, ..Default::default() });
    assert!(matches!(f3, MarkFailedOutcome::DeadLettered { .. }));
    assert_eq!(replay_outbox(root, 1, 10).len(), 0);
    let dlq = list_dead_letters(root);
    assert_eq!(dlq.len(), 1);
    assert_eq!(dlq[0].event_id, "mutation-poison");
    assert_eq!(dlq[0].last_error.as_deref(), Some("boom"));
}
