//! Native Git metadata provider.
//!
//! The provider deliberately keeps its surface small: repository discovery,
//! HEAD interpretation, and candidate construction live behind one adapter.
//! No process is launched for routine metadata.  A repository or object
//! failure becomes content-free lane accounting rather than a fabricated
//! branch or revision.

use membrane_protocol::{
    CandidateV1, FederationProviderStatusV1, FreshnessClass, FreshnessSnapshotV1,
    ProviderDiagnosticsV1, ProviderId,
    ProviderOmissionV1, ProviderOutputV1, ProviderWarningV1, ReasonCode,
    WarningSeverity, PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use membrane_provider_sdk::{Provider, ProviderContext, ProviderError, ProviderOutput};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const PROVIDER: ProviderId = ProviderId::Git;
const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Stable, content-free failures emitted by the native repository adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitProviderError {
    NotRepository,
    Repository(String),
    MissingHeadObject,
}

impl std::fmt::Display for GitProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRepository => f.write_str("not_a_repository"),
            Self::Repository(_) => f.write_str("repository_unavailable"),
            Self::MissingHeadObject => f.write_str("head_object_missing"),
        }
    }
}

impl std::error::Error for GitProviderError {}

/// Small repository adapter.  Keeping gix here prevents provider composition
/// from acquiring a second Git implementation or a process-launch fallback.
pub struct RepositoryAdapter {
    root: PathBuf,
    repository: gix::Repository,
}

