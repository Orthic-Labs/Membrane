//! Deterministic federation candidate merge.
//!
//! Merge is intentionally boring: normalize and admit lanes first, group by
//! stable identity, collapse exact duplicates, omit conflicts, then apply the
//! selected deterministic strategy. Bounded RRF is production default; fixed
//! provider/security order remains an explicit comparison control.

use crate::normalize::{normalize_provider_output, NormalizedCandidate, NormalizedProviderOutput};
use crate::omission::{
    canonical_omissions, canonical_warnings, conflict_omission, reconcile_lanes,
};
use membrane_core::{FusionBounds, DEFAULT_MAX_ITEMS, DEFAULT_RRF_K};
use membrane_protocol::{
    canonical_json_of, CandidateV1, ContextCandidateSetV1, FederationProviderStatusV1,
    FederationResponseV1, FederationStatus, FreshnessV1, FusionReceiptV1, ProviderCeilingV1,
    ProviderId, ProviderOmissionV1, ProviderOutputV1, ProviderWarningV1, ReasonCode,
    WarningSeverity, FEDERATION_RESPONSE_SCHEMA_VERSION,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// Deterministic fusion strategy selected by the composition owner.
///
/// RRF is production default. Fixed order remains an explicit comparison and
/// recovery control; neither strategy can admit a provider or make final
/// Membrane packet-selection decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FusionStrategy {
    FixedOrder,
    Rrf,
}

impl Default for FusionStrategy {
    fn default() -> Self {
        Self::Rrf
    }
}

