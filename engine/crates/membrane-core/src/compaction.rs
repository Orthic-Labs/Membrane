//! Deterministic, read-time session compaction.
//!
//! Inputs are immutable typed records.  The assembler reconstructs state in a
//! fixed category order, retains protected material first, and only invokes an
//! injected residual summarizer when deterministic retention cannot fit.  No
//! default path calls a model, mutates history, or owns durable storage.

use crate::compaction_receipt::{CacheImpactV1, CompactionReceiptV1, CompactionSourceCursor};
use membrane_protocol::digest_str;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const COMPACTION_PROJECTION_SCHEMA_VERSION: u32 = 1;

pub const CATEGORY_WORLD: &str = "world";
pub const CATEGORY_SESSION: &str = "session";
pub const CATEGORY_OBLIGATION: &str = "obligation";
pub const CATEGORY_ACTIVE_TASK: &str = "active_task";
pub const CATEGORY_DECISION: &str = "decision";
pub const CATEGORY_MEMORY: &str = "memory";
pub const CATEGORY_ADAPT_PREFERENCE: &str = "adapt_preference";
pub const CATEGORY_ADAPT_GOTCHA: &str = "adapt_gotcha";
pub const CATEGORY_NARRATIVE: &str = "narrative";
pub const CATEGORY_RESIDUAL_NARRATIVE: &str = "residual_narrative";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactionItem {
    pub id: String,
    pub category: String,
    pub content: String,
    pub priority: i32,
    pub source_seq: u64,
    pub protected: bool,
}

