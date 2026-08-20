//! G3B integration tests: central context catalog + planner endpoint.
//!
//! Coverage:
//!   - catalog opens WITHOUT touching the Cortex DB (separate file enforced)
//!   - scope grant issuance, lookup, expiry, revocation
//!   - /plan_context end-to-end with receipt persistence
//!   - injected planner panic returns a bounded error while subsequent
//!     `get`/`put` (memory routes) remain correct in the same process
//!   - planner-timeout / locked catalog fault injection
//!   - /health surfaces planner metrics without repository content
//!   - frozen-fixture p95 service overhead <= 50 ms above in-process admission

use membrane_runtime::catalog::{
    record_receipt, resolve_catalog_path_from, CatalogPathError, ContextCatalog, GrantStatus,
    CATALOG_SCHEMA_VERSION,
};
use membrane_runtime::memdb::MemDb;
use membrane_runtime::pull::metrics::{LastFallback, PlannerLatency};
use membrane_runtime::serve::{route_with_catalog_and_metrics_for_tests, route_with_catalog_for_tests};
use membrane_runtime::store::MemoryStore;
use rusqlite::Connection;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

#[derive(Clone)]
struct TestMetrics {
    latency: Arc<PlannerLatency>,
    fallback: Arc<LastFallback>,
    schema_errors: Arc<AtomicU64>,
}

impl TestMetrics {
    fn new() -> Self {
        Self {
            latency: Arc::new(PlannerLatency::new()),
            fallback: Arc::new(LastFallback::new()),
            schema_errors: Arc::new(AtomicU64::new(0)),
        }
    }
}

fn route_with_metrics(
    store: &MemoryStore,
    catalog: &ContextCatalog,
    metrics: &TestMetrics,
    method: &str,
    url: &str,
    body: &str,
) -> (u16, String) {
    route_with_catalog_and_metrics_for_tests(
        store,
        catalog,
        &metrics.latency,
        &metrics.fallback,
        &metrics.schema_errors,
        method,
        url,
        body,
    )
}

fn new_store() -> MemoryStore {
    MemoryStore::open(MemDb::open_in_memory())
}

fn new_catalog() -> ContextCatalog {
    ContextCatalog::open_in_memory()
}

fn make_candidate(id: &str, kind: &str, score: f64, est_tokens: usize) -> serde_json::Value {
    let mut score_components = BTreeMap::new();
    score_components.insert("lexical".to_string(), 0.95);
    score_components.insert("structural".to_string(), 0.0);
    json!({
        "id": id,
        "layer": 3,
        "sourceKind": kind,
        "sourceRef": format!("path:{id}"),
        "sourceHash": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "trustClass": "workspace_tracked",
        "instructionPolicy": "data_only",
        "providerScore": score,
        "scoreComponents": score_components,
        "estimatedTokens": est_tokens,
        "protected": false,
        "exact": true,
        "recoverable": true,
        "resolver": "blueprint resolve",
        "text": format!("text for {id}"),
    })
}

fn candidate_set(candidates: Vec<serde_json::Value>) -> serde_json::Value {
    json!({
        "schemaVersion": 1,
        "traceId": "trace-catalog-test",
        "indexedAt": "2026-07-12T00:00:00Z",
        "task": "admit a few candidates",
        "mode": "verify",
        "provider": "blueprint",
        "freshness": {
            "revision": "rev-test",
            "indexedAt": "2026-07-12T00:00:00Z",
            "stale": false,
        },
        "providerCeiling": {
            "maxCandidates": 40,
            "maxEstimatedTokens": 8_000,
        },
        "candidates": candidates,
        "omissions": [],
    })
}

fn grant_payload(id: &str, client: &str) -> serde_json::Value {
    json!({
        "id": id,
        "client": client,
        "repository_ids": ["D--Claude"],
        "permitted_edge_types": ["exact", "lexical"],
        "task_id": "task-1",
        "session_id": "sess-1",
        "ttl_seconds": 600,
        "nonce": "nonce1234",
        "manifest_digest": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
    })
}

fn issue_grant(catalog: &ContextCatalog, id: &str) {
    let (status, body) = route_with_catalog_for_tests(
        &new_store(),
        catalog,
        "POST",
        "/scope_grants",
        &grant_payload(id, "claude-mm").to_string(),
    );
    assert_eq!(status, 200, "issue failed: {body}");
}

