//! Focused acceptance for COMMITTED Cortex atoms awaiting verification.
//!
//! Production traces (all behavior exercised through real seams, never
//! internals):
//! - CTX-006 store admission (`try_put` / `try_put_with_metadata` →
//!   `try_admit_*` transaction in `store.rs:8080-8431`).
//! - CTX-007 quarantine (`memory_quarantine` + `restore_quarantined` in
//!   `store.rs:3152-3280`; recall gates in `store.rs:2077-2128`).
//! - CTX-009 temporal round-trip (`cortex_lifecycle::propose_temporal` →
//!   `review` → `admit_temporal` → `TemporalFactStore::validity_receipt`).
//! - CTX-011 FTS5/hybrid recall gates (`recall_scored*`, `recall_recipe`,
//!   `cortex_lifecycle::resolve_memory`; suppression via signed review).
//! - CTX-012 vector dispatch (`vector_dispatch_v2_enabled` in
//!   `store.rs:131-139`; `retrieve_hybrid_indexed` vs legacy
//!   `retrieve_hybrid`).
//! - CTX-013 hybrid fusion at the runtime seam (lexical+vector candidates,
//!   cosine-ordered scores; Membrane fusion authority itself is covered by
//!   `membrane-core/src/fusion.rs` unit tests).
//! - CTX-014 bounded resolver (`cortex_lifecycle::resolve_memory`).
//! - CTX-020 proposal intake (`cortex_lifecycle::propose` + `proposal_status`).
//! - CTX-034 skill index (`ingest_skills` / `search_skills` /
//!   `skill_read_bounded` / `skills_snapshot`; Pull handle shape mapped in
//!   `pull/federation_sources.rs:363-369`).
//! - CTX-038 exact/lower-bound envelopes (`try_list_bounded`,
//!   `lifecycle_reviews_due`, `recall_scored_detailed_timed_cancellable`).
//! - CTX-030 explain projection (real `cli::explain_memory` seam, now public).
//!
//! Conventions mirror `tests/cortex_lifecycle_gaps.rs` (`store()` helper) and
//! the `Sandbox` pattern in `src/cortex_lifecycle.rs`. Deterministic, no
//! network, tempfile or in-memory only.

use cortex_core::MemoryTier;
use cortex_store::temporal::TemporalInstantV1;
use membrane_runtime::cortex_lifecycle::{
    propose, propose_temporal, proposal_status, recall_recipe, resolve_memory, review,
    trust_path, ReviewedEffectV1, ReviewerKeyV1, ReviewerTrustV1, REVIEW_POLICY,
    MAX_PAYLOAD_BYTES,
};
use membrane_runtime::cli::explain_memory;
use membrane_runtime::digest::digest_str;
use membrane_runtime::store::CortexCompletenessState;
use membrane_runtime::{MemDb, MemoryStore, TemporalFact};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

fn store() -> MemoryStore {
    MemoryStore::open(MemDb::open_in_memory())
}

fn scopes_of(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

fn recall_ids(store: &MemoryStore, query: &str, scope: &str) -> Vec<String> {
    let mut ids: Vec<String> = store
        .recall_scored(query, 10, &scopes_of(&[scope]))
        .into_iter()
        .map(|(entry, _)| entry.id)
        .collect();
    ids.sort();
    ids
}

// ---------------------------------------------------------------------------
// Signed-review sandbox (mirrors `src/cortex_lifecycle.rs` Sandbox).
// File-backed store is required: review trust resolves through
// `event_db_path()`, which is absent for in-memory databases.
// ---------------------------------------------------------------------------

struct Sandbox {
    _dir: tempfile::TempDir,
    store: MemoryStore,
    key: Ed25519KeyPair,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp sandbox dir");
        let store = MemoryStore::try_open(
            MemDb::open(&dir.path().join("cortex.db")).expect("file-backed memdb opens"),
        )
        .expect("sandbox store opens");
        let key = Ed25519KeyPair::from_seed_unchecked(&[19; 32]).expect("fixture key");
        let trust = ReviewerTrustV1 {
            schema_version: 1,
            installation_id: store.installation_id().to_owned(),
            cortex_store_id: store.cortex_store_id(),
            reviewers: vec![ReviewerKeyV1 {
                key_id: "fixture-reviewer".into(),
                public_key_hex: hex::encode(key.public_key().as_ref()),
                repository_id: "repo".into(),
                scope_id: "scope".into(),
                allowed_operations: vec![
                    "approve".into(),
                    "reject".into(),
                    "retry".into(),
                    "suppress".into(),
                    "resume".into(),
                ],
                revoked: false,
            }],
        };
        std::fs::write(
            trust_path(&store).expect("review trust path"),
            serde_json::to_vec(&trust).expect("trust serializes"),
        )
        .expect("write review trust");
        Self {
            _dir: dir,
            store,
            key,
        }
    }

    fn effect(&self, operation: &str, target: &str, hash: &str, nonce: &str) -> ReviewedEffectV1 {
        let now = membrane_runtime::time::now_millis() as u64;
        let mut effect = ReviewedEffectV1 {
            schema_version: 1,
            policy_version: REVIEW_POLICY.into(),
            installation_id: self.store.installation_id().into(),
            cortex_store_id: self.store.cortex_store_id(),
            repository_id: "repo".into(),
            scope_id: "scope".into(),
            operation: operation.into(),
            target_id: target.into(),
            expected_content_hash: hash.into(),
            expected_control_revision: Some("none".into()),
            key_id: "fixture-reviewer".into(),
            nonce: nonce.into(),
            issued_at_ms: now,
            expires_at_ms: now + 60_000,
            signature_hex: String::new(),
        };
        self.sign(&mut effect);
        effect
    }

    fn sign(&self, effect: &mut ReviewedEffectV1) {
        effect.signature_hex =
            hex::encode(self.key.sign(&effect.signing_bytes().expect("signing bytes")).as_ref());
    }

    fn pending(&self, text: &str) -> (String, String) {
        let receipt = propose(
            &self.store,
            "repo",
            "scope",
            &json!({"text": text, "producer": "manual", "epistemicClass": "reported"}),
        )
        .expect("proposal stores");
        (
            receipt["proposalId"].as_str().expect("proposalId").into(),
            receipt["emissionHash"].as_str().expect("emissionHash").into(),
        )
    }

    fn admitted(&self, text: &str) -> String {
        let (id, hash) = self.pending(text);
        let receipt = review(
            &self.store,
            "repo",
            "scope",
            &json!(self.effect("approve", &id, &hash, &format!("approve-{id}"))),
        )
        .expect("approval reviews");
        assert_eq!(receipt["admissionState"], "completed", "{receipt}");
        receipt["admission"]["memoryId"]
            .as_str()
            .expect("memoryId")
            .to_owned()
    }
}

