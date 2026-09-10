//! Conservative evidence re-anchoring (ported from `blueprint/src/graph/reanchor.mjs`).
//!
//! No fuzzy score is allowed to choose a winner. The admissible chain is
//! exact semantic entity -> exact fingerprint -> unique normalized text. If
//! no tier yields exactly one winner, the anchor is `stale` or `ambiguous`,
//! never silently moved.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn normalized_anchor_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn anchor_fingerprint(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let hex = digest.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    format!("sha256:{hex}")
}

fn str_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| value.get(key).and_then(Value::as_str))
}

fn candidate_id(candidate: &Value, index: usize) -> String {
    str_field(candidate, &["id", "occurrenceId", "claimId"])
        .map(|s| s.to_owned())
        .unwrap_or_else(|| format!("candidate:{index}"))
}

fn unique_matches<'a>(candidates: &'a [Value], predicate: impl Fn(&Value) -> bool) -> Vec<(String, &'a Value)> {
    candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| predicate(candidate))
        .map(|(index, candidate)| (candidate_id(candidate, index), candidate))
        .collect()
}

fn outcome(tier: &str, matches: Vec<(String, &Value)>) -> Option<Value> {
    if matches.len() == 1 {
        let (id, candidate) = &matches[0];
        return Some(json!({
            "state": "reanchored", "tier": tier, "targetId": id, "target": candidate,
            "candidates": [id],
        }));
    }
    if matches.len() > 1 {
        let mut ids: Vec<String> = matches.into_iter().map(|(id, _)| id).collect();
        ids.sort();
        return Some(json!({
            "state": "ambiguous", "tier": tier, "targetId": Value::Null, "target": Value::Null,
            "candidates": ids,
        }));
    }
    None
}

/// `previous` and each entry of `current_candidates` carry the legacy shape:
/// `{ portableId|entityPortableId, fingerprint|contentFingerprint, text }`.
pub fn reanchor_evidence(previous: &Value, current_candidates: &[Value]) -> Value {
    let portable_id = str_field(previous, &["portableId", "entityPortableId"]);
    if let Some(portable_id) = portable_id {
        let matches = unique_matches(current_candidates, |candidate| {
            str_field(candidate, &["portableId", "entityPortableId"]) == Some(portable_id)
        });
        if let Some(result) = outcome("exact_entity", matches) {
            return result;
        }
    }

    let previous_text = previous.get("text").and_then(Value::as_str);
    let fingerprint: Option<String> = str_field(previous, &["fingerprint", "contentFingerprint"])
        .map(|s| s.to_owned())
        .or_else(|| previous_text.map(anchor_fingerprint));
    if let Some(fingerprint) = fingerprint {
        let matches = unique_matches(current_candidates, |candidate| {
            let candidate_text = candidate.get("text").and_then(Value::as_str);
            let candidate_fingerprint: Option<String> = str_field(candidate, &["fingerprint", "contentFingerprint"])
                .map(|s| s.to_owned())
                .or_else(|| candidate_text.map(anchor_fingerprint));
            candidate_fingerprint.as_deref() == Some(fingerprint.as_str())
        });
        if let Some(result) = outcome("exact_fingerprint", matches) {
            return result;
        }
    }

    let normalized = previous_text.map(normalized_anchor_text).unwrap_or_default();
    if !normalized.is_empty() {
        let matches = unique_matches(current_candidates, |candidate| {
            candidate.get("text").and_then(Value::as_str).map(normalized_anchor_text).as_deref() == Some(normalized.as_str())
        });
        if let Some(result) = outcome("unique_normalized_text", matches) {
            return result;
        }
    }

    json!({
        "state": "stale", "tier": "none", "targetId": Value::Null, "target": Value::Null,
        "candidates": [], "reason": "no_exact_reanchor",
    })
}
