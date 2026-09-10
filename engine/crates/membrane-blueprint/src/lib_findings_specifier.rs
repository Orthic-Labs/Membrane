//! Native port of `blueprint/src/lib/findings/specifier.mjs`, which is
//! itself a thin re-export wrapper around
//! `blueprint/src/graph/resolution/index.mjs` (the exact-first resolution
//! owner). This module ports that owner's logic faithfully, since the
//! re-export itself carries no behavior of its own.
//!
//! Closed-surface rule: a negative finding (BP001/BP002) is emitted ONLY
//! when the target module's export surface is provably CLOSED. Every other
//! case -- unsupported extension, ambiguous candidates, partial parse,
//! dynamic import, generated file, or external/bare specifier -- collapses
//! to a typed omission `{code:"resolution_unsupported", detail, reason}`
//! rather than a finding.
//!
//! KNOWN GAP (documented, not silently fixed): `findings.rs`'s own inline
//! `resolve_candidates` helper does NOT call this module -- it implements a
//! simpler, independent candidate list that does not strip/rewrite an
//! explicit `.js`/`.jsx`/`.mjs`/`.cjs` extension the way `candidate_paths`
//! here does (see that function's `stem` computation). That makes
//! `findings.rs`'s BP002 resolution diverge from the legacy detector's
//! specifier resolution in that one case. Recorded in
//! audit/qualification/windows-r5/ncl-02/lib-inventory.json for both
//! findings/detect.mjs and findings/specifier.mjs.

use serde_json::Value;
use std::collections::BTreeSet;

const SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];
const ASSET_EXTENSIONS: &[&str] = &["json", "vue", "astro"];

pub const RESOLUTION_OMISSION_CODE: &str = "resolution_unsupported";

/// Mirrors `SUPPORTED_RESOLUTION_EXTENSIONS` (source + asset extensions, sorted).
pub fn supported_resolution_extensions() -> Vec<&'static str> {
    let mut all: Vec<&'static str> = SOURCE_EXTENSIONS.iter().chain(ASSET_EXTENSIONS.iter()).copied().collect();
    all.sort_unstable();
    all
}

/// Mirrors `isRelativeSpecifier(specifier)`.
pub fn is_relative_specifier(specifier: &str) -> bool {
    specifier.starts_with('.')
}

/// Mirrors `normalizeRepoPath(value)`.
pub fn normalize_repo_path(value: &str) -> String {
    value.replace('\\', "/")
}

/// Mirrors `candidatePaths(fromPath, specifier)`: ordered resolution
/// candidates for a relative specifier -- exact path first, then the
/// TypeScript `.js -> source` rewrite plus extension completion, then
/// directory index files. Order and de-duplication (first occurrence wins,
/// mirroring `[...new Set(candidates)]`) match the legacy implementation.
pub fn candidate_paths(from_path: &str, specifier: &str) -> Vec<String> {
    let normalized_from = normalize_repo_path(from_path);
    let mut base_dir: Vec<&str> = normalized_from.split('/').collect();
    base_dir.pop(); // drop the file segment, keep the directory segments
    let raw: Vec<&str> = base_dir.into_iter().chain(specifier.split('/')).filter(|part| !part.is_empty() && *part != ".").collect();
    let mut parts: Vec<&str> = Vec::new();
    for part in raw {
        if part == ".." {
            parts.pop();
        } else {
            parts.push(part);
        }
    }
    let exact = parts.join("/");
    let stem = strip_js_extension(&exact);

    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    let push = |candidate: String, candidates: &mut Vec<String>, seen: &mut BTreeSet<String>| {
        if seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    };
    push(exact.clone(), &mut candidates, &mut seen);
    for extension in SOURCE_EXTENSIONS.iter().chain(ASSET_EXTENSIONS.iter()) {
        push(format!("{stem}.{extension}"), &mut candidates, &mut seen);
    }
    for extension in SOURCE_EXTENSIONS {
        push(format!("{stem}/index.{extension}"), &mut candidates, &mut seen);
    }
    candidates
}