// ---------------------------------------------------------------------------
// CTX-006 — scope-local near duplicates
// ---------------------------------------------------------------------------

const DEDUP_CONTENT: &str =
    "The runbook ships only after the evidence bundle passes the offline gate.";

#[test]
fn ctx006_exact_duplicate_with_new_evidence_updates_metadata_only() {
    let s = store();
    let first = s
        .try_put("first", DEDUP_CONTENT, "global", MemoryTier::Semantic)
        .expect("first write admits");
    // Same normalized content under a different id, carrying evidence the
    // existing record lacks: metadata union, never a second record, and the
    // canonical content is untouched.
    let second = s
        .try_put_with_metadata(
            "second",
            DEDUP_CONTENT,
            "global",
            MemoryTier::Semantic,
            "2026-09-06T00:00:00Z",
            &["evidence:runbook-v2".to_owned()],
        )
        .expect("evidence-carrying exact duplicate resolves to the existing id");
    assert_eq!(second, first, "UpdateMetadataOnly names the existing record");
    assert_eq!(s.entries(100).len(), 1, "no silent second record may appear");
    let entry = &s.entries(100)[0];
    assert_eq!(entry.content, DEDUP_CONTENT, "canonical content is untouched");
    let source_ids: String = s
        .db()
        .lock()
        .query_row(
            "SELECT source_ids FROM memories WHERE id = ?1",
            rusqlite::params![first],
            |row| row.get(0),
        )
        .expect("source_ids readable");
    assert!(
        source_ids.contains("evidence:runbook-v2"),
        "new evidence refs union into the existing row: {source_ids}"
    );
    // Same content with no new evidence is a pure typed no-op.
    let third = s
        .try_put("third", DEDUP_CONTENT, "global", MemoryTier::Semantic)
        .expect("exact duplicate is an idempotent no-op");
    assert_eq!(third, first);
    assert_eq!(s.entries(100).len(), 1);
}

#[test]
fn ctx006_same_content_in_different_scope_is_not_a_duplicate() {
    let s = store();
    let left = s
        .try_put("note", DEDUP_CONTENT, "global", MemoryTier::Semantic)
        .expect("global write admits");
    // The near-duplicate scan is scope-local: identical bytes in another
    // scope admit as their own record instead of collapsing or conflicting.
    let right = s
        .try_put("note", DEDUP_CONTENT, "team", MemoryTier::Semantic)
        .expect("same content in another scope is not a duplicate");
    assert_ne!(left, right, "each scope owns its record: {left} vs {right}");
    assert_eq!(s.entries(100).len(), 2);
    assert_eq!(recall_ids(&s, "evidence bundle offline gate", "global"), vec![left.clone()]);
    assert_eq!(recall_ids(&s, "evidence bundle offline gate", "team"), vec![right.clone()]);
}

// ---------------------------------------------------------------------------
// CTX-007 — quarantine invisibility across recall/resolver/recipe seams
// ---------------------------------------------------------------------------

const CONFLICT_PRIMARY: &str = "The deployment pipeline runs the full contract suite before any push to the shared integration branch.";
const CONFLICT_NEAR: &str = "The deployment pipeline runs the full contract suite before any push to the shared integration trunk.";

