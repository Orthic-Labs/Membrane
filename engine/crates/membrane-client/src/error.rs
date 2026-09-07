//! Errors exposed by the resident memory client.

use serde_json::{Map, Value};
use std::fmt;

/// A failure is classified before it crosses the CodeRight backend seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    Transport {
        message: String,
    },
    BackendUnavailable {
        message: String,
    },
    CommitUnknown {
        message: String,
        receipt_id: Option<String>,
    },
    Unavailable {
        message: String,
    },
    Timeout {
        message: String,
    },
    Cancelled,
    Incompatible {
        message: String,
    },
    Denied {
        message: String,
    },
    CorruptOrRotation {
        message: String,
    },
    Protocol {
        code: String,
        message: String,
        details: Map<String, Value>,
    },
    InvalidRequest {
        message: String,
    },
    NotFound {
        message: String,
    },
    Store {
        message: String,
    },
    Internal {
        message: String,
    },
}

impl ClientError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Transport { .. } => "transport_unavailable",
            Self::BackendUnavailable { .. } => "backend_unavailable",
            Self::CommitUnknown { .. } => "commit_unknown",
            Self::Unavailable { .. } => "service_unavailable",
            Self::Timeout { .. } => "deadline_exceeded",
            Self::Cancelled => "cancelled",
            Self::Incompatible { .. } => "incompatible",
            Self::Denied { .. } => "denied",
            Self::CorruptOrRotation { .. } => "corrupt_or_rotation",
            Self::Protocol { .. } => "protocol_error",
            Self::InvalidRequest { .. } => "invalid_request",
            Self::NotFound { .. } => "not_found",
            Self::Store { .. } => "store_error",
            Self::Internal { .. } => "internal_error",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport { .. }
                | Self::BackendUnavailable { .. }
                | Self::Unavailable { .. }
                | Self::Timeout { .. }
        )
    }

    pub fn protocol(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Protocol {
            code: code.into(),
            message: message.into(),
            details: Map::new(),
        }
    }

    pub(crate) fn from_json_error(value: &Value) -> Self {
        let message = value
            .get("error")
            .and_then(Value::as_str)
            .or_else(|| value.get("message").and_then(Value::as_str))
            .unwrap_or("resident service returned an error")
            .to_string();
        let code = value.get("code").and_then(Value::as_str).unwrap_or("");
        let receipt_id = value
            .get("details")
            .and_then(|details| {
                details
                    .get("receiptId")
                    .or_else(|| details.get("receipt_id"))
            })
            .and_then(Value::as_str)
            .map(str::to_owned);
        match code {
            "backend_unavailable" | "hub_inactive" => Self::BackendUnavailable { message },
            "commit_unknown" => Self::CommitUnknown {
                message,
                receipt_id,
            },
            "timeout" | "deadline_exceeded" => Self::Timeout { message },
            "cancelled" => Self::Cancelled,
            "not_found" | "target_not_found" => Self::NotFound { message },
            "invalid_request" | "missing_id" => Self::InvalidRequest { message },
            "denied" | "forbidden" | "unauthorized" | "context_scope_denied" => {
                Self::Denied { message }
            }
            "corrupt" | "corrupt_state" | "rotation_required" | "rotation_in_progress" => {
                Self::CorruptOrRotation { message }
            }
            "provider_failed" | "commit_failed" => Self::Store { message },
            "incompatible" | "protocol_version_unsupported" => Self::Incompatible { message },
            // Context-federation failures are protocol outcomes, not service
            // availability failures. Preserve their public codes so hosts can
            // fail closed without guessing a budget or reduction plan.
            "h8_unavailable"
            | "context_capacity_unavailable"
            | "capacity_unavailable"
            | "estimator_basis_mismatch"
            | "basis_mismatch"
            | "no_floor"
            | "no_viable_floor"
            | "no_representation_fits"
            | "capacity_changed"
            | "changed_capacity"
            | "selection_unavailable"
            | "plan_unavailable" => Self::protocol(code, message),
            _ => Self::Unavailable { message },
        }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("resident memory call cancelled"),
            Self::Protocol { code, message, .. } => {
                write!(f, "resident protocol error {code}: {message}")
            }
            Self::Transport { message } => write!(f, "resident transport failed: {message}"),
            Self::BackendUnavailable { message } => {
                write!(f, "resident backend unavailable: {message}")
            }
            Self::CommitUnknown {
                message,
                receipt_id,
            } => write!(
                f,
                "resident commit outcome unknown{}: {message}",
                receipt_id
                    .as_deref()
                    .map(|id| format!(" ({id})"))
                    .unwrap_or_default()
            ),
            Self::Unavailable { message } => write!(f, "resident service unavailable: {message}"),
            Self::Timeout { message } => write!(f, "resident memory deadline exceeded: {message}"),
            Self::Incompatible { message } => write!(f, "resident service incompatible: {message}"),
            Self::Denied { message } => write!(f, "resident service denied binding: {message}"),
            Self::CorruptOrRotation { message } => {
                write!(f, "resident installation corrupt or rotating: {message}")
            }
            Self::InvalidRequest { message } => {
                write!(f, "invalid resident memory request: {message}")
            }
            Self::NotFound { message } => write!(f, "resident memory record not found: {message}"),
            Self::Store { message } => write!(f, "resident memory store failed: {message}"),
            Self::Internal { message } => {
                write!(f, "resident memory client internal error: {message}")
            }
        }
    }
}

impl std::error::Error for ClientError {}
