//! Governed Cortex proposal intake, independently signed review and bounded
//! recovery. The event-store proposal table remains the single pending queue;
//! canonical memory effects use the existing admission transaction.
//!
//! A wire caller's reviewer/authority strings never confer review permission.
//! Trust is loaded from an installation-owned file, never a request path.

use crate::{digest::digest_str, MemoryStore};
use cortex_store::TemporalFact;
use cortex_store::temporal::{
    validate_fact_proposal, TemporalInstantV1, TemporalValidityReceiptV1, TemporalValidityV1,
};
use ring::signature::{UnparsedPublicKey, ED25519};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, io::Read, path::PathBuf};

pub const MAX_PAYLOAD_BYTES: usize = 65_536;
pub const REVIEW_POLICY: &str = "cortex-reviewed-effect-v1";
const SIGNING_DOMAIN: &[u8] = b"Membrane Cortex reviewed effect v1\0";
const MAX_REVIEW_LIFETIME_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, thiserror::Error)]
#[error("{code}: {message}")]
pub struct LifecycleError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}
type Result<T> = std::result::Result<T, LifecycleError>;
fn fail(code: &'static str, message: impl Into<String>) -> LifecycleError {
    LifecycleError { code, message: message.into(), retryable: false }
}
fn storage(error: impl std::fmt::Display) -> LifecycleError {
    LifecycleError { code: "cortex_storage_unavailable", message: error.to_string(), retryable: true }
}

/// Installation-owned public keys. Enrollment is deliberately NOT an MCP
/// operation. Protect this file like the existing installation grant registry.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewerTrustV1 {
    pub schema_version: u32,
    pub installation_id: String,
    pub cortex_store_id: String,
    pub reviewers: Vec<ReviewerKeyV1>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewerKeyV1 {
    pub key_id: String,
    pub public_key_hex: String,
    pub repository_id: String,
    pub scope_id: String,
    pub allowed_operations: Vec<String>,
    pub revoked: bool,
}

/// The complete effect is signed, including target version, exact caller
/// binding, store, expiry and policy. An approval is not a transferable role.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewedEffectV1 {
    pub schema_version: u32,
    pub policy_version: String,
    pub installation_id: String,
    pub cortex_store_id: String,
    pub repository_id: String,
    pub scope_id: String,
    pub operation: String,
    pub target_id: String,
    pub expected_content_hash: String,
    /// Suppression CAS token. The first decision requires "none"; subsequent
    /// decisions name the last decisionHash. Unused for immutable proposals.
    pub expected_control_revision: Option<String>,
    pub key_id: String,
    pub nonce: String,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    pub signature_hex: String,
}
impl ReviewedEffectV1 {
    /// Sign these exact domain-separated bytes. The empty signature field is
    /// included, preventing an ambiguous alternate JSON canonicalization.
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        let mut unsigned = self.clone();
        unsigned.signature_hex.clear();
        let mut bytes = SIGNING_DOMAIN.to_vec();
        bytes.extend(serde_json::to_vec(&unsigned).map_err(storage)?);
        Ok(bytes)
    }
}

pub fn trust_path(store: &MemoryStore) -> Result<PathBuf> {
    store.db().event_db_path()
        .map(|p| p.with_file_name("cortex-review-trust.v1.json"))
        .ok_or_else(|| fail("cortex_review_unavailable", "installation-backed review trust is unavailable"))
}
fn load_trust(store: &MemoryStore) -> Result<ReviewerTrustV1> {
    let path = trust_path(store)?;
    let meta = std::fs::symlink_metadata(&path)
        .map_err(|_| fail("cortex_review_unavailable", "review trust has not been enrolled"))?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_PAYLOAD_BYTES as u64 {
        return Err(fail("cortex_review_unavailable", "invalid installation review trust file"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path).map_err(storage)?
        .take((MAX_PAYLOAD_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(storage)?;
    if bytes.len() > MAX_PAYLOAD_BYTES { return Err(fail("cortex_review_unavailable", "review trust exceeds limit")); }
    serde_json::from_slice(&bytes).map_err(|_| fail("cortex_review_unavailable", "malformed review trust"))
}
fn verify_review(store: &MemoryStore, review: &ReviewedEffectV1, repository: &str, scope: &str) -> Result<()> {
    verify_with_trust(store, review, repository, scope, &load_trust(store)?, crate::time::now_millis() as u64)
}
fn verify_with_trust(store: &MemoryStore, review: &ReviewedEffectV1, repository: &str, scope: &str,
    trust: &ReviewerTrustV1, now: u64) -> Result<()> {
    if review.schema_version != 1 || review.policy_version != REVIEW_POLICY
        || trust.schema_version != 1
        || trust.cortex_store_id != store.cortex_store_id()
        || review.installation_id != trust.installation_id || review.cortex_store_id != trust.cortex_store_id
        || review.repository_id != repository || review.scope_id != scope {
        return Err(fail("cortex_review_binding_denied", "review does not match policy, installation, store or caller"));
    }
    if !matches!(review.operation.as_str(), "approve" | "reject" | "retry" | "suppress" | "resume")
        || review.target_id.trim().is_empty() || review.target_id.len() > 1024
        || review.nonce.trim().is_empty() || review.nonce.len() > 160
        || review.expected_content_hash.len() != 71 || !review.expected_content_hash.starts_with("sha256:")
        || !review.expected_content_hash[7..].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || review.issued_at_ms == 0 || review.issued_at_ms > now || now >= review.expires_at_ms
        || review.expires_at_ms.saturating_sub(review.issued_at_ms) > MAX_REVIEW_LIFETIME_MS {
        return Err(fail("cortex_review_invalid", "invalid or expired reviewed effect"));
    }
    let mut ids = BTreeSet::new();
    if trust.reviewers.len() > 256 || trust.reviewers.iter().any(|k| !ids.insert(&k.key_id)) {
        return Err(fail("cortex_review_unavailable", "ambiguous or oversized reviewer registry"));
    }
    let key = trust.reviewers.iter().find(|k| !k.revoked && k.key_id == review.key_id
        && k.repository_id == repository && k.scope_id == scope
        && k.allowed_operations.iter().any(|op| op == &review.operation))
        .ok_or_else(|| fail("cortex_review_denied", "key is not authorized for this effect and scope"))?;
    let key = hex::decode(&key.public_key_hex).map_err(|_| fail("cortex_review_denied", "invalid public key"))?;
    let signature = hex::decode(&review.signature_hex).map_err(|_| fail("cortex_review_denied", "invalid signature"))?;
    UnparsedPublicKey::new(&ED25519, key).verify(&review.signing_bytes()?, &signature)
        .map_err(|_| fail("cortex_review_denied", "signature verification failed"))
}

pub(crate) fn ensure_memory_schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS cortex_admission_receipts_v1(
           request_id TEXT PRIMARY KEY, payload_hash TEXT NOT NULL, disposition_json TEXT NOT NULL) STRICT;
         CREATE TABLE IF NOT EXISTS cortex_recall_suppression_v1(
           memory_id TEXT PRIMARY KEY, scope_id TEXT NOT NULL, content_hash TEXT NOT NULL,
           suppressed INTEGER NOT NULL CHECK(suppressed IN (0,1)), decision_hash TEXT NOT NULL) STRICT;
         CREATE TABLE IF NOT EXISTS cortex_reviewed_controls_v1(
           nonce_key TEXT PRIMARY KEY, effect_hash TEXT NOT NULL, receipt_json TEXT NOT NULL) STRICT;"
    )
}
fn ensure_proposal_schema(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS membrane_knowledge_proposal(
           proposal_id TEXT PRIMARY KEY,repository_id TEXT NOT NULL,scope_id TEXT NOT NULL,
           emission_json TEXT NOT NULL,emission_sha256 TEXT NOT NULL,
           state TEXT NOT NULL CHECK(state IN ('pending','approved','rejected')),
           created_at TEXT NOT NULL,decided_at TEXT,reviewer TEXT) STRICT;
         CREATE TABLE IF NOT EXISTS cortex_proposal_admission_v1(
           proposal_id TEXT PRIMARY KEY, effect_json TEXT NOT NULL, effect_hash TEXT NOT NULL,
           state TEXT NOT NULL CHECK(state IN ('pending','completed','blocked')),
           attempts INTEGER NOT NULL DEFAULT 0, next_attempt_ms INTEGER NOT NULL DEFAULT 0,
           last_error TEXT, receipt_json TEXT) STRICT;
         CREATE TABLE IF NOT EXISTS cortex_proposal_reviews_v1(
           nonce_key TEXT PRIMARY KEY, effect_hash TEXT NOT NULL, proposal_id TEXT NOT NULL) STRICT;"
    )
}
fn bound(value: &Value) -> Result<String> {
    let text = serde_json::to_string(value).map_err(storage)?;
    if text.len() > MAX_PAYLOAD_BYTES { return Err(fail("proposal_payload_too_large", "payload exceeds 65536 bytes")); }
    Ok(text)
}

/// CTX-002 frozen ordered pre-gate vocabulary. Gate order is
/// schema -> scope -> producer -> DLP -> epistemic class -> stable identity,
/// matching the legacy Cortex pipeline order. Every dimension fails closed
/// with a typed code; missing producer or epistemic evidence is denied,
/// never defaulted.
pub const PREGATE_PRODUCERS: &[&str] = &[
    "manual", "agent", "harness", "adapt_native", "ingest_hook", "dream", "episodic", "import",
    "checkpoint", "system",
];
/// How the candidate claims to know. `observed` is first-hand tool/system
/// measurement; `reported` is an agent/user assertion; `inferred` is derived
/// from other durable records; `directive` is an explicit operator decision.
pub const PREGATE_EPISTEMIC_CLASSES: &[&str] = &["observed", "reported", "inferred", "directive"];

/// Deterministic secret-material scan for the DLP pre-gate dimension.
/// Scope is secret/token/key material only; broader PII classes remain a
/// documented policy gap, not a silent pass.
fn dlp_hit(text: &str) -> Option<&'static str> {
    const KEY_HEADERS: &[&str] = &[
        "-----BEGIN PRIVATE KEY-----",
        "-----BEGIN RSA PRIVATE KEY-----",
        "-----BEGIN OPENSSH PRIVATE KEY-----",
        "-----BEGIN EC PRIVATE KEY-----",
    ];
    if KEY_HEADERS.iter().any(|header| text.contains(header)) {
        return Some("secret_key_material");
    }
    fn keyed(text: &str, prefix: &str, min_tail: usize) -> bool {
        for (pos, _) in text.match_indices(prefix) {
            let tail = &text[pos + prefix.len()..];
            let run = tail
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .count();
            if run >= min_tail {
                return true;
            }
        }
        false
    }
    if keyed(text, "AKIA", 16) {
        return Some("aws_access_key");
    }
    if keyed(text, "ghp_", 10) || keyed(text, "gho_", 10) || keyed(text, "github_pat_", 10) {
        return Some("github_token");
    }
    if keyed(text, "sk-live-", 8) || keyed(text, "sk-test-", 8) {
        return Some("api_secret");
    }
    if keyed(text, "xoxb-", 8) || keyed(text, "xoxp-", 8) || keyed(text, "xoxa-", 8) || keyed(text, "xoxs-", 8) {
        return Some("chat_token");
    }
    if keyed(text, "AIza", 10) {
        return Some("api_key");
    }
    None
}

