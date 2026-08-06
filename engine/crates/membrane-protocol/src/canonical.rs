//! Canonical JSON serialization — the single cross-language byte contract.
//!
//! Every Membrane golden fixture, and every digest computed over a protocol
//! shape, is defined over the *canonical* serialization produced here. The
//! rules are fixed and deliberately tiny so the Rust and TypeScript bindings
//! agree byte-for-byte:
//!
//!   1. Object keys are sorted lexicographically (by Unicode code point).
//!   2. No insignificant whitespace anywhere.
//!   3. Objects and arrays serialize as `{"k":v,...}` / `[v,...]`.
//!   4. `null` fields are preserved (a field that is present-but-null is NOT
//!      the same as an absent field).
//!   5. Integers serialize bare; the contract fixtures use integers only for
//!      the numeric contract fields, so Rust and JS agree exactly.
//!   6. Strings use JSON escaping; both sides must emit the same escapes for
//!      the ASCII content the contract uses.
//!
//! This mirrors the pre-existing `canonical(value)` helper in
//! `mcp/scope-grant-v1.mjs` so the runtime and the bindings digest identically.
//!
//! The `canonicalize` free function is the reusable primitive; the
//! `CanonicalSerialize` blanket impl in `lib.rs` exposes `.canonical_json()` and
//! `.canonical_digest()` on every protocol type via their `Serialize` impl.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Serialize an arbitrary `serde_json::Value` to canonical JSON.
///
/// Keys are sorted because `serde_json::Value` (without `preserve_order`)
/// stores object entries in a `BTreeMap`, so iteration is already sorted — but
/// we sort defensively so this is correct even if that feature is ever enabled.
pub fn canonicalize(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => serde_json::to_string(s).expect("string serialization is infallible"),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(canonicalize).collect();
            format!("[{}]", inner.join(","))
        }
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let inner: Vec<String> = entries
                .into_iter()
                .map(|(k, v)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).expect("key serialization is infallible"),
                        canonicalize(v)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(","))
        }
    }
}

/// Canonical JSON for any serializable protocol value.
pub fn canonical_json_of<T: Serialize + ?Sized>(value: &T) -> String {
    let value = serde_json::to_value(value).expect("protocol values serialize to JSON");
    canonicalize(&value)
}

/// The canonical content digest: `sha256:<hex>` over the canonical JSON bytes.
pub fn canonical_digest_of<T: Serialize + ?Sized>(value: &T) -> String {
    digest_str(&canonical_json_of(value))
}

/// `sha256:<hex>` of an arbitrary UTF-8 string's bytes.
pub fn digest_str(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sorts_object_keys() {
        let value = json!({ "b": 1, "a": 2, "c": { "y": true, "x": null } });
        assert_eq!(
            canonicalize(&value),
            r#"{"a":2,"b":1,"c":{"x":null,"y":true}}"#
        );
    }

    #[test]
    fn preserves_null_and_arrays() {
        let value = json!({ "list": [3, 1, 2], "maybe": null });
        assert_eq!(canonicalize(&value), r#"{"list":[3,1,2],"maybe":null}"#);
    }

    #[test]
    fn digest_is_stable() {
        // Independently computed: sha256 of the string "abc".
        assert_eq!(
            digest_str("abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
