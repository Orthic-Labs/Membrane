//! Native Rust port of `blueprint/src/lib/init/state-integrity.mjs`.
//!
//! Lane LIB3/CRYPTO (r5 closure): no prior native equivalent found (grep of
//! `sealLocalState`/`verifyLocalState`/`state_integrity_invalid` across
//! membrane-blueprint/src and membrane-runtime/src produced no match).
//! HMAC-SHA256 uses `ring::hmac`, matching Node's `createHmac("sha256", key)`
//! bit-for-bit (RFC 2104); the prior version of this module hand-rolled the
//! same construction directly against `sha2::Sha256` because no HMAC crate
//! was believed available -- `ring` is now a crate dependency, so the
//! hand-rolled implementation has been replaced. Both constructions were
//! verified to agree byte-for-byte on the RFC 4231 test vectors in the
//! parity test for this module (no behavioral bug found in the prior
//! hand-rolled version). Ported behavior: canonicalize (sorted keys,
//! `integrity` key excluded) before tagging; seal writes
//! `{namespace, keyId, tag}`; verify recomputes the tag with a
//! constant-time comparison (via `ring::hmac::verify`) and checks the
//! `keyId` binding.

use ring::hmac;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// HMAC-SHA256 per RFC 2104, via `ring::hmac`.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let signing_key = hmac::Key::new(hmac::HMAC_SHA256, key);
    let tag = hmac::sign(&signing_key, message);
    let mut out = [0u8; 32];
    out.copy_from_slice(tag.as_ref());
    out
}

/// Constant-time HMAC-SHA256 verification via `ring::hmac::verify`.
fn hmac_sha256_verify(key: &[u8], message: &[u8], expected: &[u8]) -> bool {
    let verifying_key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::verify(&verifying_key, message, expected).is_ok()
}

