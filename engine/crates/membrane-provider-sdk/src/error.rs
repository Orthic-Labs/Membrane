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

    /// The request was cancelled before the provider could complete.
    #[error("provider request cancelled")]
    Cancelled,

    /// The provider reached the one absolute request deadline.
    #[error("provider deadline exhausted")]
    DeadlineExceeded,

    /// A required source was not supplied by composition.
    #[error("required provider source is missing: {0}")]
    MissingSource(&'static str),

    /// Provider output violated the federation envelope contract.
    #[error("malformed provider output: {0}")]
    MalformedOutput(String),

    /// Provider output was validly shaped but did not cover the requested lane.
    #[error("provider output is incomplete: {0}")]
    Incomplete(String),

    /// Provider output was bound to a different provider or generation.
    #[error("provider output identity mismatch: {0}")]
    IdentityMismatch(String),

    /// A source returned an explicit typed failure.
    #[error("provider source failed: {0}")]
    SourceFailure(String),

    /// A composition or adapter invariant failed.
    #[error("provider internal error: {0}")]
    Internal(String),

    #[error("duplicate provider registration: {0}")]
    DuplicateProvider(String),

    #[error("unknown provider registration: {0}")]
    UnknownProvider(String),

    #[error("invalid provider registry: {0}")]
    InvalidRegistry(String),

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