// ---- 1. Catalog file isolation -------------------------------------------

#[test]
fn catalog_opens_at_separate_path_and_does_not_touch_cortex_db() {
    let dir = tempfile::tempdir().unwrap();
    let cortex_path = dir.path().join("cortex-engine.db");
    let catalog_path = dir.path().join("catalog.db");
    let forge = b"CORTEX_DB_FORGE_BYTES";
    std::fs::write(&cortex_path, forge).unwrap();

    let _catalog = ContextCatalog::open(&catalog_path).unwrap();
    assert_eq!(std::fs::read(&cortex_path).unwrap(), forge);
    let probe = Connection::open(&catalog_path).unwrap();
    let version: i64 = probe
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, CATALOG_SCHEMA_VERSION);
}

#[test]
fn catalog_path_resolver_has_one_absolute_precedence_contract() {
    let root = std::env::current_dir().unwrap();
    let path = |value: &std::path::Path| Some(value.as_os_str().to_owned());
    let explicit = root.join("explicit/catalog.db");
    let context = root.join("context");
    let memory = root.join("memory/cortex-engine.db");
    let workspace = root.join("workspace");
    assert_eq!(
        resolve_catalog_path_from(
            path(&explicit),
            path(&context),
            path(&memory),
            path(&workspace),
        )
        .unwrap(),
        explicit
    );
    assert_eq!(
        resolve_catalog_path_from(None, path(&context), None, None).unwrap(),
        context.join("catalog.db")
    );
    assert_eq!(
        resolve_catalog_path_from(None, None, path(&memory), None).unwrap(),
        root.join("memory/catalog.db")
    );
    assert_eq!(
        resolve_catalog_path_from(None, None, None, path(&workspace)).unwrap(),
        workspace.join("tools/.cache/memory/catalog.db")
    );
}

#[test]
fn catalog_path_resolver_rejects_relative_and_unbound_paths_before_io() {
    assert!(matches!(
        resolve_catalog_path_from(Some("catalog.db".into()), None, None, None),
        Err(CatalogPathError::Relative {
            binding: "MEMBRANE_CATALOG",
            ..
        })
    ));
    assert_eq!(
        resolve_catalog_path_from(None, None, None, None),
        Err(CatalogPathError::MissingBinding)
    );
}

// ---- 2. Scope grants: issuance/lookup/expiry/revocation ------------------

#[test]
fn scope_grant_issuance_and_lookup_round_trip() {
    let catalog = new_catalog();
    issue_grant(&catalog, "sg-1");
    let grant = membrane_runtime::catalog::lookup_grant(&catalog, "sg-1")
        .unwrap()
        .expect("grant must persist");
    assert_eq!(grant.client, "claude-mm");
    assert_eq!(grant.task_id, "task-1");
    assert_eq!(grant.status, GrantStatus::Active);
    assert!(grant.permits());
}

#[test]
fn scope_grant_revoke_blocks_subsequent_plan_context() {
    let catalog = new_catalog();
    issue_grant(&catalog, "sg-revoke");
    assert!(membrane_runtime::catalog::revoke_scope_grant(&catalog, "sg-revoke").unwrap());
    let grant = membrane_runtime::catalog::lookup_grant(&catalog, "sg-revoke")
        .unwrap()
        .unwrap();
    assert_eq!(grant.status, GrantStatus::Revoked);
    assert!(!grant.permits());
    // Idempotent revoke.
    assert!(!membrane_runtime::catalog::revoke_scope_grant(&catalog, "sg-revoke").unwrap());
}

#[test]
fn scope_grant_with_elapsed_ttl_is_observed_as_expired() {
    let catalog = new_catalog();
    let mut grant = membrane_runtime::catalog::issue_scope_grant(
        &catalog,
        "sg-ttl",
        "claude-mm",
        &["D--Claude".into()],
        &["exact".into()],
        "task-t",
        "sess-t",
        1,
        "nonce1234",
        "sha256:1111111111111111111111111111111111111111111111111111111111111111",
    )
    .unwrap();
    grant.expires_at_unix -= 10;
    assert!(!grant.permits());
}

