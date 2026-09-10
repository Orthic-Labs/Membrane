//! Non-compensatory evidence-authority resolution for competing facts.
//!
//! Native Rust port of `blueprint/src/graph/evidence-authority.mjs` (with the
//! small `provenance.mjs` helpers it depends on mirrored locally below).
//! Canon ordering from INV-005 / BPC-004: lower rank is stronger.
//! `LIVE_VERIFICATION` is a cross-check receipt, not a canonical producer
//! (§5.2).
//!
//! Operates on `serde_json::Value` candidate/fact shapes, the same idiom used
//! by [`crate::entry_points`], so behavior matches the legacy JS exactly
//! field-for-field.

use serde_json::{json, Map, Value};
use std::collections::HashSet;

// ---------------------------------------------------------------------
// provenance.mjs mirror (private: only what evidence-authority needs)
// ---------------------------------------------------------------------

pub const FACT_PROVENANCE_AUTHORITATIVE_SEMANTIC: &str = "AUTHORITATIVE_SEMANTIC";
pub const FACT_PROVENANCE_LIVE_VERIFICATION: &str = "LIVE_VERIFICATION";
pub const FACT_PROVENANCE_RULE_RESOLVED: &str = "RULE_RESOLVED";
pub const FACT_PROVENANCE_STRUCTURAL_RESOLVED: &str = "STRUCTURAL_RESOLVED";
pub const FACT_PROVENANCE_FRAMEWORK_RESOLVED: &str = "FRAMEWORK_RESOLVED";
pub const FACT_PROVENANCE_HEURISTIC_BRIDGE: &str = "HEURISTIC_BRIDGE";
pub const FACT_PROVENANCE_UNRESOLVED: &str = "UNRESOLVED";

const KNOWN_PROVENANCE: [&str; 7] = [
    FACT_PROVENANCE_AUTHORITATIVE_SEMANTIC,
    FACT_PROVENANCE_LIVE_VERIFICATION,
    FACT_PROVENANCE_RULE_RESOLVED,
    FACT_PROVENANCE_STRUCTURAL_RESOLVED,
    FACT_PROVENANCE_FRAMEWORK_RESOLVED,
    FACT_PROVENANCE_HEURISTIC_BRIDGE,
    FACT_PROVENANCE_UNRESOLVED,
];

fn is_known_provenance(provenance: &str) -> bool {
    KNOWN_PROVENANCE.contains(&provenance)
}

/// Mirrors `isInferentialProvenance`. Panics (mirroring the JS `TypeError`)
/// if `provenance` is not a known class.
pub fn is_inferential_provenance(provenance: &str) -> bool {
    assert_known_provenance(provenance);
    provenance == FACT_PROVENANCE_HEURISTIC_BRIDGE
}

fn assert_known_provenance(provenance: &str) {
    if !is_known_provenance(provenance) {
        panic!("unknown Blueprint provenance class: {provenance}");
    }
}

/// Mirrors `confidenceForProvenance`. Returns `Err` (mirroring the JS
/// `TypeError`) when `confidence` is present but not a finite number in
/// `[0,1]`. Note: JSON cannot represent `NaN`/`Infinity` directly, so those
/// legacy JS test cases have no Rust equivalent via `serde_json::Value`.
pub fn confidence_for_provenance(provenance: &str, confidence: Option<&Value>) -> Result<Option<f64>, String> {
    assert_known_provenance(provenance);
    if !is_inferential_provenance(provenance) {
        return Ok(None);
    }
    match confidence {
        None => Ok(None),
        Some(Value::Null) => Ok(None),
        Some(v) => {
            if let Value::Number(n) = v {
                if let Some(f) = n.as_f64() {
                    if f.is_finite() && (0.0..=1.0).contains(&f) {
                        return Ok(Some(f));
                    }
                }
            }
            Err(format!("inferential confidence must be a finite number in [0,1], got {v:?}"))
        }
    }
}

/// Mirrors `withFactProvenance`: returns a new object with `provenance` set
/// and `confidence` normalized through [`confidence_for_provenance`].
fn with_fact_provenance(fact: &Value, provenance: &str, confidence: Option<&Value>) -> Value {
    assert_known_provenance(provenance);
    let mut out = fact.as_object().cloned().unwrap_or_default();
    out.insert("provenance".to_string(), json!(provenance));
    let normalized = confidence_for_provenance(provenance, confidence).unwrap_or(None);
    out.insert(
        "confidence".to_string(),
        normalized.map(|v| json!(v)).unwrap_or(Value::Null),
    );
    Value::Object(out)
}

