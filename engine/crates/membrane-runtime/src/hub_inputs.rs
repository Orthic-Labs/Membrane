//! MBR: live source for the Hub popover.
//!
//! `modes.rs` used to build `HubInputsV1::unavailable("source_not_connected")`
//! unconditionally, so the popover showed "Offline" even while the local
//! Membrane resident was up and healthy. This module replaces that hardcoded
//! facade with a real (best-effort) read of the local service's
//! unauthenticated `GET /health` endpoint, mapped into the same
//! `HubInputsV1` contract the facade already understands. Any failure to
//! reach or parse the service falls back to `None`, and the caller keeps the
//! existing "unavailable" behavior — we never fabricate readiness.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::hub::{HubInputsV1, HubMetadataV1, HubReadV1, HubSubsystemInputsV1};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const IO_TIMEOUT: Duration = Duration::from_millis(300);
pub const DEFAULT_LOCAL_SERVICE_PORT: u16 = 47851;

/// Live snapshot parts produced from the resident `/health` — inputs plus
/// frozen parent state and six semantic subsystem surfaces.
#[derive(Debug, Clone)]
pub struct LiveSnapshotParts {
    pub inputs: HubInputsV1,
    pub membrane_state: membrane_protocol::MembraneParentState,
    pub subsystems: membrane_protocol::HubSubsystemsV1,
    /// Additive V1 admission-ledger aggregate. `None` when the catalog
    /// receipt store is unreadable — never a fabricated zero.
    pub admission: Option<membrane_protocol::HubAdmissionV1>,
}

/// Best-effort live read of local Membrane resident's `/health` endpoint,
/// mapped into `HubInputsV1`. Returns `None` on any connection, I/O, or
/// parse failure so the caller can fall back to the honest "unavailable"
/// facade rather than show a fabricated state.
pub fn live_inputs_from_local_service() -> Option<HubInputsV1> {
    live_snapshot_parts_from_local_service().map(|parts| parts.inputs)
}

/// Live read that also returns frozen parent state and subsystem map.
/// Returns `None` only when `/health` is unreachable (Offline case); caller
/// should fall back to `HubInputsV1::unavailable` and treat parent as Offline.
pub fn live_snapshot_parts_from_local_service() -> Option<LiveSnapshotParts> {
    let port = local_service_port()?;
    let health = fetch_health_json(port)?;
    let workspace_root = configured_workspace_root();
    let delivery = read_delivery_health_json(&workspace_root);
    let blueprint = crate::freshness::read_blueprint_status(&workspace_root);
    Some(snapshot_parts_from_health(
        &health,
        delivery.as_ref(),
        blueprint,
    ))
}

/// Canonical Hub snapshot composition from observed parts.
///
/// This is the ONE producer for the canonical snapshot shape. Both the HTTP
/// `/hub/snapshot` route and `membrane cli hub-snapshot` must emit exactly
/// this — same sections, typed frozen `membraneState`, and all six typed
/// subsystems — so no consumer can observe two different parent truths.
pub fn compose_hub_snapshot(
    parts: LiveSnapshotParts,
    observed_at_unix_ms: u64,
) -> membrane_protocol::HubSnapshotV1 {
    let reachable = parts.membrane_state != membrane_protocol::MembraneParentState::Offline;
    let facade = crate::hub::HubFacadeV1::new(reachable.then(|| membrane_protocol::HubStreamV1 {
        state: membrane_protocol::HubStateV1::Available,
        reason: "observed".into(),
        resolver: Some("hub_inputs::live_inputs_from_local_service".into()),
    }));
    facade.snapshot_with_admission(
        observed_at_unix_ms,
        parts.inputs,
        Some(parts.membrane_state),
        Some(parts.subsystems),
        parts.admission,
    )
}

/// Canonical snapshot straight from the live resident (or the honest offline
/// fallback). The exact function behind `membrane cli hub-snapshot` and the
/// resident's `/hub/snapshot` route.
pub fn compose_live_hub_snapshot() -> membrane_protocol::HubSnapshotV1 {
    let parts = live_snapshot_parts_from_local_service().unwrap_or_else(offline_snapshot_parts);
    compose_hub_snapshot(parts, now_unix_ms())
}

/// Pure mapping for tests — same as `live_snapshot_parts_from_local_service`
/// but with injected health/blueprint values.
pub fn snapshot_parts_from_health(
    health: &serde_json::Value,
    delivery: Option<&serde_json::Value>,
    blueprint: Result<serde_json::Value, String>,
) -> LiveSnapshotParts {
    let inputs = inputs_from_health_with_blueprint(health, delivery, blueprint.clone());
    let health_ok = health.get("ok").and_then(serde_json::Value::as_bool);
    // Live snapshot is available when we reached /health and parsed it.
    let live_available = true;
    let parent = membrane_protocol::membrane_parent_state(true, health_ok, live_available);
    let subsystems =
        subsystem_inputs_from_health(health, blueprint, &inputs.memory, &inputs.sentinel)
            .subsystems();
    let admission = crate::admission_producer::build_admission_report();
    LiveSnapshotParts {
        inputs,
        membrane_state: parent,
        subsystems,
        admission,
    }
}

/// Snapshot parts for the offline fallback (health unreachable).
pub fn offline_snapshot_parts() -> LiveSnapshotParts {
    let mut inputs = HubInputsV1::unavailable("hub_inactive");
    // Offline means no resident Hub exists. Keep that diagnosis distinct from
    // a live Hub whose Blueprint transport or repository is unavailable.
    inputs.repositories = HubReadV1::Unavailable {
        reason: "hub_inactive".into(),
    };
    let subsystem_inputs = HubSubsystemInputsV1::unavailable("hub_inactive");
    let subsystems = subsystem_inputs.subsystems();
    LiveSnapshotParts {
        inputs,
        membrane_state: membrane_protocol::MembraneParentState::Offline,
        subsystems,
        admission: None,
    }
}

