//! Native Blueprint graph construction.
//!
//! This module is intentionally a small, deterministic port of the former
//! static/tree-sitter provider.  It owns source discovery and fact assembly;
//! storage and query/traversal remain owned by their respective modules.

use crate::identity::{compute_generation_id, content_digest};
use crate::api::CancellationToken;
use crate::model::{GraphEdge, GraphNode};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tree_sitter::{Node, Parser};

pub const GRAPH_SCHEMA_VERSION: u32 = 1;
pub const PROVIDER_VERSION: &str = "native-rust-1";
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_DIRS: usize = 50_000;
pub const MAX_ENTRIES_PER_DIR: usize = 5_000;

const IGNORED_DIRS: &[&str] = &[
    ".agent", ".audit", ".cache", ".git", ".next", ".nuxt", ".output",
    ".parcel-cache", ".pnpm-store", ".pytest_cache", ".svelte-kit", ".tox",
    ".turbo", ".vercel", ".vscode", ".worktrees", "__pycache__", "build",
    "coverage", "dist", "fixture-repos", "htmlcov", "node_modules", "out",
    "target", "vendor",
];
const IGNORED_FILES: &[&str] = &[".DS_Store", "Thumbs.db", "architecture.md", "product.md"];

/// Canonical source-universe policy shared by graph discovery & native watch
/// snapshots. Generated payloads stay outside both semantic indexing paths.
pub fn is_canonical_ignored_dir(name: &str) -> bool {
    // `.tmp-` prefixed directories are transient build/diagnosis scratch (e.g.
    // the release pipeline's `.tmp-release-diagnosis-*` trees, which stage full
    // installer payloads — hundreds of MB of binaries). They are never source
    // and walking them both wastes the scan and, before the oversized-file fix,
    // poisoned generation completeness. Treat them like `.agent-` scratch.
    IGNORED_DIRS.iter().any(|ignored| *ignored == name)
        || name.starts_with(".agent-")
        || name.starts_with(".tmp-")
}

pub fn is_canonical_ignored_file(relative: &str, name: &str) -> bool {
    IGNORED_FILES.iter().any(|ignored| *ignored == name)
        || (relative.starts_with("docs/") && matches!(name, "product.md" | "architecture.md"))
        || matches!(relative, "docs/product/README.md" | "docs/architecture/membrane.md")
}

/// The single, reader-independent repository source-universe policy.
///
/// Whether a path is source cannot depend on *which* subsystem or mode is
/// reading the repo: the resident watcher, a Hub-less one-shot explicit
/// operation, and bootstrap reuse-detection must all prune identically, or
/// they disagree and force needless rebuilds / walk gitignored trees. This
/// type combines the canonical hardcoded exclusions with the repository's own
/// root `.gitignore` so every Blueprint reader funnels through one decision,
/// regardless of whether the Hub/daemon is running.
pub struct RepoIgnore {
    gitignore: ignore::gitignore::Gitignore,
}

impl RepoIgnore {
    pub fn for_root(root: &Path) -> Self {
        let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
        let _ = builder.add(root.join(".gitignore"));
        let gitignore = builder.build().unwrap_or_else(|_| ignore::gitignore::Gitignore::empty());
        Self { gitignore }
    }

    /// True when a directory (by repo-relative path + leaf name) is not source.
    pub fn dir_ignored(&self, relative: &str, name: &str) -> bool {
        is_canonical_ignored_dir(name)
            || self.gitignore.matched(Path::new(relative), true).is_ignore()
    }