// ---------------------------------------------------------------------
// evidence-authority.mjs
// ---------------------------------------------------------------------

pub const SEMANTIC_AUTHORITY_ORDER: [&str; 6] = [
    FACT_PROVENANCE_AUTHORITATIVE_SEMANTIC,
    FACT_PROVENANCE_RULE_RESOLVED,
    FACT_PROVENANCE_STRUCTURAL_RESOLVED,
    FACT_PROVENANCE_FRAMEWORK_RESOLVED,
    FACT_PROVENANCE_HEURISTIC_BRIDGE,
    FACT_PROVENANCE_UNRESOLVED,
];

pub const RESOLUTION_SPECIFICITY_ORDER: [&str; 4] = [
    "EXACT_RESOLUTION",
    "SAME_FILE_LEXICAL",
    "CROSS_FILE_HEURISTIC",
    "UNRESOLVED",
];

fn authority_rank_of(provenance: &str) -> i64 {
    SEMANTIC_AUTHORITY_ORDER
        .iter()
        .position(|value| *value == provenance)
        .map(|i| i as i64)
        .unwrap_or(SEMANTIC_AUTHORITY_ORDER.len() as i64)
}

fn specificity_rank_of(tier: Option<&str>) -> i64 {
    match tier {
        Some(tier) => RESOLUTION_SPECIFICITY_ORDER
            .iter()
            .position(|value| *value == tier)
            .map(|i| i as i64)
            .unwrap_or(RESOLUTION_SPECIFICITY_ORDER.len() as i64),
        None => RESOLUTION_SPECIFICITY_ORDER.len() as i64,
    }
}

fn legacy_compiler_provider(provider: &str) -> bool {
    provider == "blueprint-scip" || provider == "scip-python"
}

const COHERENT_SOURCE_STATES: [&str; 4] = ["equal", "current", "clean", "fresh"];
const STALE_SOURCE_STATES: [&str; 4] = ["stale", "behind", "ahead", "diverged"];

fn usable_identity(value: &Value) -> bool {
    match value {
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i >= 0,
            None => n.as_u64().is_some(),
        },
        _ => false,
    }
}

fn pick_defined(vals: Vec<Option<Value>>) -> Option<Value> {
    for v in vals {
        if let Some(val) = v {
            if !val.is_null() {
                return Some(val);
            }
        }
    }
    None
}

fn source_relation(candidate: &Value) -> String {
    let raw = pick_defined(vec![
        candidate.get("sourceRelation").cloned(),
        candidate
            .get("sourceState")
            .and_then(|s| s.get("relation"))
            .cloned(),
        candidate.get("sourceState").cloned(),
        candidate
            .get("freshness")
            .and_then(|f| f.get("relation"))
            .cloned(),
        candidate.get("freshness").cloned(),
    ]);
    match raw {
        Some(Value::String(s)) => s.to_lowercase(),
        _ => "unknown".to_string(),
    }
}

/// Mirrors `sourceCoherenceRank`.
pub fn source_coherence_rank(candidate: &Value, target_source_state: Option<&Value>) -> i64 {
    let relation = source_relation(candidate);
    let source_coherent_false = candidate.get("sourceCoherent") == Some(&Value::Bool(false));
    if source_coherent_false || STALE_SOURCE_STATES.contains(&relation.as_str()) {
        return 1;
    }
    let identity = pick_defined(vec![
        candidate.get("sourceStateId").cloned(),
        candidate.get("generationId").cloned(),
    ]);
    if let Some(id) = &identity {
        if !usable_identity(id) {
            return 2;
        }
    }
    if let Some(target) = target_source_state {
        let target_value = match target {
            Value::String(_) | Value::Number(_) => Some(target.clone()),
            _ => pick_defined(vec![
                target.get("id").cloned(),
                target.get("generationId").cloned(),
            ]),
        };
        let identity_usable = identity.as_ref().map(usable_identity).unwrap_or(false);
        let target_usable = target_value.as_ref().map(usable_identity).unwrap_or(false);
        if !identity_usable || !target_usable {
            return 2;
        }
        return if identity == target_value { 0 } else { 1 };
    }
    let source_coherent_true = candidate.get("sourceCoherent") == Some(&Value::Bool(true));
    if source_coherent_true || COHERENT_SOURCE_STATES.contains(&relation.as_str()) {
        return 0;
    }
    2
}

