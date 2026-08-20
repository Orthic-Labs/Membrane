//! Per-candidate recall feedback (the Membrane feedback rail).
//!
//! A `FeedbackRecord` binds one admitted candidate (by the `ContextReceiptV2` join key
//! `trace_id` + `candidate_id` + `content_sha256`) to an observed outcome. Only *verified*
//! feedback affects ranking; the source of verification decides that:
//!
//!   - `ObservedAction` — the store saw a downstream action that proves the outcome (for example,
//!     delete/supersede = contradicted). A plain `get` proves retrieval only, never usefulness.
//!     Inherently verified, per-candidate, lossless. This is the coverage fix
//!     from Council Loop 1 (grounded in Memory-R1 / AgeMem store/update/delete-as-feedback).
//!   - `CitedVerdict` — an external verdict that *explicitly names this candidate id + sha*.
//!     Verified only when `verdict_ref` resolves (fail-closed).
//!   - `Advisory` — agent self-report or an uncited verdict. Persisted for observability but
//!     NEVER drives ranking (GhostWriter write-time defense).

use cortex_core::Outcome;

/// How a feedback row's outcome was established. Determines whether it can affect ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackSource {
    /// Store-internal observed downstream action (delete / supersede). Verified, no verdict needed.
    ObservedAction,
    /// External verdict that cites this candidate id + sha. Verified iff `verdict_ref` resolves.
    CitedVerdict,
    /// Agent self-report or uncited verdict. Advisory only; never ranks.
    Advisory,
}

/// Parse an outcome token as emitted on the wire / CLI.
pub fn parse_outcome(s: &str) -> Result<Outcome, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "used" => Ok(Outcome::Used),
        "ignored" => Ok(Outcome::Ignored),
        "contradicted" => Ok(Outcome::Contradicted),
        other => Err(format!(
            "unknown outcome {other:?}; want used|ignored|contradicted"
        )),
    }
}

pub fn outcome_str(o: Outcome) -> &'static str {
    match o {
        Outcome::Used => "used",
        Outcome::Ignored => "ignored",
        Outcome::Contradicted => "contradicted",
    }
}

/// Parse a source token from the wire / CLI. Defaults to `Advisory` (fail-safe: an unspecified or
/// unknown source can never rank).
pub fn parse_source(s: &str) -> FeedbackSource {
    match s.trim().to_ascii_lowercase().as_str() {
        "observed_action" | "observed" => FeedbackSource::ObservedAction,
        "cited_verdict" | "verdict" => FeedbackSource::CitedVerdict,
        _ => FeedbackSource::Advisory,
    }
}

/// One per-candidate feedback row, pre-persistence.
#[derive(Debug, Clone)]
pub struct FeedbackRecord {
    pub trace_id: String,
    pub candidate_id: String,
    pub content_sha256: String,
    pub outcome: Outcome,
    pub source: FeedbackSource,
    /// Required (non-empty) when `source == CitedVerdict`.
    pub verdict_ref: Option<String>,
    pub scope_id: String,
}

impl FeedbackRecord {
    /// Does this row affect ranking? Only verified sources do; advisory never does.
    pub fn verified(&self) -> bool {
        matches!(
            self.source,
            FeedbackSource::ObservedAction | FeedbackSource::CitedVerdict
        )
    }

    /// Validate the record before persistence. Fail-closed: a `CitedVerdict` with no resolvable
    /// `verdict_ref` is rejected (it cannot be trusted to rank). Advisory rows always validate but
    /// carry `verified() == false`.
    pub fn validate(&self) -> Result<(), String> {
        if self.trace_id.trim().is_empty() {
            return Err("feedback: empty trace_id".into());
        }
        if self.candidate_id.trim().is_empty() {
            return Err("feedback: empty candidate_id".into());
        }
        if self.content_sha256.trim().is_empty() {
            return Err("feedback: empty content_sha256".into());
        }
        if self.source == FeedbackSource::CitedVerdict
            && self
                .verdict_ref
                .as_deref()
                .map(|r| r.trim().is_empty())
                .unwrap_or(true)
        {
            return Err(
                "feedback: a cited-verdict row must carry a non-empty verdict_ref to rank".into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(source: FeedbackSource, verdict_ref: Option<&str>, outcome: Outcome) -> FeedbackRecord {
        FeedbackRecord {
            trace_id: "trace-1".into(),
            candidate_id: "mem-1".into(),
            content_sha256: "abc123".into(),
            outcome,
            source,
            verdict_ref: verdict_ref.map(|s| s.to_string()),
            scope_id: "global".into(),
        }
    }

    #[test]
    fn ranking_outcome_requires_verified_source() {
        // Cited verdict without a ref cannot rank -> Err.
        let bad = rec(FeedbackSource::CitedVerdict, None, Outcome::Contradicted);
        assert!(bad.validate().is_err());

        // Cited verdict WITH a ref is verified and valid.
        let ok = rec(
            FeedbackSource::CitedVerdict,
            Some(".audit/x/verdict.json#mem-1"),
            Outcome::Contradicted,
        );
        assert!(ok.validate().is_ok());
        assert!(ok.verified());
    }

    #[test]
    fn observed_action_is_verified_without_a_verdict() {
        let r = rec(FeedbackSource::ObservedAction, None, Outcome::Used);
        assert!(r.validate().is_ok());
        assert!(r.verified(), "an observed action is inherently verified");
    }

    #[test]
    fn advisory_validates_but_never_ranks() {
        let r = rec(FeedbackSource::Advisory, None, Outcome::Contradicted);
        assert!(r.validate().is_ok());
        assert!(!r.verified(), "advisory feedback must not affect ranking");
    }

    #[test]
    fn empty_join_key_is_rejected() {
        let mut r = rec(FeedbackSource::ObservedAction, None, Outcome::Used);
        r.candidate_id = "  ".into();
        assert!(r.validate().is_err());
    }

    #[test]
    fn outcome_roundtrip() {
        for o in [Outcome::Used, Outcome::Ignored, Outcome::Contradicted] {
            assert_eq!(parse_outcome(outcome_str(o)).unwrap(), o);
        }
        assert!(parse_outcome("bogus").is_err());
    }
}
