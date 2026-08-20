//! Typed errors raised by the provider SDK and the conformance harness.
//!
//! Every error that flows out of a `Provider` implementation or the
//! `run_conformance` runner is one of these variants. The same error type is
//! used for both the trait surface (`handle_operation`) and the conformance
//! loop so adapters only need to learn one error vocabulary.

use thiserror::Error;

/// Every recoverable error a `Provider` implementation can surface.
///
/// `UnknownOperation` is reserved for requests that name an operation the
/// provider does not advertise in `list_capabilities`. `ConformanceMismatch`
/// is only produced by `run_conformance`; adapters MUST NOT raise it from
/// `handle_operation`.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// The provider has not been initialized (or initialization failed).
    #[error("provider has not been initialized: {0}")]
    Uninitialized(String),

    /// The named operation is not in this provider's capability set.
    #[error("unknown operation: {0}")]
    UnknownOperation(String),

    /// The provider is configured but the request envelope is malformed
    /// (missing required fields, wrong types, closed-enum violation).
    #[error("invalid request envelope for operation {operation}: {message}")]
    InvalidRequest {
        /// Operation the request targeted.
        operation: String,
        /// Human-readable reason.
        message: String,
    },

    /// The provider chose to return a typed error for this request.
    ///
    /// `code` is drawn from the operation's closed error taxonomy. `details`
    /// is an open map preserved verbatim.
    #[error("provider returned typed error {code} for {operation}: {message}")]
    Typed {
        /// Operation the request targeted.
        operation: String,
        /// Closed error code.
        code: String,
        /// Human-readable message.
        message: String,
        /// Optional operation-specific details.
        details: Option<serde_json::Value>,
    },

    /// The provider is not currently able to serve the request (transient).
    #[error("provider unavailable: {0}")]
    Unavailable(String),

    /// The conformance harness detected that the actual response did not
    /// match the expected response. Produced only by `run_conformance`.
    #[error("conformance mismatch on fixture {fixture}: {reason}")]
    ConformanceMismatch {
        /// Fixture name (e.g. `blueprint-context-scope-grant`).
        fixture: String,
        /// Why the response did not match the expected response.
        reason: String,
    },
}

/// Convenience alias for `Result<T, ProviderError>`.
pub type Result<T> = std::result::Result<T, ProviderError>;
