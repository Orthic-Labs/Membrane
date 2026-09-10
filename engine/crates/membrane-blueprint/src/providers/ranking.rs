//! Port of `blueprint/src/providers/ranking/types.mjs` (D44: ranking
//! provider types). The legacy module is a two-entry frozen vocabulary; this
//! is a field-for-field port, not a reinterpretation.

/// A provider that supplies candidates only, no scoring — mirrors
/// `RANKING_TYPES.candidateProvider === "candidate-only"`.
pub const RANKING_TYPE_CANDIDATE_PROVIDER: &str = "candidate-only";

/// A provider that reranks candidates with explainable components — mirrors
/// `RANKING_TYPES.reranker === "explainable-components"`.
pub const RANKING_TYPE_RERANKER: &str = "explainable-components";

/// The closed set of ranking-provider roles a provider may declare, mirroring
/// the frozen `RANKING_TYPES` object exactly (no additional roles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RankingType {
    CandidateProvider,
    Reranker,
}

impl RankingType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CandidateProvider => RANKING_TYPE_CANDIDATE_PROVIDER,
            Self::Reranker => RANKING_TYPE_RERANKER,
        }
    }

    /// Mirrors looking up a value against `RANKING_TYPES`'s frozen values;
    /// an unknown string is a typed `None`, never a silent default.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            RANKING_TYPE_CANDIDATE_PROVIDER => Some(Self::CandidateProvider),
            RANKING_TYPE_RERANKER => Some(Self::Reranker),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_legacy_frozen_values() {
        assert_eq!(RANKING_TYPE_CANDIDATE_PROVIDER, "candidate-only");
        assert_eq!(RANKING_TYPE_RERANKER, "explainable-components");
    }

    #[test]
    fn parse_round_trips() {
        assert_eq!(RankingType::parse("candidate-only"), Some(RankingType::CandidateProvider));
        assert_eq!(RankingType::parse("explainable-components"), Some(RankingType::Reranker));
        assert_eq!(RankingType::CandidateProvider.as_str(), "candidate-only");
        assert_eq!(RankingType::Reranker.as_str(), "explainable-components");
    }

    #[test]
    fn unknown_role_is_typed_none_not_silent_default() {
        assert_eq!(RankingType::parse("scored"), None);
        assert_eq!(RankingType::parse(""), None);
    }
}