#[test]
fn ctx007_quarantined_conflict_invisible_across_recall_resolver_recipe() {
    let s = store();
    let primary = s
        .try_put("primary", CONFLICT_PRIMARY, "global", MemoryTier::Semantic)
        .expect("primary write admits");
    // Only the final word differs: conflict band, not an exact duplicate.
    let error = s
        .try_put("near", CONFLICT_NEAR, "global", MemoryTier::Semantic)
        .expect_err("ambiguous near-duplicate must surface, not admit silently");
    assert!(
        error.contains("admission conflict") && error.contains("quarantine"),
        "typed receipt required, got: {error}"
    );
    let quarantined_id = "global/near".to_owned();
    assert_eq!(
        s.quarantined_ids(),
        vec![quarantined_id.clone()],
        "the candidate is preserved outside active recall"
    );
    // Invisibility across every ordinary recall seam.
    assert_eq!(
        recall_ids(&s, "contract suite integration", "global"),
        vec![primary.clone()],
        "recall_scored serves the primary, never the quarantined candidate"
    );
    assert!(
        !s.search("contract suite integration", 10)
            .iter()
            .any(|entry| entry.id == quarantined_id),
        "lexical search never serves quarantine"
    );
    assert!(
        !s.entries(100).iter().any(|entry| entry.id == quarantined_id),
        "the registry never serves quarantine"
    );
    let recipe = recall_recipe(
        &s,
        "global",
        &json!({
            "query": "contract suite integration",
            "recipe": {"name": "cortex.hybrid", "version": 1},
            "bounds": {"maxItems": 10, "maxPreviewChars": 120},
            "projection": "preview"
        }),
    )
    .expect("recipe recall runs");
    let recipe_ids: Vec<&str> = recipe["items"]
        .as_array()
        .expect("recipe items")
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .collect();
    assert!(
        !recipe_ids.contains(&quarantined_id.as_str()),
        "recipe recall never serves quarantine: {recipe_ids:?}"
    );
    // The resolver reads canonical active state: a quarantine-only id is
    // unavailable in caller scope, never a silent empty page.
    let resolve_error =
        resolve_memory(&s, "global", &quarantined_id, &digest_str(CONFLICT_NEAR), 0, 12_000)
            .expect_err("quarantined rows are not resolvable");
    assert_eq!(resolve_error.code, "memory_unavailable", "{resolve_error}");
    // Governed restore re-enters admission instead of inserting raw truth: the
    // still-conflicting candidate is typed, and no silent second active row
    // ever appears.
    s.restore_quarantined(&quarantined_id)
        .expect("governed restore is typed");
    let active_near = s
        .entries(100)
        .into_iter()
        .filter(|entry| entry.content.contains("integration trunk"))
        .count();
    assert!(
        active_near <= 1,
        "restore must never create duplicate active truth: {active_near}"
    );
}

// ---------------------------------------------------------------------------
// CTX-009 — point-in-time round-trip through the governed lifecycle
// ---------------------------------------------------------------------------

#[test]
fn ctx009_temporal_round_trip_keeps_observed_valid_recorded_expiry_distinct() {
    let sandbox = Sandbox::new();
    // Observed, valid-from, and expiry are three distinct instants so the
    // round-trip must keep them apart rather than coalescing.
    let fact = TemporalFact {
        fact_id: "fact-acceptance-1".into(),
        subject: "oncall-roster".into(),
        predicate: "primary".into(),
        object: json!("ada"),
        scope_id: "scope".into(),
        authority: "A1".into(),
        veracity: "supported".into(),
        observed_at: "2026-03-01T00:00:00Z".into(),
        valid_from: "2026-01-15T00:00:00Z".into(),
        valid_until: None,
        expires_at: Some("2027-06-01T00:00:00Z".into()),
        supersedes: None,
    };
    let proposed =
        propose_temporal(&sandbox.store, "repo", "scope", &fact, &[]).expect("temporal proposes");
    let proposal_id = proposed["proposalId"].as_str().expect("proposalId").to_owned();
    let emission_hash = proposed["emissionHash"].as_str().expect("emissionHash").to_owned();
    // Caller-asserted authority/veracity never reach the receipt: intake
    // strips them and admission independently verifies.
    assert_eq!(
        proposed["provenance"]["callerFieldsStripped"],
        json!(["authority", "veracity", "supersedes"])
    );
    let reviewed = review(
        &sandbox.store,
        "repo",
        "scope",
        &json!(sandbox.effect("approve", &proposal_id, &emission_hash, "temporal-approve")),
    )
    .expect("temporal approval reviews");
    assert_eq!(reviewed["admissionState"], "completed", "{reviewed}");
    let admission = &reviewed["admission"];
    assert_eq!(admission["outcome"], "temporal_admitted", "{admission}");
    assert_eq!(admission["factId"], json!("fact-acceptance-1"), "{admission}");
    assert_eq!(admission["authority"], json!("A4"), "{admission}");
    assert_eq!(admission["independentlyVerified"], json!(true), "{admission}");
    // The canonical validity receipt round-trips after admission.
    let receipt = sandbox
        .store
        .temporal_facts()
        .validity_receipt("fact-acceptance-1")
        .expect("receipt readable")
        .expect("admitted fact has a receipt");
    assert_eq!(receipt.record_id, "fact-acceptance-1");
    assert_eq!(receipt.payload_sha256, admission["payloadHash"].as_str().expect("payloadHash"));
    // Point-in-time read keeps the four temporal dimensions distinct.
    let outcome = sandbox
        .store
        .temporal_facts()
        .query_validity(
            vec!["scope".to_owned()],
            "oncall-roster".to_owned(),
            "primary".to_owned(),
            "2026-06-01T00:00:00Z".to_owned(),
        )
        .expect("point-in-time query reads");
    assert!(outcome.conflict.is_none(), "single-valued read resolves");
    assert_eq!(outcome.records.len(), 1);
    let record_value = serde_json::to_value(&outcome.records[0]).expect("record serializes");
    assert_eq!(
        record_value.pointer("/validAt"),
        Some(&json!({"status": "known", "value": "2026-01-15T00:00:00Z"})),
        "valid time is the fact's own interval start: {record_value}"
    );
    assert_eq!(
        record_value.pointer("/recordedAt"),
        Some(&json!({"status": "known", "value": "2026-03-01T00:00:00Z"})),
        "recorded time is observation, not validity: {record_value}"
    );
    assert_eq!(
        record_value.pointer("/expiresAt"),
        Some(&json!("2027-06-01T00:00:00Z")),
        "expiry stays distinct: {record_value}"
    );
    assert_eq!(record_value.pointer("/authority"), Some(&json!("A4")));
    assert_ne!(
        record_value.pointer("/validAt"),
        record_value.pointer("/recordedAt"),
        "observed and valid must never coalesce"
    );
    // TemporalInstantV1 vocabulary is the canonical one on both sides.
    assert_eq!(
        outcome.records[0].valid_at,
        TemporalInstantV1::known("2026-01-15T00:00:00Z")
    );
}

