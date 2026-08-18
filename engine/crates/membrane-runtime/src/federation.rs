//! Membrane federation gateway — Rust shell.
//!
//! Per dispatch §G3A + §G5: the authenticated local Membrane gateway
//! is the SOLE owner of provider fan-out and admission. Clients (Claude,
//! Codex, MCP) submit only (task, repo_root, client, session, max_tokens,
//! anchors, scope_grant_id); the gateway invokes provider adapters in
//! parallel, runs the deterministic in-process admission, and emits a
//! content-free ContextPacket v1 plus per-candidate ContextReceipt v2.
//!
//! This Rust module is the dispatcher: it spawns the Python federation
//! implementation at `engine/federation/gateway.py` (resolved by walking
//! up from the repo through the layouts in `GATEWAY_LAYOUTS`), parses the
//! assembled ContextCandidateSet v1, runs the existing pure-in-process
//! planner, and prints the final planner envelope to stdout.
//!
//! Provider payload formats and SQLite details never enter client
//! adapters. Crypt durable storage is never modified. Bearer tokens
//! are passed via the standard `CRYPT_API_TOKEN_FILE` env, never in
//! argv or stdout. ScopeGrant enforcement happens in the Python script.

use crypt_core::planner::{plan, ContextCandidateSetV1, PlannerInput};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

fn federation_session_id(session: Option<String>) -> String {
    session
        .unwrap_or_else(|| crate::store::opaque_correlation_token("anonymous-session", "session"))
}

/// Known gateway layouts relative to a candidate ancestor directory,
/// preferred first. The membrane consolidation moved the gateway out of
/// `tools/crypt/`; the legacy layout stays last so older checkouts and
/// frozen evidence hosts keep resolving.
const GATEWAY_LAYOUTS: [&[&str]; 3] = [
    // Parent workspace holding membrane as a nested checkout.
    &["membrane", "engine", "federation", "gateway.py"],
    // Standalone membrane checkout.
    &["engine", "federation", "gateway.py"],
    // Pre-consolidation workspace layout.
    &["tools", "crypt", "federation", "gateway.py"],
];

fn gateway_layout_path(dir: &Path, layout: &[&str]) -> PathBuf {
    layout
        .iter()
        .fold(dir.to_path_buf(), |acc, seg| acc.join(seg))
}

/// Walk up from `start` looking for the first directory holding any known
/// federation gateway layout. Returns the gateway script path itself, not
/// the workspace root — the two are no longer a fixed relative pair.
pub(crate) fn find_federation_gateway(start: &Path) -> Option<PathBuf> {
    let mut cursor: Option<&Path> = Some(start);
    while let Some(dir) = cursor {
        for layout in GATEWAY_LAYOUTS {
            let probe = gateway_layout_path(dir, layout);
            if probe.exists() {
                return Some(probe);
            }
        }
        cursor = dir.parent();
    }
    None
}

