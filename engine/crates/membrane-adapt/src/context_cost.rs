//! Persistent-context cost attribution (Adapt canon §13).
//!
//! Provider-reported billed totals are facts. Attribution to persistent sources
//! is inferred, bounded by a host-observed prefix total, and is never silently
//! redistributed. Missing visibility or source-token evidence remains explicitly
//! unattributed.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const CONTEXT_COST_SCHEMA: &str = "adapt.context-cost-analysis.v1";
pub const CONTEXT_COST_HONESTY_LIMIT: &str = "Provider billed totals and measured prefix totals are host observations. Per-source shares and recurring impact are inferred. Unknown, changed, or deleted analysis-time source state is reported without reconstructing missing evidence; unresolved prefix cost remains unattributed.";

pub const APPARENTLY_UNUSED_ALWAYS_ON_CONTEXT: &str = "apparently_unused_always_on_context";
pub const DUPLICATED_PERSISTENT_INSTRUCTION: &str = "duplicated_persistent_instruction";
pub const STALE_OR_SHADOWED_PERSISTENT_SOURCE: &str = "stale_or_shadowed_persistent_source";
pub const MEMORY_RECALL_NEVER_USED: &str = "memory_recall_never_used";
pub const ALWAYS_ON_PREFIX_DOMINATES: &str = "always_on_prefix_dominates";
pub const OVERSIZED_INSTRUCTION_FILE: &str = "oversized_instruction_file";
pub const MCP_TOOL_DEFINITIONS_DOMINATE: &str = "mcp_tool_definitions_dominate";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostClass {
    Measured,
    Inferred,
    Unattributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CostAmount {
    pub bytes: u64,
    pub tokens: Option<u64>,
}

impl CostAmount {
    pub fn checked_add(self, other: CostAmount) -> Option<CostAmount> {
        Some(CostAmount {
            bytes: self.bytes.checked_add(other.bytes)?,
            tokens: match (self.tokens, other.tokens) {
                (Some(a), Some(b)) => Some(a.checked_add(b)?),
                _ => None,
            },
        })
    }
}

/// Compatibility accounting entry retained for existing native consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostAttributionV1 {
    pub cortex_record_ref: String,
    pub class: CostClass,
    pub amount: CostAmount,
}

/// Compatibility report. New persistent-prefix analysis uses
/// [`PersistentContextAnalysisV1`] below.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextCostReportV1 {
    pub installation_id: String,
    pub by_class: BTreeMap<CostClass, CostAmount>,
    pub measured_records: Vec<CostAttributionV1>,
    #[serde(default)]
    pub inferred_records: Vec<CostAttributionV1>,
    pub unattributed_records: Vec<CostAttributionV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextCostError {
    #[error("context cost accounting overflow")]
    Overflow,
    #[error("duplicate provider-usage turn id: {0}")]
    DuplicateTurn(String),
    #[error("duplicate provider-usage observation id: {0}")]
    DuplicateObservation(String),
    #[error("duplicate persistent source id: {0}")]
    DuplicateSource(String),
    #[error("persistent source {source_id} refers to unknown turn {turn_id}")]
    UnknownTurn { source_id: String, turn_id: String },
    #[error("provider usage {turn_id} measured prefix exceeds billed context input")]
    PrefixExceedsInput { turn_id: String },
    #[error("invalid persistent source {source_id}: {reason}")]
    InvalidSource { source_id: String, reason: String },
    #[error("invalid provider usage {turn_id}: {reason}")]
    InvalidUsage { turn_id: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostOverflow;

impl std::fmt::Display for CostOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "context cost accounting overflow")
    }
}

impl std::error::Error for CostOverflow {}

impl ContextCostReportV1 {
    pub fn new(installation_id: &str) -> Self {
        Self {
            installation_id: installation_id.to_string(),
            ..Default::default()
        }
    }

    pub fn attribute(
        &mut self,
        record_ref: &str,
        class: CostClass,
        amount: CostAmount,
    ) -> Result<(), CostOverflow> {
        let slot = self.by_class.entry(class).or_insert(CostAmount {
            bytes: 0,
            tokens: None,
        });
        *slot = slot.checked_add(amount).ok_or(CostOverflow)?;
        let entry = CostAttributionV1 {
            cortex_record_ref: record_ref.to_string(),
            class,
            amount,
        };
        match class {
            CostClass::Measured => self.measured_records.push(entry),
            CostClass::Inferred => self.inferred_records.push(entry),
            CostClass::Unattributed => self.unattributed_records.push(entry),
        }
        Ok(())
    }

    pub fn measured_bytes(&self) -> u64 {
        self.by_class
            .get(&CostClass::Measured)
            .map(|a| a.bytes)
            .unwrap_or(0)
    }
    pub fn inferred_bytes(&self) -> u64 {
        self.by_class
            .get(&CostClass::Inferred)
            .map(|a| a.bytes)
            .unwrap_or(0)
    }
    pub fn unattributed_bytes(&self) -> u64 {
        self.by_class
            .get(&CostClass::Unattributed)
            .map(|a| a.bytes)
            .unwrap_or(0)
    }
}

