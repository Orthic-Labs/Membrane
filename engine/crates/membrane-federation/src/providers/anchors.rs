//! Explicit file and symbol anchors.
//!
//! File anchors are resolved only after canonical path confinement and an
//! exact ScopeGrant read-path check.  Semantic anchors remain Blueprint-owned:
//! this provider forwards them through the request-aware Blueprint adapter
//! and never guesses symbols locally.

use crate::blueprint_client::ContextualBlueprintSource;
use membrane_protocol::{
    CandidateV1, FederationProviderStatusV1, ProviderId, ProviderOmissionV1, ProviderOutputV1,
    ProviderWarningV1, ReasonCode, WarningSeverity, PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use membrane_provider_sdk::{
    Provider, ProviderContext, ProviderError, ProviderOutput, SourceResponse,
};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const MAX_ANCHOR_BYTES: usize = 1_500;
pub const MAX_ANCHOR_CANDIDATES: usize = 32;

/// File bytes are supplied through this narrow adapter so composition can
/// replace local reads with a grant-aware host implementation.
pub trait AnchorFileSource: Send + Sync {
    fn read_file(
        &self,
        canonical_path: &Path,
        max_bytes: usize,
    ) -> Result<Vec<u8>, AnchorFileError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorFileError {
    Unavailable,
    ReadFailed,
    Oversized,
}

/// Default bounded local file reader.  It performs no path authorization;
/// authorization is owned by [`AnchorsProvider`] and happens first.
#[derive(Clone, Copy, Debug, Default)]
pub struct FilesystemAnchorSource;

impl AnchorFileSource for FilesystemAnchorSource {
    fn read_file(
        &self,
        canonical_path: &Path,
        max_bytes: usize,
    ) -> Result<Vec<u8>, AnchorFileError> {
        let file = File::open(canonical_path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
                AnchorFileError::Unavailable
            }
            _ => AnchorFileError::ReadFailed,
        })?;
        let mut bytes = Vec::new();
        file.take((max_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| AnchorFileError::ReadFailed)?;
        if bytes.len() > max_bytes {
            return Err(AnchorFileError::Oversized);
        }
        Ok(bytes)
    }
}

/// Native anchors lane.  `source` is injected to keep path reads behind one
/// composition seam and to make grant/read ordering observable in tests.
pub struct AnchorsProvider {
    source: Arc<dyn AnchorFileSource>,
    blueprint: Option<Arc<dyn ContextualBlueprintSource>>,
}

impl std::fmt::Debug for AnchorsProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AnchorsProvider")
            .field("source", &"injected")
            .field("blueprint_context", &self.blueprint.is_some())
            .finish()
    }
}

impl Clone for AnchorsProvider {
    fn clone(&self) -> Self {
        Self {
            source: Arc::clone(&self.source),
            blueprint: self.blueprint.clone(),
        }
    }
}

impl Default for AnchorsProvider {
    fn default() -> Self {
        Self::with_source(FilesystemAnchorSource)
    }
}

impl AnchorsProvider {
    pub fn new(source: Arc<dyn AnchorFileSource>) -> Self {
        Self {
            source,
            blueprint: None,
        }
    }

    /// Attach the request-aware Blueprint adapter used for live symbol
    /// resolution.  A plain `BlueprintSource` is intentionally not accepted:
    /// it cannot carry caller deadline or cancellation.
    pub fn with_blueprint_source(mut self, source: Arc<dyn ContextualBlueprintSource>) -> Self {
        self.blueprint = Some(source);
        self
    }

    pub fn with_source<S>(source: S) -> Self
    where
        S: AnchorFileSource + 'static,
    {
        Self::new(Arc::new(source))
    }

    pub fn source(&self) -> &Arc<dyn AnchorFileSource> {
        &self.source
    }
}

#[async_trait::async_trait]
impl Provider for AnchorsProvider {
    async fn provide(&self, context: &ProviderContext) -> Result<ProviderOutput, ProviderError> {
        resolve_with_contextual(context, self.source.as_ref(), self.blueprint.as_deref()).await
    }
}

/// Resolve all explicit anchors in stable input order.
pub async fn resolve(
    context: &ProviderContext,
    source: &dyn AnchorFileSource,
) -> Result<ProviderOutput, ProviderError> {
    resolve_with_contextual(context, source, None).await
}

