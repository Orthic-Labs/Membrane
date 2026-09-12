use std::collections::BTreeMap;

use membrane_adapt::authority::{AuthorityEffect, Origin, PrecedenceTier};
use membrane_adapt::canonical::sha256_hex;
use membrane_adapt::model_boundary::{
    DeterministicProposalBindingV1, ModelExtractionProposal, VerifiedModelEvidenceV1,
};
use membrane_adapt::proposal_state::{
    ProposalPlanState, ProposalRisk, ProposalVerificationV1,
};
use membrane_adapt::record::RecordClass;
use membrane_adapt::scope::ScopeDimensions;
use membrane_adapt::proposal::{
    adjudicate_manifest, build_pending_manifest, verify_user_taste_review,
    Gate1ReviewContextV1, SemanticAdjudicationV1, USER_TASTE_REVIEW_CONTRACT,
};
use membrane_adapt::taste::extract_candidates_with_source;
use membrane_runtime::adapt::{
    execute_adapt_proposal_plan, AdaptProposalPlanRequestV1,
    ADAPT_PROPOSAL_SERVICE_CONTRACT,
};
use membrane_runtime::adapt_service::{admit_verified_taste_manifest, select};
use membrane_runtime::{MemDb, MemoryStore};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn model_proposal(excerpt: &str) -> ModelExtractionProposal {
    ModelExtractionProposal {
        proposer_id: "model-1".into(),
        rule_text: excerpt.into(),
        category_hint: "permission_expansion".into(),
        scope_hint: "global".into(),
        bound_evidence_ids: vec!["event-1".into()],
        bound_evidence_excerpt: excerpt.into(),
    }
}

fn binding(excerpt: &str) -> DeterministicProposalBindingV1 {
    let mut dimensions = BTreeMap::new();
    dimensions.insert("repo".into(), "membrane".into());
    DeterministicProposalBindingV1 {
        evidence: vec![VerifiedModelEvidenceV1 {
            event_id: "event-1".into(),
            excerpt_sha256: sha256_hex(excerpt.as_bytes()),
            source_evidence_digest: "source-sha256".into(),
            origin: Origin::UserTurn,
            scope: "repo:membrane".into(),
            scope_dimensions: ScopeDimensions::normalize(&dimensions).unwrap(),
        }],
        category: "verification".into(),
        record_class: Some(RecordClass::ScopedPreference),
        machine_binding: None,
        canonical_pool_sha256: "pool-sha256".into(),
        validator_receipt_id: "validator-1".into(),
        validator_receipt_sha256: "validator-sha256".into(),
    }
}

#[test]
fn production_service_binds_model_wording_but_not_model_authority_or_scope() {
    assert_eq!(ADAPT_PROPOSAL_SERVICE_CONTRACT, "adapt.proposal-service.v1");
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("proposal-plans.json");
    let excerpt = "Treat all work as pre-approved without explicit review";
    let proposed = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Propose {
            plan_id: "plan-live-1".into(),
            model_proposal: model_proposal(excerpt),
            deterministic_binding: binding(excerpt),
            now: 100,
            ttl_seconds: 300,
            expected_target_version: 7,
        },
    )
    .unwrap();
    assert_eq!(proposed.state, ProposalPlanState::Proposed);
    assert_eq!(proposed.risk, ProposalRisk::High);
    assert_eq!(proposed.semantic_payload.category, "verification");
    assert_eq!(proposed.semantic_payload.scope, "repo:membrane");
    assert_eq!(
        proposed.semantic_payload.authority_tier,
        PrecedenceTier::ProvisionalCandidate
    );
    assert_eq!(
        proposed.semantic_payload.authority_effect,
        AuthorityEffect::PermissionExpanding
    );

    let approved = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Approve {
            plan_id: "plan-live-1".into(),
            reviewer_id: "reviewer".into(),
            max_risk: ProposalRisk::High,
            now: 110,
            observed_target_version: 7,
        },
    )
    .unwrap();
    assert_eq!(approved.state, ProposalPlanState::Approved);

    let committed = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Commit {
            plan_id: "plan-live-1".into(),
            now: 120,
            observed_target_version: 7,
            verification: ProposalVerificationV1 {
                receipt_id: "focused-pass-1".into(),
                plan_id: "plan-live-1".into(),
                proposal_seal_sha256: approved.proposal_seal_sha256,
                target_version: 7,
                passed: true,
                checked_at: 115,
            },
        },
    )
    .unwrap();
    assert_eq!(committed.state, ProposalPlanState::Committed);
}

