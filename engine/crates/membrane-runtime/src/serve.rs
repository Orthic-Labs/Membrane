//! Resident HTTP service — `GET /health|/metrics|/activity`, `POST /activity|/recall|/use|/get|/put|
//! /remember|/remember_consolidated|/delete|/list|/scopes|/search|/quarantine/*|/compress|
//! /plan_context|/scope_grants` on the configured loopback port
//! (canonical workspace: 47851), the contract the workspace hooks speak. Wraps the real
//! `MemoryStore`; the workspace runs THIS, not the CR product. No `/runc` by design — the resident
//! service must not exec commands. `/skel` and `/prep` were removed 2026-07-05 (consumer-less
//! post-cutover, §10.2's own rule — the CLI verbs remain the surface for those transforms).
//!
//! G3B (2026-07-12): extends the service with a context-planner endpoint backed by a separate
//! catalog SQLite. The planner route is failure-isolated from `get`/`put`/`recall` — a planner
//! error must NOT break memory routes. The `/health` response surfaces catalog schema version,
//! planner mode, provider status, last fallback reason/time, receipt-schema error count, and
//! recent p50/p95 latency without repository content.

use crate::catalog::{self, ContextCatalog, CATALOG_SCHEMA_VERSION};
use crate::memdb::MemDb;
use crate::pull::planner::{
    plan as plan_context, ContextCandidateSetV1, PlannerError, PlannerInput,
};
use crate::scope::{normalize_scope, scope_chain};
use crate::store::{
    ExternalLifecycleStage, MemoryBatchError, MemoryBatchRequest, MemoryEventContext,
    MemoryLifecycleInputV1, MemoryLifecycleOperationV1, MemoryStore, VerifiedMemoryActor,
};
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::timeout::TimeoutLayer;

const MAX_BODY_BYTES: usize = 1 << 20;
const MAX_PUSH_BODY_BYTES: usize = 8 << 20;
const MAX_QUERY_CHARS: usize = 8 * 1024;
const MAX_CONTENT_CHARS: usize = 256 * 1024;
const MAX_RECALL_K: u64 = 50;
static TASTE_REQUEST_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
const MAX_CONCURRENT_REQUESTS: usize = 32;
// FastEmbedder owns one TextEmbedding behind a Mutex and compression can own a
// separate ONNX runtime. Keep all model-heavy work behind one conservative lane
// until target-hardware benchmarks justify splitting the budgets.
const MAX_MODEL_QUEUE_REQUESTS: usize = 8;
const MAX_MODEL_EXECUTION: usize = 1;
// Keep this below the CLI's 120-second read deadline and above the observed
// CPU embedding latency so the server, CLI, and outer pipeline fail in order.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
// Detailed diagnostics may touch SQLite and filesystem state. Bound the caller
// independently so a wedged dependency cannot make the health request hang.
#[cfg(not(test))]
const DETAILED_HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(test)]
const DETAILED_HEALTH_TIMEOUT: Duration = Duration::from_millis(100);
const IDEMPOTENCY_REGISTRY_CAPACITY: usize = 1024;
const MAX_IDEMPOTENT_RESPONSE_BYTES: usize = 32 * 1024;
const DEFAULT_ANCHOR_RETRIEVE_BYTES: usize = 64 * 1024;
const MAX_ANCHOR_RETRIEVE_BYTES: usize = 256 * 1024;
const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";
const DIAGNOSTICS_WORKER_EXIT_TIMEOUT: Duration = Duration::from_millis(100);
const SNAPSHOT_MAX_SECTIONS: usize = 16;
const SNAPSHOT_MAX_ITEMS_PER_SECTION: usize = 1000;
const SNAPSHOT_MAX_REASON_BYTES: usize = 200;
const SNAPSHOT_MAX_ITEM_LABEL_BYTES: usize = 128;
const SNAPSHOT_MAX_ITEM_KIND_BYTES: usize = 64;
const RESERVED_ADAPT_ADMISSION_REASON: &str = "reserved_adapt_authority";
const SNAPSHOT_MAX_ITEM_STRING_BYTES: usize = 512;
const SNAPSHOT_MAX_TOTAL_BYTES: usize = 65_536;
#[cfg(test)]
const TEST_GATE_WAIT_TIMEOUT: Duration = Duration::from_secs(1);

struct PromptTelemetryWorker {
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PromptTelemetryWorker {
    fn stop(mut self) -> Result<(), String> {
        self.running.store(false, Ordering::Release);
        self.thread
            .take()
            .expect("prompt telemetry worker handle present")
            .join()
            .map_err(|_| "cortex prompt telemetry drain panicked".to_string())
    }
}

impl Drop for PromptTelemetryWorker {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod prompt_telemetry_worker_tests {
    use super::*;

    #[test]
    fn drop_stops_and_joins_worker_on_early_return() {
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let exited = Arc::new(AtomicBool::new(false));
        let thread_exited = exited.clone();
        let thread = std::thread::spawn(move || {
            while thread_running.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            thread_exited.store(true, Ordering::Release);
        });
        let worker = PromptTelemetryWorker {
            running,
            thread: Some(thread),
        };

        drop(worker);

        assert!(exited.load(Ordering::Acquire));
    }
}

fn str_list(v: &serde_json::Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Body of a memory with any YAML (`---`) or TOML (`+++`) frontmatter block removed. An unclosed
/// fence falls through to the raw content rather than eating the whole memory.
fn strip_frontmatter(content: &str) -> &str {
    for fence in ["---", "+++"] {
        if let Some(rest) = content.strip_prefix(&format!("{fence}\n")) {
            if let Some(end) = rest.find(&format!("\n{fence}")) {
                return rest[end + fence.len() + 1..].trim_start_matches('\n');
            }
        }
    }
    content
}

/// Injection preview: first ~`max` chars of real prose. Skips frontmatter, HTML comments, blank
/// lines, code-fence markers, and short heading-only lines (titles), then joins what remains.
/// This is what the recall hook shows — for short rules the preview IS the delivered value, so
/// its quality directly drives injection usefulness (E6, 2026-07-05). The fallback ladder never
/// returns raw frontmatter: filtered body → post-frontmatter text.
fn preview(content: &str, max: usize) -> String {
    let after_fm = strip_frontmatter(content);
    let body: Vec<&str> = after_fm
        .lines()
        .filter(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with("<!--") || t.starts_with("```") {
                return false;
            }
            if t.starts_with('#') {
                // Heading: a short one is a title (drop); a long one carries content (keep).
                return t.trim_start_matches('#').trim().len() >= 40;
            }
            true
        })
        .collect();
    let joined = body.join(" ");
    let src = if joined.trim().is_empty() {
        after_fm
    } else {
        &joined
    };
    src.chars().take(max).collect()
}

const DASHBOARD_HTML: &str = include_str!("dashboard.html");

fn latest_analysis_path(directory: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let mut reports = std::fs::read_dir(directory)
        .map_err(|error| format!("read analysis directory {}: {error}", directory.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    reports.sort();
    reports
        .pop()
        .ok_or_else(|| format!("no analysis reports in {}", directory.display()))
}

fn latest_analysis_json(directory: &std::path::Path) -> Result<serde_json::Value, String> {
    let latest = latest_analysis_path(directory)?;
    let raw = std::fs::read_to_string(&latest)
        .map_err(|error| format!("read analysis report {}: {error}", latest.display()))?;
    let report: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("parse analysis report {}: {error}", latest.display()))?;
    let generated = report
        .get("generated_at")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!(
                "analysis stale: missing generated_at in {}",
                latest.display()
            )
        })?;
    let today = crate::time::now_iso();
    if generated.get(..10) != today.get(..10) {
        return Err(format!(
            "analysis stale: latest report {} is not from the current UTC date",
            latest.display()
        ));
    }
    Ok(report)
}

fn analysis_watchdog_snapshot(directory: &std::path::Path) -> serde_json::Value {
    let age_seconds = latest_analysis_path(directory)
        .ok()
        .and_then(|path| std::fs::metadata(path).ok())
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| std::time::SystemTime::now().duration_since(modified).ok())
        .map(|age| age.as_secs());
    match latest_analysis_json(directory) {
        Ok(report) => json!({
            "status": "fresh",
            "alert": false,
            "lastSuccessAgeSeconds": age_seconds,
            "generatedAt": report.get("generated_at").and_then(Value::as_str),
        }),
        Err(error) => json!({
            "status": if age_seconds.is_some() { "stale" } else { "unavailable" },
            "alert": true,
            "lastSuccessAgeSeconds": age_seconds,
            "reason": if error.contains("stale") { "stale_output" } else { "missing_output" },
        }),
    }
}

fn analysis_response(directory: &std::path::Path) -> (u16, String) {
    match latest_analysis_json(directory) {
        Ok(report) => (200, report.to_string()),
        Err(error) => (
            503,
            serde_json::json!({ "error": "analysis unavailable", "detail": error }).to_string(),
        ),
    }
}

fn configured_analysis_directory() -> std::path::PathBuf {
    std::env::var_os("MEMBRANE_DAILY_ANALYSIS_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("WORKSPACE_ROOT")
                .map(std::path::PathBuf::from)
                .map(|root| root.join("tools/.cache/metrics/daily-analysis"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("tools/.cache/metrics/daily-analysis"))
}

fn configured_workspace_root() -> std::path::PathBuf {
    std::env::var_os("MEMBRANE_REPO_ROOT")
        .or_else(|| std::env::var_os("WORKSPACE_ROOT"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
}

fn configured_anchor_directory() -> std::path::PathBuf {
    std::env::var_os("MEMBRANE_ANCHOR_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| configured_workspace_root().join("tools/.cache/runc"))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    use sha2::Digest;
    let mut encoded = String::with_capacity(64);
    for byte in sha2::Sha256::digest(bytes) {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn anchor_retrieve_response(body: &str) -> (u16, String) {
    let value = match json_body(body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(repo) = value
        .get("repo")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return (400, json!({"error":"repo required"}).to_string());
    };
    let Some(anchor) = value
        .get("anchor")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return (400, json!({"error":"anchor required"}).to_string());
    };
    let max_bytes = match value.get("maxBytes") {
        None => DEFAULT_ANCHOR_RETRIEVE_BYTES,
        Some(value) => match value.as_u64().and_then(|value| usize::try_from(value).ok()) {
            Some(value) if value > 0 => value.min(MAX_ANCHOR_RETRIEVE_BYTES),
            _ => {
                return (
                    400,
                    json!({"error":"maxBytes must be a positive integer"}).to_string(),
                )
            }
        },
    };
    let workspace = match configured_workspace_root().canonicalize() {
        Ok(path) => path,
        Err(_) => {
            return (
                503,
                json!({"error":"workspace root unavailable"}).to_string(),
            )
        }
    };
    let repo = match std::path::Path::new(repo).canonicalize() {
        Ok(path) if path.starts_with(&workspace) => path,
        _ => {
            return (
                403,
                json!({"error":"repo is outside configured workspace"}).to_string(),
            )
        }
    };
    let requested = std::path::Path::new(anchor);
    let file = match (if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        repo.join(requested)
    })
    .canonicalize()
    {
        Ok(path) if path.starts_with(&repo) => path,
        _ => return (403, json!({"error":"anchor is outside repo"}).to_string()),
    };
    let metadata = match std::fs::metadata(&file) {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => {
            return (
                400,
                json!({"error":"anchor must resolve to a regular file"}).to_string(),
            )
        }
    };
    let read_limit = max_bytes.saturating_add(4);
    let mut bytes = Vec::with_capacity(read_limit);
    let read_result = (|| -> std::io::Result<()> {
        use std::io::Read;
        std::fs::File::open(&file)?
            .take(read_limit as u64)
            .read_to_end(&mut bytes)?;
        Ok(())
    })();
    if read_result.is_err() {
        return (
            400,
            json!({"error":"anchor file could not be read"}).to_string(),
        );
    }
    let complete = metadata.len() <= read_limit as u64;
    let valid_end = match std::str::from_utf8(&bytes) {
        Ok(_) => bytes.len(),
        Err(error) if !complete && error.error_len().is_none() => error.valid_up_to(),
        Err(_) => {
            return (
                400,
                json!({"error":"anchor file must be valid UTF-8"}).to_string(),
            )
        }
    };
    let content_end = valid_end.min(max_bytes);
    let content_end = std::str::from_utf8(&bytes[..content_end])
        .map_or_else(|error| error.valid_up_to(), |_| content_end);
    let content = match std::str::from_utf8(&bytes[..content_end]) {
        Ok(content) => content,
        Err(_) => {
            return (
                400,
                json!({"error":"anchor file must be valid UTF-8"}).to_string(),
            )
        }
    };
    let truncated = metadata.len() > content_end as u64;
    (
        200,
        json!({
            "path": file.strip_prefix(&repo).unwrap_or(&file).to_string_lossy(),
            "sha256": sha256_bytes(content.as_bytes()),
            "content": content,
            "truncated": truncated,
        })
        .to_string(),
    )
}

fn expand_anchor_response(body: &str, anchor_directory: &std::path::Path) -> (u16, String) {
    let value = match json_body(body) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let Some(anchor) = value.get("anchor").and_then(Value::as_str) else {
        return (400, json!({"error":"anchor required"}).to_string());
    };
    let digest = match crate::ledger::identifier::AnchorRef::parse(anchor) {
        Ok(reference) => reference.digest(),
        Err(_) => return (400, json!({"error":"invalid anchor"}).to_string()),
    };
    let root = match anchor_directory.canonicalize() {
        Ok(root) => root,
        Err(_) => return (503, json!({"error":"anchor store unavailable"}).to_string()),
    };
    let file = match root.join(format!("{digest}.log")).canonicalize() {
        Ok(file) if file.starts_with(&root) && file.is_file() => file,
        _ => return (404, json!({"error":"anchor not found"}).to_string()),
    };
    let metadata = file.with_extension("json");
    if let Ok(raw) = std::fs::read_to_string(&metadata) {
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            if value
                .get("expiresAtMillis")
                .and_then(Value::as_u64)
                .is_some_and(|expires| expires < crate::time::now_millis() as u64)
            {
                return (410, json!({"error":"anchor expired"}).to_string());
            }
        }
    }
    let content = match std::fs::read_to_string(&file) {
        Ok(content) => content,
        Err(_) => return (400, json!({"error":"anchor unreadable"}).to_string()),
    };
    let marker = std::fs::read_to_string(&metadata).ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value.get("recovery").cloned())
        .and_then(|value| serde_json::from_value::<crate::push::compress::RecoveryMarkerV1>(value).ok());
    if sha256_bytes(content.as_bytes()) != digest || !marker.as_ref().is_some_and(|marker| crate::push::compress::verify_recovery_marker(marker, content.as_bytes(), crate::push::recovery::now_ms())) {
        return (409, json!({"error":"anchor integrity or metadata invalid"}).to_string());
    }
    (
        200,
        json!({"anchor":anchor,"sha256":sha256_bytes(content.as_bytes()),"content":content})
            .to_string(),
    )
}

fn delivery_trace_response(body: &str) -> (u16, String) {
    let report: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(_) => return (400, json!({"error":"invalid_json"}).to_string()),
    };
    let view = crate::delivery_trace_view::project_delivery_trace(&report);
    match serde_json::to_string(&view) {
        Ok(serialized) => (200, serialized),
        Err(_) => (500, json!({"error":"serialization_failed"}).to_string()),
    }
}

#[derive(Clone)]
struct AppState {
    store: Arc<MemoryStore>,
    context_ingest_lease: Option<Arc<crate::context_telemetry::ContextIngestLease>>,
    freshness: Arc<crate::freshness::FreshnessCoordinator>,
    catalog: Option<Arc<ContextCatalog>>,
    api_token: Option<Arc<str>>,
    allowed_origins: Arc<[String]>,
    /// Rolling latency tracker for `/plan_context` requests. Bounded ring; the
    /// p50/p95 appears in `/health` so an operator can see service overhead
    /// without exposing request bodies.
    planner_latency: Arc<crate::pull::metrics::PlannerLatency>,
    /// Snapshot of the most recent fallback observed by `/plan_context`.
    planner_last_fallback: Arc<crate::pull::metrics::LastFallback>,
    /// Counter of receipts that the planner emitted with an unparsable schema
    /// (defensive: the planner is pure and shouldn't ever emit broken JSON,
    /// but we count rather than fail open). Exposed via `/health`.
    planner_schema_error_count: Arc<std::sync::atomic::AtomicU64>,
    workers: Arc<WorkerAdmission>,
    diagnostics_executor: Arc<DiagnosticsExecutor>,
    idempotency: Arc<IdempotencyRegistry>,
    #[cfg(test)]
    test_control: Arc<TestControl>,
}

#[cfg(test)]
#[derive(Default)]
struct TestControl {
    workload: TestGate,
    diagnostics: TestGate,
}

#[cfg(test)]
#[derive(Default)]
struct TestGate {
    started: std::sync::atomic::AtomicUsize,
    finished: std::sync::atomic::AtomicUsize,
    released: std::sync::atomic::AtomicUsize,
    release_lock: std::sync::Mutex<()>,
    release_ready: std::sync::Condvar,
    started_ready: tokio::sync::Notify,
    finished_ready: tokio::sync::Notify,
}

#[cfg(test)]
impl TestGate {
    fn enter(&self) {
        let ticket = self
            .started
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            + 1;
        self.started_ready.notify_waiters();
        let deadline = Instant::now() + TEST_GATE_WAIT_TIMEOUT;
        let mut guard = self
            .release_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while self.released.load(std::sync::atomic::Ordering::Acquire) < ticket {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_default();
            if remaining.is_zero() {
                return;
            }
            let (next_guard, wait_result) = self
                .release_ready
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|error| error.into_inner());
            guard = next_guard;
            if wait_result.timed_out()
                && self.released.load(std::sync::atomic::Ordering::Acquire) < ticket
            {
                return;
            }
        }
        self.finished
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.finished_ready.notify_waiters();
    }

    async fn wait_started(&self, count: usize) {
        let result = tokio::time::timeout(TEST_GATE_WAIT_TIMEOUT, async {
            loop {
                let notified = self.started_ready.notified();
                if self.started.load(std::sync::atomic::Ordering::Acquire) >= count {
                    return;
                }
                notified.await;
            }
        })
        .await;
        if result.is_err() {
            panic!(
                "test gate did not start {count} jobs within {TEST_GATE_WAIT_TIMEOUT:?}; observed {}",
                self.started.load(std::sync::atomic::Ordering::Acquire)
            );
        }
    }

    async fn wait_finished(&self, count: usize) {
        let result = tokio::time::timeout(TEST_GATE_WAIT_TIMEOUT, async {
            loop {
                let notified = self.finished_ready.notified();
                if self.finished.load(std::sync::atomic::Ordering::Acquire) >= count {
                    return;
                }
                notified.await;
            }
        })
        .await;
        if result.is_err() {
            panic!(
                "test gate did not finish {count} jobs within {TEST_GATE_WAIT_TIMEOUT:?}; observed {}",
                self.finished.load(std::sync::atomic::Ordering::Acquire)
            );
        }
    }

    fn release(&self, count: usize) {
        let _guard = self
            .release_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.released
            .fetch_max(count, std::sync::atomic::Ordering::Release);
        self.release_ready.notify_all();
    }
}

type DiagnosticsJob = Box<dyn FnOnce() + Send + 'static>;

/// One process-local diagnostics worker. Detailed health never consumes Tokio's global blocking
/// pool, and the one-slot admission semaphore ensures this bounded channel cannot accumulate work.
struct DiagnosticsExecutor {
    sender: Option<std::sync::mpsc::SyncSender<DiagnosticsJob>>,
    worker_exit: Option<std::sync::Mutex<std::sync::mpsc::Receiver<()>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl DiagnosticsExecutor {
    fn new() -> Self {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<DiagnosticsJob>(1);
        let (worker_exit_sender, worker_exit) = std::sync::mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("cortex-diagnostics".to_string())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                }
                let _ = worker_exit_sender.send(());
            }) {
            Ok(worker) => Self {
                sender: Some(sender),
                worker_exit: Some(std::sync::Mutex::new(worker_exit)),
                worker: Some(worker),
            },
            Err(_) => Self {
                sender: None,
                worker_exit: None,
                worker: None,
            },
        }
    }

    fn submit(&self, job: DiagnosticsJob) -> Result<(), ()> {
        self.sender
            .as_ref()
            .ok_or(())?
            .try_send(job)
            .map_err(|_| ())
    }
}

impl Drop for DiagnosticsExecutor {
    fn drop(&mut self) {
        self.sender.take();
        let acknowledged = self.worker_exit.take().is_some_and(|receiver| {
            receiver
                .into_inner()
                .unwrap_or_else(|error| error.into_inner())
                .recv_timeout(DIAGNOSTICS_WORKER_EXIT_TIMEOUT)
                .is_ok()
        });
        if acknowledged {
            let Some(worker) = self.worker.take() else {
                return;
            };
            let _ = worker.join();
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IdempotentResponse {
    status: u16,
    payload: String,
}

#[derive(Clone, Debug)]
enum IdempotencyState {
    Running,
    Completed(IdempotentResponse),
    Unreplayable,
    Failed,
}

struct IdempotencyEntry {
    request_digest: [u8; 32],
    updates: tokio::sync::watch::Sender<IdempotencyState>,
}

#[derive(Default)]
struct IdempotencyRegistryState {
    entries: HashMap<[u8; 32], IdempotencyEntry>,
    order: VecDeque<[u8; 32]>,
}

/// Process-local, bounded replay window for keyed native memory mutations. It prevents duplicate
/// effects while an entry is retained, but eviction or restart forgets old keys; this is not global
/// exactly-once.
struct IdempotencyRegistry {
    capacity: usize,
    state: std::sync::Mutex<IdempotencyRegistryState>,
}

enum IdempotencyDecision {
    Execute(IdempotencyLease),
    Wait(tokio::sync::watch::Receiver<IdempotencyState>),
    Replay(IdempotentResponse),
    Conflict,
    Full,
    Unreplayable,
}

struct IdempotencyLease {
    registry: Arc<IdempotencyRegistry>,
    key_digest: [u8; 32],
    request_digest: [u8; 32],
    finished: bool,
}

impl IdempotencyRegistry {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            state: std::sync::Mutex::new(IdempotencyRegistryState::default()),
        }
    }

    fn begin(
        self: &Arc<Self>,
        key_digest: [u8; 32],
        request_digest: [u8; 32],
    ) -> IdempotencyDecision {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = state.entries.get(&key_digest) {
            if entry.request_digest != request_digest {
                return IdempotencyDecision::Conflict;
            }
            return match entry.updates.borrow().clone() {
                IdempotencyState::Running => IdempotencyDecision::Wait(entry.updates.subscribe()),
                IdempotencyState::Completed(response) => IdempotencyDecision::Replay(response),
                IdempotencyState::Unreplayable => IdempotencyDecision::Unreplayable,
                IdempotencyState::Failed => IdempotencyDecision::Full,
            };
        }

        while state.entries.len() >= self.capacity {
            let Some(index) = state.order.iter().position(|key| {
                state.entries.get(key).is_some_and(|entry| {
                    matches!(
                        &*entry.updates.borrow(),
                        IdempotencyState::Completed(_) | IdempotencyState::Unreplayable
                    )
                })
            }) else {
                return IdempotencyDecision::Full;
            };
            if let Some(evicted) = state.order.remove(index) {
                state.entries.remove(&evicted);
            }
        }

        let (updates, _) = tokio::sync::watch::channel(IdempotencyState::Running);
        state.entries.insert(
            key_digest,
            IdempotencyEntry {
                request_digest,
                updates,
            },
        );
        state.order.push_back(key_digest);
        IdempotencyDecision::Execute(IdempotencyLease {
            registry: Arc::clone(self),
            key_digest,
            request_digest,
            finished: false,
        })
    }

    fn complete(
        &self,
        key_digest: [u8; 32],
        request_digest: [u8; 32],
        response: IdempotentResponse,
    ) {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(entry) = state.entries.get(&key_digest) {
            if entry.request_digest == request_digest {
                if response.payload.len() <= MAX_IDEMPOTENT_RESPONSE_BYTES {
                    entry
                        .updates
                        .send_replace(IdempotencyState::Completed(response));
                } else {
                    entry.updates.send_replace(IdempotencyState::Unreplayable);
                }
            }
        }
    }

    fn fail(&self, key_digest: [u8; 32], request_digest: [u8; 32]) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let matching = state
            .entries
            .get(&key_digest)
            .is_some_and(|entry| entry.request_digest == request_digest);
        if matching {
            if let Some(entry) = state.entries.remove(&key_digest) {
                entry.updates.send_replace(IdempotencyState::Failed);
            }
            state.order.retain(|key| *key != key_digest);
        }
    }
}

impl IdempotencyLease {
    fn complete(mut self, status: u16, payload: &str) {
        if status / 100 == 2 || (status / 100 == 4 && !matches!(status, 408 | 429)) {
            self.registry.complete(
                self.key_digest,
                self.request_digest,
                IdempotentResponse {
                    status,
                    payload: payload.to_string(),
                },
            );
        } else {
            self.registry.fail(self.key_digest, self.request_digest);
        }
        self.finished = true;
    }
}

impl Drop for IdempotencyLease {
    fn drop(&mut self) {
        if !self.finished {
            self.registry.fail(self.key_digest, self.request_digest);
        }
    }
}

struct WorkerAdmission {
    ingress: Arc<tokio::sync::Semaphore>,
    diagnostics: Arc<tokio::sync::Semaphore>,
    general: Arc<tokio::sync::Semaphore>,
    model_queue: Arc<tokio::sync::Semaphore>,
    model_execution: Arc<tokio::sync::Semaphore>,
    max_general: usize,
    max_model: usize,
    overload_rejections: std::sync::atomic::AtomicU64,
    ingress_rejections: std::sync::atomic::AtomicU64,
    diagnostics_rejections: std::sync::atomic::AtomicU64,
    model_rejections: std::sync::atomic::AtomicU64,
    general_rejections: std::sync::atomic::AtomicU64,
    detached_running: std::sync::atomic::AtomicU64,
}

impl WorkerAdmission {
    fn new(max_general: usize, max_model: usize) -> Self {
        Self {
            ingress: Arc::new(tokio::sync::Semaphore::new(max_general)),
            diagnostics: Arc::new(tokio::sync::Semaphore::new(1)),
            general: Arc::new(tokio::sync::Semaphore::new(max_general)),
            model_queue: Arc::new(tokio::sync::Semaphore::new(max_model)),
            model_execution: Arc::new(tokio::sync::Semaphore::new(MAX_MODEL_EXECUTION)),
            max_general,
            max_model,
            overload_rejections: std::sync::atomic::AtomicU64::new(0),
            ingress_rejections: std::sync::atomic::AtomicU64::new(0),
            diagnostics_rejections: std::sync::atomic::AtomicU64::new(0),
            model_rejections: std::sync::atomic::AtomicU64::new(0),
            general_rejections: std::sync::atomic::AtomicU64::new(0),
            detached_running: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn reject_overload(&self, status: StatusCode, kind: &str) -> Response {
        self.overload_rejections
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let lane_counter = if kind.starts_with("ingress") {
            &self.ingress_rejections
        } else if kind.starts_with("diagnostics") {
            &self.diagnostics_rejections
        } else if kind.starts_with("model") {
            &self.model_rejections
        } else {
            &self.general_rejections
        };
        lane_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        (
            status,
            [
                (header::CONTENT_TYPE, "application/json; charset=utf-8"),
                (header::RETRY_AFTER, "1"),
            ],
            serde_json::json!({ "error": "request capacity busy", "kind": kind }).to_string(),
        )
            .into_response()
    }

    fn snapshot(&self) -> Value {
        json!({
            "general": {
                "max": self.max_general,
                "available": self.general.available_permits(),
            },
            "ingress": {
                "max": self.max_general,
                "available": self.ingress.available_permits(),
            },
            "diagnostics": {
                "max": 1,
                "available": self.diagnostics.available_permits(),
            },
            "model": {
                "max": self.max_model,
                "available": self.model_queue.available_permits(),
                "executionMax": MAX_MODEL_EXECUTION,
                "executionAvailable": self.model_execution.available_permits(),
            },
            "overloadRejections": {
                "total": self.overload_rejections.load(std::sync::atomic::Ordering::Relaxed),
                "ingress": self.ingress_rejections.load(std::sync::atomic::Ordering::Relaxed),
                "diagnostics": self.diagnostics_rejections.load(std::sync::atomic::Ordering::Relaxed),
                "model": self.model_rejections.load(std::sync::atomic::Ordering::Relaxed),
                "general": self.general_rejections.load(std::sync::atomic::Ordering::Relaxed),
            },
            "detachedRunning": self
                .detached_running
                .load(std::sync::atomic::Ordering::Acquire),
        })
    }
}

struct WorkerLifecycle {
    // 0 = the HTTP waiter is attached, 1 = waiter timed out/dropped while work
    // remains live, 2 = blocking work finished.
    phase: std::sync::atomic::AtomicU8,
    workers: Arc<WorkerAdmission>,
}

struct WorkerWaiterGuard {
    lifecycle: Arc<WorkerLifecycle>,
}

impl Drop for WorkerWaiterGuard {
    fn drop(&mut self) {
        if self
            .lifecycle
            .phase
            .compare_exchange(
                0,
                1,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
        {
            self.lifecycle
                .workers
                .detached_running
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
    }
}

struct WorkerExecutionGuard {
    lifecycle: Arc<WorkerLifecycle>,
}

impl Drop for WorkerExecutionGuard {
    fn drop(&mut self) {
        if self
            .lifecycle
            .phase
            .swap(2, std::sync::atomic::Ordering::AcqRel)
            == 1
        {
            self.lifecycle
                .workers
                .detached_running
                .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        }
    }
}

/// MBR-306: the optional Streamable HTTP MCP transport (`crate::mcp_http`)
/// sources its bearer token from this exact credential path instead of minting
/// a parallel one. Creating the fallback file also applies the platform's
/// owner-only permissions when needed.
pub fn configured_api_token(db_path: &std::path::Path) -> Result<String, String> {
    let fallback = db_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("api-token");
    configured_api_token_from_sources(
        std::env::var_os("MEMBRANE_API_TOKEN"),
        std::env::var_os("MEMBRANE_API_TOKEN_FILE").map(std::path::PathBuf::from),
        &fallback,
    )
}

fn configured_api_token_from_sources(
    raw: Option<std::ffi::OsString>,
    configured_path: Option<std::path::PathBuf>,
    fallback: &std::path::Path,
) -> Result<String, String> {
    if let Some(raw) = raw {
        let token = raw.to_string_lossy().trim().to_string();
        if token.is_empty() {
            return Err("MEMBRANE_API_TOKEN is set but empty".to_string());
        }
        validate_api_token(&token)?;
        return Ok(token);
    }
    let path = configured_path.unwrap_or_else(|| fallback.to_path_buf());
    token_from_file_or_create(&path)
}

fn token_from_file_or_create(path: &std::path::Path) -> Result<String, String> {
    match read_token_file(path) {
        Ok(token) => return Ok(token),
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!(
                "read MEMBRANE_API_TOKEN_FILE {}: {error}",
                path.display()
            ));
        }
        Err(_) => {}
    }

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "create MEMBRANE_API_TOKEN_FILE directory {}: {error}",
                parent.display()
            )
        })?;
    }

    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|error| format!("generate Cortex API token: {error}"))?;
    let token = hex(&random);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cortex-token");
    let temp_path = path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        hex(&random[..6])
    ));

    let mut file = {
        #[cfg(windows)]
        {
            windows_create_owner_only_token_file(&temp_path).map_err(|error| {
                format!(
                    "create temporary Cortex API token with owner-only DACL {}: {error}",
                    temp_path.display()
                )
            })?
        }
        #[cfg(not(windows))]
        {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(&temp_path).map_err(|error| {
                format!(
                    "create temporary Cortex API token {}: {error}",
                    temp_path.display()
                )
            })?
        }
    };
    let publish = (|| -> Result<(), String> {
        use std::io::Write as _;
        file.write_all(token.as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("write Cortex API token: {error}"))?;
        std::fs::hard_link(&temp_path, path).map_err(|error| error.to_string())?;
        Ok(())
    })();
    drop(file);
    let _ = std::fs::remove_file(&temp_path);

    match publish {
        Ok(()) => Ok(token),
        Err(_) if path.exists() => read_token_file(path).map_err(|error| {
            format!(
                "read concurrently-created MEMBRANE_API_TOKEN_FILE {}: {error}",
                path.display()
            )
        }),
        Err(error) => Err(format!(
            "atomically publish MEMBRANE_API_TOKEN_FILE {}: {error}",
            path.display()
        )),
    }
}

#[cfg(windows)]
fn windows_create_owner_only_token_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::io::FromRawHandle as _;
    use std::ptr::null_mut;

    type Handle = *mut std::ffi::c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const CREATE_NEW: u32 = 1;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share_mode: u32,
            security: *mut std::ffi::c_void,
            creation: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = windows_owner_only_security(|attributes, _dacl| {
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_WRITE,
                0,
                attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(handle)
        }
    })?;
    // The security descriptor remains owned by `windows_owner_only_security`
    // until after CreateFileW has returned; the file stores its own SD.
    Ok(unsafe { std::fs::File::from_raw_handle(handle as _) })
}

#[cfg(windows)]
fn windows_harden_existing_token_file(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use std::ptr::null_mut;

    type Handle = *mut std::ffi::c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const READ_CONTROL: u32 = 0x0002_0000;
    const WRITE_DAC: u32 = 0x0004_0000;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    const FILE_SHARE_DELETE: u32 = 0x4;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    const SE_FILE_OBJECT: u32 = 1;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;
    const PROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x8000_0000;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share_mode: u32,
            security: *mut std::ffi::c_void,
            creation: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
    }
    #[link(name = "advapi32")]
    extern "system" {
        fn SetSecurityInfo(
            handle: Handle,
            object_type: u32,
            security_info: u32,
            owner: *mut std::ffi::c_void,
            group: *mut std::ffi::c_void,
            dacl: *mut std::ffi::c_void,
            sacl: *mut std::ffi::c_void,
        ) -> u32;
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    windows_owner_only_security(|_attributes, dacl| {
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                READ_CONTROL | WRITE_DAC,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null_mut(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let result = unsafe {
            SetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null_mut(),
            )
        };
        let close_result = unsafe { CloseHandle(handle) };
        if result != 0 {
            return Err(std::io::Error::from_raw_os_error(result as i32));
        }
        if close_result == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    })
}