/// Mirrors `semanticAuthorityForFact`.
pub fn semantic_authority_for_fact(candidate: &Value) -> &'static str {
    let resolved_false = candidate.get("resolved") == Some(&Value::Bool(false));
    let confidence_tier = candidate.get("confidenceTier").and_then(Value::as_str);
    if resolved_false || confidence_tier == Some("UNRESOLVED") {
        return FACT_PROVENANCE_UNRESOLVED;
    }
    if let Some(provenance) = candidate.get("provenance") {
        if !provenance.is_null() {
            return match provenance.as_str() {
                Some(p) if is_known_provenance(p) => provenance_static(p),
                _ => FACT_PROVENANCE_UNRESOLVED,
            };
        }
    }
    if confidence_tier == Some("CROSS_FILE_HEURISTIC") {
        return FACT_PROVENANCE_HEURISTIC_BRIDGE;
    }
    let provider = pick_defined(vec![
        candidate.get("provider").and_then(|p| p.get("id")).cloned(),
        candidate.get("provider").cloned(),
        candidate
            .get("sourceProvider")
            .and_then(|p| p.get("id"))
            .cloned(),
    ]);
    let provider_str = provider.as_ref().and_then(Value::as_str).unwrap_or("");
    let precision_tier = candidate.get("precisionTier").and_then(Value::as_str);
    if precision_tier == Some("COMPILER") || legacy_compiler_provider(provider_str) {
        return FACT_PROVENANCE_AUTHORITATIVE_SEMANTIC;
    }
    if confidence_tier == Some("SAME_FILE_LEXICAL") {
        return FACT_PROVENANCE_STRUCTURAL_RESOLVED;
    }
    if confidence_tier == Some("EXACT_RESOLUTION") {
        return FACT_PROVENANCE_RULE_RESOLVED;
    }
    FACT_PROVENANCE_UNRESOLVED
}

fn provenance_static(p: &str) -> &'static str {
    KNOWN_PROVENANCE.iter().find(|v| **v == p).copied().unwrap_or(FACT_PROVENANCE_UNRESOLVED)
}

/// Mirrors `semanticAuthorityRankForFact`.
pub fn semantic_authority_rank_for_fact(candidate: &Value) -> i64 {
    authority_rank_of(semantic_authority_for_fact(candidate))
}

/// Mirrors `resolutionSpecificityRank`.
pub fn resolution_specificity_rank(candidate: &Value) -> i64 {
    specificity_rank_of(candidate.get("confidenceTier").and_then(Value::as_str))
}

fn requested_relation_matches(candidate: &Value, requested_relation: Option<&str>) -> bool {
    match requested_relation {
        None => true,
        Some(requested) => {
            let kind = pick_defined(vec![candidate.get("relation").cloned(), candidate.get("kind").cloned()]);
            kind.as_ref().and_then(Value::as_str) == Some(requested)
        }
    }
}

fn admissible(candidate: &Value, requested_relation: Option<&str>) -> bool {
    candidate.is_object()
        && candidate.get("admissible") != Some(&Value::Bool(false))
        && candidate.get("scopeAllowed") != Some(&Value::Bool(false))
        && requested_relation_matches(candidate, requested_relation)
}

