//! Gate 1 — Adapt proposal eligibility.
//!
//! Decides whether evidence may form a Taste candidate. Single-source
//! admission policy: category taxonomy, rule shape, duplicate detection via
//! scoped identity, deterministic safety quarantine (origin, authority
//! effects, contradictions), and versioned policy bans. Passing this gate
//! grants no authority at the Cortex or context gates.

use crate::authority::{self, AuthorityEffect, AuthorityResult, StoredRule};
use crate::canonical::{canonical_object, normalize_text, sha256_canonical, sha256_hex};
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

pub const GATE1_POLICY_CONTRACT: &str = "adapt.gate1-policy.v1";
pub const GATE1_EXECUTABLE_POLICY_FINGERPRINT_CONTRACT: &str =
    "adapt.gate1-executable-policy-fingerprint.v1";

/// Native policy bans are defined beside the eligibility function that
/// executes them, so the executable-policy fingerprint cannot omit a second
/// policy source maintained elsewhere.
pub const NATIVE_GATE1_POLICY_BANS: &[(&str, &str)] = &[
    (
        "policy-banned:instruction-precedence",
        r"(?i)\b(?:ignore|bypass|override)\b.{0,48}\b(?:system instructions?|policy|safety rules?)\b",
    ),
    (
        "policy-banned:secret-exfiltration",
        r"(?i)\b(?:exfiltrate|leak|upload|publish)\b.{0,48}\b(?:secrets?|credentials?|api[- ]?keys?|tokens?)\b",
    ),
];

/// Compile the exact ban definitions covered by the executable-policy
/// fingerprint. Callers must not construct an independent Gate 1 ban list.
pub fn native_gate1_policy_bans() -> Vec<(String, regex::Regex)> {
    compile_gate1_policy_bans(NATIVE_GATE1_POLICY_BANS)
}

pub(crate) fn compile_gate1_policy_bans(
    definitions: &[(&str, &str)],
) -> Vec<(String, regex::Regex)> {
    definitions
        .iter()
        .map(|(reason, pattern)| {
            (
                (*reason).to_string(),
                regex::Regex::new(pattern).expect("native Gate 1 policy regex compiles"),
            )
        })
        .collect()
}

/// Fingerprint the complete source surface that can alter Gate 1 eligibility.
///
/// This deliberately hashes the executable Rust sources instead of copying a
/// parallel policy manifest that could drift. The admission coordinator covers
/// action order, rule-shape rules and bans; its dependencies cover taxonomy,
/// class/identity, scope normalization, origin/effect classification,
/// contradiction semantics, candidate integrity, manifest validation, and the
/// canonical text operations those rules execute. Line endings are normalized
/// so equivalent checkouts agree.
pub fn executable_gate1_policy_sha256() -> String {
    executable_gate1_policy_sha256_for_sources(&[
        ("admission.rs", include_str!("admission.rs")),
        ("authority.rs", include_str!("authority.rs")),
        ("canonical.rs", include_str!("canonical.rs")),
        ("manifest.rs", include_str!("manifest.rs")),
        ("proposal.rs", include_str!("proposal.rs")),
        ("record.rs", include_str!("record.rs")),
        ("scope.rs", include_str!("scope.rs")),
        ("taste.rs", include_str!("taste.rs")),
    ])
}

