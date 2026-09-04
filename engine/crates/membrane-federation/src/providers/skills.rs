//! Native skills snapshot provider.
//!
//! The source owner supplies one generation-sealed index.  This adapter
//! validates its bounds and provenance, ranks metadata in process, and emits
//! resolver-backed candidates without embedding skill bodies.

#[path = "../skill_ranker.rs"]
pub mod skill_ranker;

use membrane_protocol::{
    FederationProviderStatusV1, ProviderDiagnosticsV1, ProviderId, ProviderOmissionV1,
    ProviderOutputV1, ProviderWarningV1, ReasonCode, WarningSeverity,
    PROVIDER_OUTPUT_SCHEMA_VERSION,
};
use membrane_provider_sdk::{
    Provider, ProviderContext, ProviderError, ProviderOutput, SkillCatalogEntry,
};
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub const PROVIDER_ID: ProviderId = ProviderId::Skills;
pub const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024;
pub const MAX_SKILLS: usize = 512;
pub const MAX_RESULTS: usize = 5;

pub struct SkillsProvider {
    source: Arc<dyn membrane_provider_sdk::SkillCatalogSource>,
    result_limit: usize,
}

impl std::fmt::Debug for SkillsProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SkillsProvider")
            .field("source", &"injected")
            .field("result_limit", &self.result_limit)
            .finish()
    }
}

impl Clone for SkillsProvider {
    fn clone(&self) -> Self {
        Self {
            source: Arc::clone(&self.source),
            result_limit: self.result_limit,
        }
    }
}

impl SkillsProvider {
    pub fn new(source: Arc<dyn membrane_provider_sdk::SkillCatalogSource>) -> Self {
        Self {
            source,
            result_limit: MAX_RESULTS,
        }
    }

    pub fn with_source<S>(source: S) -> Self
    where
        S: membrane_provider_sdk::SkillCatalogSource + 'static,
    {
        Self::new(Arc::new(source))
    }

    pub fn with_result_limit(mut self, limit: usize) -> Self {
        self.result_limit = limit.min(MAX_RESULTS);
        self
    }
}

impl SkillsProvider {
    async fn provide_inner(
        &self,
        context: &ProviderContext,
    ) -> Result<ProviderOutput, ProviderError> {
        if context.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        if context.is_deadline_exhausted() {
            return Err(ProviderError::DeadlineExceeded);
        }
        let response = match self.source.skills(&context.query()).await {
            Ok(response) => response,
            Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
            Err(ProviderError::DeadlineExceeded) => return Err(ProviderError::DeadlineExceeded),
            Err(error) => {
                return Ok(gap_output(
                    ReasonCode::ProviderUnavailable,
                    "source_unavailable",
                ))
            }
        };
        if context.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        if context.is_deadline_exhausted() {
            return Err(ProviderError::DeadlineExceeded);
        }

        let Some(generation) = response
            .generation
            .clone()
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(gap_output(
                ReasonCode::ProviderMalformed,
                "generation_missing",
            ));
        };
        // Prefer the snapshot generation over the release generation.
        //
        // A source here answers with the content identity of what it indexed;
        // the release generation is the Membrane build's own sha256. Checking
        // the build's identity first meant every source disagreed with it and
        // was gapped as generation_incoherent — measured on this machine as
        // seven such omissions and zero candidates on a request whose sources
        // had answered normally.
        let expected = context
            .freshness
            .generation
            .as_deref()
            .or(context.release_generation.as_deref());
        if expected.is_some_and(|value| value != generation) {
            return Ok(generation_gap(expected.unwrap_or_default(), &generation));
        }
        if response.value.len() > MAX_SKILLS {
            return Ok(gap_output(
                ReasonCode::ProviderMalformed,
                "snapshot_row_cap",
            ));
        }
        if serde_json::to_vec(&response)
            .map(|bytes| bytes.len() > MAX_SNAPSHOT_BYTES)
            .unwrap_or(true)
        {
            return Ok(gap_output(
                ReasonCode::ProviderMalformed,
                "snapshot_byte_cap",
            ));
        }

        for entry in &response.value {
            if entry.generation != generation {
                return Ok(generation_gap(&generation, &entry.generation));
            }
            if !valid_entry(entry, context) {
                return Ok(gap_output(
                    ReasonCode::ProviderMalformed,
                    "snapshot_provenance",
                ));
            }
        }

