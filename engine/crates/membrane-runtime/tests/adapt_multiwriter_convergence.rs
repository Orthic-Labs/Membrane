//! ADP-041 production-boundary acceptance.
//!
//! These tests exercise Adapt requests through Cortex's proposal, signed
//! review, admission, restart and recovery path.  No Adapt-owned database or
//! direct memory mutation is used.

use membrane_adapt::authority::PrecedenceTier;
use membrane_adapt::multiwriter::WriterRequestV1;
use membrane_runtime::cortex_lifecycle::{
    propose_multiwriter, proposal_status, recover_pending, resolve_memory, review, trust_path,
    ReviewedEffectV1, ReviewerKeyV1, ReviewerTrustV1, REVIEW_POLICY,
};
use membrane_runtime::digest::digest_str;
use membrane_runtime::{MemDb, MemoryStore};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};

struct Sandbox {
    _dir: tempfile::TempDir,
    store: MemoryStore,
    key: Ed25519KeyPair,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("sandbox directory");
        let store = MemoryStore::try_open(
            MemDb::open(&dir.path().join("cortex.db")).expect("file-backed store"),
        )
        .expect("store");
        let key = Ed25519KeyPair::from_seed_unchecked(&[41; 32]).expect("fixture key");
        let trust = ReviewerTrustV1 {
            schema_version: 1,
            installation_id: store.installation_id().to_owned(),
            cortex_store_id: store.cortex_store_id(),
            reviewers: vec![ReviewerKeyV1 {
                key_id: "adp-041-reviewer".into(),
                public_key_hex: hex::encode(key.public_key().as_ref()),
                repository_id: "repo-adp-041".into(),
                scope_id: "scope-adp-041".into(),
                allowed_operations: vec!["approve".into(), "retry".into()],
                revoked: false,
            }],
        };
        std::fs::write(
            trust_path(&store).expect("trust path"),
            serde_json::to_vec(&trust).expect("trust JSON"),
        )
        .expect("write trust");
        Self { _dir: dir, store, key }
    }

    fn effect(&self, target: &str, hash: &str) -> ReviewedEffectV1 {
        let now = membrane_runtime::time::now_millis() as u64;
        let mut effect = ReviewedEffectV1 {
            schema_version: 1,
            policy_version: REVIEW_POLICY.into(),
            installation_id: self.store.installation_id().into(),
            cortex_store_id: self.store.cortex_store_id(),
            repository_id: "repo-adp-041".into(),
            scope_id: "scope-adp-041".into(),
            operation: "approve".into(),
            target_id: target.into(),
            expected_content_hash: hash.into(),
            expected_control_revision: Some("none".into()),
            key_id: "adp-041-reviewer".into(),
            nonce: format!("approve-{target}"),
            issued_at_ms: now,
            expires_at_ms: now + 60_000,
            signature_hex: String::new(),
        };
        effect.signature_hex = hex::encode(
            self.key
                .sign(&effect.signing_bytes().expect("signing bytes"))
                .as_ref(),
        );
        effect
    }
}

fn request(writer: &str, clock: u64, digest: &str) -> WriterRequestV1 {
    WriterRequestV1::new(
        "repo-adp-041",
        "scope-adp-041",
        "prefer-pnpm",
        "7",
        writer,
        clock,
        digest,
        PrecedenceTier::ExplicitGlobalUserPreference,
    )
}

#[test]
fn competing_requests_use_one_deterministic_proposal_and_retain_conflict() {
    let sandbox = Sandbox::new();
    let first = request("writer-a", 1, "digest-a");
    let second = request("writer-b", 2, "digest-b");

    // A response loss/retry with a different arrival order is one durable
    // proposal, with one canonical winner and a review-visible conflict.
    let a = propose_multiwriter(
        &sandbox.store,
        "repo-adp-041",
        "scope-adp-041",
        &[first.clone(), second.clone()],
    )
    .expect("proposal");
    let b = propose_multiwriter(
        &sandbox.store,
        "repo-adp-041",
        "scope-adp-041",
        &[second, first],
    )
    .expect("idempotent retry");
    assert_eq!(a["proposalId"], b["proposalId"]);
    assert_eq!(a["multiwriter"]["requestCount"], 2);
    assert_eq!(a["multiwriter"]["convergence"]["winners"][0]["payloadDigest"], "digest-a");
    assert_eq!(
        a["multiwriter"]["convergence"]["winners"][0]["conflicts"],
        json!(["digest-b"])
    );
    assert_eq!(proposal_status(&sandbox.store, "repo-adp-041", "scope-adp-041", a["proposalId"].as_str().unwrap()).unwrap()["reviewState"], "pending");

    let id = a["proposalId"].as_str().unwrap();
    let hash = a["emissionHash"].as_str().unwrap();
    let approved = review(
        &sandbox.store,
        "repo-adp-041",
        "scope-adp-041",
        &json!(sandbox.effect(id, hash)),
    )
    .expect("signed review");
    assert_eq!(approved["admissionState"], "completed", "{approved}");
    let memory_id = approved["admission"]["memoryId"].as_str().unwrap();
    let raw = sandbox
        .store
        .entries(100)
        .into_iter()
        .find(|entry| entry.id == memory_id)
        .expect("admitted memory");
    let content = resolve_memory(
        &sandbox.store,
        "scope-adp-041",
        memory_id,
        &digest_str(&raw.content),
        0,
        12_000,
    )
    .expect("resolved content");
    let body: Value = serde_json::from_str(content["content"].as_str().unwrap()).expect("canonical merge body");
    assert_eq!(body["convergence"]["winners"][0]["conflicts"], json!(["digest-b"]));

    // A bounded crash after admission but before job-state bookkeeping is
    // replay-safe: the existing Cortex receipt is reused after reopen.
    sandbox.store.db().lock_events().execute(
        "UPDATE cortex_proposal_admission_v1 SET state='pending', receipt_json=NULL WHERE proposal_id=?1",
        [id],
    ).expect("simulate bounded crash");
    let reopened = MemoryStore::try_open(
        MemDb::open(&sandbox._dir.path().join("cortex.db")).expect("reopen"),
    ).expect("reopened store");
    let recovered = recover_pending(&reopened, 4).expect("recovery");
    assert_eq!(recovered["completed"], 1, "{recovered}");
    assert_eq!(proposal_status(&reopened, "repo-adp-041", "scope-adp-041", id).unwrap()["admissionState"], "completed");
}

#[test]
fn multiwriter_rejects_cross_repository_or_scope_rebinding() {
    let sandbox = Sandbox::new();
    let mut request = request("writer-a", 1, "digest-a");
    request.scope_id = "other-scope".into();
    let error = propose_multiwriter(
        &sandbox.store,
        "repo-adp-041",
        "scope-adp-041",
        &[request],
    )
    .expect_err("scope rebinding must fail closed");
    assert_eq!(error.code, "multiwriter_request_invalid");
}
