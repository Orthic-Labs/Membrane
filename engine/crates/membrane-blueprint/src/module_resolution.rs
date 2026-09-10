//! Native Rust port of the identity-resolution logic from
//! `blueprint/src/providers/build.mjs`, `.../modules/javascript.mjs`, and
//! `.../modules/python-resolver.mjs`.
//!
//! Discipline ported from the legacy source and preserved exactly:
//! - **Exact-first**: a deterministic/exact match always wins over a
//!   heuristic one; heuristics are never consulted once an exact match
//!   exists.
//! - **Same-tier ambiguity stops resolution**: when two candidates tie at
//!   the same confidence tier, resolution does not silently pick one — it
//!   surfaces as `Ambiguous` with the tied candidate list, mirroring the
//!   tier-rank discipline in `recall_circuit.rs`.
//! - **Typed miss/unsupported reasons**: any non-`Resolved` outcome carries
//!   a typed reason (an enum here, not a silent `None`/`null`), mirroring
//!   the `miss(reason)` pattern in `python-resolver.mjs`.
//!
//! Python resolution (`resolve_python_module`) is a complete, field-for-
//! field port of `python-resolver.mjs`, including the full stdlib set and
//! the exact precedence: relative (dot-prefixed) specifiers first, then
//! stdlib exclusion, then configured-root package/module resolution with
//! `__init__.py` re-export following (`namedTarget`).
//!
//! JavaScript/TypeScript resolution (`resolve_js_module`) ports the core of
//! `javascript.mjs`: relative-specifier resolution (exact / extension /
//! ambiguous-extension / index / ambiguous-index), absolute-path handling,
//! and bare-specifier handling. Scope note: the legacy resolver's
//! `package.json` `exports`/`imports` maps, `tsconfig.json`/`jsconfig.json`
//! path-alias resolution, and workspace-package resolution are NOT ported
//! here (they require multi-file JSON-with-comments parsing well beyond
//! this lane's budget) — a bare (non-relative, non-absolute) specifier
//! resolves to `JsResolution::Unsupported("bare_specifier_resolution_not_ported")`
//! rather than silently guessing, so the typed-miss discipline still holds.
//! This is recorded as a finding in the lane receipt.
//!
//! Cross-file identity resolution (exact-first / same-tier-ambiguity-stops)
//! is exposed generically via [`resolve_exact_first`], operating over the
//! same [`crate::confidence_tiers`] tier vocabulary already used by
//! `recall_circuit.rs`, so both modules share one tier-rank definition of
//! "specificity."

use crate::confidence_tiers::EDGE_CONFIDENCE_TIER_ORDER;
use crate::graph::FileRecord;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------
// Exact-first cross-file identity resolution (build.mjs discipline)
// ---------------------------------------------------------------------

/// One candidate binding for a symbol/import, tagged with the confidence
/// tier it was resolved at.
#[derive(Debug, Clone)]
pub struct IdentityCandidate {
    pub target_id: String,
    pub confidence_tier: String,
}

/// Outcome of exact-first cross-file identity resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum IdentityResolution {
    /// A single best-tier candidate, with no other candidate tying it.
    Resolved { target_id: String, confidence_tier: String },
    /// Two or more candidates tied at the best (lowest-rank) tier present.
    /// Resolution STOPS rather than silently picking one.
    Ambiguous { confidence_tier: String, candidates: Vec<String> },
    /// No candidates at all.
    Unresolved { reason: &'static str },
}

fn tier_rank(tier: &str) -> i64 {
    EDGE_CONFIDENCE_TIER_ORDER
        .iter()
        .position(|t| *t == tier)
        .map(|i| i as i64)
        .unwrap_or(EDGE_CONFIDENCE_TIER_ORDER.len() as i64)
}

