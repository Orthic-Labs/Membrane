//! Parity tests for `membrane_blueprint::delta_store` against
//! `blueprint/src/graph/delta-store.mjs`: the pure helpers
//! (`normalizeDigest`, the `domains_pending` watch-state helpers, and
//! `replaceBySource`), plus `apply_file_delta` — the native port of
//! `applyFileDelta`'s structural (non-document) path. See that module's doc
//! comment for the exact behavioral contract and the deliberate deviations
//! (document deltas rejected, `orderWrites` always `0`, no
//! query-projection-cache maintenance, no telemetry, no nested-transaction
//! composition).

use membrane_blueprint::delta_store::{
    apply_file_delta, normalize_digest, replace_by_source, ApplyFileDeltaError, ApplyOptions,
    with_treeish_worktree, EventKind, FactBatch, FileDelta, PendingDomains, SourceKeyed,
    TreeishError,
};
use membrane_blueprint::store::{open_store, save_generation, Generation};
use rusqlite::Connection;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

/// Seeds a store with one sealed generation containing a single file node
/// `a.ts`, matching the shape `applyFileDelta` expects to find already
/// present (a manifest + `generation_leaf` root) before it can reseal.
fn seeded_store() -> Connection {
    let mut db = open_store(None).unwrap();
    let generation = Generation {
        schema_version: Some(20),
        provider: Some(json!({"id": "lexical", "version": "1"})),
        manifest: Some(json!({
            "generationId": "g-0",
            "manifestDigest": "sha256:seed",
            "complete": true,
            "counts": {"nodes": 1, "edges": 0}
        })),
        repo_root: Some(json!("/repo")),
        augmentation: Some(json!({"enabled": false})),
        source_observation: Some(json!({"head": "abc", "dirty": false})),
        nodes: vec![json!({
            "id": "file:a.ts", "kind": "file", "labels": ["File"], "name": "a.ts",
            "qualifiedName": "a.ts", "path": "a.ts", "confidence": null, "evidence": null,
            "generationId": "g-0"
        })],
        edges: Vec::new(),
        file_reports: Vec::new(),
        documents: None,
        claims: None,
        claim_code_edges: None,
        document_supersession: None,
        extra: Default::default(),
    };
    save_generation(&mut db, &generation).unwrap();
    // `save_generation` does not seed `file_state`/`fact_owner`/
    // `generation_leaf` (those are delta-apply concerns, not whole-generation
    // publish concerns) — apply_file_delta must tolerate an absent prior
    // file_state row (a fresh file) and an absent leaf chain (empty root).
    db
}

fn structural_batch(node_id: &str, path: &str) -> FactBatch {
    FactBatch {
        provider_id: "lexical".to_string(),
        provider_version: "1".to_string(),
        nodes: vec![json!({
            "id": node_id, "kind": "file", "labels": ["File"], "name": path,
            "qualifiedName": path, "path": path, "confidence": null, "evidence": null
        })],
        edges: Vec::new(),
        dependencies: Vec::new(),
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .expect("git invocation failed");
    assert!(status.success(), "git {:?} failed", args);
}

fn init_git_repo() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    git(dir.path(), &["init", "--quiet"]);
    git(dir.path(), &["config", "user.email", "test@example.invalid"]);
    git(dir.path(), &["config", "user.name", "Parity Test"]);
    dir
}