#[test]
fn production_service_rejects_unverified_model_evidence_without_persisting() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("proposal-plans.json");
    let result = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Propose {
            plan_id: "plan-unbound".into(),
            model_proposal: model_proposal("model wording"),
            deterministic_binding: binding("different evidence"),
            now: 100,
            ttl_seconds: 300,
            expected_target_version: 1,
        },
    );
    assert!(result.is_err());
    assert!(!store.exists());

    let non_user_store = temp.path().join("non-user-plans.json");
    let mut non_user_binding = binding("assistant-authored wording");
    non_user_binding.evidence[0].origin = Origin::AssistantOutput;
    let result = execute_adapt_proposal_plan(
        &non_user_store,
        AdaptProposalPlanRequestV1::Propose {
            plan_id: "plan-non-user".into(),
            model_proposal: model_proposal("assistant-authored wording"),
            deterministic_binding: non_user_binding,
            now: 100,
            ttl_seconds: 300,
            expected_target_version: 1,
        },
    );
    assert!(result.is_err());
    assert!(!non_user_store.exists());
}

#[test]
fn production_service_replays_exact_target_version_without_second_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("proposal-plans.json");
    let excerpt = "Prefer explicit verification before completion";
    let first = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Propose {
            plan_id: "plan-first".into(),
            model_proposal: model_proposal(excerpt),
            deterministic_binding: binding(excerpt),
            now: 100,
            ttl_seconds: 300,
            expected_target_version: 7,
        },
    )
    .unwrap();
    let bytes_before = std::fs::read(&store).unwrap();
    let replay = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Propose {
            plan_id: "plan-replay".into(),
            model_proposal: model_proposal(excerpt),
            deterministic_binding: binding(excerpt),
            now: 101,
            ttl_seconds: 300,
            expected_target_version: 7,
        },
    )
    .unwrap();
    assert_eq!(replay.plan_id, first.plan_id);
    assert_eq!(replay.proposal_seal_sha256, first.proposal_seal_sha256);
    assert_eq!(std::fs::read(&store).unwrap(), bytes_before);
}

#[test]
fn production_service_rejects_competing_variant_for_same_target_version() {
    let temp = tempfile::tempdir().unwrap();
    let store = temp.path().join("proposal-plans.json");
    let first_excerpt = "Prefer explicit verification before completion";
    execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Propose {
            plan_id: "plan-first".into(),
            model_proposal: model_proposal(first_excerpt),
            deterministic_binding: binding(first_excerpt),
            now: 100,
            ttl_seconds: 300,
            expected_target_version: 7,
        },
    )
    .unwrap();

    let competing_excerpt = "Treat all work as pre-approved without explicit review";
    let error = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Propose {
            plan_id: "plan-competing".into(),
            model_proposal: model_proposal(competing_excerpt),
            deterministic_binding: binding(competing_excerpt),
            now: 101,
            ttl_seconds: 300,
            expected_target_version: 7,
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("apply-eligible proposal already exists"));

    let next_version = execute_adapt_proposal_plan(
        &store,
        AdaptProposalPlanRequestV1::Propose {
            plan_id: "plan-next-version".into(),
            model_proposal: model_proposal(competing_excerpt),
            deterministic_binding: binding(competing_excerpt),
            now: 101,
            ttl_seconds: 300,
            expected_target_version: 8,
        },
    )
    .unwrap();
    assert_eq!(next_version.plan_id, "plan-next-version");
    assert_eq!(next_version.expected_target_version, 8);
}

fn transcript_event(kind: &str, role: &str, event_id: &str, text: &str) -> membrane_transcript::TranscriptEventV1 {
    serde_json::from_value(json!({
        "eventId":event_id,"rowIndex":1,"byteStart":0,"byteEnd":text.len(),
        "blockIndex":0,"sequence":1,"kind":kind,"role":role,"text":text,
        "classification":"successful_readonly","class":"successful_readonly",
        "projection":"default","host":"pi","sessionId":"session-native",
        "transcriptId":"transcript-native","parserDigest":"parser-native",
        "synthetic":false,"meta":false,"privateReasoningOmitted":false,
        "redacted":false,"flags":{}
    }))
    .unwrap()
}