// ---------------------------------------------------------------------------
// CTX-011 — FTS5 lexical recall under scope/lifecycle gates
// ---------------------------------------------------------------------------

#[test]
fn ctx011_suppressed_and_quarantined_rows_excluded_from_recall_scored() {
    let sandbox = Sandbox::new();
    let text = "Suppression acceptance fixture: the northbound signal relay calibration.";
    let id = sandbox.admitted(text);
    let hash = digest_str(text);
    assert_eq!(
        recall_ids(&sandbox.store, "signal relay calibration", "scope"),
        vec![id.clone()],
        "the active record recalls before suppression"
    );
    let receipt = review(
        &sandbox.store,
        "repo",
        "scope",
        &json!(sandbox.effect("suppress", &id, &hash, "suppress-acceptance")),
    )
    .expect("suppress reviews");
    assert_eq!(receipt["suppressed"], json!(true), "{receipt}");
    assert!(
        sandbox.store.recall_scored("signal relay calibration", 10, &scopes_of(&["scope"])).is_empty(),
        "suppressed rows leave ordinary recall"
    );
    assert!(
        sandbox
            .store
            .recall_scored_detailed("signal relay calibration", 10, &scopes_of(&["scope"]))
            .is_empty(),
        "suppressed rows leave detailed recall"
    );
    let recipe = recall_recipe(
        &sandbox.store,
        "scope",
        &json!({
            "query": "signal relay calibration",
            "recipe": {"name": "cortex.hybrid", "version": 1},
            "bounds": {"maxItems": 10, "maxPreviewChars": 120},
            "projection": "preview"
        }),
    )
    .expect("recipe recall runs");
    assert!(
        recipe["items"].as_array().expect("items").is_empty(),
        "suppressed rows leave recipe recall: {recipe}"
    );
    assert_eq!(
        resolve_memory(&sandbox.store, "scope", &id, &hash, 0, 12_000)
            .expect_err("suppressed rows are not resolvable")
            .code,
        "memory_ineligible"
    );
    // Governed reversal restores eligibility without rewriting content.
    let mut resume = sandbox.effect(
        "resume",
        &id,
        &hash,
        "resume-acceptance",
    );
    resume.expected_control_revision =
        receipt["decisionHash"].as_str().map(str::to_owned);
    sandbox.sign(&mut resume);
    review(&sandbox.store, "repo", "scope", &json!(resume)).expect("resume reviews");
    assert_eq!(
        recall_ids(&sandbox.store, "signal relay calibration", "scope"),
        vec![id.clone()],
        "resume restores ordinary recall"
    );
    assert_eq!(
        resolve_memory(&sandbox.store, "scope", &id, &hash, 0, 12_000)
            .expect("resumed rows resolve")["content"],
        json!(text)
    );
}

// ---------------------------------------------------------------------------
// CTX-012 — vector dispatch with exact legacy fallback
// ---------------------------------------------------------------------------

const DISPATCH_ENV: &str = "MEMBRANE_VECTOR_DISPATCH_V2";

static DISPATCH_SERIAL: Mutex<()> = Mutex::new(());

struct DispatchEnv {
    previous: Option<std::ffi::OsString>,
    _guard: MutexGuard<'static, ()>,
}

impl DispatchEnv {
    fn lock() -> Self {
        let guard = DISPATCH_SERIAL.lock().expect("dispatch serial lock");
        let previous = std::env::var_os(DISPATCH_ENV);
        Self { previous, _guard: guard }
    }

    fn set(&self, value: Option<&str>) {
        match value {
            Some(value) => std::env::set_var(DISPATCH_ENV, value),
            None => std::env::remove_var(DISPATCH_ENV),
        }
    }
}

impl Drop for DispatchEnv {
    fn drop(&mut self) {
        match &self.previous {
            Some(previous) => std::env::set_var(DISPATCH_ENV, previous),
            None => std::env::remove_var(DISPATCH_ENV),
        }
    }
}