impl CompactionItem {
    pub fn new(
        id: impl Into<String>,
        category: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            category: category.into(),
            content: content.into(),
            priority: 0,
            source_seq: 0,
            protected: false,
        }
    }

    fn must_retain(&self) -> bool {
        self.protected
            || matches!(
                self.category.as_str(),
                CATEGORY_OBLIGATION
                    | CATEGORY_ACTIVE_TASK
                    | "protected_obligation"
                    | "id"
                    | "constraint"
                    | "error"
                    | "citation"
                    | "tool_pair"
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactionInput {
    pub source_cursor: CompactionSourceCursor,
    #[serde(default)]
    pub world: Vec<CompactionItem>,
    #[serde(default)]
    pub session: Vec<CompactionItem>,
    #[serde(default)]
    pub obligations: Vec<CompactionItem>,
    #[serde(default)]
    pub active_task: Vec<CompactionItem>,
    #[serde(default)]
    pub decisions: Vec<CompactionItem>,
    #[serde(default)]
    pub memories: Vec<CompactionItem>,
    #[serde(default)]
    pub adapt_preferences: Vec<CompactionItem>,
    #[serde(default)]
    pub adapt_gotchas: Vec<CompactionItem>,
    #[serde(default)]
    pub narrative: Vec<CompactionItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionConfig {
    pub budget_tokens: u32,
    pub cache_impact: CacheImpactV1,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            budget_tokens: 4096,
            cache_impact: CacheImpactV1::default(),
        }
    }
}

/// Optional residual fallback.  Implementations may call an external model,
/// but only when the deterministic projection has a residual to reduce.
pub trait ResidualNarrativeSummarizer {
    fn summarize(&self, residual: &[CompactionItem], budget_tokens: u32) -> Option<String>;
    fn provider(&self) -> &str;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompactionProjection {
    pub schema_version: u32,
    pub source_cursor: CompactionSourceCursor,
    pub retained: Vec<CompactionItem>,
    pub rendered_text: String,
    pub omitted_categories: Vec<String>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub budget_tokens: u32,
    pub budget_met: bool,
    pub fallback_used: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionResult {
    pub projection: CompactionProjection,
    pub receipt: CompactionReceiptV1,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompactionError {
    #[error("compaction source cursor is invalid")]
    InvalidCursor,
    #[error("compaction token budget must be positive")]
    InvalidBudget,
    #[error("compaction item id is empty")]
    EmptyItemId,
    #[error("compaction item category is empty")]
    EmptyCategory,
    #[error("duplicate compaction item id: {0}")]
    DuplicateItemId(String),
}

/// Assemble a projection without an injected fallback.
pub fn compact(
    input: &CompactionInput,
    config: &CompactionConfig,
) -> Result<CompactionResult, CompactionError> {
    assemble(input, config, None)
}

/// Assemble a projection, allowing a caller-owned summarizer only for residual
/// narrative.  Passing `None` is the safe default and never performs a model
/// call.
pub fn assemble(
    input: &CompactionInput,
    config: &CompactionConfig,
    summarizer: Option<&dyn ResidualNarrativeSummarizer>,
) -> Result<CompactionResult, CompactionError> {
    if input.source_cursor.session_id.trim().is_empty() {
        return Err(CompactionError::InvalidCursor);
    }
    if config.budget_tokens == 0 {
        return Err(CompactionError::InvalidBudget);
    }

    let all = ordered_items(input);
    validate_items(&all)?;
    let input_tokens = all.iter().map(item_tokens).sum::<usize>();
    let protected_tokens = all
        .iter()
        .filter(|item| item.must_retain())
        .map(item_tokens)
        .sum::<usize>();

    let mut retained = Vec::new();
    let mut omitted = Vec::new();
    let mut output_tokens = 0usize;
    let protected_over_budget = protected_tokens > config.budget_tokens as usize;
    for item in &all {
        let required = item.must_retain();
        let cost = item_tokens(item);
        let fits = output_tokens.saturating_add(cost) <= config.budget_tokens as usize;
        if required || (!protected_over_budget && fits) {
            output_tokens = output_tokens.saturating_add(cost);
            retained.push(item.clone());
        } else {
            omitted.push(item.category.clone());
        }
    }

    let mut fallback_used = false;
    let mut fallback_provider = None;
    let narrative_residual = all
        .iter()
        .filter(|item| {
            item.category == CATEGORY_NARRATIVE && !retained.iter().any(|r| r.id == item.id)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !narrative_residual.is_empty() && output_tokens < config.budget_tokens as usize {
        if let Some(summarizer) = summarizer {
            let remaining = (config.budget_tokens as usize - output_tokens) as u32;
            if let Some(summary) = summarizer.summarize(&narrative_residual, remaining) {
                let summary = summary.trim().to_owned();
                if !summary.is_empty() {
                    let item = CompactionItem {
                        id: "residual-narrative".to_owned(),
                        category: CATEGORY_RESIDUAL_NARRATIVE.to_owned(),
                        content: summary,
                        priority: 0,
                        source_seq: narrative_residual
                            .iter()
                            .map(|item| item.source_seq)
                            .min()
                            .unwrap_or(0),
                        protected: false,
                    };
                    if item_tokens(&item) <= remaining as usize {
                        output_tokens = output_tokens.saturating_add(item_tokens(&item));
                        retained.push(item);
                        fallback_used = true;
                        fallback_provider = Some(summarizer.provider().to_owned());
                    }
                }
            }
        }
    }

    let omitted_categories = unique_categories(&omitted);
    let rendered_text = render(&retained);
    let output_tokens = estimate_tokens(&rendered_text);
    let budget_met = !protected_over_budget && output_tokens <= config.budget_tokens as usize;
    let projection_hash = digest_str(&rendered_text);
    let projection = CompactionProjection {
        schema_version: COMPACTION_PROJECTION_SCHEMA_VERSION,
        source_cursor: input.source_cursor.clone(),
        retained,
        rendered_text,
        omitted_categories: omitted_categories.clone(),
        input_tokens: saturating_u32(input_tokens),
        output_tokens: saturating_u32(output_tokens),
        budget_tokens: config.budget_tokens,
        budget_met,
        fallback_used,
        fallback_provider: fallback_provider.clone(),
    };
    let receipt = CompactionReceiptV1 {
        schema_version: CompactionReceiptV1::SCHEMA_VERSION,
        source_cursor: input.source_cursor.clone(),
        projection_hash,
        retained_obligations: projection
            .retained
            .iter()
            .filter(|item| item.category == CATEGORY_OBLIGATION)
            .map(|item| item.id.clone())
            .collect(),
        omitted_categories,
        input_tokens: projection.input_tokens,
        output_tokens: projection.output_tokens,
        budget_tokens: projection.budget_tokens,
        budget_met: projection.budget_met,
        fallback_used,
        fallback_provider,
        cache_impact: config.cache_impact.clone(),
    };
    Ok(CompactionResult {
        projection,
        receipt,
    })
}

fn ordered_items(input: &CompactionInput) -> Vec<CompactionItem> {
    let mut items = Vec::new();
    for group in [
        &input.world,
        &input.session,
        &input.obligations,
        &input.active_task,
        &input.decisions,
        &input.memories,
        &input.adapt_preferences,
        &input.adapt_gotchas,
        &input.narrative,
    ] {
        items.extend(group.iter().cloned());
    }
    items.sort_by(|left, right| {
        category_rank(&left.category)
            .cmp(&category_rank(&right.category))
            .then_with(|| right.priority.cmp(&left.priority))
            .then_with(|| left.source_seq.cmp(&right.source_seq))
            .then_with(|| left.id.cmp(&right.id))
    });
    items
}

fn validate_items(items: &[CompactionItem]) -> Result<(), CompactionError> {
    let mut ids = BTreeSet::new();
    for item in items {
        if item.id.trim().is_empty() {
            return Err(CompactionError::EmptyItemId);
        }
        if item.category.trim().is_empty() {
            return Err(CompactionError::EmptyCategory);
        }
        if !ids.insert(item.id.as_str()) {
            return Err(CompactionError::DuplicateItemId(item.id.clone()));
        }
    }
    Ok(())
}

fn category_rank(category: &str) -> u8 {
    match category {
        CATEGORY_WORLD => 0,
        CATEGORY_SESSION => 1,
        CATEGORY_OBLIGATION => 2,
        CATEGORY_ACTIVE_TASK => 3,
        CATEGORY_DECISION => 4,
        CATEGORY_MEMORY => 5,
        CATEGORY_ADAPT_PREFERENCE => 6,
        CATEGORY_ADAPT_GOTCHA => 7,
        CATEGORY_NARRATIVE => 8,
        CATEGORY_RESIDUAL_NARRATIVE => 9,
        _ => 10,
    }
}

fn item_tokens(item: &CompactionItem) -> usize {
    estimate_tokens(&format!(
        "[{}] {}: {}",
        item.category, item.id, item.content
    ))
}

pub fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count()
}

fn render(items: &[CompactionItem]) -> String {
    items
        .iter()
        .map(|item| format!("[{}] {}: {}", item.category, item.id, item.content))
        .collect::<Vec<_>>()
        .join("\n")
}

fn unique_categories(categories: &[String]) -> Vec<String> {
    let mut categories = categories.to_vec();
    categories.sort_by(|left, right| {
        category_rank(left)
            .cmp(&category_rank(right))
            .then_with(|| left.cmp(right))
    });
    categories.dedup();
    categories
}

fn saturating_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}
