//! MBR-911: fail-closed admission across updater & platform trust domains.
//! This crate verifies evidence only; it cannot download or activate updates.

use serde::Serialize;

pub const RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const REPAIR_PATH: &str = "repair/update-signatures";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Macos,
    Windows,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TauriSignatureEvidence {
    pub key_id: String,
    pub signature: Vec<u8>,
    pub signed_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformTrustEvidence {
    pub platform: Platform,
    pub receipt_id: String,
    pub signed_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCandidate {
    pub from_version: String,
    pub to_version: String,
    pub artifact_sha256: String,
    pub tauri: TauriSignatureEvidence,
    pub platform: PlatformTrustEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformVerification {
    pub signature_valid: bool,
    /// macOS: notarization; Windows: Public Trust plus RFC3161 timestamp.
    pub platform_trust_valid: bool,
}

/// Trusted adapters perform cryptographic/platform verification. Wire evidence
/// never carries caller-asserted verification booleans.
pub trait UpdateTrustVerifier {
    fn verify_tauri(
        &self,
        artifact_sha256: &str,
        evidence: &TauriSignatureEvidence,
    ) -> bool;
    fn verify_platform(
        &self,
        artifact_sha256: &str,
        evidence: &PlatformTrustEvidence,
    ) -> PlatformVerification;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCode {
    InvalidEvidenceIdentity,
    TauriUpdaterSignatureInvalid,
    PlatformSignatureInvalid,
    PlatformTrustInvalid,
}

impl FailureCode {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidEvidenceIdentity => "invalid_update_evidence_identity",
            Self::TauriUpdaterSignatureInvalid => "tauri_updater_signature_invalid",
            Self::PlatformSignatureInvalid => "platform_signature_invalid",
            Self::PlatformTrustInvalid => "platform_trust_invalid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReceipt {
    pub schema_version: u32,
    pub outcome: &'static str,
    pub from_version: String,
    pub to_version: String,
    pub artifact_sha256: String,
    pub platform: Platform,
    pub tauri_key_id: String,
    pub platform_receipt_id: String,
    pub failures: Vec<&'static str>,
    pub repair_path: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedUpdate(pub UpdateReceipt);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedUpdate(pub UpdateReceipt);

pub fn verify<V: UpdateTrustVerifier>(
    candidate: &UpdateCandidate,
    verifier: &V,
) -> Result<VerifiedUpdate, BlockedUpdate> {
    let mut failures = Vec::new();
    if !identity_valid(candidate) {
        failures.push(FailureCode::InvalidEvidenceIdentity.code());
    } else {
        if !verifier.verify_tauri(&candidate.artifact_sha256, &candidate.tauri) {
            failures.push(FailureCode::TauriUpdaterSignatureInvalid.code());
        }
        let platform = verifier.verify_platform(&candidate.artifact_sha256, &candidate.platform);
        if !platform.signature_valid {
            failures.push(FailureCode::PlatformSignatureInvalid.code());
        }
        if !platform.platform_trust_valid {
            failures.push(FailureCode::PlatformTrustInvalid.code());
        }
    }
    let outcome = if failures.is_empty() { "verified" } else { "blocked" };
    let receipt = receipt(candidate, outcome, failures);
    if outcome == "verified" {
        Ok(VerifiedUpdate(receipt))
    } else {
        Err(BlockedUpdate(receipt))
    }
}

fn identity_valid(candidate: &UpdateCandidate) -> bool {
    !candidate.from_version.is_empty()
        && !candidate.to_version.is_empty()
        && candidate.from_version != candidate.to_version
        && valid_sha256(&candidate.artifact_sha256)
        && candidate.tauri.signed_sha256 == candidate.artifact_sha256
        && candidate.platform.signed_sha256 == candidate.artifact_sha256
        && !candidate.tauri.key_id.is_empty()
        && !candidate.tauri.signature.is_empty()
        && !candidate.platform.receipt_id.is_empty()
}

fn valid_sha256(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
}

fn receipt(
    candidate: &UpdateCandidate,
    outcome: &'static str,
    failures: Vec<&'static str>,
) -> UpdateReceipt {
    UpdateReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION,
        outcome,
        from_version: candidate.from_version.clone(),
        to_version: candidate.to_version.clone(),
        artifact_sha256: candidate.artifact_sha256.clone(),
        platform: candidate.platform.platform,
        tauri_key_id: candidate.tauri.key_id.clone(),
        platform_receipt_id: candidate.platform.receipt_id.clone(),
        failures,
        repair_path: REPAIR_PATH,
    }
}
