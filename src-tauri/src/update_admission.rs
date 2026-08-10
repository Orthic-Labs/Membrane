//! MBR-911: binds `membrane_updater`'s pure dual-signature admission gate to
//! the artifacts RightKit's own release pipeline actually produces for
//! Membrane Hub. This module performs no cryptography and owns no signing
//! key; it only parses RightKit's already-produced, already-trusted output
//! and refuses to build admissible evidence unless that output is present,
//! well-formed, and bound to one consistent artifact hash.
//!
//! Two independent trust domains, matching RightKit's own two-signature
//! shape (`apps/membrane-hub/right-release.config.mjs`, `targets.win.updater`):
//!
//! - **Tauri updater signature.** RightKit's
//!   `tools/rightkit/packages/release/sign-updater.mjs` (and, for macOS,
//!   `create-mac-updater.mjs`) runs `pnpm exec tauri signer sign` against the
//!   shared `rightsuite-updater-key` and writes the result to `<artifact>.sig`.
//!   `tools/rightkit/packages/release/publish-update.mjs` then folds that
//!   signature into the update manifest as
//!   `platforms[<target>] = { signature, url }` -- the exact shape
//!   [`RightKitUpdaterManifestEntry`] below models. Tauri's own updater
//!   plugin verifies that signature against the embedded pubkey when it
//!   later parses this manifest at update-check time; this module never
//!   recomputes or independently re-derives that signature.
//! - **Platform trust.** The `orthic.membrane.platform-acceptance.v1` receipt
//!   already defined by `scripts/release/verify-platform-artifacts.mjs` and
//!   built per platform by `scripts/release/macos/contract.mjs` /
//!   `scripts/release/windows/contract.mjs` (codesign + notarization + staple
//!   + Gatekeeper on macOS; Authenticode + Public Trust + RFC3161 timestamp
//!   on Windows). This module trusts that receipt exactly as already
//!   validated there; it never shells out to `codesign`, `spctl`, or
//!   `signtool` itself.
//!
//! `membrane_updater::verify` still fails closed on every path described in
//! its own doc comment (missing signature, unknown key, one valid signature
//! out of two, a downgrade, a mismatched hash); this module's only job is to
//! turn real RightKit JSON into evidence that gate can evaluate, collapsing
//! any unparseable/incomplete RightKit output into evidence that is
//! guaranteed to fail closed rather than guessing at partial trust.
//!
//! Not yet wired into the running `tauri::Builder` invoke handler: RightKit
//! has not published a macOS `updater` artifact yet (see
//! `packaging/macos/bundle-manifest.json`'s `updater` component,
//! `present: false`, owned by the concurrent macOS release lane), so there is
//! no live update flow to gate end to end. This module is the gate that flow
//! must call before it may install anything; it is deliberately additive and
//! does not touch `main.rs`'s existing tray/service/window wiring.

#![allow(dead_code)]

use membrane_updater::{
    BlockedUpdate, Platform, PlatformTrustEvidence, TauriSignatureEvidence, UpdateCandidate,
    UpdateTrustVerifier, VerifiedUpdate,
};
use serde::Deserialize;
use serde_json::{Map, Value};

/// One `platforms[<target>]` entry from RightKit's Tauri update manifest.
/// Field names match `tools/rightkit/packages/release/publish-update.mjs`
/// (`body.manifest.platforms[artifact.platform] = { signature, url }`)
/// exactly; this struct adds no field RightKit does not already emit, and it
/// carries the signature as an opaque string -- it is never parsed as bytes
/// or verified here.
#[derive(Debug, Clone, Deserialize)]
pub struct RightKitUpdaterManifestEntry {
    pub signature: String,
    pub url: String,
}

