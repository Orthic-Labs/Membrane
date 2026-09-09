//! Generation-bound, storage-free Phase-2 planning and sealing.

use crate::graph::GraphGeneration;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use xxhash_rust::xxh3::xxh3_128;

pub const PHASE2_INCREMENTAL_VERSION: u32 = 1;
pub const DIMENSIONS: [&str; 6] = ["architecture", "interfaces", "health", "contract", "security", "solid"];
const VERIFIABLE: [&str; 5] = ["implemented", "stale", "contradict", "decision", "canonical"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase2Queue { #[serde(default)] pub claims: Vec<Phase2Claim> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase2Claim { pub id: String, #[serde(default)] pub source: Option<String>, #[serde(default)] pub line: Option<u64>, #[serde(default)] pub status: Option<String>, #[serde(default)] pub text: Option<String>, #[serde(default)] pub candidate_files: Vec<String> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivationMetadata { pub method: String, #[serde(default)] pub evidence_refs: Vec<Value>, pub fingerprint: String, #[serde(default)] pub provider: String, #[serde(default)] pub model: String, #[serde(default)] pub version: String }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationMetadata { pub status: String, pub confidence: Value }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvalidationMetadata { pub generation_id: String, pub reason: String }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupersessionMetadata { pub state: String, #[serde(default)] pub superseded_by: Option<String> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase2Verdict { pub claim_id: String, #[serde(default)] pub verdict: Option<String>, #[serde(default)] pub evidence: Option<Value>, #[serde(default)] pub note: Option<String>, #[serde(default)] pub input_files: Vec<String>, #[serde(default)] pub input_fingerprint: Option<String>, #[serde(default)] pub reconciliation_fingerprint: Option<String>, #[serde(default)] pub source_generation_id: Option<String>, #[serde(default)] pub derivation: Option<DerivationMetadata>, #[serde(default)] pub verification: Option<VerificationMetadata>, #[serde(default)] pub invalidation: Option<InvalidationMetadata>, #[serde(default)] pub supersession: Option<SupersessionMetadata>, #[serde(flatten)] pub extra: BTreeMap<String, Value> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerdictEnvelope { #[serde(default)] pub verdicts: Vec<Phase2Verdict>, #[serde(default)] pub source_generation_id: Option<String>, #[serde(default)] pub incremental_schema_version: Option<u32> }
// Freshness is tracked as four distinct dimensions, never collapsed into one scalar:
//   source      -- per-file contentHash from the served graph generation (graph_file_hashes).
//   structural  -- file_inventory_fingerprint: the repository-wide path inventory shape (topology).
//   derived     -- input_fingerprint: this dimension's declared file set + upstream verdict result
//                  fingerprints, which changes on a behavior-only ordering/fallback/config edit even
//                  when structural topology (file_inventory_fingerprint) is unchanged.
//   verification-- carried on each Phase2Verdict via VerificationMetadata.status/confidence, which
//                  is independent of derivation/structural freshness and is never inferred from it.
// Metadata validity (well-formed derivation/verification/invalidation/supersession records) is not
// semantic verification of claim content, and an explanation/derivation is not source authority.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DimensionMetadata { #[serde(default)] pub input_files: Vec<String>, #[serde(default)] pub input_verdict_ids: Vec<String>, #[serde(default)] pub input_fingerprint: Option<String>, #[serde(default)] pub file_inventory_fingerprint: Option<String> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncrementalUnderstanding { #[serde(default)] pub schema_version: Option<u32>, #[serde(default)] pub dimensions: BTreeMap<String, DimensionMetadata> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Understanding { #[serde(default)] pub architecture: Option<Value>, #[serde(default)] pub interfaces: Option<Value>, #[serde(default)] pub health: Option<Value>, #[serde(default)] pub contract: Option<Value>, #[serde(default)] pub security: Option<Value>, #[serde(default)] pub solid: Option<Value>, #[serde(default)] pub source_generation_id: Option<String>, #[serde(default)] pub incremental: Option<IncrementalUnderstanding> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimWork { pub claim_id: String, pub input_files: Vec<String>, pub input_fingerprint: String, pub missing_inputs: Vec<String>, #[serde(skip_serializing_if = "Option::is_none")] pub reason: Option<String> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DimensionWork { pub dimension: String, pub reason: String, #[serde(default, skip_serializing_if = "Vec::is_empty")] pub missing_files: Vec<String>, #[serde(default, skip_serializing_if = "Vec::is_empty")] pub missing_verdicts: Vec<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase2Plan { pub schema_version: u32, pub source_generation_id: String, pub verdicts: PlanVerdicts, pub dimensions: PlanDimensions, pub complete: bool }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanVerdicts { pub reuse: Vec<Phase2Verdict>, pub verify: Vec<ClaimWork> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanDimensions { pub reuse: Vec<String>, pub synthesize: Vec<DimensionWork> }

fn stable(v: &Value) -> Value { match v { Value::Array(a) => Value::Array(a.iter().map(stable).collect()), Value::Object(m) => { let mut out = Map::new(); for (k,v) in m.iter() { out.insert(k.clone(), stable(v)); } Value::Object(out) }, _ => v.clone() } }
pub fn fingerprint(value: &Value) -> String { let bytes = serde_json::to_vec(&stable(value)).expect("JSON serialization"); format!("xxh128:{:032x}", xxh3_128(&bytes)) }
fn sorted(values: impl IntoIterator<Item=String>) -> Vec<String> { let mut v: Vec<_> = values.into_iter().filter(|x| !x.is_empty()).collect(); v.sort(); v.dedup(); v }
pub fn graph_file_hashes(graph: &GraphGeneration) -> BTreeMap<String, String> { let mut out = BTreeMap::new(); for n in &graph.nodes { if n.kind != "file" { continue } for e in &n.evidence { if let (Some(path), Some(hash)) = (e.get("path").and_then(Value::as_str).or(n.path.as_deref()), e.get("contentHash").and_then(Value::as_str)) { out.insert(path.to_owned(), hash.to_owned()); break; } } } out }
fn claim_files(c: &Phase2Claim) -> Vec<String> { sorted(c.source.iter().cloned().chain(c.candidate_files.iter().cloned())) }
fn claim_desc(c: &Phase2Claim, hashes: &BTreeMap<String,String>) -> (Vec<String>, String, Vec<String>) { let files=claim_files(c); let inputs: Vec<Value>=files.iter().map(|p| serde_json::json!({"path":p,"contentHash":hashes.get(p)})).collect(); let missing=files.iter().filter(|p| !hashes.contains_key(*p)).cloned().collect(); let payload=serde_json::json!({"schemaVersion":PHASE2_INCREMENTAL_VERSION,"claim":{"id":c.id,"source":c.source,"line":c.line,"status":c.status,"text":c.text},"inputs":inputs}); (files,fingerprint(&payload),missing) }
fn verdict_value(v: &Phase2Verdict) -> Value { let mut m=serde_json::to_value(v).unwrap_or(Value::Null); if let Value::Object(ref mut x)=m { for k in ["inputFiles","inputFingerprint","reconciliationFingerprint","sourceGenerationId"] { x.remove(k); } } m }
pub fn verdict_result_fingerprint(v: &Phase2Verdict) -> String { fingerprint(&serde_json::json!({"schemaVersion":PHASE2_INCREMENTAL_VERSION,"verdict":verdict_value(v)})) }
pub fn reconcile_input_fingerprint(v: &Phase2Verdict) -> String { fingerprint(&serde_json::json!({"schemaVersion":PHASE2_INCREMENTAL_VERSION,"kind":"reconciliation","verdict":verdict_value(v)})) }
pub fn claim_evidence_fingerprint(c: &Phase2Claim, graph: &GraphGeneration) -> String { claim_desc(c,&graph_file_hashes(graph)).1 }
fn dim_desc(d: &str, m: &DimensionMetadata, hashes: &BTreeMap<String,String>, verdicts: &BTreeMap<String,Phase2Verdict>) -> (String, Vec<String>, Vec<String>, String) { let files=sorted(m.input_files.clone()); let ids=sorted(m.input_verdict_ids.clone()); let missing_files=files.iter().filter(|p| !hashes.contains_key(*p)).cloned().collect::<Vec<_>>(); let missing_verdicts=ids.iter().filter(|id| !verdicts.contains_key(*id)).cloned().collect::<Vec<_>>(); let file_inventory=fingerprint(&serde_json::json!({"schemaVersion":PHASE2_INCREMENTAL_VERSION,"kind":"repository-file-inventory","paths":hashes.keys().collect::<Vec<_>>() })); let inputs:Vec<Value>=files.iter().map(|p|serde_json::json!({"path":p,"contentHash":hashes.get(p)})).chain(ids.iter().map(|id|serde_json::json!({"claimId":id,"resultFingerprint":verdicts.get(id).map(verdict_result_fingerprint)}))).collect(); let input=fingerprint(&serde_json::json!({"schemaVersion":PHASE2_INCREMENTAL_VERSION,"dimension":d,"files":inputs.iter().take(files.len()).collect::<Vec<_>>(),"verdicts":inputs.iter().skip(files.len()).collect::<Vec<_>>() })); (file_inventory,missing_files,missing_verdicts,input) }
fn dim_value<'a>(u:&'a Understanding,d:&str)->Option<&'a Value>{match d{"architecture"=>u.architecture.as_ref(),"interfaces"=>u.interfaces.as_ref(),"health"=>u.health.as_ref(),"contract"=>u.contract.as_ref(),"security"=>u.security.as_ref(),"solid"=>u.solid.as_ref(),_=>None}}
pub fn build_incremental_phase2_plan(graph:&GraphGeneration, queue:&Phase2Queue, envelope:&VerdictEnvelope, understanding:&Understanding)->Phase2Plan { let hashes=graph_file_hashes(graph); let previous: BTreeMap<_,_>=envelope.verdicts.iter().map(|v|(v.claim_id.clone(),v.clone())).collect(); let mut reuse=Vec::new(); let mut verify=Vec::new(); for c in queue.claims.iter().filter(|c| c.status.as_deref().map(|s|VERIFIABLE.contains(&s)).unwrap_or(false)||!c.candidate_files.is_empty()) { let (files,fp,missing)=claim_desc(c,&hashes); if let Some(p)=previous.get(&c.id).filter(|p|p.input_fingerprint.as_deref()==Some(fp.as_str())&&missing.is_empty()) { let mut r=(*p).clone(); r.input_files=files; r.input_fingerprint=Some(fp); reuse.push(r); } else { verify.push(ClaimWork{claim_id:c.id.clone(),input_files:files,input_fingerprint:fp,missing_inputs:missing.clone(),reason:Some(if !missing.is_empty(){"missing_input"}else if previous.contains_key(&c.id){"input_changed"}else{"cold_start"}.into())}); } } let reused: BTreeMap<_,_>=reuse.iter().map(|v|(v.claim_id.clone(),v.clone())).collect(); let mut dr=Vec::new(); let mut ds=Vec::new(); for d in DIMENSIONS { let Some(value)=dim_value(understanding,d) else { ds.push(DimensionWork{dimension:d.into(),reason:"cold_start".into(),..Default::default()}); continue }; let Some(meta)=understanding.incremental.as_ref().and_then(|i|i.dimensions.get(d)) else { let _=value; ds.push(DimensionWork{dimension:d.into(),reason:"cold_start".into(),..Default::default()}); continue }; let (inv,mf,mv,input)=dim_desc(d,meta,&hashes,&reused); let empty=meta.input_files.is_empty()&&meta.input_verdict_ids.is_empty(); let shape=meta.file_inventory_fingerprint.as_deref()!=Some(inv.as_str()); if mf.is_empty()&&mv.is_empty()&&!empty&&!shape&&meta.input_fingerprint.as_deref()==Some(input.as_str()){dr.push(d.into())}else{ds.push(DimensionWork{dimension:d.into(),reason:if !mf.is_empty()||!mv.is_empty(){"affected_dependency"}else if shape{"repository_shape_changed"}else if empty{"missing_dependencies"}else{"input_changed"}.into(),missing_files:mf,missing_verdicts:mv});} } let complete=verify.is_empty()&&ds.is_empty(); Phase2Plan{schema_version:PHASE2_INCREMENTAL_VERSION,source_generation_id:graph.generation_id.clone(),verdicts:PlanVerdicts{reuse,verify},dimensions:PlanDimensions{reuse:dr,synthesize:ds},complete} }
pub fn requeue_changed_verdicts(graph:&GraphGeneration,queue:&Phase2Queue,envelope:&VerdictEnvelope)->Vec<ClaimWork>{build_incremental_phase2_plan(graph,queue,envelope,&Understanding::default()).verdicts.verify}
#[derive(Debug, thiserror::Error)] pub enum Phase2SealError { #[error("phase2_seal_invalid: {0}")] Invalid(String) }
pub fn validate_verdict_metadata(v: &Phase2Verdict, generation_id: &str) -> Result<(), Phase2SealError> {
    let Some(d) = v.derivation.as_ref() else { return Err(Phase2SealError::Invalid(format!("{} missing derivation metadata", v.claim_id))) };
    if d.method.trim().is_empty() || d.fingerprint.trim().is_empty() || d.evidence_refs.is_empty() { return Err(Phase2SealError::Invalid(format!("{} has invalid derivation metadata", v.claim_id))) }
    // provider/model/version is a joint contract with the semantic-producer lane (BM05): a derivation
    // record without an attributable provider/model/version cannot be sealed as queryable evidence.
    if d.provider.trim().is_empty() || d.model.trim().is_empty() || d.version.trim().is_empty() { return Err(Phase2SealError::Invalid(format!("{} missing provider/model/version derivation attribution", v.claim_id))) }
    let Some(check) = v.verification.as_ref() else { return Err(Phase2SealError::Invalid(format!("{} missing verification metadata", v.claim_id))) };
    if check.status.trim().is_empty() { return Err(Phase2SealError::Invalid(format!("{} has invalid verification metadata", v.claim_id))) }
    let Some(inv) = v.invalidation.as_ref() else { return Err(Phase2SealError::Invalid(format!("{} missing invalidation metadata", v.claim_id))) };
    if inv.generation_id != generation_id || inv.reason.trim().is_empty() { return Err(Phase2SealError::Invalid(format!("{} has invalid invalidation metadata", v.claim_id))) }
    let Some(sup) = v.supersession.as_ref() else { return Err(Phase2SealError::Invalid(format!("{} missing supersession metadata", v.claim_id))) };
    if sup.state.trim().is_empty() { return Err(Phase2SealError::Invalid(format!("{} has invalid supersession metadata", v.claim_id))) }
    Ok(())
}
pub fn seal_phase2_artifacts(graph:&GraphGeneration,queue:&Phase2Queue,envelope:&VerdictEnvelope,understanding:&Understanding)->Result<(VerdictEnvelope,Understanding),Phase2SealError>{ if graph.generation_id.is_empty(){return Err(Phase2SealError::Invalid("graph generationId is missing".into()))} let hashes=graph_file_hashes(graph); let supplied: BTreeMap<_,_>=envelope.verdicts.iter().map(|v|(v.claim_id.clone(),v)).collect(); let mut sealed=Vec::new(); for c in queue.claims.iter().filter(|c| c.status.as_deref().map(|s|VERIFIABLE.contains(&s)).unwrap_or(false)||!c.candidate_files.is_empty()){let Some(v)=supplied.get(&c.id) else{return Err(Phase2SealError::Invalid(format!("verdict missing for {}",c.id)))}; validate_verdict_metadata(v,&graph.generation_id)?; let(files,fp,missing)=claim_desc(c,&hashes);if !missing.is_empty(){return Err(Phase2SealError::Invalid(format!("evidence missing for {}: {}",c.id,missing.join(", "))))} let mut x=(*v).clone();x.input_files=files;x.input_fingerprint=Some(fp);x.source_generation_id=Some(graph.generation_id.clone());x.reconciliation_fingerprint=Some(reconcile_input_fingerprint(&x));sealed.push(x)} let by:BTreeMap<_,_>=sealed.iter().map(|v|(v.claim_id.clone(),v.clone())).collect(); let Some(inc)=understanding.incremental.as_ref() else{return Err(Phase2SealError::Invalid("incremental metadata missing".into()))}; let mut dimensions=BTreeMap::new(); for d in DIMENSIONS {if dim_value(understanding,d).is_none(){return Err(Phase2SealError::Invalid(format!("understanding.{d} is missing")))} let Some(m)=inc.dimensions.get(d) else{return Err(Phase2SealError::Invalid(format!("incremental metadata missing for {d}")))};let(inv,mf,mv,input)=dim_desc(d,m,&hashes,&by);if m.input_files.is_empty()&&m.input_verdict_ids.is_empty(){return Err(Phase2SealError::Invalid(format!("{d} declares no dependencies")))}if !mf.is_empty()||!mv.is_empty(){return Err(Phase2SealError::Invalid(format!("{d} has missing dependencies")))}dimensions.insert(d.into(),DimensionMetadata{input_files:sorted(m.input_files.clone()),input_verdict_ids:sorted(m.input_verdict_ids.clone()),input_fingerprint:Some(input),file_inventory_fingerprint:Some(inv)});} let mut ve=envelope.clone();ve.verdicts=sealed;ve.source_generation_id=Some(graph.generation_id.clone());ve.incremental_schema_version=Some(PHASE2_INCREMENTAL_VERSION);let mut u=understanding.clone();u.source_generation_id=Some(graph.generation_id.clone());u.incremental=Some(IncrementalUnderstanding{schema_version:Some(PHASE2_INCREMENTAL_VERSION),dimensions});Ok((ve,u)) }
