//! Port of legacy `mcp/host/observable-ingress.cjs`.

use crate::host_observable_event::validate_observable_event;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_RECORD_BYTES: usize = 256 * 1024;

pub struct ResolvedIngressTarget {
    pub target: PathBuf,
    pub db_path: PathBuf,
}

/// Mirrors `resolveRuntimeConfigIdentity`. Returns `Err` on any identity
/// mismatch, mirroring the JS throw.
pub fn resolve_runtime_config_identity(config_path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(config_path).map_err(|e| e.to_string())?;
    let config: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if config.get("schemaVersion").and_then(Value::as_i64) != Some(1)
        || config.get("serviceId").and_then(Value::as_str) != Some("membrane-local-v1")
    {
        return Err("invalid membrane runtime config identity".to_string());
    }
    if config.get("host").and_then(Value::as_str) != Some("127.0.0.1") {
        return Err("membrane runtime host must remain loopback-only".to_string());
    }
    Ok(config)
}

/// Mirrors `resolveDefaultIngressTarget`. `cortex_db_override` mirrors
/// `env.CORTEX_DB`.
pub fn resolve_default_ingress_target(
    config_path: &Path,
    cortex_db_override: Option<&str>,
) -> Option<ResolvedIngressTarget> {
    resolve_runtime_config_identity(config_path).ok()?;
    let db_path = match cortex_db_override.filter(|s| !s.is_empty()) {
        Some(explicit) => PathBuf::from(explicit),
        None => config_path
            .parent()?
            .join("..")
            .join("..")
            .join(".cache")
            .join("memory")
            .join("cortex-engine.db"),
    };
    let db_path = dunce_canonicalize_lossy(&db_path);
    let target = db_path.parent()?.join("context-telemetry-ingress.jsonl");
    Some(ResolvedIngressTarget { target, db_path })
}

/// `path.resolve` in Node normalizes without requiring the path to exist;
/// std::fs::canonicalize requires existence, so fall back to a lexical join.
fn dunce_canonicalize_lossy(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        // Best-effort lexical normalization (collapse `..`/`.`), same intent
        // as Node's `path.resolve` for a possibly-nonexistent path.
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                other => out.push(other.as_os_str()),
            }
        }
        out
    })
}

