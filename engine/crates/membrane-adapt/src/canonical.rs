//! Canonical deterministic JSON, SHA-256 digests, and stable identifier
//! derivation for Adapt records.
//!
//! Every durable Adapt identity is derived by deterministic code from canonical
//! semantics; no model or incidental processing order ever assigns an ID.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Serialize a JSON value canonically: object keys sorted lexicographically,
/// no insignificant whitespace, UTF-8 preserved. This is the byte-stable form
/// every digest in the crate is computed over.
pub fn to_canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => {
            // serde_json string escaping is itself deterministic.
            let quoted = serde_json::to_string(s).expect("string serialization cannot fail");
            out.push_str(&quoted);
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let quoted = serde_json::to_string(key).expect("string serialization cannot fail");
                out.push_str(&quoted);
                out.push(':');
                write_canonical(&map[*key], out);
            }
            out.push('}');
        }
    }
}

/// Build a sorted, canonical object from an iterator of key/value pairs.
pub fn canonical_object<'a>(pairs: impl IntoIterator<Item = (&'a str, Value)>) -> Value {
    let mut map = Map::new();
    for (k, v) in pairs {
        map.insert(k.to_string(), v);
    }
    Value::Object(map)
}

/// SHA-256 of a byte slice, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// SHA-256 of canonical JSON text, lowercase hex.
pub fn sha256_canonical(value: &Value) -> String {
    sha256_hex(to_canonical_json(value).as_bytes())
}

pub const PREFERENCE_ID_PREFIX: &str = "adapt";
pub const PREFERENCE_HASH_LEN: usize = 10;
const NUL: u8 = 0u8;

/// Normalize preference text for identity: trim, lowercase, collapse internal
/// whitespace, strip trailing punctuation. Original wording is preserved on
/// records; only identity uses this form.
pub fn normalize_text(text: &str) -> String {
    let mut s = text.trim().to_lowercase();
    while s.contains("  ") || s.contains('\t') || s.contains('\n') {
        s = s.replace(['\t', '\n'], " ");
        while s.contains("  ") {
            s = s.replace("  ", " ");
        }
    }
    let trimmed = s.trim_end_matches(['.', '!', '?', ',', ';', ':']);
    trimmed.to_string()
}

/// Kebab-case slug of up to `max_words` alphabetic words; falls back to
/// `"preference"` when no words exist.
pub fn slug_from_text(text: &str, max_words: usize) -> String {
    let words: Vec<String> = text
        .split(|c: char| !(c.is_ascii_alphabetic() || c.is_ascii_digit()))
        .filter(|w| w.len() >= 2 && w.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
        .take(max_words)
        .map(|w| w.to_lowercase())
        .collect();
    if words.is_empty() {
        return "preference".to_string();
    }
    words.join("-")
}

/// Deterministic preference record ID:
/// `adapt-{category}-{slug}-{sha256(scope NUL category NUL normalized_rule)[:10]}`.
///
/// The NUL separators prevent `(scope="ab", category="cd")` colliding with
/// `(scope="a", category="bcd")`.
pub fn derive_preference_id(scope: &str, category: &str, rule: &str) -> String {
    let norm = normalize_text(rule);
    let mut hasher = Sha256::new();
    hasher.update(scope.as_bytes());
    hasher.update([NUL]);
    hasher.update(category.as_bytes());
    hasher.update([NUL]);
    hasher.update(norm.as_bytes());
    let suffix = &hex::encode(hasher.finalize())[..PREFERENCE_HASH_LEN];
    format!(
        "{}-{}-{}-{}",
        PREFERENCE_ID_PREFIX,
        category,
        slug_from_text(rule, 4),
        suffix
    )
}

/// Deterministic evidence ID: `ev-{sha256(scope NUL excerpt)[:8]}`.
pub fn derive_evidence_id(scope: &str, excerpt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(scope.as_bytes());
    hasher.update([NUL]);
    hasher.update(excerpt.as_bytes());
    format!("ev-{}", &hex::encode(hasher.finalize())[..8])
}

/// Deterministic failure-episode ID keyed on detector + sorted evidence spans:
/// `fc_{sha256(canonical(detector + evidence))[:24]}`.
pub fn derive_episode_id(detector: &str, evidence_spans: &[(String, i64, i64)]) -> String {
    let mut sorted: Vec<&(String, i64, i64)> = evidence_spans.iter().collect();
    sorted.sort();
    let spans: Vec<Value> = sorted
        .into_iter()
        .map(|(event_id, start, end)| {
            canonical_object([
                ("eventId", Value::String(event_id.clone())),
                ("byteEnd", Value::from(*end)),
                ("byteStart", Value::from(*start)),
            ])
        })
        .collect();
    let payload = canonical_object([
        ("detector", Value::String(detector.to_string())),
        ("evidence", Value::Array(spans)),
    ]);
    format!("fc_{}", &sha256_canonical(&payload)[..24])
}

/// Deterministic Insight issue ID keyed on family + canonical signature:
/// `ii_{sha256(...)}`. Same family + same signature across sessions
/// converges to the same issue ID, which is what makes cross-session
/// recurrence deterministic.
pub fn derive_issue_id(family: &str, signature: &str) -> String {
    let payload = canonical_object([
        ("family", Value::String(family.to_string())),
        ("signature", Value::String(signature.to_string())),
    ]);
    format!("ii_{}", sha256_canonical(&payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_is_key_sorted_and_stable() {
        let a = json!({"b": 1, "a": [2, {"z": true, "y": null}]});
        let b = json!({"a": [2, {"y": null, "z": true}], "b": 1});
        assert_eq!(to_canonical_json(&a), to_canonical_json(&b));
        assert_eq!(
            to_canonical_json(&a),
            r#"{"a":[2,{"y":null,"z":true}],"b":1}"#
        );
    }

    #[test]
    fn digest_is_sha256_of_canonical_form() {
        assert_eq!(sha256_canonical(&json!({"a":1})), sha256_hex(br#"{"a":1}"#));
    }

    #[test]
    fn preference_ids_are_deterministic_and_collision_resistant() {
        let id1 = derive_preference_id("repo-x", "workflow", "Always run focused tests first.");
        let id2 = derive_preference_id("repo-x", "workflow", "always run focused tests first");
        assert_eq!(id1, id2, "normalization must equalize case/punct/spacing");
        let id3 = derive_preference_id("repo-x", "workflow", "Never run focused tests first.");
        assert_ne!(id1, id3);
        // NUL separator prevents tuple-boundary collisions.
        let ab_cd = derive_preference_id("ab", "cd", "rule one two three four five six");
        let a_bcd = derive_preference_id("a", "bcd", "rule one two three four five six");
        assert_ne!(ab_cd, a_bcd);
    }

    #[test]
    fn slug_falls_back_when_no_words() {
        assert_eq!(slug_from_text("123 456 !!!", 4), "preference");
    }

    #[test]
    fn episode_ids_are_order_independent_over_evidence() {
        let e1 = vec![("e2".into(), 0i64, 10i64), ("e1".into(), 5, 9)];
        let e2 = vec![("e1".into(), 5, 9), ("e2".into(), 0, 10)];
        assert_eq!(derive_episode_id("d", &e1), derive_episode_id("d", &e2));
    }
}
