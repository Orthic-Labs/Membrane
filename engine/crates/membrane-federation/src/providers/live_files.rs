//! Bounded working-tree overlay provider.
//!
//! The lane consumes the request-bound freshness verdict and exact grant paths.
//! Working-tree bytes are read only after path confinement and grant checks;
//! committed bytes are obtained through the native Git object API.  A failed
//! path never turns into partial or guessed content.

use membrane_protocol::{
    digest_str, CandidateV1, FederationProviderStatusV1, FreshnessClass, ProviderDiagnosticsV1,
    ProviderId, ProviderOmissionV1, ProviderOutputV1, ProviderWarningV1, ReasonCode,
    WarningSeverity, PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use membrane_provider_sdk::{CapabilityV1, Provider, ProviderContext, ProviderError, ProviderOutput};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const PROVIDER_ID: ProviderId = ProviderId::LiveFiles;
pub const DEFAULT_MAX_PATHS: usize = 64;
pub const MAX_HISTORICAL_BLOB_BYTES: usize = 1024 * 1024;
pub const MAX_WORKING_TREE_BYTES: usize = 1024 * 1024;
pub const MAX_OVERLAY_PATH_CHARS: usize = 1024;

/// One status entry from the freshness owner.  `content_hash` is the hash
/// observed by that owner, so a later read can detect a changed verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayEntry {
    pub path: String,
    pub old_path: Option<String>,
    pub status: String,
    pub content_hash: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileReadError {
    Unavailable,
    Oversized,
    Binary,
}

pub trait WorkingTreeSource: Send + Sync {
    fn read(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, FileReadError>;
    fn exists(&self, path: &Path) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FilesystemWorkingTree;

impl WorkingTreeSource for FilesystemWorkingTree {
    fn read(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, FileReadError> {
        let file = File::open(path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => FileReadError::Unavailable,
            _ => FileReadError::Unavailable,
        })?;
        let mut bytes = Vec::new();
        file.take((max_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| FileReadError::Unavailable)?;
        if bytes.len() > max_bytes {
            return Err(FileReadError::Oversized);
        }
        if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
            return Err(FileReadError::Binary);
        }
        Ok(bytes)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists() || path.is_symlink()
    }
}

pub trait GitObjectSource: Send + Sync {
    fn read_blob(&self, repository_root: &Path, commit: &str, path: &str, max_bytes: usize)
        -> Result<Vec<u8>, FileReadError>;
}

/// Native Git object access.  This deliberately does not invoke `git` or
/// expose a shell/argv fallback.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeGitObjectSource;

impl GitObjectSource for NativeGitObjectSource {
    fn read_blob(
        &self,
        repository_root: &Path,
        commit: &str,
        path: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, FileReadError> {
        let repository = gix::open(repository_root).map_err(|_| FileReadError::Unavailable)?;
        // Freshness carries a canonical, full SHA-1 object id,
        // not a revspec.  Resolve it directly so historical reads remain
        // available with gix's default-features=false configuration.
        let commit_id = gix::ObjectId::from_hex(commit.as_bytes())
            .map_err(|_| FileReadError::Unavailable)?;
        let commit = repository
            .find_object(commit_id)
            .map_err(|_| FileReadError::Unavailable)?;
        let commit = commit.try_into_commit().map_err(|_| FileReadError::Unavailable)?;
        let mut tree = commit.tree().map_err(|_| FileReadError::Unavailable)?;
        let entry = tree
            .peel_to_entry_by_path(Path::new(path))
            .map_err(|_| FileReadError::Unavailable)?
            .ok_or(FileReadError::Unavailable)?;
        let object = entry.object().map_err(|_| FileReadError::Unavailable)?;
        let blob = object.try_into_blob().map_err(|_| FileReadError::Unavailable)?;
        if blob.data.len() > max_bytes {
            return Err(FileReadError::Oversized);
        }
        if blob.data.contains(&0) || std::str::from_utf8(&blob.data).is_err() {
            return Err(FileReadError::Binary);
        }
        Ok(blob.data.to_vec())
    }
}

pub struct LiveFilesProvider {
    working_tree: Arc<dyn WorkingTreeSource>,
    git_objects: Arc<dyn GitObjectSource>,
    max_paths: usize,
}

impl std::fmt::Debug for LiveFilesProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LiveFilesProvider")
            .field("max_paths", &self.max_paths)
            .finish_non_exhaustive()
    }
}

impl Default for LiveFilesProvider {
    fn default() -> Self {
        Self::new(Arc::new(FilesystemWorkingTree), Arc::new(NativeGitObjectSource))
    }
}

impl LiveFilesProvider {
    pub fn new(
        working_tree: Arc<dyn WorkingTreeSource>,
        git_objects: Arc<dyn GitObjectSource>,
    ) -> Self {
        Self { working_tree, git_objects, max_paths: DEFAULT_MAX_PATHS }
    }

    pub fn with_max_paths(mut self, max_paths: usize) -> Self {
        self.max_paths = max_paths.min(DEFAULT_MAX_PATHS);
        self
    }

    pub fn produce_entries(&self, context: &ProviderContext, entries: &[OverlayEntry]) -> ProviderOutputV1 {
        produce_entries(context, entries, self.max_paths, self.working_tree.as_ref(), self.git_objects.as_ref())
    }
}

#[async_trait::async_trait]
impl Provider for LiveFilesProvider {
    async fn provide(&self, context: &ProviderContext) -> Result<ProviderOutput, ProviderError> {
        if context.is_cancelled() {
            return Ok(boundary_output(FederationProviderStatusV1::Cancelled, ReasonCode::ProviderCancelled, "request_cancelled"));
        }
        if context.is_deadline_exhausted() {
            return Ok(boundary_output(FederationProviderStatusV1::Cancelled, ReasonCode::ProviderTimeout, "deadline_exhausted"));
        }
        let Some(grant) = context.scope_grant.as_ref() else {
            return Ok(boundary_output(FederationProviderStatusV1::Failed, ReasonCode::ScopeGrantMissing, "grant_required"));
        };
        if grant.repository_id != context.repository_id
            || grant.session_id != context.session_id
        {
            return Ok(boundary_output(FederationProviderStatusV1::Failed, ReasonCode::ScopeGrantInvalid, "grant_binding_mismatch"));
        }
        let entries = grant
            .read_paths
            .iter()
            .filter_map(|raw| normalize_grant_path(raw))
            .map(|path| OverlayEntry { path, old_path: None, status: " M".to_owned(), content_hash: String::new() })
            .collect::<Vec<_>>();
        Ok(self.produce_entries(context, &entries))
    }

    fn list_capabilities(&self) -> Vec<CapabilityV1> {
        Vec::new()
    }
}

pub fn produce_entries(
    context: &ProviderContext,
    entries: &[OverlayEntry],
    max_paths: usize,
    working_tree: &dyn WorkingTreeSource,
    git_objects: &dyn GitObjectSource,
) -> ProviderOutputV1 {
    let mut output = empty_output(context.freshness.generation.clone());
    let Some(base_commit) = context.freshness.base_commit.as_deref().filter(|value| !value.trim().is_empty()) else {
        return boundary_output(FederationProviderStatusV1::Partial, ReasonCode::FreshnessUnavailable, "base_commit_missing");
    };
    let Some(overlay_digest) = context.freshness.overlay_digest.as_deref().filter(|value| !value.trim().is_empty()) else {
        return boundary_output(FederationProviderStatusV1::Partial, ReasonCode::FreshnessUnavailable, "overlay_digest_missing");
    };
    if context.freshness.graph_state != "dirty_overlay" {
        output.warnings.push(warning(ReasonCode::ProviderUnavailable, "overlay_not_dirty"));
        output.status = FederationProviderStatusV1::Partial;
        return output;
    }
    let mut normalized = Vec::new();
    for entry in entries {
        let Some(path) = normalize_overlay_path(&entry.path) else {
            output.warnings.push(warning(ReasonCode::ScopeGrantInvalid, "dirty_overlay_path_rejected"));
            output.omissions.push(omission("dirty_overlay_path_rejected", ReasonCode::ScopeGrantInvalid));
            continue;
        };
        normalized.push((path, entry));
    }
    normalized.sort_by(|left, right| left.0.cmp(&right.0));
    let cap = max_paths.min(DEFAULT_MAX_PATHS);
    if normalized.len() > cap {
        output.warnings.push(warning(ReasonCode::ProviderMalformed, "dirty_overlay_truncated"));
        output.omissions.push(omission("dirty_overlay_truncated", ReasonCode::ProviderMalformed));
    }
    let root = Path::new(&context.repository_root);
    for (path, entry) in normalized.into_iter().take(cap) {
        if context.is_cancelled() {
            output.status = FederationProviderStatusV1::Cancelled;
            output.omissions.push(omission("request_cancelled", ReasonCode::ProviderCancelled));
            break;
        }
        if context.is_deadline_exhausted() {
            output.status = FederationProviderStatusV1::Partial;
            output.omissions.push(omission("deadline_exhausted", ReasonCode::ProviderTimeout));
            break;
        }
        let Some(absolute) = confined_path(root, &path) else {
            output.warnings.push(warning(ReasonCode::ScopeGrantInvalid, "dirty_overlay_path_rejected"));
            output.omissions.push(omission("dirty_overlay_path_rejected", ReasonCode::ScopeGrantInvalid));
            continue;
        };
        if !valid_sha256(&entry.content_hash) {
            output.warnings.push(warning(ReasonCode::ProviderMalformed, "dirty_overlay_hash_rejected"));
            output.omissions.push(omission("dirty_overlay_hash_rejected", ReasonCode::ProviderMalformed));
            continue;
        }
        let deleted = entry.status.contains('D');
        let working_hash = if deleted {
            if working_tree.exists(&absolute) {
                output.omissions.push(omission("dirty_overlay_changed", ReasonCode::ProviderUnavailable));
                continue;
            }
            let hash = digest_str(&format!("deleted:{path}"));
            if entry.content_hash != hash {
                output.omissions.push(omission("dirty_overlay_changed", ReasonCode::ProviderUnavailable));
                continue;
            }
            hash
        } else {
            let bytes = match working_tree.read(&absolute, MAX_WORKING_TREE_BYTES) {
                Ok(bytes) => bytes,
                Err(error) => {
                    output.omissions.push(read_omission(error));
                    continue;
                }
            };
            let value = String::from_utf8(bytes).expect("working tree source rejects binary bytes");
            let hash = digest_str(&value);
            if entry.content_hash != hash {
                output.omissions.push(omission("dirty_overlay_changed", ReasonCode::ProviderUnavailable));
                continue;
            }
            hash
        };
        let snapshot_path = if let Some(old_path) = entry.old_path.as_deref() {
            let Some(old_path) = normalize_overlay_path(old_path) else {
                output.warnings.push(warning(ReasonCode::ScopeGrantInvalid, "dirty_overlay_old_path_rejected"));
                output.omissions.push(omission("dirty_overlay_old_path_rejected", ReasonCode::ScopeGrantInvalid));
                continue;
            };
            old_path
        } else {
            path.clone()
        };
        let is_new = entry.status.contains('?') || entry.status.contains('A');
        if !is_new {
            match git_objects.read_blob(root, base_commit, &snapshot_path, MAX_HISTORICAL_BLOB_BYTES) {
                Ok(bytes) => {
                    let value = String::from_utf8(bytes).expect("Git object source rejects binary bytes");
                    output.candidates.push(snapshot_candidate(base_commit, overlay_digest, &snapshot_path, &value));
                }
                Err(error) => {
                    output.omissions.push(read_omission(error));
                }
            }
        }
        output.candidates.push(overlay_candidate(base_commit, overlay_digest, &path, &working_hash, &entry.status));
    }
    output.status = if output.omissions.is_empty() { FederationProviderStatusV1::Complete } else { FederationProviderStatusV1::Partial };
    output
}

fn snapshot_candidate(base: &str, digest: &str, path: &str, text: &str) -> CandidateV1 {
    CandidateV1 {
        id: format!("git-snapshot:{}:{path}", &base[..base.len().min(12)]), layer: 3,
        provider: Some(PROVIDER_ID.as_str().to_owned()), source_kind: "repo_code".to_owned(),
        source_ref: format!("{path}@{}", &base[..base.len().min(12)]), source_hash: digest_str(text),
        trust_class: "workspace_tracked".to_owned(), instruction_policy: "data_only".to_owned(),
        provider_score: 0.45, score_components: BTreeMap::from([(String::from("git_snapshot_relevance"), 0.45), (String::from("freshness"), 0.5)]),
        base_commit: Some(base.to_owned()), overlay_digest: Some(digest.to_owned()), freshness_class: Some(FreshnessClass::CommittedSnapshot), snapshot_id: None,
        estimated_tokens: 24, protected: false, exact: true, recoverable: true, resolver: format!("git show {base}:{path}"), text: format!("committed snapshot path={path}"),
    }
}

fn overlay_candidate(base: &str, digest: &str, path: &str, hash: &str, status: &str) -> CandidateV1 {
    CandidateV1 {
        id: format!("git-overlay:{path}"), layer: 3, provider: Some(PROVIDER_ID.as_str().to_owned()), source_kind: "live_overlay".to_owned(), source_ref: path.to_owned(), source_hash: hash.to_owned(),
        trust_class: "workspace_tracked".to_owned(), instruction_policy: "data_only".to_owned(), provider_score: 0.7,
        score_components: BTreeMap::from([(String::from("git_overlay_relevance"), 0.7), (String::from("freshness"), 1.0)]),
        base_commit: Some(base.to_owned()), overlay_digest: Some(digest.to_owned()), freshness_class: Some(FreshnessClass::DirtyOverlay), snapshot_id: None,
        estimated_tokens: 32, protected: false, exact: false, recoverable: true, resolver: format!("git diff --no-ext-diff {base} -- {path}"), text: format!("working-tree status={} path={path}", status),
    }
}

fn normalize_grant_path(raw: &str) -> Option<String> {
    let path = raw.split_once(':').map(|(path, _)| path).unwrap_or(raw);
    normalize_overlay_path(path)
}

fn valid_sha256(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else { return false };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn normalize_overlay_path(value: &str) -> Option<String> {
    let raw = value.replace('\\', "/");
    if raw.is_empty() || raw.len() > MAX_OVERLAY_PATH_CHARS || raw != raw.trim() || raw.starts_with('/') || raw.bytes().any(|byte| byte < 32 || byte == 127) {
        return None;
    }
    let mut parts = Vec::new();
    for part in raw.split('/') {
        if part.is_empty() || part == "." || part == ".." { return None; }
        parts.push(part);
    }
    let normalized = parts.join("/");
    (!normalized.starts_with(".agent/") && !normalized.starts_with(".blueprint/") && normalized.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._@+-/".contains(&byte))).then_some(normalized)
}

fn confined_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let canonical_root = root.canonicalize().ok()?;
    let mut candidate = canonical_root.clone();
    for component in Path::new(relative).components() {
        match component { Component::Normal(value) => candidate.push(value), _ => return None }
    }
    if candidate.exists() && candidate.canonicalize().ok()?.strip_prefix(&canonical_root).is_err() { return None; }
    if candidate.exists() && has_symlink_component(&canonical_root, &candidate) { return None; }
    Some(candidate)
}

fn has_symlink_component(root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(root).ok();
    let Some(relative) = relative else { return true };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        if std::fs::symlink_metadata(&current).map(|meta| meta.file_type().is_symlink()).unwrap_or(true) { return true; }
    }
    false
}

fn empty_output(generation: Option<String>) -> ProviderOutputV1 {
    ProviderOutputV1 { schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION, provider: PROVIDER_ID, status: FederationProviderStatusV1::Partial, generation, candidates: Vec::new(), warnings: Vec::new(), omissions: Vec::new(), diagnostics: Some(ProviderDiagnosticsV1 { provider: PROVIDER_ID, elapsed_ms: None, generation: None, attributes: BTreeMap::new() }), extensions: BTreeMap::new() }
}

fn boundary_output(status: FederationProviderStatusV1, reason: ReasonCode, detail: &str) -> ProviderOutputV1 {
    let mut output = empty_output(None);
    output.status = status;
    output.warnings.push(warning(reason, detail));
    output.omissions.push(omission(detail, reason));
    output
}

fn warning(reason: ReasonCode, detail: &str) -> ProviderWarningV1 { ProviderWarningV1 { provider: PROVIDER_ID, reason, severity: WarningSeverity::Warning, detail_id: Some(detail.to_owned()), stage: Some("live_overlay".to_owned()), message: None } }
fn omission(detail: &str, reason: ReasonCode) -> ProviderOmissionV1 { ProviderOmissionV1 { provider: PROVIDER_ID, reason, candidate_id: None, detail_id: Some(detail.to_owned()), stage: Some("live_overlay".to_owned()) } }
fn read_omission(error: FileReadError) -> ProviderOmissionV1 { omission(match error { FileReadError::Unavailable => "overlay_unavailable", FileReadError::Oversized => "overlay_oversized", FileReadError::Binary => "overlay_binary" }, ReasonCode::ProviderMalformed) }