/// Build the six semantic subsystem surfaces from actual typed evidence.
///
/// Ownership:
/// - Pull — semantic evidence retrieval (pull/federation). No trustworthy health
///   producer yet → Not configured (Un­available + not_instrumented).
/// - Push — reversible reduction. No instrumentation → Not configured.
/// - Cortex — durable knowledge (MemoryStore + sentinel). Source: memory
///   (catalog/database health) + sentinel (memory_sentinel). Available when
///   both are Available; Degraded when either is Degraded; otherwise Unavailable.
/// - Blueprint — repository truth. Source: live public Blueprint IPC status
///   (freshness.rs::read_blueprint_status) — reuse existing seam.
/// - Ledger — document navigation/index. No trustworthy Ledger health producer
///   yet → Not configured. Do NOT conflate with Cortex/sentinel.
/// - Adapt — learning/proposals. No instrumentation → Not configured.
fn subsystem_inputs_from_health(
    _health: &serde_json::Value,
    blueprint: Result<serde_json::Value, String>,
    memory: &HubReadV1,
    sentinel: &HubReadV1,
) -> HubSubsystemInputsV1 {
    let not_instrumented = HubReadV1::Unavailable {
        reason: "not_instrumented".into(),
    };
    let blueprint_hub = blueprint_service_hub_read(blueprint);
    let cortex = cortex_hub_read(memory, sentinel);
    HubSubsystemInputsV1 {
        pull: not_instrumented.clone(),
        push: not_instrumented.clone(),
        cortex,
        blueprint: blueprint_hub,
        ledger: not_instrumented.clone(),
        adapt: not_instrumented,
    }
}

fn cortex_hub_read(memory: &HubReadV1, sentinel: &HubReadV1) -> HubReadV1 {
    match (memory, sentinel) {
        (HubReadV1::Available { .. }, HubReadV1::Available { .. }) => HubReadV1::Available {
            items: vec![serde_json::json!({"owner": "cortex", "evidence": "health+sentinel"})],
            metadata: HubMetadataV1 {
                resolver: Some("hub_inputs::cortex_hub_read".into()),
                source: Some("cortex".into()),
                evidence: Some("GET /health + sentinel".into()),
                observed_at_unix_ms: now_unix_ms(),
                cache_age_ms: 0,
            },
        },
        (HubReadV1::Degraded { .. }, _) | (_, HubReadV1::Degraded { .. }) => HubReadV1::Degraded {
            reason: "cortex_degraded".into(),
            items: vec![serde_json::json!({"owner": "cortex"})],
            metadata: HubMetadataV1 {
                resolver: Some("hub_inputs::cortex_hub_read".into()),
                source: Some("cortex".into()),
                evidence: Some("GET /health + sentinel".into()),
                observed_at_unix_ms: now_unix_ms(),
                cache_age_ms: 0,
            },
        },
        _ => {
            let reason = match (memory, sentinel) {
                (HubReadV1::Unavailable { reason }, _) => reason.clone(),
                (_, HubReadV1::Unavailable { reason }) => reason.clone(),
                _ => "cortex_unavailable".into(),
            };
            HubReadV1::Unavailable { reason }
        }
    }
}

fn local_service_port() -> Option<u16> {
    local_service_port_from(std::env::var_os("MEMBRANE_PORT").as_deref())
}

fn local_service_port_from(canonical: Option<&std::ffi::OsStr>) -> Option<u16> {
    match canonical {
        None => Some(DEFAULT_LOCAL_SERVICE_PORT),
        Some(value) => value
            .to_str()?
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0),
    }
}

/// Mirrors `serve.rs::configured_workspace_root()` (private to that module),
/// so we replicate the same env-var precedence here rather than reach across
/// a module boundary that wasn't designed to be shared.
fn configured_workspace_root() -> std::path::PathBuf {
    std::env::var_os("MEMBRANE_REPO_ROOT")
        .or_else(|| std::env::var_os("WORKSPACE_ROOT"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        })
}

fn fetch_health_json(port: u16) -> Option<serde_json::Value> {
    let addr = format!("127.0.0.1:{port}");
    let mut stream = TcpStream::connect_timeout(&addr.parse().ok()?, CONNECT_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok()?;

    let request =
        format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw);
    // Split HTTP headers from body on the first blank line; be tolerant of
    // both CRLF and bare LF framing.
    let body = text
        .split_once("\r\n\r\n")
        .or_else(|| text.split_once("\n\n"))
        .map(|(_, body)| body)?;
    serde_json::from_str(body.trim()).ok()
}

