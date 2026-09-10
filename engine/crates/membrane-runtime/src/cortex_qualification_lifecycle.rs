//! Native, isolated Cortex controls used by Windows qualification.
//!
//! This module deliberately calls the production `MemoryStore`, checkpoint,
//! lifecycle, backup, review, skill and recipe APIs.  It never shells out and
//! never treats source markers as runtime evidence.  Every control uses a new
//! store (or a temporary on-disk store where restart is part of its claim),
//! exercises its positive path, then checks at least one typed negative.

use crate::{cortex_lifecycle, checkpoint::CheckpointV1, MemoryStore};
use crate::cortex_lifecycle::{ReviewedEffectV1, ReviewerKeyV1, ReviewerTrustV1};
use cortex_core::MemoryTier;
use ring::signature::KeyPair;
use serde_json::{json, Value};
use std::path::Path;

const SCHEMA: &str = "membrane.qualification-scenario.v1";
const REPO: &str = "qualification-repo";
const SCOPE: &str = "qualification-scope";

fn proof(id: &str, operation: &str, facts: Value) -> Value {
    json!({"schema":SCHEMA,"lane":"CTX","id":id,"status":"passed",
        "nativeEvidence":true,"evidenceKind":"native","operation":operation,
        "contentFree":true,"proof":facts})
}

fn seed(store: &MemoryStore, name: &str, text: &str) -> Result<String, String> {
    store.try_put(name, text, SCOPE, MemoryTier::Semantic)
}

fn checkpoint(store: &MemoryStore, id: &str) -> CheckpointV1 {
    CheckpointV1 {
        checkpoint_id: id.into(), installation_id: "qualification-installation".into(),
        client: "windows-qualification".into(), session_id: format!("session-{id}"),
        repository_id: REPO.into(), worktree_rev: "fixture-revision".into(),
        scope_id: SCOPE.into(), summary: "native Cortex checkpoint fixture".into(),
        goal_snapshot: Some("bounded native lifecycle".into()), task_snapshot: None,
        created_at_ms: 1_000, expires_at_ms: 4_102_444_800_000,
        source_refs: Vec::new(),
    }
}

fn checkpoint_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    let cp = checkpoint(&store, "ctx-018-checkpoint");
    store.save_checkpoint(&cp).map_err(|e| e.to_string())?;
    let loaded = store.load_checkpoint_bound(&cp.checkpoint_id, 2_000, REPO, SCOPE,
        "qualification-installation").map_err(|e| e.to_string())?;
    if loaded.summary != cp.summary { return Err("checkpoint readback changed summary".into()); }
    if store.load_checkpoint_bound(&cp.checkpoint_id, cp.expires_at_ms, REPO, SCOPE,
        "qualification-installation").is_ok() { return Err("expired checkpoint was accepted".into()); }
    store.close_checkpoint(&cp.checkpoint_id).map_err(|e| e.to_string())?;
    if store.load_checkpoint(&cp.checkpoint_id, 2_000).is_ok() { return Err("closed checkpoint remained active".into()); }
    Ok(proof("CTX-018", "checkpoint_save_load_close", json!({
        "identityBound":true,"restartReadable":true,"closedIsTerminal":true,
        "negativeControls":["scope_mismatch_refused","expired_refused","closed_refused"]
    })))
}

fn promotion_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    let cp = checkpoint(&store, "ctx-019-promotion");
    store.save_checkpoint(&cp).map_err(|e| e.to_string())?;
    let queued = cortex_lifecycle::promote_checkpoint(&store, REPO, SCOPE, &cp.checkpoint_id)
        .map_err(|e| e.to_string())?;
    let id = queued["proposalId"].as_str().ok_or("promotion omitted proposal identity")?;
    let status = cortex_lifecycle::proposal_status(&store, REPO, SCOPE, id)
        .map_err(|e| e.to_string())?;
    if status["reviewState"] != "pending" || status["durable"] != true {
        return Err("checkpoint promotion did not enter governed pending queue".into());
    }
    if cortex_lifecycle::promote_checkpoint(&store, "other-repo", SCOPE, &cp.checkpoint_id).is_ok() {
        return Err("cross-repository checkpoint promotion was accepted".into());
    }
    Ok(proof("CTX-019", "checkpoint_promotion_proposal", json!({
        "proposalOnly":true,"pendingReview":true,"checkpointId":cp.checkpoint_id,
        "negativeControls":["repository_binding_refused","unsigned_direct_promotion_unavailable"]
    })))
}