/// Store a proposal only. Scope is the verified caller binding, not a claim in
/// emission JSON. Legacy records are read but never silently reapproved.
pub fn propose(store: &MemoryStore, repository: &str, scope: &str, emission: &Value) -> Result<Value> {
    let mut emission = emission.as_object().cloned()
        .ok_or_else(|| fail("proposal_emission_text_required", "emission must be an object"))?;
    let text = emission.get("text").or_else(|| emission.get("content"))
        .and_then(Value::as_str).filter(|v| !v.trim().is_empty()).map(str::to_owned)
        .ok_or_else(|| fail("proposal_emission_text_required", "emission text is required"))?;
    if text.len() > MAX_PAYLOAD_BYTES { return Err(fail("proposal_payload_too_large", "emission exceeds limit")); }
    if emission.get("scopeId").is_some_and(|v| v.as_str() != Some(scope)) {
        return Err(fail("proposal_scope_denied", "emission scope does not match caller"));
    }
    emission.insert("scopeId".into(), json!(scope));
    // CTX-002 ordered pre-gate, dimensions 3-6. Dimensions 1-2 (schema text,
    // caller scope) are enforced above; every check below fails closed before
    // any semantic admission or durable write.
    let producer = emission.get("producer").and_then(Value::as_str).unwrap_or("").trim();
    if !PREGATE_PRODUCERS.contains(&producer) {
        return Err(fail("producer_denied", "emission producer is missing or not an admitted producer"));
    }
    if let Some(class) = dlp_hit(&text) {
        return Err(fail("dlp_denied", format!("emission carries denied secret material ({class})")));
    }
    let epistemic = emission.get("epistemicClass").and_then(Value::as_str).unwrap_or("").trim();
    if !PREGATE_EPISTEMIC_CLASSES.contains(&epistemic) {
        return Err(fail("epistemic_denied", "emission epistemicClass is missing or not admitted"));
    }
    let claimed = emission.get("id").and_then(Value::as_str)
        .map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned);
    // The identity claim is evidence about identity, not content: it is
    // checked against the derived identity, never hashed into it.
    emission.remove("id");
    let emission_json = bound(&Value::Object(emission))?;
    let hash = digest_str(&emission_json);
    let id = digest_str(&serde_json::to_string(&(repository, scope, &hash)).map_err(storage)?);
    if claimed.as_deref().is_some_and(|value| value != id) {
        return Err(fail("identity_conflict", "emission id claim does not match derived stable identity"));
    }
    {
        let db = store.db().lock_events();
        ensure_proposal_schema(&db).map_err(storage)?;
        db.execute("INSERT OR IGNORE INTO membrane_knowledge_proposal
            (proposal_id,repository_id,scope_id,emission_json,emission_sha256,state,created_at)
            VALUES(?1,?2,?3,?4,?5,'pending',?6)",
            params![id, repository, scope, emission_json, hash, crate::time::now_iso()]).map_err(storage)?;
    }
    proposal_status(store, repository, scope, &id)
}

/// Temporal MCP record is proposal-only. Caller authority & veracity are
/// discarded before validation or persistence; predicate cardinality is an
/// explicit deterministic policy input carried into signed review.
pub fn propose_temporal(
    store: &MemoryStore,
    repository: &str,
    scope: &str,
    fact: &TemporalFact,
    single_valued_predicates: &[String],
) -> Result<Value> {
    if single_valued_predicates.len() > 64
        || single_valued_predicates
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 512)
    {
        return Err(fail(
            "temporal_fact_envelope_invalid",
            "singleValuedPredicates must be a bounded list of predicate names",
        ));
    }
    let unique = single_valued_predicates
        .iter()
        .collect::<BTreeSet<_>>();
    if unique.len() != single_valued_predicates.len() {
        return Err(fail(
            "temporal_fact_envelope_invalid",
            "singleValuedPredicates must be unique",
        ));
    }
    let mut normalized = fact.clone();
    normalized.scope_id = scope.to_owned();
    normalized.authority = "A0".into();
    normalized.veracity = "unverified".into();
    normalized.supersedes = None;
    validate_fact_proposal(&normalized)
        .map_err(|message| fail("temporal_fact_invalid", message))?;
    let single_valued = unique.iter().any(|value| value.as_str() == fact.predicate);
    let text = serde_json::to_string(&json!({
        "subject": normalized.subject,
        "predicate": normalized.predicate,
        "object": normalized.object,
        "validFrom": normalized.valid_from,
        "validUntil": normalized.valid_until,
        "expiresAt": normalized.expires_at
    }))
    .map_err(storage)?;
    let mut receipt = propose(
        store,
        repository,
        scope,
        &json!({
            "kind":"temporal",
            "text":text,
            "producer":"agent",
            "epistemicClass":"observed",
            "temporalFact":normalized,
            "callerFieldsStripped":["authority","veracity","supersedes"],
            "cardinalityPolicy":{
                "policyVersion":"cortex.temporal-cardinality.v1",
                "predicate":fact.predicate,
                "cardinality":if single_valued {"single"} else {"multi"},
                "source":"explicit_single_valued_predicates"
            }
        }),
    )?;
    receipt["provenance"]["callerFieldsStripped"] =
        json!(["authority", "veracity", "supersedes"]);
    receipt["provenance"]["cardinalityPolicy"] = json!({
        "policyVersion":"cortex.temporal-cardinality.v1",
        "predicate":fact.predicate,
        "cardinality":if single_valued {"single"} else {"multi"},
        "source":"explicit_single_valued_predicates"
    });
    Ok(receipt)
}

