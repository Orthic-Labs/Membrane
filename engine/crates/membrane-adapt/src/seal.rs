//! Semantic/applicability sealing (canon §7.3).
//!
//! Immutable semantic payload — every field that changes meaning or
//! applicability — is hashed into a `seal_digest`. Mutable state (lifecycle,
//! verification counters, observations) lives in a separate envelope and may
//! change only through receipted transition events. Any post-emission mutation
//! of a sealed field changes the digest and is rejected.

use serde::{Deserialize, Serialize};

use crate::authority::{AuthorityEffect, PrecedenceTier};
use crate::canonical::{canonical_object, sha256_canonical};
use crate::record::{InfluenceClass, RecordClass};
use crate::scope::ScopeDimensions;

pub const SEAL_CONTRACT_VERSION: &str = "adapt.semantic-seal.v1";
pub const ADMISSION_POLICY_VERSION: &str = "adapt.admission.v1";
pub const REDACTION_CONTRACT_VERSION: &str = "membrane.redaction.v1";
pub const PROVENANCE_CONTRACT_VERSION: &str = "adapt.provenance.v2";

/// The immutable semantic payload. Hashing this produces the seal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticPayloadV1 {
    /// Contract version of the sealing scheme itself.
    pub seal_contract_version: String,
    /// Semantic record kind (e.g. `preference`, `insight_issue`).
    pub record_kind: String,
    /// Category / detector family.
    pub category: String,
    /// Canonical text/description (normalized for hashing).
    pub canonical_text: String,
    pub scope: String,
    pub scope_dimensions: ScopeDimensions,
    /// Authority class tier carried by the sealed semantics.
    pub authority_tier: PrecedenceTier,
    pub authority_effect: AuthorityEffect,
    pub influence_class: InfluenceClass,
    pub record_class: Option<RecordClass>,
    /// Machine/client/model applicability where it is semantically binding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_binding: Option<String>,
    /// Sorted digests of the source evidence objects.
    pub source_evidence_digests: Vec<String>,
    /// Binding to the canonical pool/version the semantics were mined under.
    pub canonical_pool_sha256: String,
    pub admission_policy_version: String,
    /// Held-out semantic-validator receipt identity.
    pub validator_receipt_id: String,
    pub validator_receipt_sha256: String,
    pub redaction_contract_version: String,
    pub provenance_contract_version: String,
}

impl SemanticPayloadV1 {
    /// Compute the seal digest over the canonical JSON form.
    pub fn seal_digest(&self) -> String {
        let value = serde_json::to_value(self).expect("payload serializes");
        sha256_canonical(&value)
    }
}