/// Mirrors `canonical(value)`: sorts object keys and drops the top-level
/// (and any nested) `integrity` key before hashing.
pub fn canonical(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            let mut keys: Vec<&String> = map.keys().filter(|k| k.as_str() != "integrity").collect();
            keys.sort();
            for key in keys {
                out.insert(key.clone(), canonical(&map[key]));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn tag(state: &Value, key: &[u8]) -> String {
    let canonical_json = serde_json::to_string(&canonical(state)).unwrap();
    hex::encode(hmac_sha256(key, canonical_json.as_bytes()))
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum StateIntegrityError {
    #[error("state_integrity_invalid")]
    Invalid,
}

/// Mirrors `sealLocalState(root, state, { namespace })`: returns `state`
/// with an `integrity: { namespace, keyId, tag }` field attached. `key` is
/// the 32-byte per-root/namespace key (the legacy module reads/creates it
/// under a platform state directory; key management is left to the caller
/// since this port is pure and filesystem-free).
pub fn seal_local_state(state: &Value, key: &[u8; 32], namespace: &str) -> Value {
    let mut key_hasher = Sha256::new();
    key_hasher.update(key);
    let key_id = hex::encode(key_hasher.finalize())[..16].to_string();
    let computed_tag = tag(state, key);
    let mut out = state.clone();
    if let Value::Object(map) = &mut out {
        map.insert(
            "integrity".to_string(),
            serde_json::json!({ "namespace": namespace, "keyId": key_id, "tag": computed_tag }),
        );
    }
    out
}

/// Mirrors `verifyLocalState(root, state, { namespace })`.
pub fn verify_local_state(
    state: &Value,
    key: &[u8; 32],
    namespace: &str,
) -> Result<(), StateIntegrityError> {
    let integrity = state
        .get("integrity")
        .ok_or(StateIntegrityError::Invalid)?;
    let integrity_namespace = integrity.get("namespace").and_then(Value::as_str);
    let key_id = integrity.get("keyId").and_then(Value::as_str).unwrap_or("");
    let stored_tag = integrity.get("tag").and_then(Value::as_str).unwrap_or("");
    let is_hex16 = key_id.len() == 16 && key_id.chars().all(|c| c.is_ascii_hexdigit());
    let is_hex64 = stored_tag.len() == 64 && stored_tag.chars().all(|c| c.is_ascii_hexdigit());
    if integrity_namespace != Some(namespace) || !is_hex16 || !is_hex64 {
        return Err(StateIntegrityError::Invalid);
    }
    let mut key_hasher = Sha256::new();
    key_hasher.update(key);
    let expected_key_id = hex::encode(key_hasher.finalize())[..16].to_string();
    let stored_bytes = hex::decode(stored_tag).map_err(|_| StateIntegrityError::Invalid)?;
    let canonical_json = serde_json::to_string(&canonical(state)).unwrap();
    if expected_key_id != key_id
        || !hmac_sha256_verify(key, canonical_json.as_bytes(), &stored_bytes)
    {
        return Err(StateIntegrityError::Invalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = i as u8;
        }
        k
    }

    #[test]
    fn hmac_sha256_matches_rfc4231_test_case_1() {
        // RFC 4231 Test Case 1: 20-byte key of 0x0b, data "Hi There".
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        let expected = hex::decode(
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
        )
        .unwrap();
        assert_eq!(hmac_sha256(&key, data).to_vec(), expected);
    }

    #[test]
    fn hmac_sha256_matches_rfc4231_test_case_2_jefe_key() {
        // RFC 4231 Test Case 2: key "Jefe", data "what do ya want for nothing?"
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        let expected = hex::decode(
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
        )
        .unwrap();
        assert_eq!(hmac_sha256(key, data).to_vec(), expected);
    }

    #[test]
    fn hmac_sha256_output_is_a_full_32_byte_digest() {
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        let mac = hmac_sha256(key, data);
        assert_eq!(mac.len(), 32);
        assert_eq!(hex::encode(mac).len(), 64);
    }

    #[test]
    fn hmac_sha256_is_deterministic_and_key_sensitive() {
        let data = b"Hi There";
        let key_a = [0x0bu8; 20];
        let mut key_b = [0x0bu8; 20];
        key_b[0] = 0x0c;
        assert_eq!(hmac_sha256(&key_a, data), hmac_sha256(&key_a, data));
        assert_ne!(hmac_sha256(&key_a, data), hmac_sha256(&key_b, data));
    }

    #[test]
    fn seal_then_verify_round_trips() {
        let state = json!({ "version": 1, "files": {} });
        let sealed = seal_local_state(&state, &key(), "install");
        assert!(sealed.get("integrity").is_some());
        verify_local_state(&sealed, &key(), "install").expect("should verify");
    }

    #[test]
    fn tampering_with_state_breaks_verification() {
        let state = json!({ "version": 1, "files": {} });
        let mut sealed = seal_local_state(&state, &key(), "install");
        sealed["version"] = json!(2);
        let err = verify_local_state(&sealed, &key(), "install").unwrap_err();
        assert_eq!(err, StateIntegrityError::Invalid);
    }

    #[test]
    fn wrong_namespace_fails_verification() {
        let state = json!({ "version": 1, "files": {} });
        let sealed = seal_local_state(&state, &key(), "install");
        let err = verify_local_state(&sealed, &key(), "update").unwrap_err();
        assert_eq!(err, StateIntegrityError::Invalid);
    }

    #[test]
    fn missing_integrity_field_is_invalid() {
        let state = json!({ "version": 1, "files": {} });
        let err = verify_local_state(&state, &key(), "install").unwrap_err();
        assert_eq!(err, StateIntegrityError::Invalid);
    }

    #[test]
    fn canonical_drops_integrity_key_and_sorts_object_keys() {
        let state = json!({ "b": 1, "a": 2, "integrity": { "tag": "x" } });
        let out = canonical(&state);
        assert_eq!(out, json!({ "a": 2, "b": 1 }));
    }
}
