//! Ledger candidate observation at the planner boundary.
//!
//! This module provides Ledger's operator/debug shadow-selection surface. The
//! native Ledger provider supplies source-bound candidates to Pull directly;
//! this diagnostic surface remains outside planner admission.

use serde::{Deserialize, Serialize};

use super::doc_shadow::{
    select_doc_candidates_for_shadow, DocCandidateFreshnessV1, DocCandidateProviderCandidateV1,
    DocCandidateProviderPolicyV1, DocCandidateShadowSelectionV1, DocTaskClassV1,
};

pub const DOC_CANDIDATE_PROVIDER_NAME: &str = "ledger";

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

/// Registered Ledger candidates use exact-freshness and task-class primitives.
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
