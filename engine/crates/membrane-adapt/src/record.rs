//! `PreferenceRecordV1` — canonical Taste record with lifecycle semantics.

use serde::{Deserialize, Serialize};

use crate::authority::{self, AuthorityEffect, StoredRule};
use crate::canonical::derive_preference_id;
use crate::scope::ScopeDimensions;
use crate::seal::{verify_seal, SemanticPayloadV1, SealError, SEAL_CONTRACT_VERSION};
use crate::authority::PrecedenceTier;

pub const PREFERENCE_RECORD_SCHEMA: &str = "adapt.preference-record.v1";
pub const KIND: &str = "preference";

/// Canonical Taste semantic classes (canon §5.2). `episodic_fact` is NOT a
/// Taste class and must be rejected from the Taste lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordClass {
    StandingPreference,
    ScopedPreference,
    OperationalPlaybook,
    ExplicitBehavioralDecision,
}

impl RecordClass {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "standing_preference" => Some(Self::StandingPreference),
            "scoped_preference" => Some(Self::ScopedPreference),
            "operational_playbook" | "operational_playbook_v2" => Some(Self::OperationalPlaybook),
            "explicit_behavioral_decision" => Some(Self::ExplicitBehavioralDecision),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::StandingPreference => "standing_preference",
            Self::ScopedPreference => "scoped_preference",
            Self::OperationalPlaybook => "operational_playbook",
            Self::ExplicitBehavioralDecision => "explicit_behavioral_decision",
        }
    }
}

/// The eight controlled preference categories. Anything else is refused to a
/// review bucket; unknown categories never silently map to an active bucket.
pub const ALLOWED_CATEGORIES: &[&str] = &[
    "workflow",
    "verification",
    "safety",
    "architecture",
    "tooling",
    "code-style",
    "documentation",
    "model-routing",
];

pub fn normalize_category(raw: &str) -> Option<&'static str> {
    let lowered = raw.trim().to_lowercase();
    ALLOWED_CATEGORIES.iter().copied().find(|c| *c == lowered)
}

/// Lifecycle states (canon §5.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Candidate,
    Active,
    Disputed,
    Deprecated,
    Superseded,
    Retired,
}

impl LifecycleState {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "candidate" => Some(Self::Candidate),
            "active" => Some(Self::Active),
            "disputed" => Some(Self::Disputed),
            "deprecated" => Some(Self::Deprecated),
            "superseded" => Some(Self::Superseded),
            "retired" => Some(Self::Retired),
            _ => None,
        }
    }

    /// Legal transitions. Deliberately conservative: nothing leaves `Retired`;
    /// candidates may only become active or retire.
    pub fn legal_transitions(self) -> &'static [LifecycleState] {
        match self {
            Self::Candidate => &[Self::Active, Self::Retired],
            Self::Active => &[Self::Disputed, Self::Deprecated, Self::Superseded, Self::Retired],
            Self::Disputed => &[Self::Active, Self::Deprecated, Self::Retired],
            Self::Deprecated => &[Self::Retired, Self::Active],
            Self::Superseded => &[Self::Retired],
            Self::Retired => &[],
        }
    }

    pub fn can_transition_to(self, target: LifecycleState) -> bool {
        self.legal_transitions().contains(&target)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleError {
    UnknownState(String),
    MissingReceipt,
    IllegalTransition { from: LifecycleState, to: LifecycleState },
}

/// A receipted lifecycle transition event. Applying it is the only way
/// lifecycle state changes on a sealed record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleTransitionEvent {
    pub record_id: String,
    pub from_state: LifecycleState,
    pub to_state: LifecycleState,
    pub reason: String,
    /// SHA-256 of the receipt authorizing this transition (operator action,
    /// contradiction resolution, supersession binding, ...).
    pub receipt_sha256: String,
    pub timestamp: String,
}

/// Influence class carried toward Cortex (canon §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InfluenceClass {
    /// Active user-authoritative Taste; eligible for directive influence.
    BehavioralDirective,
    /// Inferred/provisional; weaker influence, review required.
    Provisional,
    /// Reference/diagnostic only (Insights default).
    ReferenceOnly,
}

/// Canonical scoped identity: `(scope, record_id)` — never a bare name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RuleKey {
    pub scope: String,
    pub record_id: String,
}

impl RuleKey {
    pub fn new(scope: &str, record_id: &str) -> Self {
        Self {
            scope: scope.to_string(),
            record_id: record_id.to_string(),
        }
    }

    pub fn formatted(&self) -> String {
        if self.scope.is_empty() {
            self.record_id.clone()
        } else {
            format!("{}/{}", self.scope, self.record_id)
        }
    }
}

/// Errors constructing or updating a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    EmptyRule,
    UnknownCategory(String),
    NotTasteClass(String),
    ScopeMalformed(crate::scope::ScopeError),
}

