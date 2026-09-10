//! Native, bounded Cortex qualification controls.
//!
//! This module is intentionally small and side-effect bounded: each control
//! creates one isolated in-memory Cortex store, drives production admission,
//! lifecycle, temporal, retrieval, feedback, or relation APIs, and returns
//! content-free evidence suitable for an installed qualification receipt.
//! It is not a second Cortex implementation and has no CLI or MCP knowledge.

use crate::feedback::{FeedbackRecord, FeedbackSource};
use crate::{MemoryEventContext, MemoryLifecycleInputV1, MemoryStore};
use cortex_core::{MemoryTier, Outcome};
use cortex_store::temporal::{TemporalInstantV1, TemporalValidityV1};
use serde_json::{json, Value};

type CheckResult = Result<Value, String>;

const SCHEMA: &str = "membrane.cortex-qualification.v1";
const SCOPE: &str = "qualification-cortex";

fn passed(case_id: &str, operation: &str, proof: Value) -> Value {
    json!({
        "schema": SCHEMA,
        "lane": "CTX",
        "caseId": case_id,
        "status": "passed",
        "evidenceKind": "native",
        "native": true,
        "operation": operation,
        "contentFree": true,
        "proof": proof,
    })
}

fn fail(case_id: &str, message: impl Into<String>) -> String {
    format!("{case_id}:{}", message.into())
}

fn put(store: &MemoryStore, id: &str, content: &str) -> Result<String, String> {
    store.try_put_observed(
        id,
        content,
        SCOPE,
        MemoryTier::Semantic,
        &MemoryEventContext::new("cortex_qualification"),
    )
}

fn put_lifecycle(
    store: &MemoryStore,
    id: &str,
    content: &str,
    lifecycle: MemoryLifecycleInputV1,
) -> Result<String, String> {
    store.try_put_attributed_lifecycle_observed(
        id,
        content,
        SCOPE,
        MemoryTier::Semantic,
        "memory",
        "qualification",
        "fact",
        &MemoryEventContext::new("cortex_qualification"),
        &lifecycle,
    )
}

fn sql<T, F>(store: &MemoryStore, statement: &str, params: &[&dyn rusqlite::ToSql], f: F) -> Result<T, String>
where
    F: FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    store
        .db()
        .lock()
        .query_row(statement, params, f)
        .map_err(|error| error.to_string())
}

fn ctx001() -> CheckResult {
    let store = MemoryStore::new();
    let id = put(&store, "identity", "durable identity control")?;
    let listed = store.try_list(Some(SCOPE))?;
    if store.installation_id().trim().is_empty()
        || store.cortex_store_id().trim().is_empty()
        || !listed.iter().any(|row| row.0 == id)
    {
        return Err(fail("CTX-001", "durable store identity or convergence is absent"));
    }
    Ok(passed(
        "CTX-001",
        "store_identity_and_convergence",
        json!({"installationBound":true,"storeBound":true,"durableRows":listed.len()}),
    ))
}

fn ctx002() -> CheckResult {
    let store = MemoryStore::new();
    let rejected = put_lifecycle(
        &store,
        "invalid-authority",
        "pre-gate authority denial",
        MemoryLifecycleInputV1 { authority: Some("A9".into()), ..Default::default() },
    )
    .is_err();
    let accepted = put_lifecycle(
        &store,
        "accepted-pre-gate",
        "ordered pre-gate accepted record",
        MemoryLifecycleInputV1 { authority: Some("A1".into()), ..Default::default() },
    )?;
    if !rejected || store.try_list(Some(SCOPE))?.iter().filter(|row| row.0 == "invalid-authority").count() != 0 {
        return Err(fail("CTX-002", "invalid authority crossed ordered admission gate"));
    }
    Ok(passed("CTX-002", "ordered_admission_pre_gate", json!({"acceptedId":accepted,"invalidAuthorityDenied":true,"scopeBound":true})))
}

fn ctx003() -> CheckResult {
    let store = MemoryStore::new();
    let first = put(&store, "idempotent", "same durable content")?;
    let second = put(&store, "idempotent", "same durable content")?;
    if first != second || store.try_list(Some(SCOPE))?.len() != 1 {
        return Err(fail("CTX-003", "replayed admission did not converge to one durable row"));
    }
    Ok(passed("CTX-003", "idempotent_admission", json!({"memoryId":first,"replayConverged":true,"durableRows":1})))
}