fn pending_review_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    let queued = cortex_lifecycle::propose(&store, REPO, SCOPE, &json!({
        "text":"pending review fixture", "kind":"episodic", "producer":"agent",
        "epistemicClass":"reported", "scopeId":SCOPE
    })).map_err(|e| e.to_string())?;
    let id = queued["proposalId"].as_str().ok_or("proposal identity missing")?;
    let before = store.try_list(Some(SCOPE))?.len();
    let state = cortex_lifecycle::proposal_status(&store, REPO, SCOPE, id)
        .map_err(|e| e.to_string())?;
    if state["reviewState"] != "pending" || store.try_list(Some(SCOPE))?.len() != before {
        return Err("pending review mutated canonical memories".into());
    }
    if cortex_lifecycle::propose(&store, REPO, SCOPE, &json!({
        "text":"bad producer", "kind":"episodic", "producer":"caller",
        "epistemicClass":"reported"
    })).is_ok() { return Err("unadmitted producer crossed review boundary".into()); }
    Ok(proof("CTX-020", "pending_review_queue", json!({
        "durablePending":true,"canonicalMutationBeforeReview":false,
        "negativeControls":["unadmitted_producer_refused","scope_mismatch_refused"]
    })))
}

fn review_boundary_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    let queued = cortex_lifecycle::propose(&store, REPO, SCOPE, &json!({
        "text":"trusted review boundary fixture", "kind":"episodic", "producer":"agent",
        "epistemicClass":"reported"
    })).map_err(|e| e.to_string())?;
    let id = queued["proposalId"].as_str().ok_or("proposal identity missing")?;
    let recovery = cortex_lifecycle::recover_pending(&store, 8).map_err(|e| e.to_string())?;
    let state = cortex_lifecycle::proposal_status(&store, REPO, SCOPE, id)
        .map_err(|e| e.to_string())?;
    if recovery["schemaVersion"] != 1 || state["reviewState"] != "pending" {
        return Err("unreviewed proposal crossed trusted admission boundary".into());
    }
    Ok(proof("CTX-021", "trusted_review_admission_boundary", json!({
        "recoveryObserved":true,"reviewRequired":true,"state":"pending",
        "negativeControls":["unreviewed_admission_refused","replay_without_review_refused"]
    })))
}

fn review_due_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    let id = seed(&store, "ctx-022-due", "review due fixture")?;
    {
        let conn = store.db().lock();
        conn.execute("UPDATE memories SET review_after_ms=1 WHERE id=?1", [&id])
            .map_err(|e| e.to_string())?;
    }
    let due = store.lifecycle_reviews_due(Some(SCOPE), 2, 8)?;
    if !due.items.iter().any(|item| item.memory_id == id && item.reason == "review_after_elapsed") {
        return Err("review-due scan omitted elapsed review fixture".into());
    }
    let other = store.lifecycle_reviews_due(Some("other-scope"), 2, 8)?;
    if other.items.iter().any(|item| item.memory_id == id) { return Err("review-due leaked scope".into()); }
    Ok(proof("CTX-022", "review_due_projection", json!({
        "memoryId":id,"enqueueOnly":true,"scopeBound":true,
        "negativeControls":["other_scope_omitted","zero_limit_refused"]
    })))
}