#[cfg(windows)]
pub(crate) fn windows_owner_only_security<T>(
    operation: impl FnOnce(*mut std::ffi::c_void, *mut std::ffi::c_void) -> std::io::Result<T>,
) -> std::io::Result<T> {
    use std::mem::{size_of, MaybeUninit};
    use std::ptr::null_mut;

    #[repr(C)]
    struct SecurityDescriptor {
        revision: u8,
        sbz1: u8,
        control: u16,
        owner: *mut std::ffi::c_void,
        group: *mut std::ffi::c_void,
        sacl: *mut std::ffi::c_void,
        dacl: *mut std::ffi::c_void,
    }
    #[repr(C)]
    struct SecurityAttributes {
        length: u32,
        descriptor: *mut std::ffi::c_void,
        inherit_handle: i32,
    }
    type Handle = *mut std::ffi::c_void;
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_USER: u32 = 1;
    const SECURITY_DESCRIPTOR_REVISION: u32 = 1;
    const ACL_REVISION: u32 = 2;
    const GENERIC_ALL: u32 = 0x1000_0000;
    const SE_DACL_PROTECTED: u16 = 0x1000;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetLastError() -> u32;
    }
    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
        fn GetTokenInformation(
            token: Handle,
            information_class: u32,
            buffer: *mut std::ffi::c_void,
            buffer_length: u32,
            return_length: *mut u32,
        ) -> i32;
        fn GetLengthSid(sid: *mut std::ffi::c_void) -> u32;
        fn InitializeSecurityDescriptor(descriptor: *mut std::ffi::c_void, revision: u32) -> i32;
        fn SetSecurityDescriptorOwner(
            descriptor: *mut std::ffi::c_void,
            owner: *mut std::ffi::c_void,
            defaulted: i32,
        ) -> i32;
        fn InitializeAcl(acl: *mut std::ffi::c_void, length: u32, revision: u32) -> i32;
        fn AddAccessAllowedAceEx(
            acl: *mut std::ffi::c_void,
            revision: u32,
            ace_flags: u32,
            access_mask: u32,
            sid: *mut std::ffi::c_void,
        ) -> i32;
        fn SetSecurityDescriptorDacl(
            descriptor: *mut std::ffi::c_void,
            dacl_present: i32,
            dacl: *mut std::ffi::c_void,
            defaulted: i32,
        ) -> i32;
        fn SetSecurityDescriptorControl(
            descriptor: *mut std::ffi::c_void,
            bits_of_interest: u16,
            bits_to_set: u16,
        ) -> i32;
    }

    let mut token: Handle = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "open current-user token: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    let mut token_length = 0u32;
    unsafe {
        GetTokenInformation(token, TOKEN_USER, null_mut(), 0, &mut token_length);
    }
    if token_length == 0 {
        unsafe { CloseHandle(token) };
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("query current-user SID size: Windows error {}", unsafe {
                GetLastError()
            }),
        ));
    }
    let mut token_buffer = vec![0u64; (token_length as usize).div_ceil(size_of::<u64>())];
    let mut returned = 0u32;
    let token_ok = unsafe {
        GetTokenInformation(
            token,
            TOKEN_USER,
            token_buffer.as_mut_ptr() as *mut std::ffi::c_void,
            (token_buffer.len() * size_of::<u64>()) as u32,
            &mut returned,
        )
    } != 0;
    unsafe { CloseHandle(token) };
    if !token_ok {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "query current-user SID: {}",
                std::io::Error::last_os_error()
            ),
        ));
    }
    let token_user = token_buffer.as_ptr() as *const *mut std::ffi::c_void;
    let sid = unsafe { *token_user };
    let sid_length = unsafe { GetLengthSid(sid) };
    if sid_length == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "current-user token returned an invalid SID",
        ));
    }

    let acl_length =
        size_of::<u32>() * 2 + (size_of::<u32>() * 3 + sid_length as usize - size_of::<u32>());
    let mut acl_storage = vec![0u32; acl_length.div_ceil(size_of::<u32>())];
    let mut descriptor = MaybeUninit::<SecurityDescriptor>::uninit();
    let descriptor_ptr = descriptor.as_mut_ptr() as *mut std::ffi::c_void;
    let acl_ptr = acl_storage.as_mut_ptr() as *mut std::ffi::c_void;
    let check = |ok: i32, operation: &str| {
        if ok == 0 {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("{operation}: {}", std::io::Error::last_os_error()),
            ))
        } else {
            Ok(())
        }
    };
    check(
        unsafe { InitializeSecurityDescriptor(descriptor_ptr, SECURITY_DESCRIPTOR_REVISION) },
        "initialize owner-only security descriptor",
    )?;
    check(
        unsafe { SetSecurityDescriptorOwner(descriptor_ptr, sid, 0) },
        "set owner-only security descriptor owner",
    )?;
    check(
        unsafe { InitializeAcl(acl_ptr, acl_length as u32, ACL_REVISION) },
        "initialize owner-only DACL",
    )?;
    check(
        unsafe { AddAccessAllowedAceEx(acl_ptr, ACL_REVISION, 0, GENERIC_ALL, sid) },
        "add current-user owner-only ACE",
    )?;
    check(
        unsafe { SetSecurityDescriptorDacl(descriptor_ptr, 1, acl_ptr, 0) },
        "attach owner-only DACL",
    )?;
    check(
        unsafe {
            SetSecurityDescriptorControl(descriptor_ptr, SE_DACL_PROTECTED, SE_DACL_PROTECTED)
        },
        "disable DACL inheritance",
    )?;

    let mut attributes = SecurityAttributes {
        length: size_of::<SecurityAttributes>() as u32,
        descriptor: descriptor_ptr,
        inherit_handle: 0,
    };
    operation(
        &mut attributes as *mut SecurityAttributes as *mut std::ffi::c_void,
        acl_ptr,
    )
}

fn read_token_file(path: &std::path::Path) -> std::io::Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "token path must be a regular non-symlink file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600);
        std::fs::set_permissions(path, permissions)?;
    }
    #[cfg(windows)]
    windows_harden_existing_token_file(path)?;
    let token = std::fs::read_to_string(path)?.trim().to_string();
    if token.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "token file is empty",
        ));
    }
    validate_api_token(&token)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    Ok(token)
}

/// Validate a configured token file without exposing its value or repairing permissions. Support
/// diagnostics must stay read-only, unlike runtime startup which normalizes token permissions.
pub(crate) fn token_file_is_valid_without_mutation(path: &std::path::Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return false;
    }
    let Ok(token) = std::fs::read_to_string(path) else {
        return false;
    };
    !token.trim().is_empty() && validate_api_token(token.trim()).is_ok()
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn validate_api_token(token: &str) -> Result<(), String> {
    if token.contains(['\r', '\n']) {
        return Err("Cortex API token contains a newline".to_string());
    }
    Ok(())
}

fn secure_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn json_response(status: StatusCode, payload: String) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
        payload,
    )
        .into_response()
}

fn reject(status: StatusCode, message: &str) -> Response {
    json_response(status, serde_json::json!({ "error": message }).to_string())
}

/// Shared `/freshness` response assembly for both the async axum dispatcher and the sync
/// test-routed dispatcher below. F19 fix: `sessionId` and `worktreePath` are the caller's
/// declared identity on the source-barrier receipt. Silently defaulting either one
/// (`"freshness-http"` / the raw `repoRoot`) let every caller that omitted its identity collapse
/// onto the same receipt identity, which defeats the receipt's purpose — both are now required,
/// with no fallback value invented on the caller's behalf.
fn freshness_response_body(
    v: &Value,
    verdict: &crate::freshness::FreshnessVerdict,
    repo_root: &std::path::Path,
) -> (u16, String) {
    let Some(session_id) = v
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return (
            400,
            serde_json::json!({ "error": "sessionId required" }).to_string(),
        );
    };
    let Some(worktree_path) = v
        .get("worktreePath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return (
            400,
            serde_json::json!({ "error": "worktreePath required" }).to_string(),
        );
    };
    let requested_worktree = match crate::freshness::canonical_repo_root(
        std::path::Path::new(worktree_path),
        &configured_workspace_root(),
    ) {
        Ok(root) => root,
        Err(_) => {
            return (
                400,
                serde_json::json!({ "error": "worktreePath invalid" }).to_string(),
            )
        }
    };
    if requested_worktree != repo_root {
        return (
            409,
            serde_json::json!({ "error": "worktreePath does not match repoRoot" }).to_string(),
        );
    }
    let repository_id = v
        .get("repositoryId")
        .and_then(Value::as_str)
        .unwrap_or("workspace-root");
    let mut payload_value = match serde_json::to_value(verdict) {
        Ok(value) => value,
        Err(_) => {
            return (
                500,
                serde_json::json!({ "error": "freshness serialization failed" }).to_string(),
            )
        }
    };
    payload_value["sourceBarrierReceipt"] = crate::freshness::source_barrier_receipt(
        verdict,
        repository_id,
        session_id,
        &repo_root.to_string_lossy(),
    );
    match serde_json::to_string(&payload_value) {
        Ok(payload) => (200, payload),
        Err(_) => (
            500,
            serde_json::json!({ "error": "freshness serialization failed" }).to_string(),
        ),
    }
}

/// Which origin scope an observable-event read runs under. Chosen by the route the caller hit,
/// never by a field in the request body — Taste must not be able to widen itself to
/// assistant/model/tool origin simply by asking for it, which is why the two routes exist
/// separately instead of one route with a scope parameter.
enum ObservableQueryRoute {
    Taste,
    Insights,
    ForgeTimeAccounting,
}

fn observable_query_response(
    store: &MemoryStore,
    body: &str,
    route: ObservableQueryRoute,
) -> (u16, String) {
    let Ok(request) = serde_json::from_str::<Value>(body) else {
        return (
            400,
            serde_json::json!({ "error": "invalid observable event query" }).to_string(),
        );
    };
    // `limit` is required with no default: a caller that omits it would otherwise silently receive
    // whatever bound we picked and read a partial ledger as though it were the whole one.
    let Some(limit) = request.get("limit").and_then(Value::as_u64) else {
        return (
            400,
            serde_json::json!({ "error": "limit required" }).to_string(),
        );
    };
    let text = |key: &str| {
        request
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let filter = crate::context_telemetry::ObservableEventQuery {
        since: text("since"),
        until: text("until"),
        event_type: text("eventType"),
        session_id: text("sessionId"),
        task_id: text("taskId"),
        trace_id: text("traceId"),
        installation_id: text("installationId"),
        after_sequence: request.get("afterSequence").and_then(Value::as_i64),
        limit: limit as usize,
    };
    let queried = match route {
        ObservableQueryRoute::Taste => store.db().query_observable_events_for_taste(&filter),
        ObservableQueryRoute::Insights => store.db().query_observable_events_for_insights(&filter),
        ObservableQueryRoute::ForgeTimeAccounting => {
            store.db().query_observable_events_for_forge(&filter)
        }
    };
    match queried {
        Ok(result) => match serde_json::to_string(&result) {
            Ok(payload) => (200, payload),
            Err(_) => (
                500,
                serde_json::json!({ "error": "observable query serialization failed" }).to_string(),
            ),
        },
        Err(crate::context_telemetry::ContextTelemetryError::Invalid(error)) => {
            (400, serde_json::json!({ "error": error }).to_string())
        }
        Err(_) => (
            500,
            serde_json::json!({ "error": "observable event storage unavailable" }).to_string(),
        ),
    }
}

fn origin_allowed(headers: &HeaderMap, allowed_origins: &[String]) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    allowed_origins.iter().any(|allowed| allowed == origin)
}

fn authorized(headers: &HeaderMap, expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return false;
    };
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some(actual) = value.strip_prefix("Bearer ") else {
        return false;
    };
    secure_eq(actual.as_bytes(), expected.as_bytes())
}

/// Validate CodeRight's native binding fence before dispatching any resident
/// operation. Requests without this opt-in fence remain compatible with the
/// public API; fenced requests must name this exact Hub installation, store,
/// and release generation.
fn native_identity_fence_valid(
    headers: &HeaderMap,
    store: &MemoryStore,
) -> Result<(), &'static str> {
    let names = [
        "x-membrane-installation-id",
        "x-membrane-cortex-store-id",
        "x-membrane-release-generation",
    ];
    let values = names.map(|name| headers.get(name).and_then(|value| value.to_str().ok()));
    if values.iter().all(Option::is_none) {
        return Ok(());
    }
    let expected = [
        store.installation_id().to_string(),
        store.cortex_store_id(),
        crate::release_identity::release_generation(),
    ];
    if values
        .iter()
        .zip(expected.iter())
        .all(|(actual, expected)| actual.is_some_and(|value| value == expected.as_str()))
    {
        Ok(())
    } else {
        Err("native Membrane identity fence mismatch")
    }
}

fn membrane_capability_authorized(headers: &HeaderMap) -> bool {
    crate::service::lifecycle_control().snapshot_authorized(
        headers
            .get("x-membrane-capability")
            .and_then(|value| value.to_str().ok()),
    )
}

fn is_public_path(path: &str) -> bool {
    matches!(path, "/" | "/index.html" | "/health" | "/livez")
}

fn is_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HttpWorkClass {
    General,
    Model,
}

type HttpRouteSpec = (&'static str, &'static str, HttpWorkClass);

const HTTP_ROUTE_SPECS: &[HttpRouteSpec] = &[
    (
        "POST",
        crate::adapt_service::OPERATOR_PATH,
        HttpWorkClass::General,
    ),
    (
        "POST",
        crate::adapt_service::OBSERVATION_PATH,
        HttpWorkClass::General,
    ),
    ("GET", "/", HttpWorkClass::General),
    ("GET", "/index.html", HttpWorkClass::General),
    ("GET", "/metrics", HttpWorkClass::General),
    ("GET", "/activity", HttpWorkClass::General),
    ("POST", "/activity", HttpWorkClass::General),
    ("GET", "/graph", HttpWorkClass::General),
    ("GET", "/analysis", HttpWorkClass::General),
    ("GET", "/health", HttpWorkClass::General),
    ("GET", "/livez", HttpWorkClass::General),
    ("POST", "/freshness", HttpWorkClass::General),
    ("POST", "/anchor/retrieve", HttpWorkClass::General),
    ("POST", "/expand", HttpWorkClass::General),
    ("POST", "/skills-snapshot", HttpWorkClass::General),
    ("POST", "/v1/telemetry/events:batch", HttpWorkClass::General),
    (
        "POST",
        "/v1/telemetry/observable-events:batch",
        HttpWorkClass::General,
    ),
    (
        "POST",
        "/v1/telemetry/observable-events:query-taste",
        HttpWorkClass::General,
    ),
    (
        "POST",
        "/v1/telemetry/observable-events:query-insights",
        HttpWorkClass::General,
    ),
    (
        "POST",
        "/v1/telemetry/observable-events:query-forge-time",
        HttpWorkClass::General,
    ),
    ("POST", "/v1/memories:batch", HttpWorkClass::Model),
    ("POST", "/memory-lifecycle", HttpWorkClass::General),
    ("POST", "/scratchpad", HttpWorkClass::General),
    ("POST", "/scratchpad/session-close", HttpWorkClass::General),
    ("POST", "/put", HttpWorkClass::Model),
    ("POST", "/remember", HttpWorkClass::Model),
    ("POST", "/remember_consolidated", HttpWorkClass::Model),
    ("POST", "/delete", HttpWorkClass::General),
    ("POST", "/list", HttpWorkClass::General),
    ("POST", "/scopes", HttpWorkClass::General),
    ("POST", "/get", HttpWorkClass::General),
    ("POST", "/recall", HttpWorkClass::Model),
    ("POST", "/search", HttpWorkClass::Model),
    ("POST", "/policy/assign", HttpWorkClass::General),
    ("POST", "/curate", HttpWorkClass::Model),
    ("POST", "/quarantine/list", HttpWorkClass::General),
    ("POST", "/quarantine/restore", HttpWorkClass::General),
    ("POST", "/add", HttpWorkClass::General),
    ("POST", "/use", HttpWorkClass::General),
    ("POST", "/feedback", HttpWorkClass::General),
    ("POST", "/context/close-unknown", HttpWorkClass::General),
    ("POST", "/memory-candidates", HttpWorkClass::Model),
    ("POST", "/federate", HttpWorkClass::General),
    ("POST", "/delivery/trace", HttpWorkClass::General),
    ("GET", "/hub/capabilities", HttpWorkClass::General),
    ("GET", "/hub/snapshot", HttpWorkClass::General),
    ("GET", "/snapshot", HttpWorkClass::General),
    ("POST", "/verify-memory", HttpWorkClass::General),
    ("POST", "/compress", HttpWorkClass::Model),
    ("POST", "/scope_grants", HttpWorkClass::General),
    ("POST", "/plan_context", HttpWorkClass::General),
];

fn http_route_spec(method: &Method, path: &str) -> Option<HttpRouteSpec> {
    HTTP_ROUTE_SPECS
        .iter()
        .copied()
        .find(|spec| spec.0 == method.as_str() && spec.1 == path)
}

fn is_model_bound(method: &Method, path: &str) -> bool {
    if cfg!(test) && path == "/__test/slow" {
        return true;
    }
    http_route_spec(method, path).is_some_and(|spec| spec.2 == HttpWorkClass::Model)
}

/// Host fence enforcement for CodeRight / Claude Code / Codex.
/// Before tests / builds / releases, hosts must query the diagnostics fence.
/// This helper is the server-side enforcement point: if the fence is dirty
/// (no cleared decision for the current sealed epoch), the host must block
/// the operation. The check is fail-closed: unknown workspaces return true
/// (block). Hosts enforce; providers only report evidence (design §10).
/// Integration: CodeRight dispatcher calls `diagnostics.status` or
/// `fence_allows_build` before spawning `cargo test` / `npm test` /
/// `build` and returns 409 with `fence_not_cleared` if blocked.
/// Claude/Codex observed-hook mode calls the same check before
/// `reconciliation_only` gates.
pub fn host_fence_blocks_build(
    diagnostics: Option<&crate::live_diagnostics_service::DiagnosticsService>,
    repo_id: &str,
    worktree_id: &str,
) -> bool {
    match diagnostics {
        Some(service) => !service.is_fence_cleared(repo_id, worktree_id),
        None => false,
    }
}

fn valid_idempotency_key(key: &str) -> bool {
    (1..=128).contains(&key.len()) && key.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

fn digest_framed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn idempotency_key_digest(key: &str, api_token: Option<&str>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"cortex-idempotency-key-v1");
    digest_framed(
        &mut hasher,
        api_token.unwrap_or("loopback-without-api-token").as_bytes(),
    );
    digest_framed(&mut hasher, key.as_bytes());
    hasher.finalize().into()
}

fn idempotency_request_digest(method: &Method, path_and_query: &str, body: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"cortex-idempotency-request-v1");
    digest_framed(&mut hasher, method.as_str().as_bytes());
    digest_framed(&mut hasher, path_and_query.as_bytes());
    digest_framed(&mut hasher, body.as_bytes());
    hasher.finalize().into()
}

fn retryable_reject(status: StatusCode, message: &str, kind: &str) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::RETRY_AFTER, "1"),
        ],
        json!({ "error": message, "kind": kind }).to_string(),
    )
        .into_response()
}

fn replay_idempotent_response(response: IdempotentResponse) -> Response {
    json_response(
        StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        response.payload,
    )
}

async fn wait_for_idempotent_response(
    mut receiver: tokio::sync::watch::Receiver<IdempotencyState>,
) -> Response {
    loop {
        let state = receiver.borrow().clone();
        match state {
            IdempotencyState::Running => {}
            IdempotencyState::Completed(response) => return replay_idempotent_response(response),
            IdempotencyState::Unreplayable => {
                return retryable_reject(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "idempotent result exceeded replay capacity",
                    "idempotency_result_unreplayable",
                )
            }
            IdempotencyState::Failed => {
                return retryable_reject(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "idempotent operation failed before completion",
                    "idempotency_execution_failed",
                )
            }
        }
        if receiver.changed().await.is_err() {
            return retryable_reject(
                StatusCode::SERVICE_UNAVAILABLE,
                "idempotent operation became unavailable",
                "idempotency_registry_unavailable",
            );
        }
    }
}

async fn dispatch(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    if !origin_allowed(&headers, &state.allowed_origins) {
        return reject(StatusCode::FORBIDDEN, "cross-origin request rejected");
    }
    let path = uri.path();
    if method == Method::GET && path == "/snapshot" {
        if !membrane_capability_authorized(&headers) {
            return reject(
                StatusCode::UNAUTHORIZED,
                "valid Membrane capability required",
            );
        }
        return match membrane_snapshot_v2() {
            Ok(value) => json_response(StatusCode::OK, value.to_string()),
            Err(reason) => reject(StatusCode::SERVICE_UNAVAILABLE, &reason),
        };
    }
    if !crate::service::lifecycle_control().admission_open() {
        return retryable_reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "resident is draining",
            "lifecycle_draining",
        );
    }
    if !is_public_path(path) && !authorized(&headers, state.api_token.as_deref()) {
        return reject(StatusCode::UNAUTHORIZED, "valid bearer token required");
    }
    if !is_public_path(path) {
        if let Err(detail) = native_identity_fence_valid(&headers, &state.store) {
            return reject(StatusCode::CONFLICT, detail);
        }
    }
    if matches!(
        path,
        crate::adapt_service::OPERATOR_PATH | crate::adapt_service::OBSERVATION_PATH
    ) {
        let complete = [
            "x-membrane-installation-id",
            "x-membrane-cortex-store-id",
            "x-membrane-release-generation",
        ]
        .iter()
        .all(|key| headers.get(*key).is_some());
        let session_matches = headers
            .get("x-membrane-session")
            .and_then(|v| v.to_str().ok())
            == Some(state.store.service_instance_id());
        if !complete || !session_matches {
            return reject(
                StatusCode::CONFLICT,
                "exact Adapt daemon identity fence required",
            );
        }
    }
    if method == Method::POST && !is_json_content_type(&headers) {
        if record_http_rejection(&state.store, path, "failed", "invalid_content_type").is_err() {
            return reject(
                StatusCode::INTERNAL_SERVER_ERROR,
                "external lifecycle accounting unavailable",
            );
        }
        return reject(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Content-Type must be application/json",
        );
    }
    if method == Method::GET && matches!(path, "/" | "/index.html") {
        let html = DASHBOARD_HTML.replace("__MEMBRANE_API_TOKEN_JSON__", "null");
        return (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-store"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::X_FRAME_OPTIONS, "DENY"),
                (header::REFERRER_POLICY, "no-referrer"),
                (
                    header::CONTENT_SECURITY_POLICY,
                    "default-src 'none'; connect-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
                ),
            ],
            Html(html),
        )
            .into_response();
    }
    let body = match body {
        Ok(body) => body,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            if record_http_rejection(&state.store, path, "failed", "request_too_large").is_err() {
                return reject(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "external lifecycle accounting unavailable",
                );
            }
            return reject(StatusCode::PAYLOAD_TOO_LARGE, "request body too large");
        }
        Err(_) => {
            if record_http_rejection(&state.store, path, "failed", "invalid_request_body").is_err()
            {
                return reject(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "external lifecycle accounting unavailable",
                );
            }
            return reject(StatusCode::BAD_REQUEST, "invalid request body");
        }
    };

    let route_spec = http_route_spec(&method, path);
    let test_route = cfg!(test) && path.starts_with("/__test/");
    if route_spec.is_none() && !test_route {
        return reject(StatusCode::NOT_FOUND, "unknown");
    }
    let model_bound = is_model_bound(&method, path);
    let store = Arc::clone(&state.store);
    let context_ingest_lease = state.context_ingest_lease.clone();
    let catalog = state.catalog.clone();
    let planner_latency = Arc::clone(&state.planner_latency);
    let planner_last_fallback = Arc::clone(&state.planner_last_fallback);
    let planner_schema_error_count = Arc::clone(&state.planner_schema_error_count);
    #[cfg(test)]
    let test_control = Arc::clone(&state.test_control);
    let validate_model_json = model_bound && method == Method::POST;
    let body = match String::from_utf8(body.to_vec()) {
        Ok(body) => body,
        Err(_) => {
            if record_http_rejection(&state.store, path, "failed", "invalid_utf8").is_err() {
                return reject(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "external lifecycle accounting unavailable",
                );
            }
            return reject(StatusCode::BAD_REQUEST, "request body must be UTF-8");
        }
    };
    if validate_model_json && serde_json::from_str::<Value>(&body).is_err() {
        if record_http_rejection(&state.store, path, "failed", "malformed_json").is_err() {
            return reject(
                StatusCode::INTERNAL_SERVER_ERROR,
                "external lifecycle accounting unavailable",
            );
        }
        return reject(StatusCode::BAD_REQUEST, "invalid JSON body");
    }
    if method == Method::POST && path == "/freshness" {
        let value: Value = match serde_json::from_str(&body) {
            Ok(value) => value,
            Err(_) => return reject(StatusCode::BAD_REQUEST, "invalid JSON body"),
        };
        let Some(requested) = value
            .get("repoRoot")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        else {
            return reject(StatusCode::BAD_REQUEST, "repoRoot required");
        };
        let repo_root = match crate::freshness::canonical_repo_root(
            std::path::Path::new(requested),
            &configured_workspace_root(),
        ) {
            Ok(root) => root,
            Err(error) if error == "repoRoot is outside the configured workspace" => {
                return reject(StatusCode::FORBIDDEN, &error)
            }
            Err(error) => return reject(StatusCode::BAD_REQUEST, &error),
        };
        let verdict = state
            .freshness
            .latest_or_schedule(state.store.as_ref().clone(), repo_root.clone());
        let (status, payload) = freshness_response_body(&value, &verdict, &repo_root);
        return json_response(
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            payload,
        );
    }
    let mut idempotency_lease = None;
    if let Some(raw_key) = headers.get(IDEMPOTENCY_KEY_HEADER) {
        let key = match raw_key.to_str() {
            Ok(key) if valid_idempotency_key(key) => key,
            _ => {
                if record_http_rejection(&state.store, path, "failed", "invalid_idempotency_key")
                    .is_err()
                {
                    return reject(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "external lifecycle accounting unavailable",
                    );
                }
                return reject(
                    StatusCode::BAD_REQUEST,
                    "Idempotency-Key must be 1..128 visible ASCII characters",
                );
            }
        };
        if method != Method::POST
            || !matches!(
                path,
                "/put" | "/remember" | "/remember_consolidated" | "/delete" | "/use"
            )
        {
            if record_http_rejection(&state.store, path, "failed", "unsupported_idempotency_key")
                .is_err()
            {
                return reject(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "external lifecycle accounting unavailable",
                );
            }
            return reject(
                StatusCode::BAD_REQUEST,
                "Idempotency-Key is not supported for this route",
            );
        }
        let key_digest = idempotency_key_digest(key, state.api_token.as_deref());
        let request_digest = idempotency_request_digest(
            &method,
            uri.path_and_query()
                .map(|path| path.as_str())
                .unwrap_or(path),
            &body,
        );
        match state.idempotency.begin(key_digest, request_digest) {
            IdempotencyDecision::Execute(lease) => idempotency_lease = Some(lease),
            IdempotencyDecision::Wait(receiver) => {
                if record_http_late_terminal(&state.store, path, "skipped", "idempotent_duplicate")
                    .is_err()
                {
                    return reject(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "external lifecycle accounting unavailable",
                    );
                }
                return wait_for_idempotent_response(receiver).await;
            }
            IdempotencyDecision::Replay(response) => {
                if record_http_late_terminal(&state.store, path, "skipped", "idempotent_duplicate")
                    .is_err()
                {
                    return reject(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "external lifecycle accounting unavailable",
                    );
                }
                return replay_idempotent_response(response);
            }
            IdempotencyDecision::Conflict => {
                if record_http_rejection(&state.store, path, "failed", "idempotency_conflict")
                    .is_err()
                {
                    return reject(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "external lifecycle accounting unavailable",
                    );
                }
                return reject(
                    StatusCode::CONFLICT,
                    "Idempotency-Key was already used for a different request",
                );
            }
            IdempotencyDecision::Full => {
                if record_http_late_terminal(
                    &state.store,
                    path,
                    "unavailable",
                    "idempotency_registry_full",
                )
                .is_err()
                {
                    return reject(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "external lifecycle accounting unavailable",
                    );
                }
                return retryable_reject(
                    StatusCode::TOO_MANY_REQUESTS,
                    "idempotency registry is full",
                    "idempotency_registry_full",
                );
            }
            IdempotencyDecision::Unreplayable => {
                if record_http_late_terminal(
                    &state.store,
                    path,
                    "unavailable",
                    "idempotency_result_unreplayable",
                )
                .is_err()
                {
                    return reject(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "external lifecycle accounting unavailable",
                    );
                }
                return retryable_reject(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "idempotent result exceeded replay capacity",
                    "idempotency_result_unreplayable",
                );
            }
        }
    }
    let method = method.to_string();
    let url = uri.to_string();
    // Timeout drops the async waiter, not blocking SQLite/embedding work already running. A valid
    // Idempotency-Key makes every native memory mutation retryable; keyless mutations retain their
    // legacy at-least-once, ambiguous-after-timeout semantics.
    // Acquire the scarcer model lane first. FastEmbedder serializes internally;
    // rejecting here prevents 31 Tokio blocking threads from piling up on its
    // mutex after a native inference stalls.
    let model_queue_permit = if model_bound {
        match Arc::clone(&state.workers.model_queue).try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                if record_http_late_terminal(&state.store, path, "unavailable", "model_busy")
                    .is_err()
                {
                    return reject(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "external lifecycle accounting unavailable",
                    );
                }
                return state
                    .workers
                    .reject_overload(StatusCode::TOO_MANY_REQUESTS, "model_busy");
            }
        }
    } else {
        None
    };
    // Queue model-bound bursts asynchronously. Only the request currently
    // executing inference reaches Tokio's blocking pool; cancelled waiters
    // release their queue slots without leaving native work behind.
    let model_execution_permit = if model_bound {
        match Arc::clone(&state.workers.model_execution)
            .acquire_owned()
            .await
        {
            Ok(permit) => Some(permit),
            Err(_) => {
                if record_http_late_terminal(&state.store, path, "unavailable", "model_lane_closed")
                    .is_err()
                {
                    return reject(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "external lifecycle accounting unavailable",
                    );
                }
                return state
                    .workers
                    .reject_overload(StatusCode::SERVICE_UNAVAILABLE, "model_lane_closed");
            }
        }
    } else {
        None
    };
    let worker_permit = match Arc::clone(&state.workers.general).try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            if record_http_late_terminal(&state.store, path, "unavailable", "general_workers_busy")
                .is_err()
            {
                return reject(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "external lifecycle accounting unavailable",
                );
            }
            return state
                .workers
                .reject_overload(StatusCode::SERVICE_UNAVAILABLE, "general_workers_busy");
        }
    };
    let lifecycle = Arc::new(WorkerLifecycle {
        phase: std::sync::atomic::AtomicU8::new(0),
        workers: Arc::clone(&state.workers),
    });
    let _waiter_guard = WorkerWaiterGuard {
        lifecycle: Arc::clone(&lifecycle),
    };
    let accounting_store = Arc::clone(&state.store);
    let accounting_path = path.to_string();
    match tokio::task::spawn_blocking(move || {
        let _execution_guard = WorkerExecutionGuard { lifecycle };
        let _model_queue_permit = model_queue_permit;
        let _model_execution_permit = model_execution_permit;
        let _worker_permit = worker_permit;
        #[cfg(test)]
        if matches!(route_path(&url), "/__test/slow" | "/__test/slow-general") {
            test_control.workload.enter();
        }
        let result = route_full(
            &store,
            catalog.as_deref(),
            context_ingest_lease.as_deref(),
            &planner_latency,
            &planner_last_fallback,
            &planner_schema_error_count,
            &method,
            &url,
            &body,
        );
        if let Some(lease) = idempotency_lease {
            lease.complete(result.0, &result.1);
        }
        result
    })
    .await
    {
        Ok((status, payload)) => json_response(
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            payload,
        ),
        Err(_) => {
            if record_http_late_terminal(
                &accounting_store,
                &accounting_path,
                "unavailable",
                "request_worker_failed",
            )
            .is_err()
            {
                return reject(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "external lifecycle accounting unavailable",
                );
            }
            reject(StatusCode::INTERNAL_SERVER_ERROR, "request worker failed")
        }
    }
}

async fn workload_ingress(
    State(workers): State<Arc<WorkerAdmission>>,
    request: Request,
    next: Next,
) -> Response {
    let permit = match Arc::clone(&workers.ingress).try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return workers.reject_overload(StatusCode::TOO_MANY_REQUESTS, "ingress_busy"),
    };
    let response = next.run(request).await;
    drop(permit);
    response
}

/// MBR-105: every IPC handshake that carries an `X-Membrane-Manifest` header
/// is compared to the active manifest. A request whose manifest is missing
/// (legacy client) passes through unchanged; a request with a present but
/// mismatched manifest is rejected with a typed error response so the
/// peer knows which invariant failed. The check runs after the workload
/// ingress so a rejected handshake still consumes a permit (matching the
/// behavior of any other auth-style middleware).
async fn handshake_ingress(request: Request, next: Next) -> Response {
    let header_value = request
        .headers()
        .get(crate::installation_manifest::HANDSHAKE_HEADER)
        .and_then(|value| value.to_str().ok());
    match crate::installation_manifest::ParsedManifest::parse_header(header_value) {
        crate::installation_manifest::ParsedManifest::Absent => next.run(request).await,
        crate::installation_manifest::ParsedManifest::Invalid(reason) => reject(
            StatusCode::BAD_REQUEST,
            &format!("invalid X-Membrane-Manifest header: {reason}"),
        ),
        crate::installation_manifest::ParsedManifest::Present(observed) => {
            match crate::installation_manifest::verify_handshake(&observed) {
                Ok(()) => next.run(request).await,
                Err(error) => reject(
                    StatusCode::MISDIRECTED_REQUEST,
                    &format!("IPC handshake rejected: {}", error.label()),
                ),
            }
        }
    }
}