fn dispatch_fixture_ids() -> Vec<String> {
    let s = store();
    s.try_put(
        "alpha",
        "Dispatch eligibility fixture alpha: the harbor crane manifest records berth assignments, tide windows, and cargo weights for the Tuesday rotation.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("alpha admits");
    s.try_put(
        "beta",
        "Dispatch eligibility fixture beta: the harbor crane schedule was revised after the storm delay, moving maintenance work to Friday dawn.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("beta admits");
    s.try_put(
        "gamma",
        "Dispatch eligibility fixture gamma: the tidal chart archive for the southern estuary sits in the map room.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("gamma admits");
    recall_ids(&s, "harbor crane", "global")
}

/// `vector_dispatch_v2_enabled` (`store.rs:131-139`) honors exactly
/// `0`/`false`/`off`/`legacy` (after trim) as legacy; unset or any other
/// value keeps the v2 index. Every setting must yield identical recall
/// eligibility with no remote dependency.
#[test]
fn ctx012_hybrid_recall_identical_eligibility_across_vector_dispatch_settings() {
    let env = DispatchEnv::lock();
    // Baseline: default dispatch (unset → v2 active).
    env.set(None);
    let baseline = dispatch_fixture_ids();
    assert_eq!(baseline.len(), 2, "both crane fixtures recall: {baseline}");
    // Legacy fallback values honored by the dispatch function, plus a
    // whitespace-padded variant proving the trim.
    for setting in ["0", "false", "off", "legacy", " false "] {
        env.set(Some(setting));
        let fallback = dispatch_fixture_ids();
        assert_eq!(
            fallback, baseline,
            "MEMBRANE_VECTOR_DISPATCH_V2={setting:?} must preserve eligibility"
        );
    }
    // Explicit v2 values stay on the default path with identical eligibility.
    for setting in ["1", "true", "v2"] {
        env.set(Some(setting));
        let explicit = dispatch_fixture_ids();
        assert_eq!(
            explicit, baseline,
            "MEMBRANE_VECTOR_DISPATCH_V2={setting:?} must preserve eligibility"
        );
    }
}

// ---------------------------------------------------------------------------
// CTX-013 — runtime fusion without score confusion
// ---------------------------------------------------------------------------

#[test]
fn ctx013_hybrid_recall_fuses_lanes_without_score_confusion() {
    let s = store();
    s.try_put(
        "one",
        "Fusion fixture one: the lexical-only marquee invoice appears here.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("one admits");
    s.try_put(
        "two",
        "Fusion fixture two: harbor crane logistics overlap the query terms.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("two admits");
    s.try_put(
        "three",
        "Fusion fixture three: marquee invoice harbor crane combined wording.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("three admits");
    let scopes = scopes_of(&["global"]);
    let first = s.recall_scored_detailed("marquee invoice harbor crane", 10, &scopes);
    let second = s.recall_scored_detailed("marquee invoice harbor crane", 10, &scopes);
    // No arm is dropped: every eligible row surfaces within budget.
    let mut first_ids: Vec<&str> = first.iter().map(|hit| hit.entry.id.as_str()).collect();
    first_ids.sort();
    assert_eq!(
        first_ids,
        vec!["global/one", "global/three", "global/two"],
        "lexical and vector candidates both fuse into recall"
    );
    for hit in &first {
        assert!(
            hit.score.is_finite() && (0.0..=1.0).contains(&hit.score),
            "scores stay cosine-ordered in [0,1]; lane rank never leaks as a score: {}",
            hit.score
        );
        assert!(
            matches!(hit.origin, "semantic" | "link"),
            "lane-local origin only, never a provider score: {}",
            hit.origin
        );
    }
    // Deterministic: repeated recall orders identically.
    let first_order: Vec<&str> = first.iter().map(|hit| hit.entry.id.as_str()).collect();
    let second_order: Vec<&str> = second.iter().map(|hit| hit.entry.id.as_str()).collect();
    assert_eq!(first_order, second_order, "fused order is deterministic");
    // Descending score order holds end to end.
    let scores: Vec<f32> = first.iter().map(|hit| hit.score).collect();
    let mut ordered = scores.clone();
    ordered.sort_by(|left, right| right.partial_cmp(left).unwrap_or(std::cmp::Ordering::Equal));
    assert_eq!(scores, ordered, "hits arrive score-ordered: {scores:?}");
}

// ---------------------------------------------------------------------------
// CTX-014 — bounded resolver: preview vs full fetch, observed-only use
// ---------------------------------------------------------------------------

const RESOLVER_CONTENT: &str =
    "Observed-use fixture body with enough characters to page across two windows.";

#[test]
fn ctx014_resolve_memory_pages_and_records_observed_use_only() {
    let s = store();
    let id = s
        .try_put_with_metadata(
            "observed",
            RESOLVER_CONTENT,
            "global",
            MemoryTier::Semantic,
            "2026-09-06T00:00:00Z",
            &["evidence:resolver-fixture".to_owned()],
        )
        .expect("fixture admits");
    let hash = digest_str(RESOLVER_CONTENT);
    let access_before: i64 = s
        .db()
        .lock()
        .query_row(
            "SELECT access_count FROM memories WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .expect("access_count readable");
    // Bounded preview page, then the remainder: paging completes the record.
    let page = resolve_memory(&s, "global", &id, &hash, 0, 10).expect("preview page resolves");
    assert_eq!(
        page["content"],
        json!(&RESOLVER_CONTENT.chars().take(10).collect::<String>())
    );
    assert_eq!(page["nextOffset"], json!(10));
    assert_eq!(page["complete"], json!(false));
    assert_eq!(page["pageComplete"], json!(true));
    let rest = resolve_memory(&s, "global", &id, &hash, 10, 12_000).expect("rest resolves");
    assert_eq!(rest["nextOffset"], Value::Null);
    let combined = format!(
        "{}{}",
        page["content"].as_str().expect("page text"),
        rest["content"].as_str().expect("rest text")
    );
    assert_eq!(combined, RESOLVER_CONTENT, "offset paging completes the record");
    assert_eq!(rest["totalChars"], json!(RESOLVER_CONTENT.chars().count()));
    // One full read is `complete`; the resolver reports provenance shape.
    let full = resolve_memory(&s, "global", &id, &hash, 0, 12_000).expect("full read resolves");
    assert_eq!(full["complete"], json!(true));
    assert_eq!(full["content"], json!(RESOLVER_CONTENT));
    assert_eq!(full["observedResolution"], json!(true));
    assert_eq!(
        full["verifiedHelped"], json!(false),
        "an observed read never claims verified help"
    );
    assert!(
        matches!(
            full["authority"].as_str(),
            Some("A1" | "A2" | "A3" | "A4" | "A5")
        ),
        "admitted authority is reported distinctly: {full}"
    );
    assert_eq!(full["provenanceAvailability"], json!("explicit"));
    assert_eq!(full["originProducer"], json!("manual"));
    // Observed use is durable: three resolutions above bumped the counter.
    let access_after: i64 = s
        .db()
        .lock()
        .query_row(
            "SELECT access_count FROM memories WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .expect("access_count readable");
    assert_eq!(
        access_after,
        access_before + 3,
        "each resolution records exactly one observed use"
    );
    // Scope and content-hash bindings hold.
    assert_eq!(
        resolve_memory(&s, "other", &id, &hash, 0, 12_000)
            .expect_err("cross-scope resolution is denied")
            .code,
        "memory_unavailable"
    );
    assert_eq!(
        resolve_memory(&s, "global", &id, &digest_str("different"), 0, 12_000)
            .expect_err("stale hash is a version conflict")
            .code,
        "memory_version_conflict"
    );
    assert_eq!(
        resolve_memory(&s, "global", &id, &hash, RESOLVER_CONTENT.chars().count() + 1, 12_000)
            .expect_err("offset past the end is rejected")
            .code,
        "memory_envelope_invalid"
    );
}

// ---------------------------------------------------------------------------
// CTX-020 — bounded emission intake: pending/quarantine + readback
// ---------------------------------------------------------------------------

#[test]
fn ctx020_oversize_emission_rejected_before_any_write() {
    let s = store();
    let oversize = "x".repeat(MAX_PAYLOAD_BYTES + 1);
    let error = propose(
        &s,
        "repo",
        "scope",
        &json!({"text": oversize, "producer": "manual", "epistemicClass": "reported"}),
    )
    .expect_err("oversize emission must fail closed");
    assert_eq!(error.code, "proposal_payload_too_large", "{error}");
    assert!(s.entries(100).is_empty(), "no durable truth from a rejected emission");
    let proposal_rows: i64 = {
        let conn = s.db().lock_events();
        let table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'membrane_knowledge_proposal'",
                [],
                |row| row.get(0),
            )
            .expect("schema readable");
        if table == 0 {
            0
        } else {
            conn.query_row("SELECT COUNT(*) FROM membrane_knowledge_proposal", [], |row| {
                row.get(0)
            })
            .expect("proposal count readable")
        }
    };
    assert_eq!(proposal_rows, 0, "rejection happens before any pending write");
}

#[test]
fn ctx020_pending_proposal_readback_never_auto_admits() {
    let s = store();
    let receipt = propose(
        &s,
        "repo",
        "scope",
        &json!({"text": "A bounded emission awaiting governed review.", "producer": "manual", "epistemicClass": "reported"}),
    )
    .expect("bounded emission stores pending");
    assert_eq!(receipt["status"], json!("needs_review"));
    assert_eq!(receipt["reviewState"], json!("pending"));
    assert_eq!(receipt["durable"], json!(true));
    assert_eq!(
        receipt["readbackDigest"], receipt["emissionHash"],
        "readback digest binds the stored bytes: {receipt}"
    );
    let proposal_id = receipt["proposalId"].as_str().expect("proposalId").to_owned();
    let status =
        proposal_status(&s, "repo", "scope", &proposal_id).expect("proposal status reads back");
    assert_eq!(status["proposalId"], json!(proposal_id));
    assert_eq!(status["admissionState"], json!("not_requested"));
    assert!(
        s.entries(100).is_empty(),
        "a pending proposal never becomes durable truth on its own"
    );
}

// ---------------------------------------------------------------------------
// CTX-034 — skill index → bounded read → Pull handle shape
// ---------------------------------------------------------------------------

#[test]
fn ctx034_skill_index_search_bounded_read_handle_shape() {
    // Ingest reads only git-tracked `tools/skills/<name>/SKILL.md` inside the
    // given workspace, so the fixture is a disposable git checkout under
    // tempfile — no fixtures outside it.
    let dir = tempfile::tempdir().expect("temp workspace");
    let skill_dir = dir.path().join("tools").join("skills").join("acceptance-skill");
    std::fs::create_dir_all(&skill_dir).expect("skill dir");
    let body = "---\ndescription: Acceptance fixture skill for bounded reads\n---\n\n# Acceptance\n\nDo bounded reads with privacy placeholder {{membrane-private:token}}.\n";
    std::fs::write(skill_dir.join("SKILL.md"), body).expect("SKILL.md");
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .output()
            .expect("git runs")
    };
    assert!(run(&["init"]).status.success(), "git init works");
    assert!(
        run(&["add", "tools/skills/acceptance-skill/SKILL.md"]).status.success(),
        "git stages the fixture skill"
    );
    let s = store();
    let (ingested, _skipped, _pruned) = s.ingest_skills(dir.path());
    assert_eq!(ingested, 1, "the tracked fixture skill ingests");
    // Index search returns the governed entry.
    let found = s.search_skills("acceptance bounded", 10).expect("skill search runs");
    assert_eq!(found.items.len(), 1);
    let entry = &found.items[0];
    assert_eq!(entry.name, "acceptance-skill");
    assert!(entry.description.contains("Acceptance fixture"), "{}", entry.description);
    assert!(!entry.body_hash.is_empty(), "content hash backs the Pull handle");
    // The Pull resolver handle shape (`federation_sources.rs:363-369`) maps
    // name → id, description → title, body_hash → source_hash, snapshot
    // generation → generation: every mapped input is pinned here.
    let snapshot = s.skills_snapshot().expect("skills snapshot reads");
    assert!(!snapshot.generation.is_empty(), "generation backs the handle receipt");
    assert!(
        snapshot.skills.iter().any(|skill| skill.name == entry.name
            && skill.body_hash == entry.body_hash
            && skill.description == entry.description),
        "index, search, and snapshot agree on the entry"
    );
    // Bounded read truncates with an exact cause; a full read is exact.
    // The caller holds the privacy value, so the placeholder restores at
    // response time without ever being written back to Cortex.
    let mut privacy = BTreeMap::new();
    privacy.insert("token".to_owned(), "secret-value".to_owned());
    let short = s
        .skill_read_bounded("acceptance-skill", 5, &privacy)
        .expect("bounded skill read runs");
    assert_eq!(short.body.chars().count(), 5);
    assert_eq!(short.completeness.state, CortexCompletenessState::LowerBound);
    assert!(short.completeness.causes.contains(&"content_truncated".to_owned()));
    assert_eq!(short.stored_body_hash, entry.body_hash);
    let full = s
        .skill_read_bounded("acceptance-skill", 100_000, &privacy)
        .expect("full skill read runs");
    assert_eq!(full.completeness.state, CortexCompletenessState::Exact);
    assert!(full.body.contains("# Acceptance"));
    assert_eq!(full.restored_privacy_values, 1);
    assert_eq!(full.unresolved_privacy_values, 0);
    assert!(full.body.contains("secret-value"), "caller-held value restores at read time");
    // Unknown skills and invalid names fail typed, never empty.
    assert!(s.skill_read_bounded("no-such-skill", 100, &privacy).is_err());
    assert!(s.skill_read_bounded("bad name!", 100, &privacy).is_err());
    assert!(s.skill_read_bounded("acceptance-skill", 0, &privacy).is_err());
}

// ---------------------------------------------------------------------------
// CTX-038 — exact/lower-bound envelopes with counts
// ---------------------------------------------------------------------------

#[test]
fn ctx038_list_envelope_exact_then_lower_bound_when_capped() {
    let s = store();
    for (name, content) in [
        ("one", "Envelope list fixture one: the zebra atlas entry catalogs highland migration corridors."),
        ("two", "Envelope list fixture two: the harbor ledger reconciles berth fees against tide tables."),
        ("three", "Envelope list fixture three: the signal log tracks relay calibration across night shifts."),
    ] {
        s.try_put(name, content, "global", MemoryTier::Semantic)
            .expect("fixture admits");
    }
    let full = s.try_list_bounded(Some("global"), 10).expect("uncapped list reads");
    assert_eq!(full.completeness.state, CortexCompletenessState::Exact);
    assert!(full.completeness.counts_exact);
    assert!(full.completeness.causes.is_empty());
    assert_eq!(full.completeness.considered_count, 3);
    assert_eq!(full.completeness.returned_count, 3);
    assert_eq!(full.completeness.dropped_count, 0);
    assert_eq!(full.items.len(), 3);
    let capped = s.try_list_bounded(Some("global"), 2).expect("capped list reads");
    assert_eq!(capped.completeness.state, CortexCompletenessState::LowerBound);
    assert!(!capped.completeness.counts_exact);
    assert_eq!(capped.completeness.causes, vec!["ceiling_truncated".to_owned()]);
    assert_eq!(capped.items.len(), 2);
    assert_eq!(capped.completeness.returned_count, 2);
    assert!(capped.completeness.dropped_count >= 1);
    assert!(s.try_list_bounded(Some("global"), 0).is_err(), "zero limit is rejected");
}

#[test]
fn ctx038_review_due_envelope_exact_then_lower_bound() {
    use membrane_runtime::time::now_millis;

    let s = store();
    for (name, content) in [
        ("due-one", "Review-due fixture due-one: the clock tower ledger records chime maintenance windows."),
        ("due-two", "Review-due fixture due-two: the observatory roster tracks night watch rotations."),
    ] {
        s.try_put(name, content, "global", MemoryTier::Semantic)
            .expect("fixture admits");
    }
    s.try_put(
        "not-due",
        "Review-due fixture not-due: the greenhouse manifest lists seedling trays by row.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("fixture admits");
    // Fixture setup only: arm the review clock in the past. The read seam
    // under test (`lifecycle_reviews_due`) performs no mutation itself.
    let now = now_millis() as i64;
    {
        let conn = s.db().lock();
        for name in ["due-one", "due-two"] {
            conn.execute(
                "UPDATE memories SET review_after_ms = ?1 WHERE id = ?2",
                rusqlite::params![now - 1_000, format!("global/{name}")],
            )
            .expect("arm review clock");
        }
    }
    let full = s
        .lifecycle_reviews_due(Some("global"), now, 10)
        .expect("uncapped review-due reads");
    assert_eq!(full.completeness.state, CortexCompletenessState::Exact);
    assert_eq!(full.items.len(), 2);
    assert!(full.items.iter().all(|row| row.reason == "review_after_elapsed"));
    assert!(full.items.iter().all(|row| row.lifecycle_state == "active"));
    let capped = s
        .lifecycle_reviews_due(Some("global"), now, 1)
        .expect("capped review-due reads");
    assert_eq!(capped.completeness.state, CortexCompletenessState::LowerBound);
    assert_eq!(capped.completeness.causes, vec!["ceiling_truncated".to_owned()]);
    assert_eq!(capped.items.len(), 1);
    assert!(s.lifecycle_reviews_due(Some("global"), now, 0).is_err());
    // Surfacing review-due mutates nothing: the rows stay active and eligible.
    assert_eq!(
        recall_ids(&s, "review-due fixture", "global").len(),
        3,
        "review-due is a read-only trigger, never a rewrite"
    );
}

#[test]
fn ctx038_recall_envelope_exact_uncapped_lower_bound_capped_or_cancelled() {
    use tokio_util::sync::CancellationToken;

    let s = store();
    s.try_put(
        "alpha",
        "Recall envelope fixture alpha: the indigo envelope ledger holds the quarterly allocation tables.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("alpha admits");
    s.try_put(
        "beta",
        "Recall envelope fixture beta: the indigo envelope index maps drawer numbers to wax seals.",
        "global",
        MemoryTier::Semantic,
    )
    .expect("beta admits");
    let scopes = scopes_of(&["global"]);
    let live = CancellationToken::new();
    let (hits, _, completeness) =
        s.recall_scored_detailed_timed_cancellable("indigo envelope", 10, &scopes, false, &live);
    assert_eq!(hits.len(), 2, "both fixtures recall uncapped");
    assert_eq!(completeness.state, CortexCompletenessState::Exact);
    assert!(completeness.counts_exact);
    assert_eq!(completeness.considered_count, 2);
    assert_eq!(completeness.returned_count, 2);
    assert_eq!(completeness.dropped_count, 0);
    let (capped_hits, _, capped) =
        s.recall_scored_detailed_timed_cancellable("indigo envelope", 1, &scopes, false, &live);
    assert_eq!(capped_hits.len(), 1);
    assert_eq!(capped.state, CortexCompletenessState::LowerBound);
    assert_eq!(capped.causes, vec!["ceiling_truncated".to_owned()]);
    assert_eq!(capped.considered_count, 2);
    assert_eq!(capped.returned_count, 1);
    assert_eq!(capped.dropped_count, 1);
    let (empty, _, zero) =
        s.recall_scored_detailed_timed_cancellable("indigo envelope", 0, &scopes, false, &live);
    assert!(empty.is_empty());
    assert_eq!(zero.state, CortexCompletenessState::Exact);
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let (no_hits, _, aborted) =
        s.recall_scored_detailed_timed_cancellable("indigo envelope", 10, &scopes, false, &cancelled);
    assert!(no_hits.is_empty());
    assert_eq!(aborted.state, CortexCompletenessState::LowerBound);
    assert_eq!(aborted.causes, vec!["cancelled".to_owned()]);
}

// ---------------------------------------------------------------------------
// CTX-030 — explain/browse read-only bounded truthful output
// ---------------------------------------------------------------------------

#[test]
fn ctx030_explain_projection_is_read_only_with_lifecycle_and_provenance() {
    // File-backed store so the real `cli::explain_memory` seam (db path in,
    // bounded JSON out) is exercised, not a mirrored query.
    let dir = tempfile::tempdir().expect("explain sandbox dir");
    let path = dir.path().join("cortex.db");
    let s = MemoryStore::try_open(MemDb::open(&path).expect("explain memdb opens"))
        .expect("explain store opens");
    let id = s
        .try_put_with_metadata(
            "explained",
            "Explain fixture: the bounded glass observatory catalog.",
            "global",
            MemoryTier::Semantic,
            "2026-09-06T00:00:00Z",
            &["evidence:observatory".to_owned()],
        )
        .expect("fixture admits");
    let entries_before = s.entries(100).len();
    let access_before: i64 = s
        .db()
        .lock()
        .query_row(
            "SELECT access_count FROM memories WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .expect("access_count readable");
    let explained: Value = explain_memory(path.to_str().unwrap(), &id).expect("explain reads");
    assert_eq!(explained["schema"], json!("membrane.memory-explain.v1"));
    assert_eq!(explained["id"], json!(id));
    assert_eq!(explained["scope_id"], json!("global"));
    assert_eq!(explained["lifecycle"]["state"], json!("active"));
    assert!(explained["authority"].as_str().is_some_and(|value| !value.is_empty()));
    assert!(explained["producer"].as_str().is_some_and(|value| !value.is_empty()));
    assert!(
        explained["source_ids"].as_str().is_some_and(|value| value.contains("evidence:observatory")),
        "provenance is explicit, never guessed: {explained}"
    );
    assert!(explained.get("content").is_none(), "explain carries no payload text");
    assert!(explain_memory(path.to_str().unwrap(), "global/missing").is_err());
    // Read-only proof: counters and registry are untouched.
    let access_after: i64 = s
        .db()
        .lock()
        .query_row(
            "SELECT access_count FROM memories WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .expect("access_count readable");
    assert_eq!(access_after, access_before, "explain records no use");
    assert_eq!(s.entries(100).len(), entries_before, "explain mutates nothing");
}