fn read_delivery_health_json(workspace_root: &std::path::Path) -> Option<serde_json::Value> {
    let path = workspace_root.join("tools/.cache/memory/delivery-health.json");
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Pure mapping from the raw `/health` JSON (and optional delivery-health
/// JSON) to `HubInputsV1`. Kept separate from the networking so it can be
/// unit-tested with fixture strings and no live service.
fn inputs_from_health(
    health: &serde_json::Value,
    delivery: Option<&serde_json::Value>,
) -> HubInputsV1 {
    inputs_from_health_with_blueprint(
        health,
        delivery,
        Err("Blueprint daemon status unavailable".into()),
    )
}

fn inputs_from_health_with_blueprint(
    health: &serde_json::Value,
    delivery: Option<&serde_json::Value>,
    blueprint: Result<serde_json::Value, String>,
) -> HubInputsV1 {
    let observed_at_unix_ms = now_unix_ms();
    let metadata = || HubMetadataV1 {
        resolver: Some("hub_inputs::live_inputs_from_local_service".into()),
        source: Some("local_membrane_resident".into()),
        evidence: Some("GET /health".into()),
        observed_at_unix_ms,
        cache_age_ms: 0,
    };

    let catalog_ok = health["catalog"]["status"].as_str() == Some("ok");
    let database_ok = health["database"]["status"].as_str() == Some("ok");
    let database_status = health["database"]["status"].as_str().unwrap_or("unknown");
    let daily_analysis_status = health["dailyAnalysis"]["status"]
        .as_str()
        .unwrap_or("unknown");
    let daily_analysis_alert = health["dailyAnalysis"]["alert"].as_bool();
    let daily_analysis_ok =
        matches!(daily_analysis_status, "fresh" | "ok") && daily_analysis_alert != Some(true);

    let memory = if catalog_ok && database_ok {
        HubReadV1::Available {
            items: vec![serde_json::json!({
                "catalog": health["catalog"]["status"],
                "database": health["database"],
                "memoryCount": health["database"]["memoryCount"],
            })],
            metadata: metadata(),
        }
    } else {
        // c191b240: empty corpus (database.status == "empty") must surface as non-healthy with memoryCount
        // preserved — degraded, not ok, and evidence includes count so empty is not silent.
        let reason = if database_status == "empty" {
            "database_empty".to_string()
        } else {
            "catalog_or_database_unhealthy".to_string()
        };
        HubReadV1::Degraded {
            reason,
            items: vec![serde_json::json!({
                "catalog": health["catalog"]["status"],
                "database": health["database"],
                "memoryCount": health["database"]["memoryCount"],
                "databaseStatus": database_status,
            })],
            metadata: metadata(),
        }
    };

    let providers_items = vec![
        serde_json::json!({"service": "membrane", "ok": health["ok"]}),
        serde_json::json!({"dailyAnalysis": daily_analysis_status}),
    ];
    let providers = if daily_analysis_ok {
        HubReadV1::Available {
            items: providers_items,
            metadata: metadata(),
        }
    } else {
        HubReadV1::Degraded {
            reason: "daily_analysis_unhealthy".into(),
            items: providers_items,
            metadata: metadata(),
        }
    };

    let deliveries = match delivery {
        None => HubReadV1::Unavailable {
            reason: "delivery_health_missing".into(),
        },
        Some(delivery) => {
            let consecutive_zero = delivery["consecutiveZero"].as_i64().unwrap_or(0);
            let item = serde_json::json!({
                "consecutiveZero": delivery["consecutiveZero"],
                "lastNonEmptyAt": delivery["lastNonEmptyAt"],
                "updatedAt": delivery["updatedAt"],
            });
            if consecutive_zero == 0 {
                HubReadV1::Available {
                    items: vec![item],
                    metadata: metadata(),
                }
            } else {
                HubReadV1::Degraded {
                    reason: "consecutive_zero_deliveries".into(),
                    items: vec![item],
                    metadata: metadata(),
                }
            }
        }
    };

    let not_instrumented = || HubReadV1::Unavailable {
        reason: "not_instrumented".into(),
    };

    let producer_metadata = |resolver: &str, source: &str, evidence: &str| HubMetadataV1 {
        resolver: Some(resolver.into()),
        source: Some(source.into()),
        evidence: Some(evidence.into()),
        observed_at_unix_ms: now_unix_ms(),
        cache_age_ms: 0,
    };

    let repositories = blueprint_hub_read(blueprint);

    let adapters = match crate::agent_adapter_producer::build_adapters_report() {
        Some(report) => {
            let projected = crate::agent_adapter_view::project(&report);
            HubReadV1::Available {
                items: vec![serde_json::to_value(projected).unwrap_or(serde_json::Value::Null)],
                metadata: producer_metadata(
                    "hub_inputs::live_inputs_from_local_service",
                    "memory_identity",
                    "sqlite read of cortex-engine.db memory_identity/memory_event_log",
                ),
            }
        }
        None => HubReadV1::Unavailable {
            reason: "missing_input".into(),
        },
    };

    HubInputsV1 {
        deliveries,
        providers,
        repositories,
        adapters,
        // No struct, no producer, no defined concept in Membrane beyond an
        // unrelated `device: String` field on `AgentAdapter`. Not yet a real
        // concept — do not invent one; `not_instrumented` is truthful.
        devices: not_instrumented(),
        memory,
        // `memory_sentinel_view::project` is backed by
        // `memory_sentinel_producer`, a content-free (IDs/counts only)
        // sqlite read. Startup masking remains separate from this runtime
        // data projection.
        sentinel: match crate::memory_sentinel_producer::build_sentinel_report() {
            Some(report) => {
                let projected = crate::memory_sentinel_view::project(&report);
                HubReadV1::Available {
                    items: vec![serde_json::to_value(projected).unwrap_or(serde_json::Value::Null)],
                    metadata: producer_metadata(
                        "hub_inputs::live_inputs_from_local_service",
                        "memories",
                        "sqlite read of cortex-engine.db memories/memory_quarantine/context_feedback",
                    ),
                }
            }
            None => HubReadV1::Unavailable {
                reason: "missing_input".into(),
            },
        },
        // No general alert subsystem exists. Only narrow unrelated signals
        // exist (notifications.rs evidence-alert tracking, an unrelated
        // `alert: Option<String>` field); `dailyAnalysis.alert` is already
        // folded into `providers`. Not yet a real concept — do not invent
        // one; `not_instrumented` is truthful.
        alerts: not_instrumented(),
    }
}

/// Map the Blueprint status seam to the repositories card only. Blueprint
/// failures never alter resident health or any sibling Hub section.
fn blueprint_error_reason(error: &str) -> &'static str {
    let value = error.to_ascii_lowercase();
    if value.contains("hub_inactive")
        || value.contains("membrane_not_running")
        || value.contains("hub is inactive")
    {
        return "hub_inactive";
    }
    if value.contains("resident_owner_active") {
        return "resident_owner_active";
    }
    if value.contains("root_not_enrolled") {
        return "root_not_enrolled";
    }
    if value.contains("graph_missing") {
        return "graph_missing";
    }
    if value.contains("not_configured") || value.contains("unconfigured") {
        return "not_configured";
    }
    if value.contains("stale")
        || value.contains("generation_mismatch")
        || value.contains("watcher_unhealthy")
    {
        return "stale";
    }
    "transport_unavailable"
}

fn blueprint_hub_read(status: Result<serde_json::Value, String>) -> HubReadV1 {
    let envelope = match status {
        Ok(status) => status,
        Err(error) => {
            return match blueprint_error_reason(&error) {
                "stale" => degraded_blueprint("stale", serde_json::Value::Null),
                reason => HubReadV1::Unavailable {
                    reason: reason.into(),
                },
            };
        }
    };
    let result = envelope.get("result").unwrap_or(&envelope);
    let state = result.get("state").and_then(serde_json::Value::as_str);
    // The daemon's application-service status uses graphStatus directly
    // (`state: fresh`), while the CLI doctor envelope uses
    // `state: ready` + `artifacts.graphState: fresh`. Accept both public
    // representations without treating either as a liveness claim.
    let graph_state = result
        .get("artifacts")
        .and_then(|artifacts| artifacts.get("graphState"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| result.get("graphState").and_then(serde_json::Value::as_str));
    let graph_state = graph_state.or(state.filter(|value| *value == "fresh"));
    let graph_complete = result
        .get("manifest")
        .and_then(|manifest| manifest.get("complete"))
        .and_then(serde_json::Value::as_bool)
        .or_else(|| {
            result
                .get("completion")
                .and_then(|completion| completion.get("state"))
                .and_then(serde_json::Value::as_str)
                .map(|value| value == "complete")
        })
        .unwrap_or(false);

    let runtime = result
        .get("runtime")
        .or_else(|| result.get("serviceStatus"));
    let daemon_running = runtime
        .and_then(|value| value.get("daemonRunning").or_else(|| value.get("running")))
        .and_then(serde_json::Value::as_bool);
    let watcher_running = runtime
        .and_then(|value| {
            value
                .get("watcherRunning")
                .or_else(|| value.get("watcherAlive"))
        })
        .and_then(serde_json::Value::as_bool);
    let enrolled_count = runtime
        .and_then(|value| value.get("enrolledRepoCount"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            runtime
                .and_then(|value| value.get("enrolledRepos"))
                .and_then(serde_json::Value::as_array)
                .map(|repos| repos.len() as u64)
        });

    if matches!(state, Some("stale" | "incomplete" | "indeterminate"))
        || matches!(graph_state, Some("stale" | "incomplete" | "indeterminate"))
    {
        return degraded_blueprint("stale", result.clone());
    }

    // A successful IPC response proves only that the daemon accepted this
    // request. Require explicit watcher liveness + enrollment evidence before
    // exposing repository readiness; absent evidence stays local unavailable.
    if daemon_running == Some(false) {
        return HubReadV1::Unavailable {
            reason: "transport_unavailable".into(),
        };
    }
    if enrolled_count == Some(0) {
        return HubReadV1::Unavailable {
            reason: "not_configured".into(),
        };
    }
    if enrolled_count.is_none() || watcher_running.is_none() {
        return degraded_blueprint("transport_unavailable", result.clone());
    }
    if watcher_running == Some(false) {
        return HubReadV1::Unavailable {
            reason: "transport_unavailable".into(),
        };
    }

    if (state == Some("ready") || state == Some("fresh"))
        && graph_state == Some("fresh")
        && graph_complete
    {
        let generation_id = result
            .get("artifacts")
            .and_then(|artifacts| artifacts.get("generationId"))
            .or_else(|| {
                result
                    .get("manifest")
                    .and_then(|manifest| manifest.get("generationId"))
            })
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        return HubReadV1::Available {
            items: vec![serde_json::json!({
                "state": state,
                "graphState": graph_state,
                "generationId": generation_id,
                "repository": result["repository"],
            })],
            metadata: HubMetadataV1 {
                resolver: Some("hub_inputs::live_inputs_from_local_service".into()),
                source: Some("blueprint_daemon".into()),
                evidence: Some("Blueprint status IPC".into()),
                observed_at_unix_ms: now_unix_ms(),
                cache_age_ms: 0,
            },
        };
    }

    if matches!(state, Some("missing" | "unconfigured")) || graph_state == Some("missing") {
        HubReadV1::Unavailable {
            reason: "not_configured".into(),
        }
    } else {
        degraded_blueprint("stale", result.clone())
    }
}

fn degraded_blueprint(reason: &str, status: serde_json::Value) -> HubReadV1 {
    HubReadV1::Degraded {
        reason: reason.into(),
        items: vec![status],
        metadata: HubMetadataV1 {
            resolver: Some("hub_inputs::live_inputs_from_local_service".into()),
            source: Some("blueprint_daemon".into()),
            evidence: Some("Blueprint status IPC".into()),
            observed_at_unix_ms: now_unix_ms(),
            cache_age_ms: 0,
        },
    }
}

/// Blueprint service availability is distinct from repository configuration.
/// A typed not-configured refusal still proves daemon liveness; transport and
/// Hub lifecycle failures do not.
fn blueprint_service_hub_read(status: Result<serde_json::Value, String>) -> HubReadV1 {
    match status {
        Ok(value) => {
            let result = value.get("result").unwrap_or(&value);
            let runtime = result
                .get("runtime")
                .or_else(|| result.get("serviceStatus"));
            let watcher_running = runtime
                .and_then(|runtime| {
                    runtime
                        .get("watcherRunning")
                        .or_else(|| runtime.get("watcherAlive"))
                })
                .and_then(serde_json::Value::as_bool);
            if watcher_running == Some(false) {
                return HubReadV1::Unavailable {
                    reason: "transport_unavailable".into(),
                };
            }
            HubReadV1::Available {
                items: vec![serde_json::json!({"mode": "hub_hosted", "status": value})],
                metadata: HubMetadataV1 {
                    resolver: Some("hub_inputs::blueprint_service_hub_read".into()),
                    source: Some("blueprint_daemon".into()),
                    evidence: Some("typed Blueprint IPC response".into()),
                    observed_at_unix_ms: now_unix_ms(),
                    cache_age_ms: 0,
                },
            }
        }
        Err(error) => match blueprint_error_reason(&error) {
            "root_not_enrolled" | "graph_missing" | "not_configured" => HubReadV1::Available {
                items: vec![serde_json::json!({"mode": "hub_hosted", "typedRefusal": error})],
                metadata: HubMetadataV1 {
                    resolver: Some("hub_inputs::blueprint_service_hub_read".into()),
                    source: Some("blueprint_daemon".into()),
                    evidence: Some("typed Blueprint IPC refusal".into()),
                    observed_at_unix_ms: now_unix_ms(),
                    cache_age_ms: 0,
                },
            },
            "stale" => degraded_blueprint("stale", serde_json::json!({"error": error})),
            reason => HubReadV1::Unavailable {
                reason: reason.into(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Every test in this module can implicitly read the process-global
    /// `MEMBRANE_DB_PATH` / `MEMBRANE_CATALOG` / `WORKSPACE_ROOT` env vars
    /// through the producers. Tests that need deterministic missing stores
    /// hold this lock and point both explicit paths at guaranteed-missing
    /// files so they never read real workspace data.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_missing_db<R>(f: impl FnOnce() -> R) -> R {
        let _guard = lock_env();
        let prior_db = std::env::var_os("MEMBRANE_DB_PATH");
        let prior_catalog = std::env::var_os("MEMBRANE_CATALOG");
        unsafe {
            std::env::set_var(
                "MEMBRANE_DB_PATH",
                "/nonexistent/hub_inputs_test/cortex-engine.db",
            );
            std::env::set_var(
                "MEMBRANE_CATALOG",
                "/nonexistent/hub_inputs_test/catalog.db",
            );
        }
        let result = f();
        unsafe {
            match prior_db {
                Some(v) => std::env::set_var("MEMBRANE_DB_PATH", v),
                None => std::env::remove_var("MEMBRANE_DB_PATH"),
            }
            match prior_catalog {
                Some(v) => std::env::set_var("MEMBRANE_CATALOG", v),
                None => std::env::remove_var("MEMBRANE_CATALOG"),
            }
        }
        result
    }

    #[test]
    fn default_port_is_named_and_invalid_explicit_values_fail_closed() {
        assert_eq!(local_service_port_from(None), Some(47851));
        assert_eq!(local_service_port_from(Some("47852".as_ref())), Some(47852));
        assert_eq!(local_service_port_from(Some("0".as_ref())), None);
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let non_unicode = std::ffi::OsString::from_vec(vec![0xff]);
            assert_eq!(local_service_port_from(Some(&non_unicode)), None);
        }
    }

    #[test]
    fn fresh_service_maps_to_available_memory_and_providers() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{
                "ok": true,
                "catalog": {"status": "ok"},
                "database": {"status": "ok"},
                "dailyAnalysis": {"status": "fresh", "alert": false},
                "planner": {},
                "embedder_model": "test-model",
                "last_persist_error": null
            }"#,
        )
        .unwrap();
        let inputs = with_missing_db(|| inputs_from_health(&health, None));
        assert!(matches!(inputs.memory, HubReadV1::Available { .. }));
        assert!(matches!(inputs.providers, HubReadV1::Available { .. }));
        assert!(matches!(
            inputs.deliveries,
            HubReadV1::Unavailable { ref reason } if reason == "delivery_health_missing"
        ));
        assert!(matches!(
            inputs.repositories,
            HubReadV1::Unavailable { ref reason } if reason == "transport_unavailable"
        ));
        assert!(matches!(
            inputs.adapters,
            HubReadV1::Unavailable { ref reason } if reason == "missing_input"
        ));
        assert!(matches!(
            inputs.sentinel,
            HubReadV1::Unavailable { ref reason } if reason == "missing_input"
        ));
    }

    #[test]
    fn legacy_ok_daily_analysis_maps_to_available_providers() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{
                "ok": true,
                "catalog": {"status": "ok"},
                "database": {"status": "ok"},
                "dailyAnalysis": {"status": "ok"}
            }"#,
        )
        .unwrap();
        let inputs = inputs_from_health(&health, None);
        assert!(matches!(inputs.providers, HubReadV1::Available { .. }));
    }

    #[test]
    fn blueprint_status_maps_absent_stale_and_fresh_complete_without_affecting_peers() {
        let absent = blueprint_hub_read(Err("connect: no such file".into()));
        assert!(matches!(
            absent,
            HubReadV1::Unavailable { ref reason } if reason == "transport_unavailable"
        ));

        let stale_error = blueprint_hub_read(Err("Blueprint status failed: stale_blocked".into()));
        assert!(matches!(stale_error, HubReadV1::Degraded { ref reason, .. } if reason == "stale"));

        let stale = blueprint_hub_read(Ok(serde_json::json!({
            "protocolVersion": 1,
            "ok": true,
            "result": {
                "state": "stale",
                "artifacts": {"graphState": "stale"}
            }
        })));
        assert!(matches!(stale, HubReadV1::Degraded { ref reason, .. } if reason == "stale"));

        let incomplete = blueprint_hub_read(Ok(serde_json::json!({
            "protocolVersion": 1,
            "ok": true,
            "result": {"state": "incomplete"}
        })));
        assert!(matches!(incomplete, HubReadV1::Degraded { ref reason, .. } if reason == "stale"));

        let fresh = blueprint_hub_read(Ok(serde_json::json!({
            "protocolVersion": 1,
            "ok": true,
            "result": {
                "state": "ready",
                "artifacts": {"graphState": "fresh", "generationId": "gen-1"},
                "manifest": {"complete": true},
                "repository": {"revision": "abc123"},
                "runtime": {
                    "watcherRunning": true,
                    "enrolledRepoCount": 1
                }
            }
        })));
        assert!(matches!(fresh, HubReadV1::Available { .. }));

        let missing_runtime = blueprint_hub_read(Ok(serde_json::json!({
            "protocolVersion": 1,
            "ok": true,
            "result": {
                "state": "fresh",
                "manifest": {"complete": true}
            }
        })));
        assert!(
            matches!(missing_runtime, HubReadV1::Degraded { ref reason, .. } if reason == "transport_unavailable")
        );

        let daemon_status = blueprint_hub_read(Ok(serde_json::json!({
            "protocolVersion": 1,
            "ok": true,
            "result": {
                "state": "fresh",
                "manifest": {"complete": true},
                "runtime": {
                    "daemonRunning": true,
                    "watcherRunning": true,
                    "enrolledRepoCount": 1
                }
            }
        })));
        assert!(matches!(daemon_status, HubReadV1::Available { .. }));

        let unwatched = blueprint_hub_read(Ok(serde_json::json!({
            "protocolVersion": 1,
            "ok": true,
            "result": {
                "state": "fresh",
                "manifest": {"complete": true},
                "runtime": {
                    "watcherRunning": false,
                    "enrolledRepoCount": 1
                }
            }
        })));
        assert!(matches!(
            unwatched,
            HubReadV1::Unavailable { ref reason } if reason == "transport_unavailable"
        ));

        let unenrolled = blueprint_hub_read(Ok(serde_json::json!({
            "protocolVersion": 1,
            "ok": true,
            "result": {
                "state": "fresh",
                "manifest": {"complete": true},
                "runtime": {
                    "watcherRunning": true,
                    "enrolledRepoCount": 0
                }
            }
        })));
        assert!(
            matches!(unenrolled, HubReadV1::Unavailable { ref reason } if reason == "not_configured")
        );

        let typed_unenrolled = blueprint_hub_read(Err(
            "Blueprint status failed: {\"code\":\"root_not_enrolled\"}".into(),
        ));
        assert!(
            matches!(typed_unenrolled, HubReadV1::Unavailable { ref reason } if reason == "root_not_enrolled")
        );
        assert!(matches!(
            blueprint_service_hub_read(Err(
                "Blueprint status failed: {\"code\":\"root_not_enrolled\"}".into()
            )),
            HubReadV1::Available { .. }
        ));
    }

    #[test]
    fn degraded_daily_analysis_marks_providers_degraded() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{
                "ok": true,
                "catalog": {"status": "ok"},
                "database": {"status": "ok"},
                "dailyAnalysis": {"status": "degraded"},
                "planner": {},
                "embedder_model": "test-model",
                "last_persist_error": null
            }"#,
        )
        .unwrap();
        let inputs = inputs_from_health(&health, None);
        assert!(matches!(
            inputs.providers,
            HubReadV1::Degraded { ref reason, .. } if reason == "daily_analysis_unhealthy"
        ));
        // Memory itself is unaffected by dailyAnalysis status.
        assert!(matches!(inputs.memory, HubReadV1::Available { .. }));
    }

    #[test]
    fn alerted_daily_analysis_marks_providers_degraded() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{
                "ok": true,
                "catalog": {"status": "ok"},
                "database": {"status": "ok"},
                "dailyAnalysis": {"status": "fresh", "alert": true}
            }"#,
        )
        .unwrap();
        let inputs = inputs_from_health(&health, None);
        assert!(matches!(
            inputs.providers,
            HubReadV1::Degraded { ref reason, .. } if reason == "daily_analysis_unhealthy"
        ));
    }

    #[test]
    fn zero_delivery_count_marks_deliveries_degraded() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{
                "ok": true,
                "catalog": {"status": "ok"},
                "database": {"status": "ok"},
                "dailyAnalysis": {"status": "ok"}
            }"#,
        )
        .unwrap();
        let delivery: serde_json::Value = serde_json::from_str(
            r#"{"consecutiveZero": 3, "lastNonEmptyAt": "2026-08-01T00:00:00Z", "updatedAt": "2026-08-09T00:00:00Z"}"#,
        )
        .unwrap();
        let inputs = inputs_from_health(&health, Some(&delivery));
        assert!(matches!(
            inputs.deliveries,
            HubReadV1::Degraded { ref reason, .. } if reason == "consecutive_zero_deliveries"
        ));
    }

    #[test]
    fn unhealthy_catalog_marks_memory_degraded() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{
                "ok": false,
                "catalog": {"status": "error"},
                "database": {"status": "ok"},
                "dailyAnalysis": {"status": "ok"}
            }"#,
        )
        .unwrap();
        let inputs = inputs_from_health(&health, None);
        assert!(matches!(
            inputs.memory,
            HubReadV1::Degraded { ref reason, .. } if reason == "catalog_or_database_unhealthy"
        ));
    }

    #[test]
    fn empty_corpus_marks_memory_degraded_with_memory_count() {
        // c191b240: empty corpus must surface as degraded with memoryCount preserved
        let health: serde_json::Value = serde_json::from_str(
            r#"{
                "ok": false,
                "catalog": {"status": "ok"},
                "database": {"status": "empty", "memoryCount": 0},
                "dailyAnalysis": {"status": "ok"}
            }"#,
        )
        .unwrap();
        let inputs = inputs_from_health(&health, None);
        match inputs.memory {
            HubReadV1::Degraded { reason, items, .. } => {
                assert_eq!(reason, "database_empty");
                assert!(
                    items[0].get("memoryCount").is_some(),
                    "memoryCount must be present"
                );
                assert_eq!(items[0]["memoryCount"], 0);
                assert_eq!(items[0]["database"]["status"], "empty");
            }
            other => panic!("expected Degraded for empty corpus, got {:?}", other),
        }
    }

    #[test]
    fn populated_corpus_memory_available_includes_memory_count() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{
                "ok": true,
                "catalog": {"status": "ok"},
                "database": {"status": "ok", "memoryCount": 42},
                "dailyAnalysis": {"status": "ok"}
            }"#,
        )
        .unwrap();
        let inputs = inputs_from_health(&health, None);
        match inputs.memory {
            HubReadV1::Available { items, .. } => {
                assert!(items[0].get("memoryCount").is_some());
                assert_eq!(items[0]["memoryCount"], 42);
            }
            other => panic!("expected Available, got {:?}", other),
        }
    }

    #[test]
    fn populated_database_wires_repositories_adapters_and_sentinel_to_available() {
        let _guard = lock_env();
        let dir = std::env::temp_dir().join(format!(
            "hub_inputs_test_{}_{}",
            std::process::id(),
            now_unix_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("cortex-engine.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"CREATE TABLE memory_identity (
                    memory_id TEXT PRIMARY KEY, artifact_id TEXT, origin_event_uid TEXT,
                    installation_id TEXT, client TEXT NOT NULL, session_id TEXT, turn_id TEXT,
                    trace_id TEXT, identity_status TEXT, created_at TEXT NOT NULL
                );
                INSERT INTO memory_identity (memory_id, client, created_at) VALUES ('m1','codex','2026-08-01T00:00:00Z');
                CREATE TABLE memory_event_log (
                    event_id INTEGER PRIMARY KEY, ts TEXT NOT NULL, event_kind TEXT NOT NULL,
                    memory_id TEXT, surface TEXT NOT NULL DEFAULT 'unknown', session_id TEXT,
                    trace_id TEXT, scope_id TEXT, quantity INTEGER NOT NULL DEFAULT 1,
                    meta TEXT NOT NULL DEFAULT '{}', client TEXT
                );

                CREATE TABLE memories (
                    id TEXT PRIMARY KEY, lifecycle_state TEXT NOT NULL DEFAULT 'active',
                    effective_until_ms INTEGER, expires_at_ms INTEGER
                );
                INSERT INTO memories (id, lifecycle_state) VALUES ('m1','active');
                CREATE TABLE memory_quarantine (id TEXT PRIMARY KEY);
                CREATE TABLE context_feedback (
                    trace_id TEXT NOT NULL, candidate_id TEXT NOT NULL, outcome TEXT NOT NULL,
                    verified INTEGER NOT NULL, ts TEXT NOT NULL,
                    PRIMARY KEY (trace_id, candidate_id)
                );"#,
            )
            .unwrap();
        }
        let prior = std::env::var_os("MEMBRANE_DB_PATH");
        unsafe {
            std::env::set_var("MEMBRANE_DB_PATH", &db_path);
        }
        let health: serde_json::Value = serde_json::from_str(
            r#"{"ok": true, "catalog": {"status": "ok"}, "database": {"status": "ok"}, "dailyAnalysis": {"status": "ok"}}"#,
        )
        .unwrap();
        let inputs = inputs_from_health(&health, None);
        unsafe {
            match prior {
                Some(v) => std::env::set_var("MEMBRANE_DB_PATH", v),
                None => std::env::remove_var("MEMBRANE_DB_PATH"),
            }
        }
        std::fs::remove_dir_all(&dir).ok();

        assert!(matches!(
            inputs.repositories,
            HubReadV1::Unavailable { ref reason } if reason == "transport_unavailable"
        ));
        assert!(matches!(inputs.adapters, HubReadV1::Available { .. }));
        assert!(matches!(inputs.sentinel, HubReadV1::Available { .. }));
    }

    // --- Producer-path contract tests for frozen parent + subsystem mapping ---

    #[test]
    fn parent_healthy_resident_plus_valid_snapshot_is_running() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{"ok": true, "catalog": {"status": "ok"}, "database": {"status": "ok"}, "dailyAnalysis": {"status": "ok"}}"#,
        ).unwrap();
        let parts = snapshot_parts_from_health(&health, None, Err("no socket".into()));
        assert_eq!(
            parts.membrane_state,
            membrane_protocol::MembraneParentState::Running
        );
        // Also prove via protocol helper directly
        assert_eq!(
            membrane_protocol::membrane_parent_state(true, Some(true), true),
            membrane_protocol::MembraneParentState::Running
        );
    }

    #[test]
    fn parent_unhealthy_resident_plus_valid_snapshot_is_degraded() {
        // Real producer path: ok=false with valid snapshot must be Degraded, not Running
        let health: serde_json::Value = serde_json::from_str(
            r#"{"ok": false, "catalog": {"status": "error"}, "database": {"status": "ok"}, "dailyAnalysis": {"status": "ok"}}"#,
        ).unwrap();
        let parts = snapshot_parts_from_health(&health, None, Err("no socket".into()));
        assert_eq!(
            parts.membrane_state,
            membrane_protocol::MembraneParentState::Degraded
        );
        assert_eq!(
            membrane_protocol::membrane_parent_state(true, Some(false), true),
            membrane_protocol::MembraneParentState::Degraded
        );
        // Even with all children healthy, parent degraded when health is false
        let healthy_child_input = inputs_from_health(&health, None);
        assert!(matches!(
            healthy_child_input.providers,
            HubReadV1::Available { .. }
        ));
    }

    #[test]
    fn parent_healthy_resident_plus_invalid_snapshot_is_degraded() {
        assert_eq!(
            membrane_protocol::membrane_parent_state(true, Some(true), false),
            membrane_protocol::MembraneParentState::Degraded
        );
    }

    #[test]
    fn parent_unreachable_resident_is_offline() {
        let offline = offline_snapshot_parts();
        assert_eq!(
            offline.membrane_state,
            membrane_protocol::MembraneParentState::Offline
        );
        assert_eq!(
            membrane_protocol::membrane_parent_state(true, None, true),
            membrane_protocol::MembraneParentState::Offline
        );
        assert_eq!(
            membrane_protocol::membrane_parent_state(false, Some(true), true),
            membrane_protocol::MembraneParentState::Offline
        );
    }

    #[test]
    fn every_child_unavailable_while_resident_healthy_parent_remains_running() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{"ok": true, "catalog": {"status": "ok"}, "database": {"status": "ok"}, "dailyAnalysis": {"status": "ok"}}"#,
        ).unwrap();
        // Force all child resources to unavailable (no DB, no delivery, blueprint missing)
        let parts =
            with_missing_db(|| snapshot_parts_from_health(&health, None, Err("no socket".into())));
        assert_eq!(
            parts.membrane_state,
            membrane_protocol::MembraneParentState::Running
        );
        // Children are unavailable/degraded but parent is Running
        assert!(matches!(
            parts.inputs.deliveries,
            HubReadV1::Unavailable { .. }
        ));
        assert!(matches!(
            parts.inputs.repositories,
            HubReadV1::Unavailable { .. }
        ));
    }

    #[test]
    fn six_subsystem_states_exist_independently() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{"ok": true, "catalog": {"status": "ok"}, "database": {"status": "ok"}, "dailyAnalysis": {"status": "ok"}}"#,
        ).unwrap();
        let parts = snapshot_parts_from_health(&health, None, Err("no socket".into()));
        // The wire shape is a closed six-field struct — no unnamed or missing
        // subsystem is representable.
        let encoded = serde_json::to_value(&parts.subsystems).unwrap();
        let mut keys: Vec<&str> = encoded
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["adapt", "blueprint", "cortex", "ledger", "pull", "push"]
        );
        for name in membrane_protocol::SUBSYSTEM_NAMES {
            assert!(
                encoded.get(name).is_some_and(|value| value.is_object()),
                "missing subsystem {name}"
            );
        }
        // Operational resources are 8 and distinct from subsystems
        assert!(matches!(
            parts.inputs.deliveries,
            HubReadV1::Unavailable { .. }
        ));
        assert!(serde_json::to_value(&parts.subsystems)
            .unwrap()
            .get("deliveries")
            .is_none());
    }

    #[test]
    fn uninstrumented_subsystem_is_not_configured() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{"ok": true, "catalog": {"status": "ok"}, "database": {"status": "ok"}, "dailyAnalysis": {"status": "ok"}}"#,
        ).unwrap();
        let parts = snapshot_parts_from_health(&health, None, Err("no socket".into()));
        for name in ["pull", "push", "ledger", "adapt"] {
            let section = subsystem_section(&parts.subsystems, name);
            assert_eq!(
                section.state,
                membrane_protocol::SubsystemStateV1::NotConfigured
            );
            assert_eq!(section.reason, "not_instrumented");
        }
    }

    #[test]
    fn blueprint_unavailable_does_not_affect_parent() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{"ok": true, "catalog": {"status": "ok"}, "database": {"status": "ok"}, "dailyAnalysis": {"status": "ok"}}"#,
        ).unwrap();
        let parts = snapshot_parts_from_health(&health, None, Err("connect refused".into()));
        assert_eq!(
            parts.membrane_state,
            membrane_protocol::MembraneParentState::Running
        );
        assert_eq!(
            parts.subsystems.blueprint.state,
            membrane_protocol::SubsystemStateV1::Unavailable
        );
        assert_eq!(parts.subsystems.blueprint.reason, "transport_unavailable");
        // Parent remains Running even though Blueprint subsystem is Unavailable
    }

    #[test]
    fn operational_resources_remain_separate_from_subsystems() {
        let health: serde_json::Value = serde_json::from_str(
            r#"{"ok": true, "catalog": {"status": "ok"}, "database": {"status": "ok"}, "dailyAnalysis": {"status": "ok"}}"#,
        ).unwrap();
        let parts =
            with_missing_db(|| snapshot_parts_from_health(&health, None, Err("no socket".into())));
        // Operational sentinel should NOT be presented as Ledger
        assert_eq!(
            parts.subsystems.ledger.state,
            membrane_protocol::SubsystemStateV1::NotConfigured
        );
        assert_eq!(parts.subsystems.ledger.reason, "not_instrumented");
        // Cortex owns sentinel/memory, not Ledger
        assert_ne!(
            parts.subsystems.ledger.reason, parts.subsystems.cortex.reason,
            "Ledger must stay distinct from Cortex/sentinel"
        );
    }

    fn subsystem_section<'a>(
        subsystems: &'a membrane_protocol::HubSubsystemsV1,
        name: &str,
    ) -> &'a membrane_protocol::HubSubsystemV1 {
        match name {
            "pull" => &subsystems.pull,
            "push" => &subsystems.push,
            "cortex" => &subsystems.cortex,
            "blueprint" => &subsystems.blueprint,
            "ledger" => &subsystems.ledger,
            "adapt" => &subsystems.adapt,
            _ => panic!("unknown subsystem {name}"),
        }
    }
}
