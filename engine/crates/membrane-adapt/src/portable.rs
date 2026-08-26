//! Signed, evidence-preserving Taste export/import & team/org packaging.

use ring::signature::{self, KeyPair};
use serde::{Deserialize, Serialize};

use crate::authority::PrecedenceTier;
use crate::record::{LifecycleTransitionEvent, PreferenceRecordV1};
use crate::seal::{verify_seal, SemanticPayloadV1};

pub const PORTABLE_TASTE_SCHEMA: &str = "adapt.portable-taste.v1";
const MAX_RECORDS: usize = 1_000;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(value: &str) -> Result<Vec<u8>, PortableTasteError> {
    if value.len() % 2 != 0 {
        return Err(PortableTasteError::SignatureInvalid);
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| PortableTasteError::SignatureInvalid)
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableSignatureV1 {
    pub algorithm: String,
    pub key_id: String,
    pub public_key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableTasteRecordV1 {
    pub record: PreferenceRecordV1,
    pub semantic_payload: SemanticPayloadV1,
    pub evidence_digests: Vec<String>,
    pub provenance_receipts: Vec<String>,
    pub lifecycle_history: Vec<LifecycleTransitionEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableTastePackageV1 {
    pub schema_version: String,
    pub package_id: String,
    pub origin_installation_id: String,
    pub origin_org_id: Option<String>,
    pub exported_at: String,
    pub records: Vec<PortableTasteRecordV1>,
    pub signature: PortableSignatureV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortableTasteError {
    InvalidSchema,
    TooManyRecords,
    MissingSemanticSeal { record_id: String },
    SemanticMismatch { record_id: String },
    MissingEvidence { record_id: String },
    LifecycleBinding { record_id: String },
    SignatureInvalid,
    UntrustedKey,
    IdentityMismatch,
}

impl PortableTastePackageV1 {
    fn unsigned_value(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self).expect("portable package serializes");
        value
            .as_object_mut()
            .expect("portable package is object")
            .remove("signature");
        value
    }

    fn unsigned_bytes(&self) -> Vec<u8> {
        crate::canonical::to_canonical_json(&self.unsigned_value()).into_bytes()
    }

    fn identity_value(&self) -> serde_json::Value {
        let mut value = self.unsigned_value();
        value
            .as_object_mut()
            .expect("portable package is object")
            .remove("package_id");
        value
    }

    pub fn build(
        origin_installation_id: &str,
        origin_org_id: Option<&str>,
        exported_at: &str,
        records: Vec<PortableTasteRecordV1>,
    ) -> Result<Self, PortableTasteError> {
        let mut package = Self {
            schema_version: PORTABLE_TASTE_SCHEMA.into(),
            package_id: String::new(),
            origin_installation_id: origin_installation_id.into(),
            origin_org_id: origin_org_id.map(str::to_string),
            exported_at: exported_at.into(),
            records,
            signature: PortableSignatureV1 {
                algorithm: "Ed25519-v1".into(),
                key_id: String::new(),
                public_key: String::new(),
                value: String::new(),
            },
        };
        package.validate_content()?;
        package.package_id = format!(
            "ptp_{}",
            crate::canonical::sha256_canonical(&package.identity_value())
        );
        Ok(package)
    }

    fn validate_content(&self) -> Result<(), PortableTasteError> {
        if self.schema_version != PORTABLE_TASTE_SCHEMA {
            return Err(PortableTasteError::InvalidSchema);
        }
        if self.records.len() > MAX_RECORDS {
            return Err(PortableTasteError::TooManyRecords);
        }
        for item in &self.records {
            if item.record.semantic_digest.len() != 64
                || verify_seal(&item.semantic_payload, &item.record.semantic_digest).is_err()
            {
                return Err(PortableTasteError::MissingSemanticSeal {
                    record_id: item.record.id.clone(),
                });
            }
            let expected_machine = item
                .record
                .machine_only
                .then(|| item.record.machine.clone());
            if item.semantic_payload.record_kind != item.record.kind
                || item.semantic_payload.category != item.record.category
                || item.semantic_payload.canonical_text
                    != crate::canonical::normalize_text(&item.record.rule)
                || item.semantic_payload.scope != item.record.scope
                || item.semantic_payload.scope_dimensions != item.record.scope_dimensions
                || item.semantic_payload.authority_effect != item.record.authority_effect
                || item.semantic_payload.influence_class != item.record.influence_class
                || item.semantic_payload.record_class != Some(item.record.class)
                || item.semantic_payload.machine_binding != expected_machine
            {
                return Err(PortableTasteError::SemanticMismatch {
                    record_id: item.record.id.clone(),
                });
            }
            if item.evidence_digests.is_empty() || item.provenance_receipts.is_empty() {
                return Err(PortableTasteError::MissingEvidence {
                    record_id: item.record.id.clone(),
                });
            }
            if item
                .lifecycle_history
                .iter()
                .any(|event| event.record_id != item.record.id)
            {
                return Err(PortableTasteError::LifecycleBinding {
                    record_id: item.record.id.clone(),
                });
            }
        }
        Ok(())
    }

    pub fn sign(&mut self, key_id: &str, seed: &[u8]) -> Result<(), PortableTasteError> {
        if seed.len() != 32 {
            return Err(PortableTasteError::SignatureInvalid);
        }
        let keypair = signature::Ed25519KeyPair::from_seed_unchecked(seed)
            .map_err(|_| PortableTasteError::SignatureInvalid)?;
        self.signature = PortableSignatureV1 {
            algorithm: "Ed25519-v1".into(),
            key_id: key_id.into(),
            public_key: hex(keypair.public_key().as_ref()),
            value: hex(keypair.sign(&self.unsigned_bytes()).as_ref()),
        };
        Ok(())
    }

    pub fn verify(&self, trusted_public_key: &[u8]) -> Result<(), PortableTasteError> {
        self.validate_content()?;
        let expected_id = format!(
            "ptp_{}",
            crate::canonical::sha256_canonical(&self.identity_value())
        );
        if self.package_id != expected_id {
            return Err(PortableTasteError::IdentityMismatch);
        }
        if self.signature.algorithm != "Ed25519-v1" {
            return Err(PortableTasteError::SignatureInvalid);
        }
        if hex(trusted_public_key) != self.signature.public_key {
            return Err(PortableTasteError::UntrustedKey);
        }
        let signature = unhex(&self.signature.value)?;
        signature::UnparsedPublicKey::new(&signature::ED25519, trusted_public_key)
            .verify(&self.unsigned_bytes(), &signature)
            .map_err(|_| PortableTasteError::SignatureInvalid)
    }

    /// Verified imports remain lower-precedence candidates. Local explicit
    /// preference can override them; import never activates or promotes.
    pub fn import_candidates(
        &self,
        trusted_public_key: &[u8],
    ) -> Result<Vec<ImportedPreferenceV1>, PortableTasteError> {
        self.verify(trusted_public_key)?;
        Ok(self
            .records
            .iter()
            .map(|item| ImportedPreferenceV1 {
                record: item.record.clone(),
                precedence: PrecedenceTier::TrustedImportedPreference,
                source_package_id: self.package_id.clone(),
                requires_local_promotion: true,
            })
            .collect())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedPreferenceV1 {
    pub record: PreferenceRecordV1,
    pub precedence: PrecedenceTier,
    pub source_package_id: String,
    pub requires_local_promotion: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{PreferenceSealContext, RecordClass};
    use crate::scope::ScopeDimensions;

    fn package() -> PortableTastePackageV1 {
        let mut record = PreferenceRecordV1::new_candidate(
            "Prefer focused patches",
            "workflow",
            RecordClass::StandingPreference,
            "repo",
            ScopeDimensions::default(),
            1.0,
            vec!["e".into()],
            "t",
        )
        .unwrap();
        let context = PreferenceSealContext {
            authority_tier: PrecedenceTier::ExplicitScopedUserPreference,
            canonical_pool_sha256: "pool",
            admission_policy_version: "admission-v1",
            validator_receipt_id: "validator-1",
            validator_receipt_sha256: "a",
            redaction_contract_version: "redaction-v1",
        };
        record.seal_semantics(&context);
        let semantic_payload = record.semantic_payload(&context);
        PortableTastePackageV1::build(
            "inst",
            Some("org"),
            "t",
            vec![PortableTasteRecordV1 {
                record,
                semantic_payload,
                evidence_digests: vec!["sha256:e".into()],
                provenance_receipts: vec!["sha256:p".into()],
                lifecycle_history: vec![],
            }],
        )
        .unwrap()
    }

    #[test]
    fn signed_import_is_lower_precedence_and_reviewed() {
        let seed = [7u8; 32];
        let keypair = signature::Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let mut package = package();
        package.sign("k", &seed).unwrap();
        let imported = package
            .import_candidates(keypair.public_key().as_ref())
            .unwrap();
        assert_eq!(
            imported[0].precedence,
            PrecedenceTier::TrustedImportedPreference
        );
        assert!(imported[0].requires_local_promotion);
    }

    #[test]
    fn tampering_breaks_signature() {
        let seed = [8u8; 32];
        let keypair = signature::Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let mut package = package();
        package.sign("k", &seed).unwrap();
        package.records[0].record.rule = "tampered".into();
        assert_eq!(
            package.verify(keypair.public_key().as_ref()),
            Err(PortableTasteError::SemanticMismatch {
                record_id: package.records[0].record.id.clone(),
            })
        );
    }

    #[test]
    fn package_identity_is_content_bound() {
        let seed = [9u8; 32];
        let keypair = signature::Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let mut package = package();
        package.package_id = format!("ptp_{}", "f".repeat(64));
        package.sign("k", &seed).unwrap();
        assert_eq!(
            package.verify(keypair.public_key().as_ref()),
            Err(PortableTasteError::IdentityMismatch)
        );
    }

    #[test]
    fn forged_semantic_digest_is_refused_before_export() {
        let mut package = package();
        package.records[0].record.semantic_digest = "a".repeat(64);
        assert!(matches!(
            package.validate_content(),
            Err(PortableTasteError::MissingSemanticSeal { .. })
        ));
    }
}