    /// True when a file (by repo-relative path + leaf name) is not source.
    pub fn file_ignored(&self, relative: &str, name: &str) -> bool {
        is_canonical_ignored_file(relative, name)
            || self.gitignore.matched(Path::new(relative), false).is_ignore()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum PrecisionTier { Compiler, Ast, Lexical }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
pub enum ConfidenceTier { ExactResolution, SameFileLexical, CrossFileHeuristic, Unresolved }

impl ConfidenceTier {
    pub const fn score(self) -> f64 {
        match self { Self::ExactResolution => 1.0, Self::SameFileLexical => 0.75,
            Self::CrossFileHeuristic => 0.5, Self::Unresolved => 0.0 }
    }
    pub const fn as_str(self) -> &'static str {
        match self { Self::ExactResolution => "EXACT_RESOLUTION", Self::SameFileLexical => "SAME_FILE_LEXICAL",
            Self::CrossFileHeuristic => "CROSS_FILE_HEURISTIC", Self::Unresolved => "UNRESOLVED" }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: String,
    pub absolute_path: PathBuf,
    pub bytes: Vec<u8>,
    pub text: Option<String>,
    pub content_hash: String,
    pub semantic_content_hash: String,
    pub size: u64,
}

impl FileRecord {
    pub fn is_code(&self) -> bool { language_for_path(&self.path).is_some() }
    pub fn extension(&self) -> &str { self.path.rsplit_once('.').map(|(_, e)| e).unwrap_or("") }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanOptions {
    pub file_limit: usize,
    pub max_dirs: Option<usize>,
    pub max_entries_per_dir: Option<usize>,
    pub ignored_prefixes: Vec<String>,
    pub tracked_only: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanReport {
    pub files: Vec<FileRecord>,
    pub traversal_truncated: bool,
    pub file_limit_reached: bool,
    pub truncation_reasons: Vec<String>,
    #[serde(default)]
    pub skipped: Vec<ScanDisposition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanDisposition {
    pub path: String,
    pub state: String,
    pub reason: String,
}

const MAX_SCAN_ERROR_DETAIL_CHARS: usize = 256;

fn record_scan_skip(report: &mut ScanReport, path: String, reason: &str, detail: impl std::fmt::Display) {
    report.traversal_truncated = true;
    report.truncation_reasons.push(reason.into());
    let detail = detail.to_string();
    let detail: String = detail.chars().take(MAX_SCAN_ERROR_DETAIL_CHARS).collect();
    report.skipped.push(ScanDisposition { path, state: "unavailable".into(), reason: format!("{reason}:{detail}") });
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReport {
    pub path: String,
    pub language: Option<String>,
    pub provider: String,
    pub precision: PrecisionTier,
    pub parse_status: String,
    pub error_node_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilerFacts {
    #[serde(default)] pub nodes: Vec<GraphNode>,
    #[serde(default)] pub edges: Vec<GraphEdge>,
    pub provider: String,
    pub version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphOptions {
    #[serde(default)] pub scan: ScanOptions,
    #[serde(default)] pub compiler: Option<CompilerFacts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphGeneration {
    pub schema_version: u32,
    pub provider: String,
    pub provider_version: String,
    pub generation_id: String,
    pub source_hash: String,
    pub repo_root: String,
    pub complete: bool,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub files: Vec<FileReport>,
    pub truncation_reasons: Vec<String>,
}

/// Facts needed by the native incremental store path for one source file.
/// Provider-wide augmentations intentionally stay on the full-build path.
#[derive(Debug, Clone)]
pub struct FileFacts {
    pub file: GraphNode,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub report: FileReport,
    pub content_digest: String,
    pub size: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("graph build cancelled")]
    Cancelled,
    #[error("repository root is unavailable: {0}")] Root(String),
    #[error("repository path escapes canonical root: {0}")] EscapesRoot(String),
    #[error("source read failed for {path}: {message}")] Read { path: String, message: String },
    #[error("invalid graph fact: {0}")] InvalidFact(String),
}

macro_rules! capture_re {
    ($line:expr, $pattern:literal) => {{
        static REGEX: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new($pattern).expect("graph capture pattern is a valid literal")
        });
        REGEX
            .captures($line)
            .and_then(|captures| captures.get(1).map(|match_| match_.as_str().to_owned()))
    }};
}

static CALL_NAMES_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:\.|\b)([A-Za-z_$][A-Za-z0-9_$]*)\s*\(").expect("call-name pattern"));
static SYMBOL_NAMES_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Za-z_$][A-Za-z0-9_$]*\b").expect("symbol-name pattern"));
static GENERATED_POINTER_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\n?<!-- blueprint:docs:start -->.*?<!-- blueprint:docs:end -->\n?").expect("generated-pointer pattern"));

/// Deterministically discover bounded, root-confined files.  Symlinked files
/// and directories are skipped unless their canonical target remains inside
/// the canonical repository root; entries, depth, and bytes are bounded.
pub fn scan_repository(root: impl AsRef<Path>, options: &ScanOptions) -> Result<ScanReport, GraphError> {
    scan_repository_with_cancellation(root, options, &CancellationToken::new())
}

pub fn scan_repository_with_cancellation(root: impl AsRef<Path>, options: &ScanOptions, cancellation: &CancellationToken) -> Result<ScanReport, GraphError> {
    let root = fs::canonicalize(root.as_ref()).map_err(|e| GraphError::Root(e.to_string()))?;
    if !root.is_dir() { return Err(GraphError::Root("root is not a directory".into())); }
    // Honour the repository's own .gitignore, matching what Ledger's SourcePolicy
    // already does. Without this the scanner walked gitignored trees (e.g. a
    // repo's `.tmp-*` build/diagnosis scratch — hundreds of MB of binaries),
    // which both wasted the walk and, before the completeness fix, marked the
    // generation incomplete. A gitignored directory prunes its whole subtree.
    let ignore = RepoIgnore::for_root(&root);
    let max_dirs = options.max_dirs.unwrap_or(MAX_DIRS);
    let max_entries = options.max_entries_per_dir.unwrap_or(MAX_ENTRIES_PER_DIR);
    let prefixes: Vec<String> = options.ignored_prefixes.iter().map(|value| normalize_path(value)).collect();
    let mut stack = vec![root.clone()];
    let mut visited = HashSet::new();
    let mut report = ScanReport::default();
    while let Some(dir) = stack.pop() {
        if cancellation.is_cancelled() { return Err(GraphError::Cancelled); }
        if visited.len() >= max_dirs { report.traversal_truncated = true; report.truncation_reasons.push("directory_limit".into()); break; }
        let canonical = match fs::canonicalize(&dir) {
            Ok(path) => path,
            Err(error) if dir == root => return Err(GraphError::Root(error.to_string())),
            Err(error) => {
                let relative = normalize_path(&dir.strip_prefix(&root).unwrap_or(&dir).to_string_lossy());
                record_scan_skip(&mut report, relative, "directory_canonicalize_error", error);
                continue;
            }
        };
        if !canonical.starts_with(&root) || !visited.insert(canonical) { continue; }
        let mut entries = Vec::new();
        let directory_entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if dir == root => return Err(GraphError::Root(error.to_string())),
            Err(error) => {
                let relative = normalize_path(&dir.strip_prefix(&root).unwrap_or(&dir).to_string_lossy());
                record_scan_skip(&mut report, relative, "directory_read_error", error);
                continue;
            }
        };
        for entry in directory_entries {
            match entry {
                Ok(entry) => entries.push(entry),
                Err(error) => {
                    let relative = dir.strip_prefix(&root).map(|path| normalize_path(&path.to_string_lossy())).unwrap_or_default();
                    record_scan_skip(&mut report, relative, "directory_entry_error", error);
                }
            }
        }
        if entries.len() > max_entries { report.traversal_truncated = true; report.truncation_reasons.push("directory_entry_limit".into()); entries.truncate(max_entries); }
        entries.sort_by(|a, b| a.file_name().to_string_lossy().cmp(&b.file_name().to_string_lossy()));
        let mut child_dirs = Vec::new();
        for entry in entries {
            if cancellation.is_cancelled() { return Err(GraphError::Cancelled); }
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            let rel = match path.strip_prefix(&root) { Ok(v) => normalize_path(&v.to_string_lossy()), Err(_) => continue };
            if rel.is_empty() || prefixes.iter().any(|p| rel == *p || rel.starts_with(&format!("{p}/"))) { continue; }
            let ty = match entry.file_type() {
                Ok(v) => v,
                Err(error) => { record_scan_skip(&mut report, rel, "file_type_error", error); continue; }
            };
            // `file_type` does not follow links.  Resolve every entry before
            // admitting it so internal links are indexed while dangling or
            // escaping links receive an explicit typed disposition.
            let target = match fs::canonicalize(&path) {
                Ok(v) => v,
                Err(error) => {
                    if ty.is_symlink() {
                        report.skipped.push(ScanDisposition { path: rel, state: "skipped".into(), reason: format!("dangling_symlink:{error}") });
                    } else {
                        record_scan_skip(&mut report, rel, "canonicalize_error", error);
                    }
                    continue;
                }
            };
            if !target.starts_with(&root) {
                if ty.is_symlink() { report.skipped.push(ScanDisposition { path: rel, state: "skipped".into(), reason: "symlink_escape_root".into() }); }
                continue;
            }
            let target_meta = match fs::metadata(&path) { Ok(v) => v, Err(error) => {
                record_scan_skip(&mut report, rel, "metadata_error", error);
                continue;
            }};
            if target_meta.is_dir() {
                if ignore.dir_ignored(&rel, &name) { continue; }
                child_dirs.push(path);
            } else if target_meta.is_file() {
                if ignore.file_ignored(&rel, &name) { continue; }
                let meta = match fs::metadata(&path) { Ok(v) => v, Err(error) => {
                    record_scan_skip(&mut report, rel, "metadata_error", error);
                    continue;
                }};
                if meta.len() > MAX_FILE_BYTES {
                    // An oversized file (a binary, a vendored blob) is an
                    // expected per-file omission, not a failure to traverse the
                    // tree: record the skip but DO NOT set traversal_truncated.
                    // Marking the whole generation incomplete here is what made
                    // every real-world repo (any repo containing a .exe/.wasm)
                    // report complete=false, which in turn made query.rs suppress
                    // every search/recall/expand/impact and return zero context.
                    report.skipped.push(ScanDisposition { path: rel.clone(), state: "unsupported".into(), reason: format!("file_bytes:{}>limit:{}", meta.len(), MAX_FILE_BYTES) });
                    continue;
                }
                let bytes = match fs::read(&path) { Ok(v) => v, Err(error) => {
                    record_scan_skip(&mut report, rel, "file_read_error", error);
                    continue;
                }};
                let code = language_for_path(&rel).is_some();
                if code && bytes.contains(&0) { continue; }
                let text = String::from_utf8_lossy(&bytes).into_owned();
                let semantic = if rel == "README.md" { strip_generated_pointer(&text) } else { text.clone() };
                report.files.push(FileRecord { path: rel, absolute_path: path, size: bytes.len() as u64,
                    content_hash: content_digest(&bytes), semantic_content_hash: content_digest(semantic.as_bytes()), bytes,
                    text: if code || is_file_only(&name) { Some(text) } else { None } });
                if options.file_limit > 0 && report.files.len() >= options.file_limit { report.file_limit_reached = true; report.truncation_reasons.push("file_limit".into()); break; }
            }
        }
        child_dirs.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
        stack.extend(child_dirs.into_iter().rev());
        if report.file_limit_reached { break; }
    }
    report.files.sort_by(|a, b| compare_paths(&a.path, &b.path));
    report.skipped.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.reason.cmp(&b.reason)));
    report.truncation_reasons.sort(); report.truncation_reasons.dedup();
    Ok(report)
}

pub fn build_generation(root: impl AsRef<Path>, options: &GraphOptions) -> Result<GraphGeneration, GraphError> {
    build_generation_with_cancellation(root, options, &CancellationToken::new())
}

pub fn build_generation_with_cancellation(root: impl AsRef<Path>, options: &GraphOptions, cancellation: &CancellationToken) -> Result<GraphGeneration, GraphError> {
    let root = fs::canonicalize(root.as_ref()).map_err(|e| GraphError::Root(e.to_string()))?;
    let scan = scan_repository_with_cancellation(&root, &options.scan, cancellation)?;
    build_generation_from_files_with_cancellation(&root, scan, options, cancellation)
}

pub fn build_generation_from_files(root: &Path, scan: ScanReport, options: &GraphOptions) -> Result<GraphGeneration, GraphError> {
    build_generation_from_files_with_cancellation(root, scan, options, &CancellationToken::new())
}

pub fn build_generation_from_files_with_cancellation(root: &Path, scan: ScanReport, options: &GraphOptions, cancellation: &CancellationToken) -> Result<GraphGeneration, GraphError> {
    let mut lexical = Vec::new();
    let mut lexical_edges = Vec::new();
    let mut reports = Vec::new();
    let file_map: BTreeMap<String, &FileRecord> = scan.files.iter().map(|f| (f.path.clone(), f)).collect();
    // Compute resolver-config identity once per build. It is retained on
    // config-file evidence so dependency/projection consumers can invalidate
    // against the exact same candidate set as the legacy build pass.
    let config_digest = crate::static_provider::build_config_digest_for_files(&scan.files);
    for file in &scan.files {
        if cancellation.is_cancelled() { return Err(GraphError::Cancelled); }
        let surface = module_surface(file);
        let file_node = file_node(file, &surface);
        lexical.push(file_node.clone());
        if let Some(text) = &file.text {
            let (mut nodes, mut edges, report) = lexical_facts(file, text, &file_map, &surface);
            lexical.append(&mut nodes); lexical_edges.append(&mut edges); reports.push(report);
        } else {
            reports.push(FileReport { path: file.path.clone(), language: language_for_path(&file.path).map(str::to_owned), provider: "lexical".into(), precision: PrecisionTier::Lexical, parse_status: "unsupported".into(), error_node_count: 0, error: None });
        }
    }
    if let Some(config_digest) = config_digest {
        for node in &mut lexical {
            if node.kind != "file" || !crate::static_provider::is_build_config_file(node.path.as_deref().unwrap_or("")) { continue; }
            if let Some(evidence) = node.evidence.first_mut().and_then(Value::as_object_mut) {
                evidence.insert("configDigest".into(), json!(config_digest.clone()));
            }
        }
    }
    let mut ast_nodes = Vec::new(); let mut ast_edges = Vec::new();
    for file in &scan.files {
        if cancellation.is_cancelled() { return Err(GraphError::Cancelled); }
        if let Some(text) = &file.text {
            if let Some(language) = parser_language(file.extension()) {
                let ast = ast_facts(file, text, language, cancellation)?;
                if ast.report.parse_status != "failed" && ast.report.parse_status != "partial" {
                    ast_nodes.extend(ast.nodes); ast_edges.extend(ast.edges);
                }
                if let Some(slot) = reports.iter_mut().find(|r| r.path == file.path) {
                    if ast.report.parse_status == "ok" || ast.report.parse_status == "partial" { *slot = ast.report; }
                }
            }
        }
    }
    let (mut nodes, mut edges) = merge_facts(lexical, lexical_edges, ast_nodes, ast_edges, options.compiler.clone());
    edges = resolve_edges(edges, &nodes, &file_map);
    // Provider registry is the single build-pass admission point. Framework,
    // IaC, SCIP, and bridge providers all contribute through this ordered
    // pass; framework intelligence remains its documented post-pass.
    let provider_context = crate::providers::ProviderContext { repo_root: root, files: &scan.files, file_map: &file_map };
    let mut supplemental = crate::providers::ProviderOutput::default();
    for descriptor in crate::providers::registry() {
        supplemental.merge((descriptor.run)(&provider_context));
    }
    merge_supplemental(&mut nodes, &mut edges, supplemental, &file_map);
    let source_hash = source_hash(&scan.files);
    let mut generation = GraphGeneration { schema_version: GRAPH_SCHEMA_VERSION, provider: "native-rust".into(), provider_version: PROVIDER_VERSION.into(),
        generation_id: String::new(), source_hash: source_hash.clone(), repo_root: normalize_path(&root.to_string_lossy()), complete: !scan.traversal_truncated && !scan.file_limit_reached,
        nodes, edges, files: reports, truncation_reasons: scan.truncation_reasons };
    // Framework intelligence owns its graph-shape adapter so generated
    // domain bindings, edge evidence, and entry-point marks stay consistent
    // with direct callers of the provider module.
    crate::framework_intelligence::augment_graph_generation(&mut generation, &scan.files);
    let (nodes_json, edges_json) = generation_identity_bodies(&generation.nodes, &generation.edges);
    let generation_id = compute_generation_id(&nodes_json, &edges_json, Some(&source_hash));
    generation.generation_id = generation_id.clone();
    for node in &mut generation.nodes { node.generation_id = generation_id.clone(); }
    for edge in &mut generation.edges { edge.generation_id = generation_id.clone(); }
    Ok(generation)
}

/// Build only lexical/AST facts for one code file. This is deliberately
/// narrower than `build_generation`: provider registry, framework
/// augmentation, and cross-file resolution require a complete scan and cause
/// callers to fall back to a full refresh.
pub fn build_file_facts(root: &Path, relative_path: &str, cancellation: &CancellationToken) -> Result<Option<FileFacts>, GraphError> {
    let relative_path = normalize_path(relative_path);
    if language_for_path(&relative_path).is_none() { return Ok(None); }
    let root = fs::canonicalize(root).map_err(|e| GraphError::Root(e.to_string()))?;
    let absolute_path = root.join(&relative_path);
    crate::security::is_confined_path(&root, &absolute_path, true).map_err(|_| GraphError::EscapesRoot(relative_path.clone()))?;
    let metadata = match fs::metadata(&absolute_path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(GraphError::Read { path: relative_path, message: error.to_string() }),
    };
    if metadata.len() > MAX_FILE_BYTES { return Ok(None); }
    let bytes = fs::read(&absolute_path).map_err(|error| GraphError::Read { path: relative_path.clone(), message: error.to_string() })?;
    if bytes.contains(&0) { return Ok(None); }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let file = FileRecord {
        path: relative_path.clone(), absolute_path, size: bytes.len() as u64, bytes: bytes.clone(),
        content_hash: content_digest(&bytes),
        semantic_content_hash: content_digest(text.as_bytes()), text: Some(text.clone()),
    };
    let surface = module_surface(&file);
    let mut file_map = BTreeMap::new();
    file_map.insert(file.path.clone(), &file);
    let (lexical_nodes, lexical_edges, lexical_report) = lexical_facts(&file, &text, &file_map, &surface);
    let mut ast_nodes = Vec::new();
    let mut ast_edges = Vec::new();
    let mut report = lexical_report;
    if let Some(language) = parser_language(file.extension()) {
        let ast = ast_facts(&file, &text, language, cancellation)?;
        if ast.report.parse_status != "failed" && ast.report.parse_status != "partial" {
            ast_nodes = ast.nodes;
            ast_edges = ast.edges;
        }
        if ast.report.parse_status == "ok" || ast.report.parse_status == "partial" { report = ast.report; }
    }
    let (mut nodes, mut edges) = merge_facts(lexical_nodes, lexical_edges, ast_nodes, ast_edges, None);
    edges = resolve_edges(edges, &nodes, &file_map);
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Some(FileFacts {
        file: file_node(&file, &surface), nodes, edges, report,
        content_digest: file.content_hash, size: file.size as i64,
    }))
}