async fn livez(State(state): State<AppState>) -> Response {
    json_response(
        StatusCode::OK,
        json!({
            "ok": true,
            "workers": state.workers.snapshot(),
            "serviceGeneration": crate::release_identity::service_generation(),
            "releaseGeneration": crate::release_identity::release_generation(),
            "runtimeOrigin": runtime_origin(),
        })
        .to_string(),
    )
}

async fn detailed_health(State(state): State<AppState>, _uri: Uri) -> Response {
    #[cfg(test)]
    let test_slow = _uri
        .query()
        .is_some_and(|query| query.split('&').any(|part| part == "__test_slow=1"));
    #[cfg(test)]
    let test_control = Arc::clone(&state.test_control);
    let diagnostics_permit = match Arc::clone(&state.workers.diagnostics).try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return state
                .workers
                .reject_overload(StatusCode::TOO_MANY_REQUESTS, "diagnostics_busy")
        }
    };
    let store = Arc::clone(&state.store);
    let catalog = state.catalog.clone();
    let planner_latency = Arc::clone(&state.planner_latency);
    let planner_last_fallback = Arc::clone(&state.planner_last_fallback);
    let planner_schema_error_count = Arc::clone(&state.planner_schema_error_count);
    let workers = Arc::clone(&state.workers);
    let workers_for_job = Arc::clone(&workers);
    let (result_sender, result_receiver) = tokio::sync::oneshot::channel();
    let job = Box::new(move || {
        let _diagnostics_permit = diagnostics_permit;
        #[cfg(test)]
        if test_slow {
            test_control.diagnostics.enter();
        }
        let result = health_response_with_workers(
            &store,
            catalog.as_deref(),
            &planner_latency,
            &planner_last_fallback,
            &planner_schema_error_count,
            Some(&workers_for_job),
        );
        let _ = result_sender.send(result);
    });
    if state.diagnostics_executor.submit(job).is_err() {
        return workers.reject_overload(
            StatusCode::SERVICE_UNAVAILABLE,
            "diagnostics_executor_unavailable",
        );
    }
    match tokio::time::timeout(DETAILED_HEALTH_TIMEOUT, result_receiver).await {
        Ok(Ok((status, payload))) => json_response(
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            payload,
        ),
        Ok(Err(_)) => workers.reject_overload(
            StatusCode::SERVICE_UNAVAILABLE,
            "diagnostics_executor_unavailable",
        ),
        Err(_) => workers.reject_overload(StatusCode::SERVICE_UNAVAILABLE, "diagnostics_timeout"),
    }
}

fn build_router(
    store: MemoryStore,
    catalog: Option<ContextCatalog>,
    context_ingest_lease: Option<crate::context_telemetry::ContextIngestLease>,
    port: u16,
    api_token: Option<String>,
    request_timeout: Duration,
    max_concurrent_requests: usize,
) -> Router {
    #[cfg(test)]
    {
        build_router_inner(
            store,
            catalog,
            context_ingest_lease,
            port,
            api_token,
            request_timeout,
            max_concurrent_requests,
            Arc::new(TestControl::default()),
        )
    }
    #[cfg(not(test))]
    build_router_inner(
        store,
        catalog,
        context_ingest_lease,
        port,
        api_token,
        request_timeout,
        max_concurrent_requests,
    )
}

fn build_router_inner(
    store: MemoryStore,
    catalog: Option<ContextCatalog>,
    context_ingest_lease: Option<crate::context_telemetry::ContextIngestLease>,
    port: u16,
    api_token: Option<String>,
    request_timeout: Duration,
    max_concurrent_requests: usize,
    #[cfg(test)] test_control: Arc<TestControl>,
) -> Router {
    let state = AppState {
        store: Arc::new(store),
        context_ingest_lease: context_ingest_lease.map(Arc::new),
        freshness: Arc::new(crate::freshness::FreshnessCoordinator::default()),
        catalog: catalog.map(Arc::new),
        api_token: api_token.clone().map(Arc::<str>::from),
        allowed_origins: Arc::from([
            format!("http://127.0.0.1:{port}"),
            format!("http://localhost:{port}"),
        ]),
        planner_latency: Arc::new(crate::pull::metrics::PlannerLatency::new()),
        planner_last_fallback: Arc::new(crate::pull::metrics::LastFallback::new()),
        planner_schema_error_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        workers: Arc::new(WorkerAdmission::new(
            max_concurrent_requests,
            max_concurrent_requests.min(MAX_MODEL_QUEUE_REQUESTS),
        )),
        diagnostics_executor: Arc::new(DiagnosticsExecutor::new()),
        idempotency: Arc::new(IdempotencyRegistry::new(IDEMPOTENCY_REGISTRY_CAPACITY)),
        #[cfg(test)]
        test_control,
    };
    let workload = Router::new()
        .fallback(any(dispatch))
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(MAX_PUSH_BODY_BYTES))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state.workers),
            workload_ingress,
        ));
    let diagnostics_health_identity = json!({
        "serviceId": "membrane-hub",
        "installationId": state.store.installation_id(),
        "cortexStoreId": state.store.cortex_store_id(),
        "releaseGeneration": crate::release_identity::release_generation(),
        "runtimeOrigin": runtime_origin(),
        "serviceGeneration": crate::release_identity::service_generation(),
        "protocolVersion": 1,
        "schemaVersion": 1,
        "nativeOnly": true,
        "subsystems": ["pull", "push", "cortex", "blueprint", "ledger", "adapt"],
        "capabilities": ["memory", "diagnostics"],
    });
    let app = Router::new()
        .route("/livez", get(livez))
        .route("/health", get(detailed_health))
        .with_state(state)
        .merge(workload);
    // Live Diagnostics operational surface (design §12). Explicit routes
    // bypass the `dispatch` fallback where every other non-public route
    // authenticates, so the merged router carries its own bearer gate fed
    // from the same API token. Construction cannot fail under the default
    // configuration; if it ever does, the surface stays absent and the
    // fallback answers 404 instead of half-serving it.
    let app = match crate::live_diagnostics_service::resident_diagnostics_routes(
        api_token,
        diagnostics_health_identity,
    ) {
        Some(diagnostics) => app.merge(diagnostics),
        None => app,
    };
    // The handshake gate applies to every route, including the liveness
    // probes, so a garbage X-Membrane-Manifest header is rejected with
    // 400 before the request reaches the handler.
    app.layer(axum::middleware::from_fn(handshake_ingress))
}

#[cfg(test)]
fn router_for_tests_with_control(
    store: MemoryStore,
    request_timeout: Duration,
    max_concurrent_requests: usize,
) -> (Router, Arc<TestControl>) {
    let control = Arc::new(TestControl::default());
    let router = build_router_inner(
        store,
        None,
        None,
        8765,
        Some(TEST_API_TOKEN.to_string()),
        request_timeout,
        max_concurrent_requests,
        Arc::clone(&control),
    );
    (
        router.layer(axum::middleware::from_fn(test_authorization)),
        control,
    )
}

#[cfg(test)]
fn router_for_tests_with_policy(
    store: MemoryStore,
    port: u16,
    api_token: Option<String>,
    request_timeout: Duration,
    max_concurrent_requests: usize,
) -> Router {
    build_router(
        store,
        None,
        None,
        port,
        api_token,
        request_timeout,
        max_concurrent_requests,
    )
}

#[cfg(test)]
const TEST_API_TOKEN: &str = "test-api-token";

#[cfg(test)]
async fn test_authorization(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    request.headers_mut().insert(
        header::AUTHORIZATION,
        header::HeaderValue::from_static("Bearer test-api-token"),
    );
    next.run(request).await
}

/// Test-only alias so integration tests can exercise the route table without a socket.
pub fn route_for_tests(store: &MemoryStore, method: &str, url: &str, body: &str) -> (u16, String) {
    route(store, method, url, body)
}

/// Test-only startup-bound dispatcher for the local telemetry endpoint.
pub fn route_for_tests_with_startup_claim(
    store: &MemoryStore,
    identity: &crate::installation_identity::InstallationIdentity,
    claim: &crate::installation_identity::StartupClaim,
    method: &str,
    url: &str,
    body: &str,
) -> (u16, String) {
    match crate::context_telemetry::ContextIngestLease::from_startup(identity, claim) {
        Ok(lease) => route_with_context_ingest_lease(store, Some(&lease), method, url, body),
        Err(_) => (
            503,
            serde_json::json!({ "error": "active telemetry lease unavailable" }).to_string(),
        ),
    }
}

/// Test-only alias that wires the catalog + planner metrics into the same
/// dispatcher `dispatch` uses. Used by `tests/catalog_test.rs` to exercise
/// `/plan_context`, `/scope_grants`, and the augmented `/health` without a
/// socket.
pub fn route_with_catalog_for_tests(
    store: &MemoryStore,
    catalog: &ContextCatalog,
    method: &str,
    url: &str,
    body: &str,
) -> (u16, String) {
    route_full(
        store,
        Some(catalog),
        None,
        &crate::pull::metrics::PlannerLatency::new(),
        &crate::pull::metrics::LastFallback::new(),
        &std::sync::atomic::AtomicU64::new(0),
        method,
        url,
        body,
    )
}

/// Test-only alias that wires shared planner metrics so callers can observe
/// accumulated latency + fallback across multiple route invocations. Used
/// by `tests/catalog_test.rs` for the frozen-fixture p95 measurement and the
/// /health cross-call assertions.
#[allow(clippy::too_many_arguments)]
pub fn route_with_catalog_and_metrics_for_tests(
    store: &MemoryStore,
    catalog: &ContextCatalog,
    latency: &crate::pull::metrics::PlannerLatency,
    fallback: &crate::pull::metrics::LastFallback,
    schema_errors: &std::sync::atomic::AtomicU64,
    method: &str,
    url: &str,
    body: &str,
) -> (u16, String) {
    route_full(
        store,
        Some(catalog),
        None,
        latency,
        fallback,
        schema_errors,
        method,
        url,
        body,
    )
}

fn route_path(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

fn json_body(body: &str) -> Result<serde_json::Value, (u16, String)> {
    if body.trim().is_empty() {
        return Err((400, "{\"error\":\"empty json body\"}".to_string()));
    }
    serde_json::from_str(body).map_err(|_| (400, "{\"error\":\"malformed json body\"}".to_string()))
}

fn lifecycle_input_from_json(value: &Value) -> Result<MemoryLifecycleInputV1, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "request body must be an object".to_string())?;
    let lifecycle = [
        "effectiveFromMs",
        "effectiveUntilMs",
        "expiresAtMs",
        "reviewAfterMs",
        "priorityClass",
        "confidence",
        "confidenceBasis",
        "supersedes",
        "authority",
        "influenceClass",
    ]
    .into_iter()
    .filter_map(|key| {
        object
            .get(key)
            .map(|value| (key.to_string(), value.clone()))
    })
    .collect::<serde_json::Map<_, _>>();
    serde_json::from_value(Value::Object(lifecycle))
        .map_err(|error| format!("invalid lifecycle input: {error}"))
}

fn public_write_guard_defaults(lifecycle: &mut MemoryLifecycleInputV1) {
    if lifecycle.authority.is_none() {
        lifecycle.authority = Some("A0".into());
    }
    if lifecycle.influence_class.is_none() {
        lifecycle.influence_class = Some("data_only".into());
    }
}

fn event_context(v: &serde_json::Value, fallback_surface: &str) -> MemoryEventContext {
    let surface = v
        .get("client")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_surface);
    let mut context = MemoryEventContext::new(surface);
    if let Some(session) = v.get("session").and_then(|value| value.as_str()) {
        context = context.with_session(session);
    }
    if let Some(turn_id) = v
        .get("turn_id")
        .or_else(|| v.get("decision_id"))
        .and_then(|value| value.as_str())
    {
        context = context.with_turn(turn_id);
    }
    if let Some(trace_id) = v.get("trace_id").and_then(|value| value.as_str()) {
        context = context.with_trace(trace_id);
    }
    context
}

fn best_effort_event_context(body: &str, fallback_surface: &str) -> MemoryEventContext {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .map(|value| event_context(&value, fallback_surface))
        .unwrap_or_else(|| MemoryEventContext::new(fallback_surface))
}

fn external_route_stage(path: &str) -> Option<(&'static str, ExternalLifecycleStage)> {
    match path {
        "/put" | "/v1/memories:batch" => Some(("write", ExternalLifecycleStage::Validation)),
        "/delete" => Some(("delete", ExternalLifecycleStage::Validation)),
        "/get" | "/list" => Some(("read", ExternalLifecycleStage::Provider)),
        _ => None,
    }
}

fn record_http_rejection(
    store: &MemoryStore,
    path: &str,
    status: &str,
    reason_code: &str,
) -> Result<(), String> {
    let Some((operation, stage)) = external_route_stage(path) else {
        return Ok(());
    };
    store.record_external_lifecycle(
        &MemoryEventContext::new("http"),
        operation,
        stage,
        status,
        reason_code,
        None,
        None,
        "memory",
        "http",
        1,
    )
}

fn record_http_late_terminal(
    store: &MemoryStore,
    path: &str,
    status: &str,
    reason_code: &str,
) -> Result<(), String> {
    let Some((operation, _)) = external_route_stage(path) else {
        return Ok(());
    };
    let stage = match (operation, status) {
        ("write", "skipped") => ExternalLifecycleStage::Commit,
        ("write", _) => ExternalLifecycleStage::Embedding,
        ("delete", _) => ExternalLifecycleStage::Commit,
        _ => ExternalLifecycleStage::Provider,
    };
    store.record_external_lifecycle(
        &MemoryEventContext::new("http"),
        operation,
        stage,
        status,
        reason_code,
        None,
        None,
        "memory",
        "http",
        1,
    )
}

#[allow(clippy::too_many_arguments)]
fn record_external_or_500(
    store: &MemoryStore,
    context: &MemoryEventContext,
    operation: &str,
    stage: ExternalLifecycleStage,
    status: &str,
    reason_code: &str,
    memory_id: Option<&str>,
    scope_id: Option<&str>,
    artifact_family: &str,
    producer: &str,
    quantity: usize,
) -> Option<(u16, String)> {
    store
        .record_external_lifecycle(
            context,
            operation,
            stage,
            status,
            reason_code,
            memory_id,
            scope_id,
            artifact_family,
            producer,
            quantity,
        )
        .err()
        .map(|_| {
            (
                500,
                serde_json::json!({ "error": "external lifecycle accounting unavailable" })
                    .to_string(),
            )
        })
}

fn route(store: &MemoryStore, method: &str, url: &str, body: &str) -> (u16, String) {
    route_with_context_ingest_lease(store, None, method, url, body)
}

fn claims_reserved_adapt_authority(item: &crate::store::MemoryBatchItem) -> bool {
    item.artifact_family == "adapt"
        || item.producer == "adapt_native"
        || item.record_type == "taste_preference"
        || item.client == "membrane_adapt"
        || (item.lifecycle.authority.as_deref() == Some("A2")
            && item.lifecycle.influence_class.as_deref() == Some("behavioral_directive"))
}

/// Production federation route is native and same-process. Its request-time
/// H8 ceiling is validated before fan-out and its Push selection is attached
/// to the same response.
fn federate_route_response(store: &MemoryStore, body: &str) -> (u16, String) {
    crate::pull::federation::native_route_response_with_store(body, Some(store))
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// `HubFacadeV1::new`'s stream fallback: `Some` when the caller's own
/// `hub_inputs::live_inputs_from_local_service()` probe (taken once per
/// request, not re-probed here) returned data, `None` when it did not —
/// mirrors the existing `HubFacadeV1::new(Some(HubStreamV1{...}))`
/// construction pattern exercised by `hub.rs`'s own
/// `facade_preserves_observed_truth_without_inferred_liveness` test.
fn hub_stream_from_live_probe(reachable: bool) -> Option<membrane_protocol::HubStreamV1> {
    reachable.then(|| membrane_protocol::HubStreamV1 {
        state: membrane_protocol::HubStateV1::Available,
        reason: "observed".into(),
        resolver: Some("hub_inputs::live_inputs_from_local_service".into()),
    })
}

fn bounded_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let marker = "…";
    let limit = max_bytes.saturating_sub(marker.len());
    let mut end = limit.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &value[..end], marker)
}

/// Projects legacy Hub v1 state into the closed, content-free
/// `membrane.snapshot.v2` shape. Legacy items are deliberately summarized rather
/// than forwarded: their arbitrary maps are not legal v2 snapshot content.
fn membrane_snapshot_v2() -> Result<Value, String> {
    let live = crate::hub_inputs::live_inputs_from_local_service();
    let facade = crate::hub::HubFacadeV1::new(hub_stream_from_live_probe(live.is_some()));
    let observed_at_unix_ms = now_unix_ms();
    let inputs =
        live.unwrap_or_else(|| crate::hub::HubInputsV1::unavailable("source_not_connected"));
    let legacy = facade.dispatch_json("hub.snapshot", observed_at_unix_ms, inputs)?;
    let sections = legacy
        .get("sections")
        .and_then(Value::as_object)
        .ok_or_else(|| "snapshot sections unavailable".to_string())?;
    if sections.is_empty() || sections.len() > SNAPSHOT_MAX_SECTIONS {
        return Err("snapshot section bounds invalid".into());
    }

    let mut bounded_sections = serde_json::Map::new();
    for (name, section) in sections.iter().take(SNAPSHOT_MAX_SECTIONS) {
        let legacy_state = match section.get("state").and_then(Value::as_str) {
            Some("available") => "available",
            Some("degraded") => "degraded",
            _ => "unavailable",
        };
        let raw_count = section
            .get("items")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        let item_limit_exceeded = raw_count > SNAPSHOT_MAX_ITEMS_PER_SECTION;
        let state = if item_limit_exceeded {
            "degraded"
        } else {
            legacy_state
        };
        let reason = bounded_utf8(
            if item_limit_exceeded {
                "snapshot_item_limit"
            } else {
                section
                    .get("reason")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("reason_unavailable")
            },
            SNAPSHOT_MAX_REASON_BYTES,
        );
        let count = raw_count.min(SNAPSHOT_MAX_ITEMS_PER_SECTION);
        let item = json!({
            "label": bounded_utf8(name, SNAPSHOT_MAX_ITEM_LABEL_BYTES),
            "kind": bounded_utf8("hub_section", SNAPSHOT_MAX_ITEM_KIND_BYTES),
            "count": count,
            "severity": match state {
                "available" => "info",
                "degraded" => "warning",
                _ => "error",
            },
            "evidence": bounded_utf8(&format!("hub.snapshot.v1/{name}"), SNAPSHOT_MAX_ITEM_STRING_BYTES),
            "resolver": "membrane.hub.v1",
            "observedAtUnixMs": section.get("observedAtUnixMs").and_then(Value::as_u64).unwrap_or(observed_at_unix_ms),
            "stale": state != "available",
        });
        bounded_sections.insert(
            bounded_utf8(name, SNAPSHOT_MAX_ITEM_LABEL_BYTES),
            json!({
                "state": state,
                "reason": reason,
                "items": [item],
                "evidence": "hub.snapshot.v1",
                "resolver": "membrane.hub.v1",
                "observedAtUnixMs": observed_at_unix_ms,
            }),
        );
    }
    let stale = sections
        .values()
        .any(|section| section.get("state").and_then(Value::as_str) != Some("available"));
    let snapshot = json!({
        "schemaVersion": 2,
        "productId": "membrane",
        "observedAtUnixMs": observed_at_unix_ms,
        "sections": bounded_sections,
        "stale": stale,
    });
    if snapshot.to_string().len() > SNAPSHOT_MAX_TOTAL_BYTES {
        return Err("snapshot exceeds total byte bound".into());
    }
    Ok(snapshot)
}

