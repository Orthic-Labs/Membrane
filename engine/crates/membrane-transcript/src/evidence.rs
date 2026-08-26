//! Transcript evidence taxonomy consumed by native Adapt.
//!
//! Authorization is established by caller-selected transcript source files,
//! parser prefix digests, external-user role, and mandatory review. No host
//! login, signature, trust store, or replay database participates here.

use serde::{Deserialize, Serialize};

/// Kinds of user-originated signals used by Adapt's deterministic taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActKind {
    ExplicitPreference,
    Correction,
    Reject,
    Accept,
    PostAcceptEdit,
    RepeatedEdit,
    NamedChoice,
}

impl ActKind {
    /// Default weighting affects confidence/review only; it never grants
    /// authority outside selected transcript review.
    pub fn default_signal_strength(self) -> f64 {
        match self {
            ActKind::ExplicitPreference => 1.00,
            ActKind::Correction => 0.95,
            ActKind::PostAcceptEdit | ActKind::RepeatedEdit => 0.85,
            ActKind::NamedChoice => 0.75,
            ActKind::Reject => 0.65,
            ActKind::Accept => 0.20,
        }
    }

    pub fn is_user_authoritative_kind(self) -> bool {
        matches!(
            self,
            ActKind::ExplicitPreference
                | ActKind::Correction
                | ActKind::Reject
                | ActKind::NamedChoice
        )
    }
}

/// Evidence classes used in candidate and durable-record projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    UserAuthoritative,
    UserBehavioral,
    Diagnostic,
    ContextOnly,
}