async fn resolve_with_contextual(
    context: &ProviderContext,
    source: &dyn AnchorFileSource,
    blueprint: Option<&dyn ContextualBlueprintSource>,
) -> Result<ProviderOutput, ProviderError> {
    let mut candidates = Vec::new();
    let mut warnings = Vec::new();
    let mut omissions = Vec::new();
    let mut generation = context.freshness.generation.clone();

    if context.is_cancelled() {
        return Err(ProviderError::Cancelled);
    }
    if context.is_deadline_exhausted() {
        return Err(ProviderError::DeadlineExceeded);
    }
    if context.anchors.is_empty() {
        omissions.push(anchor_omission(
            "",
            "anchors_empty",
            ReasonCode::ProviderUnavailable,
        ));
    }

    for anchor in context.anchors.iter().take(MAX_ANCHOR_CANDIDATES) {
        if context.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        if context.is_deadline_exhausted() {
            return Err(ProviderError::DeadlineExceeded);
        }
        if anchor.trim().is_empty() {
            omissions.push(anchor_omission(
                anchor,
                "anchor_empty",
                ReasonCode::ProviderMalformed,
            ));
            continue;
        }

        match classify_path(&context.repository_root, anchor) {
            AnchorPath::File { path, relative } => {
                if !grant_allows(context, &relative) {
                    candidates.push(raw_candidate(anchor));
                    omissions.push(anchor_omission(
                        anchor,
                        "anchor_read_not_granted",
                        ReasonCode::ScopeGrantInvalid,
                    ));
                    continue;
                }
                match source.read_file(&path, MAX_ANCHOR_BYTES) {
                    Ok(bytes) if bytes.len() <= MAX_ANCHOR_BYTES => {
                        candidates.push(file_candidate(anchor, &relative, &bytes))
                    }
                    Ok(_) | Err(AnchorFileError::Oversized) => {
                        candidates.push(raw_candidate(anchor));
                        omissions.push(anchor_omission(
                            anchor,
                            "anchor_oversized",
                            ReasonCode::ProviderMalformed,
                        ));
                    }
                    Err(_) => {
                        candidates.push(raw_candidate(anchor));
                        omissions.push(anchor_omission(
                            anchor,
                            "anchor_unreadable",
                            ReasonCode::ProviderUnavailable,
                        ));
                    }
                }
            }
            AnchorPath::Rejected => {
                candidates.push(raw_candidate(anchor));
                omissions.push(anchor_omission(
                    anchor,
                    "anchor_outside_scope",
                    ReasonCode::InvalidRoot,
                ));
                warnings.push(ProviderWarningV1 {
                    provider: ProviderId::Anchors,
                    reason: ReasonCode::InvalidRoot,
                    severity: WarningSeverity::Warning,
                    detail_id: Some("anchor_unresolved".to_owned()),
                    stage: Some("anchor_resolution".to_owned()),
                    message: None,
                });
            }
            AnchorPath::Symbol(symbol) => {
                let Some(blueprint) = blueprint else {
                    candidates.push(raw_candidate(anchor));
                    omissions.push(anchor_omission(
                        anchor,
                        "blueprint_source_missing",
                        ReasonCode::ProviderUnavailable,
                    ));
                    continue;
                };
                match blueprint
                    .resolve_symbol_with_context(
                        &context.query(),
                        &symbol,
                        context.deadline,
                        context.cancellation.clone(),
                    )
                    .await
                {
                    Ok(response) => {
                        if let Some(observed) = response.generation.clone() {
                            if generation.is_none() {
                                generation = Some(observed);
                            }
                        }
                        append_source_warnings(&mut warnings, &response);
                        let resolved = response.value.candidates;
                        if resolved.is_empty() {
                            candidates.push(raw_candidate(anchor));
                            omissions.push(anchor_omission(
                                anchor,
                                "anchor_unresolved",
                                ReasonCode::ProviderUnavailable,
                            ));
                        } else {
                            for mut candidate in resolved {
                                candidate.provider = Some(ProviderId::Anchors.as_str().to_owned());
                                candidates.push(candidate);
                            }
                        }
                    }
                    Err(_) => {
                        candidates.push(raw_candidate(anchor));
                        omissions.push(anchor_omission(
                            anchor,
                            "anchor_unresolved",
                            ReasonCode::ProviderUnavailable,
                        ));
                    }
                }
            }
        }
    }

    if context.anchors.len() > MAX_ANCHOR_CANDIDATES {
        omissions.push(anchor_omission(
            "",
            "anchor_candidate_cap",
            ReasonCode::ProviderMalformed,
        ));
    }
    let status = if warnings.is_empty() && omissions.is_empty() {
        FederationProviderStatusV1::Complete
    } else {
        FederationProviderStatusV1::Partial
    };
    Ok(ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: ProviderId::Anchors,
        status,
        generation,
        candidates,
        warnings,
        omissions,
        diagnostics: None,
        extensions: BTreeMap::new(),
    })
}

fn append_source_warnings(
    warnings: &mut Vec<ProviderWarningV1>,
    response: &SourceResponse<membrane_provider_sdk::BlueprintResult>,
) {
    warnings.extend(response.warnings.iter().map(|warning| ProviderWarningV1 {
        provider: ProviderId::Anchors,
        reason: ReasonCode::ProviderUnavailable,
        severity: membrane_protocol::WarningSeverity::Warning,
        detail_id: warning.detail_id.clone(),
        stage: Some("blueprint_resolution".to_owned()),
        message: None,
    }));
}

#[derive(Debug, PartialEq, Eq)]
pub enum AnchorPath {
    File { path: PathBuf, relative: String },
    Symbol(String),
    Rejected,
}