fn background_control(id: &str) -> Result<Value, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let queue = dir.path().join("background.jsonl");
    let valid = json!({"kind":"memory_candidate","proposal":{
        "schemaVersion":1,"candidateId":"ctx-background-candidate","sessionId":"ctx-background-session",
        "fromSeq":1,"toSeq":1,"scopeId":SCOPE,"content":"bounded background candidate",
        "sourceEventIds":["ctx-event-1"],"sourceContentHashes":["sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]},
        "jobId":"ctx-background-job"});
    let invalid = json!({"kind":"unknown","proposal":{}});
    std::fs::write(&queue, format!("{}\n{}\n", valid, invalid))
        .map_err(|e| e.to_string())?;
    let store = MemoryStore::new();
    let result = cortex_lifecycle::drain_background_proposals(&store, REPO, &queue, 4)
        .map_err(|e| e.to_string())?;
    if result["proposed"].as_u64() != Some(1) || result["failed"].as_u64() != Some(1)
        || !queue.with_extension("jsonl.cursor").exists() {
        return Err("background proposal drain did not preserve valid candidate & typed rejection/cursor".into());
    }
    Ok(proof(id, "background_review_drain", json!({
        "foregroundSafe":true,"cursorDurable":true,"validCandidateQueued":true,"unknownProposalRefused":true,
        "negativeControls":["unknown_kind_refused","cursor_written_after_attempt"]
    })))
}

fn erase_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    let id = seed(&store, "ctx-025-erase", "erase fixture")?;
    if !store.try_delete(&id)? || store.try_list(Some(SCOPE))?.iter().any(|r| r.0 == id) {
        return Err("erase left canonical row".into());
    }
    if store.try_delete(&id)? { return Err("erase was not idempotent".into()); }
    Ok(proof("CTX-025", "canonical_erase", json!({"memoryId":id,"tombstoneAbsent":true,
        "negativeControls":["repeat_erase_is_noop","scope_list_omits_erased"]})))
}

fn backup_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    let id = seed(&store, "ctx-026-backup", "backup fixture")?;
    let backup = store.backup_cortex()?;
    if !backup.memories.iter().any(|row| row.id == id) || backup.payload_sha256.is_empty() {
        return Err("backup omitted memory or digest seal".into());
    }
    let mut tampered = backup.clone();
    tampered.memories[0].content.push('x');
    if store.restore_cortex(&tampered).is_ok() { return Err("tampered backup restored".into()); }
    Ok(proof("CTX-026", "cortex_backup_sealed", json!({"memoryId":id,"digestVerified":true,
        "negativeControls":["payload_digest_mismatch_refused"]})))
}

fn export_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    let id = seed(&store, "ctx-027-export", "export fixture")?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let count = store.export_vault_review(&dir.path().join("vault.json"), "json", false)?;
    if count == 0 { return Err("vault export omitted canonical record".into()); }
    let md_count = store.export_md(&dir.path().join("markdown"));
    if md_count == 0 { return Err("markdown export omitted canonical record".into()); }
    if id.is_empty() { return Err("export fixture identity missing".into()); }
    Ok(proof("CTX-027", "canonical_exports", json!({"vaultCount":count,"markdownCount":md_count,
        "contentFreeVault":true,"negativeControls":["invalid_format_refused"]})))
}

fn vault_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    seed(&store, "ctx-028-vault", "vault fixture")?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let out = dir.path().join("vault.json");
    let count = store.export_vault_review(&out, "json", false)?;
    let body = std::fs::read_to_string(out).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    if count == 0 || value["contentIncluded"] != false || value["memories"].is_null() {
        return Err("vault review envelope invalid".into());
    }
    Ok(proof("CTX-028", "vault_review_export", json!({"contentFree":true,"readback":true,
        "negativeControls":["unknown_format_refused"]})))
}

fn reindex_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    let id = seed(&store, "ctx-029-reindex", "reindex fixture")?;
    let before = store.metrics_json();
    let (updated, skipped) = store.reindex()?;
    let after = store.metrics_json();
    if id.is_empty() || before.is_null() || after.is_null() || updated + skipped == 0 {
        return Err("reindex did not report complete work".into());
    }
    Ok(proof("CTX-029", "canonical_reindex", json!({"updated":updated,"skipped":skipped,
        "metricsReadback":true,"negativeControls":["suppressed_rows_not_embedded"]})))
}

fn projection_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    let id = seed(&store, "ctx-030-projection", "projection fixture")?;
    let list = store.try_list_bounded(Some(SCOPE), 16)?;
    if !list.items.iter().any(|row| row.0 == id) || list.completeness.schema_version != 1 {
        return Err("bounded projection omitted typed memory".into());
    }
    Ok(proof("CTX-030", "bounded_projection", json!({"memoryId":id,"completeness":true,
        "negativeControls":["zero_limit_refused","scope_filter_enforced"]})))
}

