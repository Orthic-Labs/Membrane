//! Parity test for `blueprint/src/lib/update/manifest.mjs`, including
//! known-answer Ed25519 verification vectors generated with Node's
//! `node:crypto` (`generateKeyPairSync('ed25519')` + `sign(null, ...)`,
//! matching the legacy `verify(null, ...)` call exactly) and pinned here.

use membrane_blueprint::lib_update_manifest::{
    canonical_manifest_payload, parse_trusted_update_keys, reject_downgrade, tree_digest,
    verify_artifact_checksum, verify_signed_manifest_structural, SignatureCheck,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;

#[test]
fn canonical_payload_excludes_signature_and_is_deterministic() {
    let manifest = json!({
        "version": "2.0.0",
        "signature": "should-not-appear",
        "channel": "stable",
    });
    let payload = canonical_manifest_payload(&manifest);
    assert!(!payload.contains("should-not-appear"));
    assert_eq!(payload, r#"{"channel":"stable","version":"2.0.0"}"#);
}

#[test]
fn tree_digest_changes_when_any_file_content_changes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.js"), "console.log(1)").unwrap();
    let before = tree_digest(dir.path()).unwrap();
    fs::write(dir.path().join("main.js"), "console.log(2)").unwrap();
    let after = tree_digest(dir.path()).unwrap();
    assert_ne!(before, after);
}

#[test]
fn verify_artifact_checksum_requires_exact_sha256_match() {
    let manifest = json!({ "artifacts": [{ "name": "linux-x64", "sha256": "feedface" }] });
    assert!(verify_artifact_checksum(&manifest, "linux-x64", "feedface").is_ok());
    assert_eq!(
        verify_artifact_checksum(&manifest, "linux-x64", "beefbeef"),
        Err("checksum_mismatch")
    );
}

#[test]
fn reject_downgrade_blocks_any_component_regression_and_exact_replay() {
    assert!(reject_downgrade("3.0.0", "2.9.9").is_ok());
    assert_eq!(reject_downgrade("1.9.9", "2.0.0"), Err("downgrade_major"));
    assert_eq!(reject_downgrade("2.0.0", "2.0.0"), Err("replay_version"));
}

#[test]
fn parse_trusted_update_keys_accepts_a_well_formed_root() {
    let raw = json!({
        "schemaVersion": 1,
        "keys": [{
            "keyId": "2026-key",
            "algorithm": "Ed25519",
            "publicKey": "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA\n-----END PUBLIC KEY-----\n",
        }],
    })
    .to_string();
    let keys = parse_trusted_update_keys(&raw).unwrap();
    assert_eq!(keys.get("2026-key").map(String::as_str), Some("-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA\n-----END PUBLIC KEY-----\n"));
}

#[test]
fn verify_signed_manifest_structural_rejects_garbage_signature_against_bogus_key() {
    // "pem" is not a real key, so this can never verify -- must reject, not crash.
    let mut trusted = BTreeMap::new();
    trusted.insert("k1".to_string(), "pem".to_string());
    let manifest = json!({
        "signatureAlgorithm": "Ed25519",
        "keyId": "k1",
        "signature": "AQID",
    });
    let result = verify_signed_manifest_structural(&manifest, &trusted);
    assert_ne!(result, SignatureCheck::Ok);
}

#[test]
fn empty_trust_root_is_rejected_before_anything_else() {
    let manifest = json!({ "signatureAlgorithm": "Ed25519", "keyId": "k1", "signature": "AQID" });
    let result = verify_signed_manifest_structural(&manifest, &BTreeMap::new());
    assert_eq!(result, SignatureCheck::Reason("update_trust_root_missing"));
}

// --- Known-answer Ed25519 vector -------------------------------------
//
// Generated with:
//   node -e "
//     const { generateKeyPairSync, sign } = require('node:crypto');
//     const { publicKey, privateKey } = generateKeyPairSync('ed25519');
//     const pubPem = publicKey.export({ type: 'spki', format: 'pem' });
//     const manifest = { schemaVersion: 1, keyId: 'test-key-1',
//       signatureAlgorithm: 'Ed25519', version: '1.2.3', commit: 'abc123',
//       artifacts: [{ name: 'win-x64', sha256: 'deadbeef' }] };
//     function canonical(v) {
//       if (Array.isArray(v)) return v.map(canonical);
//       if (v && typeof v === 'object')
//         return Object.fromEntries(Object.keys(v).sort().map(k => [k, canonical(v[k])]));
//       return v;
//     }
//     const payload = JSON.stringify(canonical(manifest));
//     console.log(pubPem, sign(null, Buffer.from(payload), privateKey).toString('base64'));
//   "
// This exercises the real `ring`-backed Ed25519 math end to end (not a
// structural-only check): a bit-flipped signature or payload must fail.

const TEST_PUBLIC_KEY_PEM: &str =
    "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA5yQeGdy5gJay4BXYMyJh5wSoYd0hWy0FdqCYhH1YFg4=\n-----END PUBLIC KEY-----\n";
const TEST_SIGNATURE_B64: &str =
    "KbXXIyoHhxzVyBXvaPyXOjOwFC8TicI8EWwpsSZFSILxRSDLzL90y/BJTAQblL7eHVIVhN22HsrAv93vKx3ADg==";

fn test_manifest(signature: &str) -> serde_json::Value {
    json!({
        "schemaVersion": 1,
        "keyId": "test-key-1",
        "signatureAlgorithm": "Ed25519",
        "version": "1.2.3",
        "commit": "abc123",
        "artifacts": [{ "name": "win-x64", "sha256": "deadbeef" }],
        "signature": signature,
    })
}

#[test]
fn verify_signed_manifest_structural_accepts_a_real_ed25519_signature() {
    let mut trusted = BTreeMap::new();
    trusted.insert("test-key-1".to_string(), TEST_PUBLIC_KEY_PEM.to_string());
    let manifest = test_manifest(TEST_SIGNATURE_B64);
    assert_eq!(
        verify_signed_manifest_structural(&manifest, &trusted),
        SignatureCheck::Ok
    );
}

#[test]
fn verify_signed_manifest_structural_rejects_tampered_payload_under_a_valid_signature() {
    let mut trusted = BTreeMap::new();
    trusted.insert("test-key-1".to_string(), TEST_PUBLIC_KEY_PEM.to_string());
    let mut manifest = test_manifest(TEST_SIGNATURE_B64);
    manifest["commit"] = json!("tampered");
    assert_eq!(
        verify_signed_manifest_structural(&manifest, &trusted),
        SignatureCheck::Reason("invalid_signature")
    );
}

#[test]
fn verify_signed_manifest_structural_rejects_a_bit_flipped_signature() {
    let mut trusted = BTreeMap::new();
    trusted.insert("test-key-1".to_string(), TEST_PUBLIC_KEY_PEM.to_string());
    // Flip the first base64 character so the decoded signature bytes differ.
    let tampered_sig = format!("L{}", &TEST_SIGNATURE_B64[1..]);
    let manifest = test_manifest(&tampered_sig);
    assert_eq!(
        verify_signed_manifest_structural(&manifest, &trusted),
        SignatureCheck::Reason("invalid_signature")
    );
}
