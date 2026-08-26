//! # membrane-transcript
//!
//! Deterministic `TranscriptEventV1` normalizer for Membrane continuity.
//!
//! Public surface:
//! - [`parse`] / [`parse_source_events`] / [`parse_prefix_receipt`] — canonical
//!   parser entry points;
//! - [`detect_host`] — host detection from transcript bytes;
//! - [`resolve_session`] — exact-match session resolver (substring rejected);
//! - [`TranscriptEventV1`], [`EventFlags`], [`PrefixReceipt`] — V1 shapes;
//! - [`evidence::ActKind`] / [`evidence::EvidenceClass`] — Adapt evidence taxonomy;
//! - [`TranscriptError`] — typed fail-closed omission/degradation errors.
//!
//! This is an internal Adapt/continuity domain contract, not one of Membrane's
//! five public protocol shapes. No Python or Node runtime is involved.

pub mod adapters;
pub mod canonical;
pub mod classify;
pub mod error;
pub mod event;
pub mod evidence;
pub mod parser;
pub mod redact;
pub mod source;

pub use adapters::{detect_host, GENERIC_HOSTS};
pub use error::{Result, TranscriptError};
pub use event::{
    EventFlags, PrefixReceipt, TranscriptEventV1, MAX_ASSISTANT_CHARS, MAX_EVENT_CHARS,
    MAX_TOOL_CALL_CHARS, MAX_TOOL_RESULT_CHARS,
};
pub use parser::{
    parse, parse_prefix_receipt, parse_source_events, resolve_session, SessionCandidate,
};
pub use source::{discover_open, DiscoveredTranscript};

/// Canonical parser version tag.
pub const PARSER_VERSION: &str = "membrane.transcript-event.v1";

#[cfg(test)]
mod lib_tests {
    use super::*;

    #[test]
    fn parser_version_is_stable() {
        assert_eq!(PARSER_VERSION, "membrane.transcript-event.v1");
    }

    #[test]
    fn parser_digest_is_content_bound() {
        let d1 = canonical::parser_digest();
        let d2 = canonical::parser_digest();
        assert_eq!(d1, d2, "digest must be deterministic within a build");
        assert_eq!(d1.len(), 64);
        assert!(d1
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    }
}
