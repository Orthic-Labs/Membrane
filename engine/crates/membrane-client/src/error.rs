//! Errors exposed by the resident memory client.

use serde_json::{Map, Value};
use std::fmt;

/// A failure is classified before it crosses the CodeRight backend seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    Transport { message: String },
    Unavailable { message: String },
    Timeout { message: String },
    Cancelled,
    Incompatible { message: String },
    Protocol { code: String, message: String, details: Map<String, Value> },
    InvalidRequest { message: String },
    NotFound { message: String },
    Store { message: String },
    Internal { message: String },
}

impl ClientError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Transport { .. } => "transport_unavailable",
            Self::Unavailable { .. } => "service_unavailable",
            Self::Timeout { .. } => "deadline_exceeded",
            Self::Cancelled => "cancelled",
            Self::Incompatible { .. } => "incompatible",
            Self::Protocol { .. } => "protocol_error",
            Self::InvalidRequest { .. } => "invalid_request",
            Self::NotFound { .. } => "not_found",
            Self::Store { .. } => "store_error",
            Self::Internal { .. } => "internal_error",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::Transport { .. } | Self::Unavailable { .. } | Self::Timeout { .. })
    }

    pub fn protocol(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Protocol { code: code.into(), message: message.into(), details: Map::new() }
    }

    pub(crate) fn from_json_error(value: &Value) -> Self {
        let message = value.get("error").and_then(Value::as_str)
            .or_else(|| value.get("message").and_then(Value::as_str))
            .unwrap_or("resident service returned an error").to_string();
        let code = value.get("code").and_then(Value::as_str).unwrap_or("");
        match code {
            "timeout" | "deadline_exceeded" => Self::Timeout { message },
            "cancelled" => Self::Cancelled,
            "not_found" | "target_not_found" => Self::NotFound { message },
            "invalid_request" | "missing_id" => Self::InvalidRequest { message },
            "provider_failed" | "commit_failed" => Self::Store { message },
            "incompatible" | "protocol_version_unsupported" => Self::Incompatible { message },
            _ => Self::Unavailable { message },
        }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("resident memory call cancelled"),
            Self::Protocol { code, message, .. } => write!(f, "resident protocol error {code}: {message}"),
            Self::Transport { message } => write!(f, "resident transport failed: {message}"),
            Self::Unavailable { message } => write!(f, "resident service unavailable: {message}"),
            Self::Timeout { message } => write!(f, "resident memory deadline exceeded: {message}"),
            Self::Incompatible { message } => write!(f, "resident service incompatible: {message}"),
            Self::InvalidRequest { message } => write!(f, "invalid resident memory request: {message}"),
            Self::NotFound { message } => write!(f, "resident memory record not found: {message}"),
            Self::Store { message } => write!(f, "resident memory store failed: {message}"),
            Self::Internal { message } => write!(f, "resident memory client internal error: {message}"),
        }
    }
}

impl std::error::Error for ClientError {}