/// Exact-first resolution: pick the candidate(s) at the best (lowest-rank,
/// i.e. most certain) tier present. If exactly one candidate occupies that
/// tier, it resolves. If two or more tie at that tier, resolution stops and
/// reports `Ambiguous` — it never falls through to a lower tier or picks
/// arbitrarily among ties, and it never lets a worse tier "outvote" a
/// better one by count.
pub fn resolve_exact_first(candidates: &[IdentityCandidate]) -> IdentityResolution {
    if candidates.is_empty() {
        return IdentityResolution::Unresolved { reason: "no_candidates" };
    }
    let best_rank = candidates.iter().map(|c| tier_rank(&c.confidence_tier)).min().unwrap();
    let best: Vec<&IdentityCandidate> =
        candidates.iter().filter(|c| tier_rank(&c.confidence_tier) == best_rank).collect();
    if best.len() == 1 {
        IdentityResolution::Resolved {
            target_id: best[0].target_id.clone(),
            confidence_tier: best[0].confidence_tier.clone(),
        }
    } else {
        let mut ids: Vec<String> = best.iter().map(|c| c.target_id.clone()).collect();
        ids.sort();
        ids.dedup();
        IdentityResolution::Ambiguous { confidence_tier: best[0].confidence_tier.clone(), candidates: ids }
    }
}

// ---------------------------------------------------------------------
// python-resolver.mjs — complete field-for-field port
// ---------------------------------------------------------------------

pub const RESOLVED: &str = "RESOLVED";
pub const UNRESOLVED: &str = "UNRESOLVED";

/// Full CPython 3 standard-library module set, ported verbatim from
/// `python-resolver.mjs`'s `STDLIB` set (not abbreviated).
pub const PY_STDLIB: &[&str] = &[
    "__future__", "abc", "argparse", "array", "ast", "asyncio", "base64", "binascii", "bisect", "builtins",
    "bz2", "calendar", "cmath", "collections", "concurrent", "configparser", "contextlib", "contextvars",
    "copy", "csv", "ctypes", "dataclasses", "datetime", "decimal", "difflib", "dis", "email", "encodings",
    "enum", "errno", "functools", "gc", "getopt", "getpass", "gettext", "glob", "graphlib", "gzip", "hashlib",
    "heapq", "hmac", "html", "http", "importlib", "inspect", "io", "ipaddress", "itertools", "json", "keyword",
    "linecache", "locale", "logging", "lzma", "math", "mimetypes", "mmap", "multiprocessing", "operator", "os",
    "pathlib", "pdb", "pickle", "pkgutil", "platform", "pprint", "queue", "random", "re", "secrets", "select",
    "selectors", "shelve", "shlex", "shutil", "signal", "site", "socket", "sqlite3", "ssl", "stat", "statistics",
    "string", "struct", "subprocess", "sys", "sysconfig", "tempfile", "textwrap", "threading", "time", "timeit",
    "tkinter", "token", "tokenize", "tomllib", "trace", "traceback", "types", "typing", "unicodedata", "unittest",
    "urllib", "uuid", "venv", "warnings", "weakref", "webbrowser", "winreg", "xml", "zipfile", "zlib", "zoneinfo",
];

fn py_stdlib_has(name: &str) -> bool {
    PY_STDLIB.contains(&name)
}