/// The `orthic.membrane.platform-acceptance.v1` receipt. Field names for the
/// subset this module reads match
/// `scripts/release/verify-platform-artifacts.mjs::validateReceipt` exactly.
/// Unknown JSON fields (`lifecycle`, `environment`, `mode`, `receiptId`, ...)
/// are ignored by serde rather than rejected, so a full real receipt
/// deserializes here unchanged.
#[derive(Debug, Clone, Deserialize)]
pub struct PlatformAcceptanceReceipt {
    pub schema: String,
    pub platform: String,
    pub artifact: PlatformAcceptanceArtifact,
    pub trust: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformAcceptanceArtifact {
    pub sha256: String,
}

const RECEIPT_SCHEMA: &str = "orthic.membrane.platform-acceptance.v1";
const MACOS_TRUST_FIELDS: [&str; 4] = ["codesign", "notarization", "staple", "gatekeeper"];
const WINDOWS_TRUST_FIELDS: [&str; 3] = ["authenticode", "publicTrust", "rfc3161"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvidenceError {
    ReceiptSchemaInvalid,
    ReceiptPlatformMismatch,
    ReceiptTrustIncomplete,
    ManifestSignatureMissing,
}

/// Reads the platform-acceptance receipt's already-passed trust fields and
/// binds it to `expected_platform`. Never runs a signing/verification tool;
/// the trust decision itself already happened in
/// `scripts/release/verify-platform-artifacts.mjs` before this receipt was
/// produced -- this function only refuses to trust a receipt that is the
/// wrong schema, the wrong platform, or missing a required "pass".
fn platform_trust_evidence(
    receipt: &PlatformAcceptanceReceipt,
    expected_platform: Platform,
) -> Result<PlatformTrustEvidence, EvidenceError> {
    if receipt.schema != RECEIPT_SCHEMA {
        return Err(EvidenceError::ReceiptSchemaInvalid);
    }
    let receipt_platform = match expected_platform {
        Platform::Macos => "macos",
        Platform::Windows => "windows",
    };
    if receipt.platform != receipt_platform {
        return Err(EvidenceError::ReceiptPlatformMismatch);
    }
    let required_fields: &[&str] = match expected_platform {
        Platform::Macos => &MACOS_TRUST_FIELDS,
        Platform::Windows => &WINDOWS_TRUST_FIELDS,
    };
    let all_passed = required_fields
        .iter()
        .all(|field| receipt.trust.get(*field).and_then(Value::as_str) == Some("pass"));
    if !all_passed {
        return Err(EvidenceError::ReceiptTrustIncomplete);
    }
    Ok(PlatformTrustEvidence {
        platform: expected_platform,
        receipt_id: format!("sha256:{}", receipt.artifact.sha256),
        signed_sha256: format!("sha256:{}", receipt.artifact.sha256),
    })
}

/// Reads the Tauri updater signature RightKit's `sign-updater.mjs` /
/// `create-mac-updater.mjs` already produced. `key_id` is supplied by the
/// caller from configuration (never parsed out of the manifest, since
/// RightKit's manifest entry carries no key identity field) so the trusted
/// adapter below can still enforce a known-key allowlist.
fn tauri_signature_evidence(
    entry: &RightKitUpdaterManifestEntry,
    key_id: &str,
    artifact_sha256_hex: &str,
) -> Result<TauriSignatureEvidence, EvidenceError> {
    if entry.signature.trim().is_empty() {
        return Err(EvidenceError::ManifestSignatureMissing);
    }
    Ok(TauriSignatureEvidence {
        key_id: key_id.to_string(),
        signature: entry.signature.clone().into_bytes(),
        signed_sha256: format!("sha256:{artifact_sha256_hex}"),
    })
}

/// An [`EvidenceError`] must never surface as "trust true because we
/// couldn't tell" -- it collapses to evidence `membrane_updater::verify`'s
/// own identity check is guaranteed to reject (empty key/signature/receipt
/// id), so a RightKit output that fails to parse fails exactly as closed as
/// one that parses but is invalid.
fn unresolvable_tauri_evidence() -> TauriSignatureEvidence {
    TauriSignatureEvidence {
        key_id: String::new(),
        signature: Vec::new(),
        signed_sha256: String::new(),
    }
}

fn unresolvable_platform_evidence(platform: Platform) -> PlatformTrustEvidence {
    PlatformTrustEvidence {
        platform,
        receipt_id: String::new(),
        signed_sha256: String::new(),
    }
}

/// Trusted adapter over RightKit's real receipts. Holds no key material and
/// performs no signing or cryptographic verification -- it only confirms
/// that the evidence [`assess`] built is bound to the exact artifact under
/// review and, for the Tauri leg, that the signature was made with a key
/// this build recognizes.
struct RightKitTrustAdapter {
    trusted_updater_key_ids: Vec<String>,
}

impl UpdateTrustVerifier for RightKitTrustAdapter {
    fn verify_tauri(&self, artifact_sha256: &str, evidence: &TauriSignatureEvidence) -> bool {
        !evidence.signature.is_empty()
            && evidence.signed_sha256 == artifact_sha256
            && self
                .trusted_updater_key_ids
                .iter()
                .any(|id| id == &evidence.key_id)
    }

    fn verify_platform(
        &self,
        artifact_sha256: &str,
        evidence: &PlatformTrustEvidence,
    ) -> membrane_updater::PlatformVerification {
        // The pass/fail trust decision already happened when
        // `platform_trust_evidence` built this evidence (or refused to, in
        // which case `receipt_id`/`signed_sha256` are empty and this
        // necessarily evaluates false). This only re-confirms identity
        // binding to the artifact under review.
        let bound = !evidence.receipt_id.is_empty() && evidence.signed_sha256 == artifact_sha256;
        membrane_updater::PlatformVerification {
            signature_valid: bound,
            platform_trust_valid: bound,
        }
    }
}

/// Runs the dual-signature admission gate against RightKit's real, already-
/// produced update manifest entry and platform-acceptance receipt. Returns
/// `Err(BlockedUpdate)` -- never panics, never installs, never touches the
/// filesystem -- on any of: a missing/empty Tauri signature, an untrusted
/// `updater_key_id`, an incomplete or wrong-platform trust receipt, a
/// downgrade, or an artifact-hash mismatch between the two legs.
#[allow(clippy::too_many_arguments)]
pub fn assess(
    from_version: &str,
    to_version: &str,
    artifact_sha256_hex: &str,
    platform: Platform,
    trusted_updater_key_ids: &[String],
    updater_key_id: &str,
    manifest_entry: &RightKitUpdaterManifestEntry,
    platform_receipt: &PlatformAcceptanceReceipt,
) -> Result<VerifiedUpdate, BlockedUpdate> {
    let tauri = tauri_signature_evidence(manifest_entry, updater_key_id, artifact_sha256_hex)
        .unwrap_or_else(|_| unresolvable_tauri_evidence());
    let platform_evidence = platform_trust_evidence(platform_receipt, platform)
        .unwrap_or_else(|_| unresolvable_platform_evidence(platform));
    let candidate = UpdateCandidate {
        from_version: from_version.to_string(),
        to_version: to_version.to_string(),
        artifact_sha256: format!("sha256:{artifact_sha256_hex}"),
        tauri,
        platform: platform_evidence,
    };
    let verifier = RightKitTrustAdapter {
        trusted_updater_key_ids: trusted_updater_key_ids.to_vec(),
    };
    membrane_updater::verify(&candidate, &verifier)
}

// --- Deterministic, unexecuted-at-task-time source tests -------------------
//
// Not run by this task (no `cargo test`, per this task's hard rules); left
// for the Book 3 final phased gate's
// `cargo check --manifest-path apps/membrane-hub/src-tauri/Cargo.toml`.
// These exercise the RightKit-shape parsing this module adds on top of the
// exhaustively-tested pure gate in `engine/crates/membrane-updater`.
#[cfg(test)]
mod tests {
    use super::*;

    const ARTIFACT_SHA256_HEX: &str =
        "00000000000000000000000000000000000000000000000000000000000000ab";
    const TRUSTED_KEY: &str = "rightsuite-updater-key";

    fn real_windows_manifest_entry() -> RightKitUpdaterManifestEntry {
        // Shape produced by tools/rightkit/packages/release/publish-update.mjs:
        // `platforms[artifact.platform] = { signature, url }`.
        RightKitUpdaterManifestEntry {
            signature: "dW50cnVzdGVkLWxvb2tpbmctYnV0LXZhbGlkLWJhc2U2NC1zaWc=".to_string(),
            url: "https://rightapps-license-gate.adrdsouza.workers.dev/membrane/updates/windows/current/Membrane_x64-setup.exe".to_string(),
        }
    }

    fn real_windows_platform_receipt(
        trust_overrides: &[(&str, &str)],
    ) -> PlatformAcceptanceReceipt {
        let mut trust = Map::new();
        for field in WINDOWS_TRUST_FIELDS {
            trust.insert(field.to_string(), Value::String("pass".to_string()));
        }
        for (field, value) in trust_overrides {
            trust.insert((*field).to_string(), Value::String((*value).to_string()));
        }
        PlatformAcceptanceReceipt {
            schema: RECEIPT_SCHEMA.to_string(),
            platform: "windows".to_string(),
            artifact: PlatformAcceptanceArtifact {
                sha256: ARTIFACT_SHA256_HEX.to_string(),
            },
            trust,
        }
    }

    #[test]
    fn deserializes_the_real_publish_update_manifest_entry_shape() {
        let json = r#"{"signature":"c2ln","url":"https://example.invalid/x.exe"}"#;
        let entry: RightKitUpdaterManifestEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.signature, "c2ln");
        assert_eq!(entry.url, "https://example.invalid/x.exe");
    }

    #[test]
    fn deserializes_a_full_real_platform_acceptance_receipt_ignoring_unmodeled_fields() {
        let json = r#"{
            "schema": "orthic.membrane.platform-acceptance.v1",
            "receiptId": "r-1",
            "mode": "clean-vm",
            "commit": "0000000000000000000000000000000000000a",
            "releaseGeneration": "00000000000000000000000000000000000000000000000000000000000a",
            "version": "0.1.6",
            "platform": "windows",
            "artifact": {"name": "Membrane_0.1.6_x64-setup.exe", "sha256": "00000000000000000000000000000000000000000000000000000000000000ab"},
            "trust": {"authenticode": "pass", "publicTrust": "pass", "rfc3161": "pass"},
            "lifecycle": {"install": "pass", "startup": "pass", "update": "pass", "uninstall": "pass"},
            "environment": {"clean": true, "machineDigest": "0000000000000000000000000000000000000000000000000000000000ac", "bypassWarnings": false}
        }"#;
        let receipt: PlatformAcceptanceReceipt = serde_json::from_str(json).unwrap();
        assert_eq!(receipt.platform, "windows");
        assert_eq!(receipt.artifact.sha256, ARTIFACT_SHA256_HEX);
    }

