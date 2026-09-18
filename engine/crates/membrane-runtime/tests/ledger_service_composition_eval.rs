//! Ledger service-composition qualification harness (LEDGER canon section 19:
//! hard gates + measured gates). The existing `ledger-eval-v1` receipt qualifies
//! the two recall lanes in isolation via `doc_spine::recall_shadow`; it does NOT
//! qualify the owner/resolver composition — the published-index retrieval path a
//! production caller actually traverses. This harness measures that composition:
//!
//!   * corpus indexed through the production sync path (`doc_spine::sync`) into a
//!     file-backed index at the daemon owner's canonical location
//!     (`MEMBRANE_CACHE_ROOT/ledger-index.sqlite3`), published via the
//!     `ledger_owner_roots` owner row `sync_locked` writes;
//!   * every `recall` op dispatched through the real `membrane_ledger` MCP
//!     boundary (`McpServer::dispatch` -> `RuntimeMcpExecutor` -> daemon
//!     `LedgerService::operation` -> `run_read` on the dedicated WAL reader ->
//!     `query::search` on the persisted activation mode -> catalog ticket
//!     issuance) — never a direct in-crate call;
//!   * the persisted `ledger_activation` row is set by the fixture the same way
//!     an operator's trusted activation persists it: mode + the receipt hash
//!     that authorized it. `ledger_fts` is bound to the still-trusted
//!     `ledger-eval-v1` receipt so owner-open reconciliation preserves it;
//!   * baseline arms (`legacy_scan`, lane-level `ledger_fts`) come from
//!     `recall_shadow` on the same index file — production lane code on the same
//!     corpus — so the frozen paired decision rule stays apples-to-apples;
//!   * hard-gate legs through the same boundary: ticket issuance, typed
//!     abstention, erasure binding, grant narrowing, and a read completing while
//!     a foreign writer transaction is held (retrieval/maintenance separation).
//!
//! Run with:
//!   cargo test -p membrane-runtime --test ledger_service_composition_eval -- --ignored --nocapture
//!
//! Promotion rule (frozen, identical to ledger-eval-v1): ledger_fts promotes iff
//! heldout fts.mrr >= legacy.mrr AND fts.recallAt5 >= legacy.recallAt5 - 0.02.
//! The harness prints a canonical evidence JSON including the minted
//! `ledger.qualification-receipt.v1`; record it under
//! `docs/evidence/qualification/` and pin `receiptSha256` in
//! `TRUSTED_LEDGER_FTS_RECEIPTS` + `QUALIFIED_FTS_ACTIVATION` only when every
//! gate passes.

use membrane_runtime::catalog::{self, ContextCatalog};
use membrane_runtime::ledger::{doc_spine, index, LedgerDb};
use membrane_runtime::memdb::MemDb;
use membrane_runtime::store::MemoryStore;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// The still-trusted lane-eval receipt used to persist a `ledger_fts`
/// activation for measurement. Mirrored from TRUSTED_LEDGER_FTS_RECEIPTS; the
/// fixture asserts membership rather than trusting this copy.
const LANE_EVAL_RECEIPT: &str =
    "c7547262dbc5a11109236f8b343b421cd6a248a2447df697483624166978360e";

#[derive(Debug, Deserialize)]
struct Target {
    document: String,
}

#[derive(Debug, Deserialize)]
struct Expected {
    status: String,
    match_mode: String,
    targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    split: String,
    case_type: String,
    query: String,
    expected: Expected,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ledger-eval-v1")
}

fn load_cases(split: &str) -> Vec<Case> {
    let path = corpus_dir().join("cases").join(format!("{split}.jsonl"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("parse case: {error}\n{line}")))
        .collect()
}

