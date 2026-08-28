//! Typed bootstrap/control contract between native tray & headless daemon.
//!
//! Frames are newline-delimited UTF-8 JSON. This module owns only transport
//! framing & validation; runtime health remains on authenticated Hub HTTP.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

pub const DAEMON_IPC_SCHEMA_VERSION: u32 = 1;
pub const DAEMON_IPC_MAX_FRAME_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonLaunchKind {
    Launch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonCommandKind {
    Drain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonEventKind {
    Ready,
    Draining,
    Drained,
    Fatal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DaemonLaunchV1 {
    pub schema_version: u32,
    pub sequence: u64,
    pub kind: DaemonLaunchKind,
    pub workspace_root: String,
    pub http_port: u16,
    pub bearer_token: String,
    pub parent_pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DaemonCommandV1 {
    pub schema_version: u32,
    pub sequence: u64,
    pub kind: DaemonCommandKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DaemonEventV1 {
    pub schema_version: u32,
    pub sequence: u64,
    pub kind: DaemonEventKind,
    pub pid: u32,
    pub observed_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DaemonProtocolError {
    #[error("daemon IPC frame exceeds {DAEMON_IPC_MAX_FRAME_BYTES} bytes")]
    FrameTooLarge,
    #[error("daemon IPC frame is not valid UTF-8")]
    InvalidUtf8,
    #[error("daemon IPC frame must contain exactly one JSON line")]
    InvalidLine,
    #[error("daemon IPC JSON is invalid: {0}")]
    InvalidJson(String),
    #[error("unsupported daemon IPC schema version {0}")]
    UnsupportedSchemaVersion(u32),
    #[error("daemon IPC sequence must be non-zero")]
    ZeroSequence,
    #[error("daemon IPC sequence regressed from {last} to {next}")]
    SequenceRegression { last: u64, next: u64 },
    #[error("daemon launch workspace root is empty")]
    EmptyWorkspaceRoot,
    #[error("daemon launch bearer token must be exactly 64 lowercase hexadecimal characters")]
    InvalidBearerToken,
    #[error("daemon launch parent PID is zero")]
    ZeroParentPid,
    #[error("daemon event PID is zero")]
    ZeroEventPid,
}

fn validate_header(schema_version: u32, sequence: u64) -> Result<(), DaemonProtocolError> {
    if schema_version != DAEMON_IPC_SCHEMA_VERSION {
        return Err(DaemonProtocolError::UnsupportedSchemaVersion(
            schema_version,
        ));
    }
    if sequence == 0 {
        return Err(DaemonProtocolError::ZeroSequence);
    }
    Ok(())
}

fn validate_sequence(next: u64, last: Option<u64>) -> Result<(), DaemonProtocolError> {
    if let Some(last) = last {
        if next <= last {
            return Err(DaemonProtocolError::SequenceRegression { last, next });
        }
    }
    Ok(())
}

impl DaemonLaunchV1 {
    pub fn validate(&self) -> Result<(), DaemonProtocolError> {
        validate_header(self.schema_version, self.sequence)?;
        if self.workspace_root.trim().is_empty() {
            return Err(DaemonProtocolError::EmptyWorkspaceRoot);
        }
        if self.bearer_token.len() != 64
            || !self
                .bearer_token
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(DaemonProtocolError::InvalidBearerToken);
        }
        if self.parent_pid == 0 {
            return Err(DaemonProtocolError::ZeroParentPid);
        }
        Ok(())
    }
}

impl DaemonCommandV1 {
    pub fn validate(&self, last_sequence: Option<u64>) -> Result<(), DaemonProtocolError> {
        validate_header(self.schema_version, self.sequence)?;
        validate_sequence(self.sequence, last_sequence)
    }
}

impl DaemonEventV1 {
    pub fn validate(&self, last_sequence: Option<u64>) -> Result<(), DaemonProtocolError> {
        validate_header(self.schema_version, self.sequence)?;
        validate_sequence(self.sequence, last_sequence)?;
        if self.pid == 0 {
            return Err(DaemonProtocolError::ZeroEventPid);
        }
        Ok(())
    }
}

/// Encodes one JSON frame including its trailing newline.
pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, DaemonProtocolError> {
    let mut frame = serde_json::to_vec(value)
        .map_err(|error| DaemonProtocolError::InvalidJson(error.to_string()))?;
    frame.push(b'\n');
    if frame.len() > DAEMON_IPC_MAX_FRAME_BYTES {
        return Err(DaemonProtocolError::FrameTooLarge);
    }
    Ok(frame)
}

/// Decodes one JSON frame. Call its typed `validate` method after decoding.
pub fn decode_frame<T: DeserializeOwned>(frame: &[u8]) -> Result<T, DaemonProtocolError> {
    if frame.len() > DAEMON_IPC_MAX_FRAME_BYTES {
        return Err(DaemonProtocolError::FrameTooLarge);
    }
    let text = std::str::from_utf8(frame).map_err(|_| DaemonProtocolError::InvalidUtf8)?;
    let line = text
        .strip_suffix('\n')
        .ok_or(DaemonProtocolError::InvalidLine)?;
    if line.contains(['\n', '\r']) || line.is_empty() {
        return Err(DaemonProtocolError::InvalidLine);
    }
    serde_json::from_str(line).map_err(|error| DaemonProtocolError::InvalidJson(error.to_string()))
}

pub fn decode_launch_frame(frame: &[u8]) -> Result<DaemonLaunchV1, DaemonProtocolError> {
    let launch: DaemonLaunchV1 = decode_frame(frame)?;
    launch.validate()?;
    Ok(launch)
}

pub fn decode_command_frame(
    frame: &[u8],
    last_sequence: Option<u64>,
) -> Result<DaemonCommandV1, DaemonProtocolError> {
    let command: DaemonCommandV1 = decode_frame(frame)?;
    command.validate(last_sequence)?;
    Ok(command)
}

pub fn decode_event_frame(
    frame: &[u8],
    last_sequence: Option<u64>,
) -> Result<DaemonEventV1, DaemonProtocolError> {
    let event: DaemonEventV1 = decode_frame(frame)?;
    event.validate(last_sequence)?;
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch() -> DaemonLaunchV1 {
        DaemonLaunchV1 {
            schema_version: 1,
            sequence: 1,
            kind: DaemonLaunchKind::Launch,
            workspace_root: r"C:\workspace".into(),
            http_port: 4317,
            bearer_token: "a".repeat(64),
            parent_pid: 42,
        }
    }

    #[test]
    fn launch_round_trips_as_spec_shape() {
        let frame = encode_frame(&launch()).unwrap();
        assert!(frame.len() < DAEMON_IPC_MAX_FRAME_BYTES);
        let decoded = decode_launch_frame(&frame).unwrap();
        assert_eq!(decoded, launch());
        assert_eq!(
            String::from_utf8(frame).unwrap().trim_end(),
            format!(
                r#"{{"schemaVersion":1,"sequence":1,"kind":"launch","workspaceRoot":"C:\\workspace","httpPort":4317,"bearerToken":"{}","parentPid":42}}"#,
                "a".repeat(64)
            )
        );
    }

    #[test]
    fn unknown_fields_and_schema_versions_are_rejected() {
        let unknown = br#"{"schemaVersion":1,"sequence":1,"kind":"drain","extra":true}
"#;
        assert!(matches!(
            decode_command_frame(unknown, None),
            Err(DaemonProtocolError::InvalidJson(_))
        ));
        let wrong_version = br#"{"schemaVersion":2,"sequence":1,"kind":"drain"}
"#;
        assert_eq!(
            decode_command_frame(wrong_version, None),
            Err(DaemonProtocolError::UnsupportedSchemaVersion(2))
        );
    }

    #[test]
    fn sequence_regression_and_bad_frames_are_rejected() {
        let event =
            br#"{"schemaVersion":1,"sequence":2,"kind":"ready","pid":9,"observedAtUnixMs":10}
"#;
        assert_eq!(
            decode_event_frame(event, Some(2)),
            Err(DaemonProtocolError::SequenceRegression { last: 2, next: 2 })
        );
        assert_eq!(
            decode_frame::<DaemonCommandV1>(b"{}"),
            Err(DaemonProtocolError::InvalidLine)
        );
        assert_eq!(
            decode_frame::<DaemonCommandV1>(&vec![b'x'; DAEMON_IPC_MAX_FRAME_BYTES + 1]),
            Err(DaemonProtocolError::FrameTooLarge)
        );
    }

    #[test]
    fn event_optional_fields_are_omitted() {
        let event = DaemonEventV1 {
            schema_version: 1,
            sequence: 1,
            kind: DaemonEventKind::Drained,
            pid: 9,
            observed_at_unix_ms: 10,
            endpoint: None,
            reason: None,
        };
        let json = serde_json::to_value(event).unwrap();
        assert!(json.get("endpoint").is_none());
        assert!(json.get("reason").is_none());
    }

    #[test]
    fn launch_rejects_non_hex_or_wrong_length_tokens() {
        let mut invalid = launch();
        invalid.bearer_token = "token".into();
        assert_eq!(
            invalid.validate(),
            Err(DaemonProtocolError::InvalidBearerToken)
        );
        invalid.bearer_token = "A".repeat(64);
        assert_eq!(
            invalid.validate(),
            Err(DaemonProtocolError::InvalidBearerToken)
        );
    }
}