#[derive(Debug, Clone, PartialEq)]
pub struct PyMiss {
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PyHit {
    pub resolved: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PyResolution {
    Resolved(PyHit),
    Unresolved(PyMiss),
}

fn miss(reason: impl Into<String>) -> PyResolution {
    PyResolution::Unresolved(PyMiss { reason: reason.into() })
}
fn hit(resolved: PathBuf, reason: impl Into<String>) -> PyResolution {
    PyResolution::Resolved(PyHit { resolved, reason: reason.into() })
}

fn read_to_string_opt(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Mirrors `inside(root, target)`: `target` is `root` itself or strictly
/// beneath it.
fn inside(root: &Path, target: &Path) -> bool {
    match target.strip_prefix(root) {
        Ok(_) => true,
        Err(_) => false,
    }
}

#[derive(Debug, Clone)]
pub struct PySpecifier {
    pub specifier: String,
    pub imported_name: Option<String>,
    pub line: usize,
}

/// Mirrors `extractPythonModuleSpecifiers`.
pub fn extract_python_module_specifiers(text: &str) -> Vec<PySpecifier> {
    let mut found = Vec::new();
    for (index, line) in text.split(['\n']).enumerate() {
        let line = line.trim_end_matches('\r');
        if let Some(caps) = FROM_IMPORT_RE.captures(line) {
            found.push(PySpecifier {
                specifier: caps[1].to_string(),
                imported_name: Some(caps[2].to_string()),
                line: index + 1,
            });
            continue;
        }
        if let Some(caps) = IMPORT_RE.captures(line) {
            found.push(PySpecifier { specifier: caps[1].to_string(), imported_name: None, line: index + 1 });
        }
    }
    found
}

use regex::Regex;
use std::sync::LazyLock;

static FROM_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*from\s+([.A-Za-z_]\w*(?:\.\w+)*)\s+import\s+([A-Za-z_]\w*)").unwrap());
static IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*import\s+([A-Za-z_]\w*(?:\.\w+)*)").unwrap());
static PYPROJECT_PACKAGE_DIR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"package-dir\s*=\s*\{[^}]*["']?["']\s*=\s*["']([^"']+)["']"#).unwrap());
static PYPROJECT_FIND_SECTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\[tool\.setuptools\.packages\.find\](.*?)(?:\n\[|$)").unwrap());
static WHERE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bwhere\s*=\s*\[?\s*["']([^"']+)["']"#).unwrap());
static SETUPCFG_OPTIONS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\[options\](.*?)(?:\n\[|$)").unwrap());
static SETUPCFG_PACKAGE_DIR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"package_dir\s*=\s*(?:\r?\n\s*)?=\s*([^\s#;]+)").unwrap());
static SETUPCFG_FIND_SECTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\[options\.packages\.find\](.*?)(?:\n\[|$)").unwrap());
static SETUPCFG_WHERE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bwhere\s*=\s*([^\s#;]+)").unwrap());
static VALID_MODULE_NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_]\w*$").unwrap());
static RELATIVE_IMPORT_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*from\s+(\.+)([\w.]*)\s+import\s+(.+)$").unwrap());

#[derive(Debug, Clone)]
pub struct PyProjectInfo {
    pub roots: Vec<PathBuf>,
    pub has_pyproject: bool,
    pub has_setup_cfg: bool,
}

fn configured_roots(root: &Path) -> PyProjectInfo {
    if let Some(pyproject) = read_to_string_opt(&root.join("pyproject.toml")) {
        let package_dir = PYPROJECT_PACKAGE_DIR_RE.captures(&pyproject).map(|c| c[1].to_string());
        let find_section = PYPROJECT_FIND_SECTION_RE.captures(&pyproject).map(|c| c[1].to_string());
        let where_ = find_section.as_deref().and_then(|s| WHERE_RE.captures(s)).map(|c| c[1].to_string());
        let selected = package_dir.or(where_);
        if let Some(selected) = selected {
            let candidate = root.join(&selected);
            let roots = if candidate.exists() { vec![candidate] } else { vec![] };
            return PyProjectInfo { roots, has_pyproject: true, has_setup_cfg: false };
        }
        return PyProjectInfo { roots: vec![root.to_path_buf()], has_pyproject: true, has_setup_cfg: false };
    }
    if let Some(setup_cfg) = read_to_string_opt(&root.join("setup.cfg")) {
        let options = SETUPCFG_OPTIONS_RE.captures(&setup_cfg).map(|c| c[1].to_string()).unwrap_or_default();
        let package_dir = SETUPCFG_PACKAGE_DIR_RE.captures(&options).map(|c| c[1].to_string());
        let find_section = SETUPCFG_FIND_SECTION_RE.captures(&setup_cfg).map(|c| c[1].to_string());
        let where_ = find_section.as_deref().and_then(|s| SETUPCFG_WHERE_RE.captures(s)).map(|c| c[1].to_string());
        let selected = package_dir.or(where_);
        if let Some(selected) = selected {
            let candidate = root.join(&selected);
            let roots = if candidate.exists() { vec![candidate] } else { vec![] };
            return PyProjectInfo { roots, has_pyproject: false, has_setup_cfg: true };
        }
        return PyProjectInfo { roots: vec![root.to_path_buf()], has_pyproject: false, has_setup_cfg: true };
    }
    PyProjectInfo { roots: vec![root.to_path_buf()], has_pyproject: false, has_setup_cfg: false }
}

/// Mirrors `pythonProjectInfo`.
pub fn python_project_info(repo_root: &Path) -> PyProjectInfo {
    let root = repo_root.canonicalize().unwrap_or_else(|_| repo_root.to_path_buf());
    let found = configured_roots(&root);
    PyProjectInfo {
        roots: found.roots.iter().map(|p| p.canonicalize().unwrap_or_else(|_| p.clone())).collect(),
        has_pyproject: found.has_pyproject,
        has_setup_cfg: found.has_setup_cfg,
    }
}

/// Mirrors `directTarget(base, fromFile)`.
fn direct_target(base: &Path) -> Option<PyResolution> {
    let file = PathBuf::from(format!("{}.py", base.display()));
    if file.is_file() {
        return Some(hit(file, "module"));
    }
    let init = base.join("__init__.py");
    if init.is_file() {
        Some(hit(init, "package"))
    } else {
        None
    }
}

/// Mirrors `namedTarget(packageInit, importedName)`.
fn named_target(package_init: &Path, imported_name: &str) -> Option<PyResolution> {
    let dir = package_init.parent()?;
    if let Some(PyResolution::Resolved(direct)) = direct_target(&dir.join(imported_name)) {
        return Some(hit(direct.resolved, "package_submodule"));
    }
    let text = read_to_string_opt(package_init).unwrap_or_default();
    for line in text.split(['\n']) {
        let line = line.trim_end_matches('\r');
        let Some(caps) = RELATIVE_IMPORT_LINE_RE.captures(line) else { continue };
        let dots = caps[1].len();
        let names: Vec<String> = caps[3]
            .split(',')
            .map(|n| {
                let n = n.trim();
                let parts: Vec<&str> = n.split_whitespace().collect();
                // pattern: NAME (as ALIAS)? -> take alias if present else name
                if let Some(pos) = parts.iter().position(|p| *p == "as") {
                    parts.get(pos + 1).map(|s| s.to_string()).unwrap_or_else(|| parts[0].to_string())
                } else {
                    parts.first().map(|s| s.to_string()).unwrap_or_default()
                }
            })
            .collect();
        if !names.iter().any(|n| n == imported_name) {
            continue;
        }
        let mut base = dir.to_path_buf();
        for _ in 1..dots {
            base = base.parent().map(Path::to_path_buf).unwrap_or(base);
        }
        let module_path = &caps[2];
        let parts: Vec<&str> = module_path.split('.').filter(|s| !s.is_empty()).collect();
        let target_base = parts.iter().fold(base, |acc, p| acc.join(p));
        if let Some(PyResolution::Resolved(target)) = direct_target(&target_base) {
            return Some(hit(target.resolved, "init_reexport"));
        }
    }
    None
}

/// Mirrors `relativeImport`.
fn relative_import(clean: &str, dots: usize, from_file: &Path, root: &Path, imported_name: Option<&str>) -> PyResolution {
    let mut base = from_file.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    for _ in 1..dots {
        base = base.parent().map(Path::to_path_buf).unwrap_or(base);
    }
    if !inside(root, &base) {
        return miss("relative_above_root");
    }
    let rest: &str = &clean[dots..];
    let parts: Vec<&str> = rest.split('.').filter(|s| !s.is_empty()).collect();
    let dotted = ".".repeat(dots) + &parts.join(".");
    let target = if !parts.is_empty() {
        let target_base = parts.iter().fold(base.clone(), |acc, p| acc.join(p));
        direct_target(&target_base)
    } else {
        Some(hit(base.join("__init__.py"), "package"))
    };
    let Some(PyResolution::Resolved(target)) = target else {
        return miss(format!("missing_relative:{dotted}"));
    };
    if !target.resolved.is_file() {
        return miss(format!("missing_relative:{dotted}"));
    }
    let is_init = target.resolved.file_name().and_then(|n| n.to_str()) == Some("__init__.py");
    if let (Some(name), true) = (imported_name, is_init) {
        return named_target(&target.resolved, name).unwrap_or_else(|| miss(format!("missing_relative:{dotted}")));
    }
    hit(target.resolved.clone(), if is_init { "relative_package" } else { "relative_module" })
}

#[derive(Debug, Clone, Default)]
pub struct PyResolveInput<'a> {
    pub specifier: &'a str,
    pub from_file: &'a str,
    pub repo_root: &'a str,
    pub imported_name: Option<&'a str>,
}

/// Mirrors `resolvePythonModule`. Exact precedence: relative (dot-prefixed)
/// specifiers first, then stdlib exclusion, then configured-root
/// package/module resolution with `__init__.py` re-export following.
pub fn resolve_python_module(input: PyResolveInput) -> PyResolution {
    if input.specifier.trim().is_empty() || input.from_file.trim().is_empty() || input.repo_root.trim().is_empty() {
        return miss("missing_input");
    }
    let clean = input.specifier.trim();
    let root = Path::new(input.repo_root).canonicalize().unwrap_or_else(|_| PathBuf::from(input.repo_root));
    let source = Path::new(input.from_file).canonicalize().unwrap_or_else(|_| PathBuf::from(input.from_file));
    let dots = clean.chars().take_while(|c| *c == '.').count();
    if dots > 0 {
        return relative_import(clean, dots, &source, &root, input.imported_name);
    }
    let top = clean.split('.').next().unwrap_or("");
    if !VALID_MODULE_NAME_RE.is_match(top) {
        return miss(format!("invalid_module_name:{clean}"));
    }
    if py_stdlib_has(top) {
        return miss(format!("stdlib:{top}"));
    }
    for search_root in python_project_info(&root).roots {
        let parts: Vec<&str> = clean.split('.').collect();
        let target_base = parts.iter().fold(search_root.clone(), |acc, p| acc.join(p));
        let Some(target) = direct_target(&target_base) else { continue };
        let PyResolution::Resolved(target) = target else { continue };
        let is_init = target.resolved.file_name().and_then(|n| n.to_str()) == Some("__init__.py");
        if let (Some(name), true) = (input.imported_name, is_init) {
            return named_target(&target.resolved, name).unwrap_or_else(|| miss(format!("not_repo_module:{top}")));
        }
        return hit(target.resolved, "package");
    }
    miss(format!("not_repo_module:{top}"))
}

/// Mirrors `readRepoFile`: path-confinement-checked file read.
pub fn read_repo_file(repo_root: &Path, relative_path: &Path) -> Option<String> {
    let root = repo_root.canonicalize().unwrap_or_else(|_| repo_root.to_path_buf());
    let target = root.join(relative_path);
    let target = target.canonicalize().unwrap_or(target);
    if inside(&root, &target) {
        read_to_string_opt(&target)
    } else {
        None
    }
}

// ---------------------------------------------------------------------
// javascript.mjs — core relative/absolute resolution (scoped port; see
// module doc for what is intentionally not ported)
// ---------------------------------------------------------------------

const TS_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".json"];
const JS_EXTENSIONS: &[&str] = &[".js", ".mjs", ".cjs", ".json"];

#[derive(Debug, Clone, PartialEq)]
pub enum JsResolution {
    Resolved { resolved: PathBuf, reason: &'static str },
    Ambiguous { candidates: Vec<PathBuf>, reason: &'static str },
    Unresolved { reason: &'static str },
    OutsideRepo,
    Unsupported(&'static str),
}

fn is_file(p: &Path) -> bool {
    p.is_file()
}
fn is_dir(p: &Path) -> bool {
    p.is_dir()
}

/// Mirrors `tryResolveFile(baseDir, specifier, extensions)`.
fn try_resolve_file(base_dir: &Path, specifier: &str, extensions: &[&str]) -> JsResolution {
    let candidate = base_dir.join(specifier);
    if is_file(&candidate) {
        return JsResolution::Resolved { resolved: candidate, reason: "exact" };
    }
    let ext_candidates: Vec<PathBuf> = extensions
        .iter()
        .map(|ext| PathBuf::from(format!("{}{}", candidate.display(), ext)))
        .filter(|p| is_file(p))
        .collect();
    if ext_candidates.len() == 1 {
        return JsResolution::Resolved { resolved: ext_candidates[0].clone(), reason: "extension" };
    }
    if ext_candidates.len() > 1 {
        return JsResolution::Ambiguous { candidates: ext_candidates, reason: "ambiguous_extension" };
    }
    if is_dir(&candidate) {
        let index = candidate.join("index");
        let index_candidates: Vec<PathBuf> = extensions
            .iter()
            .map(|ext| PathBuf::from(format!("{}{}", index.display(), ext)))
            .filter(|p| is_file(p))
            .collect();
        if index_candidates.len() == 1 {
            return JsResolution::Resolved { resolved: index_candidates[0].clone(), reason: "index" };
        }
        if index_candidates.len() > 1 {
            return JsResolution::Ambiguous { candidates: index_candidates, reason: "ambiguous_index" };
        }
    }
    JsResolution::Unresolved { reason: "missing" }
}

#[derive(Debug, Clone)]
pub struct JsResolveInput<'a> {
    pub specifier: &'a str,
    pub from_file: &'a str,
    pub repo_root: Option<&'a str>,
    pub is_typescript: bool,
}

/// Mirrors the relative/absolute branches of `resolveModuleSpecifier`. Bare
/// (package) specifiers return `Unsupported` — see module doc scope note.
pub fn resolve_js_module(input: JsResolveInput) -> JsResolution {
    if input.specifier.is_empty() || input.from_file.is_empty() {
        return JsResolution::Unresolved { reason: "missing_input" };
    }
    let extensions: &[&str] = if input.is_typescript { TS_EXTENSIONS } else { JS_EXTENSIONS };
    let root = input.repo_root.map(|r| Path::new(r).canonicalize().unwrap_or_else(|_| PathBuf::from(r)));
    let from_file = Path::new(input.from_file);

    if Path::new(input.specifier).is_absolute() {
        let absolute = PathBuf::from(input.specifier);
        if let Some(root) = &root {
            let canon = absolute.canonicalize().unwrap_or_else(|_| absolute.clone());
            if !inside(root, &canon) {
                return JsResolution::OutsideRepo;
            }
        }
        return if is_file(&absolute) {
            JsResolution::Resolved { resolved: absolute, reason: "absolute" }
        } else {
            JsResolution::Unresolved { reason: "missing" }
        };
    }

    if input.specifier.starts_with('.') || input.specifier.starts_with('/') {
        let base_dir = from_file.parent().unwrap_or_else(|| Path::new("."));
        let result = try_resolve_file(base_dir, input.specifier, extensions);
        if let Some(root) = &root {
            let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
            let outside = match &result {
                JsResolution::Resolved { resolved, .. } => !inside(root, &canon(resolved)),
                JsResolution::Ambiguous { candidates, .. } => candidates.iter().any(|c| !inside(root, &canon(c))),
                _ => false,
            };
            if outside {
                return JsResolution::OutsideRepo;
            }
        }
        return match result {
            JsResolution::Resolved { resolved, .. } => JsResolution::Resolved { resolved, reason: "relative" },
            other => other,
        };
    }

    JsResolution::Unsupported("bare_specifier_resolution_not_ported")
}

#[derive(Debug, Clone)]
pub struct JsSpecifier {
    pub specifier: String,
    pub line: usize,
}

static IMPORT_EXPORT_FROM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:import|export)(?:\s+type)?[\s\S]*?\sfrom\s+["']([^"']+)["']"#).unwrap());
static IMPORT_BARE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"import\s*["']([^"']+)["']"#).unwrap());
static REQUIRE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"require\s*\(\s*["']([^"']+)["']\s*\)"#).unwrap());

/// Mirrors `extractJavaScriptModuleSpecifiers` (line-scoped, dedup by
/// specifier+line).
pub fn extract_javascript_module_specifiers(text: &str) -> Vec<JsSpecifier> {
    let mut found: Vec<JsSpecifier> = Vec::new();
    let mut seen: HashSet<(String, usize)> = HashSet::new();
    for (index, line) in text.split(['\n']).enumerate() {
        let line_no = index + 1;
        for re in [&*IMPORT_EXPORT_FROM_RE, &*IMPORT_BARE_RE, &*REQUIRE_RE] {
            for caps in re.captures_iter(line) {
                let spec = caps[1].to_string();
                let key = (spec.clone(), line_no);
                if seen.insert(key) {
                    found.push(JsSpecifier { specifier: spec, line: line_no });
                }
            }
        }
    }
    found
}

// ---------------------------------------------------------------------
// In-memory build-pass adapter (wires this module into graph.rs)
// ---------------------------------------------------------------------
//
// `graph.rs`'s build pass operates over an abstract `FileRecord` map, not
// the filesystem: generations are built from synthetic/scanned file sets
// (including in tests and re-anchor paths) that need not exist on disk, so
// `resolve_js_module`/`resolve_python_module` above (which stat real files)
// cannot be called directly from the build pass. These two functions are a
// verbatim, disk-free port of graph.rs's former `import_specifiers` /
// `resolve_import` — same specifier-extraction patterns, same candidate
// order, same first-match-wins semantics — so swapping the call site is a
// pure relocation with no behavior change (proved by
// `tests/parity_module_resolution_wired.rs`). Full ambiguity/typed-miss
// resolution (`JsResolution`/`PyResolution` above) requires real file/
// directory existence and is out of scope for the in-memory build pass;
// this is the module's one intentionally-unwired surface, recorded in the
// lane receipt rather than silently narrowed.

static PY_FROM_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*from\s+([.A-Za-z_]\w*(?:\.\w+)*)\s+import").unwrap());
static PY_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*import\s+([A-Za-z_]\w*(?:\.\w+)*)").unwrap());
static RS_USE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:pub\s+)?use\s+([^;]+)").unwrap());
static RS_MOD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:pub\s+)?mod\s+([A-Za-z_]\w*)\s*;").unwrap());

/// Verbatim port of graph.rs's former `import_specifiers(ext, text)`.
pub fn import_specifiers_in_files(ext: &str, text: &str) -> Vec<String> {
    let patterns: Vec<&Regex> = if ext == "py" {
        vec![&*PY_FROM_IMPORT_RE, &*PY_IMPORT_RE]
    } else if ext == "rs" {
        vec![&*RS_USE_RE, &*RS_MOD_RE]
    } else {
        vec![&*IMPORT_EXPORT_FROM_RE, &*IMPORT_BARE_RE, &*REQUIRE_RE]
    };
    let mut out = std::collections::BTreeSet::new();
    for pattern in patterns {
        for line in text.lines() {
            if let Some(c) = pattern.captures(line) {
                if let Some(m) = c.get(1) {
                    out.insert(m.as_str().to_owned());
                }
            }
        }
    }
    out.into_iter().collect()
}

/// Verbatim port of graph.rs's former `resolve_import(source, specifier,
/// files)`: relative specifiers only, first candidate present in the file
/// map wins (no ambiguity signal — see module doc scope note above).
pub fn resolve_import_in_files(source: &str, specifier: &str, files: &BTreeMap<String, &FileRecord>) -> Option<String> {
    if !specifier.starts_with('.') && !source.ends_with(".rs") && !source.ends_with(".py") {
        return None;
    }
    let source_path = Path::new(source);
    let parent = source_path.parent().unwrap_or_else(|| Path::new(""));
    // Path::join treats .worker as a hidden filename, while Python's leading
    // dots denote parent traversal. Likewise, ./worker must lose its
    // current-directory component before looking up an in-memory path.
    let (base_parent, clean_specifier) = if source.ends_with(".py") && specifier.starts_with('.') {
        let dots = specifier.chars().take_while(|c| *c == '.').count();
        let mut parent = parent.to_path_buf();
        for _ in 1..dots {
            parent = parent.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        }
        (parent, specifier[dots..].trim_start_matches('.').replace('.', "/"))
    } else {
        (parent.to_path_buf(), specifier.strip_prefix("./").unwrap_or(specifier).to_string())
    };
    let base = normalize_memory_path(&base_parent.join(clean_specifier));
    let mut candidates = vec![base.clone()];
    if source.ends_with(".py") {
        candidates.extend([format!("{base}.py"), format!("{base}/__init__.py")]);
    } else if source.ends_with(".rs") {
        candidates.extend([format!("{base}.rs"), format!("{base}/mod.rs")]);
    } else {
        for ext in ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"] {
            candidates.push(format!("{base}.{ext}"));
            candidates.push(format!("{base}/index.{ext}"));
        }
    }
    candidates.into_iter().find(|p| files.contains_key(p))
}

