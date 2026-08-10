//! MBR: live source for the Hub popover.
//!
//! `modes.rs` used to build `HubInputsV1::unavailable("source_not_connected")`
//! unconditionally, so the popover showed "Offline" even while the local
//! crypt-service was up and healthy. This module replaces that hardcoded
//! facade with a real (best-effort) read of the local service's
//! unauthenticated `GET /health` endpoint, mapped into the same
//! `HubInputsV1` contract the facade already understands. Any failure to
//! reach or parse the service falls back to `None`, and the caller keeps the
//! existing "unavailable" behavior — we never fabricate readiness.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::hub::{HubInputsV1, HubMetadataV1, HubReadV1};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const IO_TIMEOUT: Duration = Duration::from_millis(300);
pub const DEFAULT_LOCAL_SERVICE_PORT: u16 = 47851;

/// Best-effort live read of the local crypt-service's `/health` endpoint,
/// mapped into `HubInputsV1`. Returns `None` on any connection, I/O, or
/// parse failure so the caller can fall back to the honest "unavailable"
/// facade rather than show a fabricated state.
pub fn live_inputs_from_local_service() -> Option<HubInputsV1> {
    let port = local_service_port()?;
    let health = fetch_health_json(port)?;
    let workspace_root = configured_workspace_root();
    let delivery = read_delivery_health_json(&workspace_root);
    Some(inputs_from_health(&health, delivery.as_ref()))
}

fn local_service_port() -> Option<u16> {
    local_service_port_from(
        std::env::var_os("CRYPT_PORT").as_deref(),
        std::env::var_os("WORKSPACE_MEMORY_PORT").as_deref(),
    )
}