fn executable_gate1_policy_sha256_for_sources(sources: &[(&str, &str)]) -> String {
    let mut source_digests = sources
        .iter()
        .map(|(name, source)| {
            let normalized = source.replace("\r\n", "\n");
            (
                (*name).to_string(),
                canonical_object([
                    ("name", serde_json::Value::String((*name).to_string())),
                    (
                        "sha256",
                        serde_json::Value::String(sha256_hex(normalized.as_bytes())),
                    ),
                ]),
            )
        })
        .collect::<Vec<_>>();
    source_digests.sort_by(|left, right| left.0.cmp(&right.0));
    sha256_canonical(&canonical_object([
        (
            "contract",
            serde_json::Value::String(GATE1_EXECUTABLE_POLICY_FINGERPRINT_CONTRACT.into()),
        ),
        (
            "sources",
            serde_json::Value::Array(
                source_digests
                    .into_iter()
                    .map(|(_, digest)| digest)
                    .collect(),
            ),
        ),
    ]))
}

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
    if format!(" {normalized} ").contains(" should ")
        || format!(" {normalized} ").contains(" must ")
    {
        return true;
    }
    if normalized.starts_with("when ") {
        return true;
    }
    body.trim().ends_with(['.', '!', '?'])
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

    // The declared effect is transport metadata, not authority. Recompute it
    // from the proposed rule itself, refuse mismatches, and quarantine effects
    // that would expand permission or weaken security. Evidence bytes decide
    // origin authority; a benign excerpt must not launder an unsafe proposal.
    let rule_effect = authority::classify_authority_effect(body);
    let effect_name = match rule_effect {
        AuthorityEffect::Neutral => "neutral",
        AuthorityEffect::Restrictive => "restrictive",
        AuthorityEffect::PermissionExpanding => "permission-expanding",
        AuthorityEffect::SecurityWeakening => "security-weakening",
    };
    if let Some(declared) = input.declared_authority_effect {
        let declared = declared.trim().to_lowercase().replace('_', "-");
        if declared != effect_name {
            return refused(format!(
                "authority-effect-mismatch:{declared}:{effect_name}"
            ));
        }
    }
    match rule_effect {
        AuthorityEffect::PermissionExpanding | AuthorityEffect::SecurityWeakening => {
            return refused(format!("authority-effect:{effect_name}"));
        }
        AuthorityEffect::Neutral | AuthorityEffect::Restrictive => {}
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
    evidence: &[membrane_transcript::evidence::VerifiedUserActEvidence],
    index: &RuleIndex,
    stored_rules: &[StoredRule],
) -> Result<EligibilityDecision, ModelProposalError> {
    let expected = crate::canonical::sha256_hex(proposal.bound_evidence_excerpt.as_bytes());
    let all_bound = !proposal.bound_evidence_ids.is_empty() && proposal.bound_evidence_ids.iter().all(|id| {
        evidence.iter().any(|verified| verified.evidence().evidence_id == *id
            && membrane_transcript::user_act::authoritative_excerpt(verified)
                .is_some_and(|excerpt| crate::canonical::sha256_hex(excerpt.as_bytes()) == expected))
    });
    if !all_bound {
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

    #[test]
    fn executable_policy_fingerprint_binds_non_ban_policy_sources() {
        let base = executable_gate1_policy_sha256_for_sources(&[
            (
                "admission.rs",
                "const MIN_RULE_CHARS: usize = 15; const ACTIONS: &[&str] = &[\"add\"];",
            ),
            (
                "record.rs",
                "const ALLOWED_CATEGORIES: &[&str] = &[\"workflow\"]; enum RecordClass { StandingPreference }",
            ),
            ("scope.rs", "const SCOPE_DIMENSION_KEYS: &[&str] = &[\"repo\"];"),
            (
                "authority.rs",
                "fn evaluate_origin() { user_only(); } fn classify_authority_effect() { safe_first(); } fn detect_rule_contradictions() { live_only(); }",
            ),
        ]);
        for (name, changed_source) in [
            (
                "rule-shape",
                "const MIN_RULE_CHARS: usize = 16; const ACTIONS: &[&str] = &[\"add\"];",
            ),
            (
                "actions",
                "const MIN_RULE_CHARS: usize = 15; const ACTIONS: &[&str] = &[\"add\", \"replace\"];",
            ),
            (
                "taxonomy",
                "const ALLOWED_CATEGORIES: &[&str] = &[\"workflow\", \"safety\"]; enum RecordClass { StandingPreference }",
            ),
            (
                "class",
                "const ALLOWED_CATEGORIES: &[&str] = &[\"workflow\"]; enum RecordClass { StandingPreference, EpisodicFact }",
            ),
            (
                "scope",
                "const SCOPE_DIMENSION_KEYS: &[&str] = &[\"repo\", \"branch\"];",
            ),
            (
                "origin",
                "fn evaluate_origin() { allow_tool(); } fn classify_authority_effect() { safe_first(); } fn detect_rule_contradictions() { live_only(); }",
            ),
            (
                "effect",
                "fn evaluate_origin() { user_only(); } fn classify_authority_effect() { permissive_first(); } fn detect_rule_contradictions() { live_only(); }",
            ),
            (
                "contradiction",
                "fn evaluate_origin() { user_only(); } fn classify_authority_effect() { safe_first(); } fn detect_rule_contradictions() { include_retired(); }",
            ),
        ] {
            let source_name = match name {
                "rule-shape" | "actions" => "admission.rs",
                "taxonomy" | "class" => "record.rs",
                "scope" => "scope.rs",
                "origin" | "effect" | "contradiction" => "authority.rs",
                _ => unreachable!(),
            };
            let mut changed = vec![
                (
                    "admission.rs",
                    "const MIN_RULE_CHARS: usize = 15; const ACTIONS: &[&str] = &[\"add\"];",
                ),
                (
                    "record.rs",
                    "const ALLOWED_CATEGORIES: &[&str] = &[\"workflow\"]; enum RecordClass { StandingPreference }",
                ),
                ("scope.rs", "const SCOPE_DIMENSION_KEYS: &[&str] = &[\"repo\"];"),
                (
                    "authority.rs",
                    "fn evaluate_origin() { user_only(); } fn classify_authority_effect() { safe_first(); } fn detect_rule_contradictions() { live_only(); }",
                ),
            ];
            changed
                .iter_mut()
                .find(|(source, _)| *source == source_name)
                .unwrap()
                .1 = changed_source;
            assert_ne!(
                base,
                executable_gate1_policy_sha256_for_sources(&changed),
                "fingerprint omitted {name} policy"
            );
        }
    }

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
    fn benign_evidence_cannot_launder_an_unsafe_rule() {
        let idx = RuleIndex::default();
        let d = evaluate_eligibility(&base_input(
            "add",
            "Never validate TLS certificates",
            "safety",
            "repo-x",
            authority::Origin::UserTurn,
            "The user requested secure network defaults.",
            &idx,
            &[],
        ));
        assert_eq!(
            d,
            EligibilityDecision::Refused {
                reason: "authority-effect:security-weakening".into()
            }
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
        let err = evaluate_model_proposal(&proposal, &[], &RuleIndex::default(), &[]).unwrap_err();
        assert_eq!(err, ModelProposalError::UnboundEvidence);
    }
}