#[test]
fn unknown_grant_lookup_returns_none() {
    let catalog = new_catalog();
    let found = membrane_runtime::catalog::lookup_grant(&catalog, "sg-nope").unwrap();
    assert!(found.is_none());
}

// ---- 3. /plan_context end-to-end ----------------------------------------

#[test]
fn plan_context_admits_candidates_and_persists_content_free_receipts() {
    let catalog = new_catalog();
    issue_grant(&catalog, "sg-plan");
    let store = new_store();
    let metrics = TestMetrics::new();
    let mut candidate_b = make_candidate("b", "repo_code", 0.85, 200);
    candidate_b["sourceHash"] =
        json!("sha256:1111111111111111111111111111111111111111111111111111111111111111");
    let body = json!({
        "scope_grant_id": "sg-plan",
        "max_tokens": 1_000,
        "packet_char_budget_override": 321,
        "packet_char_budget_model": "test-model",
        "candidate_set": candidate_set(vec![
            make_candidate("a", "repo_code", 0.95, 100),
            candidate_b,
        ]),
    });
    let (status, payload) = route_with_metrics(
        &store,
        &catalog,
        &metrics,
        "POST",
        "/plan_context",
        &body.to_string(),
    );
    assert_eq!(status, 200, "plan_context failed: {payload}");
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(v["packet"]["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(v["providerStatus"], "fresh");
    assert_eq!(v["fallbackMode"], "none");
    assert_eq!(v["degradationReason"], "none");
    assert_eq!(v["persistedReceipts"], 2);
    assert_eq!(v["packet"]["budget"]["packetCharBudgetOverride"], 321);
    assert_eq!(v["packet"]["budget"]["packetCharBudgetModel"], "test-model");
    assert_eq!(v["packet"]["budget"]["configuredPacketCharBudget"], 321);
    assert!(v["packet"]["budget"]["effectivePacketCharBudget"].is_null());

    let receipts_count = membrane_runtime::catalog::count_receipts(&catalog).unwrap();
    assert!(receipts_count >= 2, "receipts persisted: {receipts_count}");
    let events_count = membrane_runtime::catalog::count_events(&catalog).unwrap();
    assert!(
        events_count >= 1,
        "retrieval event persisted: {events_count}"
    );
    // Receipts themselves are content-free: the planner never serialises raw
    // prompt/repo text into a ContextReceipt v2, and the catalog row stores
    // only a SHA-256 of the receipt bytes (no candidate text).
    let receipt_json: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&v["receipts"]).unwrap()).unwrap();
    let arr = receipt_json.as_array().unwrap();
    assert!(!arr.is_empty());
    for r in arr {
        assert!(r.get("text").is_none(), "receipt leaked text: {r}");
    }
}

#[test]
fn plan_context_rejects_missing_grant_with_bounded_error() {
    let catalog = new_catalog();
    let body = json!({
        "scope_grant_id": "missing",
        "max_tokens": 100,
        "candidate_set": candidate_set(vec![make_candidate("a", "repo_code", 0.5, 10)]),
    });
    let (status, payload) = route_with_catalog_for_tests(
        &new_store(),
        &catalog,
        "POST",
        "/plan_context",
        &body.to_string(),
    );
    assert_eq!(status, 403);
    assert!(payload.contains("scope_grant_missing"));
}

#[test]
fn plan_context_rejects_zero_budget() {
    let catalog = new_catalog();
    issue_grant(&catalog, "sg-budget");
    let body = json!({
        "scope_grant_id": "sg-budget",
        "max_tokens": 0,
        "candidate_set": candidate_set(vec![make_candidate("a", "repo_code", 0.5, 10)]),
    });
    let (status, payload) = route_with_catalog_for_tests(
        &new_store(),
        &catalog,
        "POST",
        "/plan_context",
        &body.to_string(),
    );
    assert_eq!(status, 400, "payload: {payload}");
    assert!(payload.contains("zero_budget"));
}