/// Exact provider-reported token counts for one billed request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderBilledUsageV1 {
    pub fresh_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_write_input_tokens: u64,
    pub output_tokens: u64,
}

impl ProviderBilledUsageV1 {
    pub fn context_input_tokens(&self) -> Result<u64, ContextCostError> {
        self.fresh_input_tokens
            .checked_add(self.cache_read_input_tokens)
            .and_then(|value| value.checked_add(self.cache_write_input_tokens))
            .ok_or(ContextCostError::Overflow)
    }

    pub fn billed_tokens(&self) -> Result<u64, ContextCostError> {
        self.context_input_tokens()?
            .checked_add(self.output_tokens)
            .ok_or(ContextCostError::Overflow)
    }

    fn checked_add(&mut self, other: Self) -> Result<(), ContextCostError> {
        self.fresh_input_tokens = self
            .fresh_input_tokens
            .checked_add(other.fresh_input_tokens)
            .ok_or(ContextCostError::Overflow)?;
        self.cache_read_input_tokens = self
            .cache_read_input_tokens
            .checked_add(other.cache_read_input_tokens)
            .ok_or(ContextCostError::Overflow)?;
        self.cache_write_input_tokens = self
            .cache_write_input_tokens
            .checked_add(other.cache_write_input_tokens)
            .ok_or(ContextCostError::Overflow)?;
        self.output_tokens = self
            .output_tokens
            .checked_add(other.output_tokens)
            .ok_or(ContextCostError::Overflow)?;
        Ok(())
    }
}

/// Host/CodeRight seam for one exact billed provider request. The optional
/// prefix value is supplied only when the host can measure persistent prefix or
/// overhead independently; Adapt never derives it from the total input bill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageObservationV1 {
    pub observation_id: String,
    pub turn_id: String,
    pub session_id: String,
    pub host: String,
    pub provider: String,
    pub model: String,
    pub usage: ProviderBilledUsageV1,
    pub measured_persistent_prefix_tokens: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistentSourceKind {
    InstructionFile,
    SystemPrompt,
    Skill,
    McpToolDefinitions,
    MemoryIndex,
    Other,
}

impl PersistentSourceKind {
    fn is_instruction(self) -> bool {
        matches!(
            self,
            Self::InstructionFile | Self::SystemPrompt | Self::Skill
        )
    }
}

/// Analysis-time state relative to the bytes/digest captured for the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SourceFileStateV1 {
    Current { analysis_digest: String },
    Changed { analysis_digest: String },
    Deleted,
    Unknown { reason: String },
    NonFile { reason: String },
}

