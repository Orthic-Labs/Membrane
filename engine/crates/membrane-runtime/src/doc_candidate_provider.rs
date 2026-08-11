//! Doc Spine candidate observation at the planner boundary.
//!
//! This module deliberately runs beside, never inside, planner admission. Document candidates
//! stay shadow-only until replay evidence authorizes a separate live-admission change.

use serde::{Deserialize, Serialize};

use crate::doc_shadow::{
    select_doc_candidates_for_shadow, DocCandidateFreshnessV1, DocCandidateProviderCandidateV1,
    DocCandidateProviderPolicyV1, DocCandidateShadowSelectionV1, DocTaskClassV1,
};

pub const DOC_CANDIDATE_PROVIDER_NAME: &str = "doc_spine";

/// Opt-in flag: document candidates are shadow-only unless explicitly enabled.
/// Default OFF — no behavior change for existing installs until `MEMBRANE_DOC_PROVIDER_ENABLED=1`.
pub fn is_doc_provider_enabled() -> bool {
    std::env::var("MEMBRANE_DOC_PROVIDER_ENABLED").map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false)
}

/// Input owned by the document provider; it never joins the planner candidate set.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DocCandidateProviderRequestV1 {
    pub task_class: DocTaskClassV1,
    pub current: DocCandidateFreshnessV1,
    pub candidates: Vec<DocCandidateProviderCandidateV1>,
    pub policy: DocCandidateProviderPolicyV1,
}

impl DocCandidateProviderRequestV1 {
    pub fn new(
        task_class: DocTaskClassV1,
        current: DocCandidateFreshnessV1,
        candidates: Vec<DocCandidateProviderCandidateV1>,
    ) -> Self {
        Self {
            task_class,
            current,
            candidates,
            policy: DocCandidateProviderPolicyV1::default(),
        }
    }
}

/// Minimal provider seam for a future replay-gated live candidate path.
pub trait DocCandidateProvider {
    fn select_shadow(
        &self,
        request: &DocCandidateProviderRequestV1,
    ) -> DocCandidateShadowSelectionV1;
}

/// Registered Doc Spine candidates use the existing exact-freshness and task-class primitives.
pub struct RegisteredDocCandidateProvider;

impl DocCandidateProvider for RegisteredDocCandidateProvider {
    fn select_shadow(
        &self,
        request: &DocCandidateProviderRequestV1,
    ) -> DocCandidateShadowSelectionV1 {
        select_doc_candidates_for_shadow(
            &request.policy,
            request.task_class,
            &request.current,
            &request.candidates,
        )
    }
}