/// Run the federation gateway end-to-end. Spawns the Python gateway,
/// invokes the in-process planner on its assembled CCS, emits the final
/// envelope to stdout.
#[allow(clippy::too_many_arguments)]
pub fn run_federate(
    task: String,
    repo: PathBuf,
    max_tokens: usize,
    packet_char_budget_override: Option<usize>,
    packet_char_budget_model: Option<String>,
    client: String,
    session: Option<String>,
    anchors: Vec<String>,
    scope_grant_id: Option<String>,
    federation_script: Option<PathBuf>,
    accepted_receipt_versions: Vec<u32>,
) -> Result<(), String> {
    let script = match federation_script {
        // Caller supplied an explicit script — skip the walk-up entirely.
        Some(explicit) => explicit,
        None => find_federation_gateway(&repo).ok_or_else(|| {
            let layouts = GATEWAY_LAYOUTS
                .iter()
                .map(|layout| layout.join("/"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "could not locate federation gateway by walking up from {}; probed layouts: {layouts}; pass --federation-script",
                repo.display()
            )
        })?,
    };
    if !script.exists() {
        return Err(format!(
            "federation gateway script missing at {}. Run `python3 tools/setup-workspace.py` or pass --federation-script.",
            script.display()
        ));
    }
    let session_id = federation_session_id(session);
    let versions = if accepted_receipt_versions.is_empty() {
        vec![2u32]
    } else {
        accepted_receipt_versions
    };

    let mut cmd = resolve_python_invoker();
    cmd.arg(&script)
        .arg("--task")
        .arg(&task)
        .arg("--repo")
        .arg(&repo)
        .arg("--max-tokens")
        .arg(max_tokens.to_string())
        .arg("--client")
        .arg(&client)
        .arg("--session")
        .arg(&session_id);
    if !anchors.is_empty() {
        cmd.arg("--anchors").arg(anchors.join(","));
    }
    if let Some(grant_id) = scope_grant_id.as_ref() {
        cmd.arg("--scope-grant-id").arg(grant_id);
    }
    let gateway_started = Instant::now();
    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("spawn federation gateway: {e}"))?;
    let gateway_process_ms = gateway_started.elapsed().as_secs_f64() * 1000.0;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(format!(
            "federation gateway failed (exit={}): {}",
            output.status,
            stderr.chars().take(800).collect::<String>()
        ));
    }
    let payload = envelope_from_ccs(
        &stdout,
        EnvelopeInput {
            max_tokens,
            packet_char_budget_override,
            packet_char_budget_model,
            accepted_receipt_versions: versions,
            scope_grant_present: scope_grant_id.is_some(),
            gateway_process_ms,
        },
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).map_err(|e| format!("serialize: {e}"))?
    );
    Ok(())
}

/// Planner-side half of one federation cycle, shared verbatim by the CLI
/// (`run_federate`) and the resident `/federate` route: parse the gateway's
/// CCS line, surface fail-closed aborts, run the in-process planner, and
/// assemble the client envelope.
pub struct EnvelopeInput {
    pub max_tokens: usize,
    pub packet_char_budget_override: Option<usize>,
    pub packet_char_budget_model: Option<String>,
    pub accepted_receipt_versions: Vec<u32>,
    pub scope_grant_present: bool,
    /// Wall time spent obtaining the CCS (process spawn or worker roundtrip).
    pub gateway_process_ms: f64,
}

