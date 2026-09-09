//! Additive V1 contracts for task-scoped Pull evidence requirements.
//!
//! These contracts classify retrieval needs only. They never convey a grant,
//! identity, repository authority, or permission to perform an effect.

use membrane_protocol::ProviderId;
use membrane_protocol::digest_str;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub const REQUIREMENTS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceDimensionV1 {
    RepositoryTruth,
    CurrentState,
    Policy,
    History,
    Diagnostics,
    DurableKnowledge,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequirementFactV1 {
    pub dimension: EvidenceDimensionV1,
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub required: bool,
    /// Stable classifier rule, never caller-provided authority.
    #[serde(default)]
    pub rule_id: String,
    /// Content-free binding to normalized task facts.
    #[serde(default)]
    pub binding_digest: String,
    /// An exact lexical target when bounded parsing finds one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_target: Option<String>,
    /// Optional caller-known content identity constraints. They constrain
    /// matching only; they never derive authority from task prose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub representation_digest: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceRequirementSetV1 {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub task_id: String,
    #[serde(default)]
    pub facts: Vec<RequirementFactV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderCapabilityV1 {
    pub provider: ProviderId,
    #[serde(default)]
    pub dimensions: Vec<EvidenceDimensionV1>,
    pub authoritative: bool,
    pub fresh: bool,
    pub ready: bool,
    pub cost_rank: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omission: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcquisitionPlanV1 {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub task_id: String,
    #[serde(default)]
    pub providers: Vec<ProviderId>,
    #[serde(default)]
    pub omissions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CandidateJourneyV1 {
    /// Canonical evidence identity, never a lexical task target.
    pub evidence_id: String,
    pub requirement_binding_digest: String,
    pub dimension: EvidenceDimensionV1,
    pub provider: ProviderId,
    pub source_hash: String,
    pub representation_digest: String,
    /// Source-owned target reference, distinct from canonical evidence ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<String>,
    #[serde(default)]
    pub acquired: bool,
    #[serde(default)]
    pub eligible: bool,
    #[serde(default)]
    pub admitted: bool,
    #[serde(default)]
    pub represented: bool,
    #[serde(default)]
    pub fenced: bool,
    #[serde(default)]
    pub emitted: bool,
    #[serde(default)]
    pub retained: bool,
    #[serde(default)]
    pub dropped: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequirementEvidenceMapV1 {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub task_id: String,
    #[serde(default)]
    pub journeys: Vec<CandidateJourneyV1>,
    #[serde(default)]
    pub unsatisfied: Vec<EvidenceDimensionV1>,
}

const LEXICAL_DIMENSIONS: [(&str, EvidenceDimensionV1); 14] = [
    ("why", EvidenceDimensionV1::RepositoryTruth),
    ("where", EvidenceDimensionV1::RepositoryTruth),
    ("how", EvidenceDimensionV1::RepositoryTruth),
    ("current", EvidenceDimensionV1::CurrentState),
    ("status", EvidenceDimensionV1::CurrentState),
    ("latest", EvidenceDimensionV1::CurrentState),
    ("rule", EvidenceDimensionV1::Policy),
    ("policy", EvidenceDimensionV1::Policy),
    ("history", EvidenceDimensionV1::History),
    ("regression", EvidenceDimensionV1::History),
    ("error", EvidenceDimensionV1::Diagnostics),
    ("failure", EvidenceDimensionV1::Diagnostics),
    ("diagnostics", EvidenceDimensionV1::Diagnostics),
    ("remember", EvidenceDimensionV1::DurableKnowledge),
];

const fn schema_version() -> u32 { REQUIREMENTS_SCHEMA_VERSION }

/// Deterministic bounded lexical classification. Text can request evidence
/// dimensions, but has no authority-bearing output.
pub fn compile_requirement_set(
    task_id: impl Into<String>,
    task: &str,
    caller_facts: &[RequirementFactV1],
) -> EvidenceRequirementSetV1 {
    let task_id = task_id.into();
    let normalized_task = task.to_ascii_lowercase();
    let tokens: BTreeSet<&str> = normalized_task
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();
    let binding_digest = digest_str(&format!("pull-requirements-v1\0{task_id}\0{task}"));
    let exact_target = exact_target(task);
    let mut facts: BTreeMap<EvidenceDimensionV1, RequirementFactV1> = BTreeMap::new();
    // Every generic task has bounded current-worktree retrieval need. This is
    // a classification default, not an authorization or repository grant.
    facts.insert(EvidenceDimensionV1::CurrentState, RequirementFactV1 {
        dimension: EvidenceDimensionV1::CurrentState,
        schema_version: REQUIREMENTS_SCHEMA_VERSION,
        required: true,
        rule_id: "current_worktree_v1".to_owned(),
        binding_digest: binding_digest.clone(),
        exact_target: exact_target.clone(),
        source_hash: None,
        representation_digest: None,
    });
    for (signal, dimension) in LEXICAL_DIMENSIONS {
        if tokens.contains(signal) {
            facts.insert(dimension.clone(), RequirementFactV1 {
                dimension,
                schema_version: REQUIREMENTS_SCHEMA_VERSION,
                required: true,
                rule_id: format!("lexical_{signal}_v1"),
                binding_digest: binding_digest.clone(),
                exact_target: exact_target.clone(),
                source_hash: None,
                representation_digest: None,
            });
        }
    }
    let mut additional_facts = Vec::new();
    for fact in caller_facts {
        let caller = normalized_caller_fact(fact, &binding_digest);
        if let Some(existing) = facts.get_mut(&caller.dimension) {
            // A classifier-created fact has no caller binding. An explicit
            // typed binding therefore strengthens it and must be retained.
            let constraint_conflict = existing.binding_digest != binding_digest
                && existing.binding_digest != caller.binding_digest
                || existing.source_hash.as_ref().zip(caller.source_hash.as_ref())
                    .map_or(false, |(left, right)| left != right)
                || existing.representation_digest.as_ref().zip(caller.representation_digest.as_ref())
                    .map_or(false, |(left, right)| left != right)
                || existing.exact_target.as_ref().zip(caller.exact_target.as_ref())
                    .map_or(false, |(left, right)| left != right);
            if constraint_conflict {
                // Distinct explicit constraints remain independent required
                // facts, so neither caller can weaken or erase other scope.
                additional_facts.push(caller);
                continue;
            }
            existing.required |= caller.required;
            if !fact.rule_id.is_empty() { existing.rule_id = caller.rule_id; }
            if !fact.binding_digest.is_empty() { existing.binding_digest = caller.binding_digest; }
            if caller.exact_target.is_some() { existing.exact_target = caller.exact_target; }
            if caller.source_hash.is_some() { existing.source_hash = caller.source_hash; }
            if caller.representation_digest.is_some() { existing.representation_digest = caller.representation_digest; }
        } else {
            facts.insert(caller.dimension.clone(), caller);
        }
    }
    EvidenceRequirementSetV1 {
        schema_version: REQUIREMENTS_SCHEMA_VERSION,
        task_id,
        facts: {
            let mut values: Vec<_> = facts.into_values().collect();
            values.extend(additional_facts);
            values.sort_by(|left, right| {
                left.dimension.cmp(&right.dimension)
                    .then_with(|| left.binding_digest.cmp(&right.binding_digest))
                    .then_with(|| left.source_hash.cmp(&right.source_hash))
                    .then_with(|| left.representation_digest.cmp(&right.representation_digest))
            });
            values
        },
    }
}

pub fn plan_acquisition(
    requirements: &EvidenceRequirementSetV1,
    capabilities: &[ProviderCapabilityV1],
) -> AcquisitionPlanV1 {
    let required: BTreeSet<_> = requirements.facts.iter()
        .filter(|fact| fact.required)
        .map(|fact| fact.dimension.clone())
        .collect();
    let mut providers: Vec<_> = capabilities.iter()
        .filter(|capability| capability.authoritative && capability.fresh && capability.ready)
        .filter(|capability| capability.dimensions.iter().any(|dimension| required.contains(dimension)))
        .collect();
    providers.sort_by_key(|capability| (capability.cost_rank, capability.provider.rank()));
    let covered: BTreeSet<_> = providers.iter()
        .flat_map(|capability| capability.dimensions.iter().cloned())
        .collect();
    let mut omissions: Vec<_> = required.iter()
        .filter(|dimension| !covered.contains(*dimension))
        .map(|dimension| format!("required_capability_unavailable:{dimension:?}"))
        .collect();
    omissions.extend(capabilities.iter().filter_map(|capability| capability.omission.clone()));
    omissions.sort();
    omissions.dedup();
    AcquisitionPlanV1 {
        schema_version: REQUIREMENTS_SCHEMA_VERSION,
        task_id: requirements.task_id.clone(),
        providers: providers.into_iter().map(|capability| capability.provider).collect(),
        omissions,
    }
}

pub fn coverage_map(
    requirements: &EvidenceRequirementSetV1,
    journeys: Vec<CandidateJourneyV1>,
    capabilities: &[ProviderCapabilityV1],
) -> RequirementEvidenceMapV1 {
    let plan = plan_acquisition(requirements, capabilities);
    let available: HashSet<_> = plan.providers.into_iter().collect();
    let unsatisfied = requirements.facts.iter()
        .filter(|fact| fact.required)
        .filter(|fact| {
            let capability_available = capabilities.iter().any(|capability| {
                available.contains(&capability.provider) && capability.dimensions.contains(&fact.dimension)
            });
            let delivered = journeys.iter().any(|journey| {
                let provider_capable = capabilities.iter().any(|capability| {
                    capability.provider == journey.provider
                        && available.contains(&capability.provider)
                        && capability.dimensions.contains(&fact.dimension)
                });
                journey.acquired && journey.eligible && journey.admitted && journey.represented
                    && journey.fenced && journey.emitted && !journey.dropped
                    && journey.requirement_binding_digest == fact.binding_digest
                    && journey.dimension == fact.dimension
                    && provider_capable
                    && fact.exact_target.as_ref().map_or(true, |target| journey.target_ref.as_ref() == Some(target))
                    && fact.source_hash.as_ref().map_or(true, |hash| hash == &journey.source_hash)
                    && fact.representation_digest.as_ref().map_or(true, |digest| digest == &journey.representation_digest)
            });
            !capability_available || !delivered
        })
        .map(|fact| fact.dimension.clone())
        .collect();
    RequirementEvidenceMapV1 {
        schema_version: REQUIREMENTS_SCHEMA_VERSION,
        task_id: requirements.task_id.clone(),
        journeys,
        unsatisfied,
    }
}

fn exact_target(task: &str) -> Option<String> {
    task.split_whitespace()
        .map(|token| token.trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '.' && character != '/' && character != '_' && character != '-'))
        .find(|token| token.contains('.') || token.contains('/'))
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
}

fn normalized_caller_fact(fact: &RequirementFactV1, default_binding: &str) -> RequirementFactV1 {
    RequirementFactV1 {
        dimension: fact.dimension.clone(),
        schema_version: REQUIREMENTS_SCHEMA_VERSION,
        required: fact.required,
        rule_id: if fact.rule_id.is_empty() { "caller_typed_v1".to_owned() } else { fact.rule_id.clone() },
        binding_digest: if fact.binding_digest.is_empty() { default_binding.to_owned() } else { fact.binding_digest.clone() },
        exact_target: fact.exact_target.clone(),
        source_hash: fact.source_hash.clone(),
        representation_digest: fact.representation_digest.clone(),
    }
}