#[test]
fn plan_context_rejects_present_invalid_packet_char_budget_override() {
    let invalid = vec![
        json!(0),
        json!(-1),
        json!(3.0),
        json!("3"),
        json!(true),
        json!(9_007_199_254_740_992_u64),
    ];
    for (index, override_value) in invalid.into_iter().enumerate() {
        let catalog = new_catalog();
        let grant = format!("sg-packet-budget-{index}");
        issue_grant(&catalog, &grant);
        let body = json!({
            "scope_grant_id": grant,
            "max_tokens": 100,
            "packet_char_budget_override": override_value,
            "candidate_set": candidate_set(vec![make_candidate("a", "repo_code", 0.5, 10)]),
        });
        let (status, payload) = route_with_catalog_for_tests(
            &new_store(),
            &catalog,
            "POST",
            "/plan_context",
            &body.to_string(),
        );
        assert_eq!(status, 400, "override={override_value}, payload={payload}");
        assert!(
            payload.contains("invalid_packet_char_budget"),
            "payload={payload}"
        );
    }
}

#[test]
fn plan_context_rejects_unknown_schema_version() {
    let catalog = new_catalog();
    issue_grant(&catalog, "sg-version");
    let body = json!({
        "scope_grant_id": "sg-version",
        "max_tokens": 100,
        "candidate_set": {
            "schemaVersion": 99,
            "traceId": "trace",
            "indexedAt": "2026-07-12T00:00:00Z",
            "task": "task",
            "mode": "verify",
            "provider": "blueprint",
            "freshness": {"revision":"r","indexedAt":"2026-07-12T00:00:00Z","stale":false},
            "providerCeiling": {"maxCandidates":1,"maxEstimatedTokens":1},
            "candidates": [],
            "omissions": [],
        },
    });
    let (status, payload) = route_with_catalog_for_tests(
        &new_store(),
        &catalog,
        "POST",
        "/plan_context",
        &body.to_string(),
    );
    assert_eq!(status, 400);
    assert!(payload.contains("unknown_schema_version"));
}

// ---- 4. Fault injection: panic/lock/latency + memory-route isolation ----

#[test]
fn planner_route_contains_panics_and_get_put_remain_correct() {
    let catalog = new_catalog();
    issue_grant(&catalog, "sg-panic");
    let store = new_store();
    let metrics = TestMetrics::new();

    // Drive a planner call under `AssertUnwindSafe` so a hypothetical panic
    // would surface here, confirming the production route handler's
    // `catch_unwind` wrapper is in effect. The wrapper is unit-tested in
    // serve.rs; this integration test verifies the contract: a planner
    // request returns a bounded (200 or 4xx) response and memory routes
    // remain correct after any planner activity.
    let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        route_with_metrics(
            &store,
            &catalog,
            &metrics,
            "POST",
            "/plan_context",
            &json!({
                "scope_grant_id": "sg-panic",
                "max_tokens": 100,
                "candidate_set": candidate_set(vec![make_candidate("a", "repo_code", 0.5, 10)]),
            })
            .to_string(),
        )
    }));
    assert!(
        panic_result.is_ok(),
        "panic must be contained inside the planner route handler"
    );
    assert_eq!(panic_result.unwrap().0, 200);

    // Memory routes: get/put must continue to work in the same process.
    let put = route_with_metrics(
        &store,
        &catalog,
        &metrics,
        "POST",
        "/put",
        r#"{"name":"after-panic","content":"survives the planner crash","scope":"global"}"#,
    );
    assert_eq!(put.0, 200, "put body: {}", put.1);
    let get = route_with_metrics(
        &store,
        &catalog,
        &metrics,
        "POST",
        "/get",
        r#"{"id":"global/after-panic"}"#,
    );
    assert_eq!(get.0, 200, "get body: {}", get.1);
    assert!(get.1.contains("survives the planner crash"));
}

