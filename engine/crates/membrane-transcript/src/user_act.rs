//! Authenticated host user-act envelopes and capability reporting.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use ring::signature;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::GENERIC_HOSTS;
use crate::canonical::sha256_hex;
use crate::evidence::{
    ActKind, EvidenceClass, EvidenceError, UserActEvidenceV1, UserActProvenanceReceiptV1,
    VerifiedUserActEvidence, USER_ACT_EVIDENCE_SCHEMA,
};

pub const USER_ACT_ADAPTER_VERSION: &str = "membrane.user-act-adapter.v2";
pub const USER_ACT_ROW_TYPE: &str = "adapt_user_act_v2";
pub const USER_ACT_TRUST_CONTRACT: &str = "adapt.user-act-trust.v1";
const SIGNATURE_DOMAIN: &[u8] = b"membrane.adapt-user-act.v2\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Supported,
    TranscriptOnly,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostActCapabilityReportV1 {
    pub schema_version: String,
    pub host: String,
    pub capabilities: BTreeMap<ActKind, CapabilityState>,
    pub authenticated_signal_rows: u32,
    pub omissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedHostIssuerV1 {
    pub issuer_id: String,
    pub key_id: String,
    pub host: String,
    pub public_key_hex: String,
    #[serde(default)]
    pub revoked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostActTrustStoreV1 {
    pub contract_version: String,
    pub installation_id: String,
    pub issuers: Vec<TrustedHostIssuerV1>,
}

impl HostActTrustStoreV1 {
    pub fn load(path: &Path) -> Result<Self, EvidenceError> {
        let bytes = fs::read(path).map_err(|_| EvidenceError::UntrustedIssuer)?;
        let store: Self = serde_json::from_slice(&bytes).map_err(|_| EvidenceError::UntrustedIssuer)?;
        store.validate()?;
        Ok(store)
    }

    pub fn validate(&self) -> Result<(), EvidenceError> {
        if self.contract_version != USER_ACT_TRUST_CONTRACT || self.installation_id.trim().is_empty() {
            return Err(EvidenceError::UntrustedIssuer);
        }
        let mut seen = BTreeSet::new();
        for issuer in &self.issuers {
            let key = (&issuer.issuer_id, &issuer.key_id, &issuer.host);
            let valid_key = issuer.public_key_hex.len() == 64
                && issuer.public_key_hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
            if issuer.issuer_id.trim().is_empty() || issuer.key_id.trim().is_empty() || issuer.host.trim().is_empty()
                || !valid_key || !seen.insert(key)
            { return Err(EvidenceError::UntrustedIssuer); }
        }
        Ok(())
    }

    fn issuer(&self, receipt: &UserActProvenanceReceiptV1) -> Option<&TrustedHostIssuerV1> {
        self.issuers.iter().find(|issuer| !issuer.revoked
            && issuer.issuer_id == receipt.issuer_id
            && issuer.key_id == receipt.key_id
            && issuer.host == receipt.host)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayClaim {
    pub installation_id: String,
    pub issuer_id: String,
    pub key_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub nonce: String,
    pub receipt_sha256: String,
    pub evidence_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayDisposition {
    Claimed,
    AlreadySeenSame,
}

pub trait HostActReplayStore {
    fn claim(&mut self, claim: &ReplayClaim) -> Result<ReplayDisposition, EvidenceError>;
}

#[derive(Default)]
pub struct MemoryReplayStore {
    by_sequence: BTreeMap<(String, String, String, String, u64), (String, String)>,
    by_nonce: BTreeMap<(String, String, String, String), String>,
}

impl HostActReplayStore for MemoryReplayStore {
    fn claim(&mut self, claim: &ReplayClaim) -> Result<ReplayDisposition, EvidenceError> {
        let sequence_key = (claim.installation_id.clone(), claim.issuer_id.clone(), claim.key_id.clone(), claim.session_id.clone(), claim.sequence);
        let nonce_key = (claim.installation_id.clone(), claim.issuer_id.clone(), claim.key_id.clone(), claim.nonce.clone());
        if let Some((digest, evidence_id)) = self.by_sequence.get(&sequence_key) {
            return if digest == &claim.receipt_sha256 && evidence_id == &claim.evidence_id {
                Ok(ReplayDisposition::AlreadySeenSame)
            } else { Err(EvidenceError::ReceiptReplay) };
        }
        if self.by_nonce.contains_key(&nonce_key) { return Err(EvidenceError::ReceiptReplay); }
        self.by_sequence.insert(sequence_key, (claim.receipt_sha256.clone(), claim.evidence_id.clone()));
        self.by_nonce.insert(nonce_key, claim.receipt_sha256.clone());
        Ok(ReplayDisposition::Claimed)
    }
}

pub struct SqliteReplayStore { conn: Connection }

impl SqliteReplayStore {
    pub fn open(path: &Path) -> Result<Self, EvidenceError> {
        let conn = Connection::open(path).map_err(|_| EvidenceError::InvalidProvenanceReceipt)?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS adapt_user_act_replay (
            installation_id TEXT NOT NULL, issuer_id TEXT NOT NULL, key_id TEXT NOT NULL,
            session_id TEXT NOT NULL, sequence INTEGER NOT NULL, nonce TEXT NOT NULL,
            receipt_sha256 TEXT NOT NULL, evidence_id TEXT NOT NULL,
            PRIMARY KEY (installation_id, issuer_id, key_id, session_id, sequence),
            UNIQUE (installation_id, issuer_id, key_id, nonce));")
            .map_err(|_| EvidenceError::InvalidProvenanceReceipt)?;
        Ok(Self { conn })
    }
}

impl HostActReplayStore for SqliteReplayStore {
    fn claim(&mut self, claim: &ReplayClaim) -> Result<ReplayDisposition, EvidenceError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|_| EvidenceError::InvalidProvenanceReceipt)?;
        let existing: Option<(String, String)> = tx.query_row(
            "SELECT receipt_sha256,evidence_id FROM adapt_user_act_replay WHERE installation_id=?1 AND issuer_id=?2 AND key_id=?3 AND session_id=?4 AND sequence=?5",
            params![claim.installation_id, claim.issuer_id, claim.key_id, claim.session_id, claim.sequence as i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(|_| EvidenceError::InvalidProvenanceReceipt)?;
        if let Some((digest, evidence_id)) = existing {
            return if digest == claim.receipt_sha256 && evidence_id == claim.evidence_id {
                Ok(ReplayDisposition::AlreadySeenSame)
            } else {
                Err(EvidenceError::ReceiptReplay)
            };
        }
        tx.execute("INSERT INTO adapt_user_act_replay (installation_id,issuer_id,key_id,session_id,sequence,nonce,receipt_sha256,evidence_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![claim.installation_id, claim.issuer_id, claim.key_id, claim.session_id, claim.sequence as i64, claim.nonce, claim.receipt_sha256, claim.evidence_id])
            .map_err(|_| EvidenceError::ReceiptReplay)?;
        tx.commit()
            .map_err(|_| EvidenceError::InvalidProvenanceReceipt)?;
        Ok(ReplayDisposition::Claimed)
    }
}

pub struct HostActVerifier<R> {
    trust: HostActTrustStoreV1,
    replay: R,
}

impl<R: HostActReplayStore> HostActVerifier<R> {
    pub fn new(trust: HostActTrustStoreV1, replay: R) -> Result<Self, EvidenceError> {
        trust.validate()?;
        Ok(Self { trust, replay })
    }

    pub fn verify_row(
        &mut self,
        row: &Value,
    ) -> Result<Option<VerifiedUserActEvidence>, EvidenceError> {
        self.verify_row_with_source(row, None)
    }

    pub fn verify_row_with_source(
        &mut self,
        row: &Value,
        source_bytes: Option<&[u8]>,
    ) -> Result<Option<VerifiedUserActEvidence>, EvidenceError> {
        match row.get("type").and_then(Value::as_str) {
            Some("adapt_user_act_v1") => return Err(EvidenceError::InvalidProvenanceReceipt),
            Some(USER_ACT_ROW_TYPE) => {}
            _ => return Ok(None),
        }
        if row.get("actor").is_some() || row.get("public_key").is_some() {
            return Err(EvidenceError::InvalidProvenanceReceipt);
        }
        let mut body = row.clone();
        body.as_object_mut()
            .ok_or(EvidenceError::InvalidProvenanceReceipt)?
            .remove("type");
        let evidence: UserActEvidenceV1 =
            serde_json::from_value(body).map_err(|_| EvidenceError::InvalidProvenanceReceipt)?;
        let mut canonical_event_ids = evidence.event_ids.clone();
        canonical_event_ids.sort();
        canonical_event_ids.dedup();
        if evidence.schema_version != USER_ACT_EVIDENCE_SCHEMA
            || evidence.evidence_id.trim().is_empty()
            || evidence.installation_id.trim().is_empty()
            || evidence.host.trim().is_empty()
            || evidence.session_id.trim().is_empty()
            || evidence.timestamp.trim().is_empty()
            || evidence.event_ids.is_empty()
            || evidence.event_ids != canonical_event_ids
            || evidence.event_ids.iter().any(|id| id.trim().is_empty())
        {
            return Err(EvidenceError::InvalidProvenanceReceipt);
        }
        let receipt = &evidence.provenance_receipt;
        if receipt.contract_version != crate::evidence::USER_ACT_RECEIPT_CONTRACT
            || receipt.issuer_id.trim().is_empty()
            || receipt.key_id.trim().is_empty()
            || receipt.installation_id.trim().is_empty()
            || receipt.host.trim().is_empty()
            || receipt.session_id.trim().is_empty()
            || receipt.sequence == 0
            || receipt.sequence > i64::MAX as u64
            || receipt.nonce.trim().is_empty()
            || receipt.payload_sha256.len() != 64
            || !receipt
                .payload_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || receipt.signature_hex.len() != 128
            || !receipt
                .signature_hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(EvidenceError::InvalidProvenanceReceipt);
        }
        if evidence.installation_id != self.trust.installation_id
            || receipt.installation_id != evidence.installation_id
        {
            return Err(EvidenceError::InstallationMismatch);
        }
        if receipt.host != evidence.host {
            return Err(EvidenceError::HostMismatch);
        }
        if receipt.session_id != evidence.session_id {
            return Err(EvidenceError::ReceiptBindingMismatch);
        }
        if (evidence.signal_strength - evidence.act_kind.default_signal_strength()).abs()
            > f64::EPSILON
        {
            return Err(EvidenceError::ReceiptBindingMismatch);
        }
        for (excerpt, digest) in [
            (
                evidence.before_excerpt.as_deref(),
                evidence.before_digest.as_deref(),
            ),
            (
                evidence.after_excerpt.as_deref(),
                evidence.after_digest.as_deref(),
            ),
        ] {
            match (excerpt, digest) {
                (Some(excerpt), Some(digest))
                    if sha256_hex(excerpt.trim().as_bytes()) == digest => {}
                (None, None) => {}
                _ => return Err(EvidenceError::ReceiptBindingMismatch),
            }
        }
        // Edit-like acts bind both the avoided content and the user's actual
        // replacement. Explicit preferences/corrections/choices must bind the
        // resulting user-authored text. Never treat the pre-edit excerpt as
        // the authoritative rule.
        let structure_ok = match evidence.act_kind {
            ActKind::PostAcceptEdit | ActKind::RepeatedEdit => {
                evidence.before_excerpt.is_some() && evidence.after_excerpt.is_some()
            }
            ActKind::ExplicitPreference | ActKind::Correction | ActKind::NamedChoice => {
                evidence.after_excerpt.is_some()
            }
            ActKind::Reject | ActKind::Accept => evidence.before_excerpt.is_some(),
        };
        if !structure_ok {
            return Err(EvidenceError::ReceiptBindingMismatch);
        }
        if let Some(span) = &evidence.user_source_span {
            if span.session_id != evidence.session_id
                || !evidence.event_ids.contains(&span.event_id)
                || span.byte_start < 0
                || span.byte_end < span.byte_start
                || span.bytes_sha256.len() != 64
            {
                return Err(EvidenceError::ReceiptBindingMismatch);
            }
            let source_bytes = source_bytes.ok_or(EvidenceError::ReceiptBindingMismatch)?;
            evidence.verify_span(source_bytes)?;
        }
        let issuer = self
            .trust
            .issuer(receipt)
            .ok_or(EvidenceError::UntrustedIssuer)?;
        let payload_bytes = serde_json::to_vec(&evidence.receipt_payload())
            .map_err(|_| EvidenceError::InvalidProvenanceReceipt)?;
        if receipt.payload_sha256 != sha256_hex(&payload_bytes) {
            return Err(EvidenceError::ReceiptBindingMismatch);
        }
        let public_key =
            hex::decode(&issuer.public_key_hex).map_err(|_| EvidenceError::UntrustedIssuer)?;
        if public_key.len() != 32 {
            return Err(EvidenceError::UntrustedIssuer);
        }
        let signature_bytes =
            hex::decode(&receipt.signature_hex).map_err(|_| EvidenceError::ReceiptForgery)?;
        let mut signed = SIGNATURE_DOMAIN.to_vec();
        signed.extend_from_slice(&payload_bytes);
        signature::UnparsedPublicKey::new(&signature::ED25519, public_key)
            .verify(&signed, &signature_bytes)
            .map_err(|_| EvidenceError::ReceiptForgery)?;
        let receipt_sha256 = sha256_hex(
            &serde_json::to_vec(receipt).map_err(|_| EvidenceError::InvalidProvenanceReceipt)?,
        );
        self.replay.claim(&ReplayClaim {
            installation_id: evidence.installation_id.clone(),
            issuer_id: receipt.issuer_id.clone(),
            key_id: receipt.key_id.clone(),
            session_id: evidence.session_id.clone(),
            sequence: receipt.sequence,
            nonce: receipt.nonce.clone(),
            receipt_sha256: receipt_sha256.clone(),
            evidence_id: evidence.evidence_id.clone(),
        })?;
        Ok(Some(VerifiedUserActEvidence::new(evidence, receipt_sha256)))
    }
}

pub fn read_user_act_rows(path: &Path) -> Result<Vec<Value>, EvidenceError> {
    let body = fs::read_to_string(path).map_err(|_| EvidenceError::InvalidProvenanceReceipt)?;
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|_| EvidenceError::InvalidProvenanceReceipt))
        .collect()
}

fn known_host(host: &str) -> bool {
    matches!(host, "claude_code" | "codex" | "coderight") || GENERIC_HOSTS.contains(&host)
}

pub fn capability_report(
    host: &str,
    evidence: &[VerifiedUserActEvidence],
    mut omissions: Vec<String>,
) -> HostActCapabilityReportV1 {
    let observed: BTreeSet<ActKind> = evidence
        .iter()
        .filter(|item| item.evidence().host == host)
        .map(|item| item.evidence().act_kind)
        .collect();
    let mut capabilities = BTreeMap::new();
    for kind in [
        ActKind::ExplicitPreference,
        ActKind::Correction,
        ActKind::Reject,
        ActKind::Accept,
        ActKind::PostAcceptEdit,
        ActKind::RepeatedEdit,
        ActKind::NamedChoice,
    ] {
        let state = if observed.contains(&kind) {
            CapabilityState::Supported
        } else if matches!(kind, ActKind::ExplicitPreference | ActKind::Correction)
            && known_host(host)
        {
            CapabilityState::TranscriptOnly
        } else {
            CapabilityState::Unavailable
        };
        capabilities.insert(kind, state);
    }
    omissions.extend(
        capabilities
            .iter()
            .filter(|(_, state)| **state == CapabilityState::Unavailable)
            .map(|(kind, _)| format!("{kind:?}:host_signal_unavailable")),
    );
    omissions.sort();
    omissions.dedup();
    HostActCapabilityReportV1 {
        schema_version: USER_ACT_ADAPTER_VERSION.into(),
        host: host.into(),
        capabilities,
        authenticated_signal_rows: evidence
            .iter()
            .filter(|e| e.evidence().host == host)
            .count() as u32,
        omissions,
    }
}

pub fn candidate_excerpt(evidence: &VerifiedUserActEvidence) -> Option<&str> {
    match evidence.evidence().act_kind {
        ActKind::ExplicitPreference
        | ActKind::Correction
        | ActKind::PostAcceptEdit
        | ActKind::RepeatedEdit
        | ActKind::NamedChoice => evidence.evidence().after_excerpt.as_deref(),
        ActKind::Reject | ActKind::Accept => None,
    }
}

pub fn authoritative_excerpt(evidence: &VerifiedUserActEvidence) -> Option<&str> {
    (evidence.classify() == EvidenceClass::UserAuthoritative)
        .then(|| candidate_excerpt(evidence))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{UserActProvenanceReceiptV1, USER_ACT_RECEIPT_CONTRACT};
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn signed_row(
        seed: [u8; 32],
        sequence: u64,
        nonce: &str,
        after: &str,
    ) -> (Value, HostActTrustStoreV1) {
        let key = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        let receipt = UserActProvenanceReceiptV1 {
            contract_version: USER_ACT_RECEIPT_CONTRACT.into(),
            issuer_id: "coderight-local".into(),
            key_id: "key-1".into(),
            installation_id: "inst-1".into(),
            host: "coderight".into(),
            session_id: "session-1".into(),
            sequence,
            nonce: nonce.into(),
            payload_sha256: "0".repeat(64),
            signature_hex: "0".repeat(128),
        };
        let mut evidence = UserActEvidenceV1::new(
            &format!("act-{sequence}-{nonce}"),
            "inst-1",
            "coderight",
            "session-1",
            vec![format!("event-{sequence}")],
            ActKind::PostAcceptEdit,
            None,
            BTreeMap::from([("repo".into(), "membrane".into())]),
            "2026-08-26T00:00:00Z",
            receipt,
        )
        .unwrap();
        evidence
            .set_counterfactual(Some("broad rewrite"), Some(after))
            .unwrap();
        let payload = serde_json::to_vec(&evidence.receipt_payload()).unwrap();
        evidence.provenance_receipt.payload_sha256 = sha256_hex(&payload);
        let mut signed = SIGNATURE_DOMAIN.to_vec();
        signed.extend_from_slice(&payload);
        evidence.provenance_receipt.signature_hex = hex::encode(key.sign(&signed).as_ref());
        let mut row = serde_json::to_value(evidence).unwrap();
        row.as_object_mut()
            .unwrap()
            .insert("type".into(), Value::String(USER_ACT_ROW_TYPE.into()));
        let trust = HostActTrustStoreV1 {
            contract_version: USER_ACT_TRUST_CONTRACT.into(),
            installation_id: "inst-1".into(),
            issuers: vec![TrustedHostIssuerV1 {
                issuer_id: "coderight-local".into(),
                key_id: "key-1".into(),
                host: "coderight".into(),
                public_key_hex: hex::encode(key.public_key().as_ref()),
                revoked: false,
            }],
        };
        (row, trust)
    }

    #[test]
    fn pinned_key_roundtrip_and_capability_use_verified_only() {
        let (row, trust) = signed_row([7; 32], 1, "nonce-a", "focused patch");
        let mut verifier = HostActVerifier::new(trust, MemoryReplayStore::default()).unwrap();
        let verified = verifier.verify_row(&row).unwrap().unwrap();
        assert_eq!(verified.classify(), EvidenceClass::UserBehavioral);
        assert_eq!(authoritative_excerpt(&verified), None);
        assert_eq!(candidate_excerpt(&verified), Some("focused patch"));
        let report = capability_report("coderight", &[verified], vec![]);
        assert_eq!(
            report.capabilities[&ActKind::PostAcceptEdit],
            CapabilityState::Supported
        );
        assert_eq!(report.authenticated_signal_rows, 1);
    }

    #[test]
    fn forgery_binding_mismatch_and_v1_self_attestation_fail_closed() {
        let (row, trust) = signed_row([8; 32], 1, "nonce-a", "focused patch");
        let mut forged = row.clone();
        forged["after_excerpt"] = Value::String("tampered patch".into());
        let mut verifier =
            HostActVerifier::new(trust.clone(), MemoryReplayStore::default()).unwrap();
        assert_eq!(
            verifier.verify_row(&forged).unwrap_err(),
            EvidenceError::ReceiptBindingMismatch
        );
        let mut wrong_install = row.clone();
        wrong_install["installation_id"] = Value::String("other".into());
        assert_eq!(
            verifier.verify_row(&wrong_install).unwrap_err(),
            EvidenceError::InstallationMismatch
        );
        let legacy = serde_json::json!({"type":"adapt_user_act_v1","actor":"authenticated_user","provenance_receipt":"anything"});
        assert_eq!(
            verifier.verify_row(&legacy).unwrap_err(),
            EvidenceError::InvalidProvenanceReceipt
        );
    }

    #[test]
    fn replay_is_idempotent_for_exact_rescan_and_persistent_for_collisions() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("replay.db");
        let (first, trust) = signed_row([9; 32], 1, "nonce-a", "focused patch");
        let mut verifier =
            HostActVerifier::new(trust.clone(), SqliteReplayStore::open(&db).unwrap()).unwrap();
        verifier.verify_row(&first).unwrap().unwrap();
        verifier.verify_row(&first).unwrap().unwrap();
        drop(verifier);
        let (collision, _) = signed_row([9; 32], 1, "nonce-b", "different patch");
        let mut reopened =
            HostActVerifier::new(trust, SqliteReplayStore::open(&db).unwrap()).unwrap();
        assert_eq!(
            reopened.verify_row(&collision).unwrap_err(),
            EvidenceError::ReceiptReplay
        );
    }

    #[test]
    fn silent_acceptance_never_yields_authoritative_excerpt() {
        let (mut row, trust) = signed_row([10; 32], 1, "nonce-a", "focused patch");
        row["act_kind"] = Value::String("accept".into());
        let mut verifier = HostActVerifier::new(trust, MemoryReplayStore::default()).unwrap();
        assert!(matches!(
            verifier.verify_row(&row),
            Err(EvidenceError::ReceiptBindingMismatch)
        ));
    }

    #[test]
    fn malformed_identity_event_order_and_sequence_fail_before_authority() {
        let (row, trust) = signed_row([11; 32], 1, "nonce-a", "focused patch");
        let mut empty_id = row.clone();
        empty_id["evidence_id"] = Value::String(String::new());
        let mut verifier =
            HostActVerifier::new(trust.clone(), MemoryReplayStore::default()).unwrap();
        assert_eq!(
            verifier.verify_row(&empty_id).unwrap_err(),
            EvidenceError::InvalidProvenanceReceipt
        );

        let mut unordered = row.clone();
        unordered["event_ids"] = serde_json::json!(["z", "a"]);
        let mut verifier =
            HostActVerifier::new(trust.clone(), MemoryReplayStore::default()).unwrap();
        assert_eq!(
            verifier.verify_row(&unordered).unwrap_err(),
            EvidenceError::InvalidProvenanceReceipt
        );

        let mut overflow = row;
        overflow["provenance_receipt"]["sequence"] = Value::from(u64::MAX);
        let mut verifier = HostActVerifier::new(trust, MemoryReplayStore::default()).unwrap();
        assert_eq!(
            verifier.verify_row(&overflow).unwrap_err(),
            EvidenceError::InvalidProvenanceReceipt
        );
    }

    #[test]
    fn edit_act_requires_signed_replacement_and_never_falls_back_to_before() {
        let (row, trust) = signed_row([13; 32], 1, "nonce-a", "focused patch");
        let mut missing_after = row.clone();
        missing_after["after_excerpt"] = Value::Null;
        missing_after["after_digest"] = Value::Null;
        let mut verifier = HostActVerifier::new(trust, MemoryReplayStore::default()).unwrap();
        assert_eq!(
            verifier.verify_row(&missing_after).unwrap_err(),
            EvidenceError::ReceiptBindingMismatch
        );
    }

    #[test]
    fn ambiguous_or_malformed_trust_store_is_rejected() {
        let (_, mut trust) = signed_row([12; 32], 1, "nonce-a", "focused patch");
        trust.issuers.push(trust.issuers[0].clone());
        assert!(matches!(
            HostActVerifier::new(trust, MemoryReplayStore::default()),
            Err(EvidenceError::UntrustedIssuer)
        ));
        let (_, mut malformed) = signed_row([12; 32], 1, "nonce-a", "focused patch");
        malformed.issuers[0].public_key_hex = "ABC".into();
        assert!(matches!(
            HostActVerifier::new(malformed, MemoryReplayStore::default()),
            Err(EvidenceError::UntrustedIssuer)
        ));
    }
}