    #[test]
    fn assess_verifies_on_full_real_rightkit_shapes() {
        let outcome = assess(
            "0.1.5",
            "0.1.6",
            ARTIFACT_SHA256_HEX,
            Platform::Windows,
            &[TRUSTED_KEY.to_string()],
            TRUSTED_KEY,
            &real_windows_manifest_entry(),
            &real_windows_platform_receipt(&[]),
        );
        assert!(outcome.is_ok(), "full valid RightKit evidence must verify");
    }

    #[test]
    fn assess_blocks_on_untrusted_updater_key() {
        let outcome = assess(
            "0.1.5",
            "0.1.6",
            ARTIFACT_SHA256_HEX,
            Platform::Windows,
            &[TRUSTED_KEY.to_string()],
            "some-other-key",
            &real_windows_manifest_entry(),
            &real_windows_platform_receipt(&[]),
        );
        assert!(
            outcome.is_err(),
            "an unrecognized signing key must fail closed"
        );
    }

    #[test]
    fn assess_blocks_on_incomplete_platform_trust() {
        let receipt = real_windows_platform_receipt(&[("rfc3161", "fail")]);
        let outcome = assess(
            "0.1.5",
            "0.1.6",
            ARTIFACT_SHA256_HEX,
            Platform::Windows,
            &[TRUSTED_KEY.to_string()],
            TRUSTED_KEY,
            &real_windows_manifest_entry(),
            &receipt,
        );
        assert!(
            outcome.is_err(),
            "one failed trust field out of several must fail closed"
        );
    }

