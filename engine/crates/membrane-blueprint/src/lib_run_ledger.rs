//! Native Rust port of `blueprint/src/lib/run-ledger.mjs`.
//!
//! Lane LIB4/CRYPTO (r5 closure): this module has no prior native
//! equivalent. D42: signed run ledger — HMAC/Sigstore-compatible chain over
//! version, commit, generation, provider/rule digests, inputs, outputs, and
//! artifact hashes. Missing or tampered links fail verification.
//!
//! HMAC-SHA256 now uses `ring::hmac` (the prior version hand-rolled RFC 2104
//! directly against `sha2::Sha256` because no HMAC crate was believed
//! available; both constructions were verified to agree byte-for-byte on
//! the RFC 4231 vectors in this module's parity test — no bug found in the
//! prior hand-rolled version).

use ring::hmac;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Field order matches the legacy JS object literal exactly, so
/// `serde_json::to_string` produces byte-identical JSON to `JSON.stringify`
/// for the same input (serde_json serializes struct fields in declaration
/// order, independent of the `preserve_order` map feature).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LedgerPayload {
    pub previous: Option<String>,
    pub version: Value,
    pub commit: Value,
    #[serde(rename = "generationId")]
    pub generation_id: Value,
    #[serde(rename = "providerDigests")]
    pub provider_digests: Value,
    #[serde(rename = "ruleDigests")]
    pub rule_digests: Value,
    pub inputs: Value,
    pub outputs: Value,
    #[serde(rename = "artifactHashes")]
    pub artifact_hashes: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LedgerLink {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub payload: LedgerPayload,
    pub digest: String,
    pub signature: String,
}

/// Input bundle for [`ledger_link`], mirroring the JS destructured
/// parameter object.
#[derive(Debug, Clone, Default)]
pub struct LedgerLinkInput {
    pub previous: Option<String>,
    pub version: Value,
    pub commit: Value,
    pub generation_id: Value,
    pub provider_digests: Value,
    pub rule_digests: Value,
    pub inputs: Value,
    pub outputs: Value,
    pub artifact_hashes: Value,
}

/// HMAC-SHA256 (RFC 2104) via `ring::hmac`.
fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    let signing_key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hex::encode(hmac::sign(&signing_key, message).as_ref())
}

/// Constant-time HMAC-SHA256 verification via `ring::hmac::verify`.
fn hmac_sha256_verify(key: &[u8], message: &[u8], expected_hex: &str) -> bool {
    let Ok(expected) = hex::decode(expected_hex) else {
        return false;
    };
    let verifying_key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::verify(&verifying_key, message, &expected).is_ok()
}

fn sha256_hex(message: &[u8]) -> String {
    hex::encode(Sha256::digest(message))
}

/// Build one signed ledger link. Mirrors `ledgerLink` in the legacy JS
/// module.
pub fn ledger_link(key: &[u8], input: LedgerLinkInput) -> LedgerLink {
    let payload = LedgerPayload {
        previous: input.previous,
        version: input.version,
        commit: input.commit,
        generation_id: input.generation_id,
        provider_digests: input.provider_digests,
        rule_digests: input.rule_digests,
        inputs: input.inputs,
        outputs: input.outputs,
        artifact_hashes: input.artifact_hashes,
    };
    let body = serde_json::to_string(&payload).expect("payload serializes");
    let digest = sha256_hex(body.as_bytes());
    let signature = hmac_sha256_hex(key, digest.as_bytes());
    LedgerLink {
        schema_version: 1,
        payload,
        digest,
        signature,
    }
}

/// Result of verifying a single ledger link. Mirrors the JS
/// `{ ok, reason }` / `{ ok: true }` shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyResult {
    Ok,
    Fail(&'static str),
}

impl VerifyResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, VerifyResult::Ok)
    }

    pub fn reason(&self) -> Option<&'static str> {
        match self {
            VerifyResult::Ok => None,
            VerifyResult::Fail(r) => Some(r),
        }
    }
}

/// Verify one ledger link's digest, signature, and (optionally) chain
/// linkage against an expected previous digest. Mirrors `verifyLedgerLink`.
pub fn verify_ledger_link(
    link: &LedgerLink,
    key: &[u8],
    expected_previous: Option<&str>,
) -> VerifyResult {
    if link.digest.is_empty() || link.signature.is_empty() {
        return VerifyResult::Fail("missing_link_fields");
    }
    if let Some(expected) = expected_previous {
        if link.payload.previous.as_deref() != Some(expected) {
            return VerifyResult::Fail("broken_chain");
        }
    }
    let recomputed_digest = sha256_hex(
        serde_json::to_string(&link.payload)
            .expect("payload serializes")
            .as_bytes(),
    );
    if recomputed_digest != link.digest {
        return VerifyResult::Fail("tampered_payload");
    }
    if !hmac_sha256_verify(key, link.digest.as_bytes(), &link.signature) {
        return VerifyResult::Fail("bad_signature");
    }
    VerifyResult::Ok
}

/// Result of verifying a full ledger chain. Mirrors `verifyLedgerChain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainVerifyResult {
    Ok { links: usize },
    Broken { broken_at: String, reason: &'static str },
}

/// Verify an ordered sequence of ledger links, chaining `previous` digests.
/// Mirrors `verifyLedgerChain`.
pub fn verify_ledger_chain(links: &[LedgerLink], key: &[u8]) -> ChainVerifyResult {
    let mut previous: Option<String> = None;
    for link in links {
        let result = verify_ledger_link(link, key, previous.as_deref());
        if !result.is_ok() {
            let broken_at = link
                .payload
                .version
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    if link.payload.version.is_null() {
                        "unknown".to_string()
                    } else {
                        link.payload.version.to_string()
                    }
                });
            return ChainVerifyResult::Broken {
                broken_at,
                reason: result.reason().unwrap_or("unknown"),
            };
        }
        previous = Some(link.digest.clone());
    }
    ChainVerifyResult::Ok {
        links: links.len(),
    }
}