/// `PreferenceRecordV1`.
///
/// `semantic_digest` binds the immutable meaning/applicability payload (see
/// [`crate::seal`]); mutable state lives in the envelope around it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceRecordV1 {
    pub schema_version: String,
    pub id: String,
    pub kind: String,
    /// Original user-facing wording (identity uses normalized form).
    pub rule: String,
    pub category: String,
    pub class: RecordClass,
    pub scope: String,
    pub scope_dimensions: ScopeDimensions,
    pub confidence: f64,
    pub needs_review: bool,
    pub evidence_count: u32,
    /// Digests of the qualifying evidence objects backing this record.
    pub source_evidence_ids: Vec<String>,
    /// Counterfactual pair where evidence was a correction/edit/rejection:
    /// preferred vs avoided alternative.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avoid_alternative: Option<String>,
    pub authority_effect: AuthorityEffect,
    pub influence_class: InfluenceClass,
    pub machine: String,
    pub machine_only: bool,
    /// Mutable lifecycle envelope state; changes only through receipted
    /// transitions. The sealed semantic payload never includes it.
    pub lifecycle_state: LifecycleState,
    pub created_at: String,
    pub updated_at: String,
    pub last_verified_at: String,
    pub verification_count: u32,
    /// Digest of the sealed immutable semantic payload.
    pub semantic_digest: String,
}

/// External bindings needed to seal one Taste record's immutable semantics.
#[derive(Debug, Clone)]
pub struct PreferenceSealContext<'a> {
    pub authority_tier: PrecedenceTier,
    pub canonical_pool_sha256: &'a str,
    pub admission_policy_version: &'a str,
    pub validator_receipt_id: &'a str,
    pub validator_receipt_sha256: &'a str,
    pub redaction_contract_version: &'a str,
}

impl PreferenceRecordV1 {
    /// Materialize every meaning/applicability field into one canonical seal.
    pub fn semantic_payload(&self, context: &PreferenceSealContext<'_>) -> SemanticPayloadV1 {
        let mut evidence = self.source_evidence_ids.clone();
        evidence.sort();
        evidence.dedup();
        SemanticPayloadV1 {
            seal_contract_version: SEAL_CONTRACT_VERSION.into(),
            record_kind: self.kind.clone(),
            category: self.category.clone(),
            canonical_text: crate::canonical::normalize_text(&self.rule),
            scope: self.scope.clone(),
            scope_dimensions: self.scope_dimensions.clone(),
            authority_tier: context.authority_tier,
            authority_effect: self.authority_effect,
            influence_class: self.influence_class,
            record_class: Some(self.class),
            machine_binding: self.machine_only.then(|| self.machine.clone()),
            source_evidence_digests: evidence,
            canonical_pool_sha256: context.canonical_pool_sha256.into(),
            admission_policy_version: context.admission_policy_version.into(),
            validator_receipt_id: context.validator_receipt_id.into(),
            validator_receipt_sha256: context.validator_receipt_sha256.into(),
            redaction_contract_version: context.redaction_contract_version.into(),
        }
    }

    pub fn seal_semantics(&mut self, context: &PreferenceSealContext<'_>) {
        self.semantic_digest = self.semantic_payload(context).seal_digest();
    }

    pub fn verify_semantics(&self, context: &PreferenceSealContext<'_>) -> Result<(), SealError> {
        verify_seal(&self.semantic_payload(context), &self.semantic_digest)
    }

    /// Construct a new candidate record. Identity derives deterministically;
    /// authority effect is computed by deterministic classification, never
    /// taken from model output.
    pub fn new_candidate(
        rule: &str,
        category: &str,
        class: RecordClass,
        scope: &str,
        scope_dimensions: ScopeDimensions,
        confidence: f64,
        evidence_ids: Vec<String>,
        now: &str,
    ) -> Result<Self, RecordError> {
        let rule = rule.trim();
        if rule.is_empty() {
            return Err(RecordError::EmptyRule);
        }
        let Some(category) = normalize_category(category) else {
            return Err(RecordError::UnknownCategory(category.to_string()));
        };
        let authority_effect = authority::classify_authority_effect(rule);
        let id = derive_preference_id(scope, category, rule);
        Ok(Self {
            schema_version: PREFERENCE_RECORD_SCHEMA.to_string(),
            id,
            kind: KIND.to_string(),
            rule: rule.to_string(),
            category: category.to_string(),
            class,
            scope: scope.to_string(),
            scope_dimensions,
            confidence: confidence.clamp(0.0, 1.0),
            needs_review: confidence < 0.5,
            evidence_count: evidence_ids.len() as u32,
            source_evidence_ids: evidence_ids,
            avoid_alternative: None,
            authority_effect,
            influence_class: InfluenceClass::Provisional,
            machine: String::new(),
            machine_only: false,
            lifecycle_state: LifecycleState::Candidate,
            created_at: now.to_string(),
            updated_at: now.to_string(),
            last_verified_at: String::new(),
            verification_count: 0,
            semantic_digest: String::new(),
        })
    }

