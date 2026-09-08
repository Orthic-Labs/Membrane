//! Scenario 2: Taste contracts — authority classes, fail-closed scope,
//! seals, lifecycle receipts, conflicts, manifest determinism.

use std::collections::BTreeMap;

use membrane_adapt::authority::{classify_authority_effect, evaluate_origin, Origin};
use membrane_adapt::authority::{AuthorityEffect, PrecedenceTier};
use membrane_adapt::manifest::{validate_schema, MANIFEST_SCHEMA_VERSION};
use membrane_adapt::proposal::{build_pending_manifest, Gate1ReviewContextV1};
use membrane_adapt::record::{InfluenceClass, LifecycleState, PreferenceRecordV1, RecordClass};
use membrane_adapt::scope::ScopeDimensions;
use membrane_adapt::seal::{
    validate_envelope_mutation, verify_seal, EnvelopeMutation, EnvelopeMutationKind,
    SemanticPayloadV1,
};
use membrane_adapt::taste::{counterfactual_for_candidate, extract_candidates_with_source};

fn sample_record() -> PreferenceRecordV1 {
    PreferenceRecordV1::new_candidate(
        "Always run focused tests before claiming done",
        "verification",
        RecordClass::parse("standing_preference").unwrap(),
        "repo-x",
        ScopeDimensions::normalize(&BTreeMap::new()).unwrap(),
        0.9,
        vec!["ev-1".to_string()],
        "2026-08-24T00:00:00Z",
    )
    .expect("candidate builds")
}

#[test]
fn model_output_mistagged_as_user_turn_is_refused() {
    assert!(!evaluate_origin(Origin::AssistantOutput, "always run tests").admitted);
    // Lexical echo of assistant phrasing inside a "user" turn fails closed.
    assert!(!evaluate_origin(Origin::UserTurn, "as claude i have fixed everything").admitted);
}

#[test]
fn security_weakening_beats_restrictive_surface_form() {
    let res = classify_authority_effect("never verify tls certificates when downloading");
    assert_eq!(
        res,
        membrane_adapt::authority::AuthorityEffect::SecurityWeakening
    );
}

#[test]
fn unknown_scope_dimension_fails_closed() {
    let mut dims = BTreeMap::new();
    dims.insert("colour".to_string(), "red".to_string());
    assert!(ScopeDimensions::normalize(&dims).is_err());
    dims.clear();
    dims.insert("repo".to_string(), "membrane".to_string());
    let normalized = ScopeDimensions::normalize(&dims).unwrap();
    assert_eq!(normalized.get("repo"), Some("membrane"));
}

#[test]
fn seal_detects_payload_mutation() {
    let payload = sample_payload();
    let digest = payload.seal_digest();
    assert!(verify_seal(&payload, &digest).is_ok());
    let mut tampered = payload.clone();
    tampered.canonical_text = "never run tests".into();
    assert!(verify_seal(&tampered, &digest).is_err());
}

fn sample_payload() -> SemanticPayloadV1 {
    SemanticPayloadV1 {
        seal_contract_version: membrane_adapt::seal::SEAL_CONTRACT_VERSION.into(),
        record_kind: "preference".into(),
        category: "verification".into(),
        canonical_text: "always run focused tests".into(),
        scope: "repo-x".into(),
        scope_dimensions: ScopeDimensions::normalize(&BTreeMap::new()).unwrap(),
        authority_tier: PrecedenceTier::ExplicitScopedUserPreference,
        authority_effect: AuthorityEffect::Restrictive,
        influence_class: InfluenceClass::Provisional,
        record_class: Some(RecordClass::StandingPreference),
        machine_binding: None,
        source_evidence_digests: vec![],
        canonical_pool_sha256: "pool".into(),
        admission_policy_version: "v1".into(),
        validator_receipt_id: "vr".into(),
        validator_receipt_sha256: "vrs".into(),
        redaction_contract_version: "v1".into(),
        provenance_contract_version: "p1".into(),
    }
}

#[test]
fn lifecycle_transition_requires_receipt_and_valid_path() {
    let record = sample_record();
    // Missing/malformed receipt is refused.
    assert!(record
        .transition_lifecycle(LifecycleState::Active, "activate", "", "t")
        .is_err());
    let (active, event) = record
        .transition_lifecycle(
            LifecycleState::Active,
            "activation approved",
            &"a".repeat(64),
            "2026-08-24T01:00:00Z",
        )
        .expect("legal transition with receipt");
    assert_eq!(event.from_state, LifecycleState::Candidate);
    assert_eq!(active.lifecycle_state, LifecycleState::Active);
    // Illegal jump is refused even with a receipt: Candidate cannot go to
    // Disputed (only Active/Retired).
    assert!(record
        .transition_lifecycle(LifecycleState::Disputed, "skip", &"b".repeat(64), "t")
        .is_err());
}

