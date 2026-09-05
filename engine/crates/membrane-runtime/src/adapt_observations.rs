//! Bounded host observation adapter and deterministic verification detector.
//! Reuses host H4/H6/H9/H10 types and Cortex's existing append-only event store.
//! Input receipts attest host submissions; they are not user-preference authority.
use crate::{adapt_efficiency, adapt_service, MemoryStore};
use membrane_protocol::host_observation::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const DETECTOR: &str = "required_verification_completion.v1";
const DETECTOR_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequencedObservationV1 {
    pub sequence: u64,
    pub observation: ExecutionObservationV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdaptObservationRequestV1 {
    Analyze {
        scope: String,
        window_id: String,
        session_id: String,
        task_id: String,
        expected_cursor: u64,
        /// Required call identities supplied by the task/verification owner,
        /// never inferred from prose or from a VerificationStarted event.
        required_call_ids: Vec<String>,
        observations: Vec<SequencedObservationV1>,
    },
    Acknowledge {
        scope: String,
        emission_receipt_id: String,
        acknowledgement: PacketDeliveryAcknowledgementV1,
        loaded: LoadedContextIdentitiesV1,
    },
    Outcome {
        scope: String,
        coverage_receipt_id: String,
        evaluation: EvaluationOutcomeV1,
        dataset_sha256: String,
        case_sha256: String,
    },
}

fn exact<T>(field: &ObservedFieldV1<T>) -> Option<&T> {
    (field.coverage == ObservationCoverageV1::Complete)
        .then_some(field.value.as_ref())
        .flatten()
}
fn hash(v: &impl Serialize) -> String {
    membrane_adapt::canonical::sha256_canonical(
        &serde_json::to_value(v).expect("typed observations serialize"),
    )
}
fn valid_id(v: &str) -> bool {
    !v.trim().is_empty() && v.len() <= 512 && !v.chars().any(char::is_control)
}
fn valid_hash(v: &str) -> bool {
    let v = v.strip_prefix("sha256:").unwrap_or(v);
    v.len() == 64
        && v.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn ledger(store: &MemoryStore) -> Result<cortex_store::AbsorbedStore, String> {
    cortex_store::AbsorbedStore::new(store.db().clone()).map_err(|e| e.to_string())
}
fn receipt(
    store: &MemoryStore,
    id: &str,
    scope: &str,
    kind: &str,
) -> Result<cortex_store::SessionEvent, String> {
    if !id.starts_with("adapt:") || !valid_id(id) {
        return Err("invalid Adapt receipt reference".into());
    }
    let event = ledger(store)?
        .events_range(id, 1, 2)
        .map_err(|e| e.to_string())?
        .into_iter()
        .next()
        .ok_or("referenced Adapt receipt unavailable")?;
    if event.scope_id != scope
        || event.event_type != kind
        || event.content_hash != hash(&event.payload)
    {
        return Err("receipt scope/type/integrity mismatch".into());
    }
    Ok(event)
}

pub fn execute(store: &MemoryStore, request: AdaptObservationRequestV1) -> Result<Value, String> {
    match request {
        AdaptObservationRequestV1::Analyze {
            scope,
            window_id,
            session_id,
            task_id,
            expected_cursor,
            required_call_ids,
            mut observations,
        } => {
            if ![&scope, &window_id, &session_id, &task_id]
                .iter()
                .all(|v| valid_id(v))
                || observations.is_empty()
                || observations.len() > 256
                || required_call_ids.len() > 128
                || required_call_ids.iter().any(|v| !valid_id(v))
            {
                return Err("invalid observation window bounds".into());
            }
            let required: BTreeSet<_> = required_call_ids.iter().cloned().collect();
            if required.len() != required_call_ids.len() {
                return Err("duplicate required call identity".into());
            }
            observations.sort_by_key(|o| o.sequence);
            let mut seen = BTreeSet::new();
            for (i, row) in observations.iter().enumerate() {
                let expected = expected_cursor
                    .checked_add(i as u64 + 1)
                    .ok_or("sequence overflow")?;
                let o = &row.observation;
                if row.sequence != expected || !seen.insert(&o.observation_id) {
                    return Err("observation gap/duplicate sequence".into());
                }
                o.validate().map_err(|e| e.to_string())?;
                if o.session_id != session_id
                    || exact(&o.task_id) != Some(&task_id)
                    || exact(&o.scope) != Some(&scope)
                    || o.observed_at_unix_ms == 0
                {
                    return Err("observation scope/session/task binding mismatch".into());
                }
            }
            let stream = format!(
                "adapt:consumer:{}",
                hash(&json!([&scope, &session_id, &task_id, DETECTOR]))
            );
            let db = ledger(store)?;
            let cursor = db.cursor(&stream).map_err(|e| e.to_string())?;
            let previous = if cursor.last_seq > 0 {
                db.events_range(&stream, cursor.last_seq, cursor.last_seq + 1)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .next()
            } else {
                None
            };
            let input_digest = hash(&json!([
                &scope,
                &window_id,
                &session_id,
                &task_id,
                expected_cursor,
                &required,
                &observations,
                DETECTOR,
                DETECTOR_VERSION,
                adapt_efficiency::DETECTOR_FAMILY_ID,
                adapt_efficiency::DETECTOR_FAMILY_VERSION,
                HOST_OBSERVATION_SCHEMA_VERSION
            ]));
            if let Some(ref prior) = previous {
                if prior.payload["window_id"] == window_id {
                    if prior.payload["input_digest"] != input_digest {
                        return Err("window identity reused with changed input or detector version".into());
                    }
                    let reference = adapt_service::journal(
                        store,
                        &scope,
                        "adapt.detector_coverage",
                        &prior.event_id,
                        prior.payload.clone(),
                    )?;
                    return Ok(json!({"coverage":prior.payload,"receipt":reference}));
                }
                if prior.payload["last_input_sequence"].as_u64() != Some(expected_cursor)
                    || prior.payload["required_call_ids"] != json!(required)
                {
                    return Err("consumer cursor/required calls changed".into());
                }
            } else if expected_cursor != 0 {
                return Err("consumer has no preceding input window".into());
            }
            let mut all_ids: BTreeSet<String> = previous
                .as_ref()
                .and_then(|p| serde_json::from_value(p.payload["processed_event_ids"].clone()).ok())
                .unwrap_or_default();
            for row in &observations {
                if !all_ids.insert(row.observation.observation_id.clone()) {
                    return Err("observation identity replayed at a new sequence".into());
                }
            }
            if all_ids.len() > 4096 {
                return Err("bounded task observation horizon exceeded".into());
            }
            let mut results: BTreeMap<String, Option<i32>> = previous
                .as_ref()
                .and_then(|p| {
                    serde_json::from_value(p.payload["verification_results"].clone()).ok()
                })
                .unwrap_or_default();
            let mut evidence: BTreeMap<String, String> = previous
                .as_ref()
                .and_then(|p| {
                    serde_json::from_value(p.payload["verification_evidence"].clone()).ok()
                })
                .unwrap_or_default();
            let mut episodes = Vec::new();
            let mut missing = BTreeSet::new();
            for row in &observations {
                let o = &row.observation;
                match o.observation_kind {
                    ExecutionObservationKindV1::VerificationResult => {
                        if let Some(call) = exact(&o.call_id).filter(|id| required.contains(*id)) {
                            results.insert(call.clone(), exact(&o.exit_code).copied());
                            evidence.insert(call.clone(), o.observation_id.clone());
                        }
                    }
                    ExecutionObservationKindV1::CompletionClaimEmitted => {
                        for call in &required {
                            match results.get(call).copied().flatten() {
                                Some(exit) if exit != 0 => episodes.push(json!({
                                    "episode_id":format!("adapt:episode:{}",hash(&json!([DETECTOR,&scope,&session_id,&task_id,call,&evidence[call],&o.observation_id]))),
                                    "detector":DETECTOR,"detector_version":DETECTOR_VERSION,"call_id":call,"failed_result_id":evidence[call],"completion_id":o.observation_id,
                                    "execution_observation_ids":[evidence[call].clone(),o.observation_id.clone()],
                                    "honesty_limit":"Required verification failed before a completion claim; no user preference, root cause or prevented failure is inferred."
                                })),
                                None => {
                                    missing.insert(format!("verification_result:{call}"));
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            if required.is_empty() {
                missing.insert("required_verification_contract".into());
            }

            let flat_observations: Vec<_> = observations
                .iter()
                .map(|row| row.observation.clone())
                .collect();
            let detector_coverages =
                adapt_efficiency::analyze_efficiency(&flat_observations, &input_digest);
            let detector_catalog = adapt_efficiency::detector_catalog();
            let payload = json!({
                "contract":"adapt.detector-coverage.v2",
                "detector":DETECTOR,
                "detector_version":DETECTOR_VERSION,
                "detector_family":{
                    "id":adapt_efficiency::DETECTOR_FAMILY_ID,
                    "version":adapt_efficiency::DETECTOR_FAMILY_VERSION,
                    "input_schema_version":HOST_OBSERVATION_SCHEMA_VERSION,
                    "catalog":detector_catalog,
                },
                "scope":scope,"window_id":window_id,"session_id":session_id,"task_id":task_id,
                "input_digest":input_digest,"first_input_sequence":observations[0].sequence,
                "last_input_sequence":observations.last().expect("nonempty").sequence,
                "required_call_ids":required,"processed_event_ids":all_ids,"verification_results":results,"verification_evidence":evidence,
                "state":if missing.is_empty(){"ran"}else{"unavailable"},"missing_fields":missing,
                "episodes":episodes,
                "detector_coverages":detector_coverages,
                "outcome_join":null
            });
            // State and coverage commit together; competing windows race on the
            // same stream sequence and one fails rather than losing a cursor.
            let state = cortex_store::SessionEvent {
                schema_version: 1,
                session_id: stream,
                seq: cursor.last_seq + 1,
                event_id: format!(
                    "adapt:window:{}",
                    hash(&json!([
                        &scope,
                        &session_id,
                        &task_id,
                        &window_id,
                        DETECTOR,
                        DETECTOR_VERSION,
                        adapt_efficiency::DETECTOR_FAMILY_VERSION
                    ]))
                ),
                event_type: "adapt.detector_state".into(),
                payload: payload.clone(),
                scope_id: scope.clone(),
                authority: "A0".into(),
                influence_class: "reference".into(),
                lifecycle: "active".into(),
                retention: "local_audit".into(),
                provenance: vec![cortex_store::ProvenanceRef {
                    source: DETECTOR.into(),
                    source_event_ids: observations
                        .iter()
                        .map(|o| o.observation.observation_id.clone())
                        .collect(),
                    producer: Some("adapt_native".into()),
                }],
                content_hash: hash(&payload),
                occurred_at_ms: observations.last().unwrap().observation.observed_at_unix_ms,
                recorded_at_ms: 0,
            };
            db.append_event(&state).map_err(|e| e.to_string())?;
            // Retry-safe reference projection; state remains authoritative if a
            // process dies between the atomic state append and this projection.
            let reference = adapt_service::journal(
                store,
                &scope,
                "adapt.detector_coverage",
                &state.event_id,
                payload.clone(),
            )?;
            Ok(json!({"coverage":payload,"receipt":reference}))
        }
        AdaptObservationRequestV1::Acknowledge {
            scope,
            emission_receipt_id,
            acknowledgement: ack,
            loaded,
        } => {
            ack.validate().map_err(|e| e.to_string())?;
            loaded.validate().map_err(|e| e.to_string())?;
            let emitted = receipt(store, &emission_receipt_id, &scope, "adapt.packet_emitted")?;
            let emission = &emitted.payload;
            let task = exact(&ack.task_id).ok_or("acknowledgement task unavailable")?;
            if ack.status != PacketDeliveryAcknowledgementStatusV1::Acknowledged
                || emission["packet_digest"] != ack.packet_digest
                || emission["session_id"] != ack.session_id
                || loaded.session_id != ack.session_id
                || exact(&loaded.compaction_generation).is_none()
                || ack.acknowledged_at_unix_ms == 0
                || loaded.observed_at_unix_ms < ack.acknowledged_at_unix_ms
                || exact(&ack.serialized_bytes).is_none()
                || emission["task_digest"] != membrane_adapt::canonical::sha256_hex(task.as_bytes())
            {
                return Err("host loading acknowledgement binding mismatch".into());
            }
            let identities = exact(&loaded.identities).ok_or("loaded identities incomplete")?;
            let records = emission["records"]
                .as_array()
                .ok_or("emission records missing")?;
            let inventory = store.taste_delivery_inventory()?;
            for record in records {
                if !identities.iter().any(|i| {
                    record["candidate_id"] == i.identity
                        && record["source_ref"] == i.source_ref
                        && record["representation_sha256"].as_str()
                            == Some(
                                i.source_digest
                                    .strip_prefix("sha256:")
                                    .unwrap_or(&i.source_digest),
                            )
                }) {
                    return Err("host did not load exact emitted preference representation".into());
                }
                if !inventory.candidates.iter().any(|c| {
                    record["record_id"] == c.record_id
                        && c.semantic_verified
                        && c.lifecycle_eligible
                        && c.lifecycle_state == membrane_adapt::record::LifecycleState::Active
                        && record["record_sha256"].as_str()
                            == inventory
                                .record_versions
                                .get(&c.record_id)
                                .map(String::as_str)
                }) {
                    return Err("preference changed before host loading acknowledgement".into());
                }
            }
            let evidence = json!({"emission_receipt":emission_receipt_id,"acknowledgement":ack,"loaded_identities":loaded,
                "records":records,"exposure":"host_acknowledged","effectiveness":null});
            adapt_service::journal(
                store,
                &scope,
                "adapt.host_acknowledgement",
                &ack.acknowledgement_id,
                evidence,
            )
        }
        AdaptObservationRequestV1::Outcome {
            scope,
            coverage_receipt_id,
            evaluation,
            dataset_sha256,
            case_sha256,
        } => {
            evaluation.validate().map_err(|e| e.to_string())?;
            if !valid_hash(&dataset_sha256)
                || !valid_hash(&case_sha256)
                || exact(&evaluation.execution_receipt).is_none()
                || exact(&evaluation.case_id).is_none()
                || exact(&evaluation.dataset_id).is_none()
            {
                return Err("exact evaluator/case receipt required".into());
            }
            let coverage = receipt(
                store,
                &coverage_receipt_id,
                &scope,
                "adapt.detector_coverage",
            )?;
            if coverage.payload["state"] != "ran"
                || exact(&evaluation.session_id).map(String::as_str)
                    != coverage.payload["session_id"].as_str()
                || exact(&evaluation.task_id).map(String::as_str)
                    != coverage.payload["task_id"].as_str()
            {
                return Err("outcome/coverage join is incomplete or mismatched".into());
            }
            let out = json!({
                "contract":"adapt.coverage-outcome-join.v1",
                "coverage_receipt":coverage_receipt_id,
                "coverage_digest":coverage.content_hash,
                "detector_family":coverage.payload["detector_family"],
                "evaluation":evaluation,
                "dataset_sha256":dataset_sha256,
                "case_sha256":case_sha256,
                "effectiveness":null,
                "missing_fields":["h4_to_h6_exact_execution_episode_binding","exact_loaded_exposure_binding"],
                "honesty_limit":"Evaluator is joined to the exact detector window and task/session. H4/H6 do not yet expose a verifiable execution-episode receipt bridge, and intervention benefit additionally requires a separately joined exact host-loaded exposure."
            });
            adapt_service::journal(
                store,
                &scope,
                "adapt.outcome_join",
                &evaluation.outcome_id,
                out,
            )
        }
    }
}

pub fn response(store: &MemoryStore, body: &str) -> (u16, String) {
    let result = if body.len() > adapt_service::MAX_INPUT_BYTES {
        Err("observation request too large".into())
    } else {
        serde_json::from_str(body)
            .map_err(|e| format!("invalid observation request: {e}"))
            .and_then(|r| execute(store, r))
    };
    match result {
        Ok(v) => (200, v.to_string()),
        Err(e) => (
            400,
            json!({"error":"adapt_observation_refused","detail":e}).to_string(),
        ),
    }
}