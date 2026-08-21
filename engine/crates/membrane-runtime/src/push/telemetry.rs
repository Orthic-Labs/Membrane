//! Push adoption telemetry.
//!
//! Push observations are process/host telemetry, never Cortex durable
//! knowledge.  When a host supplies `MEMBRANE_PUSH_TELEMETRY_PATH`, records
//! are appended as bounded JSONL.  Without that capability the observation is
//! intentionally dropped after validation; transforms remain independent of
//! telemetry availability.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushObservation<'a> {
    pub schema_version: u8,
    pub axis: &'static str,
    pub verb: &'a str,
    pub before: usize,
    pub after: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opportunity: Option<&'a str>,
}

const MAX_METADATA_BYTES: usize = 2048;

/// Emit one content-free Push observation to the host telemetry sink.
pub fn record(
    verb: &str,
    before: usize,
    after: usize,
    metadata: Option<&str>,
    opportunity: Option<&str>,
) {
    let Some(path) = std::env::var_os("MEMBRANE_PUSH_TELEMETRY_PATH") else {
        return;
    };
    if verb.is_empty() || verb.len() > 32 || metadata.is_some_and(|m| m.len() > MAX_METADATA_BYTES)
    {
        return;
    }
    let observation = PushObservation {
        schema_version: 1,
        axis: super::AXIS,
        verb,
        before,
        after,
        metadata,
        opportunity,
    };
    let Ok(line) = serde_json::to_string(&observation) else {
        return;
    };
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{line}");
    }
}