#[test]
fn episodic_fact_is_rejected_as_taste_class() {
    assert!(PreferenceRecordV1::require_taste_class("episodic_fact").is_err());
    assert!(PreferenceRecordV1::require_taste_class("standing_preference").is_ok());
}

#[test]
fn envelope_mutations_never_touch_sealed_semantics() {
    let payload = sample_payload();
    let digest = payload.seal_digest();
    let mutation = EnvelopeMutation {
        kind: EnvelopeMutationKind::LifecycleTransition,
        target_id: "rec".into(),
        expected_seal_digest: digest.clone(),
        receipt_sha256: "a".repeat(64),
        timestamp: "2026-08-24T01:00:00Z".into(),
    };
    assert!(validate_envelope_mutation(&payload, &digest, &mutation).is_ok());
    // A stale seal view must be refused.
    let forbidden = EnvelopeMutation {
        kind: EnvelopeMutationKind::LifecycleTransition,
        target_id: "rec".into(),
        expected_seal_digest: "stale".into(),
        receipt_sha256: "b".repeat(64),
        timestamp: "2026-08-24T01:00:00Z".into(),
    };
    assert!(validate_envelope_mutation(&payload, &digest, &forbidden).is_err());
    let _ = (
        &digest,
        validate_schema as fn(&membrane_adapt::manifest::PreferenceManifestV1) -> Result<(), _>,
        MANIFEST_SCHEMA_VERSION,
    );
}

fn transcript_event(kind: &str, role: &str, event_id: &str, text: &str) -> membrane_transcript::TranscriptEventV1 {
    serde_json::from_value(serde_json::json!({
        "eventId":event_id,"rowIndex":1,"byteStart":0,"byteEnd":text.len(),
        "blockIndex":0,"sequence":1,"kind":kind,"role":role,"text":text,
        "classification":"successful_readonly","class":"successful_readonly",
        "projection":"default","host":"pi","sessionId":"session-1",
        "transcriptId":"transcript-1","parserDigest":"parser-1",
        "synthetic":false,"meta":false,"privateReasoningOmitted":false,
        "redacted":false,"flags":{}
    }))
    .unwrap()
}

