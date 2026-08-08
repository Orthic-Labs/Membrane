//! Offline-verifiable, bounded context packs.
//!
//! The envelope signs canonical bytes with the existing SHA-256 primitive. The
//! key is supplied by the caller and never serialized; receipts remain
//! content-free (only hashes and resolver identifiers are carried).

use crate::{canonical_json_of, digest_str, ContextPacketV1, ScopeGrantV1};
use ring::signature::{self, KeyPair};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PORTABLE_PACK_SCHEMA_VERSION: u32 = 1;
const MAX_GRANTS: usize = 128;
const MAX_COMPONENTS: usize = 256;
const MAX_RESOLVERS: usize = 256;
const MAX_OMISSIONS: usize = 1024;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(s: &str) -> Result<Vec<u8>, PortablePackError> {
    if s.len() % 2 != 0 {
        return Err(PortablePackError::Signature);
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| PortablePackError::Signature))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortablePackSignatureV1 {
    pub algorithm: String,
    pub key_id: String,
    pub public_key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableContextPackV1 {
    pub schema_version: u32,
    pub packet: ContextPacketV1,
    pub grants: Vec<ScopeGrantV1>,
    pub source_generations: BTreeMap<String, String>,
    pub provider_versions: BTreeMap<String, String>,
    pub omissions: Vec<String>,
    pub resolvers: BTreeMap<String, String>,
    pub component_hashes: BTreeMap<String, String>,
    pub signature: PortablePackSignatureV1,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortablePackError {
    #[error("pack schema version is unsupported")]
    SchemaVersion,
    #[error("pack exceeds bounded {0} limit")]
    Bound(&'static str),
    #[error("component hash mismatch: {0}")]
    ComponentHash(String),
    #[error("source generation mismatch: {0}")]
    StaleGeneration(String),
    #[error("signature is invalid")]
    Signature,
    #[error("signature algorithm is unsupported")]
    Algorithm,
    #[error("invalid portable pack JSON")]
    Json,
    #[error("component manifest is incomplete or invalid: {0}")]
    Manifest(String),
}

impl PortableContextPackV1 {
    pub const SCHEMA_VERSION: u32 = PORTABLE_PACK_SCHEMA_VERSION;

    /// Bytes covered by signature. Signature itself is excluded by construction.
    fn unsigned_json(&self) -> String {
        let mut value = serde_json::to_value(self).expect("pack serializes");
        value
            .as_object_mut()
            .expect("pack is object")
            .remove("signature");
        crate::canonicalize(&value)
    }

    /// Sign with a 32-byte Ed25519 seed; public key is carried for key selection only.
    pub fn sign(&mut self, key_id: impl Into<String>, key: &[u8]) {
        let key_id = key_id.into();
        let kp = signature::Ed25519KeyPair::from_seed_unchecked(key)
            .expect("Ed25519 seed must be 32 bytes");
        let material = self.unsigned_json();
        self.signature = PortablePackSignatureV1 {
            algorithm: "Ed25519-v1".into(),
            key_id,
            public_key: hex(kp.public_key().as_ref()),
            value: hex(kp.sign(material.as_bytes()).as_ref()),
        };
    }

    pub fn from_json_bounded(input: &str) -> Result<Self, PortablePackError> {
        let value: serde_json::Value =
            serde_json::from_str(input).map_err(|_| PortablePackError::Json)?;
        let obj = value.as_object().ok_or(PortablePackError::Json)?;
        for (name, max) in [
            ("grants", MAX_GRANTS),
            ("componentHashes", MAX_COMPONENTS),
            ("resolvers", MAX_RESOLVERS),
            ("omissions", MAX_OMISSIONS),
        ] {
            if obj
                .get(name)
                .and_then(|v| v.as_array().map(|a| a.len()))
                .or_else(|| obj.get(name).and_then(|v| v.as_object().map(|o| o.len())))
                .unwrap_or(0)
                > max
            {
                return Err(PortablePackError::Bound(name));
            }
        }
        serde_json::from_value(value).map_err(|_| PortablePackError::Json)
    }

    pub fn verify(
        &self,
        seed: &[u8],
        expected_generations: &BTreeMap<String, String>,
    ) -> Result<(), PortablePackError> {
        let kp = signature::Ed25519KeyPair::from_seed_unchecked(seed)
            .map_err(|_| PortablePackError::Signature)?;
        self.verify_with_public_key(kp.public_key().as_ref(), expected_generations)
    }

    pub fn verify_with_public_key(
        &self,
        public_key: &[u8],
        expected_generations: &BTreeMap<String, String>,
    ) -> Result<(), PortablePackError> {
        if self.schema_version != Self::SCHEMA_VERSION {
            return Err(PortablePackError::SchemaVersion);
        }
        if self.grants.len() > MAX_GRANTS {
            return Err(PortablePackError::Bound("grants"));
        }
        if self.component_hashes.len() > MAX_COMPONENTS {
            return Err(PortablePackError::Bound("components"));
        }
        if self.resolvers.len() > MAX_RESOLVERS {
            return Err(PortablePackError::Bound("resolvers"));
        }
        if self.omissions.len() > MAX_OMISSIONS {
            return Err(PortablePackError::Bound("omissions"));
        }
        if self.signature.algorithm != "Ed25519-v1" {
            return Err(PortablePackError::Algorithm);
        }
        for (name, expected) in expected_generations {
            if self.source_generations.get(name) != Some(expected) {
                return Err(PortablePackError::StaleGeneration(name.clone()));
            }
        }
        let expected_components = [
            "packet",
            "grants",
            "sourceGenerations",
            "providerVersions",
            "omissions",
            "resolvers",
        ];
        if self
            .component_hashes
            .keys()
            .any(|k| !expected_components.contains(&k.as_str()))
            || self.component_hashes.len() != expected_components.len()
        {
            return Err(PortablePackError::Manifest("closed component set".into()));
        }
        let value = serde_json::to_value(&self.packet).expect("packet serializes");
        for (name, expected) in &self.component_hashes {
            let actual = match name.as_str() {
                "packet" => digest_str(&canonical_json_of(&value)),
                "grants" => digest_str(&canonical_json_of(&self.grants)),
                "sourceGenerations" => digest_str(&canonical_json_of(&self.source_generations)),
                "providerVersions" => digest_str(&canonical_json_of(&self.provider_versions)),
                "omissions" => digest_str(&canonical_json_of(&self.omissions)),
                "resolvers" => digest_str(&canonical_json_of(&self.resolvers)),
                _ => unreachable!(),
            };
            if &actual != expected {
                return Err(PortablePackError::ComponentHash(name.clone()));
            }
        }
        let trusted = hex(public_key);
        if trusted != self.signature.public_key {
            return Err(PortablePackError::Signature);
        }
        let pk = unhex(&self.signature.public_key)?;
        let sig = unhex(&self.signature.value)?;
        signature::UnparsedPublicKey::new(&signature::ED25519, pk)
            .verify(self.unsigned_json().as_bytes(), &sig)
            .map_err(|_| PortablePackError::Signature)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BudgetV1, ContextPacketV1};

    fn pack() -> PortableContextPackV1 {
        let packet = ContextPacketV1 {
            schema_version: 1,
            trace_id: "t".into(),
            task: "x".into(),
            mode: "m".into(),
            budget: BudgetV1 {
                max_tokens: 1,
                admitted_tokens: 0,
                packet_char_budget_default: None,
                packet_char_budget_override: None,
                packet_char_budget_model: None,
                configured_packet_char_budget: None,
                effective_packet_char_budget: None,
            },
            allocations: BTreeMap::new(),
            provider_accounting: BTreeMap::new(),
            blocks: vec![],
            omissions: vec![],
            reconciliation: None,
        };
        let mut p = PortableContextPackV1 {
            schema_version: 1,
            packet,
            grants: vec![],
            source_generations: [("repo".into(), "gen-1".into())].into_iter().collect(),
            provider_versions: BTreeMap::new(),
            omissions: vec!["no-content".into()],
            resolvers: BTreeMap::new(),
            component_hashes: BTreeMap::new(),
            signature: PortablePackSignatureV1 {
                algorithm: String::new(),
                key_id: String::new(),
                public_key: String::new(),
                value: String::new(),
            },
        };
        for (name, value) in [
            ("packet", canonical_json_of(&p.packet)),
            ("grants", canonical_json_of(&p.grants)),
            (
                "sourceGenerations",
                canonical_json_of(&p.source_generations),
            ),
            ("providerVersions", canonical_json_of(&p.provider_versions)),
            ("omissions", canonical_json_of(&p.omissions)),
            ("resolvers", canonical_json_of(&p.resolvers)),
        ] {
            p.component_hashes.insert(name.into(), digest_str(&value));
        }
        p.sign("k1", &[7; 32]);
        p
    }
    #[test]
    fn offline_verify_and_tamper_rejection() {
        let p = pack();
        assert!(p
            .verify(
                &[7; 32],
                &[("repo".into(), "gen-1".into())].into_iter().collect()
            )
            .is_ok());
        let mut bad = p.clone();
        bad.omissions.push("altered".into());
        assert_eq!(
            bad.verify(&[7; 32], &BTreeMap::new()),
            Err(PortablePackError::ComponentHash("omissions".into()))
        );
    }
    #[test]
    fn stale_generation_rejected() {
        let p = pack();
        assert_eq!(
            p.verify(
                &[7; 32],
                &[("repo".into(), "gen-2".into())].into_iter().collect()
            ),
            Err(PortablePackError::StaleGeneration("repo".into()))
        );
    }

    #[test]
    fn public_key_and_manifest_admission_are_enforced() {
        let p = pack();
        let pk = unhex(&p.signature.public_key).unwrap();
        assert!(p.verify_with_public_key(&pk, &BTreeMap::new()).is_ok());
        assert_eq!(
            p.verify_with_public_key(&[9; 32], &BTreeMap::new()),
            Err(PortablePackError::Signature)
        );
        let mut substituted = p.clone();
        substituted.signature.public_key = hex(&[9; 32]);
        assert_eq!(
            substituted.verify_with_public_key(&pk, &BTreeMap::new()),
            Err(PortablePackError::Signature)
        );
        let mut incomplete = p.clone();
        incomplete.component_hashes.remove("omissions");
        assert!(matches!(
            incomplete.verify(&[7; 32], &BTreeMap::new()),
            Err(PortablePackError::Manifest(_))
        ));
        let mut unknown = p.clone();
        unknown.component_hashes.insert("extra".into(), "x".into());
        assert!(matches!(
            unknown.verify(&[7; 32], &BTreeMap::new()),
            Err(PortablePackError::Manifest(_))
        ));
    }

    #[test]
    fn bounded_parser_rejects_unknown_fields_and_oversize() {
        let mut value = serde_json::to_value(pack()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), serde_json::json!(true));
        assert_eq!(
            PortableContextPackV1::from_json_bounded(&value.to_string()),
            Err(PortablePackError::Json)
        );
        let mut huge = pack();
        for i in 0..=MAX_COMPONENTS {
            huge.component_hashes.insert(format!("x{i}"), "x".into());
        }
        let json = serde_json::to_string(&huge).unwrap();
        assert_eq!(
            PortableContextPackV1::from_json_bounded(&json),
            Err(PortablePackError::Bound("componentHashes"))
        );
    }
}