/// A receipted mutable-state change that never touches sealed semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeMutation {
    pub kind: EnvelopeMutationKind,
    pub target_id: String,
    pub expected_seal_digest: String,
    pub receipt_sha256: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeMutationKind {
    LifecycleTransition,
    VerificationStamp,
    ObservationAppend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealError {
    DigestMismatch { expected: String, found: String },
    MutationTouchesSealedField { field: &'static str },
}

/// Verify a stored payload against its recorded seal digest.
pub fn verify_seal(payload: &SemanticPayloadV1, recorded_digest: &str) -> Result<(), SealError> {
    let found = payload.seal_digest();
    if found != recorded_digest {
        return Err(SealError::DigestMismatch {
            expected: recorded_digest.to_string(),
            found,
        });
    }
    Ok(())
}

/// Validate an envelope mutation: the envelope may only mutate when the
/// caller's view of the sealed payload still matches the recorded seal.
pub fn validate_envelope_mutation(
    payload: &SemanticPayloadV1,
    recorded_digest: &str,
    mutation: &EnvelopeMutation,
) -> Result<(), SealError> {
    verify_seal(payload, recorded_digest)?;
    if mutation.expected_seal_digest != recorded_digest {
        return Err(SealError::DigestMismatch {
            expected: recorded_digest.to_string(),
            found: mutation.expected_seal_digest.clone(),
        });
    }
    if mutation.receipt_sha256.trim().is_empty() {
        return Err(SealError::MutationTouchesSealedField {
            field: "receipt_sha256",
        });
    }
    Ok(())
}

/// Deterministic manifest-level seal for an ordered batch of payloads:
/// binds each payload's digest plus the batch ordering itself.
pub fn batch_seal(payloads: &[&SemanticPayloadV1]) -> String {
    let digests: Vec<serde_json::Value> = payloads
        .iter()
        .map(|p| serde_json::Value::String(p.seal_digest()))
        .collect();
    let value = canonical_object([
        (
            "seal_contract_version",
            serde_json::Value::String(SEAL_CONTRACT_VERSION.into()),
        ),
        ("digests", serde_json::Value::Array(digests)),
    ]);
    sha256_canonical(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn payload(text: &str) -> SemanticPayloadV1 {
        SemanticPayloadV1 {
            seal_contract_version: SEAL_CONTRACT_VERSION.into(),
            record_kind: "preference".into(),
            category: "workflow".into(),
            canonical_text: crate::canonical::normalize_text(text),
            scope: "repo-x".into(),
            scope_dimensions: ScopeDimensions::normalize(&BTreeMap::new()).unwrap(),
            authority_tier: PrecedenceTier::ExplicitScopedUserPreference,
            authority_effect: AuthorityEffect::Neutral,
            influence_class: InfluenceClass::Provisional,
            record_class: Some(RecordClass::StandingPreference),
            machine_binding: None,
            source_evidence_digests: vec!["d1".into()],
            canonical_pool_sha256: "pool".into(),
            admission_policy_version: "1".into(),
            validator_receipt_id: "vr-1".into(),
            validator_receipt_sha256: "vrd-1".into(),
            redaction_contract_version: "r1".into(),
            provenance_contract_version: "p1".into(),
        }
    }

    #[test]
    fn seal_detects_semantic_mutation() {
        let p = payload("Always run focused tests");
        let digest = p.seal_digest();
        assert!(verify_seal(&p, &digest).is_ok());
        let mut mutations = Vec::new();
        macro_rules! mutated {
            ($field:ident, $value:expr) => {{
                let mut item = p.clone();
                item.$field = $value;
                mutations.push((stringify!($field), item));
            }};
        }
        mutated!(seal_contract_version, "other".into());
        mutated!(record_kind, "insight".into());
        mutated!(category, "style".into());
        mutated!(canonical_text, "never run focused tests".into());
        mutated!(scope, "*".into());
        let mut dims = BTreeMap::new();
        dims.insert("repo".into(), "other".into());
        mutated!(scope_dimensions, ScopeDimensions::normalize(&dims).unwrap());
        mutated!(authority_tier, PrecedenceTier::ExplicitGlobalUserPreference);
        mutated!(authority_effect, AuthorityEffect::Restrictive);
        mutated!(influence_class, InfluenceClass::BehavioralDirective);
        mutated!(record_class, Some(RecordClass::ScopedPreference));
        mutated!(machine_binding, Some("other-host".into()));
        mutated!(source_evidence_digests, vec!["d2".into()]);
        mutated!(canonical_pool_sha256, "other-pool".into());
        mutated!(admission_policy_version, "other".into());
        mutated!(validator_receipt_id, "vr-2".into());
        mutated!(validator_receipt_sha256, "vrd-2".into());
        mutated!(redaction_contract_version, "r2".into());
        mutated!(provenance_contract_version, "p2".into());
        for (field, tampered) in mutations {
            assert!(
                verify_seal(&tampered, &digest).is_err(),
                "field {field} was not sealed"
            );
        }
    }

    #[test]
    fn envelope_mutation_requires_matching_digest_and_receipt() {
        let p = payload("rule one two three four five");
        let digest = p.seal_digest();
        let good = EnvelopeMutation {
            kind: EnvelopeMutationKind::LifecycleTransition,
            target_id: "x".into(),
            expected_seal_digest: digest.clone(),
            receipt_sha256: "abc".into(),
            timestamp: "t".into(),
        };
        assert!(validate_envelope_mutation(&p, &digest, &good).is_ok());
        let bad_digest = EnvelopeMutation {
            expected_seal_digest: "stale".into(),
            ..good
        };
        assert!(validate_envelope_mutation(&p, &digest, &bad_digest).is_err());
    }

    #[test]
    fn batch_seal_is_order_sensitive_and_stable() {
        let a = payload("alpha rule text here");
        let b = payload("beta rule text here");
        assert_eq!(batch_seal(&[&a, &b]), batch_seal(&[&a, &b]));
        assert_ne!(batch_seal(&[&a, &b]), batch_seal(&[&b, &a]));
    }
}