fn local_service_port_from(
    canonical: Option<&std::ffi::OsStr>,
    compatibility: Option<&std::ffi::OsStr>,
) -> Option<u16> {
    match canonical.or(compatibility) {
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
    std::env::var_os("RIGHTCONTEXT_REPO_ROOT")
        .or_else(|| std::env::var_os("WORKSPACE_ROOT"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
}

fn fetch_health_json(port: u16) -> Option<serde_json::Value> {
    let addr = format!("127.0.0.1:{port}");
    let mut stream = TcpStream::connect_timeout(
        &addr.parse().ok()?,
        CONNECT_TIMEOUT,
    )
    .ok()?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok()?;

    let request = format!(
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
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
fn inputs_from_health(health: &serde_json::Value, delivery: Option<&serde_json::Value>) -> HubInputsV1 {
    let observed_at_unix_ms = now_unix_ms();
    let metadata = || HubMetadataV1 {
        resolver: Some("hub_inputs::live_inputs_from_local_service".into()),
        source: Some("local_crypt_service".into()),
        evidence: Some("GET /health".into()),
        observed_at_unix_ms,
        cache_age_ms: 0,
    };

    let catalog_ok = health["catalog"]["status"].as_str() == Some("ok");
    let database_ok = health["database"]["status"].as_str() == Some("ok");
    let database_status = health["database"]["status"].as_str().unwrap_or("unknown");
    let memory_count = health["database"]["memoryCount"].as_i64();
    let daily_analysis_status = health["dailyAnalysis"]["status"].as_str().unwrap_or("unknown");
    let daily_analysis_alert = health["dailyAnalysis"]["alert"].as_bool();
    let daily_analysis_ok = matches!(daily_analysis_status, "fresh" | "ok")
        && daily_analysis_alert != Some(true);

    let memory = if catalog_ok && database_ok {
        HubReadV1::Available {
            items: vec![serde_json::json!({
                "catalog": health["catalog"]["status"],
                "database": health["database"]["status"],
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
                "database": health["database"]["status"],
                "memoryCount": health["database"]["memoryCount"],
                "databaseStatus": database_status,
            })],
            metadata: metadata(),
        }
    };

    let providers_items = vec![
        serde_json::json!({"service": "crypt-service", "ok": health["ok"]}),
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

    let repositories = match crate::sources_producer::build_sources_payload() {
        Some(payload) => HubReadV1::Available {
            items: vec![crate::sources_explorer::project_sources_explorer_json(&payload)],
            metadata: producer_metadata(
                "hub_inputs::live_inputs_from_local_service",
                "doc_artifacts",
                "sqlite read of crypt-engine.db doc_artifacts",
            ),
        },
        None => HubReadV1::Unavailable {
            reason: "missing_input".into(),
        },
    };

    let adapters = match crate::agent_adapter_producer::build_adapters_report() {
        Some(report) => {
            let projected = crate::agent_adapter_view::project(&report);
            HubReadV1::Available {
                items: vec![serde_json::to_value(projected).unwrap_or(serde_json::Value::Null)],
                metadata: producer_metadata(
                    "hub_inputs::live_inputs_from_local_service",
                    "memory_identity",
                    "sqlite read of crypt-engine.db memory_identity/memory_event_log",
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
        // `memory_sentinel_view::project` (src/memory_sentinel_view.rs,
        // mirrored in apps/membrane-hub/src/memory-sentinel.mjs) is now
        // backed by `memory_sentinel_producer`, a content-free (IDs/counts
        // only) sqlite read. `startup_sentinel_masked` in
        // apps/membrane-hub/src-tauri/src/main.rs is an unrelated boolean
        // startup gate — see the comment at that call site for why it stays
        // separate.
        sentinel: match crate::memory_sentinel_producer::build_sentinel_report() {
            Some(report) => {
                let projected = crate::memory_sentinel_view::project(&report);
                HubReadV1::Available {
                    items: vec![serde_json::to_value(projected).unwrap_or(serde_json::Value::Null)],
                    metadata: producer_metadata(
                        "hub_inputs::live_inputs_from_local_service",
                        "memories",
                        "sqlite read of crypt-engine.db memories/memory_quarantine/context_feedback/transform_opportunity_log",
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Every test in this module can implicitly read the process-global
    /// `CRYPT_DB_PATH` / `WORKSPACE_ROOT` env vars through the sources,
    /// adapters, and sentinel producers. Tests that need a deterministic
    /// "no database" result hold this lock and point `CRYPT_DB_PATH` at a
    /// guaranteed-missing path so they never race a real workspace database.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_missing_db<R>(f: impl FnOnce() -> R) -> R {
        let _guard = lock_env();
        let prior = std::env::var_os("CRYPT_DB_PATH");
        unsafe {
            std::env::set_var("CRYPT_DB_PATH", "/nonexistent/hub_inputs_test/crypt-engine.db");
        }
        let result = f();
        unsafe {
            match prior {
                Some(v) => std::env::set_var("CRYPT_DB_PATH", v),
                None => std::env::remove_var("CRYPT_DB_PATH"),
            }
        }
        result
    }

    #[test]
    fn default_port_is_named_and_invalid_explicit_values_fail_closed() {
        assert_eq!(local_service_port_from(None, None), Some(47851));
        assert_eq!(
            local_service_port_from(Some("47852".as_ref()), Some("47853".as_ref())),
            Some(47852)
        );
        assert_eq!(
            local_service_port_from(Some("0".as_ref()), Some("47853".as_ref())),
            None
        );
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let non_unicode = std::ffi::OsString::from_vec(vec![0xff]);
            assert_eq!(
                local_service_port_from(Some(&non_unicode), Some("47853".as_ref())),
                None
            );
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
            HubReadV1::Unavailable { ref reason } if reason == "missing_input"
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
                assert!(items[0].get("memoryCount").is_some(), "memoryCount must be present");
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
        let db_path = dir.join("crypt-engine.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE doc_artifacts (
                    doc_id TEXT NOT NULL, repository_root TEXT NOT NULL, repository_id TEXT NOT NULL,
                    revision TEXT NOT NULL, path TEXT NOT NULL, content_hash TEXT NOT NULL,
                    parser_version TEXT NOT NULL, document_class TEXT NOT NULL,
                    lifecycle_state TEXT NOT NULL DEFAULT 'active', trust_label TEXT NOT NULL,
                    influence_class TEXT NOT NULL, sensitivity TEXT NOT NULL, generated INTEGER NOT NULL DEFAULT 0,
                    index_generation INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
                    title TEXT NOT NULL DEFAULT '', summary TEXT NOT NULL DEFAULT '',
                    keywords_json TEXT NOT NULL DEFAULT '[]', superseded_by TEXT
                );
                INSERT INTO doc_artifacts (doc_id, repository_root, repository_id, revision, path,
                    content_hash, parser_version, document_class, lifecycle_state, trust_label,
                    influence_class, sensitivity, index_generation, updated_at_ms, title, summary)
                VALUES ('d1','/repo','repo-id','rev1','README.md','h1','v1','doc','active','trusted',
                    'reference','public',3,1000,'Readme','A readme summary');

                CREATE TABLE memory_identity (
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
                CREATE TABLE transform_opportunity_log (
                    opportunity_uid TEXT PRIMARY KEY, ts TEXT NOT NULL, outcome TEXT NOT NULL
                );
                CREATE TABLE context_feedback (
                    trace_id TEXT NOT NULL, candidate_id TEXT NOT NULL, outcome TEXT NOT NULL,
                    verified INTEGER NOT NULL, ts TEXT NOT NULL,
                    PRIMARY KEY (trace_id, candidate_id)
                );",
            )
            .unwrap();
        }
        let prior = std::env::var_os("CRYPT_DB_PATH");
        unsafe {
            std::env::set_var("CRYPT_DB_PATH", &db_path);
        }
        let health: serde_json::Value = serde_json::from_str(
            r#"{"ok": true, "catalog": {"status": "ok"}, "database": {"status": "ok"}, "dailyAnalysis": {"status": "ok"}}"#,
        )
        .unwrap();
        let inputs = inputs_from_health(&health, None);
        unsafe {
            match prior {
                Some(v) => std::env::set_var("CRYPT_DB_PATH", v),
                None => std::env::remove_var("CRYPT_DB_PATH"),
            }
        }
        std::fs::remove_dir_all(&dir).ok();

        assert!(matches!(inputs.repositories, HubReadV1::Available { .. }));
        assert!(matches!(inputs.adapters, HubReadV1::Available { .. }));
        assert!(matches!(inputs.sentinel, HubReadV1::Available { .. }));
    }
}
