//! Gate 1 — Adapt proposal eligibility.
//!
//! Decides whether evidence may form a Taste candidate. Single-source
//! admission policy: category taxonomy, rule shape, duplicate detection via
//! scoped identity, deterministic safety quarantine (origin, authority
//! effects, contradictions), and versioned policy bans. Passing this gate
//! grants no authority at the Cortex or context gates.

use crate::authority::{self, AuthorityResult, StoredRule};
use crate::canonical::normalize_text;
use crate::model_boundary::{ModelExtractionProposal, ModelProposalError};
use crate::record::{normalize_category, PreferenceRecordV1, RecordClass, RuleKey};
use crate::scope::ScopeDimensions;
use std::collections::BTreeMap;

/// Minimal imperative-starter set for durable sentence shape.
const IMPERATIVE_STARTERS: &[&str] = &[
    "always", "never", "use", "prefer", "run", "avoid", "stop", "do", "ensure",
    "require", "must", "keep", "check", "verify", "commit", "write", "read",
    "apply", "follow", "skip", "limit", "default",
];
const MIN_RULE_CHARS: usize = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EligibilityDecision {
    Admitted,
    Refused { reason: String },
}

impl EligibilityDecision {
    pub fn is_admitted(&self) -> bool {
        matches!(self, Self::Admitted)
    }
}

/// Require a durable sentence shape without a brittle word-count gate.
pub fn rule_shape_valid(body: &str) -> bool {
    let normalized = normalize_text(body);
    if normalized.chars().count() < MIN_RULE_CHARS {
        return false;
    }
    let first_word: String = normalized
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_end_matches("'t")
        .to_string();
    if IMPERATIVE_STARTERS.contains(&first_word.as_str()) {
        return true;
    }
    if format!(" {normalized} ").contains(" should ") {
        return true;
    }
    if normalized.starts_with("when ") {
        return true;
    }
    normalized.ends_with(['.', '!', '?'])
}

/// Scoped identity index over canonical rules.
#[derive(Debug, Clone, Default)]
pub struct RuleIndex {
    by_key: BTreeMap<RuleKey, ()>,
    by_id: BTreeMap<String, Vec<RuleKey>>,
}

impl RuleIndex {
    pub fn insert(&mut self, key: RuleKey) {
        self.by_id.entry(key.record_id.clone()).or_default().push(key.clone());
        self.by_key.insert(key, ());
    }

    pub fn has(&self, key: &RuleKey) -> bool {
        self.by_key.contains_key(key)
    }

    pub fn keys_for_id(&self, record_id: &str) -> Vec<&RuleKey> {
        self.by_id
            .get(record_id)
            .map(|keys| keys.iter().collect())
            .unwrap_or_default()
    }
}

/// Inputs to eligibility for one candidate.
pub struct EligibilityInput<'a> {
    pub operation: &'a str,
    pub rule: &'a str,
    pub category: &'a str,
    pub scope: &'a str,
    pub scope_dimensions_raw: &'a BTreeMap<String, String>,
    pub record_class: &'a str,
    pub origin: authority::Origin,
    pub evidence_text: &'a str,
    pub declared_authority_effect: Option<&'a str>,
    /// Versioned policy bans: `(reason, regex)` evaluated against the rule.
    pub policy_bans: &'a [(String, regex::Regex)],
    pub index: &'a RuleIndex,
    pub stored_rules: &'a [StoredRule],
}

