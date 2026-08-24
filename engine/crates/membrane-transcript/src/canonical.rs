//! Canonical digests, stable ids, and Python-compatible canonical JSON.
//!
//! The retired Python normalizer seeded ids with `json.dumps(..., sort_keys=True,
//! ensure_ascii=False)` (default separators `", "` / `": "`). To keep event ids
//! and fingerprints byte-identical across the port, [`py_json_dumps`] reproduces
//! exactly that serialization for the value shapes used in seeds/fingerprints
//! (null, bool, integer, string, array, object with sorted keys).

use serde_json::Value;
use sha2::{Digest, Sha256};

/// SHA-256 hex digest of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Serialize a JSON value the way Python's `json.dumps(value, sort_keys=True,
/// ensure_ascii=False)` does (default separators, sorted object keys).
///
/// `serde_json::Map` is a `BTreeMap` here (no `preserve_order`), so key order
/// is already sorted; string escaping matches Python's for all characters
/// outside raw non-ASCII output.
pub fn py_json_dumps(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => py_json_string(s),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(py_json_dumps).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Object(map) => {
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", py_json_string(k), py_json_dumps(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
    }
}

fn py_json_string(s: &str) -> String {
    // serde_json string serialization escapes ", \, and control chars exactly
    // like Python with ensure_ascii=False (which leaves non-ASCII raw).
    serde_json::to_string(s).expect("string serialization cannot fail")
}

/// Source files hashed into the parser implementation digest. Content-only and
/// order-stable: identical trees produce identical digests.
const PARSER_SOURCES: &[(&str, &[u8])] = &[
    ("adapters.rs", include_bytes!("adapters.rs")),
    ("canonical.rs", include_bytes!("canonical.rs")),
    ("classify.rs", include_bytes!("classify.rs")),
    ("error.rs", include_bytes!("error.rs")),
    ("event.rs", include_bytes!("event.rs")),
    ("evidence.rs", include_bytes!("evidence.rs")),
    ("lib.rs", include_bytes!("lib.rs")),
    ("parser.rs", include_bytes!("parser.rs")),
    ("redact.rs", include_bytes!("redact.rs")),
    ("source.rs", include_bytes!("source.rs")),
];

/// sha256 over the parser implementation bytes (`name \0 bytes \0` per file).
/// A parser change is detectable from the digest alone.
pub fn parser_digest() -> String {
    let mut digest = Sha256::new();
    for (name, bytes) in PARSER_SOURCES {
        digest.update(name.as_bytes());
        digest.update([0u8]);
        digest.update(bytes);
        digest.update([0u8]);
    }
    hex::encode(digest.finalize())
}

/// Deterministic payload fingerprint seed (Python-compatible canonical JSON).
pub fn fingerprint_payload(
    kind: &str,
    tool: Option<&str>,
    call_id: Option<&str>,
    occurrence: Option<u64>,
    text: &str,
    timestamp: Option<&str>,
) -> String {
    let v = serde_json::json!({
        "kind": kind,
        "tool": tool,
        "call_id": call_id,
        "occurrence": occurrence,
        "text": text,
        "timestamp": timestamp,
    });
    py_json_dumps(&v)
}

/// Deterministic event id: `"evt_" + sha256(seed)[:32]`.
pub fn event_id(
    host: &str,
    session_id: &str,
    row_index: u64,
    block_index: usize,
    sequence: u64,
    kind: &str,
    call_id: Option<&str>,
    fingerprint_seed: &str,
) -> String {
    let seed_value = serde_json::json!({
        "host": host,
        "sessionId": session_id,
        "rowIndex": row_index,
        "blockIndex": block_index,
        "sequence": sequence,
        "kind": kind,
        "callId": call_id,
        "payloadFingerprint": sha256_hex(fingerprint_seed.as_bytes()),
    });
    let seed = py_json_dumps(&seed_value);
    "evt_".to_string() + &sha256_hex(seed.as_bytes())[..32]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn py_dumps_matches_python_sort_keys_default_separators() {
        let v = json!({"b": 1, "a": "x", "c": null, "d": true, "e": [1, "y"]});
        assert_eq!(
            py_json_dumps(&v),
            r#"{"a": "x", "b": 1, "c": null, "d": true, "e": [1, "y"]}"#
        );
    }

    #[test]
    fn py_dumps_escapes_like_python() {
        let v = json!("line\n\"quoted\" \\ tab\t");
        assert_eq!(py_json_dumps(&v), "\"line\\n\\\"quoted\\\" \\\\ tab\\t\"");
    }

    #[test]
    fn non_ascii_stays_raw() {
        assert_eq!(py_json_dumps(&json!("héllo")), "\"héllo\"");
    }

    #[test]
    fn event_id_is_deterministic_and_prefixed() {
        let fp = fingerprint_payload("tool_call", Some("edit"), Some("t1"), Some(0), "body", None);
        let a = event_id("pi", "s", 1, 0, 1, "tool_call", Some("t1"), &fp);
        let b = event_id("pi", "s", 1, 0, 1, "tool_call", Some("t1"), &fp);
        assert_eq!(a, b);
        assert!(a.starts_with("evt_"));
        assert_eq!(a.len(), "evt_".len() + 32);
        let c = event_id("pi", "s", 1, 0, 2, "tool_call", Some("t1"), &fp);
        assert_ne!(a, c);
    }
}
