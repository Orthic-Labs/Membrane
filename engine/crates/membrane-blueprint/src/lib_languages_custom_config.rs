//! Native Rust port of `blueprint/src/lib/languages/custom-config.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `loadCustomLanguages`/`CUSTOM_CONFIG_VERSION`/builtin-language-registry
//! helpers across membrane-blueprint/src and membrane-runtime/src produced
//! no match). Ported behavior: parse the pinned `blueprint.languages.toml`
//! subset, reject absolute/parent-traversing/URL grammar paths and
//! collisions with built-in language ids/extensions, verify the grammar
//! hash when declared. The built-in id/extension sets are supplied by the
//! caller (`builtin_ids`/`builtin_extensions`) since this crate has no
//! existing native language-capability-registry export to source them from.

use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const CUSTOM_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct CustomLanguage {
    pub id: String,
    pub extensions: Vec<String>,
    pub grammar: String,
    pub grammar_sha256: String,
    pub fact_profile: String,
    pub precision_tier: String,
    pub table: Option<String>,
    pub version: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomLanguagesResult {
    pub version: u32,
    pub languages: Vec<CustomLanguage>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RawLanguageEntry {
    pub id: Option<String>,
    pub extensions: Vec<String>,
    pub grammar: Option<String>,
    pub grammar_sha256: Option<String>,
    pub fact_profile: Option<String>,
    pub precision_tier: Option<String>,
    pub table: Option<String>,
}

/// Minimal deterministic TOML subset for `[[languages]]` array-of-tables:
/// string/array/bool/number scalars. Mirrors the legacy `parseToml`.
pub fn parse_toml_languages(source: &str) -> Vec<RawLanguageEntry> {
    let mut entries: Vec<RawLanguageEntry> = Vec::new();
    let mut in_languages_table = false;
    for raw_line in source.split(['\n']) {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("[[") || line.starts_with('[') {
            let key = line.trim_start_matches('[').trim_end_matches(']');
            in_languages_table = key == "languages";
            if in_languages_table {
                entries.push(RawLanguageEntry::default());
            }
            continue;
        }
        if !in_languages_table {
            continue;
        }
        let Some(idx) = line.find('=') else { continue };
        let key = line[..idx].trim();
        let raw_value = line[idx + 1..].trim();
        let Some(entry) = entries.last_mut() else { continue };
        match key {
            "id" => entry.id = Some(strip_quotes(raw_value)),
            "extensions" => entry.extensions = parse_array(raw_value),
            "grammar" => entry.grammar = Some(strip_quotes(raw_value)),
            "grammar_sha256" => entry.grammar_sha256 = Some(strip_quotes(raw_value)),
            "fact_profile" => entry.fact_profile = Some(strip_quotes(raw_value)),
            "precision_tier" => entry.precision_tier = Some(strip_quotes(raw_value)),
            "table" => entry.table = Some(strip_quotes(raw_value)),
            _ => {}
        }
    }
    entries
}

fn strip_quotes(value: &str) -> String {
    if value.starts_with('"') {
        if let Some(end) = value.rfind('"') {
            if end > 0 {
                return value[1..end].to_string();
            }
        }
    }
    value.to_string()
}

fn parse_array(value: &str) -> Vec<String> {
    let trimmed = value.trim_start_matches('[').trim_end_matches(']');
    trimmed
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Mirrors `loadCustomLanguages({ root, configPath })`, with the built-in
/// id/extension sets supplied by the caller.
pub fn load_custom_languages(
    root: &Path,
    config_path: &str,
    builtin_ids: &HashSet<String>,
    builtin_extensions: &HashSet<String>,
) -> CustomLanguagesResult {
    let full_path: PathBuf = root.join(config_path);
    if !full_path.exists() {
        return CustomLanguagesResult {
            version: CUSTOM_CONFIG_VERSION,
            languages: Vec::new(),
            errors: Vec::new(),
        };
    }
    let source = match fs::read_to_string(&full_path) {
        Ok(s) => s,
        Err(error) => {
            return CustomLanguagesResult {
                version: CUSTOM_CONFIG_VERSION,
                languages: Vec::new(),
                errors: vec![format!("{config_path} unparseable: {error}")],
            };
        }
    };
    let entries = parse_toml_languages(&source);
    let mut languages = Vec::new();
    let mut errors = Vec::new();
    let canonical_root = match fs::canonicalize(root) {
        Ok(p) => p,
        Err(_) => root.to_path_buf(),
    };
    for entry in entries {
        let id = entry.id.clone().unwrap_or_default();
        let extensions = entry.extensions.clone();
        let grammar = entry.grammar.clone().unwrap_or_default();
        if id.is_empty() {
            errors.push("custom language missing id".to_string());
            continue;
        }
        if builtin_ids.contains(&id) {
            errors.push(format!("language_route_conflict: {id} is a built-in id"));
            continue;
        }
        if extensions.iter().any(|ext| builtin_extensions.contains(ext)) {
            errors.push("language_route_conflict: extension in use by a built-in".to_string());
            continue;
        }
        if Path::new(&grammar).is_absolute()
            || grammar.contains("..")
            || grammar.starts_with("http://")
            || grammar.starts_with("https://")
        {
            errors.push(format!("path_conflict: {grammar} must be repo-confined"));
            continue;
        }
        let grammar_path = root.join(&grammar);
        let canonical_grammar = match fs::canonicalize(&grammar_path) {
            Ok(p) => p,
            Err(_) => {
                errors.push(format!("grammar_missing: {grammar}"));
                continue;
            }
        };
        if !canonical_grammar.starts_with(&canonical_root) {
            errors.push(format!("path_escape: {grammar}"));
            continue;
        }
        let bytes = match fs::read(&grammar_path) {
            Ok(b) => b,
            Err(_) => {
                errors.push(format!("grammar_missing: {grammar}"));
                continue;
            }
        };
        let digest = sha256_hex(&bytes);
        if let Some(expected) = &entry.grammar_sha256 {
            if !expected.is_empty() && *expected != digest {
                errors.push(format!("hash_mismatch: {grammar}"));
                continue;
            }
        }
        languages.push(CustomLanguage {
            id,
            extensions,
            grammar,
            grammar_sha256: digest,
            fact_profile: entry.fact_profile.unwrap_or_else(|| "code".to_string()),
            precision_tier: entry.precision_tier.unwrap_or_else(|| "AST".to_string()),
            table: entry.table,
            version: CUSTOM_CONFIG_VERSION,
        });
    }
    CustomLanguagesResult {
        version: CUSTOM_CONFIG_VERSION,
        languages,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &Path, rel: &str, content: &[u8]) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn missing_config_file_yields_empty_result() {
        let dir = tempfile::tempdir().unwrap();
        let result = load_custom_languages(
            dir.path(),
            "blueprint.languages.toml",
            &HashSet::new(),
            &HashSet::new(),
        );
        assert_eq!(result.version, CUSTOM_CONFIG_VERSION);
        assert!(result.languages.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn loads_a_valid_custom_language() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "grammars/foo.wasm", b"wasm-bytes");
        let toml = r#"
[[languages]]
id = "foolang"
extensions = ["foo", "fooo"]
grammar = "grammars/foo.wasm"
fact_profile = "code"
precision_tier = "AST"
"#;
        write(dir.path(), "blueprint.languages.toml", toml.as_bytes());
        let result = load_custom_languages(
            dir.path(),
            "blueprint.languages.toml",
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.languages.len(), 1);
        assert_eq!(result.languages[0].id, "foolang");
        assert_eq!(result.languages[0].extensions, vec!["foo", "fooo"]);
    }

    #[test]
    fn rejects_builtin_id_collision() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "grammars/foo.wasm", b"wasm-bytes");
        let toml = r#"
[[languages]]
id = "rust"
extensions = ["foo"]
grammar = "grammars/foo.wasm"
"#;
        write(dir.path(), "blueprint.languages.toml", toml.as_bytes());
        let builtins: HashSet<String> = ["rust".to_string()].into_iter().collect();
        let result = load_custom_languages(
            dir.path(),
            "blueprint.languages.toml",
            &builtins,
            &HashSet::new(),
        );
        assert!(result.languages.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("language_route_conflict"));
    }

    #[test]
    fn rejects_absolute_and_parent_traversal_grammar_paths() {
        let dir = tempfile::tempdir().unwrap();
        let toml = r#"
[[languages]]
id = "escapee"
extensions = ["esc"]
grammar = "../outside.wasm"
"#;
        write(dir.path(), "blueprint.languages.toml", toml.as_bytes());
        let result = load_custom_languages(
            dir.path(),
            "blueprint.languages.toml",
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(result.languages.is_empty());
        assert!(result.errors[0].starts_with("path_conflict"));
    }

    #[test]
    fn rejects_grammar_hash_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "grammars/foo.wasm", b"wasm-bytes");
        let toml = r#"
[[languages]]
id = "foolang"
extensions = ["foo"]
grammar = "grammars/foo.wasm"
grammar_sha256 = "deadbeef"
"#;
        write(dir.path(), "blueprint.languages.toml", toml.as_bytes());
        let result = load_custom_languages(
            dir.path(),
            "blueprint.languages.toml",
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(result.languages.is_empty());
        assert!(result.errors[0].starts_with("hash_mismatch"));
    }
}