/// Compute source identity from an already collected scan without running any
/// parser or provider. Used to seal an incremental refresh atomically.
pub fn source_hash_for_files(files: &[FileRecord]) -> String { source_hash(files) }

/// Return whether a path has a native parser and is safe for the structural
/// incremental lane.
pub fn is_code_path(path: &str) -> bool { language_for_path(path).is_some() }

fn merge_supplemental(
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    output: crate::providers::ProviderOutput,
    file_map: &BTreeMap<String, &FileRecord>,
) {
    let mut node_ids: HashSet<String> = nodes.iter().map(|node| node.id.clone()).collect();
    let mut edge_ids: HashSet<String> = edges.iter().map(|edge| edge.id.clone()).collect();
    for mut node in output.nodes {
        normalize_provider_evidence(&mut node.evidence, node.path.as_deref(), file_map);
        if node_ids.insert(node.id.clone()) { nodes.push(node); }
    }
    for mut edge in output.edges {
        let path = edge.evidence.iter().find_map(|e| e.get("path").and_then(Value::as_str)).map(str::to_owned);
        normalize_provider_evidence(&mut edge.evidence, path.as_deref(), file_map);
        if edge_ids.insert(edge.id.clone()) { edges.push(edge); }
    }
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| a.id.cmp(&b.id));
}

/// Closed provider adapters may omit the source digest because their rich
/// payload predates the V1 graph envelope. Bind evidence to the scanned file
/// here, once, before graph identity is computed; never admit an unbound fact
/// into source-backed query candidate sets.
fn normalize_provider_evidence(
    evidence: &mut [Value],
    path: Option<&str>,
    file_map: &BTreeMap<String, &FileRecord>,
) {
    let Some(path) = path.map(|value| value.replace('\\', "/")) else { return };
    let Some(file) = file_map.get(&path) else { return };
    for row in evidence {
        let Some(object) = row.as_object_mut() else { continue };
        object.entry("path").or_insert_with(|| json!(file.path.clone()));
        object.entry("contentHash").or_insert_with(|| json!(file.content_hash.clone()));
        object.entry("startLine").or_insert_with(|| json!(1));
        object.entry("endLine").or_insert_with(|| json!(1));
    }
}

fn file_node(file: &FileRecord, module_surface: &Value) -> GraphNode {
    let evidence = json!({"path":file.path,"startLine":1,"endLine":line_count(file.text.as_deref().unwrap_or("")),"contentHash":file.content_hash,
        "provider":"lexical","precisionTier":"LEXICAL","confidenceTier":"EXACT_RESOLUTION",
        "moduleSurface": module_surface});
    GraphNode { id: format!("file:{}", file.path), kind: "file".into(), path: Some(file.path.clone()), name: file.path.rsplit('/').next().map(str::to_owned), generation_id: String::new(), evidence: vec![evidence] }
}