impl SourceFileStateV1 {
    fn uncertain(&self) -> bool {
        !matches!(self, Self::Current { .. } | Self::NonFile { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationCoverage {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountObservationV1 {
    pub coverage: ObservationCoverage,
    pub count: Option<u64>,
}

impl Default for CountObservationV1 {
    fn default() -> Self {
        Self {
            coverage: ObservationCoverage::Unavailable,
            count: None,
        }
    }
}

impl CountObservationV1 {
    fn complete_zero(&self) -> bool {
        self.coverage == ObservationCoverage::Complete && self.count == Some(0)
    }
}

/// Captured identity and exact session visibility for one persistent source.
/// `captured_token_estimate` is a source-size estimate, never a billed count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistentSourceObservationV1 {
    pub source_id: String,
    pub kind: PersistentSourceKind,
    pub path: Option<String>,
    pub captured_digest: String,
    pub captured_bytes: u64,
    pub captured_token_estimate: Option<u64>,
    pub file_state: SourceFileStateV1,
    pub always_on: bool,
    /// Exact turn ids where the host reported this source visible. Empty means
    /// no visibility evidence, not "all turns".
    #[serde(default)]
    pub visible_turn_ids: Vec<String>,
    #[serde(default)]
    /// Use count coverage applies to every exact turn in `visible_turn_ids`.
    pub observed_use: CountObservationV1,
    #[serde(default)]
    /// Recall count coverage applies to every exact turn in `visible_turn_ids`.
    pub observed_memory_recall: CountObservationV1,
    #[serde(default)]
    pub shadowed_by_source_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCostDetectorPolicyV1 {
    pub minimum_recurring_tokens: u64,
    pub oversized_source_tokens: u64,
    /// Integer basis points, so evaluation is deterministic across platforms.
    pub dominant_share_basis_points: u16,
}

impl Default for ContextCostDetectorPolicyV1 {
    fn default() -> Self {
        Self {
            minimum_recurring_tokens: 1_000,
            oversized_source_tokens: 10_000,
            dominant_share_basis_points: 5_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCostAnalysisRequestV1 {
    pub installation_id: String,
    pub analysis_timestamp: String,
    pub usage_observations: Vec<ProviderUsageObservationV1>,
    pub persistent_sources: Vec<PersistentSourceObservationV1>,
    #[serde(default)]
    pub detector_policy: ContextCostDetectorPolicyV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistentSourceAttributionV1 {
    pub source_id: String,
    pub class: CostClass,
    pub inferred_recurring_tokens: u64,
    pub attributed_turns: u64,
    pub analysis_state: SourceFileStateV1,
    pub analysis_state_uncertain: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenEfficiencyFindingV1 {
    pub finding_id: String,
    pub detector: String,
    pub severity: String,
    pub implicated_tokens: u64,
    pub source_ids: Vec<String>,
    pub observed: String,
    pub honesty_limit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistentContextAnalysisV1 {
    pub schema_version: String,
    pub installation_id: String,
    pub analysis_timestamp: String,
    pub provider_billed_totals: ProviderBilledUsageV1,
    pub provider_billed_tokens: u64,
    pub measured_persistent_prefix_tokens: u64,
    pub inferred_persistent_source_tokens: u64,
    pub unattributed_persistent_prefix_tokens: u64,
    pub source_attributions: Vec<PersistentSourceAttributionV1>,
    pub findings: Vec<TokenEfficiencyFindingV1>,
    pub honesty_limit: String,
}

fn validate_request(request: &ContextCostAnalysisRequestV1) -> Result<(), ContextCostError> {
    let mut turns = BTreeSet::new();
    let mut observations = BTreeSet::new();
    for observation in &request.usage_observations {
        if observation.turn_id.trim().is_empty()
            || observation.observation_id.trim().is_empty()
            || observation.session_id.trim().is_empty()
            || observation.host.trim().is_empty()
            || observation.provider.trim().is_empty()
            || observation.model.trim().is_empty()
        {
            return Err(ContextCostError::InvalidUsage {
                turn_id: observation.turn_id.clone(),
                reason: "identity fields must be non-empty".into(),
            });
        }
        if !turns.insert(observation.turn_id.clone()) {
            return Err(ContextCostError::DuplicateTurn(observation.turn_id.clone()));
        }
        if !observations.insert(observation.observation_id.clone()) {
            return Err(ContextCostError::DuplicateObservation(
                observation.observation_id.clone(),
            ));
        }
        let input = observation.usage.context_input_tokens()?;
        if observation
            .measured_persistent_prefix_tokens
            .is_some_and(|prefix| prefix > input)
        {
            return Err(ContextCostError::PrefixExceedsInput {
                turn_id: observation.turn_id.clone(),
            });
        }
    }

    let mut source_ids = BTreeSet::new();
    for source in &request.persistent_sources {
        if source.source_id.trim().is_empty() || source.captured_digest.trim().is_empty() {
            return Err(ContextCostError::InvalidSource {
                source_id: source.source_id.clone(),
                reason: "source id and captured digest must be non-empty".into(),
            });
        }
        if !source_ids.insert(source.source_id.clone()) {
            return Err(ContextCostError::DuplicateSource(source.source_id.clone()));
        }
        let unique_visible: BTreeSet<&str> =
            source.visible_turn_ids.iter().map(String::as_str).collect();
        if unique_visible.len() != source.visible_turn_ids.len() {
            return Err(ContextCostError::InvalidSource {
                source_id: source.source_id.clone(),
                reason: "visible turn ids must be unique".into(),
            });
        }
        for turn_id in &source.visible_turn_ids {
            if !turns.contains(turn_id) {
                return Err(ContextCostError::UnknownTurn {
                    source_id: source.source_id.clone(),
                    turn_id: turn_id.clone(),
                });
            }
        }
        match &source.file_state {
            SourceFileStateV1::Current { analysis_digest }
                if analysis_digest != &source.captured_digest =>
            {
                return Err(ContextCostError::InvalidSource {
                    source_id: source.source_id.clone(),
                    reason: "current file-state digest differs from captured digest".into(),
                });
            }
            SourceFileStateV1::Changed { analysis_digest }
                if analysis_digest == &source.captured_digest =>
            {
                return Err(ContextCostError::InvalidSource {
                    source_id: source.source_id.clone(),
                    reason: "changed file-state digest equals captured digest".into(),
                });
            }
            SourceFileStateV1::NonFile { .. } if source.path.is_some() => {
                return Err(ContextCostError::InvalidSource {
                    source_id: source.source_id.clone(),
                    reason: "non-file source must not claim a path".into(),
                });
            }
            _ => {}
        }
        if source
            .path
            .as_ref()
            .is_some_and(|path| path.trim().is_empty())
        {
            return Err(ContextCostError::InvalidSource {
                source_id: source.source_id.clone(),
                reason: "source path must not be empty".into(),
            });
        }
        if matches!(
            source.file_state,
            SourceFileStateV1::Current { .. }
                | SourceFileStateV1::Changed { .. }
                | SourceFileStateV1::Deleted
        ) && source.path.is_none()
        {
            return Err(ContextCostError::InvalidSource {
                source_id: source.source_id.clone(),
                reason: "file-backed source state requires a path".into(),
            });
        }
        for signal in [&source.observed_use, &source.observed_memory_recall] {
            if (signal.coverage == ObservationCoverage::Complete && signal.count.is_none())
                || (signal.coverage == ObservationCoverage::Unavailable && signal.count.is_some())
            {
                return Err(ContextCostError::InvalidSource {
                    source_id: source.source_id.clone(),
                    reason: "observation count does not match its coverage".into(),
                });
            }
        }
    }
    for source in &request.persistent_sources {
        if let Some(shadow) = &source.shadowed_by_source_id {
            if shadow == &source.source_id || !source_ids.contains(shadow) {
                return Err(ContextCostError::InvalidSource {
                    source_id: source.source_id.clone(),
                    reason: "shadow source must name a different observed source".into(),
                });
            }
        }
    }
    if request.detector_policy.dominant_share_basis_points > 10_000 {
        return Err(ContextCostError::InvalidUsage {
            turn_id: "policy".into(),
            reason: "dominant share basis points exceed 10000".into(),
        });
    }
    Ok(())
}

/// Allocate at most `total` tokens across sorted weighted source ids. A source
/// size sum below the measured prefix leaves an unattributed remainder.
fn allocate_bounded(
    total: u64,
    weighted: &[(String, u64)],
) -> Result<BTreeMap<String, u64>, ContextCostError> {
    let mut result = BTreeMap::new();
    let weight_sum = weighted.iter().try_fold(0u64, |sum, (_, weight)| {
        sum.checked_add(*weight).ok_or(ContextCostError::Overflow)
    })?;
    if total == 0 || weight_sum == 0 {
        return Ok(result);
    }
    let allocatable = total.min(weight_sum);
    if weight_sum <= total {
        for (source_id, weight) in weighted {
            result.insert(source_id.clone(), *weight);
        }
        return Ok(result);
    }
    let mut assigned = 0u64;
    let mut remainders = Vec::new();
    for (source_id, weight) in weighted {
        let numerator = u128::from(allocatable) * u128::from(*weight);
        let share = (numerator / u128::from(weight_sum)) as u64;
        assigned = assigned
            .checked_add(share)
            .ok_or(ContextCostError::Overflow)?;
        result.insert(source_id.clone(), share);
        remainders.push((numerator % u128::from(weight_sum), source_id.clone()));
    }
    remainders.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    let mut leftover = allocatable - assigned;
    for (_, source_id) in remainders {
        if leftover == 0 {
            break;
        }
        *result.get_mut(&source_id).expect("allocated source exists") += 1;
        leftover -= 1;
    }
    Ok(result)
}

fn stable_finding(
    detector: &str,
    severity: &str,
    tokens: u64,
    mut source_ids: Vec<String>,
    observed: String,
    honesty_limit: &str,
) -> TokenEfficiencyFindingV1 {
    source_ids.sort();
    source_ids.dedup();
    let seed = serde_json::json!({"detector": detector, "sourceIds": source_ids});
    let digest = Sha256::digest(serde_json::to_vec(&seed).expect("finding seed serializes"));
    TokenEfficiencyFindingV1 {
        finding_id: format!("ctx_{}", &hex::encode(digest)[..24]),
        detector: detector.into(),
        severity: severity.into(),
        implicated_tokens: tokens,
        source_ids,
        observed,
        honesty_limit: honesty_limit.into(),
    }
}

fn share_at_least(part: u64, total: u64, basis_points: u16) -> bool {
    total > 0 && u128::from(part) * 10_000 >= u128::from(total) * u128::from(basis_points)
}

/// Analyze provider observations and explicitly visible persistent sources.
pub fn analyze_persistent_context(
    request: &ContextCostAnalysisRequestV1,
) -> Result<PersistentContextAnalysisV1, ContextCostError> {
    validate_request(request)?;
    let mut billed_totals = ProviderBilledUsageV1::default();
    let mut measured_prefix = 0u64;
    let mut inferred_by_source: BTreeMap<String, u64> = BTreeMap::new();
    let mut turns_by_source: BTreeMap<String, u64> = BTreeMap::new();
    let mut sources: Vec<&PersistentSourceObservationV1> =
        request.persistent_sources.iter().collect();
    sources.sort_by(|left, right| left.source_id.cmp(&right.source_id));

    for observation in &request.usage_observations {
        billed_totals.checked_add(observation.usage)?;
        let Some(prefix_tokens) = observation.measured_persistent_prefix_tokens else {
            continue;
        };
        measured_prefix = measured_prefix
            .checked_add(prefix_tokens)
            .ok_or(ContextCostError::Overflow)?;
        let weighted: Vec<(String, u64)> = sources
            .iter()
            .filter(|source| {
                source
                    .visible_turn_ids
                    .iter()
                    .any(|turn| turn == &observation.turn_id)
            })
            .filter_map(|source| {
                source
                    .captured_token_estimate
                    .map(|tokens| (source.source_id.clone(), tokens))
            })
            .filter(|(_, tokens)| *tokens > 0)
            .collect();
        for (source_id, share) in allocate_bounded(prefix_tokens, &weighted)? {
            let slot = inferred_by_source.entry(source_id.clone()).or_default();
            *slot = slot.checked_add(share).ok_or(ContextCostError::Overflow)?;
            if share > 0 {
                *turns_by_source.entry(source_id).or_default() += 1;
            }
        }
    }

    let inferred_total = inferred_by_source.values().try_fold(0u64, |sum, value| {
        sum.checked_add(*value).ok_or(ContextCostError::Overflow)
    })?;
    let unattributed = measured_prefix
        .checked_sub(inferred_total)
        .ok_or(ContextCostError::Overflow)?;
    let source_attributions: Vec<_> = sources
        .iter()
        .map(|source| PersistentSourceAttributionV1 {
            source_id: source.source_id.clone(),
            class: CostClass::Inferred,
            inferred_recurring_tokens: inferred_by_source
                .get(&source.source_id)
                .copied()
                .unwrap_or(0),
            attributed_turns: turns_by_source.get(&source.source_id).copied().unwrap_or(0),
            analysis_state: source.file_state.clone(),
            analysis_state_uncertain: source.file_state.uncertain(),
        })
        .collect();

    let policy = request.detector_policy;
    let mut findings = Vec::new();
    for source in &sources {
        let tokens = inferred_by_source
            .get(&source.source_id)
            .copied()
            .unwrap_or(0);
        if source.always_on
            && tokens >= policy.minimum_recurring_tokens
            && source.observed_use.complete_zero()
        {
            findings.push(stable_finding(APPARENTLY_UNUSED_ALWAYS_ON_CONTEXT, "medium", tokens, vec![source.source_id.clone()],
                format!("{} was visible in {} attributed turns with complete use telemetry recording zero observed uses", source.source_id, turns_by_source.get(&source.source_id).copied().unwrap_or(0)),
                "No observed use does not prove no behavioral influence; 'apparently' is required."));
        }
        if source.always_on
            && !source.visible_turn_ids.is_empty()
            && (source.file_state.uncertain() || source.shadowed_by_source_id.is_some())
        {
            findings.push(stable_finding(STALE_OR_SHADOWED_PERSISTENT_SOURCE, "medium", tokens, vec![source.source_id.clone()],
                format!("{} was persistent while its analysis-time state was {:?}{}", source.source_id, source.file_state, source.shadowed_by_source_id.as_ref().map(|id| format!(" and it was shadowed by {id}")).unwrap_or_default()),
                "Analysis-time state cannot reconstruct the bytes visible during the session; captured digest and state are reported separately."));
        }
        if source.always_on
            && source.kind == PersistentSourceKind::MemoryIndex
            && tokens >= policy.minimum_recurring_tokens
            && source.observed_memory_recall.complete_zero()
        {
            findings.push(stable_finding(MEMORY_RECALL_NEVER_USED, "low", tokens, vec![source.source_id.clone()],
                format!("{} incurred inferred recurring context cost while complete recall telemetry recorded zero recalls", source.source_id),
                "This detector is emitted only with complete session/source recall observability."));
        }
        if source.kind == PersistentSourceKind::InstructionFile
            && tokens >= policy.minimum_recurring_tokens
            && source
                .captured_token_estimate
                .is_some_and(|estimate| estimate >= policy.oversized_source_tokens)
        {
            findings.push(stable_finding(OVERSIZED_INSTRUCTION_FILE, "medium", tokens, vec![source.source_id.clone()],
                format!("{} captured token estimate meets the configured oversized-source threshold", source.source_id),
                "Source token size is captured evidence; its share of billed persistent prefix remains inferred."));
        }
    }

    for left_index in 0..sources.len() {
        let left = sources[left_index];
        if !left.kind.is_instruction() {
            continue;
        }
        for right in sources.iter().skip(left_index + 1).copied() {
            if !right.kind.is_instruction() || left.captured_digest != right.captured_digest {
                continue;
            }
            let left_turns: BTreeSet<&str> =
                left.visible_turn_ids.iter().map(String::as_str).collect();
            if !right
                .visible_turn_ids
                .iter()
                .any(|turn| left_turns.contains(turn.as_str()))
            {
                continue;
            }
            let tokens = inferred_by_source
                .get(&left.source_id)
                .copied()
                .unwrap_or(0)
                .checked_add(
                    inferred_by_source
                        .get(&right.source_id)
                        .copied()
                        .unwrap_or(0),
                )
                .ok_or(ContextCostError::Overflow)?;
            findings.push(stable_finding(
                DUPLICATED_PERSISTENT_INSTRUCTION,
                "medium",
                tokens,
                vec![left.source_id.clone(), right.source_id.clone()],
                format!(
                    "{} and {} had the same captured digest and overlapped in session visibility",
                    left.source_id, right.source_id
                ),
                "Digest equality proves duplicate captured bytes, not duplicate behavioral effect.",
            ));
        }
    }

    let mcp_tokens = sources
        .iter()
        .filter(|source| source.kind == PersistentSourceKind::McpToolDefinitions)
        .try_fold(0u64, |sum, source| {
            sum.checked_add(
                inferred_by_source
                    .get(&source.source_id)
                    .copied()
                    .unwrap_or(0),
            )
            .ok_or(ContextCostError::Overflow)
        })?;
    if mcp_tokens >= policy.minimum_recurring_tokens
        && share_at_least(
            mcp_tokens,
            measured_prefix,
            policy.dominant_share_basis_points,
        )
    {
        findings.push(stable_finding(
            MCP_TOOL_DEFINITIONS_DOMINATE,
            "medium",
            mcp_tokens,
            sources
                .iter()
                .filter(|source| source.kind == PersistentSourceKind::McpToolDefinitions)
                .map(|source| source.source_id.clone())
                .collect(),
            "MCP tool definitions meet the configured dominant share of measured persistent prefix"
                .into(),
            "The aggregate share is an inferred split reconciled against a measured prefix total.",
        ));
    }
    let always_on_tokens =
        sources
            .iter()
            .filter(|source| source.always_on)
            .try_fold(0u64, |sum, source| {
                sum.checked_add(
                    inferred_by_source
                        .get(&source.source_id)
                        .copied()
                        .unwrap_or(0),
                )
                .ok_or(ContextCostError::Overflow)
            })?;
    if always_on_tokens >= policy.minimum_recurring_tokens
        && share_at_least(
            always_on_tokens,
            measured_prefix,
            policy.dominant_share_basis_points,
        )
    {
        findings.push(stable_finding(
            ALWAYS_ON_PREFIX_DOMINATES,
            "medium",
            always_on_tokens,
            sources
                .iter()
                .filter(|source| source.always_on)
                .map(|source| source.source_id.clone())
                .collect(),
            "always-on sources meet the configured dominant share of measured persistent prefix"
                .into(),
            "The aggregate share is inferred; unresolved prefix cost remains unattributed.",
        ));
    }
    findings.sort_by(|left, right| {
        left.detector
            .cmp(&right.detector)
            .then(left.finding_id.cmp(&right.finding_id))
    });

    Ok(PersistentContextAnalysisV1 {
        schema_version: CONTEXT_COST_SCHEMA.into(),
        installation_id: request.installation_id.clone(),
        analysis_timestamp: request.analysis_timestamp.clone(),
        provider_billed_tokens: billed_totals.billed_tokens()?,
        provider_billed_totals: billed_totals,
        measured_persistent_prefix_tokens: measured_prefix,
        inferred_persistent_source_tokens: inferred_total,
        unattributed_persistent_prefix_tokens: unattributed,
        source_attributions,
        findings,
        honesty_limit: CONTEXT_COST_HONESTY_LIMIT.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(turn_id: &str, prefix: Option<u64>) -> ProviderUsageObservationV1 {
        ProviderUsageObservationV1 {
            observation_id: format!("obs-{turn_id}"),
            turn_id: turn_id.into(),
            session_id: "session-1".into(),
            host: "coderight".into(),
            provider: "example".into(),
            model: "model-1".into(),
            usage: ProviderBilledUsageV1 {
                fresh_input_tokens: 2_000,
                cache_read_input_tokens: 8_000,
                cache_write_input_tokens: 0,
                output_tokens: 500,
            },
            measured_persistent_prefix_tokens: prefix,
        }
    }

    fn source(
        id: &str,
        kind: PersistentSourceKind,
        tokens: Option<u64>,
        turns: &[&str],
    ) -> PersistentSourceObservationV1 {
        PersistentSourceObservationV1 {
            source_id: id.into(),
            kind,
            path: Some(format!("/{id}.md")),
            captured_digest: format!("sha256:{id}"),
            captured_bytes: tokens.unwrap_or(0) * 4,
            captured_token_estimate: tokens,
            file_state: SourceFileStateV1::Current {
                analysis_digest: format!("sha256:{id}"),
            },
            always_on: true,
            visible_turn_ids: turns.iter().map(|turn| (*turn).into()).collect(),
            observed_use: CountObservationV1::default(),
            observed_memory_recall: CountObservationV1::default(),
            shadowed_by_source_id: None,
        }
    }

    fn request(sources: Vec<PersistentSourceObservationV1>) -> ContextCostAnalysisRequestV1 {
        ContextCostAnalysisRequestV1 {
            installation_id: "install-1".into(),
            analysis_timestamp: "2026-08-26T00:00:00Z".into(),
            usage_observations: vec![usage("t1", Some(5_000)), usage("t2", Some(5_000))],
            persistent_sources: sources,
            detector_policy: ContextCostDetectorPolicyV1::default(),
        }
    }

    #[test]
    fn cost_classes_never_merge_and_inferred_detail_is_retained() {
        let mut report = ContextCostReportV1::new("inst");
        report
            .attribute(
                "rec-1",
                CostClass::Measured,
                CostAmount {
                    bytes: 100,
                    tokens: Some(10),
                },
            )
            .unwrap();
        report
            .attribute(
                "rec-2",
                CostClass::Inferred,
                CostAmount {
                    bytes: 50,
                    tokens: None,
                },
            )
            .unwrap();
        report
            .attribute(
                "rec-3",
                CostClass::Unattributed,
                CostAmount {
                    bytes: 25,
                    tokens: None,
                },
            )
            .unwrap();
        assert_eq!(
            (
                report.measured_bytes(),
                report.inferred_bytes(),
                report.unattributed_bytes()
            ),
            (100, 50, 25)
        );
        assert_eq!(report.inferred_records[0].cortex_record_ref, "rec-2");
    }

    #[test]
    fn attribution_reconciles_without_exceeding_measured_prefix() {
        let report = analyze_persistent_context(&request(vec![
            source(
                "a",
                PersistentSourceKind::InstructionFile,
                Some(4_000),
                &["t1", "t2"],
            ),
            source(
                "b",
                PersistentSourceKind::McpToolDefinitions,
                Some(4_000),
                &["t1", "t2"],
            ),
        ]))
        .unwrap();
        assert_eq!(report.provider_billed_tokens, 21_000);
        assert_eq!(
            (
                report.measured_persistent_prefix_tokens,
                report.inferred_persistent_source_tokens,
                report.unattributed_persistent_prefix_tokens
            ),
            (10_000, 10_000, 0)
        );
        assert_eq!(
            report
                .source_attributions
                .iter()
                .map(|item| item.inferred_recurring_tokens)
                .sum::<u64>(),
            10_000
        );
    }

    #[test]
    fn unresolved_source_weight_remains_unattributed() {
        let report = analyze_persistent_context(&request(vec![
            source(
                "known",
                PersistentSourceKind::InstructionFile,
                Some(1_000),
                &["t1", "t2"],
            ),
            source("unknown", PersistentSourceKind::Other, None, &["t1", "t2"]),
        ]))
        .unwrap();
        assert_eq!(
            (
                report.inferred_persistent_source_tokens,
                report.unattributed_persistent_prefix_tokens
            ),
            (2_000, 8_000)
        );
    }

    #[test]
    fn recurring_cost_multiplies_only_exact_visible_turns() {
        let report = analyze_persistent_context(&request(vec![source(
            "once",
            PersistentSourceKind::InstructionFile,
            Some(800),
            &["t1"],
        )]))
        .unwrap();
        assert_eq!(
            (
                report.source_attributions[0].inferred_recurring_tokens,
                report.source_attributions[0].attributed_turns
            ),
            (800, 1)
        );
    }

    #[test]
    fn apparently_unused_requires_complete_observability() {
        let mut complete = source(
            "rules",
            PersistentSourceKind::InstructionFile,
            Some(2_000),
            &["t1", "t2"],
        );
        complete.observed_use = CountObservationV1 {
            coverage: ObservationCoverage::Complete,
            count: Some(0),
        };
        let report = analyze_persistent_context(&request(vec![complete])).unwrap();
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.detector == APPARENTLY_UNUSED_ALWAYS_ON_CONTEXT));
        let mut partial = source(
            "rules",
            PersistentSourceKind::InstructionFile,
            Some(2_000),
            &["t1", "t2"],
        );
        partial.observed_use = CountObservationV1 {
            coverage: ObservationCoverage::Partial,
            count: Some(0),
        };
        let report = analyze_persistent_context(&request(vec![partial])).unwrap();
        assert!(!report
            .findings
            .iter()
            .any(|finding| finding.detector == APPARENTLY_UNUSED_ALWAYS_ON_CONTEXT));

        let mut used = source(
            "rules",
            PersistentSourceKind::InstructionFile,
            Some(2_000),
            &["t1", "t2"],
        );
        used.observed_use = CountObservationV1 {
            coverage: ObservationCoverage::Complete,
            count: Some(1),
        };
        let report = analyze_persistent_context(&request(vec![used])).unwrap();
        assert!(!report
            .findings
            .iter()
            .any(|finding| finding.detector == APPARENTLY_UNUSED_ALWAYS_ON_CONTEXT));
    }

    #[test]
    fn memory_recall_detector_fails_closed_without_complete_source_observability() {
        let mut memory = source(
            "memory",
            PersistentSourceKind::MemoryIndex,
            Some(2_000),
            &["t1", "t2"],
        );
        memory.observed_memory_recall = CountObservationV1 {
            coverage: ObservationCoverage::Partial,
            count: Some(0),
        };
        let report = analyze_persistent_context(&request(vec![memory.clone()])).unwrap();
        assert!(!report
            .findings
            .iter()
            .any(|finding| finding.detector == MEMORY_RECALL_NEVER_USED));
        memory.observed_memory_recall = CountObservationV1 {
            coverage: ObservationCoverage::Complete,
            count: Some(0),
        };
        let report = analyze_persistent_context(&request(vec![memory])).unwrap();
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.detector == MEMORY_RECALL_NEVER_USED));
    }

    #[test]
    fn changed_deleted_and_unknown_sources_remain_explicitly_uncertain() {
        let mut changed = source(
            "changed",
            PersistentSourceKind::InstructionFile,
            Some(1_000),
            &["t1"],
        );
        changed.file_state = SourceFileStateV1::Changed {
            analysis_digest: "sha256:new".into(),
        };
        let mut deleted = source(
            "deleted",
            PersistentSourceKind::InstructionFile,
            Some(1_000),
            &["t1"],
        );
        deleted.file_state = SourceFileStateV1::Deleted;
        let mut unknown = source(
            "unknown",
            PersistentSourceKind::InstructionFile,
            Some(1_000),
            &["t1"],
        );
        unknown.file_state = SourceFileStateV1::Unknown {
            reason: "permission denied".into(),
        };
        let report = analyze_persistent_context(&request(vec![changed, deleted, unknown])).unwrap();
        assert!(report
            .source_attributions
            .iter()
            .all(|item| item.analysis_state_uncertain));
        assert_eq!(
            report
                .findings
                .iter()
                .filter(|finding| finding.detector == STALE_OR_SHADOWED_PERSISTENT_SOURCE)
                .count(),
            3
        );
    }

    #[test]
    fn duplicate_detector_requires_same_digest_and_overlapping_visibility() {
        let a = source(
            "a",
            PersistentSourceKind::InstructionFile,
            Some(1_000),
            &["t1"],
        );
        let mut b = source("b", PersistentSourceKind::Skill, Some(1_000), &["t1"]);
        b.captured_digest = a.captured_digest.clone();
        b.file_state = SourceFileStateV1::Current {
            analysis_digest: a.captured_digest.clone(),
        };
        let report = analyze_persistent_context(&request(vec![a, b])).unwrap();
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.detector == DUPLICATED_PERSISTENT_INSTRUCTION));
        let nonoverlap_a = source(
            "a",
            PersistentSourceKind::InstructionFile,
            Some(1_000),
            &["t2"],
        );
        let mut nonoverlap_b = source("b", PersistentSourceKind::Skill, Some(1_000), &["t1"]);
        nonoverlap_b.captured_digest = nonoverlap_a.captured_digest.clone();
        nonoverlap_b.file_state = SourceFileStateV1::Current {
            analysis_digest: nonoverlap_a.captured_digest.clone(),
        };
        let report =
            analyze_persistent_context(&request(vec![nonoverlap_a, nonoverlap_b])).unwrap();
        assert!(!report
            .findings
            .iter()
            .any(|finding| finding.detector == DUPLICATED_PERSISTENT_INSTRUCTION));
    }

    #[test]
    fn prefix_over_input_is_rejected() {
        let mut req = request(vec![]);
        req.usage_observations[0].measured_persistent_prefix_tokens = Some(10_001);
        assert!(matches!(
            analyze_persistent_context(&req),
            Err(ContextCostError::PrefixExceedsInput { .. })
        ));
    }

    #[test]
    fn aggregate_and_size_detectors_use_the_canonical_ids() {
        let mut provider_usage = usage("t1", Some(20_000));
        provider_usage.usage.fresh_input_tokens = 12_000;
        let req = ContextCostAnalysisRequestV1 {
            installation_id: "install-1".into(),
            analysis_timestamp: "2026-08-26T00:00:00Z".into(),
            usage_observations: vec![provider_usage],
            persistent_sources: vec![
                source(
                    "large-rules",
                    PersistentSourceKind::InstructionFile,
                    Some(10_000),
                    &["t1"],
                ),
                source(
                    "mcp-tools",
                    PersistentSourceKind::McpToolDefinitions,
                    Some(10_000),
                    &["t1"],
                ),
            ],
            detector_policy: ContextCostDetectorPolicyV1::default(),
        };
        let first = analyze_persistent_context(&req).unwrap();
        let second = analyze_persistent_context(&req).unwrap();
        let detectors: BTreeSet<&str> = first
            .findings
            .iter()
            .map(|finding| finding.detector.as_str())
            .collect();
        assert!(detectors.contains(OVERSIZED_INSTRUCTION_FILE));
        assert!(detectors.contains(MCP_TOOL_DEFINITIONS_DOMINATE));
        assert!(detectors.contains(ALWAYS_ON_PREFIX_DOMINATES));
        assert!(!detectors.contains("unused_always_on_context"));
        assert_eq!(
            first.findings, second.findings,
            "finding ids must be stable"
        );

        let report = analyze_persistent_context(&request(vec![source(
            "large-but-not-visible",
            PersistentSourceKind::InstructionFile,
            Some(20_000),
            &[],
        )]))
        .unwrap();
        assert!(!report
            .findings
            .iter()
            .any(|finding| finding.detector == OVERSIZED_INSTRUCTION_FILE));
    }

    #[test]
    fn file_backed_source_state_requires_a_nonempty_path() {
        let mut missing = source(
            "missing-path",
            PersistentSourceKind::InstructionFile,
            Some(1_000),
            &["t1"],
        );
        missing.path = None;
        assert!(matches!(
            analyze_persistent_context(&request(vec![missing])),
            Err(ContextCostError::InvalidSource { .. })
        ));

        let mut empty = source(
            "empty-path",
            PersistentSourceKind::InstructionFile,
            Some(1_000),
            &["t1"],
        );
        empty.path = Some("   ".into());
        assert!(matches!(
            analyze_persistent_context(&request(vec![empty])),
            Err(ContextCostError::InvalidSource { .. })
        ));
    }

    #[test]
    fn distinct_billed_turns_cannot_alias_one_observation_identity() {
        let mut req = request(vec![]);
        req.usage_observations[1].observation_id = req.usage_observations[0].observation_id.clone();
        assert!(matches!(
            analyze_persistent_context(&req),
            Err(ContextCostError::DuplicateObservation(_))
        ));
    }
}
