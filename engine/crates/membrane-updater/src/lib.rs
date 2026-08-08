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
    fn verify_tauri(&self, artifact_sha256: &str, evidence: &TauriSignatureEvidence) -> bool;
    fn verify_platform(
        &self,
        artifact_sha256: &str,
        evidence: &PlatformTrustEvidence,
    ) -> PlatformVerification;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCode {
    InvalidEvidenceIdentity,
    DowngradeRejected,
    TauriUpdaterSignatureInvalid,
    PlatformSignatureInvalid,
    PlatformTrustInvalid,
}

impl FailureCode {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidEvidenceIdentity => "invalid_update_evidence_identity",
            Self::DowngradeRejected => "update_downgrade_rejected",
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

/// Fails closed on every path: a missing signature, an unknown key (rejected
/// by the trusted `verifier`, surfacing as an invalid Tauri/platform
/// signature), one valid signature out of two, a downgrade, or a mismatched
/// artifact hash all return `Err(BlockedUpdate)`. Downgrade admission is
/// checked before either trust domain so a stale-but-validly-signed artifact
/// is refused without ever asking a trusted adapter to verify it.
pub fn verify<V: UpdateTrustVerifier>(
    candidate: &UpdateCandidate,
    verifier: &V,
) -> Result<VerifiedUpdate, BlockedUpdate> {
    let mut failures = Vec::new();
    if !identity_valid(candidate) {
        failures.push(FailureCode::InvalidEvidenceIdentity.code());
    } else if !is_upgrade(&candidate.from_version, &candidate.to_version) {
        failures.push(FailureCode::DowngradeRejected.code());
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
    let outcome = if failures.is_empty() {
        "verified"
    } else {
        "blocked"
    };
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
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

/// `to` must be a strictly greater `major.minor.patch` than `from`. Either
/// version failing to parse (missing a numeric major/minor/patch component)
/// is treated the same as a downgrade: admission fails closed rather than
/// guessing at an unparseable identity.
fn is_upgrade(from: &str, to: &str) -> bool {
    match (version_triple(from), version_triple(to)) {
        (Some(from), Some(to)) => to > from,
        _ => false,
    }
}

/// Parses the leading `major.minor.patch` numeric triple from a version
/// string. A pre-release/build suffix on the patch component (e.g.
/// `2.1.0-rc.1`) is tolerated by taking only the leading digits of that
/// component; anything else that fails to parse returns `None`.
fn version_triple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = leading_digits(parts.next()?)?;
    let minor = leading_digits(parts.next()?)?;
    let patch = leading_digits(parts.next()?)?;
    Some((major, minor, patch))
}

fn leading_digits(part: &str) -> Option<u64> {
    let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
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

// --- Deterministic, unexecuted-at-task-time source tests -------------------
//
// These `#[test]`s are NOT run by this task (per MEMBRANE-BOOK-MODE-EXECUTION-RULES.md
// and this task's hard rules: no `cargo test`). They are reasoned through by
// type and left for `cargo test --manifest-path engine/Cargo.toml --workspace`
// at the Book 3 final phased gate. Each negative test isolates exactly one
// of the five fail-closed cases the MBR-911 acceptance criterion names:
// missing signature, unknown key, one valid signature out of two, a
// downgrade, and a mismatched hash.
#[cfg(test)]
mod tests {
    use super::*;

    /// A verifier over an explicit trusted-key allowlist, standing in for a
    /// real adapter (e.g. Tauri's own minisign check, or a codesign/spctl /
    /// signtool invocation) without performing any cryptography itself. It
    /// rejects an unrecognized `key_id` exactly like a real adapter would
    /// reject a signature made with a key it does not trust.
    struct MockVerifier {
        trusted_tauri_key_ids: &'static [&'static str],
        platform_signature_valid: bool,
        platform_trust_valid: bool,
    }

    impl UpdateTrustVerifier for MockVerifier {
        fn verify_tauri(&self, artifact_sha256: &str, evidence: &TauriSignatureEvidence) -> bool {
            evidence.signed_sha256 == artifact_sha256
                && !evidence.signature.is_empty()
                && self
                    .trusted_tauri_key_ids
                    .contains(&evidence.key_id.as_str())
        }
        fn verify_platform(
            &self,
            _artifact_sha256: &str,
            _evidence: &PlatformTrustEvidence,
        ) -> PlatformVerification {
            PlatformVerification {
                signature_valid: self.platform_signature_valid,
                platform_trust_valid: self.platform_trust_valid,
            }
        }
    }

    const ARTIFACT_SHA256: &str =
        "sha256:0000000000000000000000000000000000000000000000000000000000ab";
    const TRUSTED_KEY: &str = "rightsuite-updater-key-2026";

    fn fully_trusting_verifier() -> MockVerifier {
        MockVerifier {
            trusted_tauri_key_ids: &[TRUSTED_KEY],
            platform_signature_valid: true,
            platform_trust_valid: true,
        }
    }

    fn candidate(from_version: &str, to_version: &str) -> UpdateCandidate {
        UpdateCandidate {
            from_version: from_version.to_string(),
            to_version: to_version.to_string(),
            artifact_sha256: ARTIFACT_SHA256.to_string(),
            tauri: TauriSignatureEvidence {
                key_id: TRUSTED_KEY.to_string(),
                signature: vec![1, 2, 3, 4],
                signed_sha256: ARTIFACT_SHA256.to_string(),
            },
            platform: PlatformTrustEvidence {
                platform: Platform::Windows,
                receipt_id: "receipt-001".to_string(),
                signed_sha256: ARTIFACT_SHA256.to_string(),
            },
        }
    }

    #[test]
    fn dual_valid_signatures_verify_and_carry_no_failures() {
        let outcome = verify(&candidate("0.1.5", "0.1.6"), &fully_trusting_verifier());
        let VerifiedUpdate(receipt) = outcome.expect("both signatures valid, upgrade version");
        assert_eq!(receipt.outcome, "verified");
        assert!(receipt.failures.is_empty());
    }

    #[test]
    fn missing_tauri_signature_blocks_and_preserves_current_version() {
        let mut candidate = candidate("0.1.5", "0.1.6");
        candidate.tauri.signature.clear();
        let outcome = verify(&candidate, &fully_trusting_verifier());
        let BlockedUpdate(receipt) = outcome.expect_err("empty signature bytes must fail closed");
        assert_eq!(receipt.outcome, "blocked");
        assert_eq!(
            receipt.failures,
            vec![FailureCode::InvalidEvidenceIdentity.code()]
        );
        // No activation API exists on this crate; a `BlockedUpdate` cannot
        // have mutated anything, so the current version is preserved by
        // construction. The receipt still names the version pair for audit.
        assert_eq!(receipt.from_version, "0.1.5");
        assert_eq!(receipt.to_version, "0.1.6");
    }

    #[test]
    fn unknown_tauri_key_blocks() {
        let mut candidate = candidate("0.1.5", "0.1.6");
        candidate.tauri.key_id = "attacker-controlled-key".to_string();
        let outcome = verify(&candidate, &fully_trusting_verifier());
        let BlockedUpdate(receipt) = outcome.expect_err("unrecognized key must fail closed");
        assert_eq!(
            receipt.failures,
            vec![FailureCode::TauriUpdaterSignatureInvalid.code()]
        );
    }

    #[test]
    fn tauri_valid_but_platform_signature_invalid_blocks() {
        let verifier = MockVerifier {
            trusted_tauri_key_ids: &[TRUSTED_KEY],
            platform_signature_valid: false,
            platform_trust_valid: true,
        };
        let outcome = verify(&candidate("0.1.5", "0.1.6"), &verifier);
        let BlockedUpdate(receipt) =
            outcome.expect_err("one of two signatures invalid must fail closed");
        assert_eq!(
            receipt.failures,
            vec![FailureCode::PlatformSignatureInvalid.code()]
        );
    }

    #[test]
    fn platform_valid_but_tauri_signature_invalid_blocks() {
        let verifier = MockVerifier {
            trusted_tauri_key_ids: &[],
            platform_signature_valid: true,
            platform_trust_valid: true,
        };
        let outcome = verify(&candidate("0.1.5", "0.1.6"), &verifier);
        let BlockedUpdate(receipt) =
            outcome.expect_err("the other one-of-two case must also fail closed");
        assert_eq!(
            receipt.failures,
            vec![FailureCode::TauriUpdaterSignatureInvalid.code()]
        );
    }

    #[test]
    fn signature_valid_but_platform_not_notarized_blocks() {
        let verifier = MockVerifier {
            trusted_tauri_key_ids: &[TRUSTED_KEY],
            platform_signature_valid: true,
            platform_trust_valid: false,
        };
        let outcome = verify(&candidate("0.1.5", "0.1.6"), &verifier);
        let BlockedUpdate(receipt) =
            outcome.expect_err("codesigned but unnotarized must fail closed");
        assert_eq!(
            receipt.failures,
            vec![FailureCode::PlatformTrustInvalid.code()]
        );
    }

    #[test]
    fn downgrade_blocks_even_with_two_fully_valid_signatures() {
        let outcome = verify(&candidate("0.2.0", "0.1.9"), &fully_trusting_verifier());
        let BlockedUpdate(receipt) =
            outcome.expect_err("a downgrade must fail closed regardless of signature validity");
        assert_eq!(
            receipt.failures,
            vec![FailureCode::DowngradeRejected.code()]
        );
    }

    #[test]
    fn unparseable_version_is_treated_as_a_downgrade() {
        let outcome = verify(&candidate("0.1.5", "latest"), &fully_trusting_verifier());
        let BlockedUpdate(receipt) =
            outcome.expect_err("an unparseable target version must fail closed");
        assert_eq!(
            receipt.failures,
            vec![FailureCode::DowngradeRejected.code()]
        );
    }

    #[test]
    fn mismatched_artifact_hash_between_tauri_and_platform_evidence_blocks() {
        let mut candidate = candidate("0.1.5", "0.1.6");
        candidate.platform.signed_sha256 =
            "sha256:1111111111111111111111111111111111111111111111111111111111ab".to_string();
        let outcome = verify(&candidate, &fully_trusting_verifier());
        let BlockedUpdate(receipt) =
            outcome.expect_err("evidence bound to two different artifacts must fail closed");
        assert_eq!(
            receipt.failures,
            vec![FailureCode::InvalidEvidenceIdentity.code()]
        );
    }

    #[test]
    fn malformed_sha256_blocks() {
        let mut candidate = candidate("0.1.5", "0.1.6");
        candidate.artifact_sha256 = "not-a-hash".to_string();
        let outcome = verify(&candidate, &fully_trusting_verifier());
        assert!(
            outcome.is_err(),
            "a non-SHA-256 artifact identity must fail closed"
        );
    }

    #[test]
    fn version_triple_parses_semver_and_tolerates_prerelease_suffixes() {
        assert_eq!(version_triple("0.1.5"), Some((0, 1, 5)));
        assert_eq!(version_triple("2.1.0-rc.1"), Some((2, 1, 0)));
        assert_eq!(
            version_triple("v1.2.3"),
            None,
            "a leading 'v' is not a digit"
        );
        assert_eq!(version_triple("1.2"), None, "patch component is required");
        assert_eq!(version_triple(""), None);
    }

    #[test]
    fn is_upgrade_requires_a_strictly_greater_triple() {
        assert!(is_upgrade("0.1.5", "0.1.6"));
        assert!(is_upgrade("0.1.5", "1.0.0"));
        assert!(
            !is_upgrade("0.1.5", "0.1.5"),
            "equal versions are not an upgrade"
        );
        assert!(
            !is_upgrade("0.2.0", "0.1.9"),
            "a downgrade is not an upgrade"
        );
        assert!(!is_upgrade("0.1.5", "not-a-version"));
    }
}