#[derive(Debug)]
pub enum AppendOutcome {
    Persisted,
    Unavailable(&'static str),
}

/// Mirrors `appendObservableEvent`. `explicit_target` mirrors
/// `process.env.MEMBRANE_TELEMETRY_INGRESS`; `resolved` mirrors the cached
/// default-resolution result (`None` = unresolvable).
pub fn append_observable_event(
    event: &Value,
    explicit_target: Option<&Path>,
    resolved: Option<&ResolvedIngressTarget>,
) -> Result<AppendOutcome, String> {
    validate_observable_event(event)?;

    let (target, require_drain_evidence) = match explicit_target {
        Some(explicit) => (explicit.to_path_buf(), false),
        None => match resolved {
            None => return Ok(AppendOutcome::Unavailable("telemetry_ingress_unconfigured")),
            Some(resolved) => {
                let requires_drain = !resolved.db_path.exists();
                (resolved.target.clone(), requires_drain)
            }
        },
    };

    if require_drain_evidence {
        return Ok(AppendOutcome::Unavailable(
            "telemetry_ingress_drain_service_not_running",
        ));
    }

    let record = format!("{}\n", json!({"observable_events": [event]}));
    if record.len() > MAX_RECORD_BYTES {
        return Ok(AppendOutcome::Unavailable("telemetry_record_too_large"));
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&target)
        .map_err(|_| "telemetry_ingress_unavailable".to_string())?;
    file.write_all(record.as_bytes())
        .map_err(|_| "telemetry_ingress_unavailable".to_string())?;
    file.sync_all().ok();
    Ok(AppendOutcome::Persisted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_observable_event::{build_observable_event, BuildObservableEvent};
    use tempfile::tempdir;

    fn sample_event() -> Value {
        build_observable_event(BuildObservableEvent {
            installation_id: "install-1",
            client_id: "codex",
            session_id: "session-1",
            task_id: "task-1",
            turn_id: "turn-1",
            trace_id: "trace-1",
            event_type: "tool_receipt",
            origin: "tool",
            content: "private command output",
            completeness: vec![("receipt", true)],
            timestamp: "2026-08-01T00:00:00.000Z".to_string(),
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn membrane_local_v1_runtime_json_resolves_default_ingress_identity() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("runtime.json");
        fs::write(
            &config_path,
            json!({"schemaVersion": 1, "serviceId": "membrane-local-v1", "host": "127.0.0.1", "port": 47851})
                .to_string(),
        )
        .unwrap();
        let resolved = resolve_default_ingress_target(&config_path, None).unwrap();
        assert!(resolved
            .target
            .to_string_lossy()
            .ends_with("context-telemetry-ingress.jsonl"));
        assert_eq!(resolved.target.parent(), resolved.db_path.parent());
    }

    #[test]
    fn default_path_is_used_when_unset_and_db_exists() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("cortex-engine.db");
        fs::write(&db_path, "").unwrap();
        let resolved = ResolvedIngressTarget {
            target: dir.path().join("context-telemetry-ingress.jsonl"),
            db_path: db_path.clone(),
        };
        let event = sample_event();
        let outcome = append_observable_event(&event, None, Some(&resolved)).unwrap();
        assert!(matches!(outcome, AppendOutcome::Persisted));
        let record: Value =
            serde_json::from_str(&fs::read_to_string(&resolved.target).unwrap()).unwrap();
        assert_eq!(record["observable_events"][0], event);
    }

    #[test]
    fn resolved_but_service_not_running_reports_distinct_reason() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("cortex-engine.db"); // never created
        let resolved = ResolvedIngressTarget {
            target: dir.path().join("context-telemetry-ingress.jsonl"),
            db_path,
        };
        let outcome = append_observable_event(&sample_event(), None, Some(&resolved)).unwrap();
        assert!(matches!(
            outcome,
            AppendOutcome::Unavailable("telemetry_ingress_drain_service_not_running")
        ));
        assert!(!resolved.target.exists());
    }

    #[test]
    fn explicit_target_overrides_default_bypassing_drain_gate() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("events.jsonl");
        let event = sample_event();
        let outcome = append_observable_event(&event, Some(&target), None).unwrap();
        assert!(matches!(outcome, AppendOutcome::Persisted));
        let record: Value = serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(record["observable_events"][0], event);
    }

    #[test]
    fn unresolvable_config_returns_none_never_panics() {
        let missing = PathBuf::from("does-not-exist-membrane-runtime.json");
        assert!(resolve_default_ingress_target(&missing, None).is_none());
    }

    #[test]
    fn unresolvable_config_malformed_json_returns_none() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("runtime.json");
        fs::write(&config_path, "{ not valid json").unwrap();
        assert!(resolve_default_ingress_target(&config_path, None).is_none());
    }

    #[test]
    fn wrong_identity_fields_all_fail() {
        let dir = tempdir().unwrap();
        let cases = [
            json!({"schemaVersion": 1, "serviceId": "not-membrane-local-v1", "host": "127.0.0.1", "port": 47851}),
            json!({"schemaVersion": 2, "serviceId": "membrane-local-v1", "host": "127.0.0.1", "port": 47851}),
            json!({"schemaVersion": 1, "serviceId": "membrane-local-v1", "host": "0.0.0.0", "port": 47851}),
        ];
        for (index, case) in cases.iter().enumerate() {
            let config_path = dir.path().join(format!("runtime-{index}.json"));
            fs::write(&config_path, case.to_string()).unwrap();
            assert!(resolve_default_ingress_target(&config_path, None).is_none());
            assert!(resolve_runtime_config_identity(&config_path).is_err());
        }
    }

    #[test]
    fn unresolvable_default_resolution_surfaces_as_honest_unavailable() {
        let outcome = append_observable_event(&sample_event(), None, None).unwrap();
        assert!(matches!(
            outcome,
            AppendOutcome::Unavailable("telemetry_ingress_unconfigured")
        ));
    }

    #[test]
    fn forbidden_field_validation_still_rejects_tampered_records() {
        let mut event = sample_event();
        event["extra_field"] = json!("nope");
        let err = append_observable_event(&event, Some(Path::new("ignored.jsonl")), None)
            .unwrap_err();
        assert!(err.contains("forbidden field"));
    }
}