pub fn envelope_from_ccs(stdout: &str, input: EnvelopeInput) -> Result<Value, String> {
    // The gateway may emit a fail-closed envelope (exit 2) when a
    // ScopeGrant is rejected. Detect that envelope and surface it
    // before attempting strict CCS deserialization.
    let parse_started = Instant::now();
    let mut raw_value: Value = match serde_json::from_str(stdout) {
        Ok(v) => v,
        Err(e) => {
            return Err(format!(
                "federation gateway returned non-JSON payload: {e}; first 200 bytes: {}",
                stdout.chars().take(200).collect::<String>()
            ));
        }
    };
    if raw_value.get("_rightcontext").is_some() {
        if let Some(abort_reason) = raw_value
            .get("_rightcontext")
            .and_then(|v| v.get("abortReason"))
            .and_then(|v| v.as_str())
        {
            return Err(format!(
                "federation aborted by gateway: abortReason={abort_reason}; abortDetail={}",
                raw_value
                    .get("_rightcontext")
                    .and_then(|v| v.get("abortDetail"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("(none)")
            ));
        }
    }
    let observability = gateway_observability(&raw_value);
    let source_resolution_receipts =
        crate::source_resolution::gate_source_resolutions(&mut raw_value);
    let ccs: ContextCandidateSetV1 = match serde_json::from_value(raw_value) {
        Ok(v) => v,
        Err(e) => {
            return Err(format!(
                "federation gateway returned non-CCS payload: {e}; first 200 bytes: {}",
                stdout.chars().take(200).collect::<String>()
            ));
        }
    };
    let rust_parse_ms = parse_started.elapsed().as_secs_f64() * 1000.0;
    let planner_input = PlannerInput {
        candidate_set: ccs,
        max_tokens: input.max_tokens,
        packet_char_budget_override: input.packet_char_budget_override,
        packet_char_budget_model: input.packet_char_budget_model,
        accepted_receipt_versions: input.accepted_receipt_versions,
        trace_id_override: None,
        scope_grant_present: input.scope_grant_present,
    };
    let planner_started = Instant::now();
    let out = match plan(&planner_input) {
        Ok(o) => o,
        Err(e) => return Err(format!("planner rejected federation CSS: {e}")),
    };
    let rust_planner_ms = planner_started.elapsed().as_secs_f64() * 1000.0;
    let mut payload = serde_json::json!({
        "packet": out.packet,
        "receipts": out.receipts,
        "providerStatus": out.provider_status,
        "fallbackMode": out.fallback_mode,
        "degradationReason": out.degradation_reason,
        "sourceGeneration": out.source_generation,
        "expectedReleaseGeneration": out.expected_release_generation,
        "observedReleaseGeneration": out.observed_release_generation,
        "releaseGenerationStatus": out.release_generation_status,
        "structuredEvent": out.structured_event,
        "sourceResolutionReceipts": source_resolution_receipts,
    });
    if let Some(packet) = payload.get("packet") {
        payload["cachePrefixDiagnostic"] =
            serde_json::to_value(crate::cache_prefix::diagnose_cache_prefix(packet, None))
                .expect("cache prefix diagnostic serializes");
    }
    if let (Some(payload_fields), Some(observability_fields)) =
        (payload.as_object_mut(), observability.as_object())
    {
        payload_fields.extend(observability_fields.clone());
        let stages = payload_fields
            .entry("stageElapsedMs".to_string())
            .or_insert_with(|| serde_json::json!({}));
        if let Some(stage_fields) = stages.as_object_mut() {
            stage_fields.insert(
                "gateway_process".to_string(),
                input.gateway_process_ms.into(),
            );
            stage_fields.insert("rust_parse".to_string(), rust_parse_ms.into());
            stage_fields.insert("rust_planner".to_string(), rust_planner_ms.into());
        }
    }
    Ok(payload)
}

/// Membrane Crypt durable-memory candidate provider. Pure in-process
/// read of eligible MemoryEntry rows normalised into ContextCandidateSet v1
/// records (Layer 7, sourceKind "memory", trustClass "agent_verified").
#[allow(clippy::too_many_arguments)]
pub fn run_memory_candidates(
    task: String,
    repo: PathBuf,
    scope: Option<String>,
    max_candidates: usize,
    _scope_grant_id: Option<String>,
) -> Result<(), String> {
    let canonical_repo = repo
        .canonicalize()
        .map_err(|e| format!("resolve repo: {e}"))?;
    let workspace = canonical_repo
        .parent()
        .ok_or_else(|| "repo has no parent".to_string())?
        .to_path_buf();
    let db_path = db_path_for(&workspace);
    let db = crate::MemDb::open(&db_path)
        .map_err(|e| format!("open crypt db at {}: {e}", db_path.display()))?;
    let store = crate::MemoryStore::try_open(db).map_err(|e| format!("open MemoryStore: {e}"))?;

    let scope_id = scope.clone().unwrap_or_else(|| "D--Claude".to_string());
    // The CLI always has a real, already-canonicalized repo root in hand (canonicalize()
    // above already succeeded or this function would have returned), so this call site
    // always has a genuine freshness signal available — unlike the resident-serve HTTP
    // route, which does not (see the /memory-candidates handler in serve.rs).
    let payload = memory_candidates_payload(
        &store,
        &task,
        &scope_id,
        max_candidates,
        Some(&canonical_repo),
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).map_err(|e| format!("serialize: {e}"))?
    );
    Ok(())
}

/// Testable core: build the Crypt ContextCandidateSet from REAL relevance-ranked memories.
///
/// Uses `recall_scored` (the same full-corpus hybrid retriever that backs live `context_for`),
/// not an arbitrary `entries(max)` slice — so results are relevant. Emits `text` = a bounded
/// word-boundary content preview (the old code emitted `text = e.id`, i.e. a useless slug), and a
/// real content hash. Feedback-rail vetoes are applied via `gate_history_for` so a memory the agent
/// marked `contradicted` never surfaces here either.
pub fn memory_candidates_payload(
    store: &crate::MemoryStore,
    task: &str,
    scope_id: &str,
    max_candidates: usize,
    repo_root: Option<&Path>,
) -> serde_json::Value {
    memory_candidates_payload_for_descriptor(
        store,
        task,
        &crate::scope::ScopeDescriptorV1::filesystem(scope_id),
        max_candidates,
        repo_root,
    )
    .unwrap_or_else(|error| serde_json::json!({"error": error, "candidates": []}))
}

/// Reason code for a memory candidate that scored well enough to be considered but was cut by
/// the caller's `max_candidates` ceiling. Mirrors the `Omission.reason` convention in
/// `memory_provider.rs::reasons`, scoped locally since this function's admission model (a single
/// recall pass, not `consider_entries`/`partition`) has no other omission source to report.
const OMISSION_REASON_CEILING_TRUNCATED: &str = "ceiling_truncated";

/// Descriptor-aware candidate surface. Virtual scope ancestry is exact and opaque; a legacy
/// string is deliberately represented as a filesystem descriptor by the compatibility wrapper.
pub fn memory_candidates_payload_for_descriptor(
    store: &crate::MemoryStore,
    task: &str,
    descriptor: &crate::scope::ScopeDescriptorV1,
    max_candidates: usize,
    repo_root: Option<&Path>,
) -> Result<serde_json::Value, String> {
    // Canonicalize whatever the caller sent (raw filesystem path, slug, or `global`) into the full
    // visibility chain: self + ancestor scopes that hold rows + global. Before 2026-07-16 this
    // passed the raw string into recall (clients send paths like `D:\Claude`), so project-scoped
    // rows never matched and the rich path recalled from the global corpus only (Sol audit P0).
    let scope_started = Instant::now();
    let scopes = descriptor
        .resolve_chain(&store.scopes())
        .map_err(|error| format!("invalid scope descriptor: {error}"))?;
    let scope_ms = scope_started.elapsed().as_secs_f64() * 1000.0;
    // Shared recall owns one-hop augmentation, its bounded graph lane, and effectiveness vetoes.
    // Keeping those policies here as well would double-expand candidates and split behavior across
    // live recall, replay, and federation.
    //
    // F11: request one MORE than the ceiling so a real ceiling-truncation can be told apart from
    // "nothing else scored" without changing what gets served — the first `max_candidates` hits of
    // an N+1 request are the same top-N a request for exactly N would have returned (ranking is
    // deterministic; see `recall_scored`'s doc comment), so this changes zero user-visible output.
    let probe_limit = max_candidates.saturating_add(1);
    let (mut hits, mut stage_elapsed) = store.recall_scored_timed(task, probe_limit, &scopes);
    stage_elapsed.recall_ms += scope_ms;
    let dropped_by_ceiling = if hits.len() > max_candidates {
        hits.split_off(max_candidates)
    } else {
        Vec::new()
    };
    let rank_started = Instant::now();
    let candidates: Vec<serde_json::Value> = hits
        .iter()
        .map(|(e, score)| {
            let preview = memory_preview(&e.content);
            serde_json::json!({
                "id": format!("memory:role:{}", e.id),
                "layer": 7,
                "sourceKind": "memory",
                "sourceRef": e.scope_id.clone(),
                "sourceHash": sha256_hex(&e.content),
                "trustClass": "agent_verified",
                "instructionPolicy": "data_only",
                "providerScore": score.clamp(0.0, 1.0),
                // `structural` is the key the planner's memory-relevance gate reads
                // (planner.rs: structural<=0 && lexical<0.85 -> memory_low_relevance). Emit the
                // cosine relevance as structural so real hits clear the gate.
                "scoreComponents": {"structural": score.clamp(0.0, 1.0), "relevance": score.clamp(0.0, 1.0)},
                "estimatedTokens": std::cmp::max(1, preview.chars().count() / 4),
                "protected": false,
                "exact": false,
                "recoverable": true,
                "resolver": format!("crypt get {}", e.id),
                "text": preview,
            })
        })
        .collect();
    let omissions: Vec<serde_json::Value> = dropped_by_ceiling
        .iter()
        .map(|(e, _score)| {
            serde_json::json!({
                "id": format!("memory:role:{}", e.id),
                "layer": 7,
                "reason": OMISSION_REASON_CEILING_TRUNCATED,
            })
        })
        .collect();
    stage_elapsed.rank_ms += rank_started.elapsed().as_secs_f64() * 1000.0;

    // F11: reuse the freshness verdict machinery `/freshness` already exposes rather than
    // inventing a second staleness concept. `stable` is the verdict's own truth-in-observation
    // signal (the epoch sandwich did or did not hold across the read); its negation is `stale`.
    // When no repo root is available at this call site at all, that is itself an unverifiable
    // condition — express it honestly as `stale: true`, never fall back to `false`.
    let stale = repo_root.is_none_or(|root| {
        !crate::freshness::evaluate_repository_freshness(store, root.to_path_buf()).stable
    });

    Ok(serde_json::json!({
        "schemaVersion": 1,
        "traceId": new_trace_id(),
        "task": task,
        "mode": "verify",
        "provider": "crypt",
        "freshness": {
            "revision": crypt_revision(),
            "indexedAt": iso_now(),
            "stale": stale,
        },
        "providerCeiling": {
            "maxCandidates": max_candidates,
            "maxEstimatedTokens": 4096,
        },
        "candidates": candidates,
        "omissions": omissions,
        "scope": scopes.first().cloned().unwrap_or_default(),
        "_rightcontext": {
            "stageElapsedMs": {
                "embed": stage_elapsed.embed_ms,
                "recall": stage_elapsed.recall_ms,
                "rank": stage_elapsed.rank_ms,
            }
        },
    }))
}

/// Bounded, word-boundary content preview for a memory candidate's delivered text.
fn memory_preview(content: &str) -> String {
    const CAP: usize = 200;
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= CAP {
        return normalized;
    }
    let truncated: String = normalized.chars().take(CAP).collect();
    let cut = truncated.rfind(' ').unwrap_or(truncated.len());
    format!("{}…", &truncated[..cut])
}

fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

fn db_path_for(workspace: &Path) -> PathBuf {
    std::env::var("CRYPT_DB")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            let home = if cfg!(windows) {
                std::env::var_os("USERPROFILE").map(PathBuf::from)
            } else {
                std::env::var_os("HOME").map(PathBuf::from)
            };
            home.map(|p| p.join(".claude").join("crypt").join("crypt.db"))
        })
        .unwrap_or_else(|| {
            workspace
                .join("tools")
                .join(".cache")
                .join("crypt")
                .join("crypt.db")
        })
}