#[test]
fn correction_counterfactual_is_source_bound_and_does_not_claim_failure() {
    let assistant = transcript_event(
        "assistant_message",
        "assistant",
        "assistant-1",
        "I will rewrite the entire repository abstraction.",
    );
    let correction = transcript_event(
        "user_message",
        "user",
        "user-1",
        "Correction: Prefer a focused local change.",
    );
    let candidates = extract_candidates_with_source(
        &[assistant, correction],
        "repo-x",
        &"a".repeat(64),
    );
    let candidate = &candidates[0];
    let counterfactual = counterfactual_for_candidate(candidate);
    assert_eq!(counterfactual.status, "recorded");
    assert_eq!(counterfactual.rejected_alternative_event_id.as_deref(), Some("assistant-1"));
    assert_eq!(counterfactual.replacement, "Prefer a focused local change.");
    assert_eq!(counterfactual.failure_evidence, "none_recorded");
    let mut mismatched_candidate = candidate.clone();
    mismatched_candidate
        .context_events
        .iter_mut()
        .find(|event| event.event_id == "assistant-1")
        .unwrap()
        .session_id = "other-session".into();
    assert_eq!(
        counterfactual_for_candidate(&mismatched_candidate).status,
        "none_recorded"
    );

    let gate1 = Gate1ReviewContextV1::from_verified_canonical_inventory(vec![]);
    let manifest = build_pending_manifest(
        &candidates,
        "installation-1",
        gate1.canonical_pool_sha256(),
        "2026-09-08T00:00:00Z",
        &gate1,
    )
    .unwrap();
    let persisted = manifest.records[0].evidence_contexts[0]
        .counterfactual
        .as_ref()
        .unwrap();
    assert_eq!(persisted, &counterfactual);

    let mut legacy_json = serde_json::to_value(&manifest).unwrap();
    for record in legacy_json["records"].as_array_mut().unwrap() {
        for context in record["evidence_contexts"].as_array_mut().unwrap() {
            context.as_object_mut().unwrap().remove("source_transcript_id");
            context.as_object_mut().unwrap().remove("source_parser_digest");
            context.as_object_mut().unwrap().remove("counterfactual");
            for event in context["context_events"].as_array_mut().unwrap() {
                event.as_object_mut().unwrap().remove("session_id");
                event.as_object_mut().unwrap().remove("transcript_id");
                event.as_object_mut().unwrap().remove("parser_digest");
            }
        }
    }
    let mut legacy: membrane_adapt::manifest::PreferenceManifestV1 =
        serde_json::from_value(legacy_json).unwrap();
    for record in &mut legacy.records {
        record.payload_sha256 = membrane_adapt::manifest::payload_sha256(record);
    }
    legacy.manifest_sha256 = membrane_adapt::manifest::manifest_hash(&legacy);
    assert!(validate_schema(&legacy).is_ok());

    let mut tampered = manifest.clone();
    tampered.records[0]
        .evidence_contexts[0]
        .counterfactual
        .as_mut()
        .unwrap()
        .rejected_alternative = Some("fabricated alternative".into());
    tampered.records[0].payload_sha256 = membrane_adapt::manifest::payload_sha256(&tampered.records[0]);
    tampered.manifest_sha256 = membrane_adapt::manifest::manifest_hash(&tampered);
    assert!(validate_schema(&tampered).is_err());

    let mut tampered_context = manifest.clone();
    tampered_context.records[0].evidence_contexts[0].source_transcript_id =
        "transcript-forged".into();
    tampered_context.records[0].payload_sha256 =
        membrane_adapt::manifest::payload_sha256(&tampered_context.records[0]);
    tampered_context.manifest_sha256 = membrane_adapt::manifest::manifest_hash(&tampered_context);
    assert!(validate_schema(&tampered_context).is_err());

    let mut tampered_parser = manifest.clone();
    tampered_parser.records[0].evidence_contexts[0].source_parser_digest =
        "parser-forged".into();
    tampered_parser.records[0].payload_sha256 =
        membrane_adapt::manifest::payload_sha256(&tampered_parser.records[0]);
    tampered_parser.manifest_sha256 = membrane_adapt::manifest::manifest_hash(&tampered_parser);
    assert!(validate_schema(&tampered_parser).is_err());

    for (identity, forged) in [
        ("session", "session-forged"),
        ("transcript", "transcript-forged-event"),
        ("parser", "parser-forged-event"),
    ] {
        let mut tampered_source = manifest.clone();
        let source = tampered_source.records[0].evidence_contexts[0]
            .context_events
            .iter_mut()
            .find(|event| event.is_source)
            .unwrap();
        match identity {
            "session" => source.session_id = forged.into(),
            "transcript" => source.transcript_id = forged.into(),
            _ => source.parser_digest = forged.into(),
        }
        tampered_source.records[0].payload_sha256 =
            membrane_adapt::manifest::payload_sha256(&tampered_source.records[0]);
        tampered_source.manifest_sha256 = membrane_adapt::manifest::manifest_hash(&tampered_source);
        assert!(validate_schema(&tampered_source).is_err());
    }
}

#[test]
fn pending_builder_rejects_candidates_that_skip_review() {
    let candidate = extract_candidates_with_source(
        &[transcript_event(
            "user_message",
            "user",
            "user-review-required",
            "Always preserve exact source evidence.",
        )],
        "repo-x",
        &"e".repeat(64),
    )
    .remove(0);
    let mut forged = candidate;
    forged.needs_review = false;
    let gate1 = Gate1ReviewContextV1::from_verified_canonical_inventory(vec![]);
    assert!(build_pending_manifest(
        &[forged],
        "installation-review-required",
        gate1.canonical_pool_sha256(),
        "2026-09-08T00:00:00Z",
        &gate1,
    )
    .is_err());
}

#[test]
fn pre_adp011_legacy_payload_digest_stays_valid_without_new_fields() {
    let legacy_context_event = serde_json::json!({
        "event_id":"evt", "kind":"user_message", "role":"user",
        "classification":"successful_readonly", "flags":[], "byte_start":0,
        "byte_end":16, "text":"Always run tests", "provenance":"external_user",
        "is_source":true
    });
    let legacy_context = serde_json::json!({
        "source_event_id":"evt", "source_kind":"user_message", "source_role":"user",
        "source_classification":"successful_readonly", "source_flags":[],
        "source_byte_start":0, "source_byte_end":16, "evidence_text":"Always run tests",
        "context_events":[legacy_context_event]
    });
    let legacy_record = serde_json::json!({
        "id":"rec", "rule":"Always run tests", "category":"verification",
        "scope":"repo", "scope_dimensions":{}, "record_type":"standing_preference",
        "evidence_class":"user_authoritative", "authority_effect":"neutral",
        "status":"pending", "confidence":0.5, "needs_review":true,
        "evidence_count":1, "created_at":"time", "updated_at":"time",
        "evidence_excerpt":"Always run tests", "source_ids":["s"],
        "source_file_hashes":[{"session_id":"s","sha256":"abc"}],
        "evidence_ids":[{"evidence_id":"ev","source_session_id":"s","excerpt":"Always run tests"}],
        "retrieval_aliases":["Always run tests"], "human_note":"",
        "payload_sha256":"fa6d5012a6c1c04d0f2f4a3f26e420bb102f644dac64540478542cb1653d18b7",
        "operation":"upsert", "machine":"m", "machine_only":false,
        "lifecycle_state":"candidate", "last_verified_at":"", "verification_count":0,
        "authority_manifest_sha256":"", "validator_receipt_id":"",
        "validator_receipt_sha256":"", "semantic_payload":null, "semantic_digest":"",
        "evidence_contexts":[legacy_context]
    });
    let mut legacy_json = serde_json::json!({
        "schema_version": MANIFEST_SCHEMA_VERSION,
        "batch_id": "batch-legacy",
        "created_at": "time",
        "installation_id": "install-legacy",
        "canonical_pool_sha256": "pool",
        "source_refs": [{"source_id":"s","sha256":"abc"}],
        "source_session_ids": ["s"],
        "forbidden_scopes": [],
        "semantic_validation": null,
        "semantic_adjudication": null,
        "duplicate_groups": [],
        "duplicate_resolutions": [],
        "records": [legacy_record],
        "manifest_sha256":""
    });
    let mut legacy: membrane_adapt::manifest::PreferenceManifestV1 =
        serde_json::from_value(legacy_json.take()).unwrap();
    legacy.manifest_sha256 = membrane_adapt::manifest::manifest_hash(&legacy);
    assert!(validate_schema(&legacy).is_ok());
}