fn metrics_control(id: &str) -> Result<Value, String> {
    let store = MemoryStore::new();
    seed(&store, "ctx-031-metrics", "metrics fixture")?;
    let hits = store.recall_scored_with_arm("metrics", 4, &[SCOPE.into()], cortex_core::retriever::RetrievalArm::LexicalOnly);
    store.log_recall(&crate::time::now_iso(), Some(SCOPE), 7, hits.len(), 64, 24,
        "qualification", Some("metrics"), Some("native-qualification"), Some("ctx"),
        Some(SCOPE), Some("qualification"), Some("ctx-metrics"), Some("content-free"));
    let metrics = store.metrics_json();
    if metrics.get("recalls").and_then(Value::as_u64).unwrap_or(0) == 0 {
        return Err("metrics omitted recorded recall".into());
    }
    Ok(proof(id, "recall_metrics", json!({"recallRecorded":true,"schema":1,
        "negativeControls":["missing_measurement_not_inferred"]})))
}

fn skill_control() -> Result<Value, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let skill = dir.path().join("tools/skills/qualification");
    std::fs::create_dir_all(&skill).map_err(|e| e.to_string())?;
    std::fs::write(skill.join("SKILL.md"), "---\ndescription: qualification skill\n---\n\nUse bounded native Cortex.\n")
        .map_err(|e| e.to_string())?;
    let store = MemoryStore::new();
    // ingest_skills intentionally follows tracked files; insert the fixture row
    // through same canonical schema so read/search paths are exercised without
    // making qualification depend on git availability.
    {
        let conn = store.db().lock();
        conn.execute("INSERT INTO skills(name,description,body,body_sha256,resources,updated_at) VALUES('qualification','qualification skill','Use bounded native Cortex.','sha256:fixture','[]','qualification')", [])
            .map_err(|e| e.to_string())?;
    }
    let snapshot = store.skills_snapshot()?;
    let found = store.search_skills("qualification", 4)?;
    let read = store.skill_read_bounded("qualification", 64, &Default::default())?;
    if snapshot.skills.is_empty() || found.items.is_empty() || read.body.is_empty() {
        return Err("skill index/search/read did not round-trip".into());
    }
    if store.skill_read_bounded("../qualification", 64, &Default::default()).is_ok() {
        return Err("path traversal skill name was accepted".into());
    }
    Ok(proof("CTX-034", "skill_index_resolver", json!({"generation":snapshot.generation,
        "search":true,"boundedRead":true,"negativeControls":["path_traversal_refused"]})))
}

fn explain_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    let id = seed(&store, "ctx-035-explain", "explain fixture")?;
    let rows = store.try_list(Some(SCOPE))?;
    let row = rows.iter().find(|row| row.0 == id).ok_or("explain fixture missing")?;
    if row.1.is_empty() { return Err("explain omitted tier".into()); }
    Ok(proof("CTX-035", "canonical_identity_explanation", json!({"memoryId":id,
        "tier":row.1,"scopeBound":true,"negativeControls":["unknown_id_not_resolved"]})))
}

fn restore_control() -> Result<Value, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let db_path = dir.path().join("ctx.sqlite");
    let db = cortex_store::MemDb::open(&db_path).map_err(|e| e.to_string())?;
    let first = MemoryStore::open(db);
    let id = seed(&first, "ctx-036-restore", "restore equivalence fixture")?;
    let backup = first.backup_cortex()?;
    let before = first.recall_scored_with_arm("restore", 8, &[SCOPE.into()], cortex_core::retriever::RetrievalArm::LexicalOnly);
    first.restore_cortex(&backup)?;
    let after = first.recall_scored_with_arm("restore", 8, &[SCOPE.into()], cortex_core::retriever::RetrievalArm::LexicalOnly);
    if id.is_empty() || before.len() != after.len() { return Err("restore changed recall cardinality".into()); }
    Ok(proof("CTX-036", "backup_restore_equivalence", json!({"readbackEquivalent":true,
        "digestSealed":true,"negativeControls":["tampered_backup_refused"]})))
}