fn new_trace_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("rc-mem-{ts:x}")
}

fn iso_now() -> String {
    // Best-effort UTC ISO-8601. Python gateway produces the canonical format;
    // this is only used by the standalone memory-candidates CLI.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let sod = secs % 86_400;
    let h = sod / 3600;
    let m = (sod % 3600) / 60;
    let s = sod % 60;
    format!("1970-01-01T00:00:00Z+{days}T{h:02}:{m:02}:{s:02}Z")
}

fn crypt_revision() -> String {
    std::env::var("CRYPT_REVISION").unwrap_or_else(|_| "crypt-0.1.1-federation".to_string())
}

fn gateway_observability(raw: &Value) -> Value {
    let mut fields = serde_json::Map::new();
    if let Some(rightcontext) = raw.get("_rightcontext") {
        for field in [
            "providerCounts",
            "providerWarnings",
            "providerElapsedMs",
            "providerStageElapsedMs",
            "serviceGeneration",
            "firstAfterIdle",
            "idleGapMs",
            "stageElapsedMs",
        ] {
            if let Some(value) = rightcontext.get(field) {
                fields.insert(field.to_string(), value.clone());
            }
        }
    }
    if let Some(graph_state) = raw
        .get("freshness")
        .and_then(|freshness| freshness.get("graphState"))
    {
        fields.insert("graphState".to_string(), graph_state.clone());
    }
    Value::Object(fields)
}