fn strip_js_extension(path: &str) -> String {
    for suffix in [".js", ".jsx", ".mjs", ".cjs"] {
        if let Some(stripped) = path.strip_suffix(suffix) {
            return stripped.to_owned();
        }
    }
    path.to_owned()
}

#[derive(Debug, Clone)]
pub struct ResolveResult {
    pub resolved: Option<String>,
    pub candidates: Vec<String>,
    pub alternatives: usize,
}

/// Mirrors `resolveSpecifier(fromPath, specifier, fileSet)`.
pub fn resolve_specifier(from_path: &str, specifier: &str, file_set: &BTreeSet<String>) -> ResolveResult {
    let candidates = candidate_paths(from_path, specifier);
    let matches: Vec<&String> = candidates.iter().filter(|candidate| file_set.contains(candidate.as_str())).collect();
    let resolved = matches.first().map(|s| (*s).clone());
    let alternatives = matches.len().saturating_sub(1);
    ResolveResult { resolved, candidates, alternatives }
}

#[derive(Debug, Clone, Default)]
pub struct OmissionInput<'a> {
    pub detail: Option<&'a str>,
    pub reason: Option<&'a str>,
    pub path: Option<&'a str>,
    pub specifier: Option<&'a str>,
    pub line: Option<f64>,
}

/// Mirrors `resolutionUnsupportedOmission(input)`.
pub fn resolution_unsupported_omission(input: &OmissionInput) -> Value {
    serde_json::json!({
        "code": RESOLUTION_OMISSION_CODE,
        "detail": input.detail,
        "reason": input.reason,
        "path": input.path,
        "specifier": input.specifier,
        "line": input.line,
    })
}

#[derive(Debug, Clone, Default)]
pub struct ClassifyInput<'a> {
    pub specifier: Option<&'a str>,
    pub target_surface_open_reasons: Option<Vec<String>>,
    pub parse_status: Option<&'a str>,
    pub is_generated: bool,
}

/// Mirrors `classifyResolution(input)`. Returns `None` when the surface is
/// closed (caller may emit a finding); otherwise a typed omission `Value`.
pub fn classify_resolution(input: &ClassifyInput) -> Option<Value> {
    if input.is_generated {
        return Some(resolution_unsupported_omission(&OmissionInput { detail: Some("generated_source"), reason: Some("generated"), specifier: input.specifier, ..Default::default() }));
    }
    if let Some(specifier) = input.specifier {
        if !is_relative_specifier(specifier) {
            return Some(resolution_unsupported_omission(&OmissionInput { detail: Some("bare_specifier"), reason: Some("external"), specifier: input.specifier, ..Default::default() }));
        }
    }
    if matches!(input.parse_status, Some("partial") | Some("failed")) {
        return Some(resolution_unsupported_omission(&OmissionInput { detail: input.parse_status, reason: Some("partial"), specifier: input.specifier, ..Default::default() }));
    }
    if let Some(reasons) = &input.target_surface_open_reasons {
        if !reasons.is_empty() {
            let joined = reasons.join(",");
            return Some(resolution_unsupported_omission(&OmissionInput { detail: Some(&joined), reason: Some("partial"), specifier: input.specifier, ..Default::default() }));
        }
    }
    if let Some(specifier) = input.specifier {
        if let Some(dot) = specifier.rfind('.') {
            let ext = specifier[dot + 1..].to_ascii_lowercase();
            let is_extension_like = ext.chars().all(|c| c.is_ascii_alphanumeric()) && !ext.is_empty();
            if is_extension_like {
                let supported = supported_resolution_extensions();
                if !supported.contains(&ext.as_str()) && !matches!(ext.as_str(), "js" | "jsx" | "mjs" | "cjs") {
                    let detail = format!("unsupported_extension:{ext}");
                    return Some(resolution_unsupported_omission(&OmissionInput { detail: Some(&detail), reason: Some("unsupported"), specifier: input.specifier, ..Default::default() }));
                }
            }
        }
    }
    None
}
