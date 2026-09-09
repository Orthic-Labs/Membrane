//! Generation-bound document declaration versus observed-code projection.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const GROUNDING_STATES: [&str; 6] = ["direct", "indirect", "unsupported", "contradicted", "ambiguous", "stale"];
const DETERMINISTIC: [&str; 3] = ["EXTRACTED", "DETERMINISTIC_EXTRACTION", "AUTHORITATIVE_SEMANTIC"];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentClaim { pub id: String, #[serde(default)] pub document_id: Option<String>, #[serde(default)] pub source: Option<String>, #[serde(default)] pub line: Option<u64>, #[serde(default)] pub status: Option<String>, #[serde(default)] pub sha1: Option<String>, #[serde(default)] pub edges: Vec<TruthEdge> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruthEdge { pub kind: String, #[serde(default)] pub source: Option<String>, #[serde(default)] pub target: Option<String>, #[serde(default)] pub reason: Option<String>, #[serde(default)] pub confidence_class: Option<String>, #[serde(default)] pub confidence: Option<f64>, #[serde(default)] pub evidence_doc_path: Option<String>, #[serde(default)] pub evidence_doc_line: Option<u64>, #[serde(default)] pub evidence_doc_sha1: Option<String>, #[serde(default)] pub evidence_code_path: Option<String>, #[serde(default)] pub evidence_code_node_id: Option<String>, #[serde(default)] pub evidence_code_content_hash: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Citation { pub kind: String, pub path: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] pub line: Option<u64>, #[serde(skip_serializing_if = "Option::is_none")] pub content_hash: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] pub node_id: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedFact { pub kind: String, pub source: Option<String>, pub target: Option<String>, pub reason: Option<String>, pub provenance: Option<String>, pub confidence: Option<f64>, pub evidence: Vec<Citation> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mismatch { pub present: bool, pub reason: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundedClaim { pub claim_id: String, pub declared: Declaration, pub grounding: String, pub observed: Vec<ObservedFact>, pub mismatch: Mismatch, pub citations: Vec<Citation>, pub confidence: Option<Value>, pub invalidation: Invalidation }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Declaration { pub document_id: Option<String>, pub source: Option<String>, pub line: Option<u64>, pub status: String, pub source_hash: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invalidation { pub generation_id: Option<String>, pub freshness: String, pub stale: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTruthProjection { pub schema_version: u32, pub kind: String, pub generation_id: Option<String>, pub freshness: String, pub claims: Vec<GroundedClaim>, pub supersedes: Vec<Value>, pub counts: std::collections::BTreeMap<String, usize> }

fn citations(edge: &TruthEdge) -> Vec<Citation> { let mut out=Vec::new(); if edge.evidence_doc_path.is_some(){out.push(Citation{kind:"document".into(),path:edge.evidence_doc_path.clone(),line:edge.evidence_doc_line,content_hash:edge.evidence_doc_sha1.clone(),node_id:None})} if edge.evidence_code_path.is_some(){out.push(Citation{kind:"code".into(),path:edge.evidence_code_path.clone(),line:None,content_hash:edge.evidence_code_content_hash.clone(),node_id:edge.evidence_code_node_id.clone()})} out }
fn state(edges:&[TruthEdge], freshness:&str)->String { if freshness!="fresh" {return "stale".into()} let kinds:BTreeSet<_>=edges.iter().map(|e|e.kind.as_str()).collect(); if kinds.contains("supports")&&kinds.contains("contradicts"){"ambiguous"}else if kinds.contains("contradicts"){"contradicted"}else if kinds.contains("supports"){"direct"}else if kinds.contains("supersedes"){"indirect"}else{"unsupported"}.into() }
fn public_confidence(edge:&TruthEdge)->Option<f64>{if edge.confidence_class.as_deref().map(|x|DETERMINISTIC.contains(&x)).unwrap_or(false){None}else{edge.confidence}}
pub fn project_document_truth(claims:&[DocumentClaim],supersedes:&[Value],generation_id:Option<String>,freshness:&str)->DocumentTruthProjection { let mut out=Vec::new(); for claim in claims {let grounding=state(&claim.edges,freshness);let mut cs=Vec::new();let mut seen=BTreeSet::new();let mut observed=Vec::new();for e in &claim.edges{let ec=citations(e);for c in &ec{let key=serde_json::to_string(c).unwrap_or_default();if seen.insert(key){cs.push(c.clone())}}observed.push(ObservedFact{kind:e.kind.clone(),source:e.source.clone(),target:e.target.clone(),reason:e.reason.clone(),provenance:e.confidence_class.clone(),confidence:public_confidence(e),evidence:ec})}let mismatch=if grounding=="contradicted"{Mismatch{present:true,reason:Some("declared_intent_conflicts_with_observed_code".into())}}else if grounding=="ambiguous"{Mismatch{present:true,reason:Some("conflicting_grounding_evidence".into())}}else{Mismatch{present:false,reason:None}};let confidence=if !observed.is_empty()&&observed.iter().all(|x|x.confidence.is_none()){Value::Null}else{Value::Array(observed.iter().filter_map(|x|x.confidence.map(|v|serde_json::json!(v))).collect())};out.push(GroundedClaim{claim_id:claim.id.clone(),declared:Declaration{document_id:claim.document_id.clone(),source:claim.source.clone(),line:claim.line,status:claim.status.clone().unwrap_or_else(||"unknown".into()),source_hash:claim.sha1.clone()},grounding:grounding.clone(),observed,mismatch,citations:cs,confidence:Some(confidence),invalidation:Invalidation{generation_id:generation_id.clone(),freshness:freshness.into(),stale:grounding=="stale"}})}let mut counts=std::collections::BTreeMap::new();for s in GROUNDING_STATES{counts.insert(s.into(),out.iter().filter(|x|x.grounding==s).count());}DocumentTruthProjection{schema_version:1,kind:"document-truth-grounding".into(),generation_id,freshness:freshness.into(),claims:out,supersedes:supersedes.to_vec(),counts} }