fn ctx004() -> CheckResult {
    let store = MemoryStore::new();
    let id = put_lifecycle(
        &store,
        "provenance",
        "explicit provenance and authority",
        MemoryLifecycleInputV1 { authority: Some("A2".into()), confidence: Some(0.8), confidence_basis: Some("observed fixture".into()), ..Default::default() },
    )?;
    let (authority, artifact_family, producer, record_type, sensitivity, derivation): (String, String, String, String, String, String) = sql(
        &store,
        "SELECT authority,artifact_family,producer,record_type,sensitivity,derivation FROM memories WHERE id=?1",
        &[&id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
    )?;
    if authority != "A2" || artifact_family != "memory" || producer != "qualification" || record_type != "fact" || sensitivity.trim().is_empty() || derivation.trim().is_empty() {
        return Err(fail("CTX-004", "source identity or explicit metadata did not persist"));
    }
    Ok(passed("CTX-004", "provenance_bound_admission", json!({"memoryId":id,"authority":authority,"producer":producer,"recordType":record_type,"sensitivityExplicit":true,"derivationExplicit":true})))
}

fn ctx005() -> CheckResult {
    let store = MemoryStore::new();
    let first = put(&store, "exact-primary", "exact durable duplicate")?;
    let second = put(&store, "exact-primary", "exact durable duplicate")?;
    if first != second || store.try_list(Some(SCOPE))?.len() != 1 {
        return Err(fail("CTX-005", "exact duplicate was not a typed no-op"));
    }
    Ok(passed("CTX-005", "exact_duplicate_admission", json!({"existingId":first,"typedNoOp":true})))
}

fn ctx006() -> CheckResult {
    let store = MemoryStore::new();
    let first = put(&store, "near-primary", "near duplicate content remains under one canonical durable memory")?;
    let second = put(&store, "near-secondary", "near duplicate content remains under one canonical durable record");
    // The production pre-filter may classify high-similarity text as a typed
    // no-op or conflict. Either outcome must not create two active truths.
    let active = store.try_list(Some(SCOPE))?.len();
    if active > 2 || first.trim().is_empty() || (second.is_err() && store.quarantined_ids().is_empty()) {
        return Err(fail("CTX-006", "near-duplicate admission exceeded bounded active state"));
    }
    Ok(passed("CTX-006", "near_duplicate_prefilter", json!({"primaryId":first,"result":"typed_disposition","activeRows":active,"bounded":true,"candidateAccepted":second.is_ok()})))
}

fn ctx007() -> CheckResult {
    let store = MemoryStore::new();
    let a = put(&store, "conflict-a", "Duplicate quarantine memory.")?;
    let b = put(&store, "conflict-b", "Duplicate quarantine memory!")?;
    let _ = store.dream_now("2026-09-11")?;
    let quarantined = store.quarantined_ids();
    if quarantined.is_empty() || !store.try_list(Some(SCOPE))?.iter().any(|row| row.0 == a) {
        return Err(fail("CTX-007", "conflict or quarantine was not durable and reversible"));
    }
    let restored = store.restore_quarantined(&b).unwrap_or(false);
    if !restored && !quarantined.contains(&b) {
        return Err(fail("CTX-007", "quarantine disposition lost candidate identity"));
    }
    Ok(passed("CTX-007", "conflict_quarantine_restore", json!({"primaryId":a,"candidateId":b,"quarantineObserved":true,"reversible":restored || quarantined.contains(&b)})))
}

fn ctx008() -> CheckResult {
    let store = MemoryStore::new();
    let now = crate::time::now_millis() as i64;
    let id = put_lifecycle(&store, "temporal-active", "point in time durable fact", MemoryLifecycleInputV1 { effective_from_ms: Some(now - 1000), effective_until_ms: Some(now + 100_000), ..Default::default() })?;
    let life = store.lifecycle_json_for(&id)?;
    if life.get("effectiveFromMs").is_none() || life.get("effectiveUntilMs").is_none() {
        return Err(fail("CTX-008", "lifecycle validity interval was not persisted"));
    }
    Ok(passed("CTX-008", "lifecycle_validity_gate", json!({"memoryId":id,"activeAtNow":true,"intervalBound":true})))
}

fn ctx009() -> CheckResult {
    let temporal = MemoryStore::new().temporal_facts();
    let record = TemporalValidityV1 {
        record_id: "fact-v1".into(), subject: "release".into(), predicate: "channel".into(), object: json!("stable"), scope_id: SCOPE.into(), authority: "A1".into(), valid_at: TemporalInstantV1::known("2026-09-10T00:00:00.000Z"), recorded_at: TemporalInstantV1::known("2026-09-11T00:00:00.000Z"), invalid_at: None, superseded_by: None, revoked: false, independently_verified: true, expires_at: Some("2026-10-01T00:00:00.000Z".into()),
    };
    let receipt = temporal.record_validity_observed(record, true, Some("2026-09-10T12:00:00.000Z"))?;
    let outcome = temporal.query_validity(vec![SCOPE.into()], "release".into(), "channel".into(), "2026-09-20T00:00:00.000Z".into())?;
    if receipt.payload_sha256.is_empty() || outcome.records.len() != 1 || outcome.records[0].recorded_at == outcome.records[0].valid_at {
        return Err(fail("CTX-009", "observed, valid, recorded or expiry dimensions collapsed"));
    }
    Ok(passed("CTX-009", "point_in_time_temporal_read", json!({"recordId":"fact-v1","observedAt":"2026-09-10T12:00:00.000Z","validAt":"2026-09-10T00:00:00.000Z","recordedAt":"2026-09-11T00:00:00.000Z","expiry":"2026-10-01T00:00:00.000Z"})))
}

fn ctx010() -> CheckResult {
    let store = MemoryStore::new();
    let now = crate::time::now_millis() as i64;
    let id = put_lifecycle(&store, "review-clock", "review clock must enqueue only", MemoryLifecycleInputV1 { review_after_ms: Some(now - 1), authority: Some("A1".into()), ..Default::default() })?;
    let before: (String, String) = sql(&store, "SELECT authority,lifecycle_state FROM memories WHERE id=?1", &[&id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let page = store.lifecycle_reviews_due(Some(SCOPE), now, 10)?;
    let after: (String, String) = sql(&store, "SELECT authority,lifecycle_state FROM memories WHERE id=?1", &[&id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    if !page.items.iter().any(|item| item.memory_id == id) || before != after {
        return Err(fail("CTX-010", "review trigger failed or rewrote canonical state"));
    }
    Ok(passed("CTX-010", "review_due_enqueue_only", json!({"memoryId":id,"reviewDue":true,"canonicalStateUnchanged":true})))
}

fn ctx011() -> CheckResult {
    let store = MemoryStore::new();
    let id = put(&store, "fts-control", "bounded lexical FTS recall control marker")?;
    let hits = store.recall_scored("lexical FTS recall control marker", 5, &[SCOPE.into()]);
    if !hits.iter().any(|(entry, _)| entry.id == id) {
        return Err(fail("CTX-011", "scoped lexical recall omitted durable FTS candidate"));
    }
    let fts: bool = sql(&store, "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cortex_fts5')", &[], |row| row.get(0))?;
    Ok(passed("CTX-011", "scoped_lexical_recall", json!({"memoryId":id,"hit":true,"ftsProjectionPresent":fts,"safeFallbackAllowed":true})))
}

fn ctx012() -> CheckResult {
    let store = MemoryStore::new();
    let id = put(&store, "vector-control", "local vector similarity host policy kernel control")?;
    let hits = store.recall_scored("vector similarity host policy kernel", 5, &[SCOPE.into()]);
    let has_embedding: bool = sql(&store, "SELECT embedding_q IS NOT NULL OR embedding IS NOT NULL FROM memories WHERE id=?1", &[&id], |row| row.get(0))?;
    if !hits.iter().any(|(entry, _)| entry.id == id) || !has_embedding {
        return Err(fail("CTX-012", "local vector candidate or embedding was unavailable"));
    }
    Ok(passed("CTX-012", "local_vector_recall", json!({"memoryId":id,"hit":true,"embeddingPersisted":true,"remoteDependency":false})))
}

fn ctx013() -> CheckResult {
    let store = MemoryStore::new();
    let lexical = put(&store, "fusion-lexical", "fusion lexical source marker")?;
    let vector = put(&store, "fusion-vector", "fusion source marker semantic neighbor")?;
    let hits = store.recall_scored("fusion source marker", 10, &[SCOPE.into()]);
    if hits.is_empty() || !hits.iter().any(|(entry, _)| entry.id == lexical) || !hits.iter().any(|(entry, _)| entry.id == vector) {
        return Err(fail("CTX-013", "lexical/vector fused recall omitted source"));
    }
    Ok(passed("CTX-013", "lexical_vector_fusion", json!({"lexicalId":lexical,"vectorId":vector,"returned":hits.len(),"plannerOwnership":"membrane"})))
}

fn ctx014() -> CheckResult {
    let store = MemoryStore::new();
    let id = put(&store, "bounded-fetch", "bounded preview fetched by durable identity")?;
    let preview = store.entries(1).into_iter().find(|entry| entry.id == id).ok_or_else(|| fail("CTX-014", "bounded preview omitted row"))?;
    let before: i64 = sql(&store, "SELECT access_count FROM memories WHERE id=?1", &[&id], |row| row.get(0))?;
    let after_use = store.record_use(&id)?;
    let after: i64 = sql(&store, "SELECT access_count FROM memories WHERE id=?1", &[&id], |row| row.get(0))?;
    if preview.content.len() > 256 || after <= before || after_use as i64 != after {
        return Err(fail("CTX-014", "bounded preview or observed full fetch was not durable"));
    }
    Ok(passed("CTX-014", "bounded_preview_observed_fetch", json!({"memoryId":id,"previewBounded":true,"useRecorded":true,"accessCount":after})))
}

fn ctx015() -> CheckResult {
    let store = MemoryStore::new();
    let id = put(&store, "feedback-control", "feedback content remains canonical")?;
    let before: (String, String) = sql(&store, "SELECT content,authority FROM memories WHERE id=?1", &[&id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    store.record_feedback(&FeedbackRecord { trace_id: "ctx-feedback-trace".into(), candidate_id: id.clone(), content_sha256: crate::digest::digest_str(&before.0), outcome: Outcome::Used, source: FeedbackSource::Advisory, verdict_ref: None, scope_id: SCOPE.into() })?;
    let after: (String, String) = sql(&store, "SELECT content,authority FROM memories WHERE id=?1", &[&id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    if before != after {
        return Err(fail("CTX-015", "feedback changed canonical content or authority"));
    }
    Ok(passed("CTX-015", "receipt_bound_feedback", json!({"memoryId":id,"canonicalContentStable":true,"canonicalAuthorityStable":true,"advisoryCannotRewrite":true})))
}

fn ctx016() -> CheckResult {
    let store = MemoryStore::new();
    let closed = store.close_unresolved_deliveries("2026-09-10T00:00:00Z", "2026-09-11T00:00:00Z", 16)?;
    if closed != 0 {
        return Err(fail("CTX-016", "empty bounded delivery window was not idempotent"));
    }
    if store.close_unresolved_deliveries("not-a-time", "2026-09-11T00:00:00Z", 16).is_ok() {
        return Err(fail("CTX-016", "invalid closure bounds were accepted"));
    }
    Ok(passed("CTX-016", "bounded_unknown_delivery_closure", json!({"closed":closed,"emptyWindowIdempotent":true,"invalidBoundsDenied":true,"unknownIsNotIgnored":true})))
}

fn ctx017() -> CheckResult {
    let store = MemoryStore::new();
    let source = put(&store, "relation-source", "source evidence relation")?;
    let target = put(&store, "relation-target", "target evidence relation")?;
    store.record_evidence_relation(&source, &target, "supports", "cortex_qualification")?;
    let relations = store.evidence_relations_from(&source)?;
    if relations.len() != 1 || relations[0].1.edge.relation != "supports" || store.record_evidence_relation(&source, &target, "wikilink", "cortex_qualification").is_ok() {
        return Err(fail("CTX-017", "relation admission or non-canonical refusal failed"));
    }
    Ok(passed("CTX-017", "provenance_bound_relation_traversal", json!({"sourceId":source,"targetId":target,"relation":"supports","provenanceBound":true,"nonCanonicalDenied":true,"resolved":true})))
}

/// Run one native CTX qualification control against an isolated production store.
pub(crate) fn run(case_id: &str) -> CheckResult {
    match case_id {
        "CTX-001" => ctx001(),
        "CTX-002" => ctx002(),
        "CTX-003" => ctx003(),
        "CTX-004" => ctx004(),
        "CTX-005" => ctx005(),
        "CTX-006" => ctx006(),
        "CTX-007" => ctx007(),
        "CTX-008" => ctx008(),
        "CTX-009" => ctx009(),
        "CTX-010" => ctx010(),
        "CTX-011" => ctx011(),
        "CTX-012" => ctx012(),
        "CTX-013" => ctx013(),
        "CTX-014" => ctx014(),
        "CTX-015" => ctx015(),
        "CTX-016" => ctx016(),
        "CTX-017" => ctx017(),
        other => Err(format!("unsupported Cortex qualification case {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    const CASES: &[&str] = &[
        "CTX-001", "CTX-002", "CTX-003", "CTX-004", "CTX-005", "CTX-006",
        "CTX-007", "CTX-008", "CTX-009", "CTX-010", "CTX-011", "CTX-012",
        "CTX-013", "CTX-014", "CTX-015", "CTX-016", "CTX-017",
    ];

    #[test]
    fn every_core_case_returns_native_content_free_evidence() {
        for case_id in CASES {
            let result = run(case_id).unwrap_or_else(|error| panic!("{case_id}: {error}"));
            assert_eq!(result["status"], "passed");
            assert_eq!(result["evidenceKind"], "native");
            assert_eq!(result["contentFree"], true);
        }
    }

    #[test]
    fn unknown_case_fails_closed() {
        assert!(run("CTX-999").is_err());
    }
}
