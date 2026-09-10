//! Native port of `blueprint/src/providers/compilers/scip-normalize.mjs` and
//! `blueprint/src/providers/compilers/python-scip.mjs`.
//!
//! This module is deliberately policy-neutral for the normalizer half: it
//! parses the portable SCIP JSON transport, normalizes occurrence
//! roles/ranges, preserves symbol metadata and relationships, and builds
//! exact symbol-definition identity. It does NOT assign Blueprint
//! confidence, choose graph edge kinds, or perform name matching.
//!
//! The python-scip half is the Python SCIP adapter: it reads only
//! out-of-band portable JSON under repo-read; it never installs, invokes,
//! or guesses. Transport parsing and occurrence-role semantics are shared
//! with every first-party SCIP lane via the normalizer functions in this
//! module (mirroring the legacy JS module split, collapsed into one file
//! here since both are this port's domain).
//!
//! The manifest for the `scip-python` provider
//! (`blueprint/src/providers/manifests/scip-python.json`) is embedded
//! verbatim below via `include_str!` so the Rust port carries the same
//! provider identity/version/integrity metadata as the legacy manifest.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::model::{GraphEdge, GraphNode};
use super::{ProviderContext, ProviderOutput};

/// Embedded copy of `blueprint/src/providers/manifests/scip-python.json`.
pub const SCIP_PYTHON_MANIFEST_JSON: &str =
    include_str!("../../assets/providers/scip-python.json");

pub fn scip_python_manifest() -> Value {
    serde_json::from_str(SCIP_PYTHON_MANIFEST_JSON)
        .expect("embedded scip-python.json manifest must be valid JSON")
}

// ---------------------------------------------------------------------
// scip-normalize.mjs port
// ---------------------------------------------------------------------

pub const ROLE_DEFINITION: u32 = 1;
pub const ROLE_REFERENCE: u32 = 2;
pub const ROLE_READ: u32 = 4;
pub const ROLE_WRITE: u32 = 8;

fn role_bits() -> [(&'static str, u32); 4] {
    [
        ("definition", ROLE_DEFINITION),
        ("reference", ROLE_REFERENCE),
        ("read", ROLE_READ),
        ("write", ROLE_WRITE),
    ]
}

/// Mirrors `normalizeScipRoles`: accepts either an array of role name
/// strings or a numeric SCIP role bitmask, returning a set of lowercase
/// role names.
pub fn normalize_scip_roles(roles: &Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    if let Some(array) = roles.as_array() {
        for role in array {
            let normalized = value_as_string(role).trim().to_lowercase();
            if !normalized.is_empty() {
                names.insert(normalized);
            }
        }
        return names;
    }
    if let Some(value) = roles.as_f64() {
        if value.is_finite() {
            let bits = value as i64;
            for (name, bit) in role_bits() {
                if (bits & bit as i64) == bit as i64 {
                    names.insert(name.to_string());
                }
            }
        }
    }
    names
}

fn value_as_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn normalize_range(range: &Value) -> Option<Vec<f64>> {
    let array = range.as_array()?;
    if array.len() < 2 {
        return None;
    }
    let mut values = Vec::with_capacity(array.len());
    for entry in array {
        let n = entry.as_f64()?;
        if !n.is_finite() {
            return None;
        }
        values.push(n);
    }
    Some(values)
}

#[derive(Debug, Clone)]
pub struct ScipRelationship {
    pub symbol: String,
    pub is_reference: bool,
    pub is_implementation: bool,
    pub is_type_definition: bool,
}

fn normalize_relationships(relationships: Option<&Value>) -> Vec<ScipRelationship> {
    let Some(Value::Array(items)) = relationships else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|relationship| {
            let obj = relationship.as_object()?;
            let symbol = obj.get("symbol")?.as_str()?.to_string();
            if symbol.is_empty() {
                return None;
            }
            Some(ScipRelationship {
                symbol,
                is_reference: bool_field(obj, &["isReference", "is_reference"]),
                is_implementation: bool_field(obj, &["isImplementation", "is_implementation"]),
                is_type_definition: bool_field(obj, &["isTypeDefinition", "is_type_definition"]),
            })
        })
        .collect()
}