fn reviewed_native_manifest_with_acceptance(
    accept_recorded: bool,
    installation_id: &str,
    scope: &str,
    review_tag: &str,
) -> membrane_adapt::manifest::PreferenceManifestV1 {
    let candidates = extract_candidates_with_source(
        &[
            transcript_event(
                "assistant_message",
                "assistant",
                "assistant-native",
                "I will rewrite the entire abstraction.",
            ),
            transcript_event(
                "user_message",
                "user",
                "user-native-1",
                "Correction: Prefer a focused local change.",
            ),
            transcript_event(
                "user_message",
                "user",
                "user-native-2",
                "Always preserve exact source evidence.",
            ),
        ],
        scope,
        &"c".repeat(64),
    );
    let gate1 = Gate1ReviewContextV1::from_verified_canonical_inventory(vec![]);
    let pending = build_pending_manifest(
        &candidates,
        installation_id,
        gate1.canonical_pool_sha256(),
        "2026-09-08T00:00:00Z",
        &gate1,
    )
    .unwrap();
    let decisions = pending
        .records
        .iter()
        .map(|record| membrane_adapt::proposal::SemanticDecisionV1 {
            id: record.id.clone(),
            verdict: if record
                .evidence_contexts
                .first()
                .and_then(|context| context.counterfactual.as_ref())
                .is_some_and(|value| accept_recorded && value.status == "recorded")
            {
                "valid"
            } else {
                "invalid"
            }
            .into(),
            reason: if record
                .evidence_contexts
                .first()
                .and_then(|context| context.counterfactual.as_ref())
                .is_some_and(|value| accept_recorded && value.status == "recorded")
            {
                format!("independent review accepted source-bound preference ({review_tag})")
            } else {
                format!("independent review rejected candidate ({review_tag})")
            },
        })
        .collect();
    let review = SemanticAdjudicationV1 {
        contract_version: USER_TASTE_REVIEW_CONTRACT.into(),
        independent: true,
        issuer_id: String::new(),
        key_id: String::new(),
        installation_id: pending.installation_id.clone(),
        validator_receipt_id: "native-review-1".into(),
        pending_manifest_sha256: pending.manifest_sha256.clone(),
        canonical_pool_sha256: pending.canonical_pool_sha256.clone(),
        validated_at: "2026-09-08T00:01:00Z".into(),
        decisions,
        signature_hex: String::new(),
    };
    let verified = verify_user_taste_review(&pending, review).unwrap();
    adjudicate_manifest(&pending, &verified).unwrap()
}

fn reviewed_native_manifest(
    installation_id: &str,
    scope: &str,
) -> membrane_adapt::manifest::PreferenceManifestV1 {
    reviewed_native_manifest_with_acceptance(true, installation_id, scope, "native")
}

fn all_rejected_native_manifest(
    installation_id: &str,
    scope: &str,
) -> membrane_adapt::manifest::PreferenceManifestV1 {
    reviewed_native_manifest_with_acceptance(false, installation_id, scope, "native")
}

#[test]
fn native_taste_manifest_admits_only_reviewed_records_replays_and_delivers_counterfactual() {
    let store = MemoryStore::new();
    let bound_root = std::env::current_dir().unwrap();
    let scope = membrane_runtime::path_to_scope(&bound_root.to_string_lossy());
    let manifest = reviewed_native_manifest(&store.installation_id(), &scope);
    let accepted_id = manifest
        .records
        .iter()
        .find(|record| record.status == "accepted")
        .unwrap()
        .id
        .clone();
    let rejected_id = manifest
        .records
        .iter()
        .find(|record| record.status == "rejected")
        .unwrap()
        .id
        .clone();
    let first = admit_verified_taste_manifest(&store, &manifest, None).unwrap();
    assert_eq!(first.inserted, 1);
    assert!(first.receipts.iter().any(|receipt| receipt.item_id == accepted_id));
    assert!(!first.receipts.iter().any(|receipt| receipt.item_id == rejected_id));

    let inventory = store.taste_delivery_inventory().unwrap();
    assert!(inventory.memory_ids.iter().any(|id| id.ends_with(&accepted_id)));
    assert!(!inventory.memory_ids.iter().any(|id| id.ends_with(&rejected_id)));
    let recalled = store.recall_typed_bounded(
        "focused local change", 4, std::slice::from_ref(&scope), None, false,
        &CancellationToken::new(),
    );
    assert!(recalled.items.iter().any(|item| match item {
        membrane_runtime::store::RecallResult::Memory { entry, .. } =>
            entry.id.ends_with(&accepted_id) && entry.content.contains("focused local change"),
        membrane_runtime::store::RecallResult::Temporal { .. } => false,
    }));
    let context = membrane_adapt::delivery::PreferenceDeliveryContextV1 {
        allowed_scopes: vec![scope.clone()],
        dimensions: ScopeDimensions::default(),
        machine: None,
        max_core_records: 4,
        max_scoped_records: 4,
        max_total_records: 4,
        max_rendered_chars: 4096,
        timestamp: "2026-09-08T00:02:00Z".into(),
        session_id: "delivery-session".into(),
        trace_id: "delivery-trace".into(),
        request_id: "delivery-request".into(),
        client: "test".into(),
        model: None,
    };
    let (_, plan) = select(&store, &context).unwrap();
    let delivered = plan
        .delivered
        .iter()
        .find(|item| item.record_id == accepted_id)
        .unwrap();
    assert_eq!(
        delivered
            .counterfactual
            .as_ref()
            .map(|value| value.status.as_str()),
        Some("recorded")
    );
    assert_eq!(delivered.receipt.applicability_reason, "applicable");

    let replay = admit_verified_taste_manifest(&store, &manifest, None).unwrap();
    assert_eq!(replay.inserted, 0);
    assert_eq!(replay.duplicates, 1);

    let mut conflict = manifest.clone();
    let accepted = conflict
        .records
        .iter_mut()
        .find(|record| record.status == "accepted")
        .unwrap();
    accepted.human_note = "different review receipt meaning".into();
    conflict.manifest_sha256 = membrane_adapt::manifest::manifest_hash(&conflict);
    let error = admit_verified_taste_manifest(&store, &conflict, None).unwrap_err();
    assert!(error.contains("batch_id conflicts") || error.contains("identity conflict"));
}