pub fn proposal_status(store: &MemoryStore, repository: &str, scope: &str, id: &str) -> Result<Value> {
    let db = store.db().lock_events();
    ensure_proposal_schema(&db).map_err(storage)?;
    let row = db.query_row("SELECT emission_sha256,state,created_at,emission_json FROM membrane_knowledge_proposal
        WHERE proposal_id=?1 AND repository_id=?2 AND scope_id=?3", params![id, repository, scope],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))
        .optional().map_err(storage)?.ok_or_else(|| fail("proposal_review_unknown", "proposal unavailable in caller scope"))?;
    let stored: Value = serde_json::from_str(&row.3).map_err(storage)?;
    // Identity claims are checked against the derived identity at intake and
    // never stored, so every durable proposal carries derived stable identity.
    let pregate = json!({
        "producer": stored.get("producer"),
        "epistemicClass": stored.get("epistemicClass"),
        "dlp": "pass",
        "stableIdentity": "derived",
    });
    let admission: Option<(String, Option<String>, Option<String>, String)> = db.query_row(
        "SELECT state,last_error,receipt_json,effect_hash FROM cortex_proposal_admission_v1 WHERE proposal_id=?1", [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional().map_err(storage)?;
    let admission_state = admission.as_ref().map(|r| r.0.as_str())
        .unwrap_or(if row.1 == "approved" { "legacy_review_requires_reauthorization" } else { "not_requested" });
    let receipt = admission.as_ref().and_then(|r| r.2.as_deref())
        .map(serde_json::from_str::<Value>).transpose().map_err(storage)?;
    Ok(json!({"status":if row.1 == "pending" { "needs_review" } else { "reviewed" },
        "durable":true,"proposalId":id,"durableId":id,"reviewState":row.1,
        "emissionHash":row.0,"admissionState":admission_state,"admission":receipt,
        "lastError":admission.as_ref().and_then(|r| r.1.clone()),
        "admissionRevision":admission.as_ref().map(|r|r.3.clone()).unwrap_or_else(||"legacy".into()),
        "pregate":pregate,
        "lifecycleReceipt":{"schema":"membrane.lifecycle-receipt.v1","operation":"knowledge_propose",
        "status":row.1,"durableId":id,"eventId":digest_str(&format!("proposal:{id}")),
        "readbackDigest":row.0,"recordedAt":row.2},
        "provenance":{"repositoryId":repository,"scopeId":scope,"authority":"proposal_only"}}))
}

pub fn review(store: &MemoryStore, repository: &str, scope: &str, value: &Value) -> Result<Value> {
    bound(value)?;
    let effect: ReviewedEffectV1 = serde_json::from_value(value.clone())
        .map_err(|_| fail("cortex_review_invalid", "invalid reviewed-effect envelope"))?;
    verify_review(store, &effect, repository, scope)?;
    if matches!(effect.operation.as_str(), "suppress" | "resume") {
        return apply_suppression(store, &effect);
    }
    queue_review(store, &effect)?;
    if matches!(effect.operation.as_str(), "approve" | "retry") {
        // A failed admission is a durable pending/blocked job, not a false
        // success or an approval stranded by a process death.
        let _ = recover_one(store, &effect.target_id);
    }
    proposal_status(store, repository, scope, &effect.target_id)
}
fn queue_review(store: &MemoryStore, effect: &ReviewedEffectV1) -> Result<()> {
    let effect_json = serde_json::to_string(effect).map_err(storage)?;
    let effect_hash = digest_str(&effect_json);
    let nonce_key = digest_str(&serde_json::to_string(&(&effect.key_id, &effect.repository_id, &effect.scope_id, &effect.nonce)).map_err(storage)?);
    let mut db = store.db().lock_events();
    ensure_proposal_schema(&db).map_err(storage)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(storage)?;
    let previous: Option<String> = tx.query_row("SELECT effect_hash FROM cortex_proposal_reviews_v1 WHERE nonce_key=?1",
        [&nonce_key], |r| r.get(0)).optional().map_err(storage)?;
    if let Some(previous) = previous {
        if previous != effect_hash { return Err(fail("cortex_review_replay_conflict", "nonce already binds another effect")); }
        return Ok(());
    }
    let row: Option<(String, String, String)> = tx.query_row(
        "SELECT emission_sha256,emission_json,state FROM membrane_knowledge_proposal
         WHERE proposal_id=?1 AND repository_id=?2 AND scope_id=?3",
        params![effect.target_id, effect.repository_id, effect.scope_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().map_err(storage)?;
    let Some((hash, payload, state)) = row else { return Err(fail("proposal_review_unknown", "proposal unavailable in caller scope")); };
    if hash != effect.expected_content_hash || digest_str(&payload) != hash {
        return Err(fail("cortex_review_version_conflict", "proposal content changed"));
    }
    let retry = effect.operation == "retry";
    if retry {
        let job: Option<(String,String)> = tx.query_row(
            "SELECT state,effect_hash FROM cortex_proposal_admission_v1 WHERE proposal_id=?1",
            [&effect.target_id], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?;
        let eligible = state == "approved" && job.as_ref().is_none_or(|(state,_)| state == "blocked");
        let revision = job.as_ref().map(|(_,hash)| hash.as_str()).unwrap_or("legacy");
        if !eligible || effect.expected_control_revision.as_deref() != Some(revision) {
            return Err(fail("cortex_control_version_conflict", "retry must name the current blocked or legacy approval"));
        }
    } else if state != "pending" {
        return Err(fail("proposal_already_decided", "proposal has already been decided"));
    }
    tx.execute("INSERT INTO cortex_proposal_reviews_v1 VALUES(?1,?2,?3)",
        params![nonce_key, effect_hash, effect.target_id]).map_err(storage)?;
    tx.execute("UPDATE membrane_knowledge_proposal SET state=?2,decided_at=?3,reviewer=?4
        WHERE proposal_id=?1 AND state='pending'", params![effect.target_id,
        if effect.operation == "approve" { "approved" } else { "rejected" }, crate::time::now_iso(), effect.key_id]).map_err(storage)?;
    if effect.operation == "approve" || retry {
        tx.execute("INSERT INTO cortex_proposal_admission_v1(proposal_id,effect_json,effect_hash,state)
            VALUES(?1,?2,?3,'pending') ON CONFLICT(proposal_id) DO UPDATE SET
            effect_json=excluded.effect_json,effect_hash=excluded.effect_hash,state='pending',
            attempts=0,next_attempt_ms=0,last_error=NULL,receipt_json=NULL", params![effect.target_id, effect_json, effect_hash]).map_err(storage)?;
    }
    tx.commit().map_err(storage)
}

/// Bounded reconciliation, called by the tray-owned maintenance loop and on
/// startup. It never promotes unsigned legacy approvals or fabricates trust.
/// A crash-recoverable `admission_pending` job is a row in
/// `cortex_proposal_admission_v1` with `state='pending'`; convergence runs the
/// approved payload through normal Cortex admission idempotently.
pub fn recover_pending(store: &MemoryStore, limit: usize) -> Result<Value> {
    let ids = {
        let db = store.db().lock_events();
        ensure_proposal_schema(&db).map_err(storage)?;
        let mut stmt = db.prepare("SELECT proposal_id FROM cortex_proposal_admission_v1
            WHERE state='pending' AND next_attempt_ms<=?1 ORDER BY proposal_id LIMIT ?2").map_err(storage)?;
        let rows = stmt.query_map(params![crate::time::now_millis() as i64, limit.min(32) as i64], |r| r.get::<_, String>(0))
            .map_err(storage)?.collect::<rusqlite::Result<Vec<_>>>().map_err(storage)?;
        rows
    };
    let mut completed = 0;
    let mut failed = 0;
    for id in &ids { if recover_one(store, id).is_ok() { completed += 1; } else { failed += 1; } }
    Ok(json!({"schemaVersion":1,"operation":"cortex_admission_recovery","considered":ids.len(),
        "completed":completed,"deferredOrBlocked":failed,"complete":ids.len() < limit.min(32)}))
}
fn recover_one(store: &MemoryStore, id: &str) -> Result<()> {
    let row = {
        let db = store.db().lock_events();
        ensure_proposal_schema(&db).map_err(storage)?;
        db.query_row(
            "SELECT a.effect_json,a.effect_hash,p.emission_json,p.repository_id,p.scope_id,p.emission_sha256,a.attempts
             FROM cortex_proposal_admission_v1 a JOIN membrane_knowledge_proposal p USING(proposal_id)
             WHERE a.proposal_id=?1 AND a.state='pending' AND p.state='approved'", [id],
            |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,
                r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,u32>(6)?)))
            .optional().map_err(storage)?
    };
    let Some((raw, effect_hash, payload, repository, scope, hash, attempts)) = row else { return Ok(()); };
    let outcome = (|| {
        if digest_str(&raw) != effect_hash || digest_str(&payload) != hash {
            return Err(fail("cortex_review_version_conflict", "persisted approval or proposal changed"));
        }
        let effect: ReviewedEffectV1 = serde_json::from_str(&raw)
            .map_err(|_| fail("cortex_review_invalid", "stored reviewed effect is malformed"))?;
        if effect.target_id != id || effect.expected_content_hash != hash || !matches!(effect.operation.as_str(), "approve" | "retry")
            || effect.repository_id != repository || effect.scope_id != scope {
            return Err(fail("cortex_review_binding_denied", "stored job does not match approved effect"));
        }
        let mut payload: Value = serde_json::from_str(&payload).map_err(storage)?;
        payload["scopeId"] = json!(scope);
        // A committed effect is an observed fact, not a new request to execute
        // an expired approval. Reconcile the acknowledgement before checking
        // present-day permission to execute an unfinished effect.
        if payload.get("kind").and_then(Value::as_str) == Some("temporal") {
            if let Some(receipt) = temporal_admission_receipt(store, &payload)? {
                return Ok(receipt);
            }
        } else if let Some(receipt) = store
            .reviewed_admission_receipt(id, &hash, &payload)
            .map_err(storage)?
        {
            return Ok(receipt);
        }
        verify_review(store, &effect, &repository, &scope)?;
        if payload.get("kind").and_then(Value::as_str) == Some("temporal") {
            return admit_temporal(store, id, &payload);
        }
        store.admit_reviewed_proposal(id, &hash, &payload, &scope).map_err(storage)
    })();
    let db = store.db().lock_events();
    match outcome {
        Ok(receipt) => {
            db.execute("UPDATE cortex_proposal_admission_v1 SET state='completed',receipt_json=?2,
                last_error=NULL,attempts=attempts+1 WHERE proposal_id=?1 AND state='pending'",
                params![id, serde_json::to_string(&receipt).map_err(storage)?]).map_err(storage)?;
            Ok(())
        }
        Err(error) => {
            let retry = error.retryable && attempts < 7;
            let delay = 30_000i64.saturating_mul(1i64 << attempts.min(5));
            db.execute("UPDATE cortex_proposal_admission_v1 SET state=?2,attempts=attempts+1,
                next_attempt_ms=?3,last_error=?4 WHERE proposal_id=?1 AND state='pending'",
                params![id, if retry { "pending" } else { "blocked" },
                (crate::time::now_millis() as i64).saturating_add(delay), error.code]).map_err(storage)?;
            Err(error)
        }
    }
}

fn temporal_admission_receipt(store: &MemoryStore, payload: &Value) -> Result<Option<Value>> {
    let fact: TemporalFact = serde_json::from_value(payload["temporalFact"].clone())
        .map_err(|_| fail("temporal_fact_invalid", "stored temporal proposal is malformed"))?;
    let Some(receipt) = store
        .temporal_facts()
        .validity_receipt(&fact.fact_id)
        .map_err(storage)?
    else {
        return Ok(None);
    };
    Ok(Some(temporal_receipt_json(store, payload, &receipt)?))
}

fn temporal_receipt_json(
    store: &MemoryStore,
    payload: &Value,
    receipt: &TemporalValidityReceiptV1,
) -> Result<Value> {
    let utility = MemoryStore::admission_utility_eligibility(payload);
    Ok(json!({
        "outcome":"temporal_admitted",
        "factId":receipt.record_id,
        "payloadHash":receipt.payload_sha256,
        "authority":"A4",
        "independentlyVerified":true,
        "callerFieldsStripped":payload["callerFieldsStripped"],
        "cardinalityPolicy":payload["cardinalityPolicy"],
        "transitions":receipt.transitions,
        "cortexStoreId":store.cortex_store_id(),
        "utilityEligibility":utility
    }))
}

fn admit_temporal(store: &MemoryStore, proposal_id: &str, payload: &Value) -> Result<Value> {
    let utility = MemoryStore::admission_utility_eligibility(payload);
    if utility["eligible"] != true {
        return Ok(json!({
            "outcome":"utility_ineligible",
            "proposalId":proposal_id,
            "canonicalMutation":false,
            "utilityEligibility":utility
        }));
    }
    let fact: TemporalFact = serde_json::from_value(payload["temporalFact"].clone())
        .map_err(|_| fail("temporal_fact_invalid", "stored temporal proposal is malformed"))?;
    validate_fact_proposal(&fact).map_err(|message| fail("temporal_fact_invalid", message))?;
    let policy = &payload["cardinalityPolicy"];
    let cardinality = policy.get("cardinality").and_then(Value::as_str);
    if policy.get("policyVersion").and_then(Value::as_str)
        != Some("cortex.temporal-cardinality.v1")
        || policy.get("predicate").and_then(Value::as_str) != Some(fact.predicate.as_str())
        || policy.get("source").and_then(Value::as_str)
            != Some("explicit_single_valued_predicates")
        || !matches!(cardinality, Some("single" | "multi"))
    {
        return Err(fail(
            "temporal_fact_invalid",
            "temporal cardinality policy is missing or inconsistent",
        ));
    }
    let record = TemporalValidityV1 {
        record_id: fact.fact_id,
        subject: fact.subject,
        predicate: fact.predicate,
        object: fact.object,
        scope_id: fact.scope_id,
        authority: "A4".into(),
        valid_at: TemporalInstantV1::known(fact.valid_from),
        recorded_at: TemporalInstantV1::known(fact.observed_at),
        invalid_at: fact.valid_until,
        superseded_by: None,
        revoked: false,
        independently_verified: true,
        expires_at: fact.expires_at,
    };
    store
        .temporal_facts()
        .record_validity(record, cardinality == Some("single"))
        .map_err(storage)?;
    temporal_admission_receipt(store, payload)?.ok_or_else(|| {
        storage("temporal admission committed without readable canonical receipt")
    })
}

fn apply_suppression(store: &MemoryStore, effect: &ReviewedEffectV1) -> Result<Value> {
    let hash = digest_str(&serde_json::to_string(effect).map_err(storage)?);
    let nonce_key = digest_str(&serde_json::to_string(&(&effect.key_id, &effect.repository_id, &effect.scope_id, &effect.nonce)).map_err(storage)?);
    let mut db = store.db().lock();
    ensure_memory_schema(&db).map_err(storage)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(storage)?;
    let previous: Option<(String,String)> = tx.query_row("SELECT effect_hash,receipt_json FROM cortex_reviewed_controls_v1 WHERE nonce_key=?1",
        [&nonce_key], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(storage)?;
    if let Some((previous, receipt)) = previous {
        if previous != hash { return Err(fail("cortex_review_replay_conflict", "nonce already binds another effect")); }
        return serde_json::from_str(&receipt).map_err(storage);
    }
    let record: Option<(String,String,String,Option<String>,Option<i64>,Option<i64>,Option<i64>)> = tx.query_row(
        "SELECT content,authority,lifecycle_state,superseded_by,effective_from_ms,effective_until_ms,expires_at_ms
         FROM memories WHERE id=?1 AND scope_id=?2",
        params![effect.target_id,effect.scope_id],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))
        .optional().map_err(storage)?;
    let Some((content,authority,lifecycle,superseded,from,until,expiry)) = record else {
        return Err(fail("memory_unavailable", "record unavailable in caller scope"));
    };
    if digest_str(&content) != effect.expected_content_hash {
        return Err(fail("memory_version_conflict", "record content changed"));
    }
    if effect.operation == "resume" {
        let now = crate::time::now_millis() as i64;
        if authority == "A0" || lifecycle != "active" || superseded.is_some()
            || from.is_some_and(|t| now < t) || until.is_some_and(|t| now >= t)
            || expiry.is_some_and(|t| now >= t) {
            return Err(fail("memory_ineligible", "record is not currently eligible for suppression reversal"));
        }
    }
    let previous_revision: Option<String> = tx.query_row(
        "SELECT decision_hash FROM cortex_recall_suppression_v1 WHERE memory_id=?1", [&effect.target_id], |r| r.get(0))
        .optional().map_err(storage)?;
    if effect.expected_control_revision.as_deref() != Some(previous_revision.as_deref().unwrap_or("none")) {
        return Err(fail("cortex_control_version_conflict", "suppression decision changed; fresh authorization is required"));
    }
    let suppressed = effect.operation == "suppress";
    tx.execute("INSERT INTO cortex_recall_suppression_v1 VALUES(?1,?2,?3,?4,?5)
        ON CONFLICT(memory_id) DO UPDATE SET scope_id=excluded.scope_id,content_hash=excluded.content_hash,
        suppressed=excluded.suppressed,decision_hash=excluded.decision_hash",
        params![effect.target_id,effect.scope_id,effect.expected_content_hash,suppressed as i64,hash]).map_err(storage)?;
    let receipt = json!({"schemaVersion":1,"operation":effect.operation,"memoryId":effect.target_id,
        "contentHash":effect.expected_content_hash,"suppressed":suppressed,"decisionHash":hash,
        "cortexStoreId":store.cortex_store_id(),"authorityChanged":false,"payloadErased":false});
    tx.execute("INSERT INTO cortex_reviewed_controls_v1 VALUES(?1,?2,?3)",
        params![nonce_key, hash, serde_json::to_string(&receipt).map_err(storage)?]).map_err(storage)?;
    tx.commit().map_err(storage)?;
    Ok(receipt)
}

/// Exact, bounded native record resolution. The content hash binds the whole
/// body; pagination never silently truncates a full-record claim. This reads
/// canonical state, not the potentially stale in-memory registry.
pub fn resolve_memory(store: &MemoryStore, scope: &str, id: &str, expected: &str, offset: usize, max_chars: usize) -> Result<Value> {
    if id.is_empty() || max_chars == 0 || max_chars > 12_000 {
        return Err(fail("memory_envelope_invalid", "id and a bounded character limit are required"));
    }
    let mut db = store.db().lock();
    ensure_memory_schema(&db).map_err(storage)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Deferred).map_err(storage)?;
    let row: Option<(String,String,String,String,Option<i64>,Option<i64>,Option<i64>,Option<String>,String,String)> = tx.query_row(
        "SELECT content,authority,lifecycle_state,source_ids,effective_from_ms,effective_until_ms,expires_at_ms,
          superseded_by,record_type,producer FROM memories WHERE id=?1 AND scope_id=?2", params![id,scope],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?)))
        .optional().map_err(storage)?;
    let Some((content,authority,lifecycle,sources,from,until,expiry,superseded,kind,producer)) = row else {
        return Err(fail("memory_unavailable", "record unavailable in caller scope"));
    };
    let suppressed: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM cortex_recall_suppression_v1 WHERE memory_id=?1 AND suppressed=1)", [id], |r| r.get(0)).map_err(storage)?;
    let now = crate::time::now_millis() as i64;
    if authority == "A0" || lifecycle != "active" || superseded.is_some() || suppressed
        || from.is_some_and(|t| now < t) || until.is_some_and(|t| now >= t) || expiry.is_some_and(|t| now >= t) {
        return Err(fail("memory_ineligible", "record is not currently eligible for agent resolution"));
    }
    let hash = digest_str(&content);
    if hash.strip_prefix("sha256:").unwrap_or(&hash) != expected.strip_prefix("sha256:").unwrap_or(expected) {
        return Err(fail("memory_version_conflict", "expected full-content hash no longer matches"));
    }
    let total = content.chars().count();
    if offset > total { return Err(fail("memory_envelope_invalid", "offset is beyond the record")); }
    let body = content.chars().skip(offset).take(max_chars).collect::<String>();
    let end = offset + body.chars().count();
    // A persisted observed read is not verified helped and does not reinforce
    // usefulness. Scope and content are bound before this counter changes.
    tx.execute("UPDATE memories SET access_count=access_count+1 WHERE id=?1", [id]).map_err(storage)?;
    tx.commit().map_err(storage)?;
    Ok(json!({"schemaVersion":1,"id":id,"cortexStoreId":store.cortex_store_id(),
        "contentHash":hash,"content":body,"offset":offset,"nextOffset":if end < total {Some(end)} else {None},
        "totalChars":total,"complete":offset==0 && end==total,"pageComplete":true,"authority":authority,"lifecycle":lifecycle,
        "scopeId":scope,"recordType":kind,"sourceRefs":serde_json::from_str::<Value>(&sources).map_err(storage)?,
        // CTX-004: provenance availability is explicit data, never guessed.
        // Legacy rows without recorded sources report unavailable_legacy while
        // authority, origin, lifecycle and derivation stay distinct fields.
        "provenanceAvailability":if sources.trim() != "[]" && !sources.trim().is_empty() {"explicit"} else {"unavailable_legacy"},
        "originProducer":producer,
        "derivation":superseded.as_deref().map(|parent| json!({"supersededBy":parent})).unwrap_or(Value::Null),
        "observedResolution":true,"verifiedHelped":false}))
}

/// CTX-040 minimal named/versioned retrieval recipe. Recipe selection changes
/// retrieval only; fallback must be explicit & every bound/projection is
/// echoed in receipt.
pub fn recall_recipe(store: &MemoryStore, scope: &str, args: &Value) -> Result<Value> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| fail("memory_recipe_invalid", "query is required"))?;
    let recipe = args
        .get("recipe")
        .and_then(Value::as_object)
        .ok_or_else(|| fail("memory_recipe_invalid", "recipe name and version are required"))?;
    let requested_name = recipe.get("name").and_then(Value::as_str).unwrap_or("");
    let requested_version = recipe.get("version").and_then(Value::as_u64).unwrap_or(0);
    if requested_name.is_empty() || requested_version == 0 {
        return Err(fail(
            "memory_recipe_invalid",
            "recipe name and positive version are required",
        ));
    }
    let supported = requested_name == "cortex.hybrid" && requested_version == 1;
    let fallback_allowed = recipe
        .get("allowFallback")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !supported && !fallback_allowed {
        return Err(fail(
            "memory_recipe_unsupported",
            format!("unsupported recall recipe {requested_name}@{requested_version}"),
        ));
    }
    let bounds = args.get("bounds").and_then(Value::as_object);
    let requested_items = bounds
        .and_then(|value| value.get("maxItems"))
        .and_then(Value::as_u64)
        .unwrap_or(8);
    let requested_preview = bounds
        .and_then(|value| value.get("maxPreviewChars"))
        .and_then(Value::as_u64)
        .unwrap_or(320);
    let max_items = requested_items.clamp(1, 32) as usize;
    let max_preview = requested_preview.min(2_000) as usize;
    let projection = args
        .get("projection")
        .and_then(Value::as_str)
        .unwrap_or("preview");
    if !matches!(projection, "metadata" | "preview") {
        return Err(fail(
            "memory_recipe_invalid",
            "projection must be metadata or preview",
        ));
    }
    let scopes = vec![scope.to_owned()];
    // A named recipe resolves to a frozen configuration of existing lanes. Keep
    // relationship expansion off here: CTX-040 may select lexical/vector recall,
    // but it cannot implicitly enable the CTX-039 evidence-path capability.
    let cancellation = tokio_util::sync::CancellationToken::new();
    let (hits, _, _) = store.recall_scored_detailed_timed_cancellable(
        query,
        max_items,
        &scopes,
        false,
        &cancellation,
    );
    let actual_recipe = json!({
        "name":"cortex.hybrid",
        "version":1,
        "lanes":["lexical","vector"],
        "graphTraversal":false,
        "maxItems":max_items,
        "maxPreviewChars":max_preview,
        "projection":projection
    });
    let recipe_digest = digest_str(&serde_json::to_string(&actual_recipe).map_err(storage)?);
    let items = hits
        .into_iter()
        .map(|hit| {
            let entry = hit.entry;
            let score = hit.score;
            let content_hash = digest_str(&entry.content);
            let resolver = format!(
                "membrane_memory_read:membrane_memory:get:{}:{}",
                entry.id, content_hash
            );
            let mut item = json!({
                "id":entry.id,
                "scopeId":entry.scope_id,
                "tier":entry.tier,
                "score":score,
                "contentHash":content_hash,
                "resolver":resolver
            });
            if projection == "preview" {
                item["preview"] = json!(entry.content.chars().take(max_preview).collect::<String>());
            }
            item
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "schemaVersion":1,
        "operation":"recall",
        "items":items,
        "receipt":{
            "schemaVersion":1,
            "policyVersion":"cortex.recall-recipe.v1",
            "recipeDigest":recipe_digest,
            "requested":{"name":requested_name,"version":requested_version},
            "actual":{
                "name":"cortex.hybrid",
                "version":1,
                "lanes":["lexical","vector"],
                "graphTraversal":false
            },
            "fallback":{
                "used":!supported,
                "reason":if supported {Value::Null} else {json!("unsupported_requested_recipe")}
            },
            "bounds":{
                "requested":{"maxItems":requested_items,"maxPreviewChars":requested_preview},
                "actual":{"maxItems":max_items,"maxPreviewChars":max_preview}
            },
            "projection":{"requested":projection,"actual":projection},
            "retrievalOnly":true,
            "scopeId":scope
        }
    }))
}

pub fn promote_checkpoint(store: &MemoryStore, repository: &str, scope: &str, id: &str) -> Result<Value> {
    let checkpoint = store.load_checkpoint(id, crate::time::now_millis() as i64).map_err(storage)?;
    if checkpoint.repository_id != repository || checkpoint.scope_id != scope {
        return Err(fail("checkpoint_scope_denied", "checkpoint does not match caller"));
    }
    propose(store, repository, scope, &json!({"text":checkpoint.summary,"kind":"episodic",
        "producer":"checkpoint","epistemicClass":"reported",
        "scopeId":scope,"checkpointId":checkpoint.checkpoint_id,"sessionId":checkpoint.session_id,
        "sourceRefs":checkpoint.source_refs,"worktreeRevision":checkpoint.worktree_rev}))
}

/// CTX-023/CTX-024 Cortex-specific proposal sink: drain installation-local
/// background-review JSONL records into the governed proposal queue.
///
/// Each record is validated and submitted through [`propose`], so the ordered
/// pre-gate, pending-only queueing, and signed-review requirements apply
/// unchanged. The drain preserves the provider-attested record scope as the
/// caller binding; forging that scope requires write access to the
/// installation-local file, which already implies operator trust. Draining is
/// at-least-once and safe to retry: proposal identities are deterministic, so
/// a re-drain is a typed idempotent no-op, never a second record.
pub fn drain_background_proposals(
    store: &MemoryStore,
    repository: &str,
    path: &std::path::Path,
    limit: usize,
) -> Result<Value> {
    use cortex_core::review::{
        validate_semantic_curation_proposals, MemoryCandidateV1, SemanticCurationProposalV1,
    };
    if limit == 0 {
        return Err(fail("memory_envelope_invalid", "drain limit must be greater than zero"));
    }
    let body = std::fs::read_to_string(path)
        .map_err(|_| fail("cortex_storage_unavailable", "background proposal file is unavailable"))?;
    let mut considered = 0usize;
    let mut proposed = 0usize;
    let mut proposal_ids = Vec::new();
    let mut failures = Vec::new();
    for (index, line) in body.lines().enumerate() {
        if considered >= limit {
            break;
        }
        let line_no = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        considered += 1;
        let record: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => {
                failures.push(json!({"line":line_no,"code":"proposal_emission_text_required"}));
                continue;
            }
        };
        let outcome = (|| {
            let kind = record.get("kind").and_then(Value::as_str).unwrap_or("");
            let proposal = record.get("proposal").cloned().unwrap_or(Value::Null);
            match kind {
                "semantic_curation" => {
                    let curation: SemanticCurationProposalV1 = serde_json::from_value(proposal)
                        .map_err(|_| fail("proposal_emission_text_required", "malformed curation record"))?;
                    validate_semantic_curation_proposals(std::slice::from_ref(&curation))
                        .map_err(|_| fail("proposal_emission_text_required", "invalid curation record"))?;
                    let evidence: Vec<String> = curation
                        .evidence
                        .iter()
                        .flat_map(|item| {
                            item.memory_ids
                                .iter()
                                .chain(item.source_event_ids.iter())
                                .cloned()
                        })
                        .take(64)
                        .collect();
                    let text = format!(
                        "Stage 1 {:?} review for memories [{}] per evidence [{}] (curation {})",
                        curation.kind,
                        curation.target_memory_ids.join(","),
                        evidence.join(","),
                        curation.proposal_id,
                    );
                    propose(
                        store,
                        repository,
                        &curation.scope_id,
                        &json!({
                            "text": text,
                            "kind": "semantic_curation",
                            "producer": "dream",
                            "epistemicClass": "inferred",
                            "curationId": curation.proposal_id,
                            "curationKind": format!("{:?}", curation.kind),
                            "targetMemoryIds": curation.target_memory_ids,
                            "jobId": record.get("jobId"),
                        }),
                    )
                }
                "memory_candidate" => {
                    let candidate: MemoryCandidateV1 = serde_json::from_value(proposal)
                        .map_err(|_| fail("proposal_emission_text_required", "malformed candidate record"))?;
                    if candidate.content.trim().is_empty() || candidate.scope_id.trim().is_empty() {
                        return Err(fail("proposal_emission_text_required", "candidate carries no content or scope"));
                    }
                    propose(
                        store,
                        repository,
                        &candidate.scope_id,
                        &json!({
                            "text": candidate.content,
                            "kind": "episodic",
                            "producer": "episodic",
                            "epistemicClass": "reported",
                            "candidateId": candidate.candidate_id,
                            "sessionId": candidate.session_id,
                            "fromSeq": candidate.from_seq,
                            "toSeq": candidate.to_seq,
                            "sourceEventIds": candidate.source_event_ids,
                            "sourceContentHashes": candidate.source_content_hashes,
                            "jobId": record.get("jobId"),
                        }),
                    )
                }
                _ => Err(fail("proposal_emission_text_required", "unknown background proposal kind")),
            }
        })();
        match outcome {
            Ok(receipt) => {
                proposed += 1;
                if let Some(id) = receipt.get("proposalId").and_then(Value::as_str) {
                    proposal_ids.push(id.to_owned());
                }
            }
            Err(error) => failures.push(json!({"line":line_no,"code":error.code})),
        }
    }
    Ok(json!({"schemaVersion":1,"operation":"drain_background_proposals",
        "considered":considered,"proposed":proposed,"failed":failures.len(),
        "proposalIds":proposal_ids,"failures":failures,
        "complete":body.lines().count() <= limit}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    struct Sandbox {
        _dir: tempfile::TempDir,
        store: MemoryStore,
        key: Ed25519KeyPair,
    }
    impl Sandbox {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let store = MemoryStore::try_open(crate::MemDb::open(&dir.path().join("cortex.db")).unwrap()).unwrap();
            let key = Ed25519KeyPair::from_seed_unchecked(&[19; 32]).unwrap(); // test fixture only
            let trust = ReviewerTrustV1 { schema_version: 1, installation_id: store.installation_id().into(),
                cortex_store_id: store.cortex_store_id(), reviewers: vec![ReviewerKeyV1 {
                    key_id: "fixture-reviewer".into(), public_key_hex: hex::encode(key.public_key().as_ref()),
                    repository_id: "repo".into(), scope_id: "scope".into(),
                    allowed_operations: vec!["approve".into(),"reject".into(),"retry".into(),"suppress".into(),"resume".into()], revoked: false,
                }] };
            std::fs::write(trust_path(&store).unwrap(), serde_json::to_vec(&trust).unwrap()).unwrap();
            Self { _dir: dir, store, key }
        }
        fn effect(&self, operation: &str, target: &str, hash: &str, nonce: &str) -> ReviewedEffectV1 {
            let now = crate::time::now_millis() as u64;
            let mut effect = ReviewedEffectV1 { schema_version:1,policy_version:REVIEW_POLICY.into(),
                installation_id:self.store.installation_id().into(),cortex_store_id:self.store.cortex_store_id(),
                repository_id:"repo".into(),scope_id:"scope".into(),operation:operation.into(),target_id:target.into(),
                expected_content_hash:hash.into(),expected_control_revision:Some("none".into()),
                key_id:"fixture-reviewer".into(),nonce:nonce.into(),issued_at_ms:now,expires_at_ms:now+60_000,signature_hex:String::new() };
            self.sign(&mut effect); effect
        }
        fn sign(&self, effect: &mut ReviewedEffectV1) {
            effect.signature_hex = hex::encode(self.key.sign(&effect.signing_bytes().unwrap()).as_ref());
        }
        fn pending(&self, text: &str) -> (String,String) {
            let p=propose(&self.store,"repo","scope",&json!({"text":text,"producer":"manual","epistemicClass":"reported"})).unwrap();
            (p["proposalId"].as_str().unwrap().into(),p["emissionHash"].as_str().unwrap().into())
        }
        fn admitted(&self, text: &str) -> String {
            let (id,hash)=self.pending(text);
            let r=review(&self.store,"repo","scope",&json!(self.effect("approve",&id,&hash,&format!("approve-{id}")))).unwrap();
            assert_eq!(r["admissionState"], "completed", "{r}");
            r["admission"]["memoryId"].as_str().unwrap().into()
        }
    }

    #[test]
    fn cortex_signed_review_is_bound_to_actor_scope_store_and_bytes() {
        let s=Sandbox::new(); let (id,hash)=s.pending("Use bounded transactions when rebuilding the ledger.");
        let effect=s.effect("approve",&id,&hash,"signature-test");
        assert!(verify_review(&s.store,&effect,"repo","scope").is_ok());
        let mut forged=effect.clone(); forged.operation="reject".into();
        assert_eq!(verify_review(&s.store,&forged,"repo","scope").unwrap_err().code,"cortex_review_denied");
        assert_eq!(verify_review(&s.store,&effect,"repo","other-scope").unwrap_err().code,"cortex_review_binding_denied");
        let other=Sandbox::new();
        assert_eq!(verify_review(&other.store,&effect,"repo","scope").unwrap_err().code,"cortex_review_binding_denied");
        assert_eq!(proposal_status(&s.store,"repo","scope",&id).unwrap()["reviewState"],"pending");
    }

    #[test]
    fn cortex_approval_and_job_commit_together_then_restart_recovers_once() {
        let s=Sandbox::new(); let (id,hash)=s.pending("Recheck source digests before publishing a ledger section.");
        let effect=s.effect("approve",&id,&hash,"restart");
        verify_review(&s.store,&effect,"repo","scope").unwrap();
        queue_review(&s.store,&effect).unwrap(); // crash boundary: approved plus recoverable job, no effect
        assert!(s.store.entries(100).is_empty());
        let reopened=MemoryStore::try_open(crate::MemDb::open(&s._dir.path().join("cortex.db")).unwrap()).unwrap();
        recover_pending(&reopened,4).unwrap();
        let first=proposal_status(&reopened,"repo","scope",&id).unwrap();
        assert_eq!(first["admissionState"],"completed","{first}");
        recover_pending(&reopened,4).unwrap();
        assert_eq!(reopened.entries(100).len(),1);
        let retry=review(&reopened,"repo","scope",&json!(effect)).unwrap();
        assert_eq!(retry["admission"],first["admission"]);
    }

    #[test]
    fn cortex_committed_effect_reconciles_after_approval_expiry_without_reexecuting() {
        let s=Sandbox::new(); let (id,hash)=s.pending("On response loss, reconcile the admission receipt rather than writing twice.");
        let effect=s.effect("approve",&id,&hash,"lost-response");
        verify_review(&s.store,&effect,"repo","scope").unwrap(); queue_review(&s.store,&effect).unwrap();
        let payload: String=s.store.db().lock_events().query_row("SELECT emission_json FROM membrane_knowledge_proposal WHERE proposal_id=?1",[&id],|r|r.get(0)).unwrap();
        let receipt=s.store.admit_reviewed_proposal(&id,&hash,&serde_json::from_str(&payload).unwrap(),"scope").unwrap();
        // This simulates time moving past expiry after the canonical effect
        // committed, with the event-store acknowledgement still missing.
        let mut expired=effect.clone(); expired.issued_at_ms=1; expired.expires_at_ms=2; s.sign(&mut expired);
        let raw=serde_json::to_string(&expired).unwrap();
        s.store.db().lock_events().execute("UPDATE cortex_proposal_admission_v1 SET effect_json=?2,effect_hash=?3 WHERE proposal_id=?1",params![id,raw,digest_str(&raw)]).unwrap();
        recover_pending(&s.store,4).unwrap();
        assert_eq!(proposal_status(&s.store,"repo","scope",&id).unwrap()["admission"],receipt);
        assert_eq!(s.store.entries(100).len(),1);
    }

    #[test]
    fn cortex_unexecuted_expired_and_corrupt_approvals_are_blocked() {
        let s=Sandbox::new(); let (id,hash)=s.pending("Only fresh authorized effects may create new records.");
        let mut expired=s.effect("approve",&id,&hash,"expired");
        expired.issued_at_ms=1;expired.expires_at_ms=2;s.sign(&mut expired);
        queue_review(&s.store,&expired).unwrap(); // simulate expiry after trusted queue insertion
        recover_pending(&s.store,4).unwrap();
        let status=proposal_status(&s.store,"repo","scope",&id).unwrap();
        assert_eq!(status["admissionState"],"blocked"); assert_eq!(status["lastError"],"cortex_review_invalid");
        assert!(s.store.entries(100).is_empty());
    }

    #[test]
    fn cortex_suppression_is_reversible_persistent_version_fenced_and_not_erasure() {
        let s=Sandbox::new(); let text="A bounded recovery recipe preserves the canonical source.";
        let id=s.admitted(text); let hash=digest_str(text);
        let effect=s.effect("suppress",&id,&hash,"suppress-1");
        let receipt=review(&s.store,"repo","scope",&json!(effect)).unwrap();
        assert_eq!(receipt["payloadErased"],false);
        assert_eq!(review(&s.store,"repo","scope",&json!(effect)).unwrap(),receipt);
        let reopened=MemoryStore::try_open(crate::MemDb::open(&s._dir.path().join("cortex.db")).unwrap()).unwrap();
        assert_eq!(resolve_memory(&reopened,"scope",&id,&hash,0,12000).unwrap_err().code,"memory_ineligible");
        assert!(reopened.recall_scored("canonical recovery",10,&["scope".into()]).is_empty());
        reopened.reindex().unwrap();
        assert!(reopened.recall_scored("canonical recovery",10,&["scope".into()]).is_empty());
        let suppressed_recipe = recall_recipe(&reopened,"scope",&json!({
            "query":"canonical recovery",
            "recipe":{"name":"cortex.hybrid","version":1},
            "bounds":{"maxItems":10,"maxPreviewChars":120},
            "projection":"preview"
        })).unwrap();
        assert!(suppressed_recipe["items"].as_array().unwrap().is_empty());
        let mut resume=s.effect("resume",&id,&hash,"resume-1");
        assert_eq!(review(&reopened,"repo","scope",&json!(resume)).unwrap_err().code,"cortex_control_version_conflict");
        resume.expected_control_revision=receipt["decisionHash"].as_str().map(str::to_owned);s.sign(&mut resume);
        review(&reopened,"repo","scope",&json!(resume)).unwrap();
        assert_eq!(resolve_memory(&reopened,"scope",&id,&hash,0,12000).unwrap()["content"],text);
        assert!(!reopened.recall_scored("canonical recovery",10,&["scope".into()]).is_empty());
        let resumed_recipe = recall_recipe(&reopened,"scope",&json!({
            "query":"canonical recovery",
            "recipe":{"name":"cortex.hybrid","version":1},
            "bounds":{"maxItems":10,"maxPreviewChars":120},
            "projection":"preview"
        })).unwrap();
        assert!(resumed_recipe["items"].as_array().unwrap().iter().any(|item| item["id"] == id));
    }

    #[test]
    fn cortex_suppression_resume_rechecks_current_lifecycle_and_erase_state() {
        let s=Sandbox::new();
        let text="A suppression reversal cannot reactivate superseded evidence.";
        let id=s.admitted(text); let hash=digest_str(text);
        let successor=s.admitted("The replacement record owns the current lifecycle state.");
        let suppress=s.effect("suppress",&id,&hash,"suppress-lifecycle");
        let receipt=review(&s.store,"repo","scope",&json!(suppress)).unwrap();
        let mut resume=s.effect("resume",&id,&hash,"resume-superseded");
        resume.expected_control_revision=receipt["decisionHash"].as_str().map(str::to_owned); s.sign(&mut resume);
        let event=crate::store::MemoryLifecycleEventV1::superseded(
            "fixture-supersede",
            &id,
            &successor,
            "scope",
            crate::time::now_millis() as i64,
            "fixture-reviewer",
            "A4",
            "fixture",
            "fixture-origin",
        );
        s.store.apply_lifecycle_event(&event).unwrap();
        assert_eq!(review(&s.store,"repo","scope",&json!(resume)).unwrap_err().code,"memory_ineligible");

        let erased_text="Hard erase remains distinct from suppression reversal.";
        let erased_id=s.admitted(erased_text); let erased_hash=digest_str(erased_text);
        let erased_suppress=s.effect("suppress",&erased_id,&erased_hash,"suppress-erased");
        let erased_receipt=review(&s.store,"repo","scope",&json!(erased_suppress)).unwrap();
        let mut erased_resume=s.effect("resume",&erased_id,&erased_hash,"resume-erased");
        erased_resume.expected_control_revision=erased_receipt["decisionHash"].as_str().map(str::to_owned); s.sign(&mut erased_resume);
        s.store.hard_erase(&erased_id).unwrap();
        assert_eq!(review(&s.store,"repo","scope",&json!(erased_resume)).unwrap_err().code,"memory_unavailable");
    }

    #[test]
    fn cortex_suppression_survives_backup_restore_without_erasing_canonical_record() {
        let s=Sandbox::new(); let text="Suppression is lifecycle state, not destructive erasure.";
        let id=s.admitted(text); let hash=digest_str(text);
        let effect=s.effect("suppress",&id,&hash,"suppress-backup");
        let receipt=review(&s.store,"repo","scope",&json!(effect)).unwrap();
        let backup=s.store.backup_cortex().unwrap();
        assert_eq!(backup.suppression.len(),1);
        s.store.hard_erase(&id).unwrap();
        assert!(s.store.entries(100).into_iter().all(|entry| entry.id != id));
        s.store.restore_cortex(&backup).unwrap();
        assert_eq!(resolve_memory(&s.store,"scope",&id,&hash,0,12000).unwrap_err().code,"memory_ineligible");
        let after=s.store.backup_cortex().unwrap();
        assert!(after.memories.iter().any(|row| row.id == id && row.content == text));
        assert!(after.suppression.iter().any(|row| row.memory_id == id && row.suppressed));
        let mut resume=s.effect("resume",&id,&hash,"resume-backup");
        resume.expected_control_revision=receipt["decisionHash"].as_str().map(str::to_owned); s.sign(&mut resume);
        review(&s.store,"repo","scope",&json!(resume)).unwrap();
        assert_eq!(resolve_memory(&s.store,"scope",&id,&hash,0,12000).unwrap()["content"],text);
    }

    #[test]
    fn cortex_admission_utility_precedes_mutation_and_preserves_independent_gates() {
        let s = Sandbox::new();

        let ephemeral = propose(&s.store, "repo", "scope", &json!({
            "text":"Transient scratch detail should not become durable memory.",
            "producer":"manual","epistemicClass":"reported",
            "utilityClass":"ephemeral"
        })).unwrap();
        let ephemeral_id = ephemeral["proposalId"].as_str().unwrap();
        let ephemeral_hash = ephemeral["emissionHash"].as_str().unwrap();
        let rejected = review(
            &s.store,
            "repo",
            "scope",
            &json!(s.effect("approve", ephemeral_id, ephemeral_hash, "utility-ephemeral")),
        ).unwrap();
        assert_eq!(rejected["admission"]["outcome"], "utility_ineligible");
        assert_eq!(rejected["admission"]["canonicalMutation"], false);
        assert_eq!(rejected["admission"]["utilityEligibility"]["eligible"], false);
        assert!(s.store.entries(100).is_empty());

        let explicit = propose(&s.store, "repo", "scope", &json!({
            "text":"Explicit user retention survives the utility filter.",
            "producer":"manual","epistemicClass":"reported",
            "utilityClass":"ephemeral",
            "explicitUser":true
        })).unwrap();
        let explicit_id = explicit["proposalId"].as_str().unwrap();
        let explicit_hash = explicit["emissionHash"].as_str().unwrap();
        let explicit_receipt = review(
            &s.store,
            "repo",
            "scope",
            &json!(s.effect("approve", explicit_id, explicit_hash, "utility-explicit")),
        ).unwrap();
        assert_eq!(explicit_receipt["admission"]["outcome"], "approved");
        assert_eq!(explicit_receipt["admission"]["utilityEligibility"]["protected"], true);
        assert_eq!(explicit_receipt["admission"]["utilityEligibility"]["lowUsageDecisive"], false);

        let consequence = propose(&s.store, "repo", "scope", &json!({
            "text":"High consequence evidence survives the utility filter.",
            "producer":"manual","epistemicClass":"reported",
            "retention":"transient",
            "highConsequence":true
        })).unwrap();
        let consequence_id = consequence["proposalId"].as_str().unwrap();
        let consequence_hash = consequence["emissionHash"].as_str().unwrap();
        let consequence_receipt = review(
            &s.store,
            "repo",
            "scope",
            &json!(s.effect("approve", consequence_id, consequence_hash, "utility-consequence")),
        ).unwrap();
        assert_eq!(consequence_receipt["admission"]["outcome"], "approved");
        assert_eq!(consequence_receipt["admission"]["utilityEligibility"]["protected"], true);

        let duplicate = propose(&s.store, "repo", "scope", &json!({
            "text":"Explicit user retention survives the utility filter.",
            "producer":"manual","epistemicClass":"reported",
            "kind":"semantic"
        })).unwrap();
        let duplicate_id = duplicate["proposalId"].as_str().unwrap();
        let duplicate_hash = duplicate["emissionHash"].as_str().unwrap();
        let duplicate_receipt = review(
            &s.store,
            "repo",
            "scope",
            &json!(s.effect("approve", duplicate_id, duplicate_hash, "utility-duplicate")),
        ).unwrap();
        assert_eq!(duplicate_receipt["admission"]["outcome"], "duplicate");
        assert_eq!(duplicate_receipt["admission"]["utilityEligibility"]["eligible"], true);
    }

    #[test]
    fn cortex_recall_recipe_is_versioned_bounded_deterministic_and_receipt_complete() {
        let s = Sandbox::new();
        s.admitted("Bounded recipe recall keeps lexical and vector retrieval explicit.");
        s.admitted("A second bounded recipe record makes deterministic ranking observable.");
        let args = json!({
            "query":"bounded recipe recall",
            "recipe":{"name":"cortex.hybrid","version":1},
            "bounds":{"maxItems":2,"maxPreviewChars":12},
            "projection":"preview"
        });
        let first = recall_recipe(&s.store, "scope", &args).unwrap();
        let second = recall_recipe(&s.store, "scope", &args).unwrap();
        assert_eq!(first, second);
        assert!(first["receipt"]["recipeDigest"].as_str().unwrap().starts_with("sha256:"));
        assert_eq!(first["receipt"]["actual"]["lanes"], json!(["lexical","vector"]));
        assert_eq!(first["receipt"]["actual"]["graphTraversal"], false);
        assert_eq!(first["receipt"]["fallback"]["used"], false);
        assert_eq!(first["receipt"]["bounds"]["actual"]["maxItems"], 2);
        assert_eq!(first["receipt"]["bounds"]["actual"]["maxPreviewChars"], 12);
        assert!(first["items"].as_array().unwrap().len() <= 2);
        assert!(first["items"].as_array().unwrap().iter().all(|item|
            item["preview"].as_str().unwrap().chars().count() <= 12));

        let changed = recall_recipe(&s.store, "scope", &json!({
            "query":"bounded recipe recall",
            "recipe":{"name":"cortex.hybrid","version":1},
            "bounds":{"maxItems":3,"maxPreviewChars":12},
            "projection":"preview"
        })).unwrap();
        assert_ne!(first["receipt"]["recipeDigest"], changed["receipt"]["recipeDigest"]);

        let metadata = recall_recipe(&s.store, "scope", &json!({
            "query":"bounded recipe recall",
            "recipe":{"name":"cortex.hybrid","version":1},
            "bounds":{"maxItems":2,"maxPreviewChars":12},
            "projection":"metadata"
        })).unwrap();
        assert_ne!(first["receipt"]["recipeDigest"], metadata["receipt"]["recipeDigest"]);
        assert!(metadata["items"].as_array().unwrap().iter().all(|item| item.get("preview").is_none()));

        let unsupported = recall_recipe(&s.store, "scope", &json!({
            "query":"bounded recipe recall",
            "recipe":{"name":"unknown.recipe","version":9}
        })).unwrap_err();
        assert_eq!(unsupported.code, "memory_recipe_unsupported");
        let fallback = recall_recipe(&s.store, "scope", &json!({
            "query":"bounded recipe recall",
            "recipe":{"name":"unknown.recipe","version":9,"allowFallback":true},
            "bounds":{"maxItems":99,"maxPreviewChars":9999},
            "projection":"preview"
        })).unwrap();
        assert_eq!(fallback["receipt"]["actual"]["name"], "cortex.hybrid");
        assert_eq!(fallback["receipt"]["actual"]["version"], 1);
        assert_eq!(fallback["receipt"]["fallback"]["used"], true);
        assert_eq!(fallback["receipt"]["fallback"]["reason"], "unsupported_requested_recipe");
        assert_eq!(fallback["receipt"]["bounds"]["actual"]["maxItems"], 32);
        assert_eq!(fallback["receipt"]["bounds"]["actual"]["maxPreviewChars"], 2000);
    }

    #[test]
    fn cortex_native_record_resolution_is_hash_scope_and_page_bound() {
        let s=Sandbox::new(); let text="Exact content spans two bounded pages."; let id=s.admitted(text);
        assert!(resolve_memory(&s.store,"other",&id,&digest_str(text),0,12000).is_err());
        assert_eq!(resolve_memory(&s.store,"scope",&id,&digest_str("different"),0,12000).unwrap_err().code,"memory_version_conflict");
        let page=resolve_memory(&s.store,"scope",&id,&digest_str(text),0,5).unwrap();
        assert_eq!(page["content"],"Exact");assert_eq!(page["nextOffset"],5);assert_eq!(page["complete"],false);
        let rest=resolve_memory(&s.store,"scope",&id,&digest_str(text),5,12000).unwrap();
        assert_eq!(rest["nextOffset"],Value::Null);assert_eq!(rest["complete"],false);assert_eq!(rest["verifiedHelped"],false);
    }

    #[test]
    fn cortex_checkpoint_promotion_acknowledges_one_durable_pending_proposal() {
        let s=Sandbox::new();let now=crate::time::now_millis() as i64;
        let checkpoint=crate::checkpoint::CheckpointV1 {checkpoint_id:"checkpoint-1".into(),
            installation_id:s.store.installation_id().into(),client:"fixture".into(),session_id:"session".into(),
            repository_id:"repo".into(),worktree_rev:"rev".into(),scope_id:"scope".into(),summary:"Continue bounded admission recovery.".into(),
            goal_snapshot:None,task_snapshot:None,created_at_ms:now,expires_at_ms:now+60_000,source_refs:vec![]};
        s.store.save_checkpoint(&checkpoint).unwrap();
        let one=promote_checkpoint(&s.store,"repo","scope","checkpoint-1").unwrap();
        let two=promote_checkpoint(&s.store,"repo","scope","checkpoint-1").unwrap();
        assert_eq!(one["proposalId"],two["proposalId"]);assert_eq!(one["reviewState"],"pending");
        assert!(s.store.recall_scored("admission recovery",10,&["scope".into()]).is_empty());
        assert!(promote_checkpoint(&s.store,"repo","other","checkpoint-1").is_err());
    }

    #[test]
    fn cortex_nested_scope_claims_cannot_escape_verified_proposal_scope() {
        let s=Sandbox::new();
        assert_eq!(propose(&s.store,"repo","scope",&json!({"text":"secret","scopeId":"global"})).unwrap_err().code,"proposal_scope_denied");
    }

    #[test]
    fn ctx002_ordered_pregate_rejects_each_dimension_before_admission() {
        let s=Sandbox::new();
        let base = |text: &str| json!({"text":text,"producer":"manual","epistemicClass":"reported"});
        // 1. schema: missing text fails closed.
        assert_eq!(propose(&s.store,"repo","scope",&json!({"producer":"manual","epistemicClass":"reported"})).unwrap_err().code,"proposal_emission_text_required");
        // 2. scope: forged scope fails before producer/DLP/epistemic are read.
        assert_eq!(propose(&s.store,"repo","scope",&json!({"text":"t","scopeId":"other","producer":"manual","epistemicClass":"reported"})).unwrap_err().code,"proposal_scope_denied");
        // 3. producer: missing and unknown producers are denied, never defaulted.
        assert_eq!(propose(&s.store,"repo","scope",&json!({"text":"t","epistemicClass":"reported"})).unwrap_err().code,"producer_denied");
        assert_eq!(propose(&s.store,"repo","scope",&json!({"text":"t","producer":"model","epistemicClass":"reported"})).unwrap_err().code,"producer_denied");
        // 4. DLP: secret material is denied even with valid producer/epistemic.
        assert_eq!(propose(&s.store,"repo","scope",&json!({"text":"deploy with AKIAIOSFODNN7EXAMPLE key","producer":"manual","epistemicClass":"reported"})).unwrap_err().code,"dlp_denied");
        assert_eq!(propose(&s.store,"repo","scope",&json!({"text":"-----BEGIN RSA PRIVATE KEY----- bob","producer":"manual","epistemicClass":"reported"})).unwrap_err().code,"dlp_denied");
        // 5. epistemic: missing and unknown classes are denied.
        assert_eq!(propose(&s.store,"repo","scope",&json!({"text":"t","producer":"manual"})).unwrap_err().code,"epistemic_denied");
        assert_eq!(propose(&s.store,"repo","scope",&json!({"text":"t","producer":"manual","epistemicClass":"guess"})).unwrap_err().code,"epistemic_denied");
        // 6. stable identity: a conflicting id claim is rejected; a matching
        // claim is idempotent with the derived identity.
        let clean = base("A durable candidate with full pre-gate evidence.");
        let first = propose(&s.store,"repo","scope",&clean).unwrap();
        let derived = first["proposalId"].as_str().unwrap().to_owned();
        let mut forged = clean.clone();
        forged["id"] = json!("forged-id");
        assert_eq!(propose(&s.store,"repo","scope",&forged).unwrap_err().code,"identity_conflict");
        let mut claimed = clean.clone();
        claimed["id"] = json!(derived.clone());
        let same = propose(&s.store,"repo","scope",&claimed).unwrap();
        assert_eq!(same["proposalId"], json!(derived));
        // Accept path carries receipt-visible pre-gate evidence and stays pending.
        assert_eq!(first["reviewState"], "pending");
        assert_eq!(first["pregate"]["producer"], "manual");
        assert_eq!(first["pregate"]["epistemicClass"], "reported");
        assert_eq!(first["pregate"]["dlp"], "pass");
        assert_eq!(first["pregate"]["stableIdentity"], "derived");
        assert!(s.store.entries(100).is_empty());
    }

    #[test]
    fn ctx004_resolver_reports_explicit_provenance_availability_without_guessing() {
        let s=Sandbox::new();
        // Legacy direct write carries no recorded sources: absence is explicit.
        let legacy_id=s.store.put("legacy-row", "A legacy row without recorded sources.", "scope", cortex_core::MemoryTier::Semantic);
        assert!(!legacy_id.is_empty());
        let legacy=resolve_memory(&s.store,"scope",&legacy_id,&digest_str("A legacy row without recorded sources."),0,12000).unwrap();
        assert_eq!(legacy["provenanceAvailability"], "unavailable_legacy");
        assert_eq!(legacy["authority"], "A2");
        assert_eq!(legacy["originProducer"], "manual");
        assert_eq!(legacy["lifecycle"], "active");
        assert_eq!(legacy["derivation"], Value::Null);
        // Governed review admission binds the proposal as an explicit source
        // with independently verified authority; all four bindings stay distinct.
        let text="Reviewed admission carries explicit proposal provenance.";
        let id=s.admitted(text);
        let page=resolve_memory(&s.store,"scope",&id,&digest_str(text),0,12000).unwrap();
        assert_eq!(page["provenanceAvailability"], "explicit");
        assert_eq!(page["authority"], "A4");
        assert_eq!(page["originProducer"], "manual");
        assert_eq!(page["lifecycle"], "active");
        assert_eq!(page["derivation"], Value::Null);
        assert_eq!(page["content"], text);
    }

    #[test]
    fn ctx023_ctx024_drain_submits_background_proposals_governed_and_idempotent() {
        let s=Sandbox::new();
        let dir=tempfile::tempdir().unwrap();
        let path=dir.path().join("proposals.jsonl");
        let curation=json!({"schemaVersion":1,"jobId":"job-1","kind":"semantic_curation",
            "proposal":{"schemaVersion":1,"proposalId":"curation-1","scopeId":"scope",
            "kind":"contradiction","targetMemoryIds":["scope/a"],
            "evidence":[{"memoryIds":["scope/a"],"sourceEventIds":["e1"],"sourceContentHashes":[]}]}});
        let candidate=json!({"schemaVersion":1,"jobId":"job-1","kind":"memory_candidate",
            "proposal":{"schemaVersion":1,"candidateId":"cand-1","sessionId":"s1",
            "fromSeq":1,"toSeq":2,"scopeId":"scope","content":"An episodic observation from the event window.",
            "sourceEventIds":["e1"],"sourceContentHashes":[]}});
        std::fs::write(&path, format!("{}\n{}\nnot json\n", curation, candidate)).unwrap();
        let first=drain_background_proposals(&s.store,"repo",&path,100).unwrap();
        assert_eq!(first["considered"], 3);
        assert_eq!(first["proposed"], 2);
        assert_eq!(first["failed"], 1);
        assert_eq!(first["proposalIds"].as_array().unwrap().len(), 2);
        // Proposal-first: two pending proposals, zero durable rows.
        for id in first["proposalIds"].as_array().unwrap() {
            let id=id.as_str().unwrap();
            assert_eq!(proposal_status(&s.store,"repo","scope",id).unwrap()["reviewState"], "pending");
        }
        assert!(s.store.entries(100).is_empty());
        // Re-drain is an idempotent no-op, never a second record.
        let second=drain_background_proposals(&s.store,"repo",&path,100).unwrap();
        assert_eq!(second["proposalIds"], first["proposalIds"]);
        assert_eq!(second["failed"], 1);
        // Zero limit fails closed instead of silently draining nothing.
        assert!(drain_background_proposals(&s.store,"repo",&path,0).is_err());
    }

    #[test]
    fn ctx001_canonical_open_reports_wal_and_converges_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cortex.db");
        let memdb = crate::MemDb::open(&path).unwrap();
        for report in memdb.startup_wal_reports().iter() {
            assert_eq!(report.status, "ok");
            assert!(report.degradation_reason.is_none());
        }
        assert_eq!(memdb.startup_wal_reports()[0].effective_journal_mode.as_deref(), Some("wal"));
        let store = MemoryStore::try_open(memdb).unwrap();
        let pending = propose(&store,"repo","scope",&json!({"text":"Durable authority survives reopen.","producer":"manual","epistemicClass":"reported"})).unwrap();
        let id = pending["proposalId"].as_str().unwrap().to_owned();
        drop(store);
        let reopened = MemoryStore::try_open(crate::MemDb::open(&path).unwrap()).unwrap();
        for report in reopened.db().startup_wal_reports().iter() {
            assert_eq!(report.status, "ok");
        }
        assert_eq!(proposal_status(&reopened,"repo","scope",&id).unwrap()["reviewState"], "pending");
    }
}