fn bool_field(obj: &Map<String, Value>, keys: &[&str]) -> bool {
    for key in keys {
        if let Some(v) = obj.get(*key) {
            return v.as_bool().unwrap_or(!v.is_null() && v != &Value::Bool(false));
        }
    }
    false
}

fn string_field(obj: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(v) = obj.get(*key) {
            if let Some(s) = v.as_str() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn array_field(obj: &Map<String, Value>, keys: &[&str]) -> Vec<Value> {
    for key in keys {
        if let Some(Value::Array(items)) = obj.get(*key) {
            return items.clone();
        }
    }
    Vec::new()
}

#[derive(Debug, Clone)]
pub struct ScipSymbolInformation {
    pub symbol: String,
    pub documentation: Vec<String>,
    pub relationships: Vec<ScipRelationship>,
    pub kind: Option<Value>,
    pub display_name: Option<String>,
    pub signature_documentation: Option<Value>,
    pub enclosing_symbol: Option<String>,
    pub raw: Value,
}

fn normalize_symbol_information(symbols: Option<&Value>) -> Vec<ScipSymbolInformation> {
    let Some(Value::Array(items)) = symbols else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|info| {
            let obj = info.as_object()?;
            let symbol = obj.get("symbol")?.as_str()?.to_string();
            if symbol.is_empty() {
                return None;
            }
            let documentation = array_field(obj, &["documentation"])
                .into_iter()
                .map(|v| value_as_string(&v))
                .collect();
            Some(ScipSymbolInformation {
                symbol,
                documentation,
                relationships: normalize_relationships(obj.get("relationships")),
                kind: obj.get("kind").cloned(),
                display_name: string_field(obj, &["displayName", "display_name"]),
                signature_documentation: obj
                    .get("signatureDocumentation")
                    .or_else(|| obj.get("signature_documentation"))
                    .cloned(),
                enclosing_symbol: string_field(obj, &["enclosingSymbol", "enclosing_symbol"]),
                raw: info.clone(),
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct ScipOccurrence {
    pub symbol: String,
    pub roles: BTreeSet<String>,
    pub range: Vec<f64>,
    pub document_path: String,
    pub override_documentation: Vec<String>,
    pub syntax_kind: Option<Value>,
    pub diagnostics: Vec<Value>,
    pub raw_roles: Option<Value>,
    pub raw: Value,
}

fn normalize_occurrence(occurrence: &Value, document_path: &str) -> Option<ScipOccurrence> {
    let obj = occurrence.as_object()?;
    let symbol = obj.get("symbol")?.as_str()?.to_string();
    if symbol.is_empty() {
        return None;
    }
    let range_value = obj.get("range")?;
    let range = normalize_range(range_value)?;
    let raw_roles = obj
        .get("roles")
        .or_else(|| obj.get("symbolRoles"))
        .or_else(|| obj.get("symbol_roles"))
        .cloned();
    let roles = normalize_scip_roles(raw_roles.as_ref().unwrap_or(&Value::Null));
    if roles.is_empty() {
        return None;
    }
    let override_documentation = array_field(obj, &["overrideDocumentation", "override_documentation"])
        .into_iter()
        .map(|v| value_as_string(&v))
        .collect();
    let diagnostics = array_field(obj, &["diagnostics"]);
    Some(ScipOccurrence {
        symbol,
        roles,
        range,
        document_path: document_path.to_string(),
        override_documentation,
        syntax_kind: obj.get("syntaxKind").or_else(|| obj.get("syntax_kind")).cloned(),
        diagnostics,
        raw_roles,
        raw: occurrence.clone(),
    })
}

#[derive(Debug, Clone)]
pub struct ScipDocument {
    pub path: String,
    pub occurrences: Vec<ScipOccurrence>,
    pub symbols: Vec<ScipSymbolInformation>,
    pub language: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone)]
pub struct ScipIndex {
    pub metadata: Map<String, Value>,
    pub documents: Vec<ScipDocument>,
    pub occurrences: Vec<ScipOccurrence>,
    pub definitions_by_symbol: HashMap<String, usize>, // index into `occurrences`
    pub symbol_information_by_symbol: HashMap<String, ScipSymbolInformation>,
    pub external_symbols: Vec<ScipSymbolInformation>,
    pub skipped_documents: usize,
    pub skipped_occurrences: usize,
    pub index_path: Option<PathBuf>,
}

#[derive(Debug)]
pub struct ScipError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for ScipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for ScipError {}

/// Mirrors `normalizeScipIndex`.
pub fn normalize_scip_index(parsed: &Value, index_path: Option<&Path>) -> Result<ScipIndex, ScipError> {
    let documents_value = parsed
        .as_object()
        .and_then(|obj| obj.get("documents"))
        .and_then(|v| v.as_array());
    let Some(documents_value) = documents_value else {
        let prefix = index_path
            .map(|p| format!("SCIP index at {}", p.display()))
            .unwrap_or_else(|| "SCIP index".to_string());
        return Err(ScipError {
            code: "scip_index_incompatible",
            message: format!("{prefix} has no \"documents\" array — not a recognized portable-SCIP-JSON shape"),
        });
    };

    let mut documents = Vec::new();
    let mut occurrences = Vec::new();
    let mut definitions_by_symbol: HashMap<String, usize> = HashMap::new();
    let mut symbol_information_by_symbol: HashMap<String, ScipSymbolInformation> = HashMap::new();
    let mut skipped_documents = 0usize;
    let mut skipped_occurrences = 0usize;

    for raw_document in documents_value {
        let Some(doc_obj) = raw_document.as_object() else {
            skipped_documents += 1;
            continue;
        };
        let path = string_field(doc_obj, &["relativePath", "relative_path", "path"]).unwrap_or_default();
        if path.is_empty() {
            skipped_documents += 1;
            continue;
        }
        let mut document_occurrences = Vec::new();
        let raw_occurrences = doc_obj.get("occurrences").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        for raw_occurrence in &raw_occurrences {
            match normalize_occurrence(raw_occurrence, &path) {
                Some(occurrence) => {
                    if occurrence.roles.contains("definition")
                        && !definitions_by_symbol.contains_key(&occurrence.symbol)
                    {
                        definitions_by_symbol.insert(occurrence.symbol.clone(), occurrences.len());
                    }
                    document_occurrences.push(occurrence.clone());
                    occurrences.push(occurrence);
                }
                None => skipped_occurrences += 1,
            }
        }
        let symbols_value = doc_obj
            .get("symbols")
            .or_else(|| doc_obj.get("symbolInformation"))
            .or_else(|| doc_obj.get("symbol_information"));
        let symbols = normalize_symbol_information(symbols_value);
        for info in &symbols {
            symbol_information_by_symbol
                .entry(info.symbol.clone())
                .or_insert_with(|| info.clone());
        }
        documents.push(ScipDocument {
            path,
            occurrences: document_occurrences,
            symbols,
            language: doc_obj.get("language").and_then(|v| v.as_str()).map(|s| s.to_string()),
            raw: raw_document.clone(),
        });
    }

    let parsed_obj = parsed.as_object();
    let external_symbols_value = parsed_obj.and_then(|obj| {
        obj.get("externalSymbols")
            .or_else(|| obj.get("external_symbols"))
            .or_else(|| obj.get("symbolInformation"))
            .or_else(|| obj.get("symbol_information"))
    });
    let external_symbols = normalize_symbol_information(external_symbols_value);
    for info in &external_symbols {
        symbol_information_by_symbol
            .entry(info.symbol.clone())
            .or_insert_with(|| info.clone());
    }

    let metadata = parsed_obj
        .and_then(|obj| obj.get("metadata"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    Ok(ScipIndex {
        metadata,
        documents,
        occurrences,
        definitions_by_symbol,
        symbol_information_by_symbol,
        external_symbols,
        skipped_documents,
        skipped_occurrences,
        index_path: index_path.map(|p| p.to_path_buf()),
    })
}

/// Mirrors `readNormalizedScipIndex`.
pub fn read_normalized_scip_index(index_path: &Path) -> Result<ScipIndex, ScipError> {
    let raw = fs::read_to_string(index_path).map_err(|error| ScipError {
        code: "scip_index_unreadable",
        message: format!("SCIP index at {} could not be read/parsed as JSON: {error}", index_path.display()),
    })?;
    let parsed: Value = serde_json::from_str(&raw).map_err(|error| ScipError {
        code: "scip_index_unreadable",
        message: format!("SCIP index at {} could not be read/parsed as JSON: {error}", index_path.display()),
    })?;
    normalize_scip_index(&parsed, Some(index_path))
}

/// Mirrors `scipOccurrenceEvidence`.
pub fn scip_occurrence_evidence(occurrence: &ScipOccurrence) -> Value {
    let start_line = occurrence.range.first().copied().unwrap_or(0.0);
    let start_character = occurrence.range.get(1).copied().unwrap_or(0.0);
    let end_line = occurrence.range.get(2).copied().unwrap_or(start_line);
    let end_character = occurrence.range.get(3).copied().unwrap_or(start_character);
    json!({
        "path": occurrence.document_path,
        "startLine": start_line + 1.0,
        "startCharacter": start_character,
        "endLine": end_line + 1.0,
        "endCharacter": end_character,
        "symbol": occurrence.symbol,
    })
}

// ---------------------------------------------------------------------
// python-scip.mjs port
// ---------------------------------------------------------------------

pub const PROVIDER_ID: &str = "scip-python";
pub const ADAPTER_VERSION: &str = "normalized-portable-index-v2";
pub const SUPPORTED_SCIP_PYTHON_VERSION: &str = "0.6.6";
pub const DEGRADES_TO: &str = "AST";
const PRECISION_TIER_COMPILER: &str = "COMPILER";

fn is_local_symbol(symbol: &str) -> bool {
    symbol == "local" || symbol.starts_with("local ") || symbol.starts_with("local\t")
}

fn is_parameter_symbol(symbol: &str) -> bool {
    // Mirrors /\([A-Za-z_]\w*\)$/
    let bytes = symbol.as_bytes();
    if bytes.last() != Some(&b')') {
        return false;
    }
    let Some(open) = symbol.rfind('(') else { return false };
    let inner = &symbol[open + 1..symbol.len() - 1];
    let mut chars = inner.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn symbol_tail(symbol: &str) -> String {
    symbol.split_whitespace().skip(4).collect::<Vec<_>>().join(" ")
}

fn descriptor_names(symbol: &str) -> Vec<String> {
    let tail = symbol_tail(symbol);
    let chars: Vec<char> = tail.chars().collect();
    let mut names = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            // Lookahead assertion: next char must be one of `#`, `(`, `.`, end-of-string,
            // or the two-char sequence `().` — matches JS `(?=#|\(\)\.|\(|\.|$)`.
            let ok = if i >= chars.len() {
                true
            } else {
                match chars[i] {
                    '#' | '(' | '.' => true,
                    _ => false,
                }
            };
            if ok {
                names.push(chars[start..i].iter().collect());
            } else {
                i = start + 1;
            }
        } else {
            i += 1;
        }
    }
    names
}

fn leaf_name(symbol: &str) -> String {
    let names = descriptor_names(symbol);
    names.last().cloned().unwrap_or_else(|| symbol.to_string())
}

fn symbol_labels(symbol: &str) -> Vec<&'static str> {
    let tail = symbol_tail(symbol);
    if tail.ends_with("().") {
        if tail.contains('#') {
            vec!["Method"]
        } else {
            vec!["Function"]
        }
    } else if tail.ends_with('#') {
        vec!["Class"]
    } else {
        vec!["Symbol"]
    }
}

fn occurrence_evidence(occurrence: &ScipOccurrence) -> Vec<Value> {
    vec![scip_occurrence_evidence(occurrence)]
}

fn definition_node(occurrence: &ScipOccurrence, info: Option<&ScipSymbolInformation>) -> Value {
    let symbol = &occurrence.symbol;
    let names = descriptor_names(symbol);
    let qualified_name = if names.is_empty() { symbol.clone() } else { names.join(".") };
    let mut node = json!({
        "id": format!("symbol:{}::{}", occurrence.document_path, symbol),
        "kind": "symbol",
        "labels": symbol_labels(symbol),
        "name": info.and_then(|i| i.display_name.clone()).unwrap_or_else(|| leaf_name(symbol)),
        "qualifiedName": qualified_name,
        "symbol": symbol,
        "path": occurrence.document_path,
        "precisionTier": "COMPILER",
        "provider": PROVIDER_ID,
        "evidence": occurrence_evidence(occurrence),
    });
    let obj = node.as_object_mut().unwrap();
    if let Some(info) = info {
        if !info.documentation.is_empty() {
            obj.insert("documentation".to_string(), json!(info.documentation));
        }
        if let Some(kind) = &info.kind {
            if !kind.is_null() {
                obj.insert("symbolKind".to_string(), kind.clone());
            }
        }
    }
    node
}

fn reference_edge(
    kind: &str,
    source_id: &str,
    target: Option<&Value>,
    evidence: &[Value],
    reason: Option<String>,
    serial: usize,
) -> Value {
    let resolved = target.and_then(|t| t.get("id")).and_then(|v| v.as_str()).is_some();
    let target_id = if resolved {
        target.and_then(|t| t.get("id")).and_then(|v| v.as_str()).map(|s| s.to_string())
    } else {
        None
    };
    let tier = if resolved { "EXACT_RESOLUTION" } else { "UNRESOLVED" };
    let evidence_symbol = evidence
        .first()
        .and_then(|e| e.get("symbol"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let id = format!(
        "edge:{kind}:{source_id}->{}:scip:{serial}",
        target_id.clone().unwrap_or_else(|| format!("unresolved:{evidence_symbol}"))
    );
    json!({
        "id": id,
        "kind": kind,
        "source": source_id,
        "target": target_id,
        "confidenceTier": tier,
        "provider": PROVIDER_ID,
        "precisionTier": "COMPILER",
        "resolved": resolved,
        "reason": reason,
        "evidence": evidence,
    })
}

fn included_occurrence(occurrence: &ScipOccurrence) -> bool {
    !is_local_symbol(&occurrence.symbol) && !is_parameter_symbol(&occurrence.symbol)
}

pub struct BuiltGraph {
    pub nodes: Vec<Value>,
    pub edges: Vec<Value>,
    pub definition_count: usize,
    pub reference_count: usize,
}

fn build_from_index(index: &ScipIndex) -> BuiltGraph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut definitions_by_symbol: HashMap<String, Value> = HashMap::new();
    let mut definition_count = 0usize;
    let mut reference_count = 0usize;

    for doc in &index.documents {
        let path = &doc.path;
        let file_name = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(path.as_str())
            .to_string();
        nodes.push(json!({
            "id": format!("file:{path}"),
            "kind": "file",
            "labels": ["File"],
            "name": file_name,
            "qualifiedName": path,
            "path": path,
            "precisionTier": "COMPILER",
            "provider": PROVIDER_ID,
            "evidence": [{"path": path, "startLine": 1, "endLine": 1}],
        }));
        for occurrence in &doc.occurrences {
            if !included_occurrence(occurrence) {
                continue;
            }
            if occurrence.roles.contains("definition") {
                definition_count += 1;
                if !definitions_by_symbol.contains_key(&occurrence.symbol) {
                    let info = index.symbol_information_by_symbol.get(&occurrence.symbol);
                    let node = definition_node(occurrence, info);
                    definitions_by_symbol.insert(occurrence.symbol.clone(), node.clone());
                    nodes.push(node);
                }
            }
        }
    }

    let mut serial = 0usize;
    for doc in &index.documents {
        let source_id = format!("file:{}", doc.path);
        for occurrence in &doc.occurrences {
            if !included_occurrence(occurrence) || !occurrence.roles.contains("reference") {
                continue;
            }
            reference_count += 1;
            let target = definitions_by_symbol.get(&occurrence.symbol);
            let evidence = occurrence_evidence(occurrence);
            let reason = if target.is_none() {
                Some(format!(
                    "no definition for symbol \"{}\" in the index; no name-match fallback",
                    occurrence.symbol
                ))
            } else {
                None
            };
            edges.push(reference_edge("REFERENCES", &source_id, target, &evidence, reason, serial));
            serial += 1;
            let target_is_class = target
                .and_then(|t| t.get("labels"))
                .and_then(|v| v.as_array())
                .map(|labels| labels.iter().any(|l| l.as_str() == Some("Class")))
                .unwrap_or(false);
            if target_is_class {
                edges.push(reference_edge("TYPED", &source_id, target, &evidence, None, serial));
                serial += 1;
            }
        }
    }

    BuiltGraph { nodes, edges, definition_count, reference_count }
}

/// Mirrors `findScipIndex` (imported by python-scip.mjs from
/// `../../graph/scip-provider.mjs`).
pub fn find_scip_index(repo_root: &Path, scip_index_path: Option<&str>) -> Option<PathBuf> {
    let root = repo_root.to_path_buf();
    if let Some(explicit) = scip_index_path {
        let candidate = root.join(explicit);
        return if candidate.exists() { Some(candidate) } else { None };
    }
    if let Ok(env_path) = std::env::var("BLUEPRINT_SCIP_INDEX") {
        if !env_path.is_empty() {
            let candidate = root.join(&env_path);
            return if candidate.exists() { Some(candidate) } else { None };
        }
    }
    for candidate in [root.join("index.scip.json"), root.join(".agent").join("index.scip.json")] {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct ScipContext {
    pub repo_root: PathBuf,
    pub scip_index_path: Option<String>,
}

pub enum ProbeState {
    Ok(ProbeOk),
    Partial(ProbeDegraded),
    Unavailable(ProbeDegraded),
}

pub struct ProbeOk {
    pub index_path: PathBuf,
    pub index_version: String,
    pub document_count: usize,
    pub definition_count: usize,
    pub reference_count: usize,
}

pub struct ProbeDegraded {
    pub code: String,
    pub reason: String,
    pub index_path: Option<PathBuf>,
    pub index_version: Option<String>,
    pub skipped_documents: Option<usize>,
    pub skipped_occurrences: Option<usize>,
    pub definition_count: Option<usize>,
    pub reference_count: Option<usize>,
}

fn probe_scip_index(context: &ScipContext) -> ProbeState {
    let explicit = context.scip_index_path.clone();
    let index_path = if let Some(explicit) = explicit.as_deref().filter(|p| Path::new(p).is_absolute()) {
        Some(PathBuf::from(explicit))
    } else {
        find_scip_index(&context.repo_root, explicit.as_deref())
    };

    let Some(index_path) = index_path else {
        return ProbeState::Unavailable(ProbeDegraded {
            code: "scip_index_absent".to_string(),
            reason: "no SCIP index found (set BLUEPRINT_SCIP_INDEX or pass scipIndexPath, or place index.scip.json / .agent/index.scip.json at the repo root)".to_string(),
            index_path: None,
            index_version: None,
            skipped_documents: None,
            skipped_occurrences: None,
            definition_count: None,
            reference_count: None,
        });
    };

    let index = match read_normalized_scip_index(&index_path) {
        Ok(index) => index,
        Err(error) => {
            return ProbeState::Unavailable(ProbeDegraded {
                code: error.code.to_string(),
                reason: error.message,
                index_path: Some(index_path),
                index_version: None,
                skipped_documents: None,
                skipped_occurrences: None,
                definition_count: None,
                reference_count: None,
            });
        }
    };

    let index_version = index
        .metadata
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if !index_version.is_empty() && index_version != SUPPORTED_SCIP_PYTHON_VERSION {
        return ProbeState::Unavailable(ProbeDegraded {
            code: "scip_index_version_incompatible".to_string(),
            reason: format!(
                "SCIP index at {} uses scip-python {}; supported version is {}",
                index_path.display(),
                index_version,
                SUPPORTED_SCIP_PYTHON_VERSION
            ),
            index_path: Some(index_path),
            index_version: Some(index_version),
            skipped_documents: None,
            skipped_occurrences: None,
            definition_count: None,
            reference_count: None,
        });
    }

    let mut definition_count = 0usize;
    let mut reference_count = 0usize;
    for occurrence in &index.occurrences {
        if !included_occurrence(occurrence) {
            continue;
        }
        if occurrence.roles.contains("definition") {
            definition_count += 1;
        }
        if occurrence.roles.contains("reference") {
            reference_count += 1;
        }
    }

    let mut partial_reasons = Vec::new();
    if index.skipped_documents > 0 {
        partial_reasons.push(format!("{} document(s) missing relativePath", index.skipped_documents));
    }
    if index.skipped_occurrences > 0 {
        partial_reasons.push(format!("{} structurally incomplete occurrence(s)", index.skipped_occurrences));
    }
    if definition_count == 0 {
        partial_reasons.push("index declares no definitions".to_string());
    }
    if !partial_reasons.is_empty() {
        return ProbeState::Partial(ProbeDegraded {
            code: "scip_index_partial".to_string(),
            reason: format!(
                "SCIP index at {} is partial: {}. Affected entries are skipped; no edges are fabricated for them.",
                index_path.display(),
                partial_reasons.join("; ")
            ),
            index_path: Some(index_path),
            index_version: Some(index_version),
            skipped_documents: Some(index.skipped_documents),
            skipped_occurrences: Some(index.skipped_occurrences),
            definition_count: Some(definition_count),
            reference_count: Some(reference_count),
        });
    }

    ProbeState::Ok(ProbeOk {
        index_path,
        index_version,
        document_count: index.documents.len(),
        definition_count,
        reference_count,
    })
}

fn degradation_report(degraded: &ProbeDegraded, state_kind: &str) -> Value {
    let mut report = json!({
        "kind": state_kind,
        "code": degraded.code,
        "reason": degraded.reason,
        "degradesTo": DEGRADES_TO,
        "provider": PROVIDER_ID,
        "precisionTier": PRECISION_TIER_COMPILER,
        "indexPath": degraded.index_path.as_ref().map(|p| p.display().to_string()),
    });
    let obj = report.as_object_mut().unwrap();
    if let Some(skipped) = degraded.skipped_documents {
        obj.insert("skippedDocuments".to_string(), json!(skipped));
    }
    if let Some(skipped) = degraded.skipped_occurrences {
        obj.insert("skippedOccurrences".to_string(), json!(skipped));
    }
    report
}

pub struct CollectResult {
    pub nodes: Vec<Value>,
    pub edges: Vec<Value>,
    pub reports: Vec<Value>,
    pub index: Value,
}

/// Mirrors `pythonScipProvider.collect`.
pub fn collect(context: &ScipContext) -> CollectResult {
    match probe_scip_index(context) {
        ProbeState::Unavailable(degraded) => CollectResult {
            nodes: Vec::new(),
            edges: Vec::new(),
            reports: vec![degradation_report(&degraded, "unavailable")],
            index: json!({
                "state": "unavailable",
                "code": degraded.code,
                "reason": degraded.reason,
                "indexPath": degraded.index_path.as_ref().map(|p| p.display().to_string()),
            }),
        },
        ProbeState::Partial(degraded) => {
            let index_path = degraded.index_path.clone().expect("partial probe always has an index path");
            match read_normalized_scip_index(&index_path) {
                Ok(index) => {
                    let built = build_from_index(&index);
                    CollectResult {
                        nodes: built.nodes,
                        edges: built.edges,
                        reports: vec![degradation_report(&degraded, "partial")],
                        index: json!({
                            "provider": PROVIDER_ID,
                            "indexer": index.metadata.get("indexer").and_then(|v| v.as_str()).unwrap_or("scip-python"),
                            "version": degraded.index_version.clone().unwrap_or_default(),
                            "path": index_path.display().to_string(),
                            "documentCount": index.documents.len(),
                            "definitionCount": built.definition_count,
                            "referenceCount": built.reference_count,
                            "state": "partial",
                        }),
                    }
                }
                Err(error) => CollectResult {
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    reports: vec![json!({
                        "kind": "unavailable",
                        "code": error.code,
                        "reason": error.message,
                        "degradesTo": DEGRADES_TO,
                        "provider": PROVIDER_ID,
                        "precisionTier": PRECISION_TIER_COMPILER,
                        "indexPath": index_path.display().to_string(),
                    })],
                    index: json!({"state": "unavailable", "code": error.code, "reason": error.message}),
                },
            }
        }
        ProbeState::Ok(ok) => match read_normalized_scip_index(&ok.index_path) {
            Ok(index) => {
                let built = build_from_index(&index);
                CollectResult {
                    nodes: built.nodes,
                    edges: built.edges,
                    reports: Vec::new(),
                    index: json!({
                        "provider": PROVIDER_ID,
                        "indexer": index.metadata.get("indexer").and_then(|v| v.as_str()).unwrap_or("scip-python"),
                        "version": ok.index_version,
                        "path": ok.index_path.display().to_string(),
                        "documentCount": index.documents.len(),
                        "definitionCount": built.definition_count,
                        "referenceCount": built.reference_count,
                        "state": "ok",
                    }),
                }
            }
            Err(error) => CollectResult {
                nodes: Vec::new(),
                edges: Vec::new(),
                reports: vec![json!({
                    "kind": "unavailable",
                    "code": error.code,
                    "reason": error.message,
                    "degradesTo": DEGRADES_TO,
                    "provider": PROVIDER_ID,
                    "precisionTier": PRECISION_TIER_COMPILER,
                    "indexPath": ok.index_path.display().to_string(),
                })],
                index: json!({"state": "unavailable", "code": error.code, "reason": error.message}),
            },
        },
    }
}

/// Convert rich SCIP facts to the native closed graph shapes used by the
/// registry. Rich compiler metadata remains lossless under evidence; no
/// unknown fields are placed on GraphNode/GraphEdge themselves.
fn closed_node(value: &Value) -> Option<GraphNode> {
    let obj = value.as_object()?;
    let id = obj.get("id")?.as_str()?.to_string();
    let kind = obj.get("kind")?.as_str()?.to_string();
    let path = obj.get("path").and_then(Value::as_str).map(str::to_owned);
    let name = obj.get("name").and_then(Value::as_str).map(str::to_owned);
    let evidence_path = path.clone().unwrap_or_default();
    Some(GraphNode {
        id,
        kind,
        path,
        name,
        generation_id: String::new(),
        evidence: vec![json!({"path": evidence_path, "provider": PROVIDER_ID, "providerVersion": ADAPTER_VERSION, "scipFact": value})],
    })
}

fn closed_edge(value: &Value) -> Option<GraphEdge> {
    let obj = value.as_object()?;
    let id = obj.get("id")?.as_str()?.to_string();
    let kind = obj.get("kind")?.as_str()?.to_string();
    let source = obj.get("source")?.as_str()?.to_string();
    let target = obj.get("target").and_then(Value::as_str).map(str::to_owned);
    let evidence_path = obj.get("evidence")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("path"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(GraphEdge {
        id,
        kind,
        source,
        target,
        generation_id: String::new(),
        evidence: vec![json!({"path": evidence_path, "provider": PROVIDER_ID, "providerVersion": ADAPTER_VERSION, "scipFact": value})],
    })
}

/// Registry entry for SCIP. The rich JSON returned by collect is deliberately
/// kept for the legacy-compatible adapter API; registry output uses only
/// deny-unknown-fields-compatible native graph shapes.
pub fn run(ctx: &ProviderContext<'_>) -> ProviderOutput {
    let result = collect(&ScipContext { repo_root: ctx.repo_root.to_path_buf(), scip_index_path: None });
    ProviderOutput {
        nodes: result.nodes.iter().filter_map(closed_node).collect(),
        edges: result.edges.iter().filter_map(closed_edge).collect(),
    }
}