fn inferential_confidence(candidate: &Value) -> Option<f64> {
    let provenance = semantic_authority_for_fact(candidate);
    if !is_inferential_provenance(provenance) {
        return None;
    }
    match candidate.get("confidence") {
        Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite() && (0.0..=1.0).contains(f)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AuthorityVector {
    coherence: i64,
    authority: i64,
    specificity: i64,
    inferential_confidence: Option<f64>,
}

fn vector(candidate: &Value, target_source_state: Option<&Value>) -> AuthorityVector {
    AuthorityVector {
        coherence: source_coherence_rank(candidate, target_source_state),
        authority: semantic_authority_rank_for_fact(candidate),
        specificity: resolution_specificity_rank(candidate),
        inferential_confidence: inferential_confidence(candidate),
    }
}

fn vector_to_json(v: &AuthorityVector) -> Value {
    json!({
        "coherence": v.coherence,
        "authority": v.authority,
        "specificity": v.specificity,
        "inferentialConfidence": v.inferential_confidence,
    })
}

fn compare_vectors(left: &AuthorityVector, right: &AuthorityVector) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut ord = left.coherence.cmp(&right.coherence);
    if ord == Ordering::Equal {
        ord = left.authority.cmp(&right.authority);
    }
    if ord == Ordering::Equal {
        ord = left.specificity.cmp(&right.specificity);
    }
    if ord == Ordering::Equal {
        let l = left.inferential_confidence.unwrap_or(-1.0);
        let r = right.inferential_confidence.unwrap_or(-1.0);
        // JS: (right - left); negative => left first.
        ord = (r - l).partial_cmp(&0.0).unwrap_or(Ordering::Equal);
    }
    ord
}

fn same_categorical_vector(left: &AuthorityVector, right: &AuthorityVector) -> bool {
    left.coherence == right.coherence && left.authority == right.authority && left.specificity == right.specificity
}

fn candidate_target(candidate: &Value, requested_relation: Option<&str>) -> Option<Value> {
    let obj = candidate.as_object();
    for key in ["target", "targetId", "entityId"] {
        if let Some(obj) = obj {
            if obj.contains_key(key) {
                return Some(obj.get(key).cloned().unwrap_or(Value::Null));
            }
        }
    }
    let has_source = obj.map(|o| o.contains_key("source")).unwrap_or(false);
    if requested_relation.is_some() || has_source {
        None
    } else {
        candidate.get("id").cloned()
    }
}

fn target_as_string(target: &Option<Value>) -> Option<String> {
    match target {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

fn normalized_candidate(candidate: &Value, provenance: &str) -> Value {
    with_fact_provenance(candidate, provenance, candidate.get("confidence"))
}

struct Admissible {
    raw: Value,
    vector: AuthorityVector,
}

fn frontier(reason: &str, candidates: &[Admissible], details: Map<String, Value>) -> Value {
    let mut out = Map::new();
    out.insert("state".to_string(), json!("unresolved_frontier"));
    out.insert("reason".to_string(), json!(reason));
    out.insert("admitted".to_string(), Value::Null);
    out.insert(
        "candidates".to_string(),
        Value::Array(
            candidates
                .iter()
                .map(|c| json!({ "candidate": c.raw, "vector": vector_to_json(&c.vector) }))
                .collect(),
        ),
    );
    for (k, v) in details {
        out.insert(k, v);
    }
    Value::Object(out)
}

/// Evaluate competing evidence for one requested relationship.
///
/// Governing order is non-compensatory: admissibility -> source coherence ->
/// authority -> specificity -> inferential confidence (heuristic evidence
/// only). Mirrors `evaluateEvidence`.
pub fn evaluate_evidence(
    target_source_state: Option<&Value>,
    candidates: &[Value],
    requested_relation: Option<&str>,
) -> Value {
    let admissible_candidates: Vec<Admissible> = candidates
        .iter()
        .filter(|c| admissible(c, requested_relation))
        .map(|c| Admissible { raw: c.clone(), vector: vector(c, target_source_state) })
        .collect();

    if admissible_candidates.is_empty() {
        return frontier("no_admissible_evidence", &[], Map::new());
    }

    let invalid_confidence: Vec<&Admissible> = admissible_candidates
        .iter()
        .filter(|c| {
            is_inferential_provenance(semantic_authority_for_fact(&c.raw))
                && !matches!(c.raw.get("confidence"), None | Some(Value::Null))
                && c.vector.inferential_confidence.is_none()
        })
        .collect();
    if !invalid_confidence.is_empty() {
        let owned: Vec<Admissible> = invalid_confidence
            .into_iter()
            .map(|c| Admissible { raw: c.raw.clone(), vector: c.vector })
            .collect();
        return frontier("invalid_inferential_confidence", &owned, Map::new());
    }

    let verifications: Vec<Admissible> = admissible_candidates
        .iter()
        .filter(|c| c.raw.get("provenance").and_then(Value::as_str) == Some(FACT_PROVENANCE_LIVE_VERIFICATION))
        .map(|c| Admissible { raw: c.raw.clone(), vector: c.vector })
        .collect();
    let mut admitted_candidates: Vec<Admissible> = admissible_candidates
        .into_iter()
        .filter(|c| c.raw.get("provenance").and_then(Value::as_str) != Some(FACT_PROVENANCE_LIVE_VERIFICATION))
        .collect();

    if admitted_candidates.is_empty() {
        return frontier("verification_without_canonical_evidence", &verifications, Map::new());
    }

    admitted_candidates.sort_by(|left, right| {
        compare_vectors(&left.vector, &right.vector).then_with(|| {
            let l = left.raw.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            let r = right.raw.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            l.cmp(&r)
        })
    });

    let best = &admitted_candidates[0];
    let best_vector = best.vector;

    if best_vector.coherence != 0 {
        let mut details = Map::new();
        details.insert("bestVector".to_string(), vector_to_json(&best_vector));
        return frontier("no_source_coherent_evidence", &admitted_candidates, details);
    }
    if semantic_authority_for_fact(&best.raw) == FACT_PROVENANCE_UNRESOLVED {
        let mut details = Map::new();
        details.insert("bestVector".to_string(), vector_to_json(&best_vector));
        return frontier("resolution_unresolved", &admitted_candidates, details);
    }

    let categorical_peers: Vec<&Admissible> = admitted_candidates
        .iter()
        .filter(|c| same_categorical_vector(&c.vector, &best_vector))
        .collect();
    let mut finalists: Vec<&Admissible> = categorical_peers;
    if is_inferential_provenance(semantic_authority_for_fact(&best.raw)) {
        let best_confidence = best_vector.inferential_confidence;
        finalists.retain(|c| c.vector.inferential_confidence == best_confidence);
    }

    let targets: HashSet<Option<String>> = finalists
        .iter()
        .map(|c| target_as_string(&candidate_target(&c.raw, requested_relation)))
        .collect();
    // Raw (non-string-coerced) targets, to detect missing/invalid targets.
    let raw_targets: Vec<Option<Value>> = finalists.iter().map(|c| candidate_target(&c.raw, requested_relation)).collect();
    let any_missing = raw_targets.iter().any(|t| target_as_string(t).is_none());
    if any_missing {
        let owned: Vec<Admissible> = finalists.iter().map(|c| Admissible { raw: c.raw.clone(), vector: c.vector }).collect();
        let mut details = Map::new();
        details.insert("bestVector".to_string(), vector_to_json(&best_vector));
        return frontier("resolution_target_missing", &owned, details);
    }
    if targets.len() > 1 {
        let mut sorted_targets: Vec<String> = targets.into_iter().flatten().collect();
        sorted_targets.sort();
        let owned: Vec<Admissible> = finalists.iter().map(|c| Admissible { raw: c.raw.clone(), vector: c.vector }).collect();
        let mut details = Map::new();
        details.insert("bestVector".to_string(), vector_to_json(&best_vector));
        details.insert("targets".to_string(), json!(sorted_targets));
        return frontier("authority_tie_conflict", &owned, details);
    }

    let target = candidate_target(&best.raw, requested_relation);
    let conflicts: Vec<&Admissible> = verifications
        .iter()
        .filter(|c| {
            c.vector.coherence == 0
                && {
                    let t = candidate_target(&c.raw, requested_relation);
                    !matches!(t, None | Some(Value::Null)) && t != target
                }
        })
        .collect();
    if !conflicts.is_empty() {
        let mut owned = vec![Admissible { raw: best.raw.clone(), vector: best.vector }];
        owned.extend(conflicts.into_iter().map(|c| Admissible { raw: c.raw.clone(), vector: c.vector }));
        let mut details = Map::new();
        details.insert("bestVector".to_string(), vector_to_json(&best_vector));
        return frontier("resolution_conflict", &owned, details);
    }

    json!({
        "state": "admitted",
        "reason": "categorical_precedence",
        "admitted": normalized_candidate(&best.raw, semantic_authority_for_fact(&best.raw)),
        "vector": vector_to_json(&best_vector),
        "equivalentEvidence": finalists.iter().map(|c| normalized_candidate(&c.raw, semantic_authority_for_fact(&c.raw))).collect::<Vec<_>>(),
        "verifications": verifications.iter().map(|c| normalized_candidate(&c.raw, FACT_PROVENANCE_LIVE_VERIFICATION)).collect::<Vec<_>>(),
    })
}
