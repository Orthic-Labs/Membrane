//! Token-budget accounting for context injection.
//!
//! Counts are precise when the tiktoken-rs singleton initializes successfully;
//! falls back to `chars / 4` if the tokenizer can't load (rare — only when the
//! bundled `.tiktoken` data is missing from the build).

use std::sync::OnceLock;
use tiktoken_rs::CoreBPE;

static TOKENIZER: OnceLock<Result<CoreBPE, String>> = OnceLock::new();

fn tokenizer() -> Option<&'static CoreBPE> {
    TOKENIZER
        .get_or_init(|| tiktoken_rs::o200k_base().map_err(|e| format!("tiktoken init failed: {e}")))
        .as_ref()
        .ok()
}

/// Exact o200k_base count for literal data. Unlike estimate_tokens this never
/// substitutes a ratio and never treats token-looking source strings as control.
pub fn count_o200k_exact(text: &str) -> Result<usize, String> {
    let bpe = tokenizer().ok_or_else(|| "o200k_base tokenizer unavailable".to_string())?;
    Ok(bpe.encode_ordinary(text).len())
}

/// Count the tokens in `text`. Uses the o200k_base tokenizer (covers GPT-4o,
/// GPT-4.1, GPT-5, o1/o3/o4 series, Codex — the vast majority of models routed
/// through OpenAI-compat providers). Falls back to `chars / 4` if the tokenizer
/// singleton failed to initialize.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    if let Some(bpe) = tokenizer() {
        let tokens = bpe.encode_with_special_tokens(text);
        return tokens.len().max(1);
    }
    // Fallback — tokenizer failed to load (shouldn't happen in production).
    let est = text.chars().count() / 4;
    est.max(1)
}

/// Tracks a running token budget as text is added to a context window.
#[derive(Debug, Clone)]
pub struct ContextTokenAccounting {
    budget: usize,
    used: usize,
}

impl ContextTokenAccounting {
    pub fn new(budget: usize) -> Self {
        Self { budget, used: 0 }
    }

    /// Estimate the tokens for `text`, add them to the running total, and return the
    /// number of tokens added.
    pub fn add(&mut self, text: &str) -> usize {
        let tokens = estimate_tokens(text);
        self.used = self.used.saturating_add(tokens);
        tokens
    }

    pub fn used(&self) -> usize {
        self.used
    }

    /// Remaining budget, saturating at zero.
    pub fn remaining(&self) -> usize {
        self.budget.saturating_sub(self.used)
    }

    /// Whether adding `text` would push usage past the budget.
    pub fn would_exceed(&self, text: &str) -> bool {
        self.used.saturating_add(estimate_tokens(text)) > self.budget
    }

    /// Reset used tokens back to zero (budget unchanged).
    pub fn reset(&mut self) {
        self.used = 0;
    }
}