/// Persist the lossless JS/TS module surface alongside the existing file fact.
/// The tree-sitter parse decides whether a surface may be closed; the small
/// statement scanner only records names/lines after that decision.  This keeps
/// findings independent of a later filesystem read while retaining an open
/// result for every construct that can hide exports.
fn module_surface(file: &FileRecord) -> Value {
    let ext = file.extension().to_ascii_lowercase();
    let language = match ext.as_str() {
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        _ => return json!({"path":file.path,"language":Value::Null,"parseStatus":"unsupported","exports":[],"starReexports":[],"requests":[],"open":[]}),
    };
    let text = file.text.as_deref().unwrap_or("");
    let mut surface = json!({"path":file.path,"language":language,"parseStatus":"ok","exports":[],"starReexports":[],"requests":[],"open":[]});
    let Some(parser_language) = parser_language(&ext) else { return surface; };
    let mut parser = Parser::new();
    let language_result: tree_sitter::Language = match parser_language {
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => return surface,
    };
    if parser.set_language(&language_result).is_err() {
        surface["parseStatus"] = json!("failed");
        surface["open"] = json!([{"reason":"parse_error","line":1}]);
        return surface;
    }
    let Some(tree) = parser.parse(text, None) else {
        surface["parseStatus"] = json!("failed");
        surface["open"] = json!([{"reason":"parse_error","line":1}]);
        return surface;
    };
    if tree.root_node().has_error() {
        surface["parseStatus"] = json!("failed");
        surface["open"] = json!([{"reason":"parse_error","line":1}]);
        return surface;
    }
    let mut exports = Vec::new();
    let mut stars = Vec::new();
    let mut requests = Vec::new();
    let mut open = Vec::new();
    for (line, raw) in module_surface_statements(text) {
        let code = raw.split("//").next().unwrap_or("").trim();
        if code.is_empty() { continue; }
        if code.contains("__native_surface_incomplete__") {
            open.push(json!({"reason":"incomplete_module_syntax","line":line}));
            continue;
        }
        if let Some(specifier) = capture_re!(code, r#"^import\s+[^;]*?\sfrom\s+["']([^"']+)["']"#).or_else(|| capture_re!(code, r#"^import\s+["']([^"']+)["']"#)) {
            if let Some(default_name) = capture_re!(code, r#"^import\s+([A-Za-z_$][\w$]*)\s*(?:,|from)"#) {
                requests.push(json!({"kind":"import","name":"default","localName":default_name,"specifier":specifier,"line":line}));
            }
            if let Some(namespace_name) = capture_re!(code, r#"\*\s+as\s+([A-Za-z_$][\w$]*)"#) {
                requests.push(json!({"kind":"namespace","name":"*","localName":namespace_name,"specifier":specifier,"line":line}));
            }
            if let Some(named) = capture_re!(code, r#"\{([^}]*)\}"#) {
                for item in named.split(',').map(str::trim).filter(|item| !item.is_empty()) {
                    let mut parts = item.split_whitespace();
                    let name = parts.next().unwrap_or("");
                    if name.is_empty() { continue; }
                    let local = if parts.next() == Some("as") { parts.next().unwrap_or(name) } else { name };
                    requests.push(json!({"kind":"import","name":name,"localName":local,"specifier":specifier,"line":line}));
                }
            }
        }
        if let Some(specifier) = capture_re!(code, r#"^export\s+\*\s+(?:as\s+[A-Za-z_$][\w$]*\s+)?from\s+["']([^"']+)["']"#) {
            if let Some(name) = capture_re!(code, r#"^export\s+\*\s+as\s+([A-Za-z_$][\w$]*)"#) {
                exports.push(json!({"name":name,"line":line}));
            } else { stars.push(json!({"specifier":specifier,"line":line})); }
        } else if let Some(specifier) = capture_re!(code, r#"^export\s+\{[^}]*\}\s+from\s+["']([^"']+)["']"#) {
            if let Some(named) = capture_re!(code, r#"\{([^}]*)\}"#) {
                for item in named.split(',').map(str::trim).filter(|item| !item.is_empty()) {
                    let mut parts = item.split_whitespace();
                    let local = parts.next().unwrap_or("");
                    if local.is_empty() { continue; }
                    let alias = if parts.next() == Some("as") { parts.next().unwrap_or(local) } else { local };
                    exports.push(json!({"name":alias,"line":line}));
                    requests.push(json!({"kind":"reexport","name":local,"localName":alias,"specifier":specifier,"line":line}));
                }
            }
        } else if let Some(named) = capture_re!(code, r#"^export\s*\{([^}]*)\}"#) {
            for item in named.split(',').map(str::trim).filter(|item| !item.is_empty()) {
                let mut parts = item.split_whitespace(); let local = parts.next().unwrap_or("");
                if local.is_empty() { continue; }
                let alias = if parts.next() == Some("as") { parts.next().unwrap_or(local) } else { local };
                exports.push(json!({"name":alias,"line":line}));
            }
        } else if code.starts_with("export default") { exports.push(json!({"name":"default","line":line})); }
        else if let Some(name) = capture_re!(code, r#"^export\s+(?:async\s+)?(?:function|class|interface|type|enum)\s+([A-Za-z_$][\w$]*)"#) { exports.push(json!({"name":name,"line":line})); }
        else if let Some(names) = capture_re!(code, r#"^export\s+(?:const|let|var)\s+([A-Za-z_$][\w$]*)"#) { exports.push(json!({"name":names,"line":line})); }
        if code.contains("module.exports") || code.contains("exports.") || code.contains("exports[") { open.push(json!({"reason":"commonjs_exports","line":line})); }
        if code.starts_with("export =") { open.push(json!({"reason":"export_assignment","line":line})); }
        if code.starts_with("declare module") { open.push(json!({"reason":"ambient_module","line":line})); }
        if code.starts_with("export const {") || code.starts_with("export let {") || code.starts_with("export var {") { open.push(json!({"reason":"destructured_export","line":line})); }
    }
    exports.sort_by(|a,b| a["name"].as_str().cmp(&b["name"].as_str()).then(a["line"].as_u64().cmp(&b["line"].as_u64())));
    stars.sort_by(|a,b| a["specifier"].as_str().cmp(&b["specifier"].as_str()).then(a["line"].as_u64().cmp(&b["line"].as_u64())));
    requests.sort_by(|a,b| a["line"].as_u64().cmp(&b["line"].as_u64()).then(a["kind"].as_str().cmp(&b["kind"].as_str())).then(a["name"].as_str().cmp(&b["name"].as_str())));
    open.sort_by(|a,b| a["reason"].as_str().cmp(&b["reason"].as_str()).then(a["line"].as_u64().cmp(&b["line"].as_u64())));
    surface["exports"] = Value::Array(exports); surface["starReexports"] = Value::Array(stars); surface["requests"] = Value::Array(requests); surface["open"] = Value::Array(open);
    surface
}

/// Join multiline import/export declarations before extracting names.  A
/// parser-valid declaration that cannot be assembled remains explicitly open;
/// it must never become a closed surface through line-oriented omission.
fn module_surface_statements(text: &str) -> Vec<(usize, String)> {
    let mut statements = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let code = raw.split("//").next().unwrap_or("").trim();
        if code.is_empty() { continue; }
        if let Some((start, value)) = pending.as_mut() {
            value.push(' ');
            value.push_str(code);
            if code.contains(';') || (value.contains('}') && (value.starts_with("export ") || (value.starts_with("import ") && value.contains(" from ")))) || (value.contains(" from ") && (value.contains("\"") || value.contains("'"))) {
                statements.push((*start, value.clone()));
                pending = None;
            }
        } else if (code.starts_with("import ") || code.starts_with("export ")) && !code.contains(';') && !code.contains('}') {
            pending = Some((line, code.to_owned()));
        } else {
            statements.push((line, code.to_owned()));
        }
    }
    if let Some((start, value)) = pending {
        // An unterminated declaration is typed open, not silently discarded.
        statements.push((start, format!("{value} __native_surface_incomplete__")));
    }
    statements
}

fn lexical_facts(file: &FileRecord, text: &str, files: &BTreeMap<String, &FileRecord>, module_surface: &Value) -> (Vec<GraphNode>, Vec<GraphEdge>, FileReport) {
    let ext = file.extension().to_ascii_lowercase();
    let language = language_for_path(&file.path).unwrap_or("unknown");
    let mut nodes = Vec::new(); let mut edges = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut class_stack: Vec<(String, usize, usize)> = Vec::new();
    let mut impl_stack: Vec<(String, usize)> = Vec::new();
    let mut symbols_by_line: Vec<(String, String, usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let line_no = index + 1;
        while class_stack.last().is_some_and(|(_, end, _)| line_no > *end) { class_stack.pop(); }
        while impl_stack.last().is_some_and(|(_, end)| line_no > *end) { impl_stack.pop(); }
        if let Some(name) = capture_re!(line, r"^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)") {
            let end = block_end(&lines, index); let q = if ext == "py" { name.clone() } else { name.clone() };
            let node = symbol_node(file, "class", &name, &q, line_no, end, &[
                "Class".into(), language_label(language, line),
            ], "LEXICAL", ConfidenceTier::ExactResolution); class_stack.push((name.clone(), end, line_no)); symbols_by_line.push((name, q, line_no, "Class".into())); nodes.push(node); continue;
        }
        if let Some(name) = capture_re!(line, r"^\s*class\s+([A-Za-z_]\w*)") {
            let end = python_block_end(&lines, index); let q = class_stack.iter().map(|x| x.0.clone()).chain(std::iter::once(name.clone())).collect::<Vec<_>>().join(".");
            let node = symbol_node(file, "class", &name, &q, line_no, end, &["Class".into()], "LEXICAL", ConfidenceTier::ExactResolution);
            class_stack.push((name.clone(), end, leading_indent(line))); symbols_by_line.push((name, q, line_no, "Class".into())); nodes.push(node); continue;
        }
        if let Some(name) = capture_re!(line, r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z_]\w*)")
            .or_else(|| capture_re!(line, r"^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)"))
            .or_else(|| capture_re!(line, r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s+([A-Za-z_$][\w$]*)")) {
            let end = if ext == "py" { python_block_end(&lines, index) } else { block_end(&lines, index) };
            let (qualified, label) = if let Some((class, _, _)) = class_stack.last() { (format!("{class}.{name}"), "Method") } else if let Some((imp, _)) = impl_stack.last() { (format!("{imp}.{name}"), "Method") } else { (name.clone(), "Function") };
            nodes.push(symbol_node(file, "symbol", &name, &qualified, line_no, end, &[label.into()], "LEXICAL", ConfidenceTier::ExactResolution));
            symbols_by_line.push((name, qualified, line_no, label.into())); continue;
        }
        if let Some(name) = capture_re!(line, r"^\s*(?:export\s+)?const\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s+)?(?:\([^)]*\)|[A-Za-z_$][\w$]*)\s*=>") {
            nodes.push(symbol_node(file, "symbol", &name, &name, line_no, block_end(&lines, index), &["Function".into()], "LEXICAL", ConfidenceTier::ExactResolution)); symbols_by_line.push((name.clone(), name, line_no, "Function".into())); continue;
        }
        if let Some(name) = capture_re!(line, r"^\s*(?:export\s+)?const\s+([A-Za-z_$][\w$]*)\b") {
            nodes.push(symbol_node(file, "symbol", &name, &name, line_no, line_no, &["Const".into()], "LEXICAL", ConfidenceTier::ExactResolution)); symbols_by_line.push((name.clone(), name, line_no, "Const".into()));
        }
        if let Some(name) = capture_re!(line, r"^\s*(?:export\s+)?(?:declare\s+)?(?:interface|type|enum)\s+([A-Za-z_$][\w$]*)") {
            nodes.push(symbol_node(file, "symbol", &name, &name, line_no, line_no, &["Type".into()], "LEXICAL", ConfidenceTier::ExactResolution)); symbols_by_line.push((name.clone(), name, line_no, "Type".into()));
        }
        if let Some(name) = capture_re!(line, r"^\s*impl(?:\s*<[^>]+>)?\s+(?:[A-Za-z_]\w*(?:::\w+)*\s+for\s+)?([A-Za-z_]\w*(?:::\w+)*)") { impl_stack.push((name.rsplit("::").next().unwrap_or(&name).into(), block_end(&lines, index))); }
        if let Some(name) = capture_re!(line, r#"^\s*test\s*\(\s*["']([^"']+)"#) {
            nodes.push(symbol_node(file, "symbol", &name, &name, line_no, block_end(&lines, index), &["Test".into()], "LEXICAL", ConfidenceTier::ExactResolution)); symbols_by_line.push((name.clone(), name, line_no, "Test".into()));
        }
    }
    let file_id = format!("file:{}", file.path);
    for node in &nodes { edges.push(contains_edge(&file_id, &node.id, file, ConfidenceTier::ExactResolution)); }
    for import in import_specifiers(&ext, text) {
        let target = resolve_import(&file.path, &import, files);
        let target_id = target.as_ref().map(|path| format!("file:{path}"));
        edges.push(import_edge(&file_id, target_id.as_deref(), &import, file, module_surface));
    }
    for (name, qualified, start, _label) in symbols_by_line.iter().filter(|x| x.3 == "Function" || x.3 == "Method" || x.3 == "Test") {
        let end = nodes.iter().find(|n| n.name.as_deref() == Some(name) && n.path.as_deref() == Some(file.path.as_str()) && evidence_lines(n).0 == *start).map(|n| evidence_lines(n).1).unwrap_or(*start);
        let body = lines.get(start.saturating_sub(1)..end.min(lines.len())).unwrap_or(&[]).join("\n");
        for callee in call_names(&ext, &body) {
            if callee == *name { continue; }
            let source_id = format!("symbol:{}::{}", file.path, qualified);
            let candidates: Vec<&GraphNode> = nodes.iter().filter(|n| (n.name.as_deref() == Some(callee.as_str()) || n.name.as_deref() == Some(callee.to_ascii_lowercase().as_str())) && n.evidence.iter().any(|e| e["path"] == file.path)).collect();
            if let Some(target) = candidates.first() { edges.push(edge_record("CALLS", &source_id, Some(&target.id), ConfidenceTier::SameFileLexical, file, false, Some(&callee))); }
        }
    }
    let report = FileReport { path: file.path.clone(), language: Some(language.into()), provider: "lexical".into(), precision: PrecisionTier::Lexical, parse_status: "ok".into(), error_node_count: 0, error: None };
    (nodes, edges, report)
}

fn ast_facts(file: &FileRecord, text: &str, language: &str, cancellation: &CancellationToken) -> Result<AstResult, GraphError> {
    if cancellation.is_cancelled() { return Err(GraphError::Cancelled); }
    let mut parser = Parser::new();
    let language_result: tree_sitter::Language = match language { "javascript" => tree_sitter_javascript::LANGUAGE.into(), "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(), "python" => tree_sitter_python::LANGUAGE.into(), "rust" => tree_sitter_rust::LANGUAGE.into(), "go" => tree_sitter_go::LANGUAGE.into(), "java" => tree_sitter_java::LANGUAGE.into(), "c" => tree_sitter_c::LANGUAGE.into(), "cpp" => tree_sitter_cpp::LANGUAGE.into(), "c_sharp" => tree_sitter_c_sharp::LANGUAGE.into(), "ruby" => tree_sitter_ruby::LANGUAGE.into(), "php" => tree_sitter_php::LANGUAGE_PHP.into(), "bash" => tree_sitter_bash::LANGUAGE.into(), "kotlin" => tree_sitter_kotlin_ng::LANGUAGE.into(), "swift" => tree_sitter_swift::LANGUAGE.into(), "scala" => tree_sitter_scala::LANGUAGE.into(), "dart" => tree_sitter_dart::language(), "lua" => tree_sitter_lua::LANGUAGE.into(), "json" => tree_sitter_json::LANGUAGE.into(), "yaml" => tree_sitter_yaml::LANGUAGE.into(), "toml" => tree_sitter_toml_ng::LANGUAGE.into(), "html" => tree_sitter_html::LANGUAGE.into(), "css" => tree_sitter_css::LANGUAGE.into(), "objc" => tree_sitter_objc::LANGUAGE.into(), "ocaml" => tree_sitter_ocaml::LANGUAGE_OCAML.into(), "elixir" => tree_sitter_elixir::LANGUAGE.into(), "zig" => tree_sitter_zig::LANGUAGE.into(), "elm" => tree_sitter_elm::LANGUAGE.into(), "elisp" => membrane_grammars_vendored::elisp::LANGUAGE.into(), "embedded_template" => membrane_grammars_vendored::embedded_template::LANGUAGE.into(), "ql" => membrane_grammars_vendored::ql::LANGUAGE.into(), "rescript" => membrane_grammars_vendored::rescript::LANGUAGE.into(), "solidity" => membrane_grammars_vendored::solidity::LANGUAGE.into(), "systemrdl" => membrane_grammars_vendored::systemrdl::LANGUAGE.into(), "tlaplus" => membrane_grammars_vendored::tlaplus::LANGUAGE.into(), "vue" => membrane_grammars_vendored::vue::LANGUAGE.into(), _ => return Ok(AstResult::failed(file, "unsupported parser")), };
    if parser.set_language(&language_result).is_err() { return Ok(AstResult::failed(file, "parser language unavailable")); }
    let Some(tree) = parser.parse(text, None) else { return Ok(AstResult::failed(file, "parser returned no tree")); };
    let root = tree.root_node();
    let error_count = count_error_nodes(root, cancellation)?; let partial = root.has_error();
    let mut nodes = Vec::new(); let mut edges = Vec::new();
    if !partial { walk_ast(root, text, file, &mut nodes, &mut edges, None, language, cancellation)?; }
    for node in &nodes { edges.push(contains_edge(&format!("file:{}", file.path), &node.id, file, ConfidenceTier::ExactResolution)); }
    Ok(AstResult { nodes, edges, report: FileReport { path: file.path.clone(), language: Some(language.into()), provider: "tree-sitter".into(), precision: PrecisionTier::Ast, parse_status: if partial { "partial" } else { "ok" }.into(), error_node_count: error_count, error: if partial { Some("parse contains errors".into()) } else { None } } })
}

struct AstResult { nodes: Vec<GraphNode>, edges: Vec<GraphEdge>, report: FileReport }
impl AstResult { fn failed(file: &FileRecord, message: &str) -> Self { Self { nodes: Vec::new(), edges: Vec::new(), report: FileReport { path: file.path.clone(), language: language_for_path(&file.path).map(str::to_owned), provider: "tree-sitter".into(), precision: PrecisionTier::Ast, parse_status: "failed".into(), error_node_count: 0, error: Some(message.into()) } } } }

fn walk_ast(node: Node<'_>, source: &str, file: &FileRecord, nodes: &mut Vec<GraphNode>, edges: &mut Vec<GraphEdge>, scope: Option<String>, language: &str, cancellation: &CancellationToken) -> Result<(), GraphError> {
    if cancellation.is_cancelled() { return Err(GraphError::Cancelled); }
    let kind = node.kind();
    let declaration = matches!(kind, "function_declaration"|"function_definition"|"function_item"|"function_signature"|"method_definition"|"method_declaration"|"class_declaration"|"class_definition"|"class"|"struct_item"|"enum_item"|"trait_item"|"interface_declaration"|"type_alias_declaration"|"type_spec"|"struct_specifier"|"enum_specifier"|"union_specifier"|"namespace_definition"|"interface_body"|"module"|"singleton_method"|"method"|"function_definition_statement"|"object_declaration"|"protocol_declaration"|"typealias_declaration"|"object_definition"|"trait_definition"|"enum_definition"|"val_definition"|"var_definition"|"enum_declaration"|"mixin_declaration"|"extension_declaration"|"typedef"|"class_interface"|"class_implementation"|"value_definition"|"module_definition"|"struct_declaration"|"union_declaration"|"error_set_declaration"|"test_declaration"|"opaque_declaration"|"type_declaration"|"value_declaration"|"macro_definition"|"type_binding"|"module_binding"|"component_named_def"|"operator_definition"|"contract_declaration"|"rule_set"|"keyframes_statement"|"element"|"pair"|"block_mapping_pair");
    let name = node.child_by_field_name("name").and_then(|n| n.utf8_text(source.as_bytes()).ok()).map(str::to_owned)
        .or_else(|| if matches!(kind, "function_definition"|"declaration") { declarator_name(node, source) } else { None })
        .or_else(|| fallback_declaration_name(language, kind, node, source));
    let mut next_scope = scope.clone();
    if declaration { if let Some(raw) = name {
        let qualified = scope.as_ref().map(|s| format!("{s}.{raw}")).unwrap_or_else(|| raw.clone());
        let class = kind.contains("class") || kind.contains("struct") || kind.contains("enum") || kind.contains("trait") || kind.contains("interface") || kind.contains("contract") || matches!(kind, "type_spec"|"namespace_definition"|"module"|"type_binding"|"component_named_def");
        let label = if class { "Class" } else if scope.is_some() || kind == "method_definition" { "Method" } else { "Function" };
        let n = ast_symbol(file, if class { "class" } else { "symbol" }, &raw, &qualified, node, label);
        next_scope = Some(qualified); nodes.push(n);
    }}
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) { walk_ast(child, source, file, nodes, edges, next_scope.clone(), language, cancellation)?; }
    Ok(())
}

/// Grammars for markup/data/declarative-selector languages (css, html,
/// json, toml, yaml, objc, ocaml, dart) do not label a `name` field on the
/// node kinds their declarations use, so the generic field lookup above
/// finds nothing. Recover a syntax-true name from each grammar's own shape
/// instead of fabricating one.
fn fallback_declaration_name(language: &str, kind: &str, node: Node<'_>, source: &str) -> Option<String> {
    fn text_of(n: Node<'_>, source: &str) -> Option<String> { n.utf8_text(source.as_bytes()).ok().map(str::to_owned) }
    fn first_child_of_kind<'a>(node: Node<'a>, target: &str) -> Option<Node<'a>> {
        let mut cursor = node.walk();
        let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
        children.into_iter().find(|c| c.kind() == target)
    }
    fn first_descendant_of_kind<'a>(node: Node<'a>, target: &str) -> Option<Node<'a>> {
        if node.kind() == target { return Some(node); }
        let mut cursor = node.walk();
        let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
        children.into_iter().find_map(|c| first_descendant_of_kind(c, target))
    }
    match (language, kind) {
        ("css", "rule_set") => first_child_of_kind(node, "selectors").and_then(|n| text_of(n, source)).map(|s| s.trim().to_owned()),
        ("css", "keyframes_statement") => first_child_of_kind(node, "keyframes_name").and_then(|n| text_of(n, source)),
        ("html", "element") => first_child_of_kind(node, "start_tag").and_then(|tag| first_child_of_kind(tag, "tag_name")).and_then(|n| text_of(n, source)),
        ("json", "pair") => first_child_of_kind(node, "string").and_then(|s| first_child_of_kind(s, "string_content")).and_then(|n| text_of(n, source)),
        ("toml", "pair") => first_child_of_kind(node, "bare_key").and_then(|n| text_of(n, source)),
        ("yaml", "block_mapping_pair") => first_child_of_kind(node, "flow_node").and_then(|n| text_of(n, source)).map(|s| s.trim().to_owned()),
        ("objc", "class_interface") | ("objc", "class_implementation") => first_child_of_kind(node, "identifier").and_then(|n| text_of(n, source)),
        ("objc", "method_declaration") | ("objc", "method_definition") => first_child_of_kind(node, "identifier").and_then(|n| text_of(n, source)),
        ("ocaml", "value_definition") => first_descendant_of_kind(node, "value_name").and_then(|n| text_of(n, source)),
        ("dart", "function_signature") => first_descendant_of_kind(node, "identifier").and_then(|n| text_of(n, source)),
        ("elm", "value_declaration") => first_descendant_of_kind(node, "function_declaration_left").and_then(|left| first_child_of_kind(left, "lower_case_identifier")).and_then(|n| text_of(n, source)),
        ("ql", "module") => first_child_of_kind(node, "modulename").and_then(|n| text_of(n, source)),
        ("ql", "class") => first_child_of_kind(node, "classname").and_then(|n| text_of(n, source)),
        ("systemrdl", "component_named_def") => node.child_by_field_name("id").and_then(|n| text_of(n, source)),
        _ => None,
    }
}