/// Gate 1: decide whether a candidate is eligible to become a Taste proposal.
pub fn evaluate_eligibility(input: &EligibilityInput<'_>) -> EligibilityDecision {
    let operation = input.operation.trim().to_lowercase();
    if !matches!(operation.as_str(), "add" | "update" | "deprecate") {
        return refused(format!("unsupported-action:{}", operation));
    }

    // Non-Taste semantic classes are rejected outright from the Taste lane.
    let class = match RecordClass::parse(input.record_class) {
        Some(c) => c,
        None => return refused(format!("not-taste-class:{}", input.record_class)),
    };

    // Category taxonomy: unknown categories go to review, never active.
    if normalize_category(input.category).is_none() {
        return refused("category-not-allowed".into());
    }

    let body = input.rule.trim();
    if body.is_empty() {
        return refused("rule-empty".into());
    }

    // Fail-closed scope normalization BEFORE any matching/identity decision:
    // malformed dimensions can never silently widen applicability.
    if ScopeDimensions::normalize(input.scope_dimensions_raw).is_err() {
        return refused("scope-malformed".into());
    }

    let key = RuleKey::new(input.scope, body);
    match operation.as_str() {
        "update" | "deprecate" => {
            if !input.index.has(&key) && input.index.keys_for_id(&key.record_id).is_empty() {
                return refused(format!("{operation}-target-missing"));
            }
        }
        _ => {
            if input.index.has(&key) || input.index.has(&RuleKey::new("", &key.record_id)) {
                return refused("rule-duplicate".into());
            }
        }
    }

    for (reason, pattern) in input.policy_bans {
        if pattern.is_match(body) {
            return refused(reason.clone());
        }
    }

    let AuthorityResult { admitted, reason, .. } =
        authority::evaluate_origin(input.origin, input.evidence_text);
    if !admitted {
        return refused(reason);
    }

    if !rule_shape_valid(body) {
        return refused("rule-invalid-shape".into());
    }

    let conflicts = authority::detect_rule_contradictions(body, input.scope, input.stored_rules);
    if !conflicts.is_empty() {
        return refused("rule-conflict-needs-review".into());
    }

    let _ = class; // validated above
    EligibilityDecision::Admitted
}

fn refused(reason: String) -> EligibilityDecision {
    EligibilityDecision::Refused { reason }
}

/// Convenience: run eligibility for a model-proposed extraction after binding
/// it to qualifying user evidence. The model text is untrusted until this
/// deterministic gate passes.
pub fn evaluate_model_proposal(
    proposal: &ModelExtractionProposal,
    evidence_authenticated_user: bool,
    index: &RuleIndex,
    stored_rules: &[StoredRule],
) -> Result<EligibilityDecision, ModelProposalError> {
    if !evidence_authenticated_user {
        // A model proposal with no authenticated user evidence behind it can
        // never even be evaluated for Taste eligibility.
        return Err(ModelProposalError::UnboundEvidence);
    }
    let empty_bans: Vec<(String, regex::Regex)> = Vec::new();
    let dims = BTreeMap::new();
    Ok(evaluate_eligibility(&EligibilityInput {
        operation: "add",
        rule: &proposal.rule_text,
        category: &proposal.category_hint,
        scope: &proposal.scope_hint,
        scope_dimensions_raw: &dims,
        record_class: "standing_preference",
        origin: authority::Origin::UserTurn,
        evidence_text: &proposal.bound_evidence_excerpt,
        declared_authority_effect: None,
        policy_bans: &empty_bans,
        index,
        stored_rules,
    }))
}