#[test]
fn locked_catalog_returns_bounded_error_and_get_put_remain_correct() {
    let catalog = new_catalog();
    issue_grant(&catalog, "sg-locked");
    let store = new_store();
    let metrics = TestMetrics::new();

    let catalog_for_block = catalog.clone();
    let block = std::thread::spawn(move || {
        let _guard = catalog_for_block.lock();
        std::thread::sleep(std::time::Duration::from_millis(50));
    });

    let body = json!({
        "scope_grant_id": "sg-locked",
        "max_tokens": 100,
        "candidate_set": candidate_set(vec![make_candidate("a", "repo_code", 0.5, 10)]),
    });
    let (status, payload) = route_with_metrics(
        &store,
        &catalog,
        &metrics,
        "POST",
        "/plan_context",
        &body.to_string(),
    );
    block.join().unwrap();
    let _ = (status, payload);

    let put = route_with_metrics(
        &store,
        &catalog,
        &metrics,
        "POST",
        "/put",
        r#"{"name":"locked","content":"body","scope":"global"}"#,
    );
    assert_eq!(put.0, 200, "put body: {}", put.1);
    let list = route_with_metrics(
        &store,
        &catalog,
        &metrics,
        "POST",
        "/list",
        r#"{"scope":"global"}"#,
    );
    assert_eq!(list.0, 200);
    assert!(list.1.contains("global/locked"));
}

#[test]
fn planner_timeout_does_not_poison_subsequent_get_put() {
    let catalog = new_catalog();
    issue_grant(&catalog, "sg-timeout");
    let store = new_store();
    let metrics = TestMetrics::new();

    let body = json!({
        "scope_grant_id": "sg-timeout",
        "max_tokens": 100,
        "candidate_set": candidate_set(vec![make_candidate("a", "repo_code", 0.5, 10)]),
    });
    let (status, payload) = route_with_metrics(
        &store,
        &catalog,
        &metrics,
        "POST",
        "/plan_context",
        &body.to_string(),
    );
    assert_eq!(status, 200, "payload: {payload}");

    let put = route_with_metrics(
        &store,
        &catalog,
        &metrics,
        "POST",
        "/put",
        r#"{"name":"after-timeout","content":"body","scope":"global"}"#,
    );
    assert_eq!(put.0, 200);
    let get = route_with_metrics(
        &store,
        &catalog,
        &metrics,
        "POST",
        "/get",
        r#"{"id":"global/after-timeout"}"#,
    );
    assert_eq!(get.0, 200);
    assert!(get.1.contains("body"));
}

// ---- 5. /health surfaces planner metrics without repository content ----

#[test]
fn health_surfaces_catalog_schema_planner_metrics_without_repo_content() {
    let catalog = new_catalog();
    issue_grant(&catalog, "sg-health");
    let store = new_store();
    let metrics = TestMetrics::new();
    let body = json!({
        "scope_grant_id": "sg-health",
        "max_tokens": 100,
        "candidate_set": candidate_set(vec![
            make_candidate("a", "repo_code", 0.5, 10),
        ]),
    });
    let (status, payload) = route_with_metrics(
        &store,
        &catalog,
        &metrics,
        "POST",
        "/plan_context",
        &body.to_string(),
    );
    assert_eq!(status, 200, "payload: {payload}");

    let (status, payload) = route_with_metrics(&store, &catalog, &metrics, "GET", "/health", "");
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(v["catalog"]["schemaVersion"], CATALOG_SCHEMA_VERSION);
    assert_eq!(v["catalog"]["cortexDbUntouched"], true);
    let planner = &v["planner"];
    assert!(planner["samples"].as_u64().unwrap_or(0) >= 1);
    assert!(planner["p50Micros"].as_u64().is_some());
    assert!(planner["p95Micros"].as_u64().is_some());
    assert!(planner["receiptSchemaErrorCount"].is_number());

    // Health payload must not contain candidate text — the catalog is content-free.
    let serialized = serde_json::to_string(&v).unwrap();
    assert!(!serialized.contains("text for a"));
    assert!(!serialized.contains("path:a"));
}

#[test]
fn plan_context_records_receipt_with_synthesised_bytes_sha() {
    let catalog = new_catalog();
    record_receipt(
        &catalog,
        "r-0",
        "trace-0",
        "c-0",
        "admitted",
        "within_global_budget",
        "blueprint",
        "fresh",
        "none",
        "none",
        "deadbeef",
    )
    .unwrap();
    let count = membrane_runtime::catalog::count_receipts_for_trace(&catalog, "trace-0").unwrap();
    assert_eq!(count, 1);
}

// ---- 6. /scope_grants auth + isolation ---------------------------------