/// C/C++ function definitions carry their name inside a nested `declarator`
/// chain (pointer/function declarators) rather than a direct `name` field.
/// Descend that chain to the innermost identifier.
fn declarator_name(node: Node<'_>, source: &str) -> Option<String> {
    let mut current = node.child_by_field_name("declarator")?;
    loop {
        if matches!(current.kind(), "identifier"|"field_identifier"|"type_identifier") {
            return current.utf8_text(source.as_bytes()).ok().map(str::to_owned);
        }
        if let Some(name_field) = current.child_by_field_name("declarator") {
            current = name_field;
            continue;
        }
        return None;
    }
}

fn merge_facts(lex_nodes: Vec<GraphNode>, lex_edges: Vec<GraphEdge>, ast_nodes: Vec<GraphNode>, ast_edges: Vec<GraphEdge>, compiler: Option<CompilerFacts>) -> (Vec<GraphNode>, Vec<GraphEdge>) {
    let mut nodes: BTreeMap<String, (u8, GraphNode)> = BTreeMap::new(); let mut edges: BTreeMap<String, (u8, GraphEdge)> = BTreeMap::new();
    let insert_node = |map: &mut BTreeMap<String,(u8,GraphNode)>, rank: u8, node: GraphNode| { if map.get(&node.id).is_none_or(|(old, _)| rank < *old) { map.insert(node.id.clone(), (rank,node)); } };
    let insert_edge = |map: &mut BTreeMap<String,(u8,GraphEdge)>, rank: u8, edge: GraphEdge| { if map.get(&edge.id).is_none_or(|(old, _)| rank < *old) { map.insert(edge.id.clone(), (rank,edge)); } };
    for n in lex_nodes { insert_node(&mut nodes, 2, n); } for e in lex_edges { insert_edge(&mut edges, 2, e); }
    for n in ast_nodes { insert_node(&mut nodes, 1, n); } for e in ast_edges { insert_edge(&mut edges, 1, e); }
    if let Some(facts) = compiler { for n in facts.nodes { insert_node(&mut nodes, 0, n); } for e in facts.edges { insert_edge(&mut edges, 0, e); } }
    (nodes.into_values().map(|(_,n)| n).collect(), edges.into_values().map(|(_,e)| e).collect())
}