        let ranked = skill_ranker::rank(&context.task, &response.value, self.result_limit);
        let mut output = ProviderOutputV1 {
            schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
            provider: PROVIDER_ID,
            status: if response.complete {
                FederationProviderStatusV1::Complete
            } else {
                FederationProviderStatusV1::Partial
            },
            generation: Some(generation.clone()),
            candidates: ranked
                .into_iter()
                .map(|skill| candidate(skill.entry, skill.score))
                .collect(),
            warnings: response.warnings.iter().map(source_warning).collect(),
            omissions: Vec::new(),
            diagnostics: Some(ProviderDiagnosticsV1 {
                provider: PROVIDER_ID,
                elapsed_ms: None,
                generation: Some(generation),
                attributes: BTreeMap::from([(
                    String::from("representation"),
                    String::from("index_only"),
                )]),
            }),
            extensions: BTreeMap::new(),
        };
        if output.candidates.is_empty() && output.warnings.is_empty() {
            output.status = FederationProviderStatusV1::Partial;
            output.warnings.push(warning(
                ReasonCode::ProviderUnavailable,
                "no_relevant_skill",
            ));
        }
        if !response.complete {
            output
                .warnings
                .push(warning(ReasonCode::ProviderFailed, "source_incomplete"));
        }
        Ok(output)
    }
}

impl Provider for SkillsProvider {
    fn provide<'life0, 'life1, 'async_trait>(
        &'life0 self,
        context: &'life1 ProviderContext,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderOutput, ProviderError>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(self.provide_inner(context))
    }
}

fn valid_entry(entry: &SkillCatalogEntry, context: &ProviderContext) -> bool {
    !entry.id.trim().is_empty()
        && !entry.title.trim().is_empty()
        && entry.repository_id == context.repository_id
        && entry.source_hash.len() == 64
        && entry
            .source_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

fn candidate(entry: SkillCatalogEntry, score: f64) -> membrane_protocol::CandidateV1 {
    let id = format!("skills:{}", entry.id);
    let source_ref = format!("tools/skills/{}/SKILL.md", entry.id);
    membrane_protocol::CandidateV1 {
        id,
        layer: 7,
        provider: Some(PROVIDER_ID.as_str().to_owned()),
        source_kind: "skill".to_owned(),
        source_ref,
        source_hash: entry.source_hash,
        trust_class: "workspace_tracked".to_owned(),
        instruction_policy: "data_only".to_owned(),
        provider_score: score,
        score_components: BTreeMap::from([(String::from("skill_rank"), score)]),
        base_commit: None,
        overlay_digest: None,
        freshness_class: None,
        snapshot_id: None,
        estimated_tokens: (entry.title.len() / 4).max(1) as u32,
        protected: false,
        exact: false,
        recoverable: true,
        resolver: format!("cortex skill-read {}", entry.id),
        text: entry.title.chars().take(200).collect(),
    }
}

fn source_warning(source: &membrane_provider_sdk::SourceWarning) -> ProviderWarningV1 {
    warning(
        ReasonCode::parse(&source.code).unwrap_or(ReasonCode::ProviderFailed),
        source.detail_id.as_deref().unwrap_or("source_warning"),
    )
}

fn warning(reason: ReasonCode, detail: &str) -> ProviderWarningV1 {
    ProviderWarningV1 {
        provider: PROVIDER_ID,
        reason,
        severity: WarningSeverity::Warning,
        detail_id: Some(detail.to_owned()),
        stage: Some("skills_provider".to_owned()),
        message: None,
    }
}

fn gap_output(reason: ReasonCode, detail: &str) -> ProviderOutput {
    ProviderOutputV1 {
        schema_version: PROVIDER_OUTPUT_SCHEMA_VERSION,
        provider: PROVIDER_ID,
        status: FederationProviderStatusV1::Partial,
        generation: None,
        candidates: Vec::new(),
        warnings: vec![warning(reason, detail)],
        omissions: vec![ProviderOmissionV1 {
            provider: PROVIDER_ID,
            reason,
            candidate_id: None,
            detail_id: Some(detail.to_owned()),
            stage: Some("skills_provider".to_owned()),
        }],
        diagnostics: None,
        extensions: BTreeMap::new(),
    }
}

fn generation_gap(expected: &str, observed: &str) -> ProviderOutput {
    let mut output = gap_output(
        ReasonCode::GenerationIncoherent,
        "skills_generation_changed",
    );
    output.generation = (!observed.is_empty()).then(|| observed.to_owned());
    output.omissions[0].detail_id = Some(format!("expected:{expected}"));
    output
}
