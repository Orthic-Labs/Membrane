//! Doc Spine candidate observation at the planner boundary.
//!
//! This module deliberately runs beside, never inside, planner admission. Document candidates
//! stay shadow-only until replay evidence authorizes a separate live-admission change.

use crypt_core::planner::{plan, PlannerError, PlannerInput, PlannerOutput};
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

/// Explicitly records that document candidates were not supplied to planner admission.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PlannerDocShadowObservationV1 {
    pub provider: String,
    pub admitted_to_planner: bool,
    pub selection: DocCandidateShadowSelectionV1,
}

#[derive(Debug, Serialize)]
pub struct PlannerWithDocShadowV1 {
    pub planner: PlannerOutput,
    pub doc_shadow: PlannerDocShadowObservationV1,
}

/// Opt-in live candidate path: when enabled, document candidates are surfaced in the reviewed-learning queue.
/// Disabled by default (is_doc_provider_enabled() == false → identical output to pre-unit state).
pub fn maybe_admit_doc_candidates(request: &DocCandidateProviderRequestV1, selection: &DocCandidateShadowSelectionV1) -> Option<Vec<DocCandidateProviderCandidateV1>> {
    if !is_doc_provider_enabled() {
        return None;
    }
    if selection.candidates.is_empty() {
        return None;
    }
    Some(selection.candidates.clone())
}

/// Run normal planning, then attach a packet-neutral Doc Spine shadow observation.
pub fn plan_with_doc_shadow(
    planner_input: &PlannerInput,
    provider: &impl DocCandidateProvider,
    request: DocCandidateProviderRequestV1,
) -> Result<PlannerWithDocShadowV1, PlannerError> {
    let planner = plan(planner_input)?;
    let selection = provider.select_shadow(&request);
    Ok(PlannerWithDocShadowV1 {
        planner,
        doc_shadow: PlannerDocShadowObservationV1 {
            provider: DOC_CANDIDATE_PROVIDER_NAME.to_owned(),
            admitted_to_planner: false,
            selection,
        },
    })
}
