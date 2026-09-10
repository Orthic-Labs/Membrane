//! Native Rust port of `blueprint/src/lib/update/manifest.mjs`.
//!
//! Lane LIB3/CRYPTO (r5 closure): no prior native equivalent found (grep of
//! `verifySignedManifest`/`treeDigest`/`rejectDowngrade` across
//! membrane-blueprint/src and membrane-runtime/src produced no match).
//!
//! `verify_signed_manifest_structural` performs the real Ed25519
//! point-verification math via `ring::signature::UnparsedPublicKey` with the
//! `ED25519` algorithm, using the same canonicalization bytes as the legacy
//! `verifySignedManifest` (`canonicalManifestPayload`, the manifest with
//! `signature` dropped, sorted-key JSON). The trusted public keys are stored
//! as SPKI PEM (as produced by Node's `generateKeyPairSync('ed25519')` and
//! validated by `parse_trusted_update_keys`); `pem_to_raw_ed25519_public_key`
//! extracts the 32-byte raw public key from the fixed 12-byte Ed25519 SPKI
//! DER prefix (`302a300506032b6570032100`, RFC 8410) that Node always
//! produces for this key type. The canonical trusted-key root
//! (`trusted-update-keys.json`) is copied verbatim from
//! `blueprint/src/lib/update/trusted-update-keys.json` and embedded via
//! `include_str!` below. All other exports (`canonical_manifest_payload`,
//! `tree_digest`, `verify_artifact_checksum`, `reject_downgrade`,
//! `load_trusted_update_keys` parsing) are fully ported and match the
//! legacy contract exactly.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Verbatim copy of `blueprint/src/lib/update/trusted-update-keys.json`.
pub const TRUSTED_UPDATE_KEYS_JSON: &str = include_str!("trusted-update-keys.json");

/// Fixed 12-byte SPKI DER prefix for an Ed25519 public key (RFC 8410):
/// `SEQUENCE { SEQUENCE { OID id-Ed25519 } BIT STRING (32 bytes) }` header.
/// Node's `KeyObject.export({ type: "spki", format: "der" })` always emits
/// this exact prefix for `ed25519` keys, followed by the 32 raw key bytes
/// (total DER length 44).
const ED25519_SPKI_DER_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

/// Extract the 32-byte raw Ed25519 public key from a PEM-encoded SPKI block
/// (`-----BEGIN PUBLIC KEY-----` ... `-----END PUBLIC KEY-----`).
fn pem_to_raw_ed25519_public_key(pem: &str) -> Option<[u8; 32]> {
    let body: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let der = base64_decode(body.trim())?;
    if der.len() != 44 || der[..12] != ED25519_SPKI_DER_PREFIX {
        return None;
    }
    let mut raw = [0u8; 32];
    raw.copy_from_slice(&der[12..]);
    Some(raw)
}