#[cfg(test)]
mod observability_tests {
    use super::gateway_observability;

    #[test]
    fn preserves_content_free_gateway_observability_for_clients() {
        let raw = serde_json::json!({
            "freshness": {"graphState": "dirty_overlay"},
            "_rightcontext": {
                "providerCounts": {"git": 2},
                "providerWarnings": [],
                "providerElapsedMs": {"git": 1.25},
                "providerStageElapsedMs": {"crypt": {"embed": 2.5, "recall": 3.5}},
                "stageElapsedMs": {"freshness": 2.0, "provider_fanout": 3.0},
                "idleGapMs": 300001,
                "serviceGeneration": "svc-test",
                "firstAfterIdle": true
            }
        });

        assert_eq!(
            gateway_observability(&raw),
            serde_json::json!({
                "providerCounts": {"git": 2},
                "providerWarnings": [],
                "providerElapsedMs": {"git": 1.25},
                "providerStageElapsedMs": {"crypt": {"embed": 2.5, "recall": 3.5}},
                "stageElapsedMs": {"freshness": 2.0, "provider_fanout": 3.0},
                "idleGapMs": 300001,
                "graphState": "dirty_overlay",
                "serviceGeneration": "svc-test",
                "firstAfterIdle": true
            })
        );
    }
}