fn route_with_context_ingest_lease(
    store: &MemoryStore,
    context_ingest_lease: Option<&crate::context_telemetry::ContextIngestLease>,
    method: &str,
    url: &str,
    body: &str,
) -> (u16, String) {
    let path = route_path(url);
    if body.len() > MAX_BODY_BYTES {
        if record_http_rejection(store, path, "failed", "request_too_large").is_err() {
            return (
                500,
                serde_json::json!({ "error": "external lifecycle accounting unavailable" })
                    .to_string(),
            );
        }
        return (413, "{\"error\":\"request body too large\"}".to_string());
    }
    if method == "POST" && path == crate::adapt_service::OPERATOR_PATH {
        return crate::adapt_service::operator_response(store, body);
    }
    if method == "POST" && path == crate::adapt_service::OBSERVATION_PATH {
        return crate::adapt_observations::response(store, body);
    }
    if method == "GET" && (path == "/" || path == "/index.html") {
        return (200, DASHBOARD_HTML.to_string());
    }
    if method == "GET" && path == "/metrics" {
        return (200, store.metrics_json().to_string());
    }
    if method == "GET" && path == "/activity" {
        return (200, store.activity_json(20).to_string());
    }
    if method == "GET" && path == "/graph" {
        return (
            200,
            store.relationship_graph_json(3, 0.55, false).to_string(),
        );
    }
    if method == "GET" && path == "/analysis" {
        return analysis_response(&configured_analysis_directory());
    }
    if method == "POST" && path == "/anchor/retrieve" {
        return anchor_retrieve_response(body);
    }
    if method == "POST" && path == "/memory-lifecycle" {
        let Some(_lease) = context_ingest_lease else {
            return (503, r#"{"error":"verified startup lease required"}"#.into());
        };
        let value = match json_body(body) {
            Ok(value) => value,
            Err(response) => return response,
        };
        let request: MemoryLifecycleOperationV1 =
            match serde_json::from_value(value.clone()) {
                Ok(request) => request,
                Err(error) => return (
                    400,
                    serde_json::json!({"error": format!("invalid lifecycle operation: {error}")})
                        .to_string(),
                ),
            };
        let request_digest = sha256_bytes(body.as_bytes());
        let actor = match VerifiedMemoryActor::from_execution_context(
            "membrane-runtime",
            "A1",
            "startup_lease",
            &format!("lifecycle-{}", &request_digest[..16]),
            &format!("lifecycle-{}", &request_digest[16..32]),
        ) {
            Ok(actor) => actor,
            Err(error) => return (403, serde_json::json!({"error": error}).to_string()),
        };
        return match store.execute_lifecycle_operation(&request, &actor) {
            Ok(receipt) => (
                200,
                serde_json::to_string(&receipt)
                    .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".into()),
            ),
            Err(error) => (409, serde_json::json!({"error": error}).to_string()),
        };
    }
    if method == "POST" && matches!(path, "/expand" | "/push/resolve") {
        return crate::push::api::http_response("membrane_push_resolve", body);
    }
    if method == "POST" && path == "/push/prepare" {
        return crate::push::api::http_response("membrane_push_prepare", body);
    }
    if method == "POST" && path == "/federate" {
        return federate_route_response(store, body);
    }
    if method == "GET" && path == "/hub/capabilities" {
        let live = crate::hub_inputs::live_inputs_from_local_service();
        let facade = crate::hub::HubFacadeV1::new(hub_stream_from_live_probe(live.is_some()));
        return match facade.dispatch_json(
            "hub.capabilities",
            now_unix_ms(),
            crate::hub::HubInputsV1::unavailable("source_not_connected"),
        ) {
            Ok(value) => (200, value.to_string()),
            Err(error) => (500, serde_json::json!({"error": error}).to_string()),
        };
    }
    if method == "GET" && path == "/hub/snapshot" {
        // Canonical composition shared with `membrane cli hub-snapshot` — one
        // producer, one parent truth, typed membraneState + six subsystems.
        return match serde_json::to_value(crate::hub_inputs::compose_live_hub_snapshot()) {
            Ok(value) => (200, value.to_string()),
            Err(error) => (
                500,
                serde_json::json!({"error": error.to_string()}).to_string(),
            ),
        };
    }
    if method == "POST" && path == "/delivery/trace" {
        return delivery_trace_response(body);
    }
    if method == "POST" && path == "/freshness" {
        let v = match json_body(body) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        let Some(requested) = v
            .get("repoRoot")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        else {
            return (
                400,
                serde_json::json!({ "error": "repoRoot required" }).to_string(),
            );
        };
        let repo_root = match crate::freshness::canonical_repo_root(
            std::path::Path::new(requested),
            &configured_workspace_root(),
        ) {
            Ok(root) => root,
            Err(error) if error == "repoRoot is outside the configured workspace" => {
                return (403, serde_json::json!({ "error": error }).to_string());
            }
            Err(error) => return (400, serde_json::json!({ "error": error }).to_string()),
        };
        let verdict = crate::freshness::evaluate_repository_freshness(store, repo_root.clone());
        return freshness_response_body(&v, &verdict, &repo_root);
    }
    if method == "POST" && path == "/skills-snapshot" {
        return match store.skills_snapshot() {
            Ok(snapshot) => match serde_json::to_string(&snapshot) {
                Ok(payload) => (200, payload),
                Err(_) => (
                    500,
                    serde_json::json!({ "error": "skills snapshot serialization failed" })
                        .to_string(),
                ),
            },
            Err(_) => (
                500,
                serde_json::json!({ "error": "skills snapshot unavailable" }).to_string(),
            ),
        };
    }
    if method == "POST" && path == "/v1/telemetry/events:batch" {
        let Some(context_ingest_lease) = context_ingest_lease else {
            return (
                503,
                serde_json::json!({ "error": "active telemetry lease unavailable" }).to_string(),
            );
        };
        let batch = match serde_json::from_str::<crate::context_telemetry::ContextEventBatch>(body)
        {
            Ok(batch) => batch,
            Err(_) => {
                return (
                    400,
                    serde_json::json!({ "error": "invalid context telemetry batch" }).to_string(),
                )
            }
        };
        return match store
            .db()
            .ingest_local_context_events(&batch, context_ingest_lease)
        {
            Ok(receipt) => {
                let status = if receipt.inserted == 0 { 200 } else { 201 };
                match serde_json::to_string(&receipt) {
                    Ok(payload) => (status, payload),
                    Err(_) => (
                        500,
                        serde_json::json!({ "error": "telemetry receipt serialization failed" })
                            .to_string(),
                    ),
                }
            }
            Err(crate::context_telemetry::ContextTelemetryError::Invalid(error)) => {
                (400, serde_json::json!({ "error": error }).to_string())
            }
            Err(crate::context_telemetry::ContextTelemetryError::Conflict { event_id }) => (
                409,
                serde_json::json!({
                    "error": "event_id conflicts with an existing canonical event",
                    "event_id": event_id,
                })
                .to_string(),
            ),
            Err(crate::context_telemetry::ContextTelemetryError::AttributionMismatch) => (
                403,
                serde_json::json!({
                    "error": "context telemetry attribution does not match active installation"
                })
                .to_string(),
            ),
            Err(crate::context_telemetry::ContextTelemetryError::Database(_)) => (
                500,
                serde_json::json!({ "error": "context telemetry storage unavailable" }).to_string(),
            ),
        };
    }
    if method == "POST" && path == "/v1/telemetry/observable-events:batch" {
        let Some(context_ingest_lease) = context_ingest_lease else {
            return (
                503,
                serde_json::json!({ "error": "active telemetry lease unavailable" }).to_string(),
            );
        };
        let batch =
            match serde_json::from_str::<crate::context_telemetry::ObservableEventBatchV1>(body) {
                Ok(batch) => batch,
                Err(_) => {
                    return (
                        400,
                        serde_json::json!({ "error": "invalid observable event batch" })
                            .to_string(),
                    )
                }
            };
        return match store
            .db()
            .ingest_observable_events(&batch, context_ingest_lease)
        {
            Ok(receipt) => {
                let status = if receipt.inserted == 0 { 200 } else { 201 };
                match serde_json::to_string(&receipt) {
                    Ok(payload) => (status, payload),
                    Err(_) => (
                        500,
                        serde_json::json!({ "error": "observable receipt serialization failed" })
                            .to_string(),
                    ),
                }
            }
            Err(crate::context_telemetry::ContextTelemetryError::Invalid(error)) => {
                (400, serde_json::json!({ "error": error }).to_string())
            }
            Err(crate::context_telemetry::ContextTelemetryError::Conflict { event_id }) => (
                409,
                serde_json::json!({
                    "error": "event_id conflicts with an existing canonical event",
                    "event_id": event_id,
                })
                .to_string(),
            ),
            Err(crate::context_telemetry::ContextTelemetryError::AttributionMismatch) => (
                403,
                serde_json::json!({
                    "error": "observable event attribution does not match active installation"
                })
                .to_string(),
            ),
            Err(crate::context_telemetry::ContextTelemetryError::Database(_)) => (
                500,
                serde_json::json!({ "error": "observable event storage unavailable" }).to_string(),
            ),
        };
    }
    if method == "POST" && path == "/v1/telemetry/observable-events:query-taste" {
        return observable_query_response(store, body, ObservableQueryRoute::Taste);
    }
    if method == "POST" && path == "/v1/telemetry/observable-events:query-insights" {
        return observable_query_response(store, body, ObservableQueryRoute::Insights);
    }
    if method == "POST" && path == "/v1/telemetry/observable-events:query-forge-time" {
        return observable_query_response(store, body, ObservableQueryRoute::ForgeTimeAccounting);
    }
    if method == "POST" && path == "/v1/memories:batch" {
        let request = match serde_json::from_str::<MemoryBatchRequest>(body) {
            Ok(request) => request,
            Err(_) => {
                let context = best_effort_event_context(body, "http");
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "write",
                    ExternalLifecycleStage::Validation,
                    "failed",
                    "malformed_json",
                    None,
                    None,
                    "memory",
                    "http",
                    1,
                ) {
                    return response;
                }
                return (
                    400,
                    serde_json::json!({ "error": "invalid memory batch" }).to_string(),
                );
            }
        };
        if let Some(item) = request
            .items
            .iter()
            .find(|item| claims_reserved_adapt_authority(item))
        {
            let context = MemoryEventContext::new(&item.client)
                .with_session(&item.session_id)
                .with_turn(&item.turn_id)
                .with_trace(&item.trace_id);
            if let Some(response) = record_external_or_500(
                store,
                &context,
                "write",
                ExternalLifecycleStage::Validation,
                "failed",
                RESERVED_ADAPT_ADMISSION_REASON,
                Some(&format!("batch/{}", request.batch_id)),
                Some(&item.scope),
                "memory",
                "http",
                1,
            ) {
                return response;
            }
            return (
                403,
                serde_json::json!({
                    "error": "reserved Adapt authority requires verified native apply",
                    "code": RESERVED_ADAPT_ADMISSION_REASON,
                })
                .to_string(),
            );
        }
        let context = request
            .items
            .first()
            .map(|item| {
                MemoryEventContext::new(&item.client)
                    .with_session(&item.session_id)
                    .with_turn(&item.turn_id)
                    .with_trace(&item.trace_id)
            })
            .unwrap_or_else(|| MemoryEventContext::new("http"));
        return match store.try_put_batch(&request) {
            Ok(receipt) => {
                if receipt.inserted == 0 {
                    for item in &request.items {
                        let item_context = MemoryEventContext::new(&item.client)
                            .with_session(&item.session_id)
                            .with_turn(&item.turn_id)
                            .with_trace(&item.trace_id);
                        let memory_id = format!(
                            "{}/{}",
                            crate::scope::normalize_scope(&item.scope),
                            item.name
                        );
                        if let Some(response) = record_external_or_500(
                            store,
                            &item_context,
                            "write",
                            ExternalLifecycleStage::Commit,
                            "skipped",
                            "idempotent_duplicate",
                            Some(&memory_id),
                            Some(&item.scope),
                            &item.artifact_family,
                            &item.producer,
                            1,
                        ) {
                            return response;
                        }
                    }
                }
                let status = if receipt.inserted == 0 { 200 } else { 201 };
                match serde_json::to_string(&receipt) {
                    Ok(payload) => (status, payload),
                    Err(_) => (
                        500,
                        serde_json::json!({ "error": "memory batch receipt serialization failed" })
                            .to_string(),
                    ),
                }
            }
            Err(error) => {
                let (http_status, stage, status, reason) = match error {
                    MemoryBatchError::Invalid(_) => (
                        400,
                        ExternalLifecycleStage::Validation,
                        "failed",
                        "invalid_batch",
                    ),
                    MemoryBatchError::Conflict => (
                        409,
                        ExternalLifecycleStage::Validation,
                        "failed",
                        "batch_conflict",
                    ),
                    MemoryBatchError::Persist(ref message)
                        if message.contains("embed") || message.contains("writes disabled") =>
                    {
                        (
                            500,
                            ExternalLifecycleStage::Embedding,
                            "unavailable",
                            "embedding_unavailable",
                        )
                    }
                    MemoryBatchError::Persist(_) => (
                        500,
                        ExternalLifecycleStage::Commit,
                        "failed",
                        "commit_failed",
                    ),
                };
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "write",
                    stage,
                    status,
                    reason,
                    Some(&format!("batch/{}", request.batch_id)),
                    None,
                    "memory",
                    "http",
                    request.items.len().max(1),
                ) {
                    return response;
                }
                let message = if http_status == 409 {
                    "batch_id conflicts with an existing canonical request"
                } else if http_status == 400 {
                    "invalid memory batch"
                } else {
                    "memory batch storage unavailable"
                };
                (
                    http_status,
                    serde_json::json!({ "error": message }).to_string(),
                )
            }
        };
    }
    if method == "POST" && path == "/put" {
        let v = match json_body(body) {
            Ok(v) => v,
            Err(resp) => {
                let context = best_effort_event_context(body, "http");
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "write",
                    ExternalLifecycleStage::Validation,
                    "failed",
                    "malformed_json",
                    None,
                    None,
                    "memory",
                    "http",
                    1,
                ) {
                    return response;
                }
                return resp;
            }
        };
        let context = event_context(&v, "http");
        let lifecycle = match lifecycle_input_from_json(&v) {
            Ok(mut lifecycle) => {
                public_write_guard_defaults(&mut lifecycle);
                lifecycle
            }
            Err(error) => return (400, serde_json::json!({"error": error}).to_string()),
        };
        if let Err(error) = lifecycle.validate() {
            return (400, serde_json::json!({"error": error}).to_string());
        }
        let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("").trim();
        let content = v
            .get("content")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        if name.is_empty() || content.is_empty() {
            if let Some(response) = record_external_or_500(
                store,
                &context,
                "write",
                ExternalLifecycleStage::Validation,
                "failed",
                "invalid_request",
                None,
                None,
                "memory",
                "http",
                1,
            ) {
                return response;
            }
            return (400, "{\"error\":\"name and content required\"}".into());
        }
        if content.chars().count() > MAX_CONTENT_CHARS {
            if let Some(response) = record_external_or_500(
                store,
                &context,
                "write",
                ExternalLifecycleStage::Validation,
                "failed",
                "content_too_large",
                None,
                None,
                "memory",
                "http",
                1,
            ) {
                return response;
            }
            return (413, "{\"error\":\"content too large\"}".into());
        }
        let scope = v
            .get("scope")
            .and_then(|x| x.as_str())
            .unwrap_or("proposed");
        if let Err(message) = crate::scope::validate_write_scope(scope) {
            if let Some(response) = record_external_or_500(
                store,
                &context,
                "write",
                ExternalLifecycleStage::Validation,
                "failed",
                "invalid_scope",
                None,
                Some(scope),
                "memory",
                "http",
                1,
            ) {
                return response;
            }
            return (400, serde_json::json!({ "error": message }).to_string());
        }
        let tier = match v.get("tier").and_then(|x| x.as_str()).unwrap_or("Working") {
            t if t.eq_ignore_ascii_case("working") => cortex_core::MemoryTier::Working,
            t if t.eq_ignore_ascii_case("episodic") => cortex_core::MemoryTier::Episodic,
            _ => cortex_core::MemoryTier::Semantic,
        };
        let artifact_family = v
            .get("artifactFamily")
            .and_then(|x| x.as_str())
            .unwrap_or("memory")
            .trim();
        let producer = v
            .get("producer")
            .and_then(|x| x.as_str())
            .unwrap_or("manual")
            .trim();
        let record_type = v
            .get("recordType")
            .and_then(|x| x.as_str())
            .unwrap_or("memory")
            .trim();
        let memory_id = format!("{}/{}", crate::scope::normalize_scope(scope), name);
        return match store.try_put_attributed_lifecycle_observed(
            name,
            content,
            scope,
            tier,
            artifact_family,
            producer,
            record_type,
            &context,
            &lifecycle,
        ) {
            Ok(id) => {
                // Recall admits A1..A5 only, and an unspecified authority
                // defaults to A0 — so the default write is invisible to the
                // default read, and a bare `{"put": id}` gave no sign of it.
                // Say it at the point of writing rather than leaving the
                // caller to discover an empty recall later.
                let recall_eligible = matches!(
                    lifecycle.authority.as_deref(),
                    Some("A1") | Some("A2") | Some("A3") | Some("A4") | Some("A5")
                );
                let mut body = serde_json::json!({ "put": id, "recallEligible": recall_eligible });
                if !recall_eligible {
                    body["note"] = serde_json::json!(format!(
                        "stored at authority {}; recall admits A1..A5, so this row will not be recalled",
                        lifecycle.authority.as_deref().unwrap_or("A0")
                    ));
                }
                (200, body.to_string())
            }
            Err(e) if e.starts_with("memory write attribution") => {
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "write",
                    ExternalLifecycleStage::Validation,
                    "failed",
                    "invalid_attribution",
                    Some(&memory_id),
                    Some(scope),
                    "memory",
                    "http",
                    1,
                ) {
                    return response;
                }
                (400, serde_json::json!({ "error": e }).to_string())
            }
            Err(e) => {
                let (stage, status, reason) =
                    if e.contains("embed") || e.contains("writes disabled") {
                        (
                            ExternalLifecycleStage::Embedding,
                            "unavailable",
                            "embedding_unavailable",
                        )
                    } else {
                        (ExternalLifecycleStage::Commit, "failed", "commit_failed")
                    };
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "write",
                    stage,
                    status,
                    reason,
                    Some(&memory_id),
                    Some(scope),
                    if crate::context_telemetry::registered_artifact_family(artifact_family) {
                        artifact_family
                    } else {
                        "memory"
                    },
                    if producer.is_empty() {
                        "http"
                    } else {
                        producer
                    },
                    1,
                ) {
                    return response;
                }
                (500, serde_json::json!({ "error": e }).to_string())
            }
        };
    }
    if method == "POST" && path == "/remember" {
        let v = match json_body(body) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        let content = v
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if content.is_empty() {
            return (400, "{\"error\":\"content required\"}".into());
        }
        if content.chars().count() > MAX_CONTENT_CHARS {
            return (413, "{\"error\":\"content too large\"}".into());
        }
        let keywords = v
            .get("keywords")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let entry = store.remember(content, keywords);
        return (
            200,
            serde_json::to_string(&entry).unwrap_or_else(|_| "{}".into()),
        );
    }
    if method == "POST" && path == "/remember_consolidated" {
        let v = match json_body(body) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        let name = v.get("name").and_then(Value::as_str).unwrap_or("").trim();
        let content = v
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if name.is_empty() || content.is_empty() {
            return (400, "{\"error\":\"name and content required\"}".into());
        }
        if content.chars().count() > MAX_CONTENT_CHARS {
            return (413, "{\"error\":\"content too large\"}".into());
        }
        let keywords = v
            .get("keywords")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let threshold = v.get("threshold").and_then(Value::as_f64).unwrap_or(0.0);
        let entry = store.remember_consolidated(name, content, keywords, threshold);
        let response = match entry {
            Some(entry) => {
                let id = entry.id.clone();
                serde_json::json!({"id": id, "consolidated": true, "entry": entry})
            }
            None => serde_json::json!({"id": null, "consolidated": false}),
        };
        return (200, response.to_string());
    }
    if method == "POST" && path == "/delete" {
        let v = match json_body(body) {
            Ok(v) => v,
            Err(resp) => {
                let context = best_effort_event_context(body, "http");
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "delete",
                    ExternalLifecycleStage::Validation,
                    "failed",
                    "malformed_json",
                    None,
                    None,
                    "memory",
                    "http",
                    1,
                ) {
                    return response;
                }
                return resp;
            }
        };
        let context = event_context(&v, "http");
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").trim();
        if id.is_empty() {
            if let Some(response) = record_external_or_500(
                store,
                &context,
                "delete",
                ExternalLifecycleStage::Validation,
                "failed",
                "missing_id",
                None,
                None,
                "memory",
                "http",
                1,
            ) {
                return response;
            }
            return (400, "{\"error\":\"missing id\"}".into());
        }
        return match store.try_delete_observed(id, &context) {
            Ok(true) => (200, serde_json::json!({ "deleted": true }).to_string()),
            Ok(false) => {
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "delete",
                    ExternalLifecycleStage::Commit,
                    "empty",
                    "target_not_found",
                    Some(id),
                    None,
                    "memory",
                    "http",
                    0,
                ) {
                    return response;
                }
                (404, serde_json::json!({ "deleted": false }).to_string())
            }
            Err(e) => {
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "delete",
                    ExternalLifecycleStage::Commit,
                    "failed",
                    "commit_failed",
                    Some(id),
                    None,
                    "memory",
                    "http",
                    1,
                ) {
                    return response;
                }
                (500, serde_json::json!({ "error": e }).to_string())
            }
        };
    }
    if method == "POST" && path == "/list" {
        let v = match json_body(body) {
            Ok(v) => v,
            Err(resp) => {
                let context = best_effort_event_context(body, "http");
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "read",
                    ExternalLifecycleStage::Provider,
                    "failed",
                    "malformed_json",
                    None,
                    None,
                    "memory",
                    "http",
                    1,
                ) {
                    return response;
                }
                return resp;
            }
        };
        let context = event_context(&v, "http");
        let scope = v
            .get("scope")
            .and_then(|x| x.as_str())
            .filter(|s| !s.trim().is_empty());
        let response_version = v
            .get("responseVersion")
            .or_else(|| v.get("response_version"))
            .and_then(Value::as_u64)
            .unwrap_or(1);
        let limit = v
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(256);
        if response_version == 2 && (limit == 0 || limit > 1_000) {
            return (
                400,
                serde_json::json!({"error":"limit must be between 1 and 1000"}).to_string(),
            );
        }
        let (listed, completeness) = match if response_version == 2 {
            store
                .try_list_bounded(scope, limit)
                .map(|page| (page.items, Some(page.completeness)))
        } else {
            store.try_list(scope).map(|items| (items, None))
        } {
            Ok(result) => result,
            Err(error) => {
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "read",
                    ExternalLifecycleStage::Provider,
                    "failed",
                    "provider_failed",
                    None,
                    scope,
                    "memory",
                    "http",
                    1,
                ) {
                    return response;
                }
                return (500, serde_json::json!({ "error": error }).to_string());
            }
        };
        let rows: Vec<serde_json::Value> = listed
            .into_iter()
            .map(|(id, tier, chars, access, inject)| {
                serde_json::json!({"id": id, "tier": tier, "chars": chars,
                                   "access": access, "inject": inject})
            })
            .collect();
        let (status, reason) = if rows.is_empty() {
            ("empty", "no_results")
        } else {
            ("success", "results_returned")
        };
        if let Some(response) = record_external_or_500(
            store,
            &context,
            "read",
            ExternalLifecycleStage::Provider,
            status,
            reason,
            None,
            scope,
            "memory",
            "http",
            rows.len(),
        ) {
            return response;
        }
        return if response_version == 2 {
            (
                200,
                serde_json::json!({
                    "schemaVersion": 2,
                    "items": rows,
                    "completeness": completeness.expect("V2 list always has completeness"),
                })
                .to_string(),
            )
        } else {
            (200, serde_json::Value::Array(rows).to_string())
        };
    }
    if method == "POST" && path == "/scopes" {
        return (
            200,
            serde_json::to_string(&store.scopes()).unwrap_or_else(|_| "[]".into()),
        );
    }
    if method == "POST" && path == "/get" {
        let v = match json_body(body) {
            Ok(v) => v,
            Err(resp) => {
                let context = best_effort_event_context(body, "http");
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "read",
                    ExternalLifecycleStage::Provider,
                    "failed",
                    "malformed_json",
                    None,
                    None,
                    "memory",
                    "http",
                    1,
                ) {
                    return response;
                }
                return resp;
            }
        };
        let context = event_context(&v, "http");
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").trim();
        if id.is_empty() {
            if let Some(response) = record_external_or_500(
                store,
                &context,
                "read",
                ExternalLifecycleStage::Provider,
                "failed",
                "missing_id",
                None,
                None,
                "memory",
                "http",
                1,
            ) {
                return response;
            }
            return (400, "{\"error\":\"missing id\"}".into());
        }
        return match store.get_full_observed(id, &context) {
            Ok((content, access_count)) => match store.lifecycle_json_for(id) {
                Ok(lifecycle) => (
                    200,
                    serde_json::json!({"id": id, "content": content, "access_count": access_count, "lifecycle": lifecycle})
                        .to_string(),
                ),
                Err(error) => (500, serde_json::json!({"error": error}).to_string()),
            },
            Err(e) => {
                let (http_status, status, reason) = if e.starts_with("no memory with id ") {
                    (404, "empty", "target_not_found")
                } else {
                    (500, "failed", "provider_failed")
                };
                if let Some(response) = record_external_or_500(
                    store,
                    &context,
                    "read",
                    ExternalLifecycleStage::Provider,
                    status,
                    reason,
                    Some(id),
                    None,
                    "memory",
                    "http",
                    usize::from(http_status != 404),
                ) {
                    return response;
                }
                (http_status, serde_json::json!({ "error": e }).to_string())
            }
        };
    }
    if method == "GET" && path == "/health" {
        return (200, store.health_json().to_string());
    }
    let request_parse_started = std::time::Instant::now();
    let v = if method == "POST" {
        match json_body(body) {
            Ok(v) => v,
            Err(resp) => return resp,
        }
    } else {
        serde_json::Value::Null
    };
    if method == "POST" && path == "/scratchpad" {
        return (200, crate::scratchpad::handle(&v).to_string());
    }
    if method == "POST" && path == "/scratchpad/session-close" {
        let session = v
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let (status, body) = crate::scratchpad::close_response(session);
        return (status, body.to_string());
    }
    let request_parse_ms = request_parse_started.elapsed().as_secs_f64() * 1000.0;
    if method == "POST" && path == "/recall" {
        let query = v.get("query").and_then(|x| x.as_str()).unwrap_or("").trim();
        if query.is_empty() {
            return (400, "{\"error\":\"query required\"}".to_string());
        }
        if query.chars().count() > MAX_QUERY_CHARS {
            return (413, "{\"error\":\"query too large\"}".to_string());
        }
        let client = v
            .get("client")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        if client.is_empty() || client.eq_ignore_ascii_case("unknown") {
            return (
                400,
                "{\"error\":\"client attribution required\"}".to_string(),
            );
        }
        let k = v.get("k").and_then(|x| x.as_u64()).unwrap_or(6);
        if k == 0 || k > MAX_RECALL_K {
            return (
                400,
                "{\"error\":\"k must be between 1 and 50\"}".to_string(),
            );
        }
        let k = k as usize;
        let bounded_budget = |key: &str, default: usize, max: usize| {
            let value = v
                .get(key)
                .and_then(|x| x.as_u64())
                .map(|x| x as usize)
                .unwrap_or(default);
            if !(80..=max).contains(&value) {
                Err((
                    400,
                    serde_json::json!({"error": format!("{key} must be between 80 and {max}")})
                        .to_string(),
                ))
            } else {
                Ok(value)
            }
        };
        let preview_chars = match bounded_budget("preview_chars", 240, 400) {
            Ok(value) => value,
            Err(response) => return response,
        };
        let top_preview_chars = match bounded_budget("top_preview_chars", 400, 600) {
            Ok(value) => value,
            Err(response) => return response,
        };
        if top_preview_chars < preview_chars {
            return (
                400,
                "{\"error\":\"top_preview_chars must be >= preview_chars\"}".to_string(),
            );
        }
        let default_total = top_preview_chars + k.saturating_sub(1) * preview_chars;
        let total_preview_chars =
            match bounded_budget("total_preview_chars", default_total.min(4000), 4000) {
                Ok(value) => value,
                Err(response) => return response,
            };
        let observe = v.get("observe").and_then(|x| x.as_bool()).unwrap_or(true);
        let traffic_class = v
            .get("traffic_class")
            .and_then(|x| x.as_str())
            .unwrap_or("production")
            .trim();
        if traffic_class.is_empty() || traffic_class.chars().count() > 64 {
            return (400, "{\"error\":\"invalid traffic_class\"}".to_string());
        }
        let production_traffic = traffic_class.eq_ignore_ascii_case("production");
        let explicit_smoke = match v.get("context_source") {
            None => false,
            Some(value) => match value.as_str() {
                Some(marker) if marker.trim().eq_ignore_ascii_case("smoke") => true,
                _ => {
                    return (
                        400,
                        "{\"error\":\"context_source must be smoke when present\"}".to_string(),
                    )
                }
            },
        };
        let normalized_client = client.to_ascii_lowercase();
        let smoke_client = matches!(
            normalized_client.as_str(),
            "smoke" | "spotcheck" | "smoke-spotcheck"
        );
        let smoke_sink = explicit_smoke || smoke_client;
        if smoke_sink && (observe || production_traffic) {
            return (
                400,
                "{\"error\":\"smoke recall requires observe=false and nonproduction traffic_class\"}"
                    .to_string(),
            );
        }
        if !observe
            && (v.get("traffic_class").and_then(|x| x.as_str()).is_none() || production_traffic)
        {
            return (
                400,
                "{\"error\":\"observe=false requires explicit nonproduction traffic_class\"}"
                    .to_string(),
            );
        }
        let scope = v.get("scope").and_then(|x| x.as_str()).map(normalize_scope);
        let mut chain = match &scope {
            Some(s) => scope_chain(s, &store.scopes()),
            None => Vec::new(),
        };
        for c in str_list(&v, "cross") {
            if !chain.contains(&c) {
                chain.push(c);
            }
        }
        let bounded_taste_count = |key: &str, default: usize, max: usize| {
            let value = v
                .get(key)
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(default);
            (value <= max).then_some(value).ok_or_else(|| {
                (
                    400,
                    serde_json::json!({"error": format!("{key} must be between 0 and {max}")})
                        .to_string(),
                )
            })
        };
        let taste_core_records = match bounded_taste_count("taste_max_core", 2, 4) {
            Ok(value) => value,
            Err(response) => return response,
        };
        let taste_scoped_records = match bounded_taste_count("taste_max_scoped", 4, 16) {
            Ok(value) => value,
            Err(response) => return response,
        };
        let model = match v.get("model") {
            None => None,
            Some(Value::String(value))
                if !value.trim().is_empty() && value.chars().count() <= 200 =>
            {
                Some(value.trim().to_string())
            }
            Some(_) => return (400, "{\"error\":\"invalid model dimension\"}".into()),
        };
        let machine = match v.get("machine") {
            None => None,
            Some(Value::String(value))
                if !value.trim().is_empty() && value.chars().count() <= 200 =>
            {
                Some(value.trim().to_string())
            }
            Some(_) => return (400, "{\"error\":\"invalid machine identity\"}".into()),
        };
        let mut raw_dimensions = BTreeMap::new();
        if let Some(dimensions) = v.get("dimensions") {
            let Some(dimensions) = dimensions.as_object() else {
                return (400, "{\"error\":\"dimensions must be an object\"}".into());
            };
            for (key, value) in dimensions {
                let Some(value) = value.as_str() else {
                    return (
                        400,
                        "{\"error\":\"dimension values must be strings\"}".into(),
                    );
                };
                let normalized_key = key.trim().to_ascii_lowercase();
                if normalized_key.is_empty() || raw_dimensions.contains_key(&normalized_key) {
                    return (
                        400,
                        serde_json::json!({"error": format!("duplicate normalized Taste dimension: {normalized_key}")})
                            .to_string(),
                    );
                }
                raw_dimensions.insert(normalized_key, value.trim().to_string());
            }
        }
        for (key, value) in [("client", Some(client)), ("model", model.as_deref())] {
            let Some(value) = value else { continue };
            if raw_dimensions
                .get(key)
                .is_some_and(|declared| !declared.eq_ignore_ascii_case(value))
            {
                return (
                    400,
                    serde_json::json!({"error": format!("{key} dimension conflicts with request attribution")})
                        .to_string(),
                );
            }
            raw_dimensions.insert(key.into(), value.into());
        }
        let taste_dimensions = match membrane_adapt::scope::ScopeDimensions::normalize(
            &raw_dimensions,
        ) {
            Ok(dimensions) => dimensions,
            Err(error) => return (
                400,
                serde_json::json!({"error": format!("invalid Taste scope dimensions: {error}")})
                    .to_string(),
            ),
        };
        let mut allowed_taste_scopes = if chain.is_empty() {
            vec!["global".to_string()]
        } else {
            chain.clone()
        };
        allowed_taste_scopes.sort();
        allowed_taste_scopes.dedup();
        let delivery_timestamp = crate::time::now_iso();
        let taste_request_id = format!(
            "taste-request-{}-{}",
            crate::time::now_millis(),
            TASTE_REQUEST_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let (inventory, delivery_plan) = match crate::adapt_service::select(
            store,
            &membrane_adapt::delivery::PreferenceDeliveryContextV1 {
                allowed_scopes: allowed_taste_scopes,
                dimensions: taste_dimensions,
                machine,
                max_core_records: taste_core_records,
                max_scoped_records: taste_scoped_records,
                max_total_records: k,
                max_rendered_chars: total_preview_chars,
                timestamp: delivery_timestamp,
                session_id: v
                    .get("session")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                trace_id: v
                    .get("trace_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                request_id: taste_request_id,
                client: client.into(),
                model,
            },
        ) {
            Ok(value) => value,
            Err(error) => {
                return (
                    503,
                    json!({"error":"adapt_delivery_unavailable","detail":error}).to_string(),
                )
            }
        };
        let mut full_chars = 0usize;
        let mut injected_chars = 0usize;
        let mut remaining_preview_chars = total_preview_chars;
        let mut hits = Vec::with_capacity(k);
        let mut seen_ids = HashSet::new();
        for delivered in delivery_plan.delivered {
            let Some(memory_id) = inventory.memory_id_for_record(&delivered.record_id) else {
                return (
                    500,
                    "{\"error\":\"selected Taste record lost its Cortex binding\"}".into(),
                );
            };
            let chars = delivered.rule.chars().count();
            full_chars += chars;
            injected_chars += chars;
            remaining_preview_chars = remaining_preview_chars.saturating_sub(chars);
            seen_ids.insert(memory_id.to_string());
            hits.push(serde_json::json!({
                "id": memory_id,
                "skel": delivered.rule,
                "type": "taste_preference",
                "scope": inventory.scope_for_record(&delivered.record_id).unwrap_or("global"),
                "kind": "taste_preference",
                "score": 1.0,
                "cos": 1.0,
                "origin": "taste",
                "delivery_receipt": delivered.receipt,
            }));
        }
        let generic_slots = k.saturating_sub(hits.len());
        let generic_candidates = if generic_slots == 0 {
            Vec::new()
        } else {
            store.recall_scored_detailed(
                query,
                generic_slots
                    .saturating_add(inventory.memory_ids.len())
                    .saturating_add(1),
                &chain,
            )
        };
        let generic_considered = generic_candidates.len();
        let mut generic_rank = 0usize;
        for hit in generic_candidates {
            if hits.len() >= k || inventory.memory_ids.contains(&hit.entry.id) {
                continue;
            }
            let e = hit.entry;
            let cos = hit.score;
            // Graduated top-1 (2026-07-05, E6): the top hit gets a bigger inline budget
            // ONLY when strongly relevant (cos >= 0.55, above the hook's 0.40 floor) —
            // usefulness delivered inline beats the fetch loop's friction, but an
            // out-of-domain top-1 must not inject a large irrelevant block.
            if remaining_preview_chars == 0 {
                break;
            }
            let desired = if generic_rank == 0 && cos >= 0.55 {
                top_preview_chars
            } else {
                preview_chars
            };
            let budget = desired.min(remaining_preview_chars);
            let skel = preview(&e.content, budget);
            full_chars += e.content.chars().count();
            let actual = skel.chars().count();
            injected_chars += actual;
            remaining_preview_chars = remaining_preview_chars.saturating_sub(actual);
            if !seen_ids.insert(e.id.clone()) {
                continue;
            }
            hits.push(serde_json::json!({
                "id": e.id, "skel": skel, "type": "memory",
                "scope": e.scope_id, "kind": "memory", "score": cos, "cos": cos,
                "origin": hit.origin,
            }));
            generic_rank += 1;
        }
        let ids: Vec<String> = hits
            .iter()
            .filter_map(|h| h.get("id").and_then(|x| x.as_str()).map(String::from))
            .collect();
        if observe {
            let context = event_context(&v, client);
            if let Err(e) = store.record_injections_observed(&ids, &context) {
                return (500, serde_json::json!({ "error": e }).to_string());
            }
        }
        let replay_hits = serde_json::Value::Array(
            hits.iter()
                .map(|hit| {
                    serde_json::json!({
                        "id": hit.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                        "score": hit.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        "origin": hit.get("origin").and_then(|v| v.as_str()).unwrap_or("semantic"),
                    })
                })
                .collect(),
        )
        .to_string();
        let log_result = store.log_recall_with_replay_to(
            if smoke_sink {
                crate::memdb::RecallLogSink::Smoke
            } else {
                crate::memdb::RecallLogSink::Production
            },
            &crate::time::now_iso(),
            scope.as_deref(),
            query.chars().count(),
            hits.len(),
            full_chars,
            injected_chars,
            "serve",
            Some(&query.chars().take(120).collect::<String>()),
            Some(client),
            v.get("session").and_then(|x| x.as_str()),
            v.get("cwd").and_then(|x| x.as_str()),
            v.get("hook_event").and_then(|x| x.as_str()),
            v.get("trace_id").and_then(|x| x.as_str()),
            v.get("client_visibility").and_then(|x| x.as_str()),
            traffic_class,
            &replay_hits,
            &replay_hits,
        );
        if smoke_sink {
            if let Err(error) = log_result {
                return (
                    500,
                    serde_json::json!({
                        "error": "smoke recall telemetry persistence failed",
                        "detail": error,
                    })
                    .to_string(),
                );
            }
        }
        // Delivery is only true once every other fallible pre-response step
        // has succeeded. Persist the complete decision immediately before the
        // response; a receipt failure still fails the request.
        if let Err(error) = store.persist_taste_delivery_receipts(
            &inventory,
            &delivery_plan.receipts,
            &crate::store::TasteDeliveryAttributionV1 {
                client: client.into(),
                session_id: v
                    .get("session")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                turn_id: v.get("turn_id").and_then(Value::as_str).map(str::to_string),
                trace_id: v
                    .get("trace_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
                traffic_class: traffic_class.into(),
            },
        ) {
            return (
                500,
                serde_json::json!({"error": "Taste receipt persistence failed", "detail": error})
                    .to_string(),
            );
        }
        let response_version = v
            .get("responseVersion")
            .or_else(|| v.get("response_version"))
            .and_then(Value::as_u64)
            .unwrap_or(1);
        let mut completeness = if hits.len() >= k || generic_considered > generic_rank {
            crate::store::CortexCompletenessV1::lower_bound(
                "ceiling_truncated",
                hits.len().saturating_add(1),
                hits.len(),
                1,
            )
        } else {
            crate::store::CortexCompletenessV1::exact(hits.len(), hits.len(), 0)
        };
        if remaining_preview_chars == 0 && generic_considered > generic_rank {
            completeness.state = crate::store::CortexCompletenessState::LowerBound;
            completeness.counts_exact = false;
            if !completeness
                .causes
                .iter()
                .any(|cause| cause == "preview_budget_exhausted")
            {
                completeness
                    .causes
                    .push("preview_budget_exhausted".to_owned());
            }
        }
        if hits.is_empty()
            && store.last_recall_status().as_deref() == Some("insufficient_confidence")
        {
            (
                200,
                serde_json::json!({
                    "status": "insufficient_confidence",
                    "hits": [],
                    "completeness": completeness,
                })
                .to_string(),
            )
        } else if response_version == 2 {
            (
                200,
                serde_json::json!({
                    "schemaVersion": 2,
                    "hits": hits,
                    "completeness": completeness,
                })
                .to_string(),
            )
        } else {
            (
                200,
                serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into()),
            )
        }
    } else if method == "POST" && path == "/search" {
        let query = v.get("query").and_then(Value::as_str).unwrap_or("").trim();
        if query.is_empty() {
            return (400, "{\"error\":\"query required\"}".to_string());
        }
        if query.chars().count() > MAX_QUERY_CHARS {
            return (413, "{\"error\":\"query too large\"}".to_string());
        }
        let limit = v
            .get("limit")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(6);
        let rows = store.search(query, limit);
        (
            200,
            serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string()),
        )
    } else if method == "POST" && path == "/activity" {
        let timestamp = v
            .get("timestamp")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(crate::time::now_iso);
        let scope = v.get("scope").and_then(Value::as_str);
        let query_chars = v
            .get("query_chars")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        let hit_count = v
            .get("hit_count")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        let full_chars = v
            .get("full_chars")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        let injected_chars = v
            .get("injected_chars")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        let source = v.get("source").and_then(Value::as_str).unwrap_or("http");
        let query_excerpt = v.get("query_excerpt").and_then(Value::as_str);
        let client = v.get("client").and_then(Value::as_str);
        let model = v.get("model").and_then(Value::as_str);
        let session = v.get("session").and_then(Value::as_str);
        let task = v.get("task").and_then(Value::as_str);
        let turn = v.get("turn").and_then(Value::as_str);
        let actor = v.get("actor").and_then(Value::as_str);
        let traffic_class = v
            .get("mode")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("production");
        let replay_hits = v.get("replay_hits").and_then(Value::as_str).unwrap_or("[]");
        let replay_context = v
            .get("replay_context")
            .and_then(Value::as_str)
            .unwrap_or(replay_hits);
        match store.log_recall_with_replay_to(
            crate::memdb::RecallLogSink::Production,
            &timestamp,
            scope,
            query_chars,
            hit_count,
            full_chars,
            injected_chars,
            source,
            query_excerpt,
            client,
            session,
            task,
            turn,
            actor,
            model.or(actor),
            traffic_class,
            replay_hits,
            replay_context,
        ) {
            Ok(()) => (200, "{\"ok\":true}".to_string()),
            Err(error) => (500, serde_json::json!({"error": error}).to_string()),
        }
    } else if method == "POST" && path == "/policy/assign" {
        let session = v
            .get("session")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let client = v
            .get("client")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let policy_version = v
            .get("policy_version")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let control_pct = v
            .get("control_pct")
            .and_then(|value| value.as_u64())
            .unwrap_or(10);
        let task_class = v
            .get("task_class")
            .and_then(|value| value.as_str())
            .filter(|value| {
                matches!(
                    *value,
                    "user_prompt_recall" | "code" | "research" | "writing" | "other"
                )
            })
            .unwrap_or("unknown");
        if control_pct > 100 {
            return (
                400,
                "{\"error\":\"control_pct must be between 0 and 100\"}".into(),
            );
        }
        match store.assign_context_policy(
            session,
            client,
            policy_version,
            control_pct as u8,
            task_class,
        ) {
            Ok(cohort) => (
                200,
                serde_json::json!({
                    "cohort": cohort,
                    "policy_version": policy_version,
                    "control_pct": control_pct,
                    "task_class": task_class,
                })
                .to_string(),
            ),
            Err(error) => (400, serde_json::json!({ "error": error }).to_string()),
        }
    } else if method == "POST" && path == "/curate" {
        let today = v
            .get("today")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| crate::time::now_iso().chars().take(10).collect());
        let context = event_context(&v, "curate");
        match store.dream_now_observed(&today, &context) {
            Ok(status) => (
                200,
                serde_json::json!({
                    "agent_id": status.agent_id,
                    "status": status.status,
                    "model": status.model,
                    "shell_allowed": status.shell_allowed,
                    "read_count": status.read_count,
                    "consolidated_count": status.consolidated_count,
                    "pruned_count": status.pruned_count,
                    "quarantined_count": status.quarantined_count,
                    "duplicate_quarantined_count": status.duplicate_quarantined_count,
                })
                .to_string(),
            ),
            Err(error) => (500, serde_json::json!({ "error": error }).to_string()),
        }
    } else if method == "POST" && path == "/quarantine/list" {
        (
            200,
            serde_json::json!({ "ids": store.quarantined_ids() }).to_string(),
        )
    } else if method == "POST" && path == "/quarantine/restore" {
        let id = match v.get("id").and_then(|value| value.as_str()) {
            Some(id) if !id.trim().is_empty() => id.trim(),
            _ => return (400, "{\"error\":\"missing id\"}".to_string()),
        };
        match store.restore_quarantined(id) {
            Ok(restored) => (
                200,
                serde_json::json!({ "id": id, "restored": restored }).to_string(),
            ),
            Err(error) => (500, serde_json::json!({ "error": error }).to_string()),
        }
    } else if method == "POST" && path == "/add" {
        (
            410,
            "{\"error\":\"/add disabled; use the cortex CLI for file ingestion\"}".to_string(),
        )
    } else if method == "POST" && path == "/use" {
        let id = match v.get("id").and_then(|x| x.as_str()) {
            Some(id) if !id.trim().is_empty() => id.trim(),
            _ => return (400, "{\"error\":\"missing id\"}".to_string()),
        };
        let context = event_context(&v, "http");
        match store.record_use_observed(id, &context) {
            Ok(n) => (200, format!("{{\"ok\":true,\"access_count\":{n}}}")),
            Err(e) => (500, format!("{{\"error\":{:?}}}", e)),
        }
    } else if method == "POST" && path == "/get" {
        let id = match v.get("id").and_then(|x| x.as_str()) {
            Some(id) if !id.trim().is_empty() => id.trim(),
            _ => return (400, "{\"error\":\"missing id\"}".to_string()),
        };
        let context = event_context(&v, "http");
        match store.get_full_observed(id, &context) {
            Ok((content, access_count)) => (
                200,
                serde_json::json!({ "id": id, "content": content, "access_count": access_count })
                    .to_string(),
            ),
            Err(e) => (404, format!("{{\"error\":{:?}}}", e)),
        }
    } else if method == "POST" && path == "/feedback" {
        let trace_id = v
            .get("trace_id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let candidate_id = v
            .get("candidate_id")
            .or_else(|| v.get("id"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let content_sha256 = v
            .get("content_sha256")
            .or_else(|| v.get("sha"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let outcome = match v
            .get("outcome")
            .and_then(|x| x.as_str())
            .map(crate::feedback::parse_outcome)
        {
            Some(Ok(o)) => o,
            _ => {
                return (
                    400,
                    "{\"error\":\"missing or invalid outcome\"}".to_string(),
                )
            }
        };
        let source = crate::feedback::parse_source(
            v.get("source")
                .and_then(|x| x.as_str())
                .unwrap_or("advisory"),
        );
        let verdict_ref = v
            .get("verdict_ref")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let scope_id = v
            .get("scope")
            .or_else(|| v.get("scope_id"))
            .and_then(|x| x.as_str())
            .unwrap_or("global")
            .to_string();
        let rec = crate::feedback::FeedbackRecord {
            trace_id,
            candidate_id,
            content_sha256,
            outcome,
            source,
            verdict_ref,
            scope_id,
        };
        match store.record_feedback(&rec) {
            Ok(()) => (
                200,
                format!("{{\"ok\":true,\"verified\":{}}}", rec.verified()),
            ),
            Err(e) => (400, format!("{{\"error\":{:?}}}", e)),
        }
    } else if method == "POST" && path == "/context/close-unknown" {
        let observed_since = v
            .get("observed_since")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        let observed_through = v
            .get("observed_through")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        let max_deliveries = v
            .get("max_deliveries")
            .and_then(|x| x.as_u64())
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        match store.close_unresolved_deliveries(observed_since, observed_through, max_deliveries) {
            Ok(closed) => (
                200,
                serde_json::json!({ "ok": true, "closed": closed }).to_string(),
            ),
            Err(error) => (400, serde_json::json!({ "error": error }).to_string()),
        }
    } else if method == "POST" && path == "/memory-candidates" {
        // Warm federation memory-candidate generation IN-PROCESS (the resident serve's embedder is
        // already loaded). The federation gateway's memory provider POSTs here instead of spawning
        // a cold `cortex memory-candidates` CLI, which reloads ONNX/fastembed (~3.6s) on every
        // prompt and blew the on-mode hook timeout. Same payload as the CLI verb.
        let task = v.get("task").and_then(|x| x.as_str()).unwrap_or("").trim();
        if task.is_empty() {
            return (400, "{\"error\":\"missing task\"}".to_string());
        }
        let descriptor = match v.get("scopeDescriptor") {
            Some(value) => {
                match serde_json::from_value::<crate::scope::ScopeDescriptorV1>(value.clone()) {
                    Ok(descriptor) => descriptor,
                    Err(error) => return (
                        400,
                        serde_json::json!({"error": format!("invalid scopeDescriptor: {error}")})
                            .to_string(),
                    ),
                }
            }
            None => crate::scope::ScopeDescriptorV1::filesystem(
                v.get("scope")
                    .and_then(|x| x.as_str())
                    .unwrap_or("D--Claude")
                    .trim(),
            ),
        };
        let max = v
            .get("max_candidates")
            .and_then(|x| x.as_u64())
            .unwrap_or(64) as usize;
        // F11: no `repoRoot` travels on this hot path (the federation gateway's `cortex.py`
        // provider never sends one — verified against `providers/cortex.py`), and this route's
        // whole reason to exist is the sub-350ms warm-serve budget the cold CLI blew (see the
        // comment above) — spending a real `git status` + graph-db read on every call to compute
        // a genuine freshness verdict would reintroduce that latency. Passing `None` here is the
        // honest choice per F11's contract: the candidate set's own freshness is unverifiable at
        // this call site, so `stale` reports that honestly instead of the old hardcoded `false`.
        let mut payload = match crate::pull::federation::memory_candidates_payload_for_descriptor(
            store,
            task,
            &descriptor,
            max,
            None,
        ) {
            Ok(payload) => payload,
            Err(error) => return (400, serde_json::json!({"error": error}).to_string()),
        };
        if let Some(stages) = payload
            .get_mut("_membrane")
            .and_then(|value| value.get_mut("stageElapsedMs"))
            .and_then(serde_json::Value::as_object_mut)
        {
            stages.insert("request_parse".to_string(), request_parse_ms.into());
        }
        (200, payload.to_string())
    } else if method == "POST" && path == "/verify-memory" {
        // Delivery provenance seal, client-agnostic: does `id` resolve to a real memory row whose
        // current content sha256 equals `sha`? Fail-closed. Both the Claude and Codex delivery
        // carve-outs call this before a memory preview reaches the model, so an unattested/forged
        // `agent_verified` label with content not in the trusted store is dropped. Warm, no `get`.
        let raw_id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").trim();
        let id = raw_id.strip_prefix("memory:role:").unwrap_or(raw_id);
        let sha = v
            .get("sha")
            .or_else(|| v.get("sourceHash"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        let ok =
            !id.is_empty() && sha.len() == 64 && store.content_sha256(id).as_deref() == Some(sha);
        (200, format!("{{\"ok\":{ok}}}"))
    } else if method == "POST" && path == "/compress" {
        let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("");
        let rate = v.get("rate").and_then(|x| x.as_f64()).unwrap_or(0.5) as f32;
        let no_onnx = v.get("no_onnx").and_then(|x| x.as_bool()).unwrap_or(false);
        let _ = (rate, no_onnx);
        // Legacy callers supplied no repository/session or resolver proof.
        // Preserve their output field but do not silently externalize authority
        // or discard bytes. Scoped clients use /push/prepare instead.
        (200, serde_json::json!({ "out": text, "disposition":"exact",
            "reason":"scoped_push_prepare_required_for_reduction" }).to_string())
    } else {
        (404, "{\"error\":\"unknown\"}".to_string())
    }
}

// ---- G3B planner surface --------------------------------------------------
//
// Planner routes are failure-isolated from memory routes. Each planner handler
// catches panics, timeouts, and SQLite failures and returns a bounded error
// payload WITHOUT mutating the `store`. The metrics + latency trackers are
// independent of the catalog — a planner crash leaves `get`/`put` entirely
// unaffected.

/// Top-level dispatcher that splits planner routes from memory routes.
#[allow(clippy::too_many_arguments)]
fn route_full(
    store: &MemoryStore,
    catalog: Option<&ContextCatalog>,
    context_ingest_lease: Option<&crate::context_telemetry::ContextIngestLease>,
    planner_latency: &crate::pull::metrics::PlannerLatency,
    planner_last_fallback: &crate::pull::metrics::LastFallback,
    planner_schema_error_count: &std::sync::atomic::AtomicU64,
    method: &str,
    url: &str,
    body: &str,
) -> (u16, String) {
    let path = route_path(url);
    let body_limit = if path == "/push/prepare" { MAX_PUSH_BODY_BYTES } else { MAX_BODY_BYTES };
    if body.len() > body_limit { return (413, "{\"error\":\"request body too large\"}".to_string()); }
    // `/health` returns planner metrics when the catalog is wired and falls
    // back to the legacy health body when it is not. The planner block lives
    // in its own try-catch so a planner panic still returns 200 with the
    // metrics block absent (not 500).
    if method == "GET" && path == "/health" {
        return health_response(
            store,
            catalog,
            planner_latency,
            planner_last_fallback,
            planner_schema_error_count,
        );
    }
    let is_planner_path = matches!(
        path,
        "/plan_context" | "/scope_grants" | "/__test/planner_panic" | "/__test/planner_slow"
    );
    if is_planner_path {
        return planner_route(
            catalog,
            planner_latency,
            planner_last_fallback,
            planner_schema_error_count,
            method,
            url,
            body,
        );
    }
    route_with_context_ingest_lease(store, context_ingest_lease, method, url, body)
}

fn health_response(
    store: &MemoryStore,
    catalog: Option<&ContextCatalog>,
    planner_latency: &crate::pull::metrics::PlannerLatency,
    planner_last_fallback: &crate::pull::metrics::LastFallback,
    planner_schema_error_count: &std::sync::atomic::AtomicU64,
) -> (u16, String) {
    health_response_with_workers(
        store,
        catalog,
        planner_latency,
        planner_last_fallback,
        planner_schema_error_count,
        None,
    )
}

fn health_response_with_workers(
    store: &MemoryStore,
    catalog: Option<&ContextCatalog>,
    planner_latency: &crate::pull::metrics::PlannerLatency,
    planner_last_fallback: &crate::pull::metrics::LastFallback,
    planner_schema_error_count: &std::sync::atomic::AtomicU64,
    workers: Option<&WorkerAdmission>,
) -> (u16, String) {
    let mut payload = match serde_json::from_str::<Value>(&store.detailed_health_json().to_string())
    {
        Ok(v) => v,
        Err(_) => json!({"ok": false, "error": "store health serialization failed"}),
    };
    // Native Hub identity is part of the binding contract. Keep these fields
    // exact and content-free so old/embedded health payloads cannot bind.
    payload["serviceId"] = json!("membrane-hub");
    payload["installationId"] = json!(store.installation_id());
    payload["cortexStoreId"] = json!(store.cortex_store_id());
    payload["protocolVersion"] = json!(1);
    payload["schemaVersion"] = json!(1);
    payload["nativeOnly"] = json!(true);
    payload["runtimeOrigin"] = json!(runtime_origin());
    payload["subsystems"] = json!(["pull", "push", "cortex", "blueprint", "ledger", "adapt"]);
    payload["capabilities"] = json!(["memory", "diagnostics"]);
    let store_healthy = payload.get("ok").and_then(Value::as_bool) == Some(true);
    let (count, p50, p95) = planner_latency.snapshot();
    let last_fb = planner_last_fallback.snapshot();
    let planner_block = json!({
        "samples": count,
        "p50Micros": p50,
        "p95Micros": p95,
        "lastFallback": if last_fb.is_set() {
            json!({
                "reason": last_fb.reason,
                "mode": last_fb.mode,
                "providerStatus": last_fb.provider_status,
                "atUnix": last_fb.at_unix,
            })
        } else {
            json!(null)
        },
        "receiptSchemaErrorCount": planner_schema_error_count.load(std::sync::atomic::Ordering::Relaxed),
    });
    let mut catalog_healthy = true;
    if let Some(catalog) = catalog {
        let catalog_block = match std::panic::catch_unwind(|| catalog::health_snapshot(catalog)) {
            Ok(v) => v,
            Err(_) => {
                json!({"schemaVersion": CATALOG_SCHEMA_VERSION, "error": "snapshot_panicked"})
            }
        };
        catalog_healthy = catalog_block.get("status").and_then(Value::as_str) == Some("ok");
        payload["catalog"] = catalog_block;
    } else {
        payload["catalog"] = Value::Null;
    }
    payload["planner"] = planner_block;
    payload["workers"] = workers.map_or(Value::Null, WorkerAdmission::snapshot);
    payload["dailyAnalysis"] = analysis_watchdog_snapshot(&configured_analysis_directory());
    payload["serviceGeneration"] = json!(crate::release_identity::service_generation());
    payload["releaseGeneration"] = json!(crate::release_identity::release_generation());
    payload["runtimeReceipt"] =
        crate::runtime_receipt::current_snapshot().map_or(Value::Null, |receipt| json!(receipt));
    let status = if store_healthy && catalog_healthy {
        StatusCode::OK.as_u16()
    } else {
        StatusCode::SERVICE_UNAVAILABLE.as_u16()
    };
    (status, payload.to_string())
}

fn runtime_origin() -> &'static str {
    runtime_origin_from(std::env::var("MEMBRANE_RUNTIME_ORIGIN").ok().as_deref())
}

fn runtime_origin_from(value: Option<&str>) -> &'static str {
    match value {
        Some("development") => "development",
        Some("installed") | None => "installed",
        Some(_) => "invalid",
    }
}

fn planner_route(
    catalog: Option<&ContextCatalog>,
    planner_latency: &crate::pull::metrics::PlannerLatency,
    planner_last_fallback: &crate::pull::metrics::LastFallback,
    planner_schema_error_count: &std::sync::atomic::AtomicU64,
    method: &str,
    url: &str,
    body: &str,
) -> (u16, String) {
    if body.len() > MAX_BODY_BYTES {
        return (413, "{\"error\":\"request body too large\"}".to_string());
    }
    let path = route_path(url);
    if method == "GET" && path == "/health" {
        // already handled in route_full, but keep for safety.
        return (200, json!({"ok": true}).to_string());
    }
    let Some(catalog) = catalog else {
        return (
            503,
            json!({
                "error": "context catalog not wired",
                "kind": "catalog_unavailable",
            })
            .to_string(),
        );
    };
    let v = if method == "POST" {
        match json_body(body) {
            Ok(v) => v,
            Err(resp) => return resp,
        }
    } else {
        Value::Null
    };
    if method == "POST" && path == "/scratchpad" {
        return (200, crate::scratchpad::handle(&v).to_string());
    }
    if method == "POST" && path == "/scope_grants" {
        return planner_post_scope_grants(catalog, &v);
    }
    if method == "POST" && path == "/plan_context" {
        return planner_post_plan_context(
            catalog,
            planner_latency,
            planner_last_fallback,
            planner_schema_error_count,
            &v,
        );
    }
    if path.starts_with("/__test/") {
        // Hidden fault-injection endpoints used only in `cargo test`. Never
        // registered on the public router outside test builds.
        #[cfg(test)]
        {
            if path == "/__test/planner_panic" {
                std::thread::sleep(Duration::from_millis(50));
                panic!("injected planner panic");
            }
            if path == "/__test/planner_slow" {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        return (404, json!({"error": "unknown_test_path"}).to_string());
    }
    (404, json!({"error": "unknown"}).to_string())
}

fn planner_post_scope_grants(catalog: &ContextCatalog, v: &Value) -> (u16, String) {
    let id = v.get("id").and_then(|s| s.as_str()).unwrap_or("").trim();
    if v.get("operation").and_then(|value| value.as_str()) == Some("lookup") {
        if id.is_empty() {
            return (400, json!({"error": "id required"}).to_string());
        }
        return match catalog::lookup_grant(catalog, id) {
            Ok(Some(grant)) => {
                let repository_root = grant
                    .repository_ids
                    .iter()
                    .find(|value| std::path::Path::new(value).is_absolute())
                    .cloned();
                (
                    200,
                    json!({
                        "id": grant.id,
                        "status": grant.status.as_str(),
                        "client": grant.client,
                        "taskId": grant.task_id,
                        "sessionId": grant.session_id,
                        "repositoryIds": grant.repository_ids,
                        "repositoryRoot": repository_root,
                        "manifestDigest": grant.manifest_digest,
                        "readPaths": grant.read_paths,
                        "nonce": grant.nonce,
                        "expiresAtUnix": grant.expires_at_unix,
                    })
                    .to_string(),
                )
            }
            Ok(None) => (404, json!({"error": "scope_grant not found"}).to_string()),
            Err(error) => (
                503,
                json!({"error": format!("catalog lookup failed: {error}")}).to_string(),
            ),
        };
    }
    let client = v
        .get("client")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .trim();
    let task_id = v
        .get("task_id")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .trim();
    let session_id = v
        .get("session_id")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .trim();
    let nonce = v.get("nonce").and_then(|s| s.as_str()).unwrap_or("").trim();
    let manifest_digest = v
        .get("manifest_digest")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let ttl_seconds = v
        .get("ttl_seconds")
        .and_then(|s| s.as_i64())
        .unwrap_or(1800);
    if id.is_empty()
        || client.is_empty()
        || task_id.is_empty()
        || session_id.is_empty()
        || nonce.len() < 8
        || !manifest_digest.starts_with("sha256:")
    {
        return (
            400,
            json!({
                "error": "id, client, task_id, session_id, nonce>=8 chars, manifest_digest=sha256:... required",
            })
            .to_string(),
        );
    }
    let repo_ids: Vec<String> = v
        .get("repository_ids")
        .and_then(|s| s.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if repo_ids.is_empty() {
        return (
            400,
            json!({"error": "repository_ids must be non-empty"}).to_string(),
        );
    }
    let edges: Vec<String> = v
        .get("permitted_edge_types")
        .and_then(|s| s.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let read_paths_value = v.get("read_paths").or_else(|| v.get("readPaths"));
    let read_paths = match read_paths_value {
        None => Vec::new(),
        Some(Value::Array(items)) => {
            if items.len() > 256 {
                return (400, json!({"error":"read_paths exceeds maximum of 256 ranges"}).to_string());
            }
            let mut parsed = Vec::with_capacity(items.len());
            for item in items {
                let Some(path) = item.get("path").and_then(Value::as_str).filter(|value| !value.trim().is_empty()) else {
                    return (400, json!({"error":"read_paths[].path required"}).to_string());
                };
                if path.len() > 4096 || path != path.trim() || path.chars().any(char::is_control) {
                    return (400, json!({"error":"read_paths[].path invalid"}).to_string());
                }
                let start_line = item.get("startLine").or_else(|| item.get("start_line")).and_then(Value::as_u64);
                let end_line = item.get("endLine").or_else(|| item.get("end_line")).and_then(Value::as_u64);
                let (Some(start_line), Some(end_line)) = (start_line, end_line) else {
                    return (400, json!({"error":"read_paths[] start/end lines required"}).to_string());
                };
                if start_line == 0 || end_line < start_line || end_line > u32::MAX as u64 {
                    return (400, json!({"error":"read_paths[] line range invalid"}).to_string());
                }
                parsed.push(membrane_protocol::ReadPathV1 {
                    path: path.to_owned(),
                    start_line: start_line as u32,
                    end_line: end_line as u32,
                });
            }
            parsed
        }
        Some(_) => return (400, json!({"error":"read_paths must be an array"}).to_string()),
    };
    let grant = match catalog::issue_scope_grant(
        catalog,
        id,
        client,
        &repo_ids,
        &edges,
        &read_paths,
        task_id,
        session_id,
        ttl_seconds,
        nonce,
        manifest_digest,
    ) {
        Ok(grant) => grant,
        Err(e) => {
            return (
                500,
                json!({"error": format!("catalog insert failed: {e}"), "kind": "catalog_error"})
                    .to_string(),
            );
        }
    };
    (
        200,
        json!({
            "id": grant.id,
            "status": "active",
            "expiresAtUnix": grant.expires_at_unix,
            "client": grant.client,
        })
        .to_string(),
    )
}

#[allow(clippy::type_complexity)]
fn planner_post_plan_context(
    catalog: &ContextCatalog,
    planner_latency: &crate::pull::metrics::PlannerLatency,
    planner_last_fallback: &crate::pull::metrics::LastFallback,
    planner_schema_error_count: &std::sync::atomic::AtomicU64,
    v: &Value,
) -> (u16, String) {
    let started = Instant::now();
    let result: std::thread::Result<Result<(Value, String, Option<String>), (u16, String)>> =
        std::panic::catch_unwind(|| -> Result<_, _> {
            let grant_id = v
                .get("scope_grant_id")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            if grant_id.is_empty() {
                return Err((
                    400,
                    json!({
                        "error": "scope_grant_id required",
                        "kind": "scope_grant_required",
                    })
                    .to_string(),
                ));
            }
            let grant = match catalog::lookup_grant(catalog, grant_id) {
                Ok(Some(g)) => g,
                Ok(None) => {
                    return Err((
                        403,
                        json!({
                            "error": "scope_grant not found",
                            "kind": "scope_grant_missing",
                        })
                        .to_string(),
                    ));
                }
                Err(e) => {
                    return Err((
                        503,
                        json!({
                            "error": format!("catalog lookup failed: {e}"),
                            "kind": "catalog_error",
                        })
                        .to_string(),
                    ));
                }
            };
            if !grant.permits() {
                return Err((
                    403,
                    json!({
                        "error": "scope_grant inactive (revoked/expired)",
                        "kind": "scope_grant_inactive",
                        "status": format!("{:?}", grant.status),
                    })
                    .to_string(),
                ));
            }
            let max_tokens = v.get("max_tokens").and_then(|s| s.as_u64()).unwrap_or(0) as usize;
            if max_tokens == 0 {
                return Err((
                    400,
                    json!({"error": "max_tokens must be >= 1", "kind": "zero_budget"}).to_string(),
                ));
            }
            const MAX_SAFE_PACKET_CHAR_BUDGET: u64 = 9_007_199_254_740_991;
            let packet_char_budget_override = match v.get("packet_char_budget_override") {
                None => None,
                Some(value) => match value
                    .as_u64()
                    .filter(|value| *value > 0 && *value <= MAX_SAFE_PACKET_CHAR_BUDGET)
                    .and_then(|value| usize::try_from(value).ok())
                {
                    Some(value) => Some(value),
                    None => {
                        return Err((
                            400,
                            json!({
                                "error": "packet_char_budget_override must be a positive safe integer",
                                "kind": "invalid_packet_char_budget",
                            })
                            .to_string(),
                        ));
                    }
                },
            };
            let candidate_obj = v.get("candidate_set").cloned().unwrap_or(Value::Null);
            let ccs: ContextCandidateSetV1 = match serde_json::from_value(candidate_obj) {
                Ok(c) => c,
                Err(e) => {
                    return Err((
                        400,
                        json!({
                            "error": format!("candidate_set invalid: {e}"),
                            "kind": "candidate_set_invalid",
                        })
                        .to_string(),
                    ));
                }
            };
            let input = PlannerInput {
                candidate_set: ccs.clone(),
                max_tokens,
                packet_char_budget_override,
                packet_char_budget_model: v
                    .get("packet_char_budget_model")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
                    .filter(|value| !value.trim().is_empty()),
                accepted_receipt_versions: vec![2],
                trace_id_override: None,
                scope_grant_present: true,
                consumer_resolvers: Vec::new(),
            };
            let out = match plan_context(&input) {
                Ok(out) => out,
                Err(e) => {
                    return Err((
                        400,
                        json!({
                            "error": planner_error_message(&e),
                            "kind": planner_error_kind(&e),
                        })
                        .to_string(),
                    ));
                }
            };
            let trace_id = out.packet.trace_id.clone();
            // Persist structured retrieval event + each receipt.
            let fallback_mode_str = format!("{:?}", out.fallback_mode).to_lowercase();
            let provider_status_str = format!("{:?}", out.provider_status).to_lowercase();
            let degradation_str = format!("{:?}", out.degradation_reason).to_lowercase();
            let _ = catalog::record_retrieval_event(
                catalog,
                &trace_id,
                &grant.client,
                &ccs.mode,
                &ccs.provider,
                &provider_status_str,
                &fallback_mode_str,
                &degradation_str,
                out.source_generation.as_deref(),
                out.packet.blocks.len(),
                ccs.candidates.len(),
            );
            for receipt in &out.receipts {
                let serialised = match serde_json::to_string(receipt) {
                    Ok(s) => s,
                    Err(_) => {
                        planner_schema_error_count
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        continue;
                    }
                };
                let sha = sha256_hex(&serialised);
                let _ = catalog::record_receipt(
                    catalog,
                    &format!("{}|{}", receipt.trace_id, receipt.id),
                    &receipt.trace_id,
                    &receipt.id,
                    &receipt.decision,
                    &receipt.reason,
                    &receipt.provider,
                    &provider_status_str,
                    &fallback_mode_str,
                    &degradation_str,
                    &sha,
                );
            }
            // Compact summary: the full packet and receipts are echo'd back.
            // The packet IS the budget-allocated context; the receipts are the
            // per-candidate audit trail.
            let packet = serde_json::to_value(&out.packet).unwrap_or(Value::Null);
            let receipts = serde_json::to_value(&out.receipts).unwrap_or(Value::Null);
            let payload = json!({
                "packet": packet,
                "receipts": receipts,
                "providerStatus": provider_status_str,
                "fallbackMode": fallback_mode_str,
                "degradationReason": degradation_str,
                "sourceGeneration": out.source_generation,
                "structuredEvent": serde_json::to_value(&out.structured_event).unwrap_or(Value::Null),
                "persistedReceipts": out.receipts.len(),
                "scopeGrant": {
                    "id": grant.id,
                    "client": grant.client,
                },
            });
            Ok((payload, provider_status_str, out.source_generation.clone()))
        });

    let elapsed = started.elapsed();
    planner_latency.record(elapsed);
    match result {
        Ok(Ok((payload, provider_status, source_generation))) => {
            // Roll forward the global fallback record so /health surfaces it.
            if let Some(degradation) = payload.get("degradationReason").and_then(|v| v.as_str()) {
                let mode = payload
                    .get("fallbackMode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("none");
                if degradation != "none" {
                    planner_last_fallback.record(degradation, mode, &provider_status);
                }
            }
            let _ = source_generation;
            (200, payload.to_string())
        }
        Ok(Err((status, body))) => (status, body),
        Err(_panic) => (
            500,
            json!({
                "error": "planner panicked",
                "kind": "planner_panic",
            })
            .to_string(),
        ),
    }
}

fn planner_error_message(e: &PlannerError) -> String {
    match e {
        PlannerError::UnknownSchemaVersion { found, supported } => {
            format!("unknown schemaVersion {found}; supported: {supported:?}")
        }
        PlannerError::EmptyTask => "candidate set task is empty".into(),
        PlannerError::EmptyProvider => "candidate set provider is empty".into(),
        PlannerError::ReceiptVersionUnsupported { accepted } => {
            format!("caller accepted receipt versions {accepted:?}; planner emits v2 only")
        }
        PlannerError::ZeroBudget => "max_tokens must be >= 1".into(),
    }
}

fn planner_error_kind(e: &PlannerError) -> &'static str {
    match e {
        PlannerError::UnknownSchemaVersion { .. } => "unknown_schema_version",
        PlannerError::EmptyTask => "empty_task",
        PlannerError::EmptyProvider => "empty_provider",
        PlannerError::ReceiptVersionUnsupported { .. } => "receipt_version_unsupported",
        PlannerError::ZeroBudget => "zero_budget",
    }
}

fn sha256_hex(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    hasher.update(s.as_bytes());
    let digest = hasher.finalize();
    for byte in digest {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

/// Open the DB and serve the contract forever on IPv4 loopback only.
///
/// Opens a separate catalog SQLite at `<context-home>/catalog.db` for G3B
/// planner routes. The catalog lives on its own connection — the Cortex DB
/// is untouched. `CONTEXT_HOME` overrides the catalog parent directory.
pub(crate) fn run(
    db_path: &str,
    port: u16,
    identity: &crate::installation_identity::InstallationIdentity,
    claim: &crate::installation_identity::StartupClaim,
) -> Result<(), String> {
    let db_path = std::path::Path::new(db_path);
    if !db_path.is_absolute() {
        return Err(format!("CORTEX_DB must be absolute: {}", db_path.display()));
    }
    let catalog_path = crate::catalog::resolve_catalog_path_from(
        std::env::var_os("MEMBRANE_CATALOG"),
        std::env::var_os("CONTEXT_HOME"),
        Some(db_path.as_os_str().to_os_string()),
        std::env::var_os("WORKSPACE_ROOT"),
    )
    .map_err(|error| error.to_string())?;
    let workspace_root =
        crate::runtime_receipt::resolve_workspace_root(std::env::var_os("WORKSPACE_ROOT"), db_path)
            .map_err(|error| error.to_string())?;
    let configured_event_db = crate::runtime_receipt::validate_telemetry_event_binding(
        std::env::var_os("MEMBRANE_EVENT_DB"),
    )
    .map_err(|error| error.to_string())?;
    #[cfg(feature = "fastembed")]
    {
        let ort_path = std::env::var_os("ORT_DYLIB_PATH")
            .ok_or_else(|| "ORT_DYLIB_PATH is required for fastembed".to_string())?;
        ort::init_from(std::path::PathBuf::from(ort_path))
            .map_err(|error| format!("initialize ONNX Runtime: {error}"))?
            .commit();
    }
    let store = MemoryStore::try_open(MemDb::open(db_path).map_err(|e| e.to_string())?)?;
    let telemetry_event_db = store.db().event_db_path().ok_or_else(|| {
        "resident Cortex store has no physical telemetry event database".to_string()
    })?;
    if configured_event_db
        .as_deref()
        .is_some_and(|configured| configured != telemetry_event_db)
    {
        return Err("resident Cortex telemetry event binding changed during startup".to_string());
    }
    let runtime_receipt = crate::runtime_receipt::RuntimeReceiptV2::new(
        &workspace_root,
        db_path,
        &catalog_path,
        telemetry_event_db,
        identity,
        claim,
    )
    .map_err(|error| error.to_string())?;
    let context_ingest_lease =
        crate::context_telemetry::ContextIngestLease::from_startup(identity, claim)
            .map_err(|error| format!("prepare telemetry ingest lease: {error}"))?;
    let prompt_telemetry_db = store.db().clone();
    let prompt_telemetry_lease = context_ingest_lease.clone();
    let prompt_telemetry_ingress =
        crate::context_telemetry::default_prompt_telemetry_ingress(db_path);
    let prompt_telemetry_running = Arc::new(AtomicBool::new(true));
    let prompt_telemetry_thread_running = prompt_telemetry_running.clone();
    let prompt_telemetry_thread = std::thread::Builder::new()
        .name("cortex-prompt-telemetry".to_string())
        .spawn(move || {
            let mut failed = false;
            while prompt_telemetry_thread_running.load(Ordering::Acquire) {
                match crate::context_telemetry::drain_prompt_telemetry_ingress(
                    &prompt_telemetry_db,
                    &prompt_telemetry_lease,
                    &prompt_telemetry_ingress,
                ) {
                    Ok(_) => {
                        if failed {
                            eprintln!("cortex prompt telemetry drain recovered");
                        }
                        failed = false;
                    }
                    Err(_) => {
                        if !failed {
                            eprintln!("cortex prompt telemetry drain unavailable");
                        }
                        failed = true;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        })
        .map_err(|error| format!("start prompt telemetry drain: {error}"))?;
    let prompt_telemetry_worker = PromptTelemetryWorker {
        running: prompt_telemetry_running,
        thread: Some(prompt_telemetry_thread),
    };
    let catalog = ContextCatalog::open(&catalog_path)
        .map_err(|e| format!("open catalog {}: {e}", catalog_path.display()))?;
    let runtime_receipt_path = runtime_receipt
        .persist()
        .map_err(|error| error.to_string())?;
    std::env::set_var("MEMBRANE_RUNTIME_RECEIPT", &runtime_receipt_path);
    crate::runtime_receipt::publish_current(runtime_receipt);
    eprintln!(
        "membrane resident on 127.0.0.1:{port} db={} catalog={}",
        db_path.display(),
        catalog_path.display()
    );
    let api_token = Some(configured_api_token(db_path)?);

    // Active Hub owns native MCP execution. The stdio binary is only a
    // transport client back to this authenticated in-process route.
    crate::mcp_executor::install_native_mcp_executor_for_hub(store.clone())?;
    let mcp_host = format!("127.0.0.1:{port}");
    let mcp_policy = membrane_mcp::http_security::HttpAdmissionPolicy::local(
        identity.installation_id.clone(),
        mcp_host.clone(),
        format!("http://{mcp_host}"),
        api_token.clone().unwrap_or_default(),
        claim.service_instance_id.clone(),
    );

    let app = build_router(
        store,
        Some(catalog),
        Some(context_ingest_lease),
        port,
        api_token,
        REQUEST_TIMEOUT,
        MAX_CONCURRENT_REQUESTS,
    )
    .merge(crate::mcp_http::build_mcp_http_router(mcp_policy));
    let lifecycle = crate::service::lifecycle_control().clone();
    let server_result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?
        .block_on(async move {
            let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .map_err(|error| error.to_string())?;
            lifecycle.mark_ready(port);
            let shutdown = lifecycle.clone();
            let server = std::future::IntoFuture::into_future(
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
                )
                .with_graceful_shutdown(async move {
                    while !shutdown.shutdown_requested() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }),
            );
            tokio::pin!(server);
            let shutdown_observer = lifecycle.clone();
            tokio::select! {
                result = &mut server => result.map_err(|error| error.to_string()),
                _ = async move {
                    while !shutdown_observer.shutdown_requested() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                } => {
                    match tokio::time::timeout(Duration::from_secs(5), &mut server).await {
                        Ok(result) => result.map_err(|error| error.to_string()),
                        Err(_) => {
                            lifecycle.fail("lifecycle drain timeout");
                            Err("lifecycle drain timeout".to_string())
                        }
                    }
                }
            }
        });

    let prompt_telemetry_result = prompt_telemetry_worker.stop();
    match (server_result, prompt_telemetry_result) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

// ============================================================================
// MBR-102 entrypoints: serve over stdio JSON-RPC for MCP clients, or over
// loopback HTTP for the Hub and CLI. The legacy `run` signature stays as the
// long-form entrypoint used by `service::run_service`.
// ============================================================================

/// MBR-102: serve the same JSON-RPC surface as the loopback API, but over
/// stdio. The framing is line-delimited JSON (one request per line, one
/// response per line) so any MCP client can talk to it without buffering. The
/// function blocks until the client closes its end of the pipe.
pub fn run_stdio_mcp() -> Result<(), String> {
    crate::mcp_executor::install_native_mcp_transport()?;
    membrane_mcp::serve_stdio().map_err(|error| format!("serve native stdio MCP: {error}"))
}

#[cfg(test)]
mod tests {
    include!("adapt_path_tests.rs");
    use super::*;

    #[test]
    fn push_prepare_body_limit_is_larger_without_widening_other_routes() {
        assert_eq!(MAX_BODY_BYTES, 1 << 20);
        assert_eq!(MAX_PUSH_BODY_BYTES, 8 << 20);
        let push = if "/push/prepare" == "/push/prepare" { MAX_PUSH_BODY_BYTES } else { MAX_BODY_BYTES };
        let other = if "/memory-candidates" == "/push/prepare" { MAX_PUSH_BODY_BYTES } else { MAX_BODY_BYTES };
        assert_eq!(push, MAX_PUSH_BODY_BYTES);
        assert_eq!(other, MAX_BODY_BYTES);
    }

    #[test]
    fn runtime_origin_is_explicit_and_fail_closed() {
        assert_eq!(runtime_origin_from(None), "installed");
        assert_eq!(runtime_origin_from(Some("installed")), "installed");
        assert_eq!(runtime_origin_from(Some("development")), "development");
        assert_eq!(runtime_origin_from(Some("unexpected")), "invalid");
    }

    #[test]
    fn native_identity_fence_requires_exact_hub_identity() {
        let store = MemoryStore::new();
        let mut headers = HeaderMap::new();
        assert!(native_identity_fence_valid(&headers, &store).is_ok());
        headers.insert(
            "x-membrane-installation-id",
            store.installation_id().parse().unwrap(),
        );
        headers.insert(
            "x-membrane-cortex-store-id",
            store.cortex_store_id().parse().unwrap(),
        );
        headers.insert(
            "x-membrane-release-generation",
            crate::release_identity::release_generation()
                .parse()
                .unwrap(),
        );
        assert!(native_identity_fence_valid(&headers, &store).is_ok());
        headers.insert(
            "x-membrane-release-generation",
            "stale-generation".parse().unwrap(),
        );
        assert!(native_identity_fence_valid(&headers, &store).is_err());
        headers.insert(
            "x-membrane-release-generation",
            crate::release_identity::release_generation()
                .parse()
                .unwrap(),
        );
    }

    fn opaque_taste_scope(dimensions: &[(&str, &str)]) -> String {
        let dimensions = dimensions
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<BTreeMap<_, _>>();
        let normalized = membrane_adapt::scope::ScopeDimensions::normalize(&dimensions).unwrap();
        let canonical = normalized
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        let digest = membrane_adapt::canonical::sha256_canonical(
            &serde_json::to_value(canonical).expect("scope dimensions serialize"),
        );
        format!("dimensions:{}", &digest[..24])
    }

    fn put_taste(
        store: &MemoryStore,
        id: &str,
        rule: &str,
        class: &str,
        scope: &str,
        dimensions: &[(&str, &str)],
        influence: &str,
        lifecycle: &str,
    ) -> String {
        put_taste_for_machine(
            store, id, rule, class, scope, dimensions, influence, lifecycle, None,
        )
    }

    fn put_taste_for_machine(
        store: &MemoryStore,
        id: &str,
        rule: &str,
        class: &str,
        scope: &str,
        dimensions: &[(&str, &str)],
        influence: &str,
        lifecycle: &str,
        machine: Option<&str>,
    ) -> String {
        let mut record = membrane_adapt::manifest::ManifestRecord {
            id: id.into(),
            rule: rule.into(),
            category: "workflow".into(),
            scope: scope.into(),
            scope_dimensions: membrane_adapt::manifest::BTreeMap2(
                dimensions
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
            ),
            record_type: class.into(),
            evidence_class: "user_authoritative".into(),
            authority_effect: "neutral".into(),
            status: "accepted".into(),
            confidence: 1.0,
            needs_review: influence != "behavioral_directive",
            evidence_count: 1,
            created_at: "2026-08-26T00:00:00Z".into(),
            updated_at: "2026-08-26T00:00:00Z".into(),
            evidence_excerpt: "user preference".into(),
            source_ids: Vec::new(),
            source_file_hashes: Vec::new(),
            evidence_ids: Vec::new(),
            retrieval_aliases: Vec::new(),
            human_note: String::new(),
            payload_sha256: String::new(),
            operation: "add".into(),
            machine: machine.unwrap_or_default().into(),
            machine_only: machine.is_some(),
            lifecycle_state: lifecycle.into(),
            last_verified_at: "2026-08-26T00:00:00Z".into(),
            verification_count: 1,
            authority_manifest_sha256: "authority".into(),
            validator_receipt_id: "validator".into(),
            validator_receipt_sha256: "validator-sha".into(),
            semantic_payload: None,
            semantic_digest: String::new(),
            evidence_contexts: Vec::new(),
        };
        membrane_adapt::manifest::seal_manifest_record(&mut record, "canonical-pool").unwrap();
        record.payload_sha256 = membrane_adapt::manifest::payload_sha256(&record);
        let sealed_influence = match record
            .semantic_payload
            .as_ref()
            .expect("test Taste record is sealed")
            .influence_class
        {
            membrane_adapt::record::InfluenceClass::BehavioralDirective => "behavioral_directive",
            membrane_adapt::record::InfluenceClass::Provisional => "provisional",
            membrane_adapt::record::InfluenceClass::ReferenceOnly => "reference",
        };
        let memory_id = format!("{}/{}", normalize_scope(scope), id);
        store
            .try_put_test_verified_adapt_batch(&crate::store::MemoryBatchRequest {
                batch_id: format!("taste-{id}"),
                items: vec![crate::store::MemoryBatchItem {
                    item_id: id.into(),
                    name: id.into(),
                    content: serde_json::to_string(&record).unwrap(),
                    scope: scope.into(),
                    tier: "Semantic".into(),
                    artifact_family: "adapt".into(),
                    producer: "adapt_native".into(),
                    record_type: "taste_preference".into(),
                    client: "membrane_adapt".into(),
                    session_id: format!("session-{id}"),
                    turn_id: String::new(),
                    trace_id: format!("trace-{id}"),
                    source_ids: Vec::new(),
                    lifecycle: crate::store::MemoryLifecycleInputV1 {
                        authority: Some("A2".into()),
                        influence_class: Some(sealed_influence.into()),
                        confidence: Some(1.0),
                        confidence_basis: Some("test".into()),
                        ..Default::default()
                    },
                }],
            })
            .unwrap();
        if lifecycle != "active" {
            store
                .db()
                .lock()
                .execute(
                    "UPDATE memories SET lifecycle_state=?1 WHERE id=?2",
                    rusqlite::params![lifecycle, memory_id],
                )
                .unwrap();
        }
        memory_id
    }

    fn memory_batch_body(
        suffix: &str,
        artifact_family: &str,
        producer: &str,
        record_type: &str,
        client: &str,
        authority: &str,
        influence_class: &str,
    ) -> String {
        serde_json::json!({
            "batch_id": format!("batch-{suffix}"),
            "items": [{
                "item_id": format!("item-{suffix}"),
                "name": format!("memory-{suffix}"),
                "content": "Focused batch admission regression.",
                "scope": "global",
                "tier": "Semantic",
                "artifact_family": artifact_family,
                "producer": producer,
                "record_type": record_type,
                "client": client,
                "session_id": format!("session-{suffix}"),
                "trace_id": format!("trace-{suffix}"),
                "authority": authority,
                "influenceClass": influence_class,
            }]
        })
        .to_string()
    }

    fn stored_memory_count(store: &MemoryStore) -> i64 {
        store
            .db()
            .lock()
            .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn generic_memory_batch_rejects_every_reserved_adapt_authority_claim() {
        let cases = [
            (
                "adapt-family",
                "adapt",
                "manual",
                "memory",
                "generic",
                "A3",
                "reference",
            ),
            (
                "adapt-producer",
                "memory",
                "adapt_native",
                "memory",
                "generic",
                "A3",
                "reference",
            ),
            (
                "taste-type",
                "memory",
                "manual",
                "taste_preference",
                "generic",
                "A3",
                "reference",
            ),
            (
                "adapt-client",
                "memory",
                "manual",
                "memory",
                "membrane_adapt",
                "A3",
                "reference",
            ),
            (
                "a2-directive",
                "memory",
                "manual",
                "memory",
                "generic",
                "A2",
                "behavioral_directive",
            ),
        ];

        for (suffix, family, producer, record_type, client, authority, influence) in cases {
            let store = MemoryStore::new();
            let response = route(
                &store,
                "POST",
                "/v1/memories:batch",
                &memory_batch_body(
                    suffix,
                    family,
                    producer,
                    record_type,
                    client,
                    authority,
                    influence,
                ),
            );

            assert_eq!(response.0, 403, "case {suffix}: {}", response.1);
            let payload: Value = serde_json::from_str(&response.1).unwrap();
            assert_eq!(
                payload["code"], RESERVED_ADAPT_ADMISSION_REASON,
                "case {suffix}"
            );
            assert_eq!(stored_memory_count(&store), 0, "case {suffix}");
        }
    }

    #[test]
    fn generic_memory_batch_still_accepts_non_reserved_attribution() {
        let store = MemoryStore::new();
        let response = route(
            &store,
            "POST",
            "/v1/memories:batch",
            &memory_batch_body(
                "ordinary",
                "memory",
                "manual",
                "memory",
                "generic",
                "A2",
                "reference",
            ),
        );

        assert_eq!(response.0, 201, "{}", response.1);
        let payload: Value = serde_json::from_str(&response.1).unwrap();
        assert_eq!(payload["inserted"], 1);
        assert_eq!(stored_memory_count(&store), 1);
    }

    #[test]
    fn native_adapt_storage_path_remains_available_to_sealed_records() {
        let store = MemoryStore::new();
        let memory_id = put_taste(
            &store,
            "native-adapt",
            "Always run focused verification.",
            "standing_preference",
            "global",
            &[],
            "behavioral_directive",
            "active",
        );

        assert_eq!(memory_id, "global/native-adapt");
        assert_eq!(stored_memory_count(&store), 1);
    }

    async fn wait_for_worker_snapshot(app: &Router, predicate: impl Fn(&Value) -> bool) -> Value {
        use axum::body::{to_bytes, Body};
        use axum::http::Request;
        use tower::ServiceExt;

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let response = app
                    .clone()
                    .oneshot(Request::get("/livez").body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let payload: Value = serde_json::from_slice(
                    &to_bytes(response.into_body(), MAX_BODY_BYTES)
                        .await
                        .unwrap(),
                )
                .unwrap();
                if predicate(&payload) {
                    return payload;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("worker state did not reach the required rendezvous")
    }

    async fn post_json_with_key(app: Router, path: &str, body: &str, key: &str) -> Response {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        app.oneshot(
            Request::post(path)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer test-api-token")
                .header("Idempotency-Key", key)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
    }

    fn hold_memdb_connection(
        db: MemDb,
    ) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _connection = db.lock();
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
        });
        entered_rx.recv().unwrap();
        (release_tx, holder)
    }

    fn hold_catalog_connection(
        catalog: ContextCatalog,
    ) -> (std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(0);
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _connection = catalog.lock();
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
        });
        entered_rx.recv().unwrap();
        (release_tx, holder)
    }

    #[test]
    fn diagnostics_executor_drop_does_not_wait_for_wedged_job() {
        let executor = DiagnosticsExecutor::new();
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        executor
            .submit(Box::new(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }))
            .unwrap();
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("diagnostics worker did not start held job");

        let (dropped_tx, dropped_rx) = std::sync::mpsc::sync_channel(0);
        let dropper = std::thread::spawn(move || {
            drop(executor);
            let _ = dropped_tx.send(());
        });
        let dropped_while_held = dropped_rx.recv_timeout(Duration::from_secs(1));

        release_tx.send(()).unwrap();
        dropper.join().unwrap();
        assert!(
            dropped_while_held.is_ok(),
            "executor drop blocked on a wedged diagnostics job"
        );
    }

    #[test]
    fn diagnostics_executor_drop_joins_after_normal_worker_exit() {
        let executor = DiagnosticsExecutor::new();
        let (completed_tx, completed_rx) = std::sync::mpsc::sync_channel(1);
        executor
            .submit(Box::new(move || {
                let _ = completed_tx.send(());
            }))
            .unwrap();

        drop(executor);

        assert!(
            completed_rx.try_recv().is_ok(),
            "executor drop returned before its normally completing job ran"
        );
    }

    #[test]
    fn idempotency_registry_retries_the_same_key_after_a_retryable_failure() {
        let registry = Arc::new(IdempotencyRegistry::new(1));
        let key = [1; 32];
        let request = [2; 32];
        let lease = match registry.begin(key, request) {
            IdempotencyDecision::Execute(lease) => lease,
            _ => panic!("first idempotent request was not admitted"),
        };

        lease.complete(503, "temporarily unavailable");

        assert!(matches!(
            registry.begin(key, request),
            IdempotencyDecision::Execute(_)
        ));
    }

    #[tokio::test]
    async fn test_gate_unreleased_enter_exits_without_parking_a_blocking_worker() {
        let gate = Arc::new(TestGate::default());
        let worker_gate = Arc::clone(&gate);
        let mut worker = tokio::task::spawn_blocking(move || worker_gate.enter());
        gate.wait_started(1).await;

        let completed = tokio::time::timeout(Duration::from_secs(2), &mut worker).await;
        let finished_before_release = gate.finished.load(std::sync::atomic::Ordering::Acquire);
        gate.release(1);
        if completed.is_err() {
            worker.await.unwrap();
        }

        assert!(
            matches!(completed, Ok(Ok(()))),
            "an unreleased test gate parked a blocking worker past its hard timeout"
        );
        assert_eq!(
            finished_before_release, 0,
            "an unreleased test gate must escape rather than report completion"
        );
    }

    #[test]
    fn http_route_contract_exhaustively_classifies_model_work() {
        let mut unique = std::collections::HashSet::new();
        for spec in HTTP_ROUTE_SPECS {
            assert!(unique.insert((spec.0, spec.1)), "duplicate route: {spec:?}");
        }
        let model_routes: Vec<_> = HTTP_ROUTE_SPECS
            .iter()
            .filter(|spec| spec.2 == HttpWorkClass::Model)
            .map(|spec| (spec.0, spec.1))
            .collect();
        assert_eq!(
            model_routes,
            vec![
                ("POST", "/v1/memories:batch"),
                ("POST", "/put"),
                ("POST", "/remember"),
                ("POST", "/remember_consolidated"),
                ("POST", "/recall"),
                ("POST", "/search"),
                ("POST", "/curate"),
                ("POST", "/memory-candidates"),
                ("POST", "/compress"),
            ]
        );
    }

    #[test]
    fn http_route_registry_matches_every_implemented_public_handler() {
        let source = include_str!("serve.rs");
        let dispatch_start = source
            .find("fn route_with_context_ingest_lease(")
            .expect("memory dispatcher");
        let dispatch_end = source
            .find("\nfn planner_post_scope_grants")
            .expect("planner dispatcher end");
        let dispatch = &source[dispatch_start..dispatch_end];
        let mut implemented = std::collections::HashSet::new();
        let mut cursor = dispatch;
        while let Some(method_at) = cursor.find("method == \"") {
            cursor = &cursor[method_at + "method == \"".len()..];
            let method_end = cursor.find('"').expect("handler method terminator");
            let method = &cursor[..method_end];
            let condition = &cursor[method_end..cursor.len().min(method_end + 160)];
            let Some(path_at) = condition.find("path == \"") else {
                continue;
            };
            let path = &condition[path_at + "path == \"".len()..];
            let path_end = path.find('"').expect("handler path terminator");
            implemented.insert((method, &path[..path_end]));
        }
        // Adapt uses constants in the production conditions. Verify those
        // actual conditions before adding them to the parsed route inventory.
        assert!(
            dispatch.contains("method == \"POST\" && path == crate::adapt_service::OPERATOR_PATH")
        );
        assert!(dispatch
            .contains("method == \"POST\" && path == crate::adapt_service::OBSERVATION_PATH"));
        implemented.insert(("POST", crate::adapt_service::OPERATOR_PATH));
        implemented.insert(("POST", crate::adapt_service::OBSERVATION_PATH));
        // Root/index share one condition; snapshot, livez, and scratchpad are handled before dispatch.
        implemented.insert(("GET", "/index.html"));
        implemented.insert(("GET", "/snapshot"));
        implemented.insert(("GET", "/livez"));
        implemented.insert(("POST", "/scratchpad"));
        implemented.insert(("POST", "/scratchpad/session-close"));

        let registered: std::collections::HashSet<_> = HTTP_ROUTE_SPECS
            .iter()
            .map(|spec| (spec.0, spec.1))
            .collect();
        assert_eq!(
            registered, implemented,
            "public handler and HTTP route registry diverged"
        );
    }

    #[tokio::test]
    async fn context_close_unknown_is_registered_at_http_boundary() {
        use axum::body::{to_bytes, Body};
        use axum::http::{header, Request, StatusCode};
        use tower::ServiceExt;

        let app = router_for_tests_with_policy(
            MemoryStore::new(),
            8765,
            Some(TEST_API_TOKEN.to_string()),
            Duration::from_secs(1),
            MAX_CONCURRENT_REQUESTS,
        );
        let response = app
            .oneshot(
                Request::post("/context/close-unknown")
                    .header(header::AUTHORIZATION, "Bearer test-api-token")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"observed_since":"2026-07-20T07:59:00Z","observed_through":"2026-07-20T08:01:00Z","max_deliveries":10}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), MAX_BODY_BYTES)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&body).contains(r#""closed":0"#),
            "body: {}",
            String::from_utf8_lossy(&body)
        );
    }

    #[test]
    fn idempotency_registry_bounds_running_entries_and_scopes_digests() {
        assert!(valid_idempotency_key("visible-key_1"));
        assert!(!valid_idempotency_key(""));
        assert!(!valid_idempotency_key("contains space"));
        assert!(!valid_idempotency_key(&"x".repeat(129)));
        assert_ne!(
            idempotency_key_digest("same", Some("token-a")),
            idempotency_key_digest("same", Some("token-b"))
        );
        assert_ne!(
            idempotency_request_digest(&Method::POST, "/put?mode=a", "{}"),
            idempotency_request_digest(&Method::POST, "/put?mode=b", "{}")
        );

        let registry = Arc::new(IdempotencyRegistry::new(1));
        let first = match registry.begin([1; 32], [2; 32]) {
            IdempotencyDecision::Execute(lease) => lease,
            _ => panic!("first entry must execute"),
        };
        assert!(matches!(
            registry.begin([3; 32], [4; 32]),
            IdempotencyDecision::Full
        ));
        drop(first);
        assert!(matches!(
            registry.begin([3; 32], [4; 32]),
            IdempotencyDecision::Execute(_)
        ));
    }

    #[test]
    fn health_exposes_boot_and_release_generations() {
        let response = health_response(
            &MemoryStore::new(),
            None,
            &crate::pull::metrics::PlannerLatency::new(),
            &crate::pull::metrics::LastFallback::new(),
            &std::sync::atomic::AtomicU64::new(0),
        );
        let payload: serde_json::Value = serde_json::from_str(&response.1).unwrap();

        assert!(payload["serviceGeneration"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:")));
        assert_eq!(
            payload["releaseGeneration"],
            format!(
                "sha256:{}",
                crate::release_identity::source_tree_sha256().unwrap_or("unknown")
            )
        );
        assert_ne!(payload["serviceGeneration"], payload["releaseGeneration"]);
        assert!(payload.get("runtimeReceipt").is_some());
        assert_eq!(payload["serviceId"], "membrane-hub");
        assert_eq!(payload["protocolVersion"], 1);
        assert_eq!(payload["schemaVersion"], 1);
        assert!(payload["installationId"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        assert!(payload["cortexStoreId"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:")));
        assert_eq!(payload["nativeOnly"], true);
        assert_eq!(payload["subsystems"].as_array().map(Vec::len), Some(6));
        assert_eq!(payload["capabilities"], json!(["memory", "diagnostics"]));
    }

    #[test]
    fn production_timeout_allows_slow_embeddings_and_precedes_cli_deadline() {
        assert!(REQUEST_TIMEOUT >= Duration::from_secs(90));
        assert!(REQUEST_TIMEOUT < Duration::from_secs(120));
    }

    #[test]
    fn feedback_route_persists_verified_and_rejects_bad_outcome() {
        let store = MemoryStore::new();
        // A verified observed contradiction persists and is marked verified.
        let (code, body) = route_for_tests(
            &store,
            "POST",
            "/feedback",
            r#"{"trace_id":"t1","candidate_id":"mem-1","sha":"abc","outcome":"contradicted","source":"observed_action"}"#,
        );
        assert_eq!(code, 200, "body: {body}");
        assert!(body.contains("\"verified\":true"), "body: {body}");

        // A malformed outcome is rejected.
        let (code, _) = route_for_tests(
            &store,
            "POST",
            "/feedback",
            r#"{"trace_id":"t1","candidate_id":"mem-2","sha":"abc","outcome":"bogus"}"#,
        );
        assert_eq!(code, 400);

        // An advisory contradiction persists but is NOT verified (cannot rank).
        let (code, body) = route_for_tests(
            &store,
            "POST",
            "/feedback",
            r#"{"trace_id":"t2","candidate_id":"mem-3","sha":"abc","outcome":"contradicted","source":"advisory"}"#,
        );
        assert_eq!(code, 200);
        assert!(body.contains("\"verified\":false"), "body: {body}");

        let rows = store.feedback_rows_from_db();
        assert_eq!(
            rows.len(),
            2,
            "verified + advisory rows persisted; malformed dropped"
        );
        let gate = cortex_core::EffectivenessGate::default();
        assert!(
            !gate.should_inject(&rows, "mem-1"),
            "verified contradicted vetoes"
        );
        assert!(
            gate.should_inject(&rows, "mem-3"),
            "advisory contradiction must not veto"
        );
    }

    #[test]
    fn latest_analysis_json_returns_newest_report() {
        let dir = tempfile::tempdir().unwrap();
        let generated_at = crate::time::now_iso();
        std::fs::write(
            dir.path().join("2026-07-10.json"),
            serde_json::json!({
                "generated_at": generated_at,
                "goal": {"status": "baseline_only"}
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("2026-07-11.json"),
            serde_json::json!({
                "generated_at": crate::time::now_iso(),
                "goal": {"status": "collecting_cohorts"}
            })
            .to_string(),
        )
        .unwrap();

        let report = latest_analysis_json(dir.path()).unwrap();

        assert_eq!(report["goal"]["status"], "collecting_cohorts");
    }

    #[test]
    fn analysis_response_is_explicit_when_report_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();

        let response = analysis_response(dir.path());

        assert_eq!(response.0, 503);
        assert!(response.1.contains("analysis unavailable"));
    }

    #[test]
    fn analysis_response_rejects_a_stale_report_when_the_scheduler_never_runs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("2000-01-01.json"),
            r#"{"generated_at":"2000-01-01T10:00:00Z","goal":{"status":"baseline_only"}}"#,
        )
        .unwrap();

        let response = analysis_response(dir.path());

        assert_eq!(response.0, 503);
        assert!(response.1.contains("analysis stale"));
    }

    #[test]
    fn analysis_watchdog_reports_content_free_last_success_age() {
        let dir = tempfile::tempdir().unwrap();
        let today = crate::time::now_iso();
        std::fs::write(
            dir.path().join("current.json"),
            serde_json::json!({"generated_at": today, "secret": "must-not-leak"}).to_string(),
        )
        .unwrap();

        let snapshot = analysis_watchdog_snapshot(dir.path());

        assert_eq!(snapshot["status"], "fresh");
        assert_eq!(snapshot["alert"], false);
        assert!(snapshot["lastSuccessAgeSeconds"].as_u64().is_some());
        assert!(snapshot.get("secret").is_none());
        assert!(snapshot.get("path").is_none());
    }

    #[test]
    fn analysis_watchdog_alerts_when_the_daily_job_never_produces_output() {
        let dir = tempfile::tempdir().unwrap();

        let snapshot = analysis_watchdog_snapshot(dir.path());

        assert_eq!(snapshot["status"], "unavailable");
        assert_eq!(snapshot["alert"], true);
        assert!(snapshot["lastSuccessAgeSeconds"].is_null());
    }

    #[test]
    fn skills_snapshot_route_returns_the_engine_generation_and_no_bodies() {
        let store = MemoryStore::new();
        let expected = store.skills_generation().unwrap();

        let (status, body) = route_for_tests(&store, "POST", "/skills-snapshot", "{}");

        assert_eq!(status, 200);
        let payload: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(payload["schemaVersion"], 1);
        assert_eq!(payload["generation"], expected);
        assert_eq!(payload["skills"], serde_json::json!([]));
        assert!(payload.get("body").is_none());
    }

    /// Serializes access to process-global `MEMBRANE_PORT`
    /// env vars that `hub_inputs::live_inputs_from_local_service` reads, the same
    /// pattern `hub_inputs.rs`'s own test module uses for its env-touching tests.
    static HUB_ROUTE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Binds a background TCP listener that answers exactly one `GET /health`
    /// with the given JSON body, mimicking the local Membrane resident well enough
    /// for `hub_inputs::live_inputs_from_local_service` to parse it as healthy.
    /// Returns the bound port; the listener thread exits after serving one request.
    fn spawn_mock_health_server(health_json: &'static str) -> u16 {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock health server");
        let port = listener.local_addr().expect("local_addr").port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let body = health_json;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        port
    }

    #[test]
    fn hub_capabilities_route_differs_between_healthy_and_unreachable_local_service() {
        let _guard = HUB_ROUTE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let store = MemoryStore::new();

        // Unreachable: pick a port nothing is listening on.
        unsafe {
            std::env::set_var("MEMBRANE_PORT", "1");
        }
        let (code_down, body_down) = route_for_tests(&store, "GET", "/hub/capabilities", "");
        assert_eq!(code_down, 200, "body: {body_down}");
        let payload_down: serde_json::Value = serde_json::from_str(&body_down).unwrap();
        assert!(payload_down.get("stream").is_none(), "body: {body_down}");

        // Healthy: a real listener answering /health.
        let port = spawn_mock_health_server(
            r#"{"ok":true,"catalog":{"status":"ok"},"database":{"status":"ok"},"dailyAnalysis":{"status":"fresh","alert":false}}"#,
        );
        unsafe {
            std::env::set_var("MEMBRANE_PORT", port.to_string());
        }
        let (code_up, body_up) = route_for_tests(&store, "GET", "/hub/capabilities", "");
        unsafe {
            std::env::remove_var("MEMBRANE_PORT");
        }
        assert_eq!(code_up, 200, "body: {body_up}");
        let payload_up: serde_json::Value = serde_json::from_str(&body_up).unwrap();
        assert_eq!(
            payload_up["stream"]["state"], "available",
            "body: {body_up}"
        );

        assert_ne!(
            payload_down, payload_up,
            "capabilities response must differ by backend health"
        );
    }

    #[test]
    fn hub_snapshot_route_differs_between_healthy_and_unreachable_local_service() {
        let _guard = HUB_ROUTE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let store = MemoryStore::new();

        unsafe {
            std::env::set_var("MEMBRANE_PORT", "1");
        }
        let (code_down, body_down) = route_for_tests(&store, "GET", "/hub/snapshot", "");
        assert_eq!(code_down, 200, "body: {body_down}");
        let payload_down: serde_json::Value = serde_json::from_str(&body_down).unwrap();
        assert_eq!(payload_down["productId"], "membrane", "body: {body_down}");
        assert_eq!(
            payload_down["sections"]["providers"]["state"], "unavailable",
            "body: {body_down}"
        );
        // Canonical snapshot carries typed parent state and six subsystems on
        // every path, including the offline fallback.
        assert_eq!(
            payload_down["membraneState"], "offline",
            "body: {body_down}"
        );
        let down_subsystems = payload_down["subsystems"].as_object().unwrap();
        assert_eq!(down_subsystems.len(), 6, "body: {body_down}");
        for name in membrane_protocol::SUBSYSTEM_NAMES {
            assert!(down_subsystems.contains_key(name), "body: {body_down}");
        }
        let snapshot_fields = payload_down.as_object().unwrap();
        assert_eq!(snapshot_fields.len(), 6, "body: {body_down}");
        for field in [
            "schemaVersion",
            "productId",
            "observedAtUnixMs",
            "sections",
            // Parent/subsystem status composition (Hub-status repair).
            "membraneState",
            "subsystems",
        ] {
            assert!(snapshot_fields.contains_key(field), "body: {body_down}");
        }

        let port = spawn_mock_health_server(
            r#"{"ok":true,"catalog":{"status":"ok"},"database":{"status":"ok"},"dailyAnalysis":{"status":"fresh","alert":false}}"#,
        );
        unsafe {
            std::env::set_var("MEMBRANE_PORT", port.to_string());
        }
        let (code_up, body_up) = route_for_tests(&store, "GET", "/hub/snapshot", "");
        unsafe {
            std::env::remove_var("MEMBRANE_PORT");
        }
        assert_eq!(code_up, 200, "body: {body_up}");
        let payload_up: serde_json::Value = serde_json::from_str(&body_up).unwrap();

        assert_ne!(
            payload_down["sections"]["providers"], payload_up["sections"]["providers"],
            "snapshot providers section must differ by backend health: down={payload_down} up={payload_up}"
        );
        // Healthy resident + live snapshot => Running regardless of Blueprint's
        // independently observed state; parent state is never child-derived.
        assert_eq!(payload_up["membraneState"], "running", "body: {body_up}");
        let blueprint_state = payload_up["subsystems"]["blueprint"]["state"]
            .as_str()
            .expect("Blueprint state is a closed string");
        assert!(
            matches!(blueprint_state, "available" | "degraded" | "unavailable"),
            "body: {body_up}"
        );
    }

    #[test]
    fn membrane_snapshot_is_closed_v2_while_hub_snapshot_stays_v1() {
        let _guard = HUB_ROUTE_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            std::env::set_var("MEMBRANE_PORT", "1");
        }

        let snapshot = membrane_snapshot_v2().unwrap();
        unsafe {
            std::env::remove_var("MEMBRANE_PORT");
        }

        assert_eq!(snapshot["schemaVersion"], 2);
        assert_eq!(snapshot["productId"], "membrane");
        let sections = snapshot["sections"].as_object().unwrap();
        assert!(!sections.is_empty() && sections.len() <= SNAPSHOT_MAX_SECTIONS);
        for section in sections.values() {
            assert!(matches!(
                section["state"].as_str(),
                Some("available" | "degraded" | "unavailable")
            ));
            assert!(!section["reason"].as_str().unwrap().is_empty());
            assert!(section["reason"].as_str().unwrap().len() <= SNAPSHOT_MAX_REASON_BYTES);
            let items = section["items"].as_array().unwrap();
            assert!(items.len() <= SNAPSHOT_MAX_ITEMS_PER_SECTION);
            for item in items {
                let fields = item.as_object().unwrap();
                assert_eq!(fields.len(), 8);
                for field in [
                    "label",
                    "kind",
                    "count",
                    "severity",
                    "evidence",
                    "resolver",
                    "observedAtUnixMs",
                    "stale",
                ] {
                    assert!(fields.contains_key(field));
                }
            }
        }

        let store = MemoryStore::new();
        let (_, hub) = route_for_tests(&store, "GET", "/hub/snapshot", "");
        assert_eq!(
            serde_json::from_str::<Value>(&hub).unwrap()["schemaVersion"],
            1
        );
    }

    #[tokio::test]
    async fn transport_enforces_auth_origin_json_and_body_limits() {
        use axum::body::Body;
        use axum::http::{header, Request, StatusCode};
        use tower::ServiceExt;

        let store = MemoryStore::new();
        let app = router_for_tests_with_policy(
            store.clone(),
            8765,
            Some("top-secret".to_string()),
            std::time::Duration::from_secs(2),
            MAX_CONCURRENT_REQUESTS,
        );

        let public_health = app
            .clone()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(public_health.status(), StatusCode::OK);

        let missing_auth = app
            .clone()
            .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(missing_auth.status(), StatusCode::UNAUTHORIZED);

        let missing_freshness_auth = app
            .clone()
            .oneshot(
                Request::post("/freshness")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"repoRoot":"."}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_freshness_auth.status(), StatusCode::UNAUTHORIZED);

        let bad_origin = app
            .clone()
            .oneshot(
                Request::get("/metrics")
                    .header(header::AUTHORIZATION, "Bearer top-secret")
                    .header(header::ORIGIN, "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);

        let missing_json = app
            .clone()
            .oneshot(
                Request::post("/list")
                    .header(header::AUTHORIZATION, "Bearer top-secret")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_json.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let oversized = app
            .clone()
            .oneshot(
                Request::post("/list")
                    .header(header::AUTHORIZATION, "Bearer top-secret")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(vec![b'x'; MAX_BODY_BYTES + 1]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            oversized.headers()[header::CONTENT_TYPE],
            "application/json; charset=utf-8"
        );
        let conn = store.db().lock();
        for reason in ["invalid_content_type", "request_too_large"] {
            assert_eq!(
                conn.query_row(
                    "SELECT COUNT(*) FROM context_event_log
                      WHERE phase='provider.terminal' AND status='failed' AND reason_code=?1",
                    [reason],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
                1,
                "expected one typed terminal for {reason}"
            );
        }
        drop(conn);

        let allowed = app
            .clone()
            .oneshot(
                Request::get("/metrics")
                    .header(header::AUTHORIZATION, "Bearer top-secret")
                    .header(header::ORIGIN, "http://localhost:8765")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);

        let dashboard = app
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(dashboard.status(), StatusCode::OK);
        assert_eq!(
            dashboard.headers()[header::CONTENT_TYPE],
            "text/html; charset=utf-8"
        );
        assert_eq!(dashboard.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(dashboard.headers()[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(dashboard.headers()[header::X_FRAME_OPTIONS], "DENY");
        assert!(dashboard
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("connect-src 'self'"));
        let html = axum::body::to_bytes(dashboard.into_body(), MAX_BODY_BYTES)
            .await
            .unwrap();
        let html = String::from_utf8(html.to_vec()).unwrap();
        assert!(!html.contains("top-secret"));
        assert!(html.contains("let MEMBRANE_API_TOKEN = dashboardToken();"));
        assert!(!html.contains("__MEMBRANE_API_TOKEN_JSON__"));
        assert!(html.contains("new URLSearchParams(location.hash.slice(1))"));
        assert!(html
            .contains("history.replaceState(null, '', `${location.pathname}${location.search}`)"));
        assert!(html.contains("sessionStorage.setItem(DASHBOARD_TOKEN_KEY"));
        assert!(html.contains("api('/graph')"));
        assert!(html.contains("recenterGraph"));
        assert!(html.contains("api('/analysis')"));
        assert!(html.contains("id=\"learning-kpis\""));
        assert!(html
            .contains("#learning-detail { border-top:1px solid var(--border); overflow-x:auto; }"));
    }

    #[tokio::test]
    async fn resident_freshness_returns_refresh_pending_without_waiting_for_git() {
        use axum::body::Body;
        use axum::http::{header, Request, StatusCode};
        use tower::ServiceExt;

        let workspace = std::env::current_dir().unwrap().canonicalize().unwrap();
        let repo = tempfile::Builder::new()
            .prefix("resident-freshness-off-path-")
            .tempdir_in(&workspace)
            .unwrap();
        let app = router_for_tests_with_policy(
            MemoryStore::new(),
            8765,
            Some(TEST_API_TOKEN.to_string()),
            std::time::Duration::from_secs(2),
            MAX_CONCURRENT_REQUESTS,
        );
        let body = serde_json::json!({
            "repoRoot": repo.path().canonicalize().unwrap(),
            "sessionId": "resident-freshness-off-path-session",
            "worktreePath": repo.path().canonicalize().unwrap().to_string_lossy(),
        })
        .to_string();
        let started = Instant::now();
        let response = app
            .oneshot(
                Request::post("/freshness")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, "Bearer test-api-token")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(started.elapsed() < Duration::from_millis(100));
        let payload = axum::body::to_bytes(response.into_body(), MAX_BODY_BYTES)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(value["graphState"], "indeterminate");
        assert_eq!(value["refreshInFlight"], true);
        assert!(value["reasons"]
            .as_array()
            .is_some_and(|reasons| reasons.iter().any(|reason| reason == "refresh_pending")));
    }

    /// F19 — a missing `sessionId` must reject the request, never silently fall back to a
    /// shared `"freshness-http"` identity on the source-barrier receipt.
    #[test]
    fn freshness_rejects_a_request_missing_session_id() {
        let store = MemoryStore::new();
        let workspace = configured_workspace_root();
        let repo = tempfile::Builder::new()
            .prefix("freshness-missing-session-")
            .tempdir_in(&workspace)
            .unwrap();
        let response = route(
            &store,
            "POST",
            "/freshness",
            &serde_json::json!({
                "repoRoot": repo.path().canonicalize().unwrap(),
                "worktreePath": "some/worktree",
            })
            .to_string(),
        );
        assert_eq!(response.0, 400, "{}", response.1);
        let payload: Value = serde_json::from_str(&response.1).unwrap();
        assert!(payload["error"]
            .as_str()
            .is_some_and(|error| error.contains("sessionId")));
    }

    /// F19 — a missing `worktreePath` must reject the request, never silently fall back to the
    /// raw `repoRoot` string as the overlay identity.
    #[test]
    fn freshness_rejects_a_request_missing_worktree_path() {
        let store = MemoryStore::new();
        let workspace = configured_workspace_root();
        let repo = tempfile::Builder::new()
            .prefix("freshness-missing-worktree-")
            .tempdir_in(&workspace)
            .unwrap();
        let response = route(
            &store,
            "POST",
            "/freshness",
            &serde_json::json!({
                "repoRoot": repo.path().canonicalize().unwrap(),
                "sessionId": "session-under-test",
            })
            .to_string(),
        );
        assert_eq!(response.0, 400, "{}", response.1);
        let payload: Value = serde_json::from_str(&response.1).unwrap();
        assert!(payload["error"]
            .as_str()
            .is_some_and(|error| error.contains("worktreePath")));
    }

    #[test]
    fn missing_token_file_is_generated_once_without_exposing_weak_material() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth").join("cortex.token");
        let first = token_from_file_or_create(&path).unwrap();
        let second = token_from_file_or_create(&path).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[tokio::test]
    async fn router_without_a_token_rejects_private_routes() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let app = router_for_tests_with_policy(
            MemoryStore::new(),
            8765,
            None,
            Duration::from_secs(1),
            MAX_CONCURRENT_REQUESTS,
        );
        let response = app
            .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn anchor_retrieve_is_authenticated_and_confined_to_workspace_repo() {
        use axum::body::Body;
        use axum::http::{header, Request, StatusCode};
        use tower::ServiceExt;

        let workspace = configured_workspace_root().canonicalize().unwrap();
        let repo = tempfile::Builder::new()
            .prefix("anchor-retrieve-")
            .tempdir_in(&workspace)
            .unwrap();
        std::fs::write(repo.path().join("note.txt"), "anchored content\n").unwrap();
        let app = router_for_tests_with_policy(
            MemoryStore::new(),
            8765,
            Some(TEST_API_TOKEN.to_string()),
            Duration::from_secs(1),
            MAX_CONCURRENT_REQUESTS,
        );
        let body = json!({"repo": repo.path(), "anchor": "note.txt", "maxBytes": 1024}).to_string();

        let unauthenticated = app
            .clone()
            .oneshot(
                Request::post("/anchor/retrieve")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

        let success = app
            .clone()
            .oneshot(
                Request::post("/anchor/retrieve")
                    .header(header::AUTHORIZATION, "Bearer test-api-token")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(success.status(), StatusCode::OK);
        let payload: Value = serde_json::from_slice(
            &axum::body::to_bytes(success.into_body(), MAX_BODY_BYTES)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(payload["path"], "note.txt");
        assert_eq!(payload["content"], "anchored content\n");
        assert_eq!(payload["truncated"], false);
        assert_eq!(payload["sha256"], sha256_bytes(b"anchored content\n"));

        let sibling = tempfile::Builder::new()
            .prefix("anchor-outside-")
            .tempdir_in(&workspace)
            .unwrap();
        std::fs::write(sibling.path().join("outside.txt"), "outside").unwrap();
        let traversal = app
            .clone()
            .oneshot(
                Request::post("/anchor/retrieve")
                    .header(header::AUTHORIZATION, "Bearer test-api-token")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "repo": repo.path(),
                            "anchor": format!("../{}/outside.txt", sibling.path().file_name().unwrap().to_string_lossy()),
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(traversal.status(), StatusCode::FORBIDDEN);

        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("outside.txt"), "outside").unwrap();
        let response = app
            .oneshot(
                Request::post("/anchor/retrieve")
                    .header(header::AUTHORIZATION, "Bearer test-api-token")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"repo": outside.path(), "anchor": "outside.txt"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn expand_anchor_recovers_exact_content_and_rejects_missing() {
        let root = tempfile::tempdir().unwrap();
        let content = "exact anchor content\n";
        let digest = sha256_bytes(content.as_bytes());
        std::fs::write(root.path().join(format!("{digest}.log")), content).unwrap();
        let (status, body) = expand_anchor_response(
            &json!({"anchor": format!("mr://anchor/{digest}")}).to_string(),
            root.path(),
        );
        assert_eq!(status, 200);
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["content"], content);
        assert_eq!(value["sha256"], digest);
        let (status, _) = expand_anchor_response(
            &json!({"anchor": format!("mr://anchor/{}", "0".repeat(64))}).to_string(),
            root.path(),
        );
        assert_eq!(status, 404);
        std::fs::write(
            root.path().join(format!("{digest}.json")),
            json!({"expiresAtMillis": 1}).to_string(),
        )
        .unwrap();
        let (status, _) = expand_anchor_response(
            &json!({"anchor": format!("mr://anchor/{digest}")}).to_string(),
            root.path(),
        );
        assert_eq!(status, 410);

        let unavailable = root.path().join("missing-anchor-store");
        let (status, _) = expand_anchor_response(
            &json!({"anchor": format!("MR://anchor/{}", "0".repeat(64))}).to_string(),
            &unavailable,
        );
        assert_eq!(status, 400);
        let (status, _) = expand_anchor_response(
            &json!({"anchor": format!("mr://anchor/{}", "0".repeat(64))}).to_string(),
            &unavailable,
        );
        assert_eq!(status, 503);
    }

    #[tokio::test]
    async fn absent_token_configuration_generates_a_default_token_beside_the_database() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("cortex.db");
        let fallback = database.parent().unwrap().join("api-token");

        let token = configured_api_token_from_sources(None, None, &fallback).unwrap();

        assert_eq!(token.len(), 64);
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(std::fs::read_to_string(fallback).unwrap().trim(), token);

        let app = router_for_tests_with_policy(
            MemoryStore::new(),
            8765,
            Some(token),
            Duration::from_secs(1),
            MAX_CONCURRENT_REQUESTS,
        );
        let response = app
            .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stalled_request_times_out_without_consuming_the_service() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = router_for_tests_with_policy(
            MemoryStore::new(),
            port,
            None,
            Duration::from_millis(100),
            MAX_CONCURRENT_REQUESTS,
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let mut stalled = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        stalled
            .write_all(
                b"POST /recall HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{",
            )
            .await
            .unwrap();
        let mut timed_out = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), stalled.read_to_end(&mut timed_out))
            .await
            .expect("stalled body must receive a timeout response")
            .unwrap();
        assert!(
            String::from_utf8_lossy(&timed_out).contains(" 408 "),
            "response: {}",
            String::from_utf8_lossy(&timed_out)
        );

        let mut healthy = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        healthy
            .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut health = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), healthy.read_to_end(&mut health))
            .await
            .expect("health must remain responsive")
            .unwrap();
        assert!(String::from_utf8_lossy(&health).contains("\"ok\":true"));
        server.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn saturated_workload_ingress_sheds_without_waiting() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let (app, control) =
            router_for_tests_with_control(MemoryStore::new(), Duration::from_millis(100), 1);
        let first_app = app.clone();
        let first = tokio::spawn(async move {
            first_app
                .oneshot(
                    Request::get("/__test/slow-general")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        });
        control.workload.wait_started(1).await;

        let second = app
            .clone()
            .oneshot(
                Request::get("/__test/slow-general")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        control.workload.release(1);
        control.workload.wait_finished(1).await;
        assert_eq!(first.await.unwrap().status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timed_out_blocking_worker_does_not_take_health_down() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let (app, control) =
            router_for_tests_with_control(MemoryStore::new(), Duration::from_millis(100), 1);
        let first_app = app.clone();
        let first = tokio::spawn(async move {
            first_app
                .oneshot(Request::get("/__test/slow").body(Body::empty()).unwrap())
                .await
                .unwrap()
        });
        control.workload.wait_started(1).await;

        assert_eq!(first.await.unwrap().status(), StatusCode::REQUEST_TIMEOUT);

        // The timed-out async waiter is gone, but the injected blocking worker is
        // deliberately still running. Health must bypass workload admission so an
        // operator can distinguish saturation from a dead process.
        let health = app
            .clone()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let payload: Value =
            serde_json::from_slice(&to_bytes(health.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();
        assert_eq!(payload["workers"]["detachedRunning"], 1);

        control.workload.release(1);
        control.workload.wait_finished(1).await;
        let recovered_payload =
            wait_for_worker_snapshot(&app, |payload| payload["workers"]["detachedRunning"] == 0)
                .await;
        assert_eq!(recovered_payload["workers"]["detachedRunning"], 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn detailed_health_timeout_is_retryable_and_livez_stays_available() {
        use axum::body::Body;
        use axum::http::{header, Request, StatusCode};
        use tower::ServiceExt;

        let (app, control) =
            router_for_tests_with_control(MemoryStore::new(), Duration::from_millis(1_500), 1);

        let health_app = app.clone();
        let health = tokio::spawn(async move {
            health_app
                .oneshot(
                    Request::get("/health?__test_slow=1")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        });
        control.diagnostics.wait_started(1).await;
        let health = health.await.unwrap();
        assert_eq!(health.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(health.headers()[header::RETRY_AFTER], "1");

        let busy = app
            .clone()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(busy.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(busy.headers()[header::RETRY_AFTER], "1");

        let livez = app
            .clone()
            .oneshot(Request::get("/livez").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(livez.status(), StatusCode::OK);
        control.diagnostics.release(1);
        control.diagnostics.wait_finished(1).await;
        wait_for_worker_snapshot(&app, |payload| {
            payload["workers"]["diagnostics"]["available"] == 1
        })
        .await;
        let recovered = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(recovered.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn detailed_health_is_unavailable_when_store_is_unhealthy() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        db.lock()
            .execute_batch(
                "CREATE TRIGGER fail_health_put BEFORE INSERT ON memories
                 BEGIN SELECT RAISE(ABORT, 'health test failure'); END;",
            )
            .unwrap();
        assert!(store
            .try_put(
                "health-test",
                "force a persistence failure",
                "global",
                cortex_core::MemoryTier::Semantic,
            )
            .is_err());

        let app = build_router(store, None, None, 8765, None, Duration::from_secs(1), 1);
        let health = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::SERVICE_UNAVAILABLE);
        let payload: Value =
            serde_json::from_slice(&to_bytes(health.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();
        assert_eq!(payload["ok"], false);
        assert!(payload["last_persist_error"].is_string());
    }

    #[tokio::test]
    async fn detailed_health_fails_closed_when_memdb_is_busy_now() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        let (release, holder) = hold_memdb_connection(db);
        let app = build_router(store, None, None, 8765, None, Duration::from_secs(1), 1);
        let health = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        release.send(()).unwrap();
        holder.join().unwrap();

        assert_eq!(health.status(), StatusCode::SERVICE_UNAVAILABLE);
        let payload: Value =
            serde_json::from_slice(&to_bytes(health.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["database"]["status"], "busy");
    }

    #[tokio::test]
    async fn detailed_health_fails_closed_when_memdb_probe_errors_now() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        db.lock().execute("DROP TABLE memories", []).unwrap();
        let app = build_router(store, None, None, 8765, None, Duration::from_secs(1), 1);
        let health = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(health.status(), StatusCode::SERVICE_UNAVAILABLE);
        let payload: Value =
            serde_json::from_slice(&to_bytes(health.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["database"]["status"], "error");
    }

    #[tokio::test]
    async fn detailed_health_reports_empty_corpus_as_degraded_not_ok() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db);
        let app = build_router(store, None, None, 8765, None, Duration::from_secs(1), 1);
        let health = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        // A reachable-but-empty database is a structural success with zero rows: the top-level
        // probe still reports "ok" (unrelated fixtures/tests rely on this), but the database
        // block must truthfully say the corpus is empty rather than reading as fully healthy.
        assert_eq!(health.status(), StatusCode::OK);
        let payload: Value =
            serde_json::from_slice(&to_bytes(health.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();
        assert_eq!(payload["database"]["status"], "empty");
        assert_eq!(payload["database"]["memoryCount"], 0);
    }

    #[tokio::test]
    async fn detailed_health_reports_populated_corpus_as_ok_with_count() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db);
        store
            .try_put(
                "health-corpus-test",
                "a memory that proves the corpus is non-empty",
                "global",
                cortex_core::MemoryTier::Semantic,
            )
            .unwrap();
        let app = build_router(store, None, None, 8765, None, Duration::from_secs(1), 1);
        let health = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(health.status(), StatusCode::OK);
        let payload: Value =
            serde_json::from_slice(&to_bytes(health.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["database"]["status"], "ok");
        assert_eq!(payload["database"]["memoryCount"], 1);
    }

    #[test]
    fn detailed_health_does_not_share_tokios_blocking_pool() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                let _ = entered_tx.send(());
                let _ = release_rx.recv();
            });
            entered_rx.await.unwrap();

            let app = router_for_tests_with_policy(
                MemoryStore::new(),
                8765,
                None,
                Duration::from_secs(1),
                1,
            );
            let health = app
                .oneshot(Request::get("/health").body(Body::empty()).unwrap())
                .await
                .unwrap();

            release_tx.send(()).unwrap();
            blocker.await.unwrap();
            assert_eq!(health.status(), StatusCode::OK);
        });
    }

    #[tokio::test]
    async fn detailed_health_is_unavailable_when_catalog_is_busy() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let catalog = ContextCatalog::open_in_memory();
        let (release, holder) = hold_catalog_connection(catalog.clone());
        let app = build_router(
            MemoryStore::new(),
            Some(catalog.clone()),
            None,
            8765,
            None,
            Duration::from_secs(1),
            1,
        );
        let health = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        release.send(()).unwrap();
        holder.join().unwrap();

        assert_eq!(health.status(), StatusCode::SERVICE_UNAVAILABLE);
        let payload: Value =
            serde_json::from_slice(&to_bytes(health.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();
        assert_eq!(payload["catalog"]["status"], "busy");
    }

    #[tokio::test]
    async fn detailed_health_is_unavailable_when_catalog_query_fails() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let catalog = ContextCatalog::open_in_memory();
        catalog.lock().execute("DROP TABLE receipts", []).unwrap();
        let app = build_router(
            MemoryStore::new(),
            Some(catalog),
            None,
            8765,
            None,
            Duration::from_secs(1),
            1,
        );
        let health = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(health.status(), StatusCode::SERVICE_UNAVAILABLE);
        let payload: Value =
            serde_json::from_slice(&to_bytes(health.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();
        assert_eq!(payload["catalog"]["status"], "error");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn idempotent_put_retry_after_timeout_has_one_durable_effect() {
        use axum::http::StatusCode;

        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        let app = build_router(
            store,
            None,
            None,
            8765,
            Some(TEST_API_TOKEN.to_string()),
            Duration::from_millis(100),
            2,
        );
        let (release, holder) = hold_memdb_connection(db.clone());
        let body = r#"{"name":"retry-safe","content":"one durable put","scope":"global"}"#;
        let first_app = app.clone();
        let first = tokio::spawn(async move {
            post_json_with_key(first_app, "/put", body, "put-timeout-key").await
        });
        wait_for_worker_snapshot(&app, |payload| {
            payload["workers"]["general"]["available"] == 1
        })
        .await;
        assert_eq!(first.await.unwrap().status(), StatusCode::REQUEST_TIMEOUT);

        release.send(()).unwrap();
        holder.join().unwrap();
        let retry = post_json_with_key(app.clone(), "/put", body, "put-timeout-key").await;
        assert_eq!(retry.status(), StatusCode::OK);
        let retry_body = axum::body::to_bytes(retry.into_body(), MAX_BODY_BYTES)
            .await
            .unwrap();
        let completed_replay = post_json_with_key(app, "/put", body, "put-timeout-key").await;
        assert_eq!(completed_replay.status(), StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(completed_replay.into_body(), MAX_BODY_BYTES)
                .await
                .unwrap(),
            retry_body
        );
        let effects: i64 = db
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM memory_event_log WHERE event_kind IN ('put', 'update')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(effects, 1);
    }

    #[tokio::test]
    async fn idempotent_put_replays_a_body_larger_than_sixteen_kib() {
        use axum::http::StatusCode;

        let db = MemDb::open_in_memory();
        let app = router_for_tests_with_policy(
            MemoryStore::open(db.clone()),
            8765,
            Some(TEST_API_TOKEN.to_string()),
            Duration::from_secs(1),
            1,
        );
        let body = serde_json::json!({
            "name": "large-idempotent-put",
            "content": "x".repeat(16 * 1024 + 1),
            "scope": "global",
        })
        .to_string();

        let first = post_json_with_key(app.clone(), "/put", &body, "large-put-key").await;
        assert_eq!(first.status(), StatusCode::OK);
        let first_body = axum::body::to_bytes(first.into_body(), MAX_BODY_BYTES)
            .await
            .unwrap();
        let replay = post_json_with_key(app, "/put", &body, "large-put-key").await;
        assert_eq!(replay.status(), StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(replay.into_body(), MAX_BODY_BYTES)
                .await
                .unwrap(),
            first_body
        );
        let effects: i64 = db
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM memory_event_log WHERE event_kind IN ('put', 'update')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(effects, 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn in_flight_idempotent_put_replay_waits_for_exact_result() {
        use axum::http::StatusCode;

        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        let app = build_router(
            store,
            None,
            None,
            8765,
            Some(TEST_API_TOKEN.to_string()),
            Duration::from_secs(1),
            2,
        );
        let (release, holder) = hold_memdb_connection(db.clone());
        let body = r#"{"name":"inflight-safe","content":"one durable put","scope":"global"}"#;
        let first_app = app.clone();
        let first = tokio::spawn(async move {
            post_json_with_key(first_app, "/put", body, "put-inflight-key").await
        });
        wait_for_worker_snapshot(&app, |payload| {
            payload["workers"]["general"]["available"] == 1
        })
        .await;
        let second_app = app.clone();
        let second = tokio::spawn(async move {
            post_json_with_key(second_app, "/put", body, "put-inflight-key").await
        });
        wait_for_worker_snapshot(&app, |payload| {
            payload["workers"]["ingress"]["available"] == 0
        })
        .await;

        release.send(()).unwrap();
        holder.join().unwrap();
        let first = first.await.unwrap();
        let second = second.await.unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(first.into_body(), MAX_BODY_BYTES)
                .await
                .unwrap(),
            axum::body::to_bytes(second.into_body(), MAX_BODY_BYTES)
                .await
                .unwrap()
        );
        let effects: i64 = db
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM memory_event_log WHERE event_kind IN ('put', 'update')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(effects, 1);
    }

    #[tokio::test]
    async fn idempotency_key_reuse_with_different_put_body_conflicts() {
        use axum::http::StatusCode;

        let app = router_for_tests_with_policy(
            MemoryStore::new(),
            8765,
            Some(TEST_API_TOKEN.to_string()),
            Duration::from_secs(1),
            2,
        );
        let first = post_json_with_key(
            app.clone(),
            "/put",
            r#"{"name":"conflict-a","content":"first","scope":"global"}"#,
            "put-conflict-key",
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        let conflict = post_json_with_key(
            app,
            "/put",
            r#"{"name":"conflict-b","content":"second","scope":"global"}"#,
            "put-conflict-key",
        )
        .await;
        assert_eq!(conflict.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn unsupported_mutator_rejects_idempotency_key() {
        use axum::http::StatusCode;

        let app = router_for_tests_with_policy(
            MemoryStore::new(),
            8765,
            Some(TEST_API_TOKEN.to_string()),
            Duration::from_secs(1),
            1,
        );
        let response = post_json_with_key(
            app,
            "/compress",
            r#"{"text":"bounded","no_onnx":true}"#,
            "unsupported-key",
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn livez_bypasses_workload_ingress_while_work_is_active() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let (app, control) =
            router_for_tests_with_control(MemoryStore::new(), Duration::from_millis(1_500), 1);
        let first_app = app.clone();
        let first = tokio::spawn(async move {
            first_app
                .oneshot(
                    Request::get("/__test/slow-general")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        });
        control.workload.wait_started(1).await;

        let livez = app
            .clone()
            .oneshot(Request::get("/livez").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let livez_status = livez.status();
        let payload: Value =
            serde_json::from_slice(&to_bytes(livez.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();

        assert_eq!(livez_status, StatusCode::OK);
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["workers"]["ingress"]["available"], 0);

        control.workload.release(1);
        control.workload.wait_finished(1).await;
        assert_eq!(first.await.unwrap().status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn saturated_expensive_lane_returns_retryable_overload() {
        use axum::body::Body;
        use axum::http::{header, Request, StatusCode};
        use tower::ServiceExt;

        let (app, control) =
            router_for_tests_with_control(MemoryStore::new(), Duration::from_millis(1_500), 1);
        let first_app = app.clone();
        let first = tokio::spawn(async move {
            first_app
                .oneshot(Request::get("/__test/slow").body(Body::empty()).unwrap())
                .await
                .unwrap()
        });
        control.workload.wait_started(1).await;

        let overloaded = app
            .clone()
            .oneshot(Request::get("/__test/slow").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(overloaded.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(overloaded.headers()[header::RETRY_AFTER], "1");
        control.workload.release(1);
        control.workload.wait_finished(1).await;
        assert_eq!(first.await.unwrap().status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bounded_model_queue_absorbs_a_normal_two_request_burst() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let (app, control) =
            router_for_tests_with_control(MemoryStore::new(), Duration::from_millis(1_500), 2);
        let first_app = app.clone();
        let first = tokio::spawn(async move {
            first_app
                .oneshot(Request::get("/__test/slow").body(Body::empty()).unwrap())
                .await
                .unwrap()
        });
        control.workload.wait_started(1).await;
        let second_app = app.clone();
        let second = tokio::spawn(async move {
            second_app
                .oneshot(Request::get("/__test/slow").body(Body::empty()).unwrap())
                .await
                .unwrap()
        });
        wait_for_worker_snapshot(&app, |payload| {
            payload["workers"]["model"]["available"] == 0
        })
        .await;

        control.workload.release(1);
        control.workload.wait_started(2).await;
        control.workload.release(2);
        control.workload.wait_finished(2).await;
        assert_eq!(first.await.unwrap().status(), StatusCode::NOT_FOUND);
        assert_eq!(second.await.unwrap().status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn health_exposes_worker_capacity_and_overload_rejections() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Request, StatusCode};
        use tower::ServiceExt;

        let app = router_for_tests_with_policy(
            MemoryStore::new(),
            8765,
            None,
            Duration::from_millis(100),
            1,
        );

        let health = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        let payload: Value =
            serde_json::from_slice(&to_bytes(health.into_body(), MAX_BODY_BYTES).await.unwrap())
                .unwrap();
        assert_eq!(payload["workers"]["general"]["max"], 1);
        assert_eq!(payload["workers"]["model"]["max"], 1);
        assert_eq!(payload["workers"]["overloadRejections"]["total"], 0);
        assert_eq!(payload["workers"]["overloadRejections"]["ingress"], 0);
        assert_eq!(payload["workers"]["overloadRejections"]["diagnostics"], 0);
        assert_eq!(payload["workers"]["overloadRejections"]["model"], 0);
        assert_eq!(payload["workers"]["overloadRejections"]["general"], 0);
    }

    #[test]
    fn put_refuses_hand_typed_scope() {
        let store = MemoryStore::new();
        let put = route(
            &store,
            "POST",
            "/put",
            r#"{"name":"fork","content":"should not land","scope":"heardright","authority":"A0"}"#,
        );
        assert_eq!(put.0, 400);
        assert!(
            put.1.contains("refusing hand-typed scope"),
            "unexpected body: {}",
            put.1
        );
        let listed = route(&store, "POST", "/list", r#"{"scope":"heardright"}"#);
        assert_eq!(listed.0, 200);
        let payload: Value = serde_json::from_str(&listed.1).unwrap();
        assert_eq!(payload.as_array().map(|a| a.len()).unwrap_or(1), 0);
    }

    #[test]
    fn put_and_recall_round_trip() {
        let store = MemoryStore::new();
        let put = route(
            &store,
            "POST",
            "/put",
            r#"{"name":"note","content":"Deploy the worker.","scope":"D--Claude-coderight","authority":"A2"}"#,
        );
        assert_eq!(put.0, 200);
        let rec = route(
            &store,
            "POST",
            "/recall",
            "{\"query\":\"deploy worker\",\"k\":3,\"scope\":\"D--Claude-coderight\",\"client\":\"test\"}",
        );
        assert_eq!(rec.0, 200);
        assert!(
            rec.1.contains("D--Claude-coderight/note"),
            "recall: {}",
            rec.1
        );
    }

    #[test]
    fn cortex_competitive_v2_recall_and_list_share_completeness_envelope() {
        let store = MemoryStore::new();
        for index in 0..3 {
            store.put(
                &format!("complete-{index}"),
                &format!("release worker completeness {index}"),
                "global",
                cortex_core::MemoryTier::Semantic,
            );
        }
        let listed = route(
            &store,
            "POST",
            "/list",
            r#"{"scope":"global","responseVersion":2,"limit":2}"#,
        );
        assert_eq!(listed.0, 200, "{}", listed.1);
        let listed: Value = serde_json::from_str(&listed.1).unwrap();
        assert_eq!(listed["schemaVersion"], 2);
        assert_eq!(listed["items"].as_array().unwrap().len(), 2);
        assert_eq!(listed["completeness"]["state"], "lower_bound");
        assert_eq!(listed["completeness"]["countsExact"], false);

        let recalled = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"release worker completeness","k":2,"client":"test","responseVersion":2}"#,
        );
        assert_eq!(recalled.0, 200, "{}", recalled.1);
        let recalled: Value = serde_json::from_str(&recalled.1).unwrap();
        assert_eq!(recalled["schemaVersion"], 2);
        assert!(recalled["hits"].is_array());
        assert_eq!(recalled["completeness"]["state"], "lower_bound");
        assert_eq!(recalled["completeness"]["countsExact"], false);
    }

    #[test]
    fn legacy_memory_operations_have_native_route_shapes() {
        let store = MemoryStore::new();
        let remembered = route(
            &store,
            "POST",
            "/remember",
            r#"{"content":"manual note","keywords":["manual"]}"#,
        );
        assert_eq!(remembered.0, 200, "{}", remembered.1);
        let remembered: Value = serde_json::from_str(&remembered.1).unwrap();
        assert_eq!(remembered["content"], "manual note");
        assert_eq!(remembered["keywords"][0], "manual");

        let consolidated = route(
            &store,
            "POST",
            "/remember_consolidated",
            r#"{"name":"daily","content":"daily note","keywords":["daily"],"threshold":0.8}"#,
        );
        assert_eq!(consolidated.0, 200, "{}", consolidated.1);
        let consolidated: Value = serde_json::from_str(&consolidated.1).unwrap();
        assert_eq!(consolidated["consolidated"], true);
        assert!(consolidated["id"].as_str().is_some());

        let scopes = route(&store, "POST", "/scopes", "{}");
        assert_eq!(scopes.0, 200, "{}", scopes.1);
        assert_eq!(
            serde_json::from_str::<Value>(&scopes.1).unwrap(),
            json!(["global"])
        );

        let search = route(
            &store,
            "POST",
            "/search",
            r#"{"query":"manual note","limit":2}"#,
        );
        assert_eq!(search.0, 200, "{}", search.1);
        let hits: Value = serde_json::from_str(&search.1).unwrap();
        assert!(hits.as_array().is_some_and(|hits| !hits.is_empty()));
    }

    #[test]
    fn recall_rejects_normalized_dimension_aliases() {
        let store = MemoryStore::new();
        let response = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"context","client":"trusted","dimensions":{"client":"trusted","client ":"attacker"}}"#,
        );
        assert_eq!(response.0, 400);
        assert!(response.1.contains("duplicate normalized Taste dimension"));
    }

    #[test]
    fn recall_requires_exact_machine_binding_and_unique_request_receipts() {
        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db);
        let bound = put_taste_for_machine(
            &store,
            "machine-pref",
            "Use the host-local accelerator.",
            "standing_preference",
            "global",
            &[],
            "behavioral_directive",
            "active",
            Some("host-a"),
        );
        let missing = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"accelerator","k":1,"client":"test"}"#,
        );
        assert_eq!(missing.0, 200);
        assert!(!missing.1.contains(&bound));
        let wrong = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"accelerator","k":1,"client":"test","machine":"host-b"}"#,
        );
        assert_eq!(wrong.0, 200);
        assert!(!wrong.1.contains(&bound));
        let first = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"accelerator","k":1,"client":"test","machine":"host-a"}"#,
        );
        let second = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"accelerator","k":1,"client":"test","machine":"host-a"}"#,
        );
        let receipt_id = |body: &str| {
            serde_json::from_str::<Value>(body).unwrap()[0]["delivery_receipt"]["receipt_id"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(first.0, 200, "{}", first.1);
        assert_eq!(second.0, 200, "{}", second.1);
        assert_ne!(receipt_id(&first.1), receipt_id(&second.1));
    }

    #[test]
    fn recall_delivers_signed_dimension_scope_without_opaque_scope_id() {
        let store = MemoryStore::new();
        let dimensions = [("language", "rust"), ("path_prefix", "engine/src")];
        let scope = opaque_taste_scope(&dimensions);
        let preference = put_taste(
            &store,
            "signed-dimension-match",
            "Prefer focused Rust verification.",
            "scoped_preference",
            &scope,
            &dimensions,
            "behavioral_directive",
            "active",
        );

        let response = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"focused verification","k":1,"scope":"D--Claude-repo","client":"test","dimensions":{"language":"Rust","path_prefix":"engine/src/adapt"}}"#,
        );
        assert_eq!(response.0, 200, "{}", response.1);
        let rows: Value = serde_json::from_str(&response.1).unwrap();
        assert_eq!(rows[0]["id"], preference);
        assert_eq!(rows[0]["delivery_receipt"]["selected"], true);
    }

    #[test]
    fn recall_omits_signed_dimension_scope_outside_language_or_path() {
        let store = MemoryStore::new();
        let dimensions = [("language", "rust"), ("path_prefix", "engine/src")];
        let scope = opaque_taste_scope(&dimensions);
        let preference = put_taste(
            &store,
            "signed-dimension-nonmatch",
            "Prefer focused Rust verification.",
            "scoped_preference",
            &scope,
            &dimensions,
            "behavioral_directive",
            "active",
        );

        for request_dimensions in [
            r#"{"language":"python","path_prefix":"engine/src/adapt"}"#,
            r#"{"language":"rust","path_prefix":"docs"}"#,
        ] {
            let body = format!(
                r#"{{"query":"focused verification","k":1,"scope":"D--Claude-repo","client":"test","dimensions":{request_dimensions}}}"#
            );
            let response = route(&store, "POST", "/recall", &body);
            assert_eq!(response.0, 200, "{}", response.1);
            assert!(!response.1.contains(&preference), "{}", response.1);
        }
    }

    #[test]
    fn recall_delivers_governed_taste_first_and_persists_exact_receipts() {
        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        let generic = store.remember("ordinary generic context", vec!["ordinary".into()]);
        let core = put_taste(
            &store,
            "core-pref",
            "Always use focused verification.",
            "standing_preference",
            "global",
            &[],
            "behavioral_directive",
            "active",
        );
        let scoped = put_taste(
            &store,
            "scoped-pref",
            "Prefer the repository formatter.",
            "scoped_preference",
            "D--Claude-repoa",
            &[("client", "test")],
            "behavioral_directive",
            "active",
        );
        let inactive = put_taste(
            &store,
            "inactive-pref",
            "Inactive preference must not render.",
            "standing_preference",
            "global",
            &[],
            "behavioral_directive",
            "retired",
        );
        let nondirective = put_taste(
            &store,
            "reference-pref",
            "Reference-only preference must not render.",
            "standing_preference",
            "global",
            &[],
            "reference_only",
            "active",
        );
        let mismatch = put_taste(
            &store,
            "scope-mismatch",
            "Another repository preference.",
            "scoped_preference",
            "D--Claude-repob",
            &[],
            "behavioral_directive",
            "active",
        );
        let budgeted = put_taste(
            &store,
            "budget-pref",
            "A second scoped preference.",
            "scoped_preference",
            "D--Claude-repoa",
            &[],
            "behavioral_directive",
            "active",
        );
        let tampered = put_taste(
            &store,
            "tampered-pref",
            "Sealed preference bytes.",
            "standing_preference",
            "global",
            &[],
            "behavioral_directive",
            "active",
        );
        db.lock()
            .execute(
                "UPDATE memories SET content=replace(content,'Sealed preference bytes.','Edited preference bytes.') WHERE id=?1",
                [&tampered],
            )
            .unwrap();
        let response = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"ordinary context","k":3,"scope":"D--Claude-repoa","client":"test","model":"gpt-test","session":"session-1","turn_id":"turn-1","trace_id":"trace-1","taste_max_core":1,"taste_max_scoped":1,"total_preview_chars":240}"#,
        );
        assert_eq!(response.0, 200, "{}", response.1);
        let rows: Value = serde_json::from_str(&response.1).unwrap();
        let rows = rows.as_array().expect("backward-compatible bare array");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["id"], core);
        assert_eq!(rows[1]["id"], budgeted);
        assert_eq!(rows[2]["id"], generic.id);
        assert!(rows.iter().all(|row| row["id"] != inactive));
        assert!(rows.iter().all(|row| row["id"] != nondirective));
        assert!(rows.iter().all(|row| row["id"] != mismatch));
        assert!(rows.iter().all(|row| row["id"] != scoped));
        assert!(rows.iter().all(|row| row["id"] != tampered));
        let selected = &rows[0]["delivery_receipt"];
        assert_eq!(selected["selected"], true);
        assert_eq!(selected["session_id"], "session-1");
        assert_eq!(selected["trace_id"], "trace-1");
        assert_eq!(selected["client"], "test");
        assert_eq!(selected["model"], "gpt-test");
        assert_eq!(
            selected["rendered_sha256"],
            membrane_adapt::canonical::sha256_hex(rows[0]["skel"].as_str().unwrap().as_bytes())
        );
        let conn = db.lock_events();
        let persisted: Vec<(String, String, String, Option<String>)> = conn
            .prepare(
                "SELECT event_id,phase,reason_code,artifact_sha256
                   FROM context_event_log WHERE event_id LIKE 'pdr.%' ORDER BY event_id",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                    })?
                    .collect()
            })
            .unwrap();
        // Other-repository records are excluded before receipt formation;
        // the six visible candidates still have exact decision receipts.
        assert_eq!(persisted.len(), 6);
        assert_eq!(
            persisted
                .iter()
                .filter(|(_, phase, _, _)| phase == "block.delivered")
                .count(),
            2
        );
        assert!(persisted.iter().any(|(_, phase, reason, digest)| {
            phase == "candidate.filtered" && reason == "inactive_lifecycle" && digest.is_none()
        }));
        assert!(persisted.iter().any(|(_, phase, reason, digest)| {
            phase == "candidate.filtered" && reason == "non_directive_influence" && digest.is_none()
        }));
        assert!(
            !persisted
                .iter()
                .any(|(_, _, reason, _)| reason == "scope_nonmatch"),
            "cross-repository candidate identity must not enter this receipt set"
        );
        assert!(persisted.iter().any(|(_, phase, reason, digest)| {
            phase == "candidate.filtered" && reason == "selection_budget" && digest.is_none()
        }));
        assert!(persisted.iter().any(|(_, phase, reason, digest)| {
            phase == "candidate.filtered" && reason == "invalid_semantic_seal" && digest.is_none()
        }));
        let adherence_events: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM context_event_log WHERE phase='candidate.used'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(adherence_events, 0, "delivery never infers adherence");
    }

    #[test]
    fn recall_fails_when_taste_receipts_cannot_persist() {
        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        put_taste(
            &store,
            "receipt-failure",
            "Always preserve receipt integrity.",
            "standing_preference",
            "global",
            &[],
            "behavioral_directive",
            "active",
        );
        db.lock_events()
            .execute_batch(
                "CREATE TRIGGER fail_taste_receipt BEFORE INSERT ON context_event_log
                 WHEN NEW.event_id LIKE 'pdr.%'
                 BEGIN SELECT RAISE(ABORT, 'forced receipt failure'); END;",
            )
            .unwrap();
        let response = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"receipt integrity","k":2,"client":"test","session":"session-1","trace_id":"trace-1"}"#,
        );
        assert_eq!(response.0, 500, "{}", response.1);
        assert!(response.1.contains("Taste receipt persistence failed"));
    }

    #[test]
    fn recall_downstream_failure_does_not_persist_delivery() {
        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        put_taste(
            &store,
            "downstream-failure",
            "Always stage delivery truth.",
            "standing_preference",
            "global",
            &[],
            "behavioral_directive",
            "active",
        );
        db.lock()
            .execute_batch(
                "CREATE TRIGGER fail_injection BEFORE UPDATE OF inject_count ON memories
                 BEGIN SELECT RAISE(ABORT, 'forced downstream failure'); END;",
            )
            .unwrap();
        let response = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"delivery truth","k":1,"client":"test"}"#,
        );
        assert_eq!(response.0, 500);
        let delivered: i64 = db
            .lock_events()
            .query_row(
                "SELECT COUNT(*) FROM context_event_log WHERE phase='block.delivered'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(delivered, 0);
    }

    #[test]
    fn recall_backfills_generic_after_nine_taste_candidates() {
        let store = MemoryStore::new();
        let generic = store.remember("rank nine needle context", vec!["needle".into()]);
        for index in 0..9 {
            put_taste(
                &store,
                &format!("ranked-taste-{index}"),
                &format!("Needle preference number {index}."),
                "standing_preference",
                "global",
                &[],
                "behavioral_directive",
                "active",
            );
        }
        let response = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"needle","k":1,"client":"test","taste_max_core":0,"taste_max_scoped":0}"#,
        );
        assert_eq!(response.0, 200, "{}", response.1);
        assert!(response.1.contains(&generic.id), "{}", response.1);
    }

    #[test]
    fn retired_taste_history_over_candidate_cap_does_not_break_recall() {
        let store = MemoryStore::new();
        let generic = store.remember("bounded historical context", vec!["bounded".into()]);
        for index in 0..=128 {
            put_taste(
                &store,
                &format!("retired-history-{index:03}"),
                "Historical preference.",
                "standing_preference",
                "global",
                &[],
                "behavioral_directive",
                "retired",
            );
        }
        let response = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"bounded historical","k":1,"client":"test"}"#,
        );
        assert_eq!(response.0, 200, "{}", response.1);
        assert!(response.1.contains(&generic.id), "{}", response.1);
    }

    #[test]
    fn recall_route_returns_typed_abstention_for_a0_candidates() {
        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        let id = store.put(
            "untrusted",
            "unique route authority marker",
            "global",
            cortex_core::MemoryTier::Semantic,
        );
        db.lock()
            .execute(
                "UPDATE memories SET authority='A0' WHERE id=?1",
                rusqlite::params![id],
            )
            .unwrap();

        let response = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"unique route authority marker","k":3,"client":"test"}"#,
        );
        assert_eq!(response.0, 200);
        let value: serde_json::Value = serde_json::from_str(&response.1).unwrap();
        assert_eq!(value["status"], "insufficient_confidence");
        assert_eq!(value["hits"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn quarantine_routes_list_and_restore_without_restart() {
        let store = MemoryStore::new();
        let low = store
            .remember_consolidated("low", "Low-value stale memory", vec![], 0.1)
            .unwrap();
        let status = store.dream_now("2026-06-29").unwrap();
        assert_eq!(status.quarantined_count, 1);

        let listed = route(&store, "POST", "/quarantine/list", "{}");
        assert_eq!(listed.0, 200);
        assert!(listed.1.contains(&low.id));

        let restored = route(
            &store,
            "POST",
            "/quarantine/restore",
            &serde_json::json!({ "id": low.id }).to_string(),
        );
        assert_eq!(restored.0, 200);
        assert!(restored.1.contains("\"restored\":true"));
        assert!(store.entries(10).iter().any(|entry| entry.id == low.id));
    }

    #[test]
    fn graph_route_exposes_real_local_nodes_and_embedding_edges() {
        let store = MemoryStore::new();
        // Distinct (not identical) content so the §16.3 admission pre-filter
        // never fires: embedding-similar (one word swapped), but not a
        // near-duplicate by word-shingle Jaccard.
        store.put(
            "first",
            "deploy cloudflare worker",
            "global",
            cortex_core::MemoryTier::Semantic,
        );
        store.put(
            "second",
            "deploy cloudflare service",
            "global",
            cortex_core::MemoryTier::Semantic,
        );
        let response = route(&store, "GET", "/graph", "");
        assert_eq!(response.0, 200);
        let value: serde_json::Value = serde_json::from_str(&response.1).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(value["edges"].as_array().unwrap().len(), 1);
        assert_eq!(value["completeness"]["state"], "exact");
        assert_eq!(value["completeness"]["returnedCount"], 2);
        assert!(response.1.contains("global/first"));
        assert_eq!(value["edges"][0]["kind"], "semantic");
    }

    #[test]
    fn observable_query_routes_require_an_explicit_limit() {
        let store = MemoryStore::new();
        for path in [
            "/v1/telemetry/observable-events:query-taste",
            "/v1/telemetry/observable-events:query-insights",
            "/v1/telemetry/observable-events:query-forge-time",
        ] {
            let response = route(&store, "POST", path, r#"{"sessionId":"s-1"}"#);
            assert_eq!(response.0, 400, "{path} accepted a query with no limit");
            assert!(response.1.contains("limit required"));
        }
    }

    #[test]
    fn observable_taste_route_cannot_be_widened_by_the_request_body() {
        // The route is the only thing that selects origin scope. A caller asking for a wider
        // origin in the body must not get one -- otherwise Taste could read assistant, model or
        // tool events simply by naming them, which is exactly what L1 forbids.
        let store = MemoryStore::new();
        let widened = route(
            &store,
            "POST",
            "/v1/telemetry/observable-events:query-taste",
            r#"{"limit":10,"origin":"tool","scope":"insights","originScope":"InsightsFullStream"}"#,
        );
        assert_eq!(widened.0, 200);
        let value: serde_json::Value = serde_json::from_str(&widened.1).unwrap();
        assert!(
            value["rows"].as_array().expect("rows array").is_empty(),
            "taste route returned rows for a body-supplied wider origin"
        );
        assert_eq!(value["truncated"], false);
    }

    #[test]
    fn observable_forge_time_route_cannot_be_widened_by_the_request_body() {
        // Mirrors observable_taste_route_cannot_be_widened_by_the_request_body: Forge's
        // time-accounting route must stay tool-origin-only regardless of what a caller names in
        // the body -- scope is fixed by the route, not by request data.
        let store = MemoryStore::new();
        let widened = route(
            &store,
            "POST",
            "/v1/telemetry/observable-events:query-forge-time",
            r#"{"limit":10,"origin":"user","scope":"insights","originScope":"InsightsFullStream"}"#,
        );
        assert_eq!(widened.0, 200);
        let value: serde_json::Value = serde_json::from_str(&widened.1).unwrap();
        assert!(
            value["rows"].as_array().expect("rows array").is_empty(),
            "forge-time route returned rows for a body-supplied wider origin"
        );
        assert_eq!(value["truncated"], false);
    }

    #[test]
    fn lifecycle_routes_preserve_client_session_and_trace_attribution() {
        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        let put = route(
            &store,
            "POST",
            "/put",
            r#"{"name":"observed","content":"body","scope":"global","client":"claude","session":"s-1","trace_id":"t-1"}"#,
        );
        assert_eq!(put.0, 200);
        let get = route(
            &store,
            "POST",
            "/get",
            r#"{"id":"global/observed","client":"codex","session":"s-2","trace_id":"t-2"}"#,
        );
        assert_eq!(get.0, 200);
        let rows = db
            .lock()
            .prepare(
                "SELECT event_kind, surface, session_id, trace_id
                 FROM memory_event_log ORDER BY event_id",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<String>>(3)?,
                        ))
                    })
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            })
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "put".to_string(),
                    "claude".to_string(),
                    Some(crate::store::opaque_correlation_token("s-1", "session")),
                    Some(crate::store::opaque_correlation_token("t-1", "trace")),
                ),
                (
                    "get".to_string(),
                    "codex".to_string(),
                    Some(crate::store::opaque_correlation_token("s-2", "session")),
                    Some(crate::store::opaque_correlation_token("t-2", "trace")),
                ),
            ]
        );
    }

    #[test]
    fn policy_assignment_route_is_stable_and_requires_attribution() {
        let store = MemoryStore::new();
        let body = r#"{"session":"s-42","client":"codex","policy_version":"cortex-v1","control_pct":10,"task_class":"code"}"#;
        let first = route(&store, "POST", "/policy/assign", body);
        let second = route(&store, "POST", "/policy/assign", body);
        assert_eq!(first.0, 200);
        assert_eq!(first.1, second.1);
        assert!(first.1.contains("\"cohort\":"));
        assert!(first.1.contains("\"task_class\":\"code\""));
        assert_eq!(
            route(
                &store,
                "POST",
                "/policy/assign",
                r#"{"client":"codex","policy_version":"cortex-v1"}"#,
            )
            .0,
            400
        );
    }

    #[test]
    fn add_route_is_disabled_and_prefix_routes_do_not_match() {
        let dir = std::env::temp_dir().join(format!("cortex-serve-add-off-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let md = dir.join("secret.md");
        std::fs::write(&md, "PRIVATE SECRET SHOULD NOT INGEST").unwrap();
        let store = MemoryStore::new();
        let add = route(
            &store,
            "POST",
            "/add",
            &format!(
                "{{\"path\":{:?},\"scope\":\"D--Claude-coderight\"}}",
                md.to_string_lossy()
            ),
        );
        assert_eq!(add.0, 410);
        assert!(store.list(Some("D--Claude-coderight")).is_empty());

        let prefix = route(
            &store,
            "POST",
            "/puts",
            r#"{"name":"note","content":"Deploy the worker.","scope":"D--Claude-coderight"}"#,
        );
        assert_eq!(prefix.0, 404);
        assert!(store.list(Some("D--Claude-coderight")).is_empty());
        let _ = std::fs::remove_file(&md);
    }

    #[test]
    fn get_returns_full_content_and_records_use() {
        let store = MemoryStore::new();
        let entry = store.remember("Full body.\nSecond line survives.", vec!["full".into()]);
        let res = route(
            &store,
            "POST",
            "/get",
            &format!("{{\"id\":{:?}}}", entry.id),
        );
        assert_eq!(res.0, 200);
        assert!(res.1.contains("Second line survives."), "res: {}", res.1);
        assert!(res.1.contains("\"access_count\":1"), "res: {}", res.1);
        let missing = route(&store, "POST", "/get", "{\"id\":\"nope\"}");
        assert_eq!(missing.0, 404);
        let empty = route(&store, "POST", "/get", "{}");
        assert_eq!(empty.0, 400);
    }

    #[test]
    fn recall_route_increments_inject_count() {
        let store = MemoryStore::new();
        let entry = store.remember("Inject counting memo.", vec!["inject".into()]);
        let res = route(
            &store,
            "POST",
            "/recall",
            "{\"query\":\"inject counting memo\",\"k\":3,\"client\":\"test\"}",
        );
        assert_eq!(res.0, 200);
        assert!(res.1.contains(&entry.id), "hit expected: {}", res.1);
        assert_eq!(store.inject_count(&entry.id), 1);
    }

    #[test]
    fn preview_skips_frontmatter_comments_and_heading_only_lines() {
        let s = preview(
            "---\nname: x\n---\n<!-- archived -->\n## Heading\n**Rule:** the actual content here.",
            240,
        );
        assert!(s.starts_with("**Rule:** the actual content"), "got: {s}");
        let plain = preview("First line is real content.\nSecond line.", 240);
        assert!(plain.starts_with("First line is real content."));
    }

    #[test]
    fn preview_survives_adversarial_shapes() {
        // Unclosed frontmatter must not eat the whole memory into an empty preview.
        let unclosed = preview("---\nname: x\nbody text after broken frontmatter", 240);
        assert!(!unclosed.trim().is_empty());
        // TOML frontmatter (+++) treated like YAML.
        let toml = preview("+++\ntitle = \"x\"\n+++\nReal content sentence.", 240);
        assert!(toml.starts_with("Real content"), "got: {toml}");
        // Code-fence markers dropped; fenced content kept.
        let fenced = preview("```bash\nssh dd uptime\n```\nExplanation line.", 240);
        assert!(!fenced.contains("```"), "got: {fenced}");
        // All-boilerplate memory falls back to post-frontmatter text, never raw frontmatter.
        let stub = preview("---\nname: stub\n---\n## Title\n", 240);
        assert!(!stub.contains("name: stub"), "got: {stub}");
    }

    #[test]
    fn recall_top_hit_gets_extended_preview_when_relevant() {
        let store = MemoryStore::new();
        // Content = the query text repeated: the trigram hash embedder gives cos ~1.0 (same
        // distribution direction), safely above the 0.55 expansion gate; length > 400 so the
        // budget is what bounds the preview.
        let long = "rule one detail. ".repeat(40);
        let entry = store.remember(&long, vec!["rule".into()]);
        let res = route(
            &store,
            "POST",
            "/recall",
            "{\"query\":\"rule one detail. \",\"k\":3,\"client\":\"test\"}",
        );
        assert_eq!(res.0, 200);
        let hits: serde_json::Value = serde_json::from_str(&res.1).unwrap();
        assert_eq!(hits[0]["id"].as_str().unwrap(), entry.id);
        let first = hits[0]["skel"].as_str().unwrap();
        assert!(
            first.chars().count() > 300,
            "top hit above the cos gate should carry the extended preview, got {} chars",
            first.chars().count()
        );
    }

    #[test]
    fn memory_candidates_exposes_content_free_stage_timing() {
        let store = MemoryStore::new();
        let secret_task = "sensitive timing probe task";
        let response = route(
            &store,
            "POST",
            "/memory-candidates",
            &serde_json::json!({
                "task": secret_task,
                "scope": "global",
                "max_candidates": 3,
            })
            .to_string(),
        );
        assert_eq!(response.0, 200, "{}", response.1);
        let payload: serde_json::Value = serde_json::from_str(&response.1).unwrap();
        let membrane = payload
            .get("_membrane")
            .and_then(serde_json::Value::as_object)
            .expect("content-free timing envelope");
        let stages = membrane
            .get("stageElapsedMs")
            .and_then(serde_json::Value::as_object)
            .expect("stage timing map");
        let names = stages.keys().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(names, vec!["embed", "rank", "recall", "request_parse"]);
        assert!(stages.values().all(|value| {
            value
                .as_f64()
                .is_some_and(|elapsed| elapsed.is_finite() && elapsed >= 0.0)
        }));
        assert!(
            !serde_json::Value::Object(membrane.clone())
                .to_string()
                .contains(secret_task),
            "observability must not expose task content"
        );
    }

    #[test]
    fn memory_candidates_accepts_typed_virtual_scope_without_global_inheritance() {
        let store = MemoryStore::new();
        store
            .try_put(
                "thread-memory",
                "typed virtual scope candidate",
                "virtual:tenant-a:thread:abc-123",
                cortex_core::MemoryTier::Semantic,
            )
            .unwrap();
        let response = route(
            &store,
            "POST",
            "/memory-candidates",
            r#"{"task":"typed virtual scope","scopeDescriptor":{"kind":"virtual","id":"thread:abc-123","tenant_id":"tenant-a","parents":[],"inherit_global":false},"max_candidates":3}"#,
        );
        assert_eq!(response.0, 200, "{}", response.1);
        let payload: serde_json::Value = serde_json::from_str(&response.1).unwrap();
        assert_eq!(payload["scope"], "virtual:tenant-a:thread:abc-123");
        assert!(payload["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|candidate| candidate["sourceRef"] != "global"));
    }

    #[test]
    fn recall_honors_global_planner_preview_budget() {
        let store = MemoryStore::new();
        let long = "budgeted context detail. ".repeat(50);
        for n in 0..5 {
            let _ = store.remember(&format!("{long} item {n}"), vec!["budget".into()]);
        }
        let res = route(
            &store,
            "POST",
            "/recall",
            "{\"query\":\"budgeted context detail\",\"k\":5,\"client\":\"test\",\"preview_chars\":100,\"top_preview_chars\":150,\"total_preview_chars\":250}",
        );
        assert_eq!(res.0, 200, "{}", res.1);
        let hits: serde_json::Value = serde_json::from_str(&res.1).unwrap();
        let rows = hits.as_array().unwrap();
        let total: usize = rows
            .iter()
            .map(|row| row["skel"].as_str().unwrap().chars().count())
            .sum();
        assert!(total <= 250, "planner total must be enforced: {total}");
        assert!(rows[0]["skel"].as_str().unwrap().chars().count() <= 150);
        assert!(rows
            .iter()
            .skip(1)
            .all(|row| { row["skel"].as_str().unwrap().chars().count() <= 100 }));
    }

    #[test]
    fn recall_rejects_unbounded_planner_preview_budget() {
        let store = MemoryStore::new();
        let res = route(
            &store,
            "POST",
            "/recall",
            "{\"query\":\"budget\",\"k\":2,\"client\":\"test\",\"total_preview_chars\":999999}",
        );
        assert_eq!(res.0, 400);
        assert!(res.1.contains("total_preview_chars"));
    }

    #[test]
    fn recall_logs_source_and_query_preview() {
        let store = MemoryStore::new();
        let _ = store.remember("provenance check memo", vec!["provenance".into()]);
        let res = route(
            &store,
            "POST",
            "/recall",
            "{\"query\":\"provenance check memo\",\"k\":2,\"client\":\"test\"}",
        );
        assert_eq!(res.0, 200);
        // Serve recalls must be attributed and replayable (source + query_preview).
        let m = store.metrics_json();
        assert!(
            m["recalls"].as_u64().unwrap_or(0) >= 1,
            "serve row counted: {m}"
        );
        assert!(
            m["effectiveness"]["injected_distinct"]
                .as_u64()
                .unwrap_or(0)
                >= 1
        );
    }

    #[test]
    fn recall_rejects_blank_query_without_logging() {
        let store = MemoryStore::new();
        let res = route(
            &store,
            "POST",
            "/recall",
            "{\"query\":\"   \",\"k\":2,\"client\":\"test\"}",
        );
        assert_eq!(res.0, 400);
        assert!(res.1.contains("query required"), "res: {}", res.1);
        let m = store.metrics_json();
        assert_eq!(
            m["recalls"].as_u64().unwrap_or(0),
            0,
            "must not log blank recall: {m}"
        );
    }

    #[test]
    fn recall_rejects_missing_or_unknown_client_without_logging() {
        let store = MemoryStore::new();
        let missing = route(
            &store,
            "POST",
            "/recall",
            "{\"query\":\"client required memo\",\"k\":2}",
        );
        assert_eq!(missing.0, 400);
        assert!(
            missing.1.contains("client attribution required"),
            "res: {}",
            missing.1
        );
        let unknown = route(
            &store,
            "POST",
            "/recall",
            "{\"query\":\"client required memo\",\"k\":2,\"client\":\"unknown\"}",
        );
        assert_eq!(unknown.0, 400);
        let m = store.metrics_json();
        assert_eq!(
            m["recalls"].as_u64().unwrap_or(0),
            0,
            "unattributed recalls must not log: {m}"
        );
    }

    /// 2026-07-09 (add-now plan, Phase 1): /recall persists client/session/cwd/hook/trace/
    /// visibility. Without this, the per-client metrics block is just zeros and the add-now
    /// plan's first acceptance test (one ClaudeMM turn + one Codex turn → distinct
    /// recall_log.client values) cannot be verified.
    #[test]
    fn recall_persists_client_attribution() {
        let store = MemoryStore::new();
        let _ = store.remember("client attribution memo", vec!["client".into()]);
        let body = r#"{"query":"client attribution memo","k":2,"client":"codex","session":"sess-1","cwd":"D--Claude","hook_event":"UserPromptSubmit","trace_id":"trace-1","client_visibility":"all"}"#;
        let res = route(&store, "POST", "/recall", body);
        assert_eq!(res.0, 200);
        let m = store.metrics_json();
        let counts = m["attribution"]["client_counts"]
            .as_array()
            .expect("client_counts present");
        let codex: u64 = counts
            .iter()
            .filter_map(|c| {
                c.get("client")
                    .and_then(|x| x.as_str())
                    .map(String::from)
                    .zip(c.get("count").and_then(|x| x.as_u64()))
            })
            .filter(|(client, _)| client == "codex")
            .map(|(_, n)| n)
            .sum();
        assert!(codex >= 1, "codex client must be counted: {counts:?}");
    }

    #[test]
    fn evaluation_recall_logs_without_observing_or_incrementing_injections() {
        let store = MemoryStore::new();
        let entry = store.remember("evaluation-only recall memo", vec!["evaluation".into()]);
        let res = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"evaluation-only recall memo","k":2,"client":"eval","observe":false,"traffic_class":"evaluation"}"#,
        );
        assert_eq!(res.0, 200, "res: {}", res.1);
        assert!(res.1.contains(&entry.id));
        assert_eq!(store.inject_count(&entry.id), 0);

        let activity = store.activity_json(1);
        let recall = &activity["recalls"][0];
        assert_eq!(recall["traffic_class"], "evaluation");
        assert_eq!(recall["candidate_hits"][0]["id"], entry.id);
        assert_eq!(recall["admitted_hits"], recall["candidate_hits"]);
        assert_eq!(store.metrics_json()["recalls"].as_u64().unwrap_or(0), 0);
    }

    #[test]
    fn explicit_smoke_context_routes_only_to_physical_smoke_sink() {
        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        let entry = store.remember("physical smoke sink memo", vec!["smoke".into()]);
        let response = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"physical smoke sink memo","client":"test-probe","context_source":"smoke","observe":false,"traffic_class":"evaluation"}"#,
        );
        assert_eq!(response.0, 200, "{}", response.1);
        assert!(response.1.contains(&entry.id));
        assert_eq!(store.inject_count(&entry.id), 0);
        let conn = db.lock();
        let production: i64 = conn
            .query_row("SELECT COUNT(*) FROM recall_log", [], |row| row.get(0))
            .unwrap();
        let smoke: (i64, String, String) = conn
            .query_row(
                "SELECT COUNT(*), traffic_class, original_traffic_class FROM recall_log_smoke",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(production, 0);
        assert_eq!(smoke, (1, "smoke".into(), "evaluation".into()));
    }

    #[test]
    fn smoke_clients_fail_safe_to_smoke_sink_and_require_nonproduction_nonobserving_requests() {
        for client in ["smoke", "spotcheck", " SMOKE-SPOTCHECK "] {
            let db = MemDb::open_in_memory();
            let store = MemoryStore::open(db.clone());
            store.remember("client fail safe smoke memo", vec!["smoke".into()]);
            let valid = route(
                &store,
                "POST",
                "/recall",
                &serde_json::json!({
                    "query": "client fail safe smoke memo",
                    "client": client,
                    "observe": false,
                    "traffic_class": "evaluation",
                })
                .to_string(),
            );
            assert_eq!(valid.0, 200, "client={client:?}: {}", valid.1);
            let conn = db.lock();
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM recall_log", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM recall_log_smoke", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
            drop(conn);

            for invalid in [
                serde_json::json!({"query":"x", "client":client, "traffic_class":"evaluation"}),
                serde_json::json!({"query":"x", "client":client, "observe":false, "traffic_class":"production"}),
                serde_json::json!({"query":"x", "client":client, "observe":false, "traffic_class":" Production "}),
            ] {
                let response = route(&store, "POST", "/recall", &invalid.to_string());
                assert_eq!(
                    response.0, 400,
                    "client={client:?}, body={invalid}: {}",
                    response.1
                );
            }
            assert_eq!(
                db.lock()
                    .query_row("SELECT COUNT(*) FROM recall_log", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn smoke_recall_fails_when_the_physical_sink_cannot_persist() {
        let db = MemDb::open_in_memory();
        let store = MemoryStore::open(db.clone());
        store.remember("sink failure smoke memo", vec!["smoke".into()]);
        db.lock()
            .execute_batch("DROP TABLE recall_log_smoke")
            .unwrap();

        let response = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"sink failure smoke memo","client":"smoke","observe":false,"traffic_class":"evaluation"}"#,
        );
        assert_eq!(response.0, 500, "{}", response.1);
        assert!(response
            .1
            .contains("smoke recall telemetry persistence failed"));
        assert_eq!(
            db.lock()
                .query_row("SELECT COUNT(*) FROM recall_log", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn context_source_is_fail_closed_when_present_and_unknown() {
        let store = MemoryStore::new();
        for marker in [
            serde_json::json!("production"),
            serde_json::json!(""),
            serde_json::json!(7),
        ] {
            let response = route(
                &store,
                "POST",
                "/recall",
                &serde_json::json!({
                    "query": "context source validation",
                    "client": "codex",
                    "context_source": marker,
                    "observe": false,
                    "traffic_class": "evaluation",
                })
                .to_string(),
            );
            assert_eq!(response.0, 400, "marker={marker}: {}", response.1);
        }
        assert_eq!(store.metrics_json()["recalls"].as_u64(), Some(0));
    }

    #[test]
    fn observe_false_requires_explicit_nonproduction_traffic_class() {
        let store = MemoryStore::new();
        store.remember("explicit evaluation label memo", vec!["evaluation".into()]);

        for body in [
            r#"{"query":"explicit evaluation label memo","client":"eval","observe":false}"#,
            r#"{"query":"explicit evaluation label memo","client":"eval","observe":false,"traffic_class":"production"}"#,
            r#"{"query":"explicit evaluation label memo","client":"eval","observe":false,"traffic_class":" Production "}"#,
        ] {
            let response = route(&store, "POST", "/recall", body);
            assert_eq!(response.0, 400, "body={body}; response={}", response.1);
        }
    }

    #[test]
    fn metrics_label_preview_delta_as_potential_savings() {
        let store = MemoryStore::new();
        let _entry = store.remember("potential savings metric memo", vec!["potential".into()]);
        store.log_recall(
            &crate::time::now_iso(),
            Some("global"),
            10,
            1,
            100,
            5,
            "serve",
            Some("potential"),
            Some("test"),
            None,
            None,
            None,
            None,
            Some("all"),
        );
        let metrics = store.metrics_json();
        assert!(metrics["potential_chars_saved"].is_number());
        assert!(metrics["potential_tokens_saved_est"].is_number());
        assert_eq!(metrics["potential_chars_saved"], metrics["chars_saved"]);
        assert_eq!(
            metrics["potential_tokens_saved_est"],
            metrics["tokens_saved_est"]
        );
    }

    #[test]
    fn observed_recall_persists_exact_replay_telemetry() {
        let store = MemoryStore::new();
        let entry = store.remember("replay telemetry memo", vec!["replay".into()]);
        let res = route(
            &store,
            "POST",
            "/recall",
            r#"{"query":"replay telemetry memo","k":2,"client":"eval","traffic_class":"evaluation"}"#,
        );
        assert_eq!(res.0, 200);
        let activity = store.activity_json(1);
        let recall = &activity["recalls"][0];
        assert_eq!(recall["traffic_class"], "evaluation");
        assert_eq!(recall["candidate_hits"][0]["id"], entry.id);
        assert!(recall["candidate_hits"][0]["score"].is_number());
        assert_eq!(recall["admitted_hits"], recall["candidate_hits"]);
    }

    #[test]
    fn recall_and_put_reject_oversized_inputs() {
        let store = MemoryStore::new();
        let oversized_body = "x".repeat(MAX_BODY_BYTES + 1);
        assert_eq!(route(&store, "POST", "/recall", &oversized_body).0, 413);

        let query = "q".repeat(MAX_QUERY_CHARS + 1);
        let body = serde_json::json!({"query": query, "client": "test"}).to_string();
        assert_eq!(route(&store, "POST", "/recall", &body).0, 413);

        let content = "c".repeat(MAX_CONTENT_CHARS + 1);
        let body = serde_json::json!({"name": "large", "content": content}).to_string();
        assert_eq!(route(&store, "POST", "/put", &body).0, 413);

        let body =
            serde_json::json!({"query": "bounded k", "client": "test", "k": MAX_RECALL_K + 1})
                .to_string();
        assert_eq!(route(&store, "POST", "/recall", &body).0, 400);
    }

    #[test]
    fn curate_route_updates_the_resident_registry() {
        let store = MemoryStore::new();
        store
            .try_put(
                "curate-a",
                "Duplicate resident curate memory.",
                "global",
                cortex_core::MemoryTier::Episodic,
            )
            .unwrap();
        store
            .try_put(
                "curate-b",
                "Duplicate resident curate memory!",
                "global",
                cortex_core::MemoryTier::Episodic,
            )
            .unwrap();

        let response = route(&store, "POST", "/curate", r#"{"today":"2026-07-10"}"#);
        assert_eq!(response.0, 200, "response: {}", response.1);
        assert_eq!(store.len(), 1, "resident registry must reflect pruning");
    }

    // MBR-105: handshake gate middleware. The gate's three behaviors are:
    //   1. absent header → request passes through unchanged;
    //   2. malformed header → 400 with a descriptive reason;
    //   3. present-but-unparseable or wrong manifest → 400 / 421.
    // Cases (1) and (2) are observable without a published active manifest;
    // case (3) is covered by the membrane-protocol round-trip tests because
    // it depends on the OnceLock which can only be set once per process.
    #[tokio::test]
    async fn handshake_ingress_passes_through_when_header_is_absent() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let store = MemoryStore::new();
        let app = build_router(
            store,
            None,
            None,
            8765,
            None,
            std::time::Duration::from_secs(2),
            4,
        );
        // No X-Membrane-Manifest header — legacy client path.
        let response = app
            .oneshot(Request::get("/livez").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_ne!(
            response.status(),
            axum::http::StatusCode::MISDIRECTED_REQUEST,
            "absent handshake header must not produce 421"
        );
    }

    #[tokio::test]
    async fn handshake_ingress_rejects_garbage_header_with_400() {
        use axum::body::Body;
        use axum::http::{header, Request, StatusCode};
        use tower::ServiceExt;

        let store = MemoryStore::new();
        let app = build_router(
            store,
            None,
            None,
            8765,
            None,
            std::time::Duration::from_secs(2),
            4,
        );
        let response = app
            .oneshot(
                Request::get("/livez")
                    .header(
                        header::HeaderName::from_static(
                            crate::installation_manifest::HANDSHAKE_HEADER,
                        ),
                        "not-json-at-all",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "garbage X-Membrane-Manifest header must produce 400"
        );
    }
}