impl FusionStrategy {
    pub const fn policy(self) -> &'static str {
        match self {
            Self::FixedOrder => FusionReceiptV1::POLICY,
            Self::Rrf => FusionReceiptV1::RRF_POLICY,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MergeResult {
    pub candidates: Vec<NormalizedCandidate>,
    pub warnings: Vec<ProviderWarningV1>,
    pub omissions: Vec<ProviderOmissionV1>,
    pub providers: Vec<NormalizedProviderOutput>,
    pub fusion_receipt: FusionReceiptV1,
}

impl MergeResult {
    pub fn canonicalize(&mut self) {
        let fused_ranks = self
            .fusion_receipt
            .decisions
            .iter()
            .filter(|decision| decision.decision == "selected")
            .filter_map(|decision| {
                decision
                    .fused_rank
                    .map(|rank| ((decision.provider.clone(), decision.id.clone()), rank))
            })
            .collect::<BTreeMap<_, _>>();
        self.candidates.sort_by(|left, right| {
            let left_rank = left
                .candidate
                .provider
                .as_deref()
                .and_then(|provider| fused_ranks.get(&(provider.to_owned(), left.id().to_owned())))
                .copied()
                .unwrap_or(u32::MAX);
            let right_rank = right
                .candidate
                .provider
                .as_deref()
                .and_then(|provider| fused_ranks.get(&(provider.to_owned(), right.id().to_owned())))
                .copied()
                .unwrap_or(u32::MAX);
            left_rank
                .cmp(&right_rank)
                .then_with(|| candidate_order(left, right))
        });
        self.providers
            .sort_by_key(|provider| provider.provider.rank());
        self.warnings = canonical_warnings(std::mem::take(&mut self.warnings));
        self.omissions = canonical_omissions(std::mem::take(&mut self.omissions));
    }

    pub fn is_partial(&self) -> bool {
        !self.warnings.is_empty() || !self.omissions.is_empty()
    }

    pub fn response(
        &self,
        request_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> FederationResponseV1 {
        let mut response = FederationResponseV1 {
            schema_version: FEDERATION_RESPONSE_SCHEMA_VERSION,
            request_id: request_id.into(),
            trace_id: trace_id.into(),
            status: if self.is_partial() {
                FederationStatus::Partial
            } else {
                FederationStatus::Complete
            },
            providers: self.providers.iter().map(provider_output).collect(),
            candidates: self
                .candidates
                .iter()
                .map(|candidate| candidate.candidate.clone())
                .collect(),
            warnings: self.warnings.clone(),
            omissions: self.omissions.clone(),
            diagnostics: None,
            error: None,
            extensions: BTreeMap::new(),
        };
        response.extensions.insert(
            "fusionReceipt".to_owned(),
            serde_json::to_value(&self.fusion_receipt).unwrap_or(serde_json::Value::Null),
        );
        let fused_candidates = response.candidates.clone();
        response.canonicalize_collections();
        response.candidates = fused_candidates;
        response
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MergeError {
    #[error("provider output normalization failed: {0}")]
    Normalization(#[from] crate::normalize::CandidateNormalizationError),
    #[error("provider output does not contain expected lane")]
    MissingProvider,
}

/// Normalize all expected provider lanes, perform generation admission before
/// dedupe, and merge deterministic candidates.
pub fn merge_outputs(
    expected: &[ProviderId],
    outputs: &[ProviderOutputV1],
    expected_generation: Option<&str>,
) -> Result<MergeResult, MergeError> {
    merge_outputs_with_strategy(
        expected,
        outputs,
        expected_generation,
        FusionStrategy::default(),
    )
}

/// Merge provider outputs with an explicitly selected fusion strategy.
pub fn merge_outputs_with_strategy(
    expected: &[ProviderId],
    outputs: &[ProviderOutputV1],
    expected_generation: Option<&str>,
    strategy: FusionStrategy,
) -> Result<MergeResult, MergeError> {
    let accounting = reconcile_lanes(expected, outputs, expected_generation)?;
    Ok(merge_normalized_with_strategy(
        accounting.outputs,
        accounting.warnings,
        accounting.omissions,
        strategy,
    ))
}

/// Merge already-normalized lanes.  This is the integration seam for a
/// scheduler that has performed lane accounting itself.
pub fn merge_normalized(
    providers: Vec<NormalizedProviderOutput>,
    warnings: Vec<ProviderWarningV1>,
    omissions: Vec<ProviderOmissionV1>,
) -> MergeResult {
    merge_normalized_with_strategy(providers, warnings, omissions, FusionStrategy::default())
}

/// Merge already-normalized lanes with an explicitly selected strategy.
pub fn merge_normalized_with_strategy(
    providers: Vec<NormalizedProviderOutput>,
    mut warnings: Vec<ProviderWarningV1>,
    mut omissions: Vec<ProviderOmissionV1>,
    strategy: FusionStrategy,
) -> MergeResult {
    let mut by_id: BTreeMap<String, Vec<NormalizedCandidate>> = BTreeMap::new();
    for provider in &providers {
        for candidate in &provider.candidates {
            by_id
                .entry(candidate.id().to_owned())
                .or_default()
                .push(candidate.clone());
        }
    }

    let mut candidates = Vec::new();
    for (id, mut entries) in by_id {
        entries.sort_by(candidate_order);
        let first = entries.first().expect("group is never empty");
        if entries
            .iter()
            .all(|entry| equivalent_identity(first, entry))
        {
            candidates.push(first.clone());
            continue;
        }

        // The sealed parity corpus defines duplicate IDs across provider
        // lanes as canonical first-wins: provider order is the authority for
        // deterministic identity ownership.  Conflicting observations within
        // one provider remain unsafe and are omitted below.
        let first_provider = first.provider;
        let first_provider_entries = entries
            .iter()
            .filter(|entry| entry.provider == first_provider)
            .collect::<Vec<_>>();
        if first_provider_entries
            .iter()
            .all(|entry| equivalent_identity(first_provider_entries[0], entry))
        {
            candidates.push(first.clone());
            continue;
        }

        // A conflict has no safe winner.  Exclude every identity and leave a
        // content-free receipt for each involved lane.
        let mut providers_seen = BTreeSet::new();
        for entry in entries {
            if providers_seen.insert(entry.provider.as_str().to_owned()) {
                omissions.push(conflict_omission(entry.provider, id.clone()));
                warnings.push(ProviderWarningV1 {
                    provider: entry.provider,
                    reason: ReasonCode::CandidateIdentityConflict,
                    severity: WarningSeverity::Warning,
                    detail_id: Some(id.clone()),
                    stage: Some("merge".to_owned()),
                    message: None,
                });
            }
        }
    }

    let (candidates, fusion_receipt) = fuse_normalized(&providers, candidates, strategy);
    let mut result = MergeResult {
        candidates,
        warnings,
        omissions,
        providers,
        fusion_receipt,
    };
    result.canonicalize();
    result
}

/// Apply the selected strategy after lane normalization, generation admission,
/// id conflict handling, and exact duplicate collapse.
fn fuse_normalized(
    providers: &[NormalizedProviderOutput],
    candidates: Vec<NormalizedCandidate>,
    strategy: FusionStrategy,
) -> (Vec<NormalizedCandidate>, FusionReceiptV1) {
    match strategy {
        FusionStrategy::FixedOrder => fuse_fixed_normalized(providers, candidates),
        FusionStrategy::Rrf => fuse_rrf_normalized(providers, candidates),
    }
}

/// Preserve the comparison merge control: provider rank first, then
/// protected/exact identity fields and stable candidate fields.
fn fuse_fixed_normalized(
    providers: &[NormalizedProviderOutput],
    mut candidates: Vec<NormalizedCandidate>,
) -> (Vec<NormalizedCandidate>, FusionReceiptV1) {
    candidates.sort_by(candidate_order);
    let mut provider_order = providers
        .iter()
        .map(|provider| provider.provider.as_str().to_owned())
        .collect::<Vec<_>>();
    provider_order.sort_by_key(|provider| {
        ProviderId::parse(provider)
            .map(|value| value.rank())
            .unwrap_or(usize::MAX)
    });
    let mut provider_quotas = BTreeMap::new();
    for provider in providers {
        let count = candidates
            .iter()
            .filter(|candidate| candidate.provider == provider.provider)
            .count();
        provider_quotas.insert(
            provider.provider.as_str().to_owned(),
            u32::try_from(count).unwrap_or(u32::MAX),
        );
    }
    let mut provider_ranks = BTreeMap::<String, u32>::new();
    let mut decisions = Vec::with_capacity(candidates.len());
    for (index, candidate) in candidates.iter().enumerate() {
        let provider = candidate.provider.as_str().to_owned();
        let rank = provider_ranks
            .entry(provider.clone())
            .and_modify(|value| *value = value.saturating_add(1))
            .or_insert(1);
        decisions.push(membrane_protocol::FusionDecisionV1 {
            id: candidate.id().to_owned(),
            provider,
            provider_rank: *rank,
            rrf_denominator: DEFAULT_RRF_K.saturating_add(*rank),
            fused_rank: Some(u32::try_from(index + 1).unwrap_or(u32::MAX)),
            decision: "selected".to_owned(),
            reason: "fixed_order_control".to_owned(),
        });
    }
    decisions.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.provider_rank.cmp(&right.provider_rank))
            .then_with(|| left.id.cmp(&right.id))
    });
    let count = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
    (
        candidates,
        FusionReceiptV1 {
            schema_version: FusionReceiptV1::SCHEMA_VERSION,
            policy: FusionStrategy::FixedOrder.policy().to_owned(),
            fallback_policy: FusionReceiptV1::FALLBACK_POLICY.to_owned(),
            provider_order,
            provider_quotas,
            rrf_k: DEFAULT_RRF_K,
            max_items: count,
            candidates_received: count,
            candidates_selected: count,
            decisions,
        },
    )
}

/// Run bounded core RRF as production fusion.
/// Federation owns provider eligibility; core owns rank fusion and receipt
/// decisions.
fn fuse_rrf_normalized(
    providers: &[NormalizedProviderOutput],
    candidates: Vec<NormalizedCandidate>,
) -> (Vec<NormalizedCandidate>, FusionReceiptV1) {
    let mut by_provider: BTreeMap<String, Vec<NormalizedCandidate>> = BTreeMap::new();
    for candidate in candidates {
        by_provider
            .entry(candidate.provider.as_str().to_owned())
            .or_default()
            .push(candidate);
    }

    let mut sets = Vec::new();
    let mut quotas = BTreeMap::new();
    for (provider, entries) in &by_provider {
        let candidates = entries
            .iter()
            .map(|entry| entry.candidate.clone())
            .collect::<Vec<CandidateV1>>();
        quotas.insert(
            provider.clone(),
            u32::try_from(candidates.len()).unwrap_or(u32::MAX),
        );
        let estimated_tokens = candidates.iter().fold(0u32, |total, candidate| {
            total.saturating_add(candidate.estimated_tokens)
        });
        let generation = providers
            .iter()
            .find(|value| value.provider.as_str() == provider)
            .and_then(|value| value.generation.clone())
            .unwrap_or_default();
        sets.push(ContextCandidateSetV1 {
            schema_version: 1,
            trace_id: "fusion".to_owned(),
            indexed_at: generation.clone(),
            task: "fusion".to_owned(),
            mode: "native".to_owned(),
            provider: provider.clone(),
            freshness: FreshnessV1 {
                revision: generation.clone(),
                indexed_at: generation,
                stale: false,
                graph_state: None,
                snapshot_id: None,
                base_commit: None,
                overlay_digest: None,
                expected_release_generation: None,
                observed_release_generation: None,
                release_generation_status: None,
                overlay_identity: None,
            },
            provider_ceiling: ProviderCeilingV1 {
                max_candidates: u32::try_from(candidates.len()).unwrap_or(u32::MAX),
                max_estimated_tokens: estimated_tokens,
            },
            candidates,
            omissions: Vec::new(),
        });
    }

    let fused = membrane_core::fuse(
        &sets,
        FusionBounds {
            provider_quotas: quotas,
            rrf_k: DEFAULT_RRF_K,
            // Fusion has a hard candidate-processing bound. The Membrane
            // planner still owns final grant, sufficiency, attention budget,
            // representation, & publication authority.
            max_items: DEFAULT_MAX_ITEMS,
        },
    );

    let mut by_key = by_provider;
    let mut normalized = Vec::with_capacity(fused.candidates.len());
    for candidate in fused.candidates {
        let Some(provider) = candidate.provider.as_deref().and_then(ProviderId::parse) else {
            continue;
        };
        let Some(entries) = by_key.get_mut(provider.as_str()) else {
            continue;
        };
        if let Some(index) = entries
            .iter()
            .position(|entry| entry.candidate.id == candidate.id)
        {
            normalized.push(entries.remove(index));
        }
    }
    (normalized, fused.receipt)
}

/// Convenience response-producing entry point used by resident composition.
pub fn merge_provider_outputs(
    request_id: impl Into<String>,
    trace_id: impl Into<String>,
    expected: &[ProviderId],
    outputs: &[ProviderOutputV1],
    expected_generation: Option<&str>,
) -> Result<FederationResponseV1, MergeError> {
    Ok(merge_outputs(expected, outputs, expected_generation)?.response(request_id, trace_id))
}

/// Response-producing merge with an explicit fusion strategy.
pub fn merge_provider_outputs_with_strategy(
    request_id: impl Into<String>,
    trace_id: impl Into<String>,
    expected: &[ProviderId],
    outputs: &[ProviderOutputV1],
    expected_generation: Option<&str>,
    strategy: FusionStrategy,
) -> Result<FederationResponseV1, MergeError> {
    Ok(
        merge_outputs_with_strategy(expected, outputs, expected_generation, strategy)?
            .response(request_id, trace_id),
    )
}

pub fn canonical_response_bytes(response: &FederationResponseV1) -> String {
    let mut response = response.clone();
    let fused_candidates = response
        .extensions
        .contains_key("fusionReceipt")
        .then(|| response.candidates.clone());
    response.canonicalize_collections();
    if let Some(fused_candidates) = fused_candidates {
        response.candidates = fused_candidates;
    }
    canonical_json_of(&response)
}

pub fn candidate_order(left: &NormalizedCandidate, right: &NormalizedCandidate) -> Ordering {
    left.provider
        .rank()
        .cmp(&right.provider.rank())
        .then_with(|| right.candidate.protected.cmp(&left.candidate.protected))
        .then_with(|| right.candidate.exact.cmp(&left.candidate.exact))
        .then_with(|| left.candidate.id.cmp(&right.candidate.id))
        .then_with(|| left.candidate.source_hash.cmp(&right.candidate.source_hash))
        .then_with(|| left.candidate.source_ref.cmp(&right.candidate.source_ref))
        .then_with(|| left.candidate.text.cmp(&right.candidate.text))
        .then_with(|| left.canonical_bytes().cmp(&right.canonical_bytes()))
}

fn equivalent_identity(left: &NormalizedCandidate, right: &NormalizedCandidate) -> bool {
    left.provider == right.provider
        && left.provider_version == right.provider_version
        && left.generation == right.generation
        && left.provenance == right.provenance
        && left.semantic_bytes() == right.semantic_bytes()
}

fn provider_output(provider: &NormalizedProviderOutput) -> ProviderOutputV1 {
    ProviderOutputV1 {
        schema_version: provider.schema_version,
        provider: provider.provider,
        status: provider.status,
        generation: provider.generation.clone(),
        candidates: provider
            .candidates
            .iter()
            .map(|candidate| candidate.candidate.clone())
            .collect(),
        warnings: provider.warnings.clone(),
        omissions: provider.omissions.clone(),
        diagnostics: None,
        extensions: BTreeMap::new(),
    }
}

/// Normalize one output for schedulers that want to retain explicit lane
/// status without performing a full merge.
pub fn normalize_lane(
    output: &ProviderOutputV1,
) -> Result<NormalizedProviderOutput, crate::normalize::CandidateNormalizationError> {
    normalize_provider_output(output, output.provider)
}

/// Construct a provider-local warning without carrying message text into
/// canonical merge decisions.
pub fn merge_warning(
    provider: ProviderId,
    reason: ReasonCode,
    detail_id: Option<String>,
) -> ProviderWarningV1 {
    ProviderWarningV1 {
        provider,
        reason,
        severity: WarningSeverity::Warning,
        detail_id,
        stage: Some("merge".to_owned()),
        message: None,
    }
}

/// Provider status contributes to response degradation, but does not reorder
/// or erase candidates from other lanes.
pub fn has_failed_lane(providers: &[NormalizedProviderOutput]) -> bool {
    providers.iter().any(|provider| {
        matches!(
            provider.status,
            FederationProviderStatusV1::Partial
                | FederationProviderStatusV1::Failed
                | FederationProviderStatusV1::Cancelled
        )
    })
}