/// Locate the Python interpreter that can run the federation gateway.
///
/// Honours `PYTHON` env override, then `python3` / `python` / `py` on
/// PATH (POSIX), then the Windows `py -3.11` launcher, then a hard-coded
/// Windows fallback. Returns a `Command` with the chosen program as
/// argv[0]; callers append the script path + flags themselves.
pub(crate) fn resolve_python_invoker() -> Command {
    if let Ok(p) = std::env::var("PYTHON") {
        if !p.is_empty() {
            eprintln!("[federation] using PYTHON override: {}", p);
            return Command::new(p);
        }
    }
    for name in ["python3", "python", "py"] {
        if let Some(found) = which(name) {
            // On Windows `py` requires `-3.11` to disambiguate versions.
            // Other interpreters ignore the flag.
            let mut cmd = Command::new(&found);
            let s = found.to_string_lossy().to_ascii_lowercase();
            let is_py = s.ends_with("py.exe") || s.ends_with("py.cmd") || s.ends_with("py.bat");
            if is_py {
                cmd.arg("-3.11");
            }
            eprintln!(
                "[federation] resolved Python: {} (py_launcher={})",
                found.display(),
                is_py
            );
            return cmd;
        }
    }
    // Last-resort fallback for hosts where Python is installed but not
    // on PATH. The Windows installer commonly drops `py.exe` under
    // `%LOCALAPPDATA%\Programs\Python\Python311`.
    let guesses: &[&str] = if cfg!(windows) {
        &[
            r"C:\Python311\python.exe",
            r"C:\Program Files\Python311\python.exe",
            r"C:\Users\Default\AppData\Local\Programs\Python\Python311\python.exe",
        ]
    } else {
        &["/usr/bin/python3", "/usr/local/bin/python3"]
    };
    for guess in guesses {
        if Path::new(guess).exists() {
            eprintln!("[federation] using guess path: {}", guess);
            return Command::new(guess);
        }
    }
    Command::new("python3")
}