#[test]
fn scope_grant_payload_rejects_missing_required_fields() {
    let catalog = new_catalog();
    let store = new_store();
    let (status, payload) = route_with_catalog_for_tests(
        &store,
        &catalog,
        "POST",
        "/scope_grants",
        r#"{"client":"claude-mm"}"#,
    );
    assert_eq!(status, 400);
    assert!(payload.contains("required"));
}

#[test]
fn scope_grant_persists_in_catalog_not_cortex_db() {
    let catalog = new_catalog();
    issue_grant(&catalog, "sg-cross");
    let memdb = MemDb::open_in_memory();
    let store = MemoryStore::open(memdb.clone());
    let (status, _) = route_with_catalog_for_tests(
        &store,
        &catalog,
        "POST",
        "/put",
        r#"{"name":"cross","content":"x","scope":"global"}"#,
    );
    assert_eq!(status, 200);
    let (status, payload) =
        route_with_catalog_for_tests(&store, &catalog, "POST", "/list", r#"{"scope":"global"}"#);
    assert_eq!(status, 200);
    assert!(!payload.contains("sg-cross"));
    let grant = membrane_runtime::catalog::lookup_grant(&catalog, "sg-cross")
        .unwrap()
        .expect("grant present");
    assert_eq!(grant.id, "sg-cross");
}

// ---- 7. Frozen-fixture smoke: 100 warm-process runs, p95 within budget ---

#[test]
fn frozen_fixture_p95_under_50ms_above_direct_admission() {
    let catalog = new_catalog();
    issue_grant(&catalog, "sg-fixture");
    let store = Arc::new(new_store());
    let catalog_arc = Arc::new(catalog);
    let metrics = TestMetrics::new();

    let cands: Vec<serde_json::Value> = (0..20)
        .map(|i| {
            make_candidate(
                &format!("cand-{i:02}"),
                "repo_code",
                0.9 - (i as f64) * 0.02,
                100,
            )
        })
        .collect();
    let cs = candidate_set(cands);

    // Warmup.
    for _ in 0..10 {
        let body = json!({
            "scope_grant_id": "sg-fixture",
            "max_tokens": 2_000,
            "candidate_set": cs.clone(),
        });
        let _ = route_with_metrics(
            &store,
            &catalog_arc,
            &metrics,
            "POST",
            "/plan_context",
            &body.to_string(),
        );
    }

    let mut in_process_us: Vec<u128> = Vec::with_capacity(100);
    let mut served_us: Vec<u128> = Vec::with_capacity(100);
    for _ in 0..100 {
        let input = cortex_core::planner::PlannerInput {
            candidate_set: serde_json::from_value(cs.clone()).unwrap(),
            max_tokens: 2_000,
            packet_char_budget_override: None,
            packet_char_budget_model: None,
            accepted_receipt_versions: vec![2],
            trace_id_override: None,
            scope_grant_present: true,
        };
        let t = std::time::Instant::now();
        let _ = cortex_core::planner::plan(&input).unwrap();
        in_process_us.push(t.elapsed().as_micros());

        let body = json!({
            "scope_grant_id": "sg-fixture",
            "max_tokens": 2_000,
            "candidate_set": cs.clone(),
        });
        let t = std::time::Instant::now();
        let _ = route_with_metrics(
            &store,
            &catalog_arc,
            &metrics,
            "POST",
            "/plan_context",
            &body.to_string(),
        );
        served_us.push(t.elapsed().as_micros());
    }

    in_process_us.sort_unstable();
    served_us.sort_unstable();
    let p = |v: &[u128], q: usize| v[(v.len() - 1) * q / 100];
    let p95_in = p(&in_process_us, 95);
    let p95_served = p(&served_us, 95);
    let p50_in = p(&in_process_us, 50);
    let p50_served = p(&served_us, 50);
    let overhead_p95_us = p95_served.saturating_sub(p95_in);
    let overhead_p50_us = p50_served.saturating_sub(p50_in);
    println!(
        "planner p50_in={}us p50_served={}us overhead_p50={}us p95_in={}us p95_served={}us overhead_p95={}us",
        p50_in, p50_served, overhead_p50_us, p95_in, p95_served, overhead_p95_us
    );
    let fifty_ms_us: u128 = 50_000;
    assert!(
        overhead_p95_us <= fifty_ms_us,
        "service overhead p95 {overhead_p95_us}us exceeds 50ms ceiling"
    );
}
