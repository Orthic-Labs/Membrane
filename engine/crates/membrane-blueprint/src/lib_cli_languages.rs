//! Native Rust port of the CLI `languages` verb
//! (`blueprint/scripts/cli/commands.mjs:453` case `"languages"`, which
//! forwards to `blueprint/src/graph/language-registry.mjs`'s
//! `languagesJson()`).
//!
//! Lane V3 (r5 closure). `languagesJson()` reads two static, versioned
//! catalog files shipped with the legacy `blueprint` package
//! (`blueprint/grammars/catalog.json`, `blueprint/grammars/manifest.json`)
//! and has no dependency on a repository root, a store, or a published
//! generation — it is pure data. This port embeds the same two files at
//! compile time via `include_str!` (they are shipped alongside the crate
//! source in this monorepo) so the native binary carries an identical,
//! versioned catalog with no runtime filesystem dependency on the legacy
//! `blueprint/` package layout.

use serde::Serialize;
use serde_json::Value;

const CATALOG_JSON: &str = include_str!("../assets/grammars/catalog.json");
const MANIFEST_JSON: &str = include_str!("../assets/grammars/manifest.json");

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LanguageCapabilityRecord {
    pub language: String,
    pub extensions: Vec<String>,
    #[serde(rename = "factProfile")]
    pub fact_profile: String,
    #[serde(rename = "precisionTier")]
    pub precision_tier: String,
    #[serde(rename = "grammarFile")]
    pub grammar_file: Option<String>,
    pub limits: Vec<Value>,
}

/// Mirrors `manifestDigest()`.
pub fn manifest_digest() -> Option<String> {
    let manifest: Value = serde_json::from_str(MANIFEST_JSON).ok()?;
    manifest.get("digest").and_then(Value::as_str).map(str::to_owned)
}

/// Mirrors `languageCapabilityRecords()`.
pub fn language_capability_records() -> Vec<LanguageCapabilityRecord> {
    let catalog: Value = serde_json::from_str(CATALOG_JSON).unwrap_or(Value::Null);
    let grammars = catalog.get("grammars").and_then(Value::as_array).cloned().unwrap_or_default();
    grammars
        .into_iter()
        .map(|grammar| LanguageCapabilityRecord {
            language: grammar.get("language").and_then(Value::as_str).unwrap_or_default().to_string(),
            extensions: grammar
                .get("extensions")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
            fact_profile: grammar.get("factProfile").and_then(Value::as_str).unwrap_or_default().to_string(),
            precision_tier: grammar.get("precisionTier").and_then(Value::as_str).unwrap_or_default().to_string(),
            grammar_file: grammar.get("grammarFile").and_then(Value::as_str).map(str::to_owned),
            limits: grammar.get("limits").and_then(Value::as_array).cloned().unwrap_or_default(),
        })
        .collect()
}

/// Mirrors `languageByExtension(extension)`: known extensions resolve to
/// their catalog record; unknown extensions fall back to a lexical-only
/// pseudo-record with `fallback: true`.
pub fn language_by_extension(extension: &str) -> Value {
    let ext = extension.trim_start_matches('.').to_lowercase();
    let catalog: Value = serde_json::from_str(CATALOG_JSON).unwrap_or(Value::Null);
    let grammars = catalog.get("grammars").and_then(Value::as_array).cloned().unwrap_or_default();
    for grammar in &grammars {
        let extensions = grammar.get("extensions").and_then(Value::as_array).cloned().unwrap_or_default();
        if extensions.iter().any(|e| e.as_str() == Some(ext.as_str())) {
            let mut record = grammar.clone();
            if let Value::Object(map) = &mut record {
                map.insert("fallback".into(), Value::Bool(false));
            }
            return record;
        }
    }
    serde_json::json!({
        "language": ext,
        "extensions": [ext],
        "factProfile": "unknown",
        "precisionTier": "LEXICAL",
        "fallback": true,
    })
}

/// Mirrors `languagesJson()`: the exact CLI `languages --json` payload
/// shape.
pub fn languages_json() -> Value {
    serde_json::json!({
        "schemaVersion": 1,
        "digest": manifest_digest(),
        "languages": language_capability_records(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_json_has_schema_version_one_and_a_digest() {
        let payload = languages_json();
        assert_eq!(payload["schemaVersion"], 1);
        assert!(payload["digest"].is_string());
        assert!(payload["languages"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn known_extension_resolves_without_fallback() {
        let record = language_by_extension("rs");
        // Whichever grammar owns the "rs" extension, it must not be marked
        // as a lexical fallback.
        if record["fallback"] == Value::Bool(true) {
            panic!("expected a known extension to resolve to a catalog record, got fallback: {record}");
        }
    }

    #[test]
    fn unknown_extension_falls_back_to_lexical() {
        let record = language_by_extension("zzz-not-a-real-extension");
        assert_eq!(record["fallback"], Value::Bool(true));
        assert_eq!(record["precisionTier"], "LEXICAL");
        assert_eq!(record["language"], "zzz-not-a-real-extension");
    }

    #[test]
    fn language_by_extension_strips_leading_dot_and_lowercases() {
        let a = language_by_extension(".RS");
        let b = language_by_extension("rs");
        assert_eq!(a["language"], b["language"]);
    }
}