/// Classify an anchor without reading source content.  Existing files become
/// file anchors; all other in-scope values are semantic anchors for Blueprint.
pub fn classify_path(repository_root: &str, anchor: &str) -> AnchorPath {
    let raw = anchor.strip_prefix("file:").unwrap_or(anchor);
    let symbol = anchor.strip_prefix("symbol:").unwrap_or(anchor).trim();
    if anchor.starts_with("symbol:") {
        return if symbol.is_empty() {
            AnchorPath::Rejected
        } else {
            AnchorPath::Symbol(symbol.to_owned())
        };
    }
    if looks_windows_absolute(raw) {
        return AnchorPath::Rejected;
    }
    let root = Path::new(repository_root);
    let Some(root) = root.canonicalize().ok() else {
        return AnchorPath::Rejected;
    };
    let requested = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        root.join(raw)
    };
    if !lexically_contained(&root, &requested) {
        return AnchorPath::Rejected;
    }
    if !requested.is_file() {
        return AnchorPath::Symbol(symbol.to_owned());
    }
    if has_symlink_component(&root, &requested) {
        return AnchorPath::Rejected;
    }
    let Ok(path) = requested.canonicalize() else {
        return AnchorPath::Rejected;
    };
    let Ok(relative) = path.strip_prefix(&root).map(normalize_relative) else {
        return AnchorPath::Rejected;
    };
    AnchorPath::File { path, relative }
}

fn lexically_contained(root: &Path, requested: &Path) -> bool {
    let Ok(relative) = requested.strip_prefix(root) else {
        return false;
    };
    let mut depth = 0usize;
    for component in relative.components() {
        match component {
            Component::RootDir | Component::Prefix(_) | Component::CurDir => {}
            Component::ParentDir => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            Component::Normal(_) => depth += 1,
        }
    }
    true
}

fn has_symlink_component(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        if fs::symlink_metadata(&current)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(true)
        {
            return true;
        }
    }
    false
}

fn normalize_relative(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn looks_windows_absolute(value: &str) -> bool {
    value.starts_with("\\\\")
        || (value.len() >= 3
            && value.as_bytes()[1] == b':'
            && matches!(value.as_bytes()[2], b'/' | b'\\'))
}

fn grant_allows(context: &ProviderContext, relative: &str) -> bool {
    let Some(grant) = context.scope_grant.as_ref() else {
        return false;
    };
    grant.read_paths.iter().any(|entry| {
        let (path, _) = entry.rsplit_once(':').unwrap_or((entry.as_str(), ""));
        normalize_relative(Path::new(path)) == relative
    })
}

pub fn raw_candidate(anchor: &str) -> CandidateV1 {
    CandidateV1 {
        id: format!("anchor:raw:{anchor}"),
        layer: 1,
        provider: Some(ProviderId::Anchors.as_str().to_owned()),
        source_kind: "anchor".to_owned(),
        source_ref: anchor.to_owned(),
        source_hash: format!("sha256:{}", "0".repeat(64)),
        trust_class: "user_direct".to_owned(),
        instruction_policy: "data_only".to_owned(),
        provider_score: 0.6,
        score_components: BTreeMap::from([(String::from("anchor_relevance"), 0.6)]),
        base_commit: None,
        overlay_digest: None,
        freshness_class: None,
        snapshot_id: None,
        estimated_tokens: 8,
        protected: true,
        exact: true,
        recoverable: true,
        resolver: format!("anchor {anchor}"),
        text: anchor.to_owned(),
    }
}

fn file_candidate(anchor: &str, relative: &str, bytes: &[u8]) -> CandidateV1 {
    let text: String = String::from_utf8_lossy(bytes)
        .chars()
        .take(MAX_ANCHOR_BYTES)
        .collect();
    CandidateV1 {
        id: format!("anchor:file:{relative}"),
        layer: 3,
        provider: Some(ProviderId::Anchors.as_str().to_owned()),
        source_kind: "anchor".to_owned(),
        source_ref: relative.to_owned(),
        source_hash: membrane_protocol::digest_str(&text),
        trust_class: "user_direct".to_owned(),
        instruction_policy: "data_only".to_owned(),
        provider_score: 0.95,
        score_components: BTreeMap::from([(String::from("anchor_relevance"), 1.0)]),
        base_commit: None,
        overlay_digest: None,
        freshness_class: None,
        snapshot_id: None,
        estimated_tokens: (text.chars().count() / 4).max(1) as u32,
        protected: true,
        exact: true,
        recoverable: true,
        resolver: format!("anchor resolve {anchor}"),
        text,
    }
}

fn anchor_omission(anchor: &str, detail: &str, reason: ReasonCode) -> ProviderOmissionV1 {
    ProviderOmissionV1 {
        provider: ProviderId::Anchors,
        reason,
        candidate_id: (!anchor.is_empty()).then(|| format!("anchor:raw:{anchor}")),
        detail_id: Some(detail.to_owned()),
        stage: Some("anchor_resolution".to_owned()),
    }
}
