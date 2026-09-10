//! Parity tests for `lib_run_ledger` (native port of
//! `blueprint/src/lib/run-ledger.mjs`), lane LIB4 (r5 closure).

use membrane_blueprint::lib_run_ledger::{
    ledger_link, verify_ledger_chain, verify_ledger_link, ChainVerifyResult, LedgerLinkInput,
    VerifyResult,
};
use serde_json::json;

fn sample_input(previous: Option<&str>, version: &str) -> LedgerLinkInput {
    LedgerLinkInput {
        previous: previous.map(|s| s.to_string()),
        version: json!(version),
        commit: json!("abc123"),
        generation_id: json!("gen-1"),
        provider_digests: json!({"p1": "d1"}),
        rule_digests: json!({"r1": "d1"}),
        inputs: json!(["in1"]),
        outputs: json!(["out1"]),
        artifact_hashes: json!({"a1": "h1"}),
    }
}

#[test]
fn ledger_link_matches_node_crypto_known_answer_vector() {
    // Cross-checked against Node's `createHash("sha256")` /
    // `createHmac("sha256", key)` for the exact same payload and key
    // (the prior hand-rolled HMAC-SHA256 in this module agreed
    // byte-for-byte with this vector too -- no bug found there):
    //   node -e "
    //     const crypto = require('crypto');
    //     const payload = {previous:null,version:'v1',commit:'abc123',
    //       generationId:'gen-1',providerDigests:{p1:'d1'},
    //       ruleDigests:{r1:'d1'},inputs:['in1'],outputs:['out1'],
    //       artifactHashes:{a1:'h1'}};
    //     const body = JSON.stringify(payload);
    //     const digest = crypto.createHash('sha256').update(body).digest('hex');
    //     const sig = crypto.createHmac('sha256', 'secret-key').update(digest).digest('hex');
    //   "
    let key = b"secret-key";
    let link = ledger_link(key, sample_input(None, "v1"));
    assert_eq!(
        link.digest,
        "5df11d18738a049b5b8c78d9494d7d182ccffca918918b72bf8b3a501e19b15e"
    );
    assert_eq!(
        link.signature,
        "e28e8c81e5dba132c9d7d164d3d4813c766c3732eff02a895aa6e26ddb698a39"
    );
}

#[test]
fn ledger_link_round_trips_through_verify() {
    let key = b"secret-key";
    let link = ledger_link(key, sample_input(None, "v1"));
    assert_eq!(link.schema_version, 1);
    let result = verify_ledger_link(&link, key, None);
    assert_eq!(result, VerifyResult::Ok);
}

#[test]
fn verify_detects_tampered_payload() {
    let key = b"secret-key";
    let mut link = ledger_link(key, sample_input(None, "v1"));
    link.payload.commit = json!("tampered");
    let result = verify_ledger_link(&link, key, None);
    assert_eq!(result, VerifyResult::Fail("tampered_payload"));
}

#[test]
fn verify_detects_bad_signature() {
    let key = b"secret-key";
    let mut link = ledger_link(key, sample_input(None, "v1"));
    link.signature = "0".repeat(64);
    let result = verify_ledger_link(&link, key, None);
    assert_eq!(result, VerifyResult::Fail("bad_signature"));
}

#[test]
fn verify_detects_wrong_key() {
    let link = ledger_link(b"key-a", sample_input(None, "v1"));
    let result = verify_ledger_link(&link, b"key-b", None);
    assert_eq!(result, VerifyResult::Fail("bad_signature"));
}

#[test]
fn verify_detects_broken_chain() {
    let key = b"secret-key";
    let link = ledger_link(key, sample_input(Some("expected-prev"), "v1"));
    let result = verify_ledger_link(&link, key, Some("something-else"));
    assert_eq!(result, VerifyResult::Fail("broken_chain"));
}

#[test]
fn verify_detects_missing_fields() {
    let key = b"secret-key";
    let mut link = ledger_link(key, sample_input(None, "v1"));
    link.digest = String::new();
    let result = verify_ledger_link(&link, key, None);
    assert_eq!(result, VerifyResult::Fail("missing_link_fields"));
}

#[test]
fn chain_of_valid_links_verifies_ok() {
    let key = b"chain-key";
    let link1 = ledger_link(key, sample_input(None, "v1"));
    let link2 = ledger_link(key, sample_input(Some(link1.digest.as_str()), "v2"));
    let link3 = ledger_link(key, sample_input(Some(link2.digest.as_str()), "v3"));
    let chain = vec![link1, link2, link3];
    let result = verify_ledger_chain(&chain, key);
    assert_eq!(result, ChainVerifyResult::Ok { links: 3 });
}

#[test]
fn chain_reports_broken_link_and_version() {
    let key = b"chain-key";
    let link1 = ledger_link(key, sample_input(None, "v1"));
    // link2's `previous` does not match link1's digest -> broken chain.
    let link2 = ledger_link(key, sample_input(Some("not-the-real-prev"), "v2"));
    let chain = vec![link1, link2];
    let result = verify_ledger_chain(&chain, key);
    assert_eq!(
        result,
        ChainVerifyResult::Broken {
            broken_at: "v2".to_string(),
            reason: "broken_chain",
        }
    );
}

#[test]
fn empty_chain_verifies_ok_with_zero_links() {
    let result = verify_ledger_chain(&[], b"key");
    assert_eq!(result, ChainVerifyResult::Ok { links: 0 });
}