#[test]
fn apply_file_delta_inserts_a_new_file_and_reseals_the_manifest() {
    let mut db = seeded_store();
    let delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("deadbeef".to_string()),
        fact_batches: vec![structural_batch("file:b.ts", "b.ts")],
        source_clock: Some(1),
        ..Default::default()
    };

    let result = apply_file_delta(&mut db, &delta, ApplyOptions::default()).unwrap();
    assert!(result.applied);
    assert!(!result.noop);
    assert_eq!(result.path, "b.ts");
    assert_eq!(result.applied_clock, 1);
    assert!(result.root_digest.is_some(), "leaf chain must produce a root digest once a file leaf exists");

    let stored_path: String = db.query_row("SELECT path FROM files WHERE path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(stored_path, "b.ts");
    let content_hash: String = db.query_row("SELECT content_hash FROM files WHERE path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(content_hash, "deadbeef", "content_hash is stored without the xxh128: scheme prefix, matching legacy fileNode()");

    let fact_owner_count: i64 = db.query_row("SELECT COUNT(*) FROM fact_owner WHERE source_path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(fact_owner_count, 1);

    let file_state_digest: String = db.query_row("SELECT content_digest FROM file_state WHERE path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(file_state_digest, "xxh128:deadbeef", "file_state stores the normalizeDigest-prefixed value, matching updateFileState");

    let manifest: String = db.query_row("SELECT value FROM generation WHERE key='manifest'", [], |row| row.get(0)).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_ne!(manifest["generationId"], json!("g-0"), "resealGenerationIdentityDelta must mint a new generationId");
    assert_eq!(manifest["counts"]["nodes"], json!(2), "refreshManifestCounts must count the pre-existing a.ts plus the newly inserted b.ts");
}

#[test]
fn apply_file_delta_is_a_noop_on_unchanged_digest() {
    let mut db = seeded_store();
    let delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("deadbeef".to_string()),
        fact_batches: vec![structural_batch("file:b.ts", "b.ts")],
        source_clock: Some(1),
        ..Default::default()
    };
    apply_file_delta(&mut db, &delta, ApplyOptions::default()).unwrap();
    let manifest_after_first: String = db.query_row("SELECT value FROM generation WHERE key='manifest'", [], |row| row.get(0)).unwrap();

    // Re-apply the identical digest with a later source clock: the legacy
    // noop short-circuit must acknowledge the clock without touching graph
    // rows or reseal ing the manifest.
    let repeat = FileDelta { source_clock: Some(2), ..delta };
    let result = apply_file_delta(&mut db, &repeat, ApplyOptions::default()).unwrap();
    assert!(result.noop);
    assert!(result.applied);
    assert_eq!(result.applied_clock, 2, "noop path still acknowledges the higher of previous/incoming clock");
    assert_eq!(result.order_writes, 0);

    let manifest_after_second: String = db.query_row("SELECT value FROM generation WHERE key='manifest'", [], |row| row.get(0)).unwrap();
    assert_eq!(manifest_after_first, manifest_after_second, "noop path must not reseal the manifest");

    let applied_clock: i64 = db.query_row("SELECT applied_clock FROM file_state WHERE path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(applied_clock, 2);
}

#[test]
fn apply_file_delta_delete_retracts_facts_and_unresolves_incoming_edges() {
    let mut db = seeded_store();
    // Insert b.ts with a symbol, plus an edge from a.ts's file node into
    // that symbol (mirrors the "an edge target gets deleted" retraction
    // case: `UPDATE edges SET resolved = 0 WHERE target = ?`).
    let mut batch = structural_batch("file:b.ts", "b.ts");
    batch.nodes.push(json!({
        "id": "symbol:b.ts::run", "kind": "function", "labels": ["Function"], "name": "run",
        "qualifiedName": "b.ts::run", "path": "b.ts", "confidence": 0.9, "evidence": []
    }));
    batch.edges.push(json!({
        "id": "edge:calls", "kind": "CALLS", "source": "file:a.ts", "target": "symbol:b.ts::run",
        "confidence": null, "evidence": [], "resolved": true
    }));
    let insert_delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("deadbeef".to_string()),
        fact_batches: vec![batch],
        source_clock: Some(1),
        ..Default::default()
    };
    apply_file_delta(&mut db, &insert_delta, ApplyOptions::default()).unwrap();
    let resolved_before: i64 = db.query_row("SELECT resolved FROM edges WHERE id='edge:calls'", [], |row| row.get(0)).unwrap();
    assert_eq!(resolved_before, 1);

    let delete_delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Delete,
        source_clock: Some(2),
        ..Default::default()
    };
    let result = apply_file_delta(&mut db, &delete_delta, ApplyOptions::default()).unwrap();
    assert!(result.applied);
    assert!(!result.noop);

    let file_count: i64 = db.query_row("SELECT COUNT(*) FROM files WHERE path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(file_count, 0, "delete removes the files row");
    let file_state_count: i64 = db.query_row("SELECT COUNT(*) FROM file_state WHERE path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(file_state_count, 0, "delete removes the file_state row");
    let symbol_count: i64 = db.query_row("SELECT COUNT(*) FROM symbols WHERE path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(symbol_count, 0, "delete retracts owned symbol facts via deleteFactsByOwner");
    let resolved_after: i64 = db.query_row("SELECT resolved FROM edges WHERE id='edge:calls'", [], |row| row.get(0)).unwrap();
    assert_eq!(resolved_after, 0, "the incoming edge into the deleted symbol node must be unresolved, matching the legacy retraction loop");
}

#[test]
fn apply_file_delta_refreshes_dependency_index_from_batch_dependencies() {
    let mut db = seeded_store();
    let mut batch = structural_batch("file:b.ts", "b.ts");
    batch.dependencies.push(("b.ts".to_string(), "a.ts".to_string(), "import".to_string()));
    let delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("deadbeef".to_string()),
        fact_batches: vec![batch],
        source_clock: Some(1),
        ..Default::default()
    };
    apply_file_delta(&mut db, &delta, ApplyOptions::default()).unwrap();

    let reason: String = db
        .query_row(
            "SELECT reason FROM dependency_index WHERE source_path='b.ts' AND dependent_path='a.ts'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reason, "import");
}

#[test]
fn apply_file_delta_rejects_document_deltas() {
    let mut db = seeded_store();
    let delta = FileDelta {
        path: "README.md".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("deadbeef".to_string()),
        is_document_delta: true,
        source_clock: Some(1),
        ..Default::default()
    };
    let error = apply_file_delta(&mut db, &delta, ApplyOptions::default()).unwrap_err();
    assert!(matches!(error, ApplyFileDeltaError::UnsupportedDocumentDelta(path) if path == "README.md"));
}

#[test]
fn normalize_digest_matches_legacy_prefixing() {
    // Legacy: normalizeDigest(value) => text.startsWith("xxh128:") ? text : `xxh128:${text}`
    assert_eq!(normalize_digest("deadbeef"), "xxh128:deadbeef");
    assert_eq!(normalize_digest("xxh128:deadbeef"), "xxh128:deadbeef");
    assert_eq!(normalize_digest(""), "xxh128:");
}

#[test]
fn pending_domains_matches_legacy_watch_state_semantics() {
    // Legacy readPendingDomains: split(",").trim().filter(Boolean).sort() —
    // note this does NOT dedup, matching the ported behaviour exactly.
    let read = PendingDomains::from_stored("structural, doc,,doc");
    assert_eq!(read.domains(), &["doc".to_string(), "doc".to_string(), "structural".to_string()]);

    // Legacy markDomainPending: append if absent, store sorted-join.
    let mut marked = PendingDomains::from_stored("");
    marked.mark("structural");
    assert_eq!(marked.to_stored().as_deref(), Some("structural"));
    marked.mark("doc");
    assert_eq!(marked.to_stored().as_deref(), Some("doc,structural"));

    // Legacy clearDomainPending: filter out; delete row (None) when empty.
    marked.clear("structural");
    assert_eq!(marked.to_stored().as_deref(), Some("doc"));
    marked.clear("doc");
    assert_eq!(marked.to_stored(), None, "legacy deletes the watch_state row once no domain remains pending");
}

#[test]
fn replace_by_source_matches_legacy_splice_semantics() {
    // Mirrors replaceDocumentArtifacts's use of replaceBySource to swap a
    // renamed/edited document's stale-claims entries in place.
    let entries = vec![
        SourceKeyed { source: "docs/a.md".to_string(), payload: "old-a-1" },
        SourceKeyed { source: "docs/b.md".to_string(), payload: "b" },
        SourceKeyed { source: "docs/a.md".to_string(), payload: "old-a-2" },
    ];
    let replacements = vec![SourceKeyed { source: "docs/a.md".to_string(), payload: "new-a" }];

    let result = replace_by_source(&entries, "docs/a.md", &replacements);
    assert_eq!(
        result,
        vec![
            SourceKeyed { source: "docs/a.md".to_string(), payload: "new-a" },
            SourceKeyed { source: "docs/b.md".to_string(), payload: "b" },
        ],
        "both old entries for the source collapse to the replacement set, inserted at the first match's position"
    );
}

#[test]
fn replace_by_source_is_a_pure_append_for_a_brand_new_source() {
    let entries = vec![SourceKeyed { source: "docs/b.md".to_string(), payload: "b" }];
    let replacements = vec![SourceKeyed { source: "docs/new.md".to_string(), payload: "new" }];
    let result = replace_by_source(&entries, "docs/new.md", &replacements);
    assert_eq!(
        result,
        vec![
            SourceKeyed { source: "docs/b.md".to_string(), payload: "b" },
            SourceKeyed { source: "docs/new.md".to_string(), payload: "new" },
        ]
    );
}

#[test]
fn replace_by_source_with_empty_replacements_deletes_the_source() {
    let entries = vec![
        SourceKeyed { source: "docs/a.md".to_string(), payload: "a" },
        SourceKeyed { source: "docs/b.md".to_string(), payload: "b" },
    ];
    let result: Vec<SourceKeyed<&str>> = replace_by_source(&entries, "docs/a.md", &[]);
    assert_eq!(result, vec![SourceKeyed { source: "docs/b.md".to_string(), payload: "b" }]);
}

#[test]
fn apply_file_delta_rename_moves_facts_and_retracts_old_path() {
    let mut db = seeded_store();
    let insert_delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("deadbeef".to_string()),
        fact_batches: vec![structural_batch("file:b.ts", "b.ts")],
        source_clock: Some(1),
        ..Default::default()
    };
    apply_file_delta(&mut db, &insert_delta, ApplyOptions::default()).unwrap();

    // Legacy `rename` retracts the old path's facts exactly like `delete`
    // (unresolve incoming edges, deleteFactsByOwner, drop files/file_state
    // rows, remove the leaf), then reseals the manifest without touching
    // node/edge rows for the new path (this port carries no factBatches on
    // rename, matching the "no fact batches" reseal-only branch).
    let rename_delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Rename,
        rename_to: Some("c.ts".to_string()),
        source_clock: Some(2),
        ..Default::default()
    };
    let result = apply_file_delta(&mut db, &rename_delta, ApplyOptions::default()).unwrap();
    assert!(result.applied);
    assert!(!result.noop);
    assert_eq!(result.applied_clock, 2);

    let old_file_count: i64 = db.query_row("SELECT COUNT(*) FROM files WHERE path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(old_file_count, 0, "rename retracts the old path's files row exactly like delete");
    let old_state_count: i64 = db.query_row("SELECT COUNT(*) FROM file_state WHERE path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(old_state_count, 0, "rename retracts the old path's file_state row exactly like delete");
}

#[test]
fn apply_file_delta_repair_is_never_noop_short_circuited() {
    let mut db = seeded_store();
    let delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("deadbeef".to_string()),
        fact_batches: vec![structural_batch("file:b.ts", "b.ts")],
        source_clock: Some(1),
        ..Default::default()
    };
    apply_file_delta(&mut db, &delta, ApplyOptions::default()).unwrap();

    // Legacy: `!["delete", "rename", "repair"].includes(eventKind)` gates
    // the noop short-circuit — a `repair` event with an unchanged digest
    // must still re-run the full apply path (re-insert facts, reseal),
    // never short-circuit like `create`/`modify` do.
    let repair_delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Repair,
        content_digest: Some("deadbeef".to_string()),
        fact_batches: vec![structural_batch("file:b.ts", "b.ts")],
        source_clock: Some(2),
        ..Default::default()
    };
    let result = apply_file_delta(&mut db, &repair_delta, ApplyOptions::default()).unwrap();
    assert!(result.applied);
    assert!(!result.noop, "repair must never take the noop short-circuit even on an unchanged digest");
    assert_eq!(result.applied_clock, 2);

    let fact_owner_count: i64 = db.query_row("SELECT COUNT(*) FROM fact_owner WHERE source_path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(fact_owner_count, 1, "repair re-inserts the fact rather than leaving them untouched");
}

#[test]
fn apply_file_delta_multiple_providers_retract_independently() {
    let mut db = seeded_store();
    let mut lexical_batch = structural_batch("file:b.ts", "b.ts");
    lexical_batch.nodes.push(json!({
        "id": "symbol:b.ts::lexical-fn", "kind": "function", "labels": ["Function"], "name": "lexicalFn",
        "qualifiedName": "b.ts::lexicalFn", "path": "b.ts", "confidence": 0.9, "evidence": []
    }));
    let mut other_batch = FactBatch {
        provider_id: "other-provider".to_string(),
        provider_version: "2".to_string(),
        nodes: vec![json!({
            "id": "symbol:b.ts::other-fn", "kind": "function", "labels": ["Function"], "name": "otherFn",
            "qualifiedName": "b.ts::otherFn", "path": "b.ts", "confidence": 0.8, "evidence": []
        })],
        edges: Vec::new(),
        dependencies: Vec::new(),
    };
    let insert_delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("deadbeef".to_string()),
        fact_batches: vec![lexical_batch, other_batch.clone()],
        source_clock: Some(1),
        ..Default::default()
    };
    apply_file_delta(&mut db, &insert_delta, ApplyOptions::default()).unwrap();

    let owner_count_before: i64 = db.query_row("SELECT COUNT(*) FROM fact_owner WHERE source_path='b.ts'", [], |row| row.get(0)).unwrap();
    assert_eq!(owner_count_before, 3, "two lexical nodes (file + symbol) plus one other-provider symbol");

    // A second apply carrying only the `other-provider` batch must retract
    // and re-insert only that provider's facts, leaving `lexical`'s facts
    // (including the file node itself) untouched — legacy's
    // `structuralProviders` scoping of the retraction-then-reinsert loop.
    other_batch.nodes[0] = json!({
        "id": "symbol:b.ts::other-fn", "kind": "function", "labels": ["Function"], "name": "otherFnRenamed",
        "qualifiedName": "b.ts::otherFnRenamed", "path": "b.ts", "confidence": 0.8, "evidence": []
    });
    let update_delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Modify,
        content_digest: Some("cafebabe".to_string()),
        fact_batches: vec![other_batch],
        source_clock: Some(2),
        ..Default::default()
    };
    apply_file_delta(&mut db, &update_delta, ApplyOptions::default()).unwrap();

    let lexical_owner_count: i64 = db
        .query_row("SELECT COUNT(*) FROM fact_owner WHERE source_path='b.ts' AND provider_id='lexical'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(lexical_owner_count, 2, "lexical's file+symbol facts survive an update scoped to another provider");
    let other_name: String = db
        .query_row("SELECT name FROM symbols WHERE id='symbol:b.ts::other-fn'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(other_name, "otherFnRenamed", "the other-provider symbol was retracted and reinserted with the new payload");
}

#[test]
fn apply_file_delta_acknowledges_the_event_journal_row() {
    let mut db = seeded_store();
    db.execute_batch(
        "INSERT INTO event_journal(seq, observed_ms, event_kind, path, source_clock, applied)
         VALUES (1, 1000, 'create', 'b.ts', 1, 0)",
    )
    .unwrap();

    let delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("deadbeef".to_string()),
        fact_batches: vec![structural_batch("file:b.ts", "b.ts")],
        source_clock: Some(1),
        journal_seq: Some(1),
        ..Default::default()
    };
    apply_file_delta(&mut db, &delta, ApplyOptions::default()).unwrap();

    let (applied, applied_clock): (i64, Option<i64>) = db
        .query_row("SELECT applied, applied_clock FROM event_journal WHERE seq=1", [], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap();
    assert_eq!(applied, 1, "a non-deferred journal_seq must be acknowledged in the same transaction");
    assert_eq!(applied_clock, Some(1));
}

#[test]
fn apply_file_delta_defers_journal_ack_when_requested() {
    let mut db = seeded_store();
    db.execute_batch(
        "INSERT INTO event_journal(seq, observed_ms, event_kind, path, source_clock, applied)
         VALUES (1, 1000, 'create', 'b.ts', 1, 0)",
    )
    .unwrap();

    let delta = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("deadbeef".to_string()),
        fact_batches: vec![structural_batch("file:b.ts", "b.ts")],
        source_clock: Some(1),
        journal_seq: Some(1),
        ..Default::default()
    };
    let options = ApplyOptions { defer_journal_ack: true };
    apply_file_delta(&mut db, &delta, options).unwrap();

    let applied: i64 = db.query_row("SELECT applied FROM event_journal WHERE seq=1", [], |row| row.get(0)).unwrap();
    assert_eq!(applied, 0, "deferJournalAck must leave the event_journal row unacknowledged");
}

#[test]
fn apply_file_delta_changes_the_manifest_root_digest_and_generation_id_per_apply() {
    let mut db = seeded_store();
    let insert_b = FileDelta {
        path: "b.ts".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("deadbeef".to_string()),
        fact_batches: vec![structural_batch("file:b.ts", "b.ts")],
        source_clock: Some(1),
        ..Default::default()
    };
    let first = apply_file_delta(&mut db, &insert_b, ApplyOptions::default()).unwrap();
    let manifest_after_b: String = db.query_row("SELECT value FROM generation WHERE key='manifest'", [], |row| row.get(0)).unwrap();
    let manifest_after_b: serde_json::Value = serde_json::from_str(&manifest_after_b).unwrap();

    let insert_c = FileDelta {
        path: "c.ts".to_string(),
        event_kind: EventKind::Create,
        content_digest: Some("f00dcafe".to_string()),
        fact_batches: vec![structural_batch("file:c.ts", "c.ts")],
        source_clock: Some(2),
        ..Default::default()
    };
    let second = apply_file_delta(&mut db, &insert_c, ApplyOptions::default()).unwrap();
    let manifest_after_c: String = db.query_row("SELECT value FROM generation WHERE key='manifest'", [], |row| row.get(0)).unwrap();
    let manifest_after_c: serde_json::Value = serde_json::from_str(&manifest_after_c).unwrap();

    assert_ne!(first.root_digest, second.root_digest, "adding a distinct file leaf must change the merkle root digest");
    assert_ne!(
        manifest_after_b["generationId"], manifest_after_c["generationId"],
        "resealGenerationIdentityDelta folds the prior generationId, so each apply mints a fresh one"
    );
    assert_ne!(
        manifest_after_b["manifestDigest"], manifest_after_c["manifestDigest"],
        "a changed generationId/counts must change the recomputed manifestDigest"
    );
    assert_eq!(manifest_after_c["counts"]["nodes"], json!(3), "a.ts (seed) + b.ts + c.ts");
}

#[test]
fn treeish_worktree_materializes_commit_and_unregisters_on_success() {
    let repo = init_git_repo();
    fs::write(repo.path().join("a.rs"), "fn a() {}\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "init"]);
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let head = String::from_utf8_lossy(&output.stdout).trim().to_owned();

    let path = with_treeish_worktree(repo.path(), &head, |worktree| {
        assert!(worktree.join("a.rs").is_file());
        Ok::<_, std::io::Error>(worktree.to_owned())
    })
    .unwrap();
    assert!(!path.exists(), "successful callback must not leak checkout directory");
    let listing = Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let worktree_count = String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter(|line| line.starts_with("worktree "))
        .count();
    assert_eq!(worktree_count, 1);
}

#[test]
fn treeish_worktree_cleanup_runs_when_callback_fails() {
    let repo = init_git_repo();
    fs::write(repo.path().join("a.rs"), "fn a() {}\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "--quiet", "-m", "init"]);
    let error = with_treeish_worktree(repo.path(), "HEAD", |_worktree| {
        Err::<(), _>(std::io::Error::other("synthetic build failure"))
    })
    .unwrap_err();
    assert!(matches!(error, TreeishError::Callback(message) if message.contains("synthetic build failure")));

    let listing = Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let worktree_count = String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter(|line| line.starts_with("worktree "))
        .count();
    assert_eq!(worktree_count, 1);
}

#[test]
fn treeish_worktree_rejects_bad_reference_with_stable_error_code() {
    let repo = init_git_repo();
    let error = with_treeish_worktree(repo.path(), "not-a-real-treeish", |_worktree| {
        Ok::<_, std::io::Error>(())
    })
    .unwrap_err();
    assert_eq!(error.code(), "treeish_invalid");
    assert!(matches!(error, TreeishError::Invalid { .. }));
}