fn resolve_edges(mut edges: Vec<GraphEdge>, nodes: &[GraphNode], files: &BTreeMap<String, &FileRecord>) -> Vec<GraphEdge> {
    let functions: HashMap<String, Vec<&GraphNode>> = nodes.iter().filter(|n| n.kind == "symbol").filter_map(|n| n.name.clone().map(|name| (name, n))).fold(HashMap::new(), |mut m, (k,v)| { m.entry(k).or_default().push(v); m });
    for edge in &mut edges {
        if edge.kind != "CALLS" || edge.target.is_some() { continue; }
        let name = edge.evidence.first().and_then(|e| e.get("callName")).and_then(Value::as_str).map(str::to_owned);
        let Some(name) = name else { continue; };
        if let Some(candidates) = functions.get(&name) {
            if candidates.len() == 1 {
                edge.target = Some(candidates[0].id.clone());
                edge.evidence.push(json!({"confidenceTier":"CROSS_FILE_HEURISTIC","confidence":ConfidenceTier::CrossFileHeuristic.score()}));
            } else if candidates.len() > 1 {
                edge.evidence.push(json!({"confidenceTier":"UNRESOLVED","confidence":0.0,"reason":"ambiguous call"}));
            }
        }
    }
    let _ = files; edges
}

fn symbol_node(file: &FileRecord, kind: &str, name: &str, qualified: &str, start: usize, end: usize, labels: &[String], provider: &str, tier: ConfidenceTier) -> GraphNode {
    GraphNode { id: format!("symbol:{}::{}", file.path, qualified), kind: kind.into(), path: Some(file.path.clone()), name: Some(name.into()), generation_id: String::new(), evidence: vec![json!({"path":file.path,"startLine":start,"endLine":end,"contentHash":file.content_hash,"labels":labels,"qualifiedName":qualified,"provider":provider,"precisionTier":if provider == "tree-sitter" { "AST" } else { "LEXICAL" },"confidenceTier":tier.as_str(),"confidence":tier.score()})] }
}
fn ast_symbol(file: &FileRecord, kind: &str, name: &str, qualified: &str, node: Node<'_>, label: &str) -> GraphNode { symbol_node(file, kind, name, qualified, node.start_position().row + 1, node.end_position().row + 1, &[label.into()], "tree-sitter", ConfidenceTier::ExactResolution) }
fn contains_edge(source: &str, target: &str, file: &FileRecord, tier: ConfidenceTier) -> GraphEdge { edge_record("CONTAINS", source, Some(target), tier, file, true, None) }
fn edge_record(kind: &str, source: &str, target: Option<&str>, tier: ConfidenceTier, file: &FileRecord, resolved: bool, call_name: Option<&str>) -> GraphEdge { let target_label = target.unwrap_or_else(|| call_name.unwrap_or("unresolved")); let id = format!("edge:{kind}:{source}->{}", target_label); GraphEdge { id, kind: kind.into(), source: source.into(), target: target.map(str::to_owned), generation_id: String::new(), evidence: vec![json!({"path":file.path,"startLine":1,"endLine":1,"contentHash":file.content_hash,"provider":"native-rust","confidenceTier":tier.as_str(),"confidence":tier.score(),"resolved":resolved,"callName":call_name})] } }
fn import_edge(source: &str, target: Option<&str>, specifier: &str, file: &FileRecord, module_surface: &Value) -> GraphEdge {
    let mut edge = edge_record("IMPORTS", source, target, if target.is_some() { ConfidenceTier::ExactResolution } else { ConfidenceTier::Unresolved }, file, target.is_some(), Some(specifier));
    if let Some(evidence) = edge.evidence.first_mut().and_then(Value::as_object_mut) {
        evidence.insert("moduleSurface".into(), module_surface.clone());
    }
    edge
}

fn parser_language(ext: &str) -> Option<&'static str> { match ext.to_ascii_lowercase().as_str() { "rs" => Some("rust"), "py" => Some("python"), "js"|"jsx"|"mjs"|"cjs" => Some("javascript"), "ts"|"mts"|"cts" => Some("typescript"), "tsx" => Some("tsx"), "go" => Some("go"), "java" => Some("java"), "c"|"h" => Some("c"), "cpp"|"cc"|"cxx"|"hpp"|"hh"|"hxx" => Some("cpp"), "cs" => Some("c_sharp"), "rb" => Some("ruby"), "php" => Some("php"), "sh"|"bash" => Some("bash"), "kt"|"kts" => Some("kotlin"), "swift" => Some("swift"), "scala"|"sc" => Some("scala"), "dart" => Some("dart"), "lua" => Some("lua"), "json" => Some("json"), "yaml"|"yml" => Some("yaml"), "toml" => Some("toml"), "html"|"htm" => Some("html"), "css" => Some("css"), "m"|"mm" => Some("objc"), "ml"|"mli" => Some("ocaml"), "ex"|"exs" => Some("elixir"), "zig" => Some("zig"), "elm" => Some("elm"), "el" => Some("elisp"), "erb" => Some("embedded_template"), "ql" => Some("ql"), "res"|"resi" => Some("rescript"), "sol" => Some("solidity"), "rdl" => Some("systemrdl"), "tla" => Some("tlaplus"), "vue" => Some("vue"), _ => None } }
fn language_for_path(path: &str) -> Option<&'static str> { parser_language(path.rsplit_once('.').map(|(_,e)| e).unwrap_or("")) }
fn is_file_only(name: &str) -> bool { matches!(name.rsplit_once('.').map(|(_,e)| e).unwrap_or(""), "md"|"markdown"|"txt"|"json"|"jsonl"|"yaml"|"yml"|"toml"|"html"|"css"|"svg"|"sql"|"csv"|"tsv") }
pub(crate) fn normalize_path(path: &str) -> String { path.replace('\\', "/").trim_start_matches("./").to_owned() }
fn compare_paths(a: &str, b: &str) -> std::cmp::Ordering { a.as_bytes().cmp(b.as_bytes()) }
fn line_count(text: &str) -> usize { text.lines().count().max(1) }
fn strip_generated_pointer(text: &str) -> String { GENERATED_POINTER_REGEX.replace(text, "").into_owned() }
fn leading_indent(line: &str) -> usize { line.chars().take_while(|c| c.is_whitespace()).count() }
fn block_end(lines: &[&str], start: usize) -> usize { let mut depth = 0usize; let mut found = false; for (i,line) in lines.iter().enumerate().skip(start) { for c in line.chars() { if c == '{' { depth += 1; found = true; } else if c == '}' { depth = depth.saturating_sub(1); } } if found && depth == 0 { return i + 1; } } lines.len().max(start + 1) }
fn python_block_end(lines: &[&str], start: usize) -> usize { let indent = leading_indent(lines[start]); lines.iter().enumerate().skip(start + 1).find(|(_, line)| !line.trim().is_empty() && leading_indent(line) <= indent).map(|(i,_)| i).unwrap_or(lines.len()) }
fn language_label(language: &str, line: &str) -> String { if line.contains("struct") { "Struct" } else if line.contains("enum") { "Enum" } else if line.contains("trait") { "Trait" } else { language }.to_owned() }
fn evidence_lines(node: &GraphNode) -> (usize,usize) { let e = node.evidence.first().cloned().unwrap_or(Value::Null); (e["startLine"].as_u64().unwrap_or(1) as usize, e["endLine"].as_u64().unwrap_or(1) as usize) }
fn source_hash(files: &[FileRecord]) -> String { let body = files.iter().filter(|f| !f.text.as_deref().unwrap_or("").starts_with("<!-- generated by blueprint")).map(|f| format!("{}:{}", f.path, f.semantic_content_hash)).collect::<Vec<_>>().join("\n"); content_digest(body.as_bytes()) }
/// Serialize the generation body in the same insertion order as the JS
/// authority.  Identity is sealed before persistence adds generation IDs, so
/// those storage-only fields are intentionally omitted here.  Do not replace
/// this with typed serde serialization: map/struct field order is not the
/// authority contract.
pub fn generation_identity_bodies(nodes: &[GraphNode], edges: &[GraphEdge]) -> (String, String) {
    let node_body = nodes.iter().map(identity_node_json).collect::<Vec<_>>().join(",");
    let edge_body = edges.iter().map(identity_edge_json).collect::<Vec<_>>().join(",");
    (format!("[{node_body}]"), format!("[{edge_body}]"))
}

fn identity_node_json(node: &GraphNode) -> String {
    // This is the insertion order of static-provider's normalized node object.
    // generationId is storage metadata and is deliberately absent from the
    // pre-seal body, exactly as in generation-identity.mjs.
    let evidence = node.evidence.first();
    let labels = evidence.and_then(|v| v.get("labels")).cloned().unwrap_or_else(|| if node.kind == "file" { json!(["File"]) } else { json!([]) });
    let qualified = evidence.and_then(|v| v.get("qualifiedName")).cloned().or_else(|| {
        // static-provider normalizes file nodes with repository-relative path
        // as qualifiedName; basename is only the display name.
        if node.kind == "file" { node.path.clone().map(Value::String) } else { node.name.clone().map(Value::String) }
    }).unwrap_or(Value::Null);
    let confidence = evidence.and_then(|v| v.get("confidence")).map(json_number_value).unwrap_or_else(|| "1".into());
    let provider = fact_provider_json(evidence.and_then(|v| v.get("provider")).and_then(Value::as_str));
    let mut fields = vec![
        format!("\"id\":{}", json_string(&node.id)), format!("\"kind\":{}", json_string(&node.kind)),
        format!("\"labels\":{}", json_value(&labels)), format!("\"name\":{}", json_value(&node.name)),
        format!("\"qualifiedName\":{}", json_value(&qualified)), format!("\"path\":{}", json_value(&node.path)),
        format!("\"confidence\":{}", confidence), format!("\"evidence\":{}", json_array(&node.evidence)),
    ];
    for key in ["provider", "grammarHash", "precisionTier", "sourceProvider"] {
        if let Some(value) = evidence.and_then(|v| v.get(key)) {
            let omit_lexical_provider = key == "provider" && matches!(value.as_str(), Some("lexical" | "native-rust"));
            if !omit_lexical_provider { fields.push(format!("\"{key}\":{}", json_value(value))); }
        }
    }
    fields.push(format!("\"factProvider\":{}", provider));
    format!("{{{}}}", fields.join(","))
}

fn identity_edge_json(edge: &GraphEdge) -> String {
    let evidence = edge.evidence.first();
    let confidence = evidence.and_then(|v| v.get("confidence")).map(json_number_value).unwrap_or_else(|| "0".into());
    let tier = evidence.and_then(|v| v.get("confidenceTier")).cloned().unwrap_or_else(|| Value::String(if edge.target.is_some() { "EXACT_RESOLUTION" } else { "UNRESOLVED" }.into()));
    let resolved = evidence.and_then(|v| v.get("resolved")).cloned().unwrap_or_else(|| Value::Bool(edge.target.is_some()));
    let specifier = evidence.and_then(|v| v.get("specifier")).cloned().or_else(|| evidence.and_then(|v| v.get("callName")).cloned()).unwrap_or(Value::Null);
    let mut fields = vec![
        format!("\"id\":{}", json_string(&edge.id)), format!("\"kind\":{}", json_string(&edge.kind)),
        format!("\"source\":{}", json_string(&edge.source)), format!("\"target\":{}", json_value(&edge.target)),
        format!("\"confidence\":{}", confidence), format!("\"confidenceTier\":{}", json_value(&tier)),
        format!("\"resolved\":{}", json_value(&resolved)), format!("\"specifier\":{}", json_value(&specifier)),
    ];
    if let Some(reason) = evidence.and_then(|v| v.get("reason")) { fields.push(format!("\"reason\":{}", json_value(reason))); }
    fields.push(format!("\"evidence\":{}", json_array(&edge.evidence)));
    for key in ["provider", "grammarHash", "precisionTier"] {
        if let Some(value) = evidence.and_then(|v| v.get(key)) {
            let omit_lexical_provider = key == "provider" && matches!(value.as_str(), Some("lexical" | "native-rust"));
            if !omit_lexical_provider { fields.push(format!("\"{key}\":{}", json_value(value))); }
        }
    }
    if let Some(value) = evidence.and_then(|v| v.get("sourceProvider")) { fields.push(format!("\"sourceProvider\":{}", json_value(value))); }
    fields.push(format!("\"factProvider\":{}", fact_provider_json(evidence.and_then(|v| v.get("provider")).and_then(Value::as_str))));
    format!("{{{}}}", fields.join(","))
}

fn fact_provider_json(provider: Option<&str>) -> String {
    let id = match provider { Some("tree-sitter") => "treesitter", Some(value) if value != "native-rust" => value, _ => "lexical" };
    let version = match id { "lexical" => "repo-local-deterministic-v4", "treesitter" => "standalone-v1", _ => PROVIDER_VERSION };
    format!("{{\"id\":{},\"version\":{}}}", json_string(id), json_string(version))
}

fn json_number_value(value: &Value) -> String {
    if let Some(number) = value.as_f64() {
        if number.fract() == 0.0 { return format!("{}", number as i64); }
    }
    serde_json::to_string(value).unwrap()
}

fn json_string(value: &str) -> String { serde_json::to_string(value).unwrap() }
fn json_value<T: serde::Serialize>(value: &T) -> String { serde_json::to_string(value).unwrap() }
fn json_array(values: &[Value]) -> String { format!("[{}]", values.iter().map(|value| serde_json::to_string(value).unwrap()).collect::<Vec<_>>().join(",")) }

fn symbol_names(text: &str) -> Vec<String> { SYMBOL_NAMES_REGEX.find_iter(text).map(|m| m.as_str().to_owned()).collect() }
fn call_names(ext: &str, body: &str) -> Vec<String> { let mut out = BTreeSet::new(); for c in CALL_NAMES_REGEX.captures_iter(body) { if let Some(v)=c.get(1) { out.insert(v.as_str().to_owned()); } } if matches!(ext, "py"|"rs") { out.extend(symbol_names(body).into_iter().filter(|v| body.contains(&format!("{v}(")))); } out.into_iter().collect() }
// import_specifiers/resolve_import were ported to
// `crate::module_resolution::{import_specifiers_in_files, resolve_import_in_files}`
// (lane P4, 2026-09-10) — a disk-free relocation of this exact logic, not a
// behavior change. See that module's "In-memory build-pass adapter" section
// for the equivalence rationale and `tests/parity_module_resolution_wired.rs`
// for the proof.
use crate::module_resolution::{import_specifiers_in_files as import_specifiers, resolve_import_in_files as resolve_import};
fn count_error_nodes(root: Node<'_>, cancellation: &CancellationToken) -> Result<usize, GraphError> {
    if cancellation.is_cancelled() { return Err(GraphError::Cancelled); }
    let mut count = usize::from(root.is_error());
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) { count += count_error_nodes(child, cancellation)?; }
    Ok(count)
}

pub fn registered_relationship_kinds() -> &'static [&'static str] { &["COVERS","GENERATES","IMPORTS","CALLS","CONTAINS","REFERENCES","DEFINES","CONFIGURES","READS","WRITES","PRODUCES","CONSUMES","DEPLOYS","HANDLES","ROUTES_TO","AUTHORED_BY","READ_DURING","CHANGED_BY","DOCS_LINK","TESTS"] }
pub fn is_registered_relationship_kind(kind: &str) -> bool { registered_relationship_kinds().contains(&kind) }
pub fn confidence_from_resolution_path(path: &str) -> ConfidenceTier { match path { "compiler"|"scip" => ConfidenceTier::ExactResolution, "same-file" => ConfidenceTier::SameFileLexical, "cross-file-heuristic" => ConfidenceTier::CrossFileHeuristic, _ => ConfidenceTier::Unresolved } }