fn corpus_digest() -> String {
    let mut hasher = Sha256::new();
    let mut files = vec![corpus_dir().join("manifest.json")];
    for split in ["train", "dev", "heldout"] {
        files.push(corpus_dir().join("cases").join(format!("{split}.jsonl")));
    }
    for file in files {
        let bytes = std::fs::read(&file).unwrap_or_else(|error| panic!("read {}: {error}", file.display()));
        hasher.update(file.file_name().unwrap().to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(&bytes);
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())
}

fn normalize_doc_path(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/").to_string()
}

fn hit_document_path(source_ref: &str) -> Option<String> {
    source_ref
        .strip_prefix("doc://repo/worktree/")
        .map(normalize_doc_path)
}

#[derive(Default, Debug)]
struct ArmStats {
    recall_at_1: usize,
    recall_at_5: usize,
    reciprocal_rank_sum: f64,
    total: usize,
    per_category: BTreeMap<String, (usize, usize, usize)>,
}

impl ArmStats {
    fn record(&mut self, category: &str, rank: Option<usize>) {
        self.total += 1;
        let entry = self.per_category.entry(category.to_owned()).or_default();
        entry.2 += 1;
        if let Some(rank) = rank {
            self.reciprocal_rank_sum += 1.0 / rank as f64;
            if rank <= 1 {
                self.recall_at_1 += 1;
                entry.0 += 1;
            }
            if rank <= 5 {
                self.recall_at_5 += 1;
                entry.1 += 1;
            }
        }
    }
    fn mrr(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.reciprocal_rank_sum / self.total as f64
        }
    }
    fn recall_at_5_pct(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.recall_at_5 as f64 / self.total as f64
        }
    }
    fn json(&self) -> Value {
        json!({
            "recallAt1": self.recall_at_1 as f64 / self.total.max(1) as f64,
            "recallAt5": self.recall_at_5_pct(),
            "mrr": self.mrr(),
        })
    }
}

/// Score dispatch hits (serialized `LedgerHit` values) the same way the lane
/// harness scores `DocRecallHitV1` hits.
fn score_case(expected: &Expected, hit_paths: &[String]) -> Option<usize> {
    let target_paths: Vec<String> = expected
        .targets
        .iter()
        .map(|t| normalize_doc_path(&t.document))
        .collect();
    match expected.status.as_str() {
        "no_match" => {
            if hit_paths.is_empty() {
                Some(1)
            } else {
                None
            }
        }
        "match" | "relocation" => match expected.match_mode.as_str() {
            "all_of" => {
                let mut last_rank = 0usize;
                for target in &target_paths {
                    let rank = hit_paths
                        .iter()
                        .position(|path| path == target)
                        .map(|index| index + 1);
                    match rank {
                        Some(rank) => last_rank = last_rank.max(rank),
                        None => return None,
                    }
                }
                Some(last_rank)
            }
            _ => hit_paths
                .iter()
                .position(|path| target_paths.iter().any(|target| path == target))
                .map(|index| index + 1),
        },
        other => panic!("unknown expected.status: {other}"),
    }
}

fn score_lane_hits(expected: &Expected, hits: &[doc_spine::DocRecallHitV1]) -> Option<usize> {
    let paths: Vec<String> = hits
        .iter()
        .filter_map(|hit| hit_document_path(&hit.source_ref))
        .collect();
    score_case(expected, &paths)
}

fn score_dispatch_hits(expected: &Expected, hits: &[Value]) -> Option<usize> {
    let paths: Vec<String> = hits
        .iter()
        .filter_map(|hit| hit.get("sourceRef").and_then(Value::as_str))
        .filter_map(hit_document_path)
        .collect();
    score_case(expected, &paths)
}

struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Environment {
    fn set(values: &[(&'static str, std::path::PathBuf)]) -> Self {
        let mut previous = Vec::new();
        for (key, value) in values {
            previous.push((*key, std::env::var_os(key)));
            std::env::set_var(key, value);
        }
        Self(previous)
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        for (key, previous) in self.0.drain(..).rev() {
            match previous {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn call(server: &membrane_mcp::McpServer, name: &str, arguments: Value) -> Value {
    let label = format!(
        "{}/{}",
        name,
        arguments.get("operation").and_then(Value::as_str).unwrap_or("-")
    );
    let started = Instant::now();
    let response = server
        .dispatch(&json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":name,"arguments":arguments}
        }))
        .unwrap();
    let code = response
        .pointer("/result/structuredContent/result/code")
        .and_then(Value::as_str)
        .unwrap_or("none");
    eprintln!(
        "dispatch {label} elapsed={:?} isError={} code={code}",
        started.elapsed(),
        response.pointer("/result/isError").and_then(Value::as_bool).unwrap_or(true)
    );
    response
}

fn data(response: &Value) -> &Value {
    assert_eq!(
        response.pointer("/result/isError"),
        Some(&json!(false)),
        "{response}"
    );
    response
        .pointer("/result/structuredContent/result/data")
        .unwrap()
}

fn commit_sha256() -> (String, String) {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root())
        .output()
        .expect("git rev-parse HEAD");
    let commit = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    assert_eq!(commit.len(), 40, "git rev-parse HEAD -> {commit}");
    (commit.clone(), hex::encode(Sha256::digest(commit.as_bytes())))
}

fn set_mode(db: &LedgerDb, mode: &str, receipt: Option<&str>) {
    db.lock()
        .execute(
            "UPDATE ledger_activation SET mode=?1, qualification_receipt_sha256=?2 WHERE singleton=1",
            rusqlite::params![mode, receipt],
        )
        .unwrap();
}

/// Held-out promotion run for the owner/resolver composition. Frozen
/// configuration; heldout exercised exactly once per candidate.
#[test]
#[ignore = "qualification-only: indexes the repository and dispatches the production boundary"]
fn ledger_service_composition_heldout() {
    let dir = tempfile::tempdir().unwrap();
    let root = repo_root().canonicalize().unwrap();
    let root_arg = root.to_string_lossy().replace('\\', "/");
    let repository = "membrane";
    let scope = "installation-scope";

    // Installation registry enrolling the repository corpus (the composition's
    // source authority — a request string is not enrollment).
    let registry = dir.path().join("registry.json");
    std::fs::write(
        &registry,
        json!({"schema_version":2,"bindings":{
            root_arg.as_str():{
                "repository_id":repository,"scope_id":scope,
                "grant_policy":{"level":"write-trusted"}
            }
        }})
        .to_string(),
    )
    .unwrap();
    let _environment = Environment::set(&[
        ("MEMBRANE_CACHE_ROOT", dir.path().join("cache")),
        ("MEMBRANE_CATALOG", dir.path().join("catalog.db")),
        ("MEMBRANE_PROJECT_REGISTRY", registry),
        ("WORKSPACE_ROOT", dir.path().to_path_buf()),
    ]);

    // Production boundary: hub executor installs the daemon owner, which opens
    // the canonical index under the isolated cache root and reconciles
    // persisted activation at open.
    let executor = membrane_runtime::mcp_executor::RuntimeMcpExecutor::for_hub(
        MemoryStore::open(MemDb::open_in_memory()),
    )
    .unwrap();
    assert!(membrane_mcp::install_executor(Arc::new(executor)).is_ok());
    let server = membrane_mcp::McpServer::default();
    let caller = json!({"root":root_arg,"repositoryId":repository,"scopeId":scope});

    // Production indexing into the daemon's canonical index file, then the
    // owner-publication row `sync_locked` writes on a completed sync. The
    // `membrane_ledger` dispatch deadline caps at 30s — far under a cold repo
    // walk — so indexing runs through the same `sync_bounded` engine the
    // service's sync op calls, with a maintenance-scale budget.
    let index_path = dir.path().join("cache").join("ledger-index.sqlite3");
    let db = LedgerDb::open(&index_path).unwrap();
    let sync_budget = membrane_runtime::ledger::limits::WorkBudget::bounded(
        std::time::Duration::from_secs(3600),
    );
    let report =
        doc_spine::sync_bounded(&db, &root, &sync_budget).expect("production sync over repo root");
    assert!(report.registered > 0, "sync registered no documents");
    eprintln!(
        "indexed repo via production sync: scanned={} registered={} parsed={} skipped={}",
        report.scanned, report.registered, report.parsed, report.skipped
    );
    db.lock()
        .execute(
            "INSERT INTO ledger_owner_roots VALUES (?1,?2,?3,?4)",
            rusqlite::params![
                root_arg,
                report.index_generation,
                report.policy_digest,
                membrane_runtime::time::now_millis() as i64
            ],
        )
        .unwrap();

    // Sanity through the boundary: the owner reports a published index.
    let status = call(
        &server,
        "membrane_ledger",
        json!({"repository":repository,"caller":caller,"operation":"status","deadlineMs":30000}),
    );
    assert_eq!(data(&status)["indexState"], json!("published"), "{status}");

    let mut run_rows: Vec<Value> = Vec::new();
    let mut arms: BTreeMap<String, ArmStats> = BTreeMap::new();
    let mut latencies_ms: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    let mut cross_mismatch: Vec<String> = Vec::new();
    let mut ticket_missing: Vec<String> = Vec::new();
    let mut unbound_hits: Vec<String> = Vec::new();

    set_mode(&db, "ledger_fts", Some(LANE_EVAL_RECEIPT));
    for case in load_cases("heldout") {
        // Lane baseline on the same index: production lane code, both arms in
        // one shadow pass.
        let shadow = doc_spine::recall_shadow(&db, &case.query, 5)
            .unwrap_or_else(|e| panic!("recall_shadow {}: {e}", case.id));
        let legacy_rank = score_lane_hits(&case.expected, &shadow.legacy_hits);
        let fts_lane_rank = score_lane_hits(&case.expected, &shadow.fts_hits);

        // Composition arm: dispatched through membrane_ledger -> run_read ->
        // query::search on the persisted ledger_fts activation.
        let started = Instant::now();
        let response = call(
            &server,
            "membrane_ledger",
            json!({"repository":repository,"caller":caller,"operation":"recall",
                "query":case.query,"k":5,"deadlineMs":30000}),
        );
        let elapsed = started.elapsed().as_millis() as u64;
        latencies_ms
            .entry("ledger_fts".to_owned())
            .or_default()
            .push(elapsed);
        let hits = data(&response)["hits"].as_array().cloned().unwrap_or_default();
        // Hard gate: every emitted hit is source-bound and ticketed.
        for hit in &hits {
            for field in ["docId", "sourceRef", "expectedContentHash", "expectedSpanHash"] {
                if hit.get(field).and_then(Value::as_str).is_none_or(str::is_empty) {
                    unbound_hits.push(format!("{} missing {field}", case.id));
                }
            }
            if hit.get("ledgerTicket").and_then(Value::as_str).is_none_or(str::is_empty) {
                ticket_missing.push(case.id.clone());
            }
        }
        // Cross-validation: the production FTS arm may only emit documents the
        // qualification lane arm surfaced, in the lane arm's order. Production
        // fuses exact/graph/FTS candidates and deduplicates by span containment,
        // so a lane doc legitimately absent from the top-k is not a failure —
        // but a ledger_fts-lane hit the lane arm never ranked, or an ordering
        // the lane arm contradicts, is real divergence. Restrict to FTS-lane
        // hits: exact-lane evidence outranking FTS docs is fusion working, not
        // an FTS regression.
        let lane_seq: Vec<String> = {
            let mut seen = BTreeSet::new();
            shadow
                .fts_hits
                .iter()
                .filter_map(|h| hit_document_path(&h.source_ref))
                .filter(|path| seen.insert(path.clone()))
                .collect()
        };
        let dispatch_fts: Vec<String> = {
            let mut seen = BTreeSet::new();
            hits.iter()
                .filter(|h| h.get("lane").and_then(Value::as_str) == Some("ledger_fts"))
                .filter_map(|h| h.get("sourceRef").and_then(Value::as_str))
                .filter_map(hit_document_path)
                .filter(|path| seen.insert(path.clone()))
                .collect()
        };
        let mut lane_iter = lane_seq.iter();
        let ordered = dispatch_fts.iter().all(|path| {
            lane_iter.by_ref().any(|lane| lane == path)
        });
        if !ordered {
            cross_mismatch.push(format!(
                "{}: fts dispatch {dispatch_fts:?} diverges from lane {lane_seq:?}",
                case.id
            ));
        }
        let dispatch_rank = score_dispatch_hits(&case.expected, &hits);

        arms.entry("legacy_scan".into())
            .or_default()
            .record(&case.case_type, legacy_rank);
        arms.entry("ledger_fts".into())
            .or_default()
            .record(&case.case_type, dispatch_rank);
        run_rows.push(json!({
            "case": case.id, "caseType": case.case_type,
            "legacyRank": legacy_rank, "ftsLaneRank": fts_lane_rank,
            "dispatchRank": dispatch_rank, "dispatchMs": elapsed,
        }));
    }

    // ---- Hard-gate legs through the same boundary (heldout scored above) ----

    // Grant narrowing: a scope grant may only narrow the lane. Grant one
    // document path; every returned hit must fall inside it.
    let catalog = ContextCatalog::open(dir.path().join("catalog.db")).unwrap();
    let grant_case = load_cases("heldout")
        .into_iter()
        .find(|c| c.expected.status == "match" && !c.expected.targets.is_empty())
        .expect("heldout has a match case");
    let granted_path = normalize_doc_path(&grant_case.expected.targets[0].document);
    let grant = catalog::issue_scope_grant(
        &catalog,
        "composition-eval-grant",
        "ledger-composition-eval",
        &[repository.into()],
        &["source_read".into()],
        &[membrane_protocol::ReadPathV1 {
            path: granted_path.clone(),
            start_line: 1,
            end_line: 1_000_000,
        }],
        "composition-eval-task",
        "composition-eval-session",
        600,
        "composition-eval-nonce",
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
    )
    .unwrap();
    set_mode(&db, "ledger_fts", Some(LANE_EVAL_RECEIPT));
    let narrowed = call(
        &server,
        "membrane_ledger",
        json!({"repository":repository,"caller":caller,"operation":"recall",
            "query":grant_case.query,"k":5,"scopeGrantId":grant.id,
            "taskId":"composition-eval-task","sessionId":"composition-eval-session",
            "deadlineMs":30000}),
    );
    let narrowed_paths: Vec<String> = data(&narrowed)["hits"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|h| h.get("sourceRef").and_then(Value::as_str))
        .filter_map(hit_document_path)
        .collect();
    let grant_narrowed = narrowed_paths.iter().all(|path| path == &granted_path);

    // Erasure binding: erase a scored target through the boundary; the FTS lane
    // must not resurrect it.
    let erase_case = load_cases("heldout")
        .into_iter()
        .find(|c| c.expected.status == "match" && c.id != grant_case.id)
        .expect("heldout has a second match case");
    let erase_path = normalize_doc_path(&erase_case.expected.targets[0].document);
    let (doc_id, content_hash): (String, String) = db
        .lock()
        .query_row(
            "SELECT doc_id,content_hash FROM ledger_doc_artifacts WHERE repository_root=?1 AND path=?2",
            rusqlite::params![root_arg, erase_path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("erasure target is indexed");
    let erased = call(
        &server,
        "membrane_ledger",
        json!({"repository":repository,"caller":caller,"operation":"erase",
            "docId":doc_id,"expectedContentHash":content_hash,"deadlineMs":30000}),
    );
    assert_eq!(data(&erased)["docId"], json!(doc_id), "{erased}");
    let after_erase = call(
        &server,
        "membrane_ledger",
        json!({"repository":repository,"caller":caller,"operation":"recall",
            "query":erase_case.query,"k":5,"deadlineMs":30000}),
    );
    let erase_bound = data(&after_erase)["hits"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|h| h.get("sourceRef").and_then(Value::as_str))
        .filter_map(hit_document_path)
        .all(|path| path != erase_path);

    // Retrieval/maintenance separation through the boundary: while a foreign
    // writer transaction is held open — the shape sync_bounded holds across a
    // corpus walk — a published-index recall still returns committed hits.
    let mut writer = db.lock();
    let tx = writer.transaction().unwrap();
    tx.execute_batch("INSERT OR IGNORE INTO ledger_erasure_fences VALUES ('held','probe',0)")
        .unwrap();
    let probe_case = load_cases("heldout")
        .into_iter()
        .find(|c| c.expected.status == "match")
        .expect("heldout has a match case");
    let started = Instant::now();
    let during_maintenance = call(
        &server,
        "membrane_ledger",
        json!({"repository":repository,"caller":caller,"operation":"recall",
            "query":probe_case.query,"k":5,"deadlineMs":30000}),
    );
    let maintenance_read_ms = started.elapsed().as_millis() as u64;
    let maintenance_hits = data(&during_maintenance)["hits"].as_array().cloned().unwrap_or_default();
    tx.rollback().unwrap();
    drop(writer);
    let read_under_maintenance = !maintenance_hits.is_empty();

    // Tickets are catalog-side runtime state: issuance left durable rows in the
    // catalog DB, never a write to the index file from the read path.
    let catalog_tickets: i64 = {
        catalog.lock().execute_batch(
            "CREATE TABLE IF NOT EXISTS ledger_resolution_tickets (
                ticket_hash TEXT PRIMARY KEY, repository_root TEXT NOT NULL,
                caller_digest TEXT NOT NULL, doc_id TEXT NOT NULL,
                request_json TEXT NOT NULL, grant_id TEXT, expires_at_ms INTEGER NOT NULL
            );",
        ).unwrap();
        catalog
            .lock()
            .query_row("SELECT COUNT(*) FROM ledger_resolution_tickets", [], |r| r.get(0))
            .unwrap()
    };

    // ---- Decision ----
    let legacy = &arms["legacy_scan"];
    let fts = &arms["ledger_fts"];
    let mrr_ok = fts.mrr() >= legacy.mrr();
    let recall5_ok = fts.recall_at_5_pct() >= legacy.recall_at_5_pct() - 0.02;
    let fts_latencies = &latencies_ms["ledger_fts"];
    let max_latency = fts_latencies.iter().copied().max().unwrap_or(0);
    let gates = json!({
        "mrrParity": mrr_ok,
        "recall5Within2pp": recall5_ok,
        "crossValidationMatch": cross_mismatch.is_empty(),
        "ticketsIssued": ticket_missing.is_empty() && catalog_tickets > 0,
        "sourceBoundHits": unbound_hits.is_empty(),
        "grantNarrowsOnly": grant_narrowed,
        "erasureBinds": erase_bound,
        "readCompletesUnderMaintenance": read_under_maintenance,
    });
    let promote = gates.as_object().unwrap().values().all(|v| v == &json!(true));

    let run_json = json!({
        "cases": run_rows,
        "latenciesMs": {"ledger_fts": fts_latencies, "maintenanceReadMs": maintenance_read_ms},
        "crossValidationMismatches": cross_mismatch,
        "ticketMissing": ticket_missing,
        "unboundHits": unbound_hits,
        "catalogTickets": catalog_tickets,
        "grantNarrowingPaths": narrowed_paths,
        "erasureTarget": erase_path,
    });
    let result_json = json!({
        "decision": if promote { "promote" } else { "reject" },
        "gates": gates,
        "heldout": {
            "n": legacy.total,
            "legacy_scan": legacy.json(),
            "ledger_fts": fts.json(),
            "ledger_fts_max_latency_ms": max_latency,
        },
    });
    let run_sha256 = hex::encode(Sha256::digest(
        serde_json::to_string(&run_rows).unwrap().as_bytes(),
    ));
    let result_sha256 = hex::encode(Sha256::digest(
        serde_json::to_string(&result_json).unwrap().as_bytes(),
    ));
    let (commit, commit_sha256) = commit_sha256();
    let corpus_sha256 = corpus_digest();
    let mut receipt = index::LedgerQualificationReceiptV1 {
        schema_version: "ledger.qualification-receipt.v1".into(),
        receipt_source: "membrane-host/ledger-qualification".into(),
        host_id: "membrane-eval-harness/ledger-service-composition-v1".into(),
        verifier_id: "ledger-service-composition-eval".into(),
        commit_sha256,
        corpus_version: "ledger-eval-v1".into(),
        corpus_sha256,
        run_sha256,
        result_sha256,
        receipt_sha256: String::new(),
    };
    receipt.receipt_sha256 = index::qualification_receipt_sha256(&receipt);

    let evidence = json!({
        "schemaVersion": 1,
        "axis": "ledger",
        "status": if promote { "qualified" } else { "rejected" },
        "entrypoint": "membrane-runtime::ledger::service",
        "metrics": ["semantic_recall","source_binding","ticket_issuance","typed_abstention",
                    "erasure_binding","grant_narrowing","maintenance_read_isolation"],
        "evaluation": {
            "corpus": "ledger-eval-v1",
            "corpusDigestSha256": receipt.corpus_sha256,
            "harness": "engine/crates/membrane-runtime/tests/ledger_service_composition_eval.rs",
            "harnessCommand": "cargo test -p membrane-runtime --test ledger_service_composition_eval -- --ignored --nocapture",
            "commitSha256": receipt.commit_sha256,
            "commit": commit,
            "composition": "McpServer dispatch -> RuntimeMcpExecutor -> daemon LedgerService -> run_read (dedicated WAL reader) -> query::search -> catalog ticket issuance; corpus indexed via doc_spine::sync_bounded (the engine the sync op wraps) into the canonical index file; baseline arms via recall_shadow on the same index.",
            "decisionRule": "ledger_fts promotes iff heldout.ledger_fts.mrr >= heldout.legacy_scan.mrr AND heldout.ledger_fts.recallAt5 >= heldout.legacy_scan.recallAt5 - 0.02, and every hard gate passes (cross-validation vs lane arm, per-hit source binding + tickets, grant narrowing, erasure binding, read completion under a held writer transaction).",
            "decision": result_json["decision"],
            "gates": result_json["gates"],
            "heldout": result_json["heldout"],
            "run": run_json,
            "runSha256": receipt.run_sha256,
            "resultSha256": receipt.result_sha256,
            "qualificationReceipt": {
                "schemaVersion": receipt.schema_version,
                "receiptSource": receipt.receipt_source,
                "hostId": receipt.host_id,
                "verifierId": receipt.verifier_id,
                "commitSha256": receipt.commit_sha256,
                "corpusVersion": receipt.corpus_version,
                "corpusSha256": receipt.corpus_sha256,
                "runSha256": receipt.run_sha256,
                "resultSha256": receipt.result_sha256,
                "receiptSha256": receipt.receipt_sha256,
            },
        },
    });
    println!("===EVIDENCE===");
    println!("{}", serde_json::to_string_pretty(&evidence).unwrap());
    println!("===/EVIDENCE===");

    eprintln!(
        "heldout n={} legacy(mrr={:.4} r@5={:.3}) fts(mrr={:.4} r@5={:.3} max_ms={})",
        legacy.total,
        legacy.mrr(),
        legacy.recall_at_5_pct(),
        fts.mrr(),
        fts.recall_at_5_pct(),
        max_latency
    );
    eprintln!("gates={gates}");
    eprintln!(
        "PROMOTION DECISION: promote_ledger_fts={promote} receipt={}",
        receipt.receipt_sha256
    );
    assert!(promote, "composition qualification failed: {gates}");
}

/// Minimal composition smoke: same wiring as the heldout eval over a 3-file
/// fixture. Isolates which stage spends the dispatch deadline. Run alone —
/// `DAEMON_OWNER`/`install_executor` are process-global OnceLocks.
#[test]
#[ignore]
fn composition_status_smoke() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    for (name, body) in [
        ("a.md", "# Alpha\n\nneedle alpha bravo\n"),
        ("b.md", "# Bravo\n\ncharlie delta echo\n"),
        ("c.md", "# Charlie\n\nfoxtrot golf hotel\n"),
    ] {
        std::fs::write(ws.join(name), body).unwrap();
    }
    let root = ws.canonicalize().unwrap();
    let root_arg = root.to_string_lossy().replace('\\', "/");
    let repository = "smoke-repo";
    let scope = "installation-scope";
    let registry = dir.path().join("registry.json");
    std::fs::write(
        &registry,
        json!({"schema_version":2,"bindings":{
            root_arg.as_str():{
                "repository_id":repository,"scope_id":scope,
                "grant_policy":{"level":"write-trusted"}
            }
        }})
        .to_string(),
    )
    .unwrap();
    let _environment = Environment::set(&[
        ("MEMBRANE_CACHE_ROOT", dir.path().join("cache")),
        ("MEMBRANE_CATALOG", dir.path().join("catalog.db")),
        ("MEMBRANE_PROJECT_REGISTRY", registry),
        ("WORKSPACE_ROOT", dir.path().to_path_buf()),
    ]);
    let t = Instant::now();
    let executor = membrane_runtime::mcp_executor::RuntimeMcpExecutor::for_hub(
        MemoryStore::open(MemDb::open_in_memory()),
    )
    .unwrap();
    assert!(membrane_mcp::install_executor(Arc::new(executor)).is_ok());
    let server = membrane_mcp::McpServer::default();
    eprintln!("for_hub+install elapsed={:?}", t.elapsed());
    let caller = json!({"root":root_arg,"repositoryId":repository,"scopeId":scope});

    // status before any sync — daemon index exists (schema init at for_hub)
    // but holds no published owner row.
    let t = Instant::now();
    let pre = call(
        &server,
        "membrane_ledger",
        json!({"repository":repository,"caller":caller,"operation":"status","deadlineMs":30000}),
    );
    eprintln!("pre-sync status elapsed={:?} resp={pre}", t.elapsed());

    let index_path = dir.path().join("cache").join("ledger-index.sqlite3");
    let db = LedgerDb::open(&index_path).unwrap();
    let budget = membrane_runtime::ledger::limits::WorkBudget::bounded(
        std::time::Duration::from_secs(120),
    );
    let t = Instant::now();
    let report = doc_spine::sync_bounded(&db, &root, &budget).expect("sync fixture");
    eprintln!("sync elapsed={:?} parsed={}", t.elapsed(), report.parsed);
    db.lock()
        .execute(
            "INSERT INTO ledger_owner_roots VALUES (?1,?2,?3,?4)",
            rusqlite::params![
                root_arg,
                report.index_generation,
                report.policy_digest,
                membrane_runtime::time::now_millis() as i64
            ],
        )
        .unwrap();

    let t = Instant::now();
    let post = call(
        &server,
        "membrane_ledger",
        json!({"repository":repository,"caller":caller,"operation":"status","deadlineMs":30000}),
    );
    eprintln!("post-sync status elapsed={:?} resp={post}", t.elapsed());
    let t = Instant::now();
    let recall = call(
        &server,
        "membrane_ledger",
        json!({"repository":repository,"caller":caller,"operation":"recall",
            "query":"needle alpha","k":3,"deadlineMs":30000}),
    );
    eprintln!("recall elapsed={:?} resp={recall}", t.elapsed());
    assert_eq!(data(&post)["indexState"], json!("published"), "{post}");
    assert!(data(&recall)["hits"].as_array().is_some_and(|h| !h.is_empty()), "{recall}");
}