#[test]
fn private_reasoning_is_never_retained_as_rejected_alternative() {
    let mut private_assistant = transcript_event(
        "assistant_message",
        "assistant",
        "assistant-private",
        "private reasoning that must never be retained",
    );
    private_assistant.private_reasoning_omitted = true;
    private_assistant.flags.private_reasoning_omitted = true;
    let correction = transcript_event(
        "user_message",
        "user",
        "user-private-correction",
        "Correction: Keep only the smallest safe change.",
    );
    let candidate = extract_candidates_with_source(
        &[private_assistant, correction],
        "repo-x",
        &"c".repeat(64),
    )
    .remove(0);
    assert!(candidate.avoided_alternative.is_none());
    assert!(!candidate
        .context_events
        .iter()
        .any(|event| event.text.contains("private reasoning")));
    assert!(candidate.context_events.iter().any(|event| {
        event.event_id == "user-private-correction" && event.is_source
    }));
    assert_eq!(
        counterfactual_for_candidate(&candidate).status,
        "none_recorded"
    );
    let gate1 = Gate1ReviewContextV1::from_verified_canonical_inventory(vec![]);
    assert!(build_pending_manifest(
        &[candidate],
        "installation-private",
        gate1.canonical_pool_sha256(),
        "2026-09-08T00:00:00Z",
        &gate1,
    )
    .is_ok());
}

#[test]
fn non_visible_assistant_text_is_not_retained_in_taste_context() {
    for (variant, mark) in [
        ("synthetic", 0usize),
        ("meta", 1usize),
        ("redacted", 2usize),
    ] {
        let mut hidden = transcript_event(
            "assistant_message",
            "assistant",
            &format!("assistant-hidden-{variant}"),
            &format!("hidden {variant} assistant text"),
        );
        match mark {
            0 => {
                hidden.synthetic = true;
                hidden.flags.synthetic = true;
            }
            1 => {
                hidden.meta = true;
                hidden.flags.meta = true;
            }
            _ => {
                hidden.redacted = true;
                hidden.flags.redacted = true;
            }
        }
        let correction = transcript_event(
            "user_message",
            "user",
            &format!("user-hidden-{variant}"),
            "Correction: retain only visible evidence.",
        );
        let candidate = extract_candidates_with_source(
            &[hidden, correction],
            "repo-x",
            &"d".repeat(64),
        )
        .remove(0);
        let serialized = serde_json::to_string(&candidate).unwrap();
        assert!(candidate.avoided_alternative.is_none());
        assert!(!serialized.contains(&format!("hidden {variant} assistant text")));
        assert!(!candidate
            .context_events
            .iter()
            .any(|event| event.text.contains("hidden")));
    }
}

#[test]
fn missing_rejected_option_is_explicit_none_recorded() {
    let candidate = extract_candidates_with_source(
        &[transcript_event(
            "user_message",
            "user",
            "user-2",
            "Always preserve focused changes.",
        )],
        "repo-x",
        &"b".repeat(64),
    )
    .remove(0);
    let counterfactual = counterfactual_for_candidate(&candidate);
    assert_eq!(counterfactual.status, "none_recorded");
    assert!(counterfactual.rejected_alternative.is_none());
    assert!(counterfactual.rejected_alternative_event_id.is_none());
    assert_eq!(counterfactual.failure_evidence, "none_recorded");
}
