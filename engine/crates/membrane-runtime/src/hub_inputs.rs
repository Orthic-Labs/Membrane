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

/// Best-effort live read of the local crypt-service's `/health` endpoint,
/// mapped into `HubInputsV1`. Returns `None` on any connection, I/O, or
/// parse failure so the caller can fall back to the honest "unavailable"
/// facade rather than show a fabricated state.
pub fn live_inputs_from_local_service() -> Option<HubInputsV1> {
    let port = local_service_port();
    let health = fetch_health_json(port)?;
    let workspace_root = configured_workspace_root();
    let delivery = read_delivery_health_json(&workspace_root);
    Some(inputs_from_health(&health, delivery.as_ref()))
}

fn local_service_port() -> u16 {
    std::env::var("CRYPT_PORT")
        .ok()
        .or_else(|| std::env::var("WORKSPACE_MEMORY_PORT").ok())
        .and_then(|value| value.trim().parse::<u16>().ok())
        .unwrap_or(47851)
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
    let daily_analysis_status = health["dailyAnalysis"]["status"].as_str().unwrap_or("unknown");
    let daily_analysis_alert = health["dailyAnalysis"]["alert"].as_bool();
    let daily_analysis_ok = matches!(daily_analysis_status, "fresh" | "ok")
        && daily_analysis_alert != Some(true);

    let memory = if catalog_ok && database_ok {
        HubReadV1::Available {
            items: vec![serde_json::json!({
                "catalog": health["catalog"]["status"],
                "database": health["database"]["status"],
            })],
            metadata: metadata(),
        }
    } else {
        HubReadV1::Degraded {
            reason: "catalog_or_database_unhealthy".into(),
            items: vec![serde_json::json!({
                "catalog": health["catalog"]["status"],
                "database": health["database"]["status"],
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

    HubInputsV1 {
        deliveries,
        providers,
        repositories: not_instrumented(),
        adapters: not_instrumented(),
        devices: not_instrumented(),
        memory,
        sentinel: not_instrumented(),
        alerts: not_instrumented(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let inputs = inputs_from_health(&health, None);
        assert!(matches!(inputs.memory, HubReadV1::Available { .. }));
        assert!(matches!(inputs.providers, HubReadV1::Available { .. }));
        assert!(matches!(
            inputs.deliveries,
            HubReadV1::Unavailable { ref reason } if reason == "delivery_health_missing"
        ));
        assert!(matches!(inputs.repositories, HubReadV1::Unavailable { .. }));
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
}