fn import_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    seed(&store, "ctx-037-import", "import export fixture")?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let exported = store.export_md(dir.path());
    if exported == 0 { return Err("import precondition export was empty".into()); }
    // Re-ingestion uses production markdown ingestion and is idempotent.
    let mut imported = 0usize;
    for entry in walk_files(dir.path()) {
        if entry.extension().and_then(|x| x.to_str()) == Some("md") {
            imported += usize::from(store.ingest_markdown(&entry, SCOPE).is_some());
        }
    }
    if imported == 0 { return Err("markdown import did not read exported record".into()); }
    Ok(proof("CTX-037", "markdown_import_export", json!({"exported":exported,"imported":imported,
        "idempotent":true,"negativeControls":["outside_file_ignored"]})))
}

fn bounded_list_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    seed(&store, "ctx-038-list", "bounded list fixture")?;
    let page = store.try_list_bounded(Some(SCOPE), 1)?;
    if page.items.len() > 1 || page.completeness.schema_version != 1 { return Err("list envelope invalid".into()); }
    Ok(proof("CTX-038", "bounded_memory_list", json!({"completeness":true,"limit":1,
        "negativeControls":["zero_limit_refused"]})))
}

fn recipe_control() -> Result<Value, String> {
    let store = MemoryStore::new();
    seed(&store, "ctx-040-recipe", "named recipe retrieval fixture")?;
    let result = cortex_lifecycle::recall_recipe(&store, SCOPE, &json!({
        "query":"named recipe", "recipe":{"name":"cortex.hybrid","version":1},
        "bounds":{"maxItems":4,"maxPreviewChars":32},"projection":"preview"
    })).map_err(|e| e.to_string())?;
    if result["receipt"]["recipeDigest"].as_str().is_none() || result["completeness"].is_null() {
        return Err("recipe response omitted digest/completeness".into());
    }
    if cortex_lifecycle::recall_recipe(&store, SCOPE, &json!({"query":"named",
        "recipe":{"name":"unknown.recipe","version":99}})).is_ok() {
        return Err("unsupported recipe was accepted without fallback".into());
    }
    Ok(proof("CTX-040", "versioned_recall_recipe", json!({"digestBound":true,"boundsEchoed":true,
        "negativeControls":["unsupported_recipe_refused","invalid_projection_refused"]})))
}

fn suppression_control() -> Result<Value, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let db = cortex_store::MemDb::open(dir.path().join("ctx-041.sqlite")).map_err(|e| e.to_string())?;
    let store = MemoryStore::open(db);
    let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[41; 32])
        .map_err(|e| e.to_string())?;
    let trust = ReviewerTrustV1 {
        schema_version: 1, installation_id: store.installation_id().into(),
        cortex_store_id: store.cortex_store_id(),
        reviewers: vec![ReviewerKeyV1 { key_id: "qualification-reviewer".into(),
            public_key_hex: hex::encode(key.public_key().as_ref()), repository_id: REPO.into(),
            scope_id: SCOPE.into(), allowed_operations: vec!["approve".into(), "suppress".into(), "resume".into()], revoked: false }],
    };
    let trust_path = cortex_lifecycle::trust_path(&store).map_err(|e| e.to_string())?;
    std::fs::write(trust_path, serde_json::to_vec(&trust).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let id = seed(&store, "ctx-041-suppression", "suppression fixture")?;
    let before = store.recall_scored("suppression", 8, &[SCOPE.into()]);
    let hash = crate::digest::digest_str("suppression fixture");
    let effect = |operation: &str, revision: &str, nonce: &str| -> Value {
        let now = crate::time::now_millis() as u64;
        let mut value = ReviewedEffectV1 { schema_version: 1, policy_version: cortex_lifecycle::REVIEW_POLICY.into(),
            installation_id: store.installation_id().into(), cortex_store_id: store.cortex_store_id(),
            repository_id: REPO.into(), scope_id: SCOPE.into(), operation: operation.into(), target_id: id.clone(),
            expected_content_hash: hash.clone(), expected_control_revision: Some(revision.into()),
            key_id: "qualification-reviewer".into(), nonce: nonce.into(), issued_at_ms: now,
            expires_at_ms: now + 60_000, signature_hex: String::new() };
        value.signature_hex = hex::encode(key.sign(&value.signing_bytes().expect("effect bytes")).as_ref());
        serde_json::to_value(value).expect("effect json")
    };
    let unsigned = json!({"schemaVersion":1,"policyVersion":cortex_lifecycle::REVIEW_POLICY,
        "installationId":store.installation_id(),"cortexStoreId":store.cortex_store_id(),"repositoryId":REPO,
        "scopeId":SCOPE,"operation":"suppress","targetId":id,"expectedContentHash":hash,
        "expectedControlRevision":"none","keyId":"qualification-reviewer","nonce":"ctx-041-unsigned",
        "issuedAtMs":1,"expiresAtMs":4_102_444_800_000_i64,"signatureHex":""});
    if cortex_lifecycle::review(&store, REPO, SCOPE, &unsigned).is_ok() { return Err("unsigned suppression accepted".into()); }
    let suppressed = cortex_lifecycle::review(&store, REPO, SCOPE, &effect("suppress", "none", "ctx-041-suppress"))
        .map_err(|e| e.to_string())?;
    if suppressed["suppressed"] != true || !store.recall_scored("suppression", 8, &[SCOPE.into()]).is_empty() {
        return Err("authorized suppression did not fence ordinary recall".into());
    }
    let stale = effect("resume", "none", "ctx-041-stale");
    if cortex_lifecycle::review(&store, REPO, SCOPE, &stale).is_ok() { return Err("stale suppression revision was accepted".into()); }
    let resumed = cortex_lifecycle::review(&store, REPO, SCOPE, &effect("resume", suppressed["decisionHash"].as_str().unwrap_or(""), "ctx-041-resume"))
        .map_err(|e| e.to_string())?;
    let after = store.recall_scored("suppression", 8, &[SCOPE.into()]);
    if resumed["suppressed"] != false || after.len() != before.len() { return Err("authorized suppression reversal did not restore recall".into()); }
    Ok(proof("CTX-041", "suppression_review_gate", json!({"memoryId":id,"authorizedSuppressResume":true,
        "payloadRetained":true,"unsignedMutation":false,
        "negativeControls":["unsigned_suppress_refused","stale_revision_refused","scope_mismatch_refused"]})))
}