/// Build a candidate record from an admitted proposal. This is the ONLY
/// constructor path out of gate 1, and it stamps provisional influence +
/// candidate lifecycle; nothing admitted here is durably authoritative yet.
pub fn build_candidate(
    rule: &str,
    category: &str,
    class: RecordClass,
    scope: &str,
    dims: ScopeDimensions,
    confidence: f64,
    evidence_ids: Vec<String>,
    now: &str,
) -> Result<PreferenceRecordV1, crate::record::RecordError> {
    PreferenceRecordV1::new_candidate(rule, category, class, scope, dims, confidence, evidence_ids, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_input<'a>(
        operation: &'a str,
        rule: &'a str,
        category: &'a str,
        scope: &'a str,
        origin: authority::Origin,
        evidence_text: &'a str,
        index: &'a RuleIndex,
        stored: &'a [StoredRule],
    ) -> EligibilityInput<'a> {
        static EMPTY: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
        let empty = EMPTY.get_or_init(BTreeMap::new);
        EligibilityInput {
            operation,
            rule,
            category,
            scope,
            scope_dimensions_raw: empty,
            record_class: "standing_preference",
            origin,
            evidence_text,
            declared_authority_effect: None,
            policy_bans: &[],
            index,
            stored_rules: stored,
        }
    }

    #[test]
    fn admits_a_clean_candidate() {
        let idx = RuleIndex::default();
        let d = evaluate_eligibility(&base_input(
            "add",
            "Always run the focused test before claiming verified",
            "verification",
            "repo-x",
            authority::Origin::UserTurn,
            "user said it in chat",
            &idx,
            &[],
        ));
        assert!(d.is_admitted());
    }

    #[test]
    fn refuses_unknown_categories_and_bad_shapes() {
        let idx = RuleIndex::default();
        assert_eq!(
            evaluate_eligibility(&base_input("add", "Always do the thing properly ok", "branding", "s", authority::Origin::UserTurn, "ev", &idx, &[])),
            EligibilityDecision::Refused { reason: "category-not-allowed".into() }
        );
        assert_eq!(
            evaluate_eligibility(&base_input("add", "hi", "workflow", "s", authority::Origin::UserTurn, "ev", &idx, &[])),
            EligibilityDecision::Refused { reason: "rule-invalid-shape".into() }
        );
    }

    #[test]
    fn refuses_duplicates_via_scoped_identity() {
        let mut idx = RuleIndex::default();
        idx.insert(RuleKey::new("repo-x", "duplicate-id"));
        assert_eq!(
            evaluate_eligibility(&base_input("add", "duplicate-id", "workflow", "repo-x", authority::Origin::UserTurn, "ev text here", &idx, &[])),
            EligibilityDecision::Refused { reason: "rule-duplicate".into() }
        );
    }

    #[test]
    fn refuses_non_user_origin() {
        let idx = RuleIndex::default();
        let d = evaluate_eligibility(&base_input(
            "add",
            "Always run focused tests first",
            "verification",
            "repo-x",
            authority::Origin::AssistantOutput,
            "I'll always run focused tests first",
            &idx,
            &[],
        ));
        assert_eq!(
            d,
            EligibilityDecision::Refused { reason: "origin-not-user:assistant_output".into() }
        );
    }

    #[test]
    fn refuses_malformed_scope_fail_closed() {
        let mut dims = BTreeMap::new();
        dims.insert("colour".to_string(), "red".to_string());
        let index = RuleIndex::default();
        let stored: Vec<StoredRule> = vec![];
        let input = EligibilityInput {
            scope_dimensions_raw: &dims,
            ..base_input("add", "Always run focused tests first ok", "verification", "repo-x", authority::Origin::UserTurn, "user said", &index, &stored)
        };
        assert_eq!(
            evaluate_eligibility(&input),
            EligibilityDecision::Refused { reason: "scope-malformed".into() }
        );
    }

    #[test]
    fn refuses_contradictions_with_stored_rules() {
        let stored = vec![StoredRule {
            id: "r1".into(),
            rule: "Never squash commits".into(),
            scope: "workspace".into(),
            lifecycle_state: "active".into(),
        }];
        let idx = RuleIndex::default();
        let d = evaluate_eligibility(&base_input(
            "add",
            "Always squash commits before merging",
            "workflow",
            "repo-x",
            authority::Origin::UserTurn,
            "user said squash",
            &idx,
            &stored,
        ));
        assert_eq!(
            d,
            EligibilityDecision::Refused { reason: "rule-conflict-needs-review".into() }
        );
    }

    #[test]
    fn model_proposals_require_bound_user_evidence() {
        let proposal = ModelExtractionProposal {
            proposer_id: "m1".into(),
            rule_text: "Always run focused tests first".into(),
            category_hint: "verification".into(),
            scope_hint: "repo-x".into(),
            bound_evidence_ids: vec![],
            bound_evidence_excerpt: "always run focused tests first".into(),
        };
        let err = evaluate_model_proposal(&proposal, false, &RuleIndex::default(), &[]).unwrap_err();
        assert_eq!(err, ModelProposalError::UnboundEvidence);
    }
}