// Normalize repository-relative paths used by the graph's in-memory file map.
fn normalize_memory_path(path: &Path) -> String {
    let mut components: Vec<String> = Vec::new();
    for component in path.to_string_lossy().replace('\\', "/").split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.last().is_some_and(|value| value != "..") {
                    components.pop();
                } else {
                    components.push("..".to_owned());
                }
            }
            value => components.push(value.to_owned()),
        }
    }
    components.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_first_resolves_single_best_tier_candidate() {
        let candidates = vec![
            IdentityCandidate { target_id: "a".into(), confidence_tier: "EXACT_RESOLUTION".into() },
            IdentityCandidate { target_id: "b".into(), confidence_tier: "CROSS_FILE_HEURISTIC".into() },
        ];
        assert_eq!(
            resolve_exact_first(&candidates),
            IdentityResolution::Resolved { target_id: "a".into(), confidence_tier: "EXACT_RESOLUTION".into() }
        );
    }

    #[test]
    fn same_tier_ambiguity_stops_resolution() {
        let candidates = vec![
            IdentityCandidate { target_id: "a".into(), confidence_tier: "EXACT_RESOLUTION".into() },
            IdentityCandidate { target_id: "b".into(), confidence_tier: "EXACT_RESOLUTION".into() },
        ];
        match resolve_exact_first(&candidates) {
            IdentityResolution::Ambiguous { candidates, .. } => assert_eq!(candidates, vec!["a".to_string(), "b".to_string()]),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn no_candidates_unresolved() {
        assert_eq!(resolve_exact_first(&[]), IdentityResolution::Unresolved { reason: "no_candidates" });
    }

    #[test]
    fn stdlib_set_matches_legacy_count_and_spot_checks() {
        assert_eq!(PY_STDLIB.len(), 113);
        for name in ["os", "sys", "typing", "asyncio", "zoneinfo", "__future__", "tomllib"] {
            assert!(py_stdlib_has(name), "missing stdlib entry {name}");
        }
        assert!(!py_stdlib_has("numpy"));
    }

    #[test]
    fn extract_python_specifiers_handles_from_and_plain_import() {
        let text = "from foo.bar import Baz\nimport os\nimport a.b.c\n";
        let found = extract_python_module_specifiers(text);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].specifier, "foo.bar");
        assert_eq!(found[0].imported_name.as_deref(), Some("Baz"));
        assert_eq!(found[1].specifier, "os");
        assert_eq!(found[1].imported_name, None);
        assert_eq!(found[2].specifier, "a.b.c");
    }

    #[test]
    fn resolve_python_module_missing_input_is_typed() {
        let result = resolve_python_module(PyResolveInput { specifier: "", from_file: "x.py", repo_root: "/r", imported_name: None });
        assert_eq!(result, PyResolution::Unresolved(PyMiss { reason: "missing_input".into() }));
    }

    #[test]
    fn resolve_python_module_stdlib_is_typed_miss() {
        let result = resolve_python_module(PyResolveInput { specifier: "os", from_file: "/repo/x.py", repo_root: "/repo", imported_name: None });
        assert_eq!(result, PyResolution::Unresolved(PyMiss { reason: "stdlib:os".into() }));
    }

    #[test]
    fn resolve_python_module_invalid_name_is_typed_miss() {
        let result = resolve_python_module(PyResolveInput { specifier: "1bad", from_file: "/repo/x.py", repo_root: "/repo", imported_name: None });
        assert_eq!(result, PyResolution::Unresolved(PyMiss { reason: "invalid_module_name:1bad".into() }));
    }

    #[test]
    fn js_bare_specifier_is_typed_unsupported_not_silent() {
        let result = resolve_js_module(JsResolveInput { specifier: "lodash", from_file: "/repo/a.ts", repo_root: Some("/repo"), is_typescript: true });
        assert_eq!(result, JsResolution::Unsupported("bare_specifier_resolution_not_ported"));
    }

    #[test]
    fn js_missing_input_is_typed() {
        let result = resolve_js_module(JsResolveInput { specifier: "", from_file: "/repo/a.ts", repo_root: None, is_typescript: true });
        assert_eq!(result, JsResolution::Unresolved { reason: "missing_input" });
    }
}
