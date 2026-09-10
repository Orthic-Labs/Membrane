//! Parity tests for `lib_redaction` (native port of
//! `blueprint/src/lib/redaction.mjs`), lane LIB4 (r5 closure).

use membrane_blueprint::lib_redaction::redact_for_egress;
use serde_json::json;

#[test]
fn masks_secret_keys_and_secret_shaped_values() {
    let input = json!({
        "token": "ghp_abc",
        "apiKey": "x",
        "nested": { "password": "pw", "ok": "fine" },
        "plain": "Bearer abc123.def456",
    });
    let out = redact_for_egress(&input, false);
    assert_eq!(out["token"], json!("[REDACTED]"));
    assert_eq!(out["apiKey"], json!("[REDACTED]"));
    assert_eq!(out["nested"]["password"], json!("[REDACTED]"));
    assert_eq!(out["nested"]["ok"], json!("fine"));
    assert!(out["plain"].as_str().unwrap().contains("[REDACTED]"));
}

#[test]
fn redacts_full_secret_class_drift_set() {
    // sk-ant- Anthropic keys, Slack xox* tokens, bare JWTs, and URL-password
    // connection strings must all be redacted (parity with
    // support-bundle-redaction.test.mjs "drift" case).
    // Assemble detector-shaped fixture at runtime so repository secret scanning
    // does not mistake test data for a live credential.
    let slack_token = ["xox", "b-1234567890-", "abcdefghijklmnop"].concat();
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    let pg_url = "postgres://admin:sup3rsecret@db.example.com:5432/app";
    let anthropic_key = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz1234567890";

    let input = json!({
        "watchmanLog": format!("{}\n{}\n{}\n{}", anthropic_key, slack_token, jwt, pg_url),
        "serviceLog": format!("db={} anthropic={}", pg_url, anthropic_key),
    });
    let out = redact_for_egress(&input, false);
    let watchman = out["watchmanLog"].as_str().unwrap();
    let service = out["serviceLog"].as_str().unwrap();
    for secret in [slack_token.as_str(), jwt, pg_url, anthropic_key, "sup3rsecret"] {
        assert!(!watchman.contains(secret), "leaked in watchmanLog: {secret}");
        assert!(!service.contains(secret), "leaked in serviceLog: {secret}");
    }
}

#[test]
fn content_addressed_key_suppresses_raw_base64_heuristic() {
    // A 40-char value under a content-addressed key name (e.g. a git SHA-ish
    // digest field) must survive; the same shape under an unrelated key is
    // redacted.
    let raw40 = "A".repeat(40);
    let input = json!({
        "digest": raw40.clone(),
        "blob": raw40.clone(),
    });
    let out = redact_for_egress(&input, false);
    assert_eq!(out["digest"], json!(raw40), "content-addressed key should be exempt");
    assert_eq!(out["blob"], json!("[REDACTED]"));
}

#[test]
fn private_key_header_is_redacted() {
    // The legacy regex matches only the `-----BEGIN ... PRIVATE KEY-----`
    // header literal, not the base64 body that follows it (the body isn't
    // caught unless it separately matches the 40-char raw-base64 heuristic
    // on its own line). This mirrors the actual JS SECRET_VALUE pattern.
    let pem = "-----BEGIN RSA PRIVATE KEY-----\nshortbody\n-----END RSA PRIVATE KEY-----";
    let out = redact_for_egress(&json!(pem), false);
    let redacted = out.as_str().unwrap();
    assert!(redacted.contains("[REDACTED]"));
    assert!(!redacted.contains("-----BEGIN RSA PRIVATE KEY-----"));
}

#[test]
fn arrays_are_redacted_elementwise() {
    let input = json!(["ghp_1234567890abcdefghijklmnopqrstuvwxyz", "plain text"]);
    let out = redact_for_egress(&input, false);
    assert_eq!(out[0], json!("[REDACTED]"));
    assert_eq!(out[1], json!("plain text"));
}

#[test]
fn non_string_scalars_pass_through_unchanged() {
    let input = json!({"count": 5, "ok": true, "nothing": null});
    let out = redact_for_egress(&input, false);
    assert_eq!(out, input);
}