/// Supervised by serve::run, never a standalone service. The owner supplies the
/// same resident store handle used by MCP. A stop wakes the idle wait; work is
/// bounded to four journals per tick and seven attempts per journal.
pub(crate) struct AdmissionRecoveryWorker {
    stop: std::sync::mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl AdmissionRecoveryWorker {
    pub(crate) fn start(store: MemoryStore) -> std::result::Result<Self, String> {
        let (stop, stopped) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new().name("cortex-admission-recovery".into()).spawn(move || {
            let mut previously_unavailable = false;
            loop {
                if stopped.try_recv().is_ok() { break; }
                match recover_pending(&store, 4) {
                    Ok(summary) => {
                        if summary["considered"].as_u64().unwrap_or(0) > 0 || previously_unavailable {
                            eprintln!("cortex-lifecycle {summary}");
                        }
                        previously_unavailable = false;
                    }
                    Err(error) => {
                        if !previously_unavailable {
                            // No payloads, prompts, signatures or storage paths in telemetry.
                            eprintln!("cortex-lifecycle {}", json!({"operation":"cortex_admission_recovery","status":"unavailable","code":error.code}));
                        }
                        previously_unavailable = true;
                    }
                }
                match stopped.recv_timeout(std::time::Duration::from_secs(30)) {
                    Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        }).map_err(|error| error.to_string())?;
        Ok(Self { stop, thread: Some(thread) })
    }
}
impl Drop for AdmissionRecoveryWorker {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}
