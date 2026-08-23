//! Immutable request-bound context passed to every native provider.

use crate::source::{ScopeGrantView, SourceQuery, SourceSet};
use membrane_protocol::{FederationRequestV1, FreshnessSnapshotV1};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// The validated grant view exposed to providers.  It is intentionally a
/// narrow alias rather than the signing/envelope type.
pub type ValidatedScopeGrantView = ScopeGrantView;

/// Immutable provider input.  Composition creates one instance per request;
/// providers receive references and cannot replace request identity, grant,
/// freshness, deadline, or source handles.
#[derive(Clone, Debug)]
pub struct ProviderContext {
    pub request_id: String,
    pub repository_root: String,
    pub repository_id: String,
    pub task: String,
    pub session_id: String,
    pub client: String,
    pub anchors: Vec<String>,
    pub scope_grant: Option<ValidatedScopeGrantView>,
    pub release_generation: Option<String>,
    pub freshness: FreshnessSnapshotV1,
    /// Absolute monotonic deadline.  It is never serialized or reset by a
    /// provider.
    pub deadline: Instant,
    pub cancellation: CancellationToken,
    pub trace_id: String,
    pub sources: SourceSet,
}

impl ProviderContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: impl Into<String>,
        repository_root: impl Into<String>,
        repository_id: impl Into<String>,
        task: impl Into<String>,
        session_id: impl Into<String>,
        client: impl Into<String>,
        anchors: Vec<String>,
        scope_grant: Option<ValidatedScopeGrantView>,
        release_generation: Option<String>,
        freshness: FreshnessSnapshotV1,
        deadline: Instant,
        cancellation: CancellationToken,
        trace_id: impl Into<String>,
        sources: SourceSet,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            repository_root: repository_root.into(),
            repository_id: repository_id.into(),
            task: task.into(),
            session_id: session_id.into(),
            client: client.into(),
            anchors,
            scope_grant,
            release_generation,
            freshness,
            deadline,
            cancellation,
            trace_id: trace_id.into(),
            sources,
        }
    }

    /// Construct from the serialized federation request after the caller has
    /// validated root, budget, grant, and freshness.
    pub fn from_request(
        request: &FederationRequestV1,
        repository_id: impl Into<String>,
        scope_grant: Option<ValidatedScopeGrantView>,
        freshness: FreshnessSnapshotV1,
        deadline: Instant,
        cancellation: CancellationToken,
        sources: SourceSet,
    ) -> Self {
        Self::new(
            request.request_id.clone(),
            request.repository_root.clone(),
            repository_id,
            request.task.clone(),
            request.session_id.clone(),
            request.client.clone(),
            request.anchors.clone(),
            scope_grant,
            request.release_generation.clone(),
            freshness,
            deadline,
            cancellation,
            request.trace_id.clone(),
            sources,
        )
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub fn is_deadline_exhausted(&self) -> bool {
        Instant::now() >= self.deadline
    }

    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    pub fn query(&self) -> SourceQuery {
        SourceQuery {
            request_id: self.request_id.clone(),
            repository_id: self.repository_id.clone(),
            repository_root: self.repository_root.clone(),
            task: self.task.clone(),
            session_id: self.session_id.clone(),
            generation: self.release_generation.clone(),
            anchors: self.anchors.clone(),
        }
    }
}