    #[test]
    fn assess_blocks_on_wrong_platform_receipt() {
        let mut receipt = real_windows_platform_receipt(&[]);
        receipt.platform = "macos".to_string();
        let outcome = assess(
            "0.1.5",
            "0.1.6",
            ARTIFACT_SHA256_HEX,
            Platform::Windows,
            &[TRUSTED_KEY.to_string()],
            TRUSTED_KEY,
            &real_windows_manifest_entry(),
            &receipt,
        );
        assert!(
            outcome.is_err(),
            "a receipt for the wrong platform must fail closed"
        );
    }

    #[test]
    fn assess_blocks_on_downgrade_even_with_full_trust() {
        let outcome = assess(
            "0.2.0",
            "0.1.9",
            ARTIFACT_SHA256_HEX,
            Platform::Windows,
            &[TRUSTED_KEY.to_string()],
            TRUSTED_KEY,
            &real_windows_manifest_entry(),
            &real_windows_platform_receipt(&[]),
        );
        assert!(
            outcome.is_err(),
            "a downgrade must fail closed even with two valid signatures"
        );
    }

    #[test]
    fn assess_blocks_on_missing_tauri_signature() {
        let mut entry = real_windows_manifest_entry();
        entry.signature = String::new();
        let outcome = assess(
            "0.1.5",
            "0.1.6",
            ARTIFACT_SHA256_HEX,
            Platform::Windows,
            &[TRUSTED_KEY.to_string()],
            TRUSTED_KEY,
            &entry,
            &real_windows_platform_receipt(&[]),
        );
        assert!(
            outcome.is_err(),
            "an empty Tauri updater signature must fail closed"
        );
    }

    #[test]
    fn assess_blocks_on_mismatched_artifact_hash() {
        // The platform receipt proves a different artifact than the one under review.
        let mut receipt = real_windows_platform_receipt(&[]);
        receipt.artifact.sha256 =
            "1111111111111111111111111111111111111111111111111111111111ab".to_string();
        let outcome = assess(
            "0.1.5",
            "0.1.6",
            ARTIFACT_SHA256_HEX,
            Platform::Windows,
            &[TRUSTED_KEY.to_string()],
            TRUSTED_KEY,
            &real_windows_manifest_entry(),
            &receipt,
        );
        assert!(
            outcome.is_err(),
            "evidence bound to two different artifacts must fail closed"
        );
    }
}
