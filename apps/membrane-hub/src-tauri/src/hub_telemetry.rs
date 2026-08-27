use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubDiagnostics {
    pub schema_version: u32,
    pub product_id: &'static str,
    pub owner: &'static str,
    pub service_identity: &'static str,
    pub service_state: String,
    pub snapshot_state: String,
    pub last_reason: Option<String>,
    pub observed_at_unix_ms: u64,
    pub log_path: String,
    pub resident_log_path: String,
}

pub struct HubTelemetry {
    path: PathBuf,
    state: Mutex<HubDiagnostics>,
    writer: Mutex<()>,
}

impl HubTelemetry {
    pub fn new(path: PathBuf) -> Self {
        Self {
            state: Mutex::new(HubDiagnostics {
                schema_version: 1,
                product_id: "membrane",
                owner: "hub",
                service_identity: "membrane-local-v1",
                service_state: "starting".into(),
                snapshot_state: "unknown".into(),
                last_reason: None,
                observed_at_unix_ms: now_ms(),
                log_path: path.to_string_lossy().into_owned(),
                resident_log_path: path
                    .with_file_name("resident.log")
                    .to_string_lossy()
                    .into_owned(),
            }),
            path,
            writer: Mutex::new(()),
        }
    }

    pub fn service(&self, state: &str, reason: Option<&str>) {
        self.update("service", state, reason);
    }

    pub fn snapshot(&self, state: &str, reason: Option<&str>) {
        self.update("snapshot", state, reason);
    }

    pub fn event_required(
        &self,
        event: &str,
        state: &str,
        reason: Option<&str>,
    ) -> Result<(), String> {
        self.write_required(event, state, reason)
    }

    pub fn report(&self) -> HubDiagnostics {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_else(|_| HubDiagnostics {
                schema_version: 1,
                product_id: "membrane",
                owner: "hub",
                service_identity: "membrane-local-v1",
                service_state: "unknown".into(),
                snapshot_state: "unknown".into(),
                last_reason: Some("telemetry_state_unavailable".into()),
                observed_at_unix_ms: now_ms(),
                log_path: self.path.to_string_lossy().into_owned(),
                resident_log_path: self
                    .path
                    .with_file_name("resident.log")
                    .to_string_lossy()
                    .into_owned(),
            })
    }

    fn update(&self, component: &str, value: &str, reason: Option<&str>) {
        let changed = self.state.lock().map(|mut state| {
            let previous = if component == "service" {
                state.service_state.as_str()
            } else {
                state.snapshot_state.as_str()
            };
            let changed = previous != value || state.last_reason.as_deref() != reason;
            let slot = if component == "service" {
                &mut state.service_state
            } else {
                &mut state.snapshot_state
            };
            *slot = value.into();
            state.last_reason = reason.map(str::to_owned);
            state.observed_at_unix_ms = now_ms();
            changed
        });
        if changed.unwrap_or(true) {
            self.write(&format!("{component}_state"), value, reason);
        }
    }

    fn write(&self, event: &str, state: &str, reason: Option<&str>) {
        let _ = self.write_required(event, state, reason);
    }

    fn write_required(&self, event: &str, state: &str, reason: Option<&str>) -> Result<(), String> {
        let _guard = self
            .writer
            .lock()
            .map_err(|_| "hub_telemetry_lock_unavailable".to_string())?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|_| "hub_telemetry_directory_unavailable")?;
        }
        if fs::metadata(&self.path).is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES) {
            let previous = self.path.with_extension("previous.jsonl");
            if let Err(error) = fs::remove_file(&previous) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    return Err("hub_telemetry_rotation_failed".into());
                }
            }
            fs::rename(&self.path, previous)
                .map_err(|_| "hub_telemetry_rotation_failed".to_string())?;
        }
        let record = serde_json::json!({
            "schemaVersion": 1,
            "productId": "membrane",
            "component": "hub",
            "event": event,
            "state": state,
            "reason": reason,
            "observedAtUnixMs": now_ms(),
        });
        let bytes = serde_json::to_vec(&record)
            .map_err(|_| "hub_telemetry_serialization_failed".to_string())?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|_| "hub_telemetry_open_failed".to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_data())
            .map_err(|_| "hub_telemetry_write_failed".to_string())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_and_jsonl_track_typed_transitions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hub.jsonl");
        let telemetry = HubTelemetry::new(path.clone());
        telemetry.service("running", None);
        telemetry.snapshot("unavailable", Some("snapshot_timeout"));
        let report = telemetry.report();
        assert_eq!(report.service_state, "running");
        assert_eq!(report.snapshot_state, "unavailable");
        assert_eq!(report.last_reason.as_deref(), Some("snapshot_timeout"));
        let records = fs::read_to_string(path).unwrap();
        assert!(records
            .lines()
            .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok()));
        assert!(records.contains("service_state"));
        assert!(records.contains("snapshot_timeout"));
    }

    #[test]
    fn unchanged_state_does_not_flood_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hub.jsonl");
        let telemetry = HubTelemetry::new(path.clone());
        telemetry.service("running", None);
        telemetry.service("running", None);
        assert_eq!(fs::read_to_string(path).unwrap().lines().count(), 1);
    }

    #[test]
    fn required_event_is_durable_or_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("required.jsonl");
        let telemetry = HubTelemetry::new(path.clone());
        telemetry
            .event_required("workspace_config_migration", "migrated", None)
            .unwrap();
        assert!(fs::read_to_string(path)
            .unwrap()
            .contains("workspace_config_migration"));

        let blocked_parent = dir.path().join("blocked-parent");
        fs::write(&blocked_parent, b"not-a-directory").unwrap();
        let blocked = HubTelemetry::new(blocked_parent.join("hub.jsonl"));
        assert!(blocked
            .event_required("workspace_config_migration", "migrated", None)
            .is_err());
    }
}