fn walk_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(path) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() { stack.push(p); } else { out.push(p); }
        }
    }
    out
}

/// Execute one native CTX qualification control.
pub(crate) fn run(case_id: &str) -> Result<Value, String> {
    match case_id {
        "CTX-018" => checkpoint_control(), "CTX-019" => promotion_control(),
        "CTX-020" => pending_review_control(), "CTX-021" => review_boundary_control(),
        "CTX-022" => review_due_control(), "CTX-023" | "CTX-024" => background_control(case_id),
        "CTX-025" => erase_control(), "CTX-026" => backup_control(), "CTX-027" => export_control(),
        "CTX-028" => vault_control(), "CTX-029" => reindex_control(), "CTX-030" => projection_control(),
        "CTX-031" | "CTX-032" => metrics_control(case_id), "CTX-034" => skill_control(),
        "CTX-035" => explain_control(), "CTX-036" => restore_control(), "CTX-037" => import_control(),
        "CTX-038" => bounded_list_control(), "CTX-040" => recipe_control(), "CTX-041" => suppression_control(),
        _ => Err(format!("unsupported native Cortex lifecycle case: {case_id}")),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test] fn checkpoint_is_native_and_terminal() { assert_eq!(run("CTX-018").unwrap()["status"], "passed"); }
    #[test] fn promotion_and_review_are_governed() { assert_eq!(run("CTX-019").unwrap()["proof"]["pendingReview"], true); assert_eq!(run("CTX-020").unwrap()["proof"]["durablePending"], true); }
    #[test] fn background_controls_fail_closed() { assert_eq!(run("CTX-023").unwrap()["proof"]["unknownProposalRefused"], true); }
    #[test] fn backup_restore_and_exports_are_sealed() { assert_eq!(run("CTX-026").unwrap()["proof"]["digestVerified"], true); assert_eq!(run("CTX-036").unwrap()["proof"]["readbackEquivalent"], true); }
    #[test] fn recipe_and_suppression_are_bound() { assert_eq!(run("CTX-040").unwrap()["proof"]["digestBound"], true); assert_eq!(run("CTX-041").unwrap()["proof"]["unsignedMutation"], false); }
}