impl RepositoryAdapter {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, GitProviderError> {
        let root = root.into();
        let repository = gix::open(&root).map_err(|error| {
            let detail = error.to_string();
            let lower = detail.to_ascii_lowercase();
            if !root.join(".git").exists()
                || lower.contains("not a git repository")
                || lower.contains("repository not found")
            {
                GitProviderError::NotRepository
            } else {
                GitProviderError::Repository(detail)
            }
        })?;
        Ok(Self { root, repository })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read symbolic HEAD name and peeled object without invoking Git.
    pub fn head(&self) -> Result<HeadMetadata, GitProviderError> {
        let head = self
            .repository
            .head()
            .map_err(|error| GitProviderError::Repository(error.to_string()))?;
        let branch = head
            .referent_name()
            .map(|name| name.shorten().to_string());
        let revision = match head.id() {
            Some(id) => {
                let value = id.to_string();
                id.object()
                    .map_err(|_| GitProviderError::MissingHeadObject)?;
                Some(value)
            }
            None => None,
        };
        if revision.is_none() && branch.is_none() {
            return Err(GitProviderError::MissingHeadObject);
        }
        Ok(HeadMetadata { branch, revision })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadMetadata {
    pub branch: Option<String>,
    pub revision: Option<String>,
}

/// Produce the native Git lane for one repository root.
pub fn produce(root: impl AsRef<Path>) -> ProviderOutputV1 {
    let mut output = empty_output();
    let adapter = match RepositoryAdapter::open(root.as_ref()) {
        Ok(adapter) => adapter,
        Err(error) => {
            fail_output(&mut output, &error);
            return output;
        }
    };
    let head = match adapter.head() {
        Ok(head) => head,
        Err(error) => {
            fail_output(&mut output, &error);
            return output;
        }
    };

    if let Some(branch) = head.branch.as_deref() {
        output.candidates.push(metadata_candidate(
            format!("git:meta:branch:{branch}"),
            format!("git:branch:{branch}"),
            ZERO_HASH,
            0.4,
            0.5,
            format!("current branch {branch}"),
        ));
    } else {
        output.candidates.push(metadata_candidate(
            "git:meta:branch:HEAD".to_owned(),
            "git:branch:HEAD".to_owned(),
            ZERO_HASH,
            0.4,
            0.5,
            "detached HEAD".to_owned(),
        ));
        warning(&mut output, "head_detached", ReasonCode::ProviderUnavailable);
    }
    if let Some(revision) = head.revision.as_deref() {
        output.candidates.push(metadata_candidate(
            format!("git:meta:head:{}", &revision[..revision.len().min(12)]),
            format!("git:commit:{revision}"),
            revision,
            0.3,
            0.3,
            format!("HEAD commit {}", &revision[..revision.len().min(12)]),
        ));
    } else {
        warning(&mut output, "head_unborn", ReasonCode::ProviderUnavailable);
        output.omissions.push(ProviderOmissionV1 {
            provider: PROVIDER,
            reason: ReasonCode::ProviderUnavailable,
            candidate_id: None,
            detail_id: Some("head_unborn".to_owned()),
            stage: Some("repository".to_owned()),
        });
    }
    output
}

/// Attach freshness-owner worktree classification and provenance to Git
/// metadata. Dirty state comes from the request-bound source observation;
/// this lane does not introduce a second Git status implementation.
pub fn produce_with_freshness(
    root: impl AsRef<Path>,
    freshness: &FreshnessSnapshotV1,
) -> ProviderOutputV1 {
    let mut output = produce(root);
    apply_freshness(&mut output, freshness);
    output
}

fn apply_freshness(output: &mut ProviderOutputV1, freshness: &FreshnessSnapshotV1) {
    output.generation = freshness.generation.clone();
    let classification = match freshness.graph_state.as_str() {
        "dirty_overlay" => FreshnessClass::DirtyOverlay,
        "fresh" | "clean" => FreshnessClass::Current,
        "stale_snapshot" => FreshnessClass::StaleSnapshot,
        _ => FreshnessClass::Unknown,
    };
    for candidate in &mut output.candidates {
        candidate.freshness_class = Some(classification);
        candidate.base_commit = freshness.base_commit.clone();
        candidate.overlay_digest = freshness.overlay_digest.clone();
    }
    if let Some(diagnostics) = output.diagnostics.as_mut() {
        diagnostics
            .attributes
            .insert("graphState".to_owned(), freshness.graph_state.clone());
        diagnostics
            .attributes
            .insert("provenance".to_owned(), "freshness_source".to_owned());
        if let Some(base) = freshness.base_commit.as_deref() {
            diagnostics
                .attributes
                .insert("baseCommit".to_owned(), base.to_owned());
        }
        if let Some(overlay) = freshness.overlay_digest.as_deref() {
            diagnostics
                .attributes
                .insert("overlayDigest".to_owned(), overlay.to_owned());
        }
    }
    if freshness.graph_state == "dirty_overlay" && freshness.overlay_digest.is_none() {
        warning(output, "dirty_overlay_digest_missing", ReasonCode::FreshnessUnavailable);
        output.omissions.push(ProviderOmissionV1 {
            provider: PROVIDER,
            reason: ReasonCode::FreshnessUnavailable,
            candidate_id: None,
            detail_id: Some("dirty_overlay_digest_missing".to_owned()),
            stage: Some("freshness".to_owned()),
        });
    }
}

fn empty_output() -> ProviderOutputV1 {
    ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: PROVIDER,
        status: FederationProviderStatusV1::Complete,
        generation: None,
        candidates: Vec::new(),
        warnings: Vec::new(),
        omissions: Vec::new(),
        diagnostics: Some(ProviderDiagnosticsV1 {
            provider: PROVIDER,
            elapsed_ms: None,
            generation: None,
            attributes: BTreeMap::new(),
        }),
        extensions: BTreeMap::new(),
    }
}

fn metadata_candidate(
    id: String,
    source_ref: String,
    source_hash: &str,
    provider_score: f64,
    relevance_score: f64,
    text: String,
) -> CandidateV1 {
    let resolver = source_ref_to_resolver(&source_ref);
    CandidateV1 {
        id,
        layer: 1,
        provider: Some(PROVIDER.as_str().to_owned()),
        source_kind: "git_meta".to_owned(),
        source_ref,
        source_hash: source_hash.to_owned(),
        trust_class: "workspace_tracked".to_owned(),
        instruction_policy: "data_only".to_owned(),
        provider_score,
        score_components: BTreeMap::from([(String::from("git_relevance"), relevance_score)]),
        base_commit: None,
        overlay_digest: None,
        freshness_class: None,
        snapshot_id: None,
        estimated_tokens: 8,
        protected: false,
        exact: true,
        recoverable: true,
        resolver,
        text,
    }
}

fn source_ref_to_resolver(source_ref: &str) -> String {
    if source_ref.starts_with("git:branch:") {
        "git rev-parse --abbrev-ref HEAD".to_owned()
    } else if source_ref.starts_with("git:commit:") {
        "git log -1 --format=%H".to_owned()
    } else {
        "git metadata unavailable".to_owned()
    }
}

fn warning(output: &mut ProviderOutputV1, detail_id: &str, reason: ReasonCode) {
    output.status = FederationProviderStatusV1::Partial;
    output.warnings.push(ProviderWarningV1 {
        provider: PROVIDER,
        reason,
        severity: WarningSeverity::Warning,
        detail_id: Some(detail_id.to_owned()),
        stage: Some("repository".to_owned()),
        message: None,
    });
}

fn fail_output(output: &mut ProviderOutputV1, error: &GitProviderError) {
    output.status = if matches!(error, GitProviderError::NotRepository) {
        FederationProviderStatusV1::Partial
    } else {
        FederationProviderStatusV1::Failed
    };
    let (reason, detail_id) = match error {
        GitProviderError::NotRepository => (ReasonCode::ProviderUnavailable, "not_a_repository"),
        GitProviderError::MissingHeadObject => (ReasonCode::ProviderMalformed, "head_object_missing"),
        GitProviderError::Repository(_) => (ReasonCode::ProviderFailed, "repository_open_failed"),
    };
    output.warnings.push(ProviderWarningV1 {
        provider: PROVIDER,
        reason,
        severity: WarningSeverity::Warning,
        detail_id: Some(detail_id.to_owned()),
        stage: Some("repository".to_owned()),
        message: None,
    });
    output.omissions.push(ProviderOmissionV1 {
        provider: PROVIDER,
        reason,
        candidate_id: None,
        detail_id: Some(detail_id.to_owned()),
        stage: Some("repository".to_owned()),
    });
    if let Some(diagnostics) = output.diagnostics.as_mut() {
        diagnostics
            .attributes
            .insert("failure".to_owned(), error.to_string());
    }
}

/// Native federation adapter used by the frozen provider registry.
#[derive(Debug, Default, Clone, Copy)]
pub struct GitProvider;

#[async_trait::async_trait]
impl Provider for GitProvider {
    async fn provide(&self, context: &ProviderContext) -> Result<ProviderOutput, ProviderError> {
        if context.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        if context.is_deadline_exhausted() {
            return Err(ProviderError::DeadlineExceeded);
        }
        Ok(produce_with_freshness(&context.repository_root, &context.freshness))
    }
}