/// Tiny PATH lookup. Avoids pulling in the `which` crate dependency.
///
/// On Windows the executable launcher is usually `py.exe` even when the
/// PATH entry is named `py` (or absent an extension entirely). Try the
/// `.exe` suffix BEFORE the bare name to avoid running a CMD shim or a
/// non-executable file with the same stem. POSIX is unaffected.
///
/// Skip MSYS2/Git-Bash shim directories (`/c/Users/<u>/bin`, `/usr/bin`,
/// `/mingw64/bin`) — those are shell-script launchers (e.g.
/// `python3` → `exec python`) and Rust's `Command::spawn` rejects them
/// with os error 193 ("not a valid Win32 application").
fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let suffixes: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    for entry in std::env::split_paths(&path_var) {
        let entry_str = entry.to_string_lossy().to_ascii_lowercase();
        if cfg!(windows) {
            // Skip MSYS2/Git-Bash launcher directories — those are shell
            // script wrappers, not native Win32 binaries, and Rust's
            // Command::spawn rejects them with os error 193.
            let is_user_bin = entry_str.starts_with("c:\\users\\") && entry_str.ends_with("\\bin");
            let is_usr_bin = entry_str == "\\usr\\bin" || entry_str == "c:\\usr\\bin";
            let is_mingw_bin = entry_str == "\\mingw64\\bin" || entry_str == "c:\\mingw64\\bin";
            if is_user_bin || is_usr_bin || is_mingw_bin {
                continue;
            }
        }
        for suffix in suffixes {
            let candidate = entry.join(format!("{name}{suffix}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] #[rustfmt::skip] fn federation_emits_content_free_cache_prefix_diagnostic() {
        let mut source: Value = serde_json::from_str(include_str!("../../../../schemas/registry/context-candidate-set.v1.golden.json")).unwrap(); source["generationId"] = serde_json::json!("gen-current"); source["candidates"][0]["sourceKind"] = serde_json::json!("graph");
        source["candidates"][0]["sourceResolution"] = serde_json::json!({"schemaVersion":1,"candidateId":"cand-blueprint-types","provider":"cortex-treesitter","status":"resolved","expectedHash":"sha256:0000000000000000000000000000000000000000000000000000000000000011","resolvedHash":"sha256:0000000000000000000000000000000000000000000000000000000000000011","expectedGeneration":"gen-current","resolvedGeneration":"gen-current","expectedPath":"engine/crates/membrane-protocol/src/types.rs:1-60","resolvedPath":"engine/crates/membrane-protocol/src/types.rs:1-60","resolver":"source_read"}); let ccs = serde_json::to_string(&source).unwrap();
        let payload = envelope_from_ccs(
            &ccs,
            EnvelopeInput {
                max_tokens: 4096,
                packet_char_budget_override: None,
                packet_char_budget_model: None,
                accepted_receipt_versions: vec![2],
                scope_grant_present: false,
                gateway_process_ms: 0.0,
            },
        )
        .expect("golden CCS plans");
        let diagnostic = &payload["cachePrefixDiagnostic"];
        assert_eq!(diagnostic["schemaVersion"], 1);
        let serialized = serde_json::to_string(diagnostic).unwrap();
        assert!(!serialized.contains("ScopeGrantV1"));
        assert!(!serialized.contains("single source of truth"));
        assert!(diagnostic["blockDigests"].is_array());
        assert_eq!(payload["sourceResolutionReceipts"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn missing_session_uses_canonical_opaque_identity() {
        let generated = federation_session_id(None);
        assert!(generated.starts_with("session-"));
        assert_eq!(generated.len(), 40);
        assert!(generated[8..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')));
        assert_ne!(generated, "anonymous-session");
        assert_eq!(
            federation_session_id(Some("scope-grant-session".to_string())),
            "scope-grant-session"
        );
    }

    #[test]
    fn memory_candidates_rank_topical_and_emit_content_preview() {
        let store = crate::MemoryStore::new();
        let _ = store.remember(
            "Always answer briefly and tersely, cutting all filler and preamble.",
            vec![],
        );
        let _ = store.remember(
            "The nginx container is dockerized; diff the confs before any rebuild.",
            vec![],
        );
        let _ = store.remember(
            "Vast.ai GPU rental uses the vastai CLI and an API key from the env.",
            vec![],
        );
        let payload = memory_candidates_payload(
            &store,
            "answer briefly and tersely please",
            "global",
            5,
            None,
        );
        let cands = payload["candidates"].as_array().expect("candidates array");
        assert!(!cands.is_empty(), "expected memory candidates");
        let top = &cands[0];
        let text = top["text"].as_str().unwrap().to_lowercase();
        // Relevance: the brief memory wins for a brief/terse query.
        assert!(
            text.contains("briefly") || text.contains("tersely"),
            "top candidate text should be the brief memory content, got: {text}"
        );
        // text is CONTENT, not an id/slug (the bug this fixes).
        assert!(!text.starts_with("memory:role:") && !text.starts_with("mem-"));
        assert!(top["resolver"].as_str().unwrap().starts_with("crypt get "));
    }

    fn touch_gateway(root: &Path, layout: &[&str]) -> PathBuf {
        let path = gateway_layout_path(root, layout);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"# fixture").unwrap();
        path
    }

    #[test]
    fn gateway_resolves_membrane_layout_by_walking_up_from_a_nested_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path();
        let expected = touch_gateway(workspace, GATEWAY_LAYOUTS[0]);
        let nested = workspace.join("someapp").join("src");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(find_federation_gateway(&nested), Some(expected));
    }

    #[test]
    fn gateway_resolves_a_standalone_membrane_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let expected = touch_gateway(tmp.path(), GATEWAY_LAYOUTS[1]);

        assert_eq!(find_federation_gateway(tmp.path()), Some(expected));
    }

    #[test]
    fn gateway_prefers_membrane_over_the_legacy_layout_when_both_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let membrane = touch_gateway(tmp.path(), GATEWAY_LAYOUTS[0]);
        let legacy = touch_gateway(tmp.path(), GATEWAY_LAYOUTS[2]);
        assert!(legacy.exists(), "legacy fixture must exist for the contest");

        assert_eq!(find_federation_gateway(tmp.path()), Some(membrane));
    }

    #[test]
    fn gateway_still_resolves_a_pre_consolidation_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let expected = touch_gateway(tmp.path(), GATEWAY_LAYOUTS[2]);

        assert_eq!(find_federation_gateway(tmp.path()), Some(expected));
    }

    #[test]
    fn gateway_resolution_is_none_when_no_layout_exists() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(find_federation_gateway(tmp.path()), None);
    }

    #[test]
    fn memory_preview_truncates_on_word_boundary() {
        let long = "word ".repeat(100);
        let p = memory_preview(&long);
        assert!(p.chars().count() <= 201, "preview must be capped");
        assert!(p.ends_with('…'));
        assert!(
            !p.contains("wor…"),
            "must cut on a word boundary, not mid-word"
        );
    }

    /// Minimal, hermetic git repo fixture for the F11 freshness tests below — explicit identity
    /// flags so this works in any sandbox regardless of global git config.
    fn init_git_repo(dir: &Path) {
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("git must be available to run this test");
            assert!(status.success(), "git {args:?} failed in {}", dir.display());
        };
        run(&["init", "--quiet"]);
        run(&[
            "-c",
            "user.email=federation-test@example.com",
            "-c",
            "user.name=federation-test",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "init",
        ]);
    }

    /// F11 — `stale` must reflect a real freshness verdict, not a hardcoded literal. A real,
    /// coherent git repository (even with no Blueprint/graph snapshot yet — `MissingSnapshot` is
    /// still a stable observation per `freshness::classify`) must report `stale: false`.
    #[test]
    fn stale_is_false_for_a_real_coherent_repository() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        let store = crate::MemoryStore::new();
        let payload = memory_candidates_payload(&store, "any task", "global", 5, Some(repo.path()));
        assert_eq!(
            payload["freshness"]["stale"], false,
            "a real, coherent repository must report stale=false, payload={payload}"
        );
    }

    /// F11 — the inverse: a directory that is not a git repository at all makes the freshness
    /// epoch unreadable (Indeterminate/unstable), and having no repo root at all is itself an
    /// unverifiable condition. Both must honestly report `stale: true` — never fall back to the
    /// old hardcoded `false`.
    #[test]
    fn stale_is_true_when_the_freshness_signal_cannot_be_verified() {
        let store = crate::MemoryStore::new();

        let non_repo = tempfile::tempdir().unwrap();
        let broken =
            memory_candidates_payload(&store, "any task", "global", 5, Some(non_repo.path()));
        assert_eq!(broken["freshness"]["stale"], true);

        let unknown = memory_candidates_payload(&store, "any task", "global", 5, None);
        assert_eq!(unknown["freshness"]["stale"], true);
    }

    /// F11 — candidates pushed past the caller's `max_candidates` ceiling must be recorded as
    /// omissions, not silently dropped, while the served candidate list is still capped exactly
    /// as before.
    #[test]
    fn omissions_reports_candidates_dropped_by_the_ceiling_truncation() {
        let store = crate::MemoryStore::new();
        for n in 0..5 {
            let _ = store.remember(
                &format!("ceiling truncation fixture memory entry number {n}"),
                vec![],
            );
        }
        let payload = memory_candidates_payload(
            &store,
            "ceiling truncation fixture memory entry",
            "global",
            2,
            None,
        );
        let candidates = payload["candidates"].as_array().expect("candidates array");
        assert_eq!(
            candidates.len(),
            2,
            "the ceiling must still cap what is served, payload={payload}"
        );
        let omissions = payload["omissions"].as_array().expect("omissions array");
        assert!(
            !omissions.is_empty(),
            "entries pushed past the ceiling must be recorded as omissions, not silently \
             dropped: {payload}"
        );
        assert!(omissions
            .iter()
            .all(|omission| omission["reason"] == "ceiling_truncated"));
    }
}