    /// Reject non-Taste semantic classes (`episodic_fact`, `unclassified`)
    /// from the Taste lane at construction time.
    pub fn require_taste_class(raw_class: &str) -> Result<RecordClass, RecordError> {
        RecordClass::parse(raw_class).ok_or_else(|| RecordError::NotTasteClass(raw_class.to_string()))
    }

    /// Apply a receipted lifecycle transition, enforcing legality. Returns the
    /// transition event on success; the caller persists it alongside the
    /// record's updated envelope.
    pub fn transition_lifecycle(
        &self,
        target: LifecycleState,
        reason: &str,
        receipt_sha256: &str,
        timestamp: &str,
    ) -> Result<(Self, LifecycleTransitionEvent), LifecycleError> {
        if receipt_sha256.trim().len() != 64 {
            return Err(LifecycleError::MissingReceipt);
        }
        let current = self.lifecycle_state;
        if current == target {
            return Err(LifecycleError::IllegalTransition { from: current, to: target });
        }
        if !current.can_transition_to(target) {
            return Err(LifecycleError::IllegalTransition { from: current, to: target });
        }
        let mut updated = self.clone();
        updated.lifecycle_state = target;
        updated.updated_at = timestamp.to_string();
        let event = LifecycleTransitionEvent {
            record_id: self.id.clone(),
            from_state: current,
            to_state: target,
            reason: reason.to_string(),
            receipt_sha256: receipt_sha256.to_string(),
            timestamp: timestamp.to_string(),
        };
        Ok((updated, event))
    }

    /// View this record as a stored rule for contradiction detection.
    pub fn as_stored_rule(&self, lifecycle_state: &str) -> StoredRule {
        StoredRule {
            id: self.id.clone(),
            rule: self.rule.clone(),
            scope: self.scope.clone(),
            lifecycle_state: lifecycle_state.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn candidate() -> PreferenceRecordV1 {
        PreferenceRecordV1::new_candidate(
            "Always run the focused test before claiming verified",
            "verification",
            RecordClass::StandingPreference,
            "repo-x",
            ScopeDimensions::default(),
            0.9,
            vec!["ev-1".into()],
            "2026-08-24T00:00:00Z",
        )
        .unwrap()
    }

    #[test]
    fn episodic_fact_is_rejected_from_taste_lane() {
        assert!(matches!(
            PreferenceRecordV1::require_taste_class("episodic_fact"),
            Err(RecordError::NotTasteClass(_))
        ));
        assert!(PreferenceRecordV1::require_taste_class("standing_preference").is_ok());
    }

    #[test]
    fn unknown_category_is_refused_not_mapped() {
        assert_eq!(
            PreferenceRecordV1::new_candidate("rule text here ok", "branding", RecordClass::StandingPreference, "s", ScopeDimensions::default(), 0.9, vec![], "t")
                .unwrap_err(),
            RecordError::UnknownCategory("branding".into())
        );
    }

    #[test]
    fn illegal_transitions_are_refused() {
        let rec = candidate();
        // candidate -> disputed is illegal without first being active.
        assert!(matches!(
            rec.transition_lifecycle(LifecycleState::Disputed, "r", &"c".repeat(64), "t"),
            Err(LifecycleError::IllegalTransition { .. })
        ));
        // retired is terminal.
        assert!(!LifecycleState::Retired.can_transition_to(LifecycleState::Active));
        assert!(!LifecycleState::Superseded.can_transition_to(LifecycleState::Active));
    }

    #[test]
    fn legal_transition_produces_receipted_event() {
        let rec = candidate();
        let (updated, event) = rec
            .transition_lifecycle(LifecycleState::Active, "admitted", &"a".repeat(64), "t2")
            .unwrap();
        assert_eq!(event.from_state, LifecycleState::Candidate);
        assert_eq!(event.to_state, LifecycleState::Active);
        assert_eq!(updated.lifecycle_state, LifecycleState::Active);
    }

    #[test]
    fn ids_are_stable_for_identical_semantics() {
        let a = candidate();
        let b = candidate();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn security_weakening_rules_get_quarantined_effect() {
        let rec = PreferenceRecordV1::new_candidate(
            "Never validate TLS certificates",
            "safety",
            RecordClass::StandingPreference,
            "repo-x",
            ScopeDimensions::normalize(&BTreeMap::new()).unwrap(),
            0.99,
            vec![],
            "t",
        )
        .unwrap();
        assert_eq!(rec.authority_effect, AuthorityEffect::SecurityWeakening);
    }

    #[test]
    fn preference_semantic_seal_detects_scope_mutation() {
        let mut rec = candidate();
        let context = PreferenceSealContext {
            authority_tier: PrecedenceTier::ExplicitScopedUserPreference,
            canonical_pool_sha256: "pool",
            admission_policy_version: "v1",
            validator_receipt_id: "vr1",
            validator_receipt_sha256: "digest",
            redaction_contract_version: "r1",
        };
        rec.seal_semantics(&context);
        assert!(rec.verify_semantics(&context).is_ok());
        rec.scope = "global".into();
        assert!(rec.verify_semantics(&context).is_err());
    }
}
