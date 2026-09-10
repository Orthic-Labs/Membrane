//! Parity test for `blueprint/src/lib/init/state-integrity.mjs`.

use membrane_blueprint::lib_init_state_integrity::{
    canonical, hmac_sha256, seal_local_state, verify_local_state, StateIntegrityError,
};
use serde_json::json;

#[test]
fn hmac_sha256_matches_rfc4231_known_answer_vector() {
    // RFC 4231 Test Case 1, cross-checked against ring::hmac replacing the
    // prior hand-rolled construction; the two agreed byte-for-byte.
    let key = [0x0bu8; 20];
    let expected =
        hex::decode("b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7").unwrap();
    assert_eq!(hmac_sha256(&key, b"Hi There").to_vec(), expected);
}

fn key() -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = (i * 7) as u8;
    }
    k
}

#[test]
fn seal_then_verify_round_trips_for_install_namespace() {
    let state = json!({ "version": 1, "files": {} });
    let sealed = seal_local_state(&state, &key(), "install");
    assert!(sealed["integrity"]["namespace"] == "install");
    verify_local_state(&sealed, &key(), "install").expect("seal output must verify");
}

#[test]
fn seal_then_verify_round_trips_for_update_namespace() {
    let state = json!({ "phase": "prepared", "appDir": "/repo/app" });
    let sealed = seal_local_state(&state, &key(), "update");
    verify_local_state(&sealed, &key(), "update").expect("seal output must verify");
}

#[test]
fn any_tamper_after_sealing_is_detected() {
    let state = json!({ "version": 1, "files": { "a": 1 } });
    let mut sealed = seal_local_state(&state, &key(), "install");
    sealed["files"]["a"] = json!(2);
    assert_eq!(
        verify_local_state(&sealed, &key(), "install").unwrap_err(),
        StateIntegrityError::Invalid
    );
}

#[test]
fn wrong_key_fails_verification() {
    let state = json!({ "version": 1, "files": {} });
    let sealed = seal_local_state(&state, &key(), "install");
    let mut other_key = key();
    other_key[0] ^= 0xFF;
    assert_eq!(
        verify_local_state(&sealed, &other_key, "install").unwrap_err(),
        StateIntegrityError::Invalid
    );
}

#[test]
fn canonicalization_excludes_integrity_and_sorts_keys_recursively() {
    let value = json!({
        "z": { "y": 1, "x": 2 },
        "a": 1,
        "integrity": { "tag": "should-be-dropped" },
    });
    let out = canonical(&value);
    assert_eq!(out, json!({ "a": 1, "z": { "x": 2, "y": 1 } }));
}