fn canonical(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                out.insert(key.clone(), canonical(&map[key]));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Mirrors `canonicalManifestPayload(manifest)`: drops the `signature`
/// field, then canonicalizes and serializes deterministically.
pub fn canonical_manifest_payload(manifest: &Value) -> String {
    let mut unsigned = manifest.clone();
    if let Value::Object(map) = &mut unsigned {
        map.remove("signature");
    }
    serde_json::to_string(&canonical(&unsigned)).unwrap()
}

#[derive(Debug, Clone, PartialEq)]
pub enum SignatureCheck {
    Ok,
    Reason(&'static str),
}

/// Mirrors `verifySignedManifest(manifest, { trustedKeys })`, including the
/// real Ed25519 mathematical verification via `ring`.
pub fn verify_signed_manifest_structural(
    manifest: &Value,
    trusted_keys: &BTreeMap<String, String>,
) -> SignatureCheck {
    if trusted_keys.is_empty() {
        return SignatureCheck::Reason("update_trust_root_missing");
    }
    let algorithm = manifest.get("signatureAlgorithm").and_then(Value::as_str);
    let key_id = manifest.get("keyId").and_then(Value::as_str);
    if algorithm != Some("Ed25519") || key_id.is_none() {
        return SignatureCheck::Reason("invalid_signature_metadata");
    }
    let key_id = key_id.unwrap();
    let Some(public_key_pem) = trusted_keys.get(key_id) else {
        return SignatureCheck::Reason("untrusted_key_id");
    };
    let signature = manifest.get("signature").and_then(Value::as_str);
    let Some(signature) = signature else {
        return SignatureCheck::Reason("invalid_signature");
    };
    let is_base64_shaped = !signature.is_empty()
        && signature
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=');
    if !is_base64_shaped {
        return SignatureCheck::Reason("invalid_signature");
    }
    let signature_bytes = match base64_decode(signature) {
        Some(bytes) if !bytes.is_empty() && base64_encode(&bytes) == signature => bytes,
        _ => return SignatureCheck::Reason("invalid_signature"),
    };
    let Some(raw_public_key) = pem_to_raw_ed25519_public_key(public_key_pem) else {
        return SignatureCheck::Reason("invalid_signature");
    };
    let payload = canonical_manifest_payload(manifest);
    let verifying_key =
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &raw_public_key);
    match verifying_key.verify(payload.as_bytes(), &signature_bytes) {
        Ok(()) => SignatureCheck::Ok,
        Err(_) => SignatureCheck::Reason("invalid_signature"),
    }
}

/// Verify a manifest while preserving the legacy distinction between a
/// missing trust root (`null`) and a present but empty root (`{}`).
pub fn verify_signed_manifest(
    manifest: &Value,
    trusted_keys: Option<&BTreeMap<String, String>>,
) -> SignatureCheck {
    let Some(trusted_keys) = trusted_keys else {
        return SignatureCheck::Reason("update_trust_root_missing");
    };
    if trusted_keys.is_empty() {
        let key_id = manifest.get("keyId").and_then(Value::as_str);
        if manifest.get("signatureAlgorithm").and_then(Value::as_str) != Some("Ed25519")
            || key_id.is_none()
        {
            return SignatureCheck::Reason("invalid_signature_metadata");
        }
        return SignatureCheck::Reason("untrusted_key_id");
    }
    verify_signed_manifest_structural(manifest, trusted_keys)
}

/// Validate the UpdateManifestV1 contract before any signature or artifact
/// decision. This mirrors the legacy `validateContract` call in
/// `loadUpdateManifest` and intentionally allows the schema's extension
/// fields and additional artifact fields.
pub fn validate_update_manifest(manifest: &Value) -> Result<(), &'static str> {
    let Value::Object(object) = manifest else {
        return Err("invalid_update_manifest");
    };
    const ALLOWED: &[&str] = &[
        "schemaVersion", "channel", "version", "commit", "publishedAt", "artifacts",
        "signatureAlgorithm", "keyId", "signature", "details", "properties", "extensions",
    ];
    if object.keys().any(|key| !ALLOWED.contains(&key.as_str())) {
        return Err("invalid_update_manifest");
    }
    if !is_schema_version_one(object.get("schemaVersion"))
        || !matches!(object.get("channel").and_then(Value::as_str), Some("stable" | "beta" | "nightly"))
        || object.get("version").and_then(Value::as_str).is_none_or(str::is_empty)
        || object.get("commit").and_then(Value::as_str).is_none_or(str::is_empty)
        || object.get("publishedAt").and_then(Value::as_str).is_none()
        || !object.get("artifacts").is_some_and(Value::is_array)
        || object.get("signatureAlgorithm").and_then(Value::as_str) != Some("Ed25519")
        || object.get("keyId").and_then(Value::as_str).is_none_or(|id| {
            id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
        || object.get("signature").and_then(Value::as_str).is_none_or(str::is_empty)
    {
        return Err("invalid_update_manifest");
    }
    for field in ["details", "properties", "extensions"] {
        if let Some(value) = object.get(field) {
            if !value.is_object() {
                return Err("invalid_update_manifest");
            }
        }
    }
    Ok(())
}

// Minimal standard-alphabet base64 codec (no new dependency required: only
// used for the structural well-formedness check above).
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[((n >> 18) & 63) as usize] as char);
        out.push(B64[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { B64[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let clean: Vec<u8> = input.bytes().filter(|&b| b != b'=').collect();
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut bits = 0;
    for b in clean {
        let val = B64.iter().position(|&c| c == b)? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

/// Mirrors `treeDigest(root)`: hashes each safe relative name and exact
/// file bytes, in lexical order; refuses symlinks and non-regular entries.
pub fn tree_digest(root: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(root).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("artifact must be a directory".to_string());
    }
    let base = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    visit_tree(&base, &base, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

fn visit_tree(base: &Path, dir: &Path, hasher: &mut Sha256) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() || (!meta.is_dir() && !meta.is_file()) {
            return Err(format!("unsafe artifact entry: {}", path.display()));
        }
        if meta.is_dir() {
            visit_tree(base, &path, hasher)?;
        } else {
            let rel = path
                .strip_prefix(base)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if rel.is_empty() || rel.starts_with("../") || rel.contains("/../") {
                return Err(format!("unsafe artifact path: {}", path.display()));
            }
            let bytes = fs::read(&path).map_err(|e| e.to_string())?;
            hasher.update(rel.as_bytes());
            hasher.update([0u8]);
            hasher.update(&bytes);
            hasher.update([0u8]);
        }
    }
    Ok(())
}

/// Mirrors `verifyArtifactChecksum(manifest, artifactName, actualSha256)`.
pub fn verify_artifact_checksum(
    manifest: &Value,
    artifact_name: &str,
    actual_sha256: &str,
) -> Result<(), &'static str> {
    let artifacts = manifest.get("artifacts").and_then(Value::as_array);
    let artifact = artifacts
        .and_then(|arr| arr.iter().find(|a| a.get("name").and_then(Value::as_str) == Some(artifact_name)));
    let Some(artifact) = artifact else {
        return Err("artifact_not_in_manifest");
    };
    if artifact.get("sha256").and_then(Value::as_str) != Some(actual_sha256) {
        return Err("checksum_mismatch");
    }
    Ok(())
}

fn parse_version(version: &str) -> (i64, i64, i64) {
    let mut parts = version.split('.').map(|p| p.parse::<i64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

/// Mirrors `rejectDowngrade(candidateVersion, currentVersion)`.
pub fn reject_downgrade(candidate_version: &str, current_version: &str) -> Result<(), &'static str> {
    let current = parse_version(current_version);
    let candidate = parse_version(candidate_version);
    if candidate.0 < current.0 {
        return Err("downgrade_major");
    }
    if candidate.0 == current.0 && candidate.1 < current.1 {
        return Err("downgrade_minor");
    }
    if candidate.0 == current.0 && candidate.1 == current.1 && candidate.2 < current.2 {
        return Err("downgrade_patch");
    }
    if candidate == current {
        return Err("replay_version");
    }
    Ok(())
}

/// Mirrors `loadTrustedUpdateKeys(path)`'s parsing/validation (the read
/// itself is left to the caller so this stays filesystem-injectable).
pub fn parse_trusted_update_keys(raw: &str) -> Result<BTreeMap<String, String>, &'static str> {
    let parsed: Value = serde_json::from_str(raw).map_err(|_| "update_trust_root_corrupt")?;
    let obj = parsed.as_object().ok_or("update_trust_root_corrupt")?;
    if obj.len() != 2 || !is_schema_version_one(obj.get("schemaVersion")) {
        return Err("update_trust_root_corrupt");
    }
    let keys = obj.get("keys").and_then(Value::as_array).ok_or("update_trust_root_corrupt")?;
    let mut out = BTreeMap::new();
    for entry in keys {
        let obj = entry.as_object().ok_or("update_trust_root_corrupt")?;
        let mut field_names: Vec<&String> = obj.keys().collect();
        field_names.sort();
        let expected: Vec<&str> = vec!["algorithm", "keyId", "publicKey"];
        if field_names != expected {
            return Err("update_trust_root_corrupt");
        }
        let key_id = obj.get("keyId").and_then(Value::as_str).ok_or("update_trust_root_corrupt")?;
        let algorithm = obj.get("algorithm").and_then(Value::as_str).ok_or("update_trust_root_corrupt")?;
        let public_key = obj.get("publicKey").and_then(Value::as_str).ok_or("update_trust_root_corrupt")?;
        let key_id_valid = !key_id.is_empty()
            && key_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
        let pem_valid = regex::Regex::new(
            r"^-----BEGIN PUBLIC KEY-----\r?\n[\s\S]+\r?\n-----END PUBLIC KEY-----\r?\n?$",
        )
        .is_ok_and(|pattern| pattern.is_match(public_key));
        if !key_id_valid || algorithm != "Ed25519" || !pem_valid || out.contains_key(key_id) {
            return Err("update_trust_root_corrupt");
        }
        out.insert(key_id.to_string(), public_key.to_string());
    }
    Ok(out)
}

fn is_schema_version_one(value: Option<&Value>) -> bool {
    value.is_some_and(|value| value.as_i64() == Some(1) || value.as_f64() == Some(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn canonical_manifest_payload_drops_signature_and_sorts_keys() {
        let manifest = json!({ "b": 1, "a": 2, "signature": "abc" });
        let payload = canonical_manifest_payload(&manifest);
        assert_eq!(payload, r#"{"a":2,"b":1}"#);
    }

    #[test]
    fn tree_digest_is_order_independent_and_content_sensitive() {
        let dir1 = tempfile::tempdir().unwrap();
        fs::write(dir1.path().join("b.txt"), "hello").unwrap();
        fs::write(dir1.path().join("a.txt"), "world").unwrap();
        let d1 = tree_digest(dir1.path()).unwrap();

        let dir2 = tempfile::tempdir().unwrap();
        fs::write(dir2.path().join("a.txt"), "world").unwrap();
        fs::write(dir2.path().join("b.txt"), "hello").unwrap();
        let d2 = tree_digest(dir2.path()).unwrap();
        assert_eq!(d1, d2);

        fs::write(dir2.path().join("a.txt"), "WORLD").unwrap();
        let d3 = tree_digest(dir2.path()).unwrap();
        assert_ne!(d1, d3);
    }

    #[test]
    fn verify_artifact_checksum_matches_or_rejects() {
        let manifest = json!({ "artifacts": [{ "name": "win-x64", "sha256": "abcd" }] });
        assert!(verify_artifact_checksum(&manifest, "win-x64", "abcd").is_ok());
        assert_eq!(
            verify_artifact_checksum(&manifest, "win-x64", "wrong"),
            Err("checksum_mismatch")
        );
        assert_eq!(
            verify_artifact_checksum(&manifest, "missing", "abcd"),
            Err("artifact_not_in_manifest")
        );
    }

    #[test]
    fn reject_downgrade_covers_major_minor_patch_and_replay() {
        assert_eq!(reject_downgrade("1.0.0", "2.0.0"), Err("downgrade_major"));
        assert_eq!(reject_downgrade("2.0.0", "2.1.0"), Err("downgrade_minor"));
        assert_eq!(reject_downgrade("2.1.0", "2.1.1"), Err("downgrade_patch"));
        assert_eq!(reject_downgrade("2.1.1", "2.1.1"), Err("replay_version"));
        assert!(reject_downgrade("2.1.2", "2.1.1").is_ok());
    }

    #[test]
    fn parse_trusted_update_keys_validates_shape() {
        let raw = json!({
            "schemaVersion": 1,
            "keys": [{
                "keyId": "key-1",
                "algorithm": "Ed25519",
                "publicKey": "-----BEGIN PUBLIC KEY-----\nabc\n-----END PUBLIC KEY-----\n",
            }],
        })
        .to_string();
        let keys = parse_trusted_update_keys(&raw).unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys.contains_key("key-1"));
    }

    #[test]
    fn parse_trusted_update_keys_rejects_malformed_root() {
        assert_eq!(parse_trusted_update_keys("not json"), Err("update_trust_root_corrupt"));
        assert_eq!(
            parse_trusted_update_keys(&json!({ "schemaVersion": 2, "keys": [] }).to_string()),
            Err("update_trust_root_corrupt")
        );
    }

    #[test]
    fn verify_signed_manifest_structural_rejects_structurally_valid_but_wrong_signature() {
        let keys: BTreeMap<String, String> = [("key-1".to_string(), "pem".to_string())].into_iter().collect();
        let manifest = json!({
            "signatureAlgorithm": "Ed25519",
            "keyId": "key-1",
            "signature": base64_encode(b"not-a-real-signature"),
        });
        // Structurally valid base64 shape but not a valid Ed25519 signature
        // against a garbage "pem" key: must never claim success.
        assert_eq!(
            verify_signed_manifest_structural(&manifest, &keys),
            SignatureCheck::Reason("invalid_signature")
        );
    }

    #[test]
    fn verify_signed_manifest_structural_rejects_untrusted_key() {
        let keys: BTreeMap<String, String> = [("key-1".to_string(), "pem".to_string())].into_iter().collect();
        let manifest = json!({
            "signatureAlgorithm": "Ed25519",
            "keyId": "unknown-key",
            "signature": base64_encode(b"sig"),
        });
        assert_eq!(
            verify_signed_manifest_structural(&manifest, &keys),
            SignatureCheck::Reason("untrusted_key_id")
        );
    }

    #[test]
    fn verify_signed_manifest_structural_rejects_wrong_algorithm() {
        let keys: BTreeMap<String, String> = [("key-1".to_string(), "pem".to_string())].into_iter().collect();
        let manifest = json!({
            "signatureAlgorithm": "RSA",
            "keyId": "key-1",
            "signature": base64_encode(b"sig"),
        });
        assert_eq!(
            verify_signed_manifest_structural(&manifest, &keys),
            SignatureCheck::Reason("invalid_signature_metadata")
        );
    }
}