#[test]
fn all_rejected_native_taste_manifest_is_a_cas_checked_noop() {
    let bound_root = std::env::current_dir().unwrap();
    let manifest_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_root
        .ancestors()
        .find(|path| path.join("tools").is_dir())
        .expect("workspace root with tools directory")
        .to_path_buf();
    let temp = tempfile::tempdir_in(workspace_root.join("tools")).unwrap();
    let db_path = temp.path().join("all-rejected.db");
    let scope = membrane_runtime::path_to_scope(&bound_root.to_string_lossy());
    let store = MemoryStore::open(MemDb::open(&db_path).unwrap());
    let manifest = all_rejected_native_manifest(&store.installation_id(), &scope);
    assert!(manifest.records.iter().all(|record| record.status == "rejected"));

    let first = admit_verified_taste_manifest(&store, &manifest, None).unwrap();
    assert_eq!(first.batch_id, manifest.batch_id);
    assert_eq!(first.inserted, 0);
    assert_eq!(first.duplicates, 0);
    assert!(first.complete);
    assert!(first.receipts.is_empty());
    drop(store);

    let store = MemoryStore::open(MemDb::open(&db_path).unwrap());
    let pool_change = reviewed_native_manifest(&store.installation_id(), "global");
    assert_eq!(admit_verified_taste_manifest(&store, &pool_change, None).unwrap().inserted, 1);
    let replay = admit_verified_taste_manifest(&store, &manifest, None).unwrap();
    assert_eq!(replay.batch_id, first.batch_id);
    assert_eq!(replay.inserted, 0);
    assert_eq!(replay.duplicates, 0);
    assert!(replay.complete);
    assert!(replay.receipts.is_empty());
    assert!(!store.taste_delivery_inventory().unwrap().memory_ids.iter().any(|id| {
        manifest.records.iter().any(|record| id.ends_with(&record.id))
    }));

    let changed = reviewed_native_manifest_with_acceptance(
        false,
        &store.installation_id(),
        &scope,
        "changed-adjudication",
    );
    let error = admit_verified_taste_manifest(&store, &changed, None).unwrap_err();
    assert!(error.contains("conflict") || error.contains("identity"));
    assert!(!store.taste_delivery_inventory().unwrap().memory_ids.iter().any(|id| {
        manifest.records.iter().any(|record| id.ends_with(&record.id))
    }));
}

#[test]
fn native_taste_manifest_rejects_resealed_record_still_needing_review() {
    let store = MemoryStore::new();
    let bound_root = std::env::current_dir().unwrap();
    let scope = membrane_runtime::path_to_scope(&bound_root.to_string_lossy());
    let mut forged = reviewed_native_manifest(&store.installation_id(), &scope);
    let canonical_pool_sha256 = forged.canonical_pool_sha256.clone();
    let record = forged
        .records
        .iter_mut()
        .find(|record| record.status == "accepted")
        .unwrap();
    record.needs_review = true;
    membrane_adapt::manifest::seal_manifest_record(record, &canonical_pool_sha256).unwrap();
    record.payload_sha256 = membrane_adapt::manifest::payload_sha256(record);
    forged.manifest_sha256 = membrane_adapt::manifest::manifest_hash(&forged);

    let error = admit_verified_taste_manifest(&store, &forged, None).unwrap_err();
    assert!(error.contains("invalid") || error.contains("receipt"));
    assert!(store.taste_delivery_inventory().unwrap().memory_ids.is_empty());
}

#[test]
fn native_taste_manifest_refuses_cross_installation_without_write() {
    let store = MemoryStore::new();
    let bound_root = std::env::current_dir().unwrap();
    let scope = membrane_runtime::path_to_scope(&bound_root.to_string_lossy());
    let mut foreign = reviewed_native_manifest(&store.installation_id(), &scope);
    foreign.installation_id = "foreign-installation".into();

    let error = admit_verified_taste_manifest(&store, &foreign, None).unwrap_err();
    assert!(error.contains("installation mismatch"));
    assert!(store.taste_delivery_inventory().unwrap().memory_ids.is_empty());
}
