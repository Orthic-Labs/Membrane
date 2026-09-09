//! Deterministic lexical symbol index used by Blueprint graph consumers.
//!
//! This is an acceleration/projection over graph facts, not another graph or
//! authority.  Raw identifiers are retained in postings; normalization is
//! used only for lookup.

use crate::graph::GraphGeneration;
use crate::model::GraphNode;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexToken { pub raw: String, pub normalized: String }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LexicalIndex {
    pub generation_id: String,
    pub source_hash: String,
    /// Only complete indexes may be used for pruning.  A false value always
    /// broadens to an authoritative source scan.
    pub complete: bool,
    pub terms: BTreeMap<String, BTreeSet<String>>,
    pub trigrams: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexHit { pub node_id: String, pub raw_terms: Vec<String> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSet {
    pub ids: Vec<String>,
    pub complete: bool,
    pub fallback_required: bool,
    pub reason: Option<String>,
}

impl LexicalIndex {
    pub fn build(generation: &GraphGeneration) -> Self {
        let mut index = Self { generation_id: generation.generation_id.clone(), source_hash: generation.source_hash.clone(), complete: generation.complete, ..Self::default() };
        for node in &generation.nodes { index.add_node(node); }
        index
    }

    pub fn add_node(&mut self, node: &GraphNode) {
        let mut raw = Vec::new();
        if let Some(name) = &node.name { raw.push(name.clone()); }
        if let Some(path) = &node.path { raw.push(path.clone()); }
        for evidence in &node.evidence {
            if let Some(q) = evidence.get("qualifiedName").and_then(|v| v.as_str()) { raw.push(q.to_owned()); }
            if let Some(text) = evidence.get("text").and_then(|v| v.as_str()) { raw.push(text.to_owned()); }
        }
        for value in raw {
            for token in tokenize_identifier(&value) {
                self.terms.entry(token.normalized.clone()).or_default().insert(node.id.clone());
                for gram in trigrams(&token.normalized) { self.trigrams.entry(gram).or_default().insert(node.id.clone()); }
            }
        }
    }

    pub fn search(&self, query: &str) -> Vec<IndexHit> {
        let tokens = tokenize_identifier(query);
        if tokens.is_empty() { return Vec::new(); }
        let mut ids = BTreeSet::new();
        for token in &tokens { if let Some(found) = self.terms.get(&token.normalized) { ids.extend(found.iter().cloned()); } }
        ids.into_iter().map(|node_id| IndexHit { node_id, raw_terms: tokens.iter().map(|t| t.raw.clone()).collect() }).collect()
    }

    pub fn candidate_ids_for_literal(&self, literal: &str) -> Vec<String> {
        self.candidates_for_literal(literal, None, None).ids
    }

    /// Return trigram candidates with explicit freshness/completeness state.
    /// Callers must not interpret `ids` as an authoritative result when
    /// `fallback_required` is true.
    pub fn candidates_for_literal(&self, literal: &str, expected_generation: Option<&str>, expected_source_hash: Option<&str>) -> CandidateSet {
        if !self.complete {
            return CandidateSet { ids: Vec::new(), complete: false, fallback_required: true, reason: Some("index_incomplete".into()) };
        }
        if expected_generation.is_some_and(|expected| expected != self.generation_id) {
            return CandidateSet { ids: Vec::new(), complete: false, fallback_required: true, reason: Some("generation_mismatch".into()) };
        }
        if expected_source_hash.is_some_and(|expected| expected != self.source_hash) {
            return CandidateSet { ids: Vec::new(), complete: false, fallback_required: true, reason: Some("source_hash_mismatch".into()) };
        }
        let grams = trigrams(&literal.to_ascii_lowercase());
        if grams.is_empty() { return CandidateSet { ids: Vec::new(), complete: true, fallback_required: false, reason: None }; }
        let mut sets = grams.iter().filter_map(|g| self.trigrams.get(g));
        let Some(first) = sets.next() else { return CandidateSet { ids: Vec::new(), complete: true, fallback_required: false, reason: None }; };
        let mut ids = first.clone();
        for set in sets { ids.retain(|id| set.contains(id)); }
        CandidateSet { ids: ids.into_iter().collect(), complete: true, fallback_required: false, reason: None }
    }

    /// Resolve literal candidates without allowing an index to create false
    /// negatives.  Trigrams are advisory only: authoritative resolution scans
    /// the complete generation corpus, while completeness/freshness remains
    /// explicit for telemetry and future bounded acceleration.
    pub fn authoritative_literal_candidates(&self, literal: &str, generation: &GraphGeneration) -> Vec<String> {
        let _candidate_set = self.candidates_for_literal(literal, Some(&generation.generation_id), Some(&generation.source_hash));
        let needle = literal.to_ascii_lowercase();
        generation.nodes.iter().filter(|node| node_fields(node).iter().any(|field| field.to_ascii_lowercase().contains(&needle))).map(|node| node.id.clone()).collect()
    }
}

pub fn tokenize_identifier(value: &str) -> Vec<IndexToken> {
    let mut out = Vec::new();
    for raw in value.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if raw.is_empty() { continue; }
        let mut start = 0usize;
        let chars: Vec<char> = raw.chars().collect();
        for i in 1..chars.len() {
            let boundary = (chars[i].is_uppercase() && chars[i - 1].is_lowercase())
                || (chars[i].is_uppercase() && chars[i - 1].is_uppercase() && i + 1 < chars.len() && chars[i + 1].is_lowercase());
            if boundary { push_token(&mut out, &chars[start..i]); start = i; }
        }
        push_token(&mut out, &chars[start..]);
    }
    out
}

fn push_token(out: &mut Vec<IndexToken>, chars: &[char]) {
    if chars.is_empty() { return; }
    let raw: String = chars.iter().collect();
    let normalized = raw.chars().flat_map(|c| c.to_lowercase()).collect::<String>();
    out.push(IndexToken { raw, normalized });
}

fn trigrams(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 3 { return vec![value.to_owned()]; }
    chars.windows(3).map(|w| w.iter().collect()).collect()
}

fn node_fields(node: &GraphNode) -> Vec<String> {
    let mut fields = Vec::new();
    if let Some(name) = &node.name { fields.push(name.clone()); }
    if let Some(path) = &node.path { fields.push(path.clone()); }
    for evidence in &node.evidence {
        if let Some(q) = evidence.get("qualifiedName").and_then(|v| v.as_str()) { fields.push(q.to_owned()); }
        if let Some(text) = evidence.get("text").and_then(|v| v.as_str()) { fields.push(text.to_owned()); }
    }
    fields
}

pub fn index_generation(generation: &GraphGeneration) -> LexicalIndex { LexicalIndex::build(generation) }
