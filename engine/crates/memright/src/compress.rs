//! L3/L5 prose compressor.
//!
//! Two paths, picked at runtime when both are available:
//!
//! 1. **ONNX LLMLingua-2** (gated by `--features llmlingua-onnx`) — runs the
//!    token-classification head exported from
//!    `microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank`. Tokenizes
//!    with HF `bert-base-multilingual-cased`, scores every token with keep-prob,
//!    keeps top-`rate` (force-kept for punctuation / newlines), detokenizes.
//!    Parity with the reference Python llmlingua ≥ 0.90 word-Jaccard at the
//!    default rate (0.5). See `spike/SPIKE-RESULT.md` for the full gate evidence.
//!
//! 2. **Deterministic heuristic** (always available) — `memright_format::compress_prose`,
//!    a quality-degraded fallback. Selected when `--no-onnx` is set, when the
//!    `llmlingua-onnx` feature isn't built, or when the ONNX assets are missing.
//!
//! Asset resolution: `$MEMRIGHT_LLMLINGUA_MODEL` and `$MEMRIGHT_LLMLINGUA_TOKENIZER`
//! override the defaults (`crates/memright/assets/llmlingua/`). The DLL is
//! loaded from `$ORT_DYLIB_PATH` (same pattern as the BGE/fastembed path).
//!
//! The core algorithm pieces — softmax-keep-prob, top-K + force-token selection,
//! and asset-path resolution — are exposed as pure helpers and tested without the
//! 677MB ONNX model so CI catches structural regressions even when the parity
//! test (which needs the model) is skipped.

use serde::Serialize;

/// Compress prose, keeping approximately `rate` (0.0–1.0) of the original.
///
/// `rate` is the fraction to KEEP (workspace parity), not a "ratio to drop".
pub fn compress(text: &str, rate: f32) -> String {
    compress_with_options(text, rate, false)
}

/// Result of a budget-targeted compression attempt.
///
/// `budget_met` is false only when protected spans alone exceed the requested
/// budget. Callers must surface that state instead of silently dropping them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetCompression {
    pub text: String,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub budget_tokens: usize,
    pub protected_tokens: usize,
    pub budget_met: bool,
}

/// Content-free accounting for material removed by a transform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DropManifest {
    pub dropped_identifiers: usize,
    pub dropped_error_lines: usize,
    pub dropped_numeric_literals: usize,
    pub kept_identifier_ratio_milli: usize,
    pub risk: &'static str,
}

/// Compare source and output with cheap, deterministic classes suitable for
/// telemetry. It never stores source content in a manifest or event.
pub fn drop_manifest(before: &str, after: &str) -> DropManifest {
    let before_identifiers = identifier_count(before);
    let after_identifiers = identifier_count(after);
    let dropped_identifiers = before_identifiers.saturating_sub(after_identifiers);
    let dropped_error_lines = error_line_count(before).saturating_sub(error_line_count(after));
    let dropped_numeric_literals =
        numeric_literal_count(before).saturating_sub(numeric_literal_count(after));
    let kept_identifier_ratio_milli = if before_identifiers == 0 {
        1_000
    } else {
        after_identifiers.min(before_identifiers) * 1_000 / before_identifiers
    };
    let risk = if dropped_identifiers > 0 || dropped_error_lines > 0 {
        "high"
    } else if dropped_numeric_literals > 0 {
        "medium"
    } else {
        "low"
    };
    DropManifest {
        dropped_identifiers,
        dropped_error_lines,
        dropped_numeric_literals,
        kept_identifier_ratio_milli,
        risk,
    }
}

/// Compact, content-free metadata value for `transform_log.meta`.
pub fn drop_manifest_meta(manifest: &DropManifest) -> String {
    format!(
        "dropped_identifiers={};dropped_error_lines={};dropped_numeric_literals={};kept_identifier_ratio_milli={};risk={}",
        manifest.dropped_identifiers,
        manifest.dropped_error_lines,
        manifest.dropped_numeric_literals,
        manifest.kept_identifier_ratio_milli,
        manifest.risk,
    )
}

fn identifier_count(text: &str) -> usize {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|token| {
            token.contains('_')
                || token
                    .chars()
                    .skip(1)
                    .any(|character| character.is_ascii_uppercase())
        })
        .count()
        + text.match_indices("::").count()
}

fn numeric_literal_count(text: &str) -> usize {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-')
        .filter(|token| token.chars().any(|character| character.is_ascii_digit()))
        .count()
}

fn error_line_count(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            [
                "error",
                "exception",
                "panic",
                "traceback",
                "failed",
                "failure",
            ]
            .iter()
            .any(|needle| lower.contains(needle))
        })
        .count()
}

/// Stable lexical token estimate used by budget routing and telemetry.
///
/// This deliberately counts word-like spans instead of characters, so a
/// budget cannot be defeated by whitespace. The ONNX path may select content,
/// but this accounting remains available when its large model assets are not.
pub fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Compress prose to a token budget while preserving safety-critical lines.
///
/// The legacy rate API stays byte-for-byte independent. New callers use this
/// path so identifiers, error traces, paths, URLs, numeric literals, negation,
/// and fenced code never disappear merely to satisfy a target. If those spans
/// exceed `budget_tokens`, their exact source is returned with `budget_met`
/// false; dropping them is not an allowed fallback.
pub fn compress_to_budget(text: &str, budget_tokens: usize) -> BudgetCompression {
    compress_to_budget_with_options(text, budget_tokens, false)
}

/// [`compress_to_budget`] with an explicit ONNX opt-out for deterministic
/// hooks and tests.
pub fn compress_to_budget_with_options(
    text: &str,
    budget_tokens: usize,
    no_onnx: bool,
) -> BudgetCompression {
    let input_tokens = estimate_tokens(text);
    let mut lines = Vec::new();
    let mut protected = String::new();
    let mut in_fence = false;

    for line in text.split_inclusive('\n') {
        let is_fence = line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~");
        let keep = in_fence || is_fence || protected_line(line);
        if keep {
            protected.push_str(line);
        }
        lines.push((line, keep));
        if is_fence {
            in_fence = !in_fence;
        }
    }
    // `split_inclusive` omits no content, including an empty final segment.
    let protected_tokens = estimate_tokens(&protected);
    if protected_tokens > budget_tokens {
        return BudgetCompression {
            text: text.to_string(),
            input_tokens,
            output_tokens: input_tokens,
            budget_tokens,
            protected_tokens,
            budget_met: false,
        };
    }

    let ordinary_tokens = lines
        .iter()
        .filter(|(_, keep)| !keep)
        .map(|(line, _)| estimate_tokens(line))
        .sum::<usize>();
    let ordinary_budget = budget_tokens.saturating_sub(protected_tokens);
    let mut out = String::with_capacity(text.len());
    let mut remaining_budget = ordinary_budget;
    let mut remaining_tokens = ordinary_tokens;
    for (line, keep) in lines {
        if keep {
            out.push_str(line);
            continue;
        }
        let line_tokens = estimate_tokens(line);
        let line_budget = if remaining_tokens == 0 {
            0
        } else {
            (remaining_budget * line_tokens + remaining_tokens - 1) / remaining_tokens
        };
        if line_budget >= line_tokens {
            out.push_str(line);
        } else {
            let rate = if line_tokens == 0 {
                0.0
            } else {
                line_budget as f32 / line_tokens as f32
            };
            out.push_str(&truncate_to_tokens(
                &compress_with_options(line, rate, no_onnx),
                line_budget,
            ));
        }
        remaining_budget = remaining_budget.saturating_sub(line_budget);
        remaining_tokens = remaining_tokens.saturating_sub(line_tokens);
    }
    if text.is_empty() {
        out.clear();
    }
    let output_tokens = estimate_tokens(&out);
    BudgetCompression {
        text: out,
        input_tokens,
        output_tokens,
        budget_tokens,
        protected_tokens,
        budget_met: output_tokens <= budget_tokens,
    }
}

fn truncate_to_tokens(text: &str, budget: usize) -> String {
    if budget == 0 {
        return String::new();
    }
    let mut words = 0usize;
    let mut in_word = false;
    for (idx, c) in text.char_indices() {
        if c.is_whitespace() {
            in_word = false;
            continue;
        }
        if !in_word {
            words += 1;
            if words > budget {
                return text[..idx].trim_end().to_string();
            }
            in_word = true;
        }
    }
    text.to_string()
}

fn protected_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let error = [
        "error",
        "exception",
        "panic",
        "traceback",
        "failed",
        "failure",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    let negation = [
        " not ",
        " never ",
        " cannot ",
        " can't ",
        " don't ",
        " doesn't ",
        " without ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        || lower.starts_with("no ")
        || lower.starts_with("not ");
    let identifier = line.contains("::")
        || line.contains('_')
        || line.chars().any(|c| c.is_ascii_digit())
        || line
            .split_whitespace()
            .any(|word| word.chars().skip(1).any(|c| c.is_ascii_uppercase()));
    let path_or_url = line.contains("://")
        || line.split_whitespace().any(|word| {
            word.starts_with('/')
                || word.starts_with("./")
                || word.contains("/src/")
                || word.contains("\\\\")
                || word.rsplit_once(':').is_some_and(|(_, suffix)| {
                    suffix
                        .trim_matches(|c: char| !c.is_ascii_digit())
                        .chars()
                        .any(|c| c.is_ascii_digit())
                })
        });
    error || negation || identifier || path_or_url
}

#[cfg(feature = "llmlingua-onnx")]
const LLMLINGUA_MAX_SEQ_LEN: usize = 512;

#[cfg(feature = "llmlingua-onnx")]
fn token_chunks<T>(tokens: &[T]) -> impl Iterator<Item = &[T]> {
    tokens.chunks(LLMLINGUA_MAX_SEQ_LEN)
}

/// Same as [`compress`] but allows forcing the heuristic path even when the
/// ONNX feature is enabled.
pub fn compress_with_options(text: &str, rate: f32, no_onnx: bool) -> String {
    let rate = rate.clamp(0.0, 1.0);

    #[cfg(not(feature = "llmlingua-onnx"))]
    let _ = no_onnx;

    #[cfg(feature = "llmlingua-onnx")]
    {
        if !no_onnx {
            match compress_llmlingua_onnx(text, rate) {
                Ok(out) => return out,
                Err(err) => {
                    eprintln!(
                        "[memright] llmlingua-onnx unavailable ({err}); falling back to heuristic"
                    );
                }
            }
        }
    }

    // Deterministic heuristic fallback (always available).
    memright_format::compress_prose(text, rate)
}

// ============================================================================
// ONNX path
// ============================================================================

#[cfg(feature = "llmlingua-onnx")]
fn compress_llmlingua_onnx(text: &str, rate: f32) -> Result<String, String> {
    use ort::value::Tensor;

    if rate <= 0.0 || text.is_empty() {
        return Ok(String::new());
    }

    with_llmlingua_runtime(|runtime| {
        let enc = runtime
            .tokenizer
            .encode(text, false)
            .map_err(|e| format!("tokenize: {e}"))?;
        let ids: Vec<i64> = enc.get_ids().iter().map(|&i| i as i64).collect();
        if ids.is_empty() {
            return Ok(String::new());
        }
        let mut keep_prob = Vec::with_capacity(ids.len());
        let mut force_mask = Vec::with_capacity(ids.len());

        // The model accepts at most 512 tokens. Score every chunk and select
        // over the full input so long documents are never silently truncated.
        for chunk in token_chunks(&ids) {
            let n = chunk.len();
            let shape = [1i64, n as i64];
            let iids_t = Tensor::<i64>::from_array((shape, chunk.to_vec()))
                .map_err(|e| format!("input_ids tensor: {e}"))?;
            let am_t = Tensor::<i64>::from_array((shape, vec![1i64; n]))
                .map_err(|e| format!("attention_mask tensor: {e}"))?;
            let tt_t = Tensor::<i64>::from_array((shape, vec![0i64; n]))
                .map_err(|e| format!("token_type_ids tensor: {e}"))?;
            let outputs = runtime
                .session
                .run(ort::inputs![iids_t, am_t, tt_t])
                .map_err(|e| format!("session.run: {e}"))?;
            let (out_shape, logits_flat) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| format!("extract logits: {e}"))?;
            let dims: &[i64] = out_shape;
            match dims {
                [_, seq, classes] if *seq as usize == n && *classes == 2 => {}
                s => return Err(format!("unexpected logits shape: {s:?}")),
            }
            keep_prob.extend((0..n).map(|i| {
                softmax_keep_prob_two_class(&[logits_flat[i * 2], logits_flat[i * 2 + 1]])
            }));
            force_mask.extend(chunk.iter().map(|id| {
                let piece = runtime
                    .tokenizer
                    .decode(&[*id as u32], true)
                    .unwrap_or_default();
                ['\n', '.', '!', '?', ',']
                    .iter()
                    .any(|c| piece.contains(*c))
            }));
        }

        let keep_set = select_keep_indices(&keep_prob, &force_mask, rate);

        let kept_ids: Vec<u32> = keep_set.iter().map(|&i| ids[i] as u32).collect();
        runtime
            .tokenizer
            .decode(&kept_ids, true)
            .map_err(|e| format!("decode: {e}"))
    })
}

#[cfg(feature = "llmlingua-onnx")]
struct LlmlinguaRuntime {
    tokenizer: tokenizers::Tokenizer,
    session: ort::session::Session,
}

#[cfg(feature = "llmlingua-onnx")]
fn load_llmlingua_runtime() -> Result<LlmlinguaRuntime, String> {
    let (model_path, tok_path) = llmlingua_asset_paths()?;
    let tokenizer =
        tokenizers::Tokenizer::from_file(&tok_path).map_err(|e| format!("tokenizer load: {e}"))?;
    let session = ort::session::Session::builder()
        .map_err(|e| format!("session builder: {e}"))?
        .commit_from_file(&model_path)
        .map_err(|e| format!("commit model: {e}"))?;
    Ok(LlmlinguaRuntime { tokenizer, session })
}

#[cfg(feature = "llmlingua-onnx")]
thread_local! {
    static LLMLINGUA_RUNTIME: std::cell::RefCell<Option<Result<LlmlinguaRuntime, String>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(feature = "llmlingua-onnx")]
fn with_llmlingua_runtime<T>(
    f: impl FnOnce(&mut LlmlinguaRuntime) -> Result<T, String>,
) -> Result<T, String> {
    LLMLINGUA_RUNTIME.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(load_llmlingua_runtime());
        }
        let mut borrowed = cell.borrow_mut();
        match borrowed.as_mut().expect("runtime initialized") {
            Ok(runtime) => f(runtime),
            Err(err) => Err(err.clone()),
        }
    })
}

/// Softmax over a 2-element binary-classification row, returning the
/// keep-probability (index 1). Numerically stable (subtract max first).
///
/// Exposed (crate-internal) for unit testing without the ONNX model.
#[cfg(feature = "llmlingua-onnx")]
fn softmax_keep_prob_two_class(logits: &[f32; 2]) -> f32 {
    let max = logits[0].max(logits[1]);
    let exp_sum = (logits[0] - max).exp() + (logits[1] - max).exp();
    (logits[1] - max).exp() / exp_sum
}

/// Top-K + force-token selection. Given per-position keep-probabilities, a
/// force-mask (positions that must be kept), and `rate` (fraction to KEEP),
/// returns the set of original positions to retain. Force positions are always
/// kept; remaining budget is allocated to the highest keep-prob positions.
///
/// `rate` is the fraction to KEEP, matching `compress.py --rate`. Edge cases:
/// - `rate <= 0` returns empty (caller should pre-check and return empty string)
/// - `rate >= 1` returns all positions
/// - `force_count > total_keep` shrinks the top-K to zero (force alone fills
///   the budget; could exceed it, which we accept for simplicity)
#[cfg(feature = "llmlingua-onnx")]
fn select_keep_indices(
    keep_prob: &[f32],
    force_mask: &[bool],
    rate: f32,
) -> std::collections::BTreeSet<usize> {
    use std::collections::BTreeSet;
    let n = keep_prob.len();
    debug_assert_eq!(force_mask.len(), n);

    if n == 0 {
        return BTreeSet::new();
    }
    let rate = rate.clamp(0.0, 1.0);
    let n_total_keep = ((n as f32 * rate).round() as usize).clamp(1, n);
    let n_force = force_mask.iter().filter(|x| **x).count();
    let n_topk = n_total_keep.saturating_sub(n_force);

    let mut indexed: Vec<(usize, f32)> = (0..n)
        .filter(|i| !force_mask[*i])
        .map(|i| (i, keep_prob[i]))
        .collect();
    // Sort descending by keep-prob. NaN ties broken by ascending index
    // (deterministic; matches Rust's `sort_by` on f64 NaN but for f32).
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let top_idx: BTreeSet<usize> = indexed.iter().take(n_topk).map(|(i, _)| *i).collect();

    (0..n)
        .filter(|i| force_mask[*i] || top_idx.contains(i))
        .collect()
}

/// Pure asset-path resolver. Returns `(model_path, tokenizer_path, default_dir)`.
///
/// `model_env` / `tokenizer_env` are the values of `$MEMRIGHT_LLMLINGUA_MODEL`
/// / `$MEMRIGHT_LLMLINGUA_TOKENIZER`, or `None` if unset. Pulled out of
/// `llmlingua_asset_paths` for testability (no env mutation in tests).
///
/// `manifest_dir` is the crate root at build time (use `env!("CARGO_MANIFEST_DIR")`).
/// PathBuf::join + Path::is_file are cross-platform, so this is OS-independent.
#[cfg(feature = "llmlingua-onnx")]
fn resolve_asset_paths(
    manifest_dir: &std::path::Path,
    model_env: Option<&str>,
    tokenizer_env: Option<&str>,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let default_dir = manifest_dir.join("assets").join("llmlingua");
    let model = model_env
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| default_dir.join("model.onnx"));
    let tokenizer = tokenizer_env
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| default_dir.join("tokenizer.json"));
    (model, tokenizer, default_dir)
}

#[cfg(feature = "llmlingua-onnx")]
fn llmlingua_asset_paths() -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let model_env = std::env::var("MEMRIGHT_LLMLINGUA_MODEL").ok();
    let tok_env = std::env::var("MEMRIGHT_LLMLINGUA_TOKENIZER").ok();
    let (model, tokenizer, default_dir) =
        resolve_asset_paths(&manifest, model_env.as_deref(), tok_env.as_deref());

    if !model.is_file() {
        return Err(format!(
            "model not found at {} (set $MEMRIGHT_LLMLINGUA_MODEL, or populate {} from the spike export per crates/memright/assets/llmlingua/README.md)",
            model.display(),
            default_dir.display()
        ));
    }
    if !tokenizer.is_file() {
        return Err(format!("tokenizer not found at {}", tokenizer.display()));
    }
    Ok((model, tokenizer))
}

/// Returns true iff both `model.onnx` and `tokenizer.json` are reachable at the
/// resolved default paths (or via env vars). Lets the parity test skip cleanly
/// in CI checkouts without the 677MB asset.
#[cfg(feature = "llmlingua-onnx")]
#[cfg_attr(not(test), allow(dead_code))]
fn assets_present() -> bool {
    llmlingua_asset_paths().is_ok()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(all(test, feature = "llmlingua-onnx"))]
mod tests {
    use super::*;

    #[test]
    fn token_chunks_cover_long_inputs_without_truncation() {
        let tokens: Vec<usize> = (0..1_025).collect();
        let chunks: Vec<Vec<usize>> = token_chunks(&tokens).map(|chunk| chunk.to_vec()).collect();
        assert_eq!(
            chunks.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![512, 512, 1]
        );
        assert_eq!(chunks.into_iter().flatten().collect::<Vec<_>>(), tokens);
    }
    use std::collections::HashSet;

    // ------------------------------------------------------------------------
    // Always-on tests (no 677MB model, no tokenizer.json required).
    // These catch structural regressions in the pure algorithm pieces
    // so CI fails loudly even when the parity test is skipped.
    // ------------------------------------------------------------------------

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn softmax_keep_prob_picks_higher_logit() {
        // If keep-logit is much higher than drop-logit, keep-prob → 1.
        let p = softmax_keep_prob_two_class(&[-10.0, 10.0]);
        assert!(approx_eq(p, 1.0), "expected ~1.0, got {p}");
        // Symmetric.
        let p = softmax_keep_prob_two_class(&[10.0, -10.0]);
        assert!(approx_eq(p, 0.0), "expected ~0.0, got {p}");
        // Equal logits → 0.5.
        let p = softmax_keep_prob_two_class(&[1.5, 1.5]);
        assert!(approx_eq(p, 0.5), "expected 0.5, got {p}");
    }

    #[test]
    fn softmax_keep_prob_numerically_stable_for_large_logits() {
        // Without subtract-max, exp(1000) overflows. With it, both inputs share
        // the same shift so the ratio is stable.
        let p = softmax_keep_prob_two_class(&[1000.0, 1001.0]);
        assert!(p > 0.0 && p < 1.0, "expected in (0,1), got {p}");
        // The logit-difference is 1, so keep-prob = e^1 / (1 + e^1) ≈ 0.7311.
        assert!(approx_eq(p, 0.731_058_6), "expected ~0.7311, got {p}");
    }

    #[test]
    fn select_keep_indices_respects_rate() {
        // 10 tokens, all with equal keep-prob (no force). rate=0.5 → keep 5.
        let keep_prob = vec![0.5; 10];
        let force_mask = vec![false; 10];
        let kept = select_keep_indices(&keep_prob, &force_mask, 0.5);
        assert_eq!(kept.len(), 5);
    }

    #[test]
    fn select_keep_indices_force_overrides_topk() {
        // 10 tokens, force-keep the lowest-3 by keep-prob. rate=0.5 → keep 5,
        // but 3 of those slots are forced; top-K fills the remaining 2 from
        // the non-forced set.
        // Initial keep_prob is descending: index 0 = 1.0, index 9 = 0.1.
        // Then we zero out indices 0..3 so the forced tokens have the LOWEST
        // keep-prob — they would never be picked by top-K alone, but the
        // force-mask keeps them anyway.
        let mut keep_prob: Vec<f32> = (0..10).map(|i| 1.0 - i as f32 * 0.1).collect();
        keep_prob[0] = 0.0;
        keep_prob[1] = 0.0;
        keep_prob[2] = 0.0;
        let force_mask = vec![
            true, true, true, false, false, false, false, false, false, false,
        ];
        let kept = select_keep_indices(&keep_prob, &force_mask, 0.5);
        assert!(kept.contains(&0), "forced token 0 must be kept");
        assert!(kept.contains(&1), "forced token 1 must be kept");
        assert!(kept.contains(&2), "forced token 2 must be kept");
        assert_eq!(kept.len(), 5, "rate=0.5 → 5 kept, got {kept:?}");
        // The 2 non-forced slots go to the highest remaining keep-prob tokens
        // (indexes 3 and 4 with probs 0.7 and 0.6).
        assert!(
            kept.contains(&3) && kept.contains(&4),
            "expected top-prob tokens 3 and 4 in {kept:?}"
        );
    }

    #[test]
    fn select_keep_indices_force_exceeds_budget_shrinks_topk() {
        // 5 tokens, all forced. rate=0.5 → budget 3, but 5 forced → topk=0,
        // all 5 forced are kept (over budget, accepted for simplicity).
        let keep_prob = vec![0.1; 5];
        let force_mask = vec![true; 5];
        let kept = select_keep_indices(&keep_prob, &force_mask, 0.5);
        assert_eq!(kept.len(), 5);
    }

    #[test]
    fn select_keep_indices_rate_zero_keeps_only_force() {
        // rate=0 → budget is clamped to 1; with no force, top-K picks 1 token.
        // With force, force wins.
        let keep_prob = vec![0.5; 5];
        let force_mask = vec![false, true, false, false, false];
        let kept = select_keep_indices(&keep_prob, &force_mask, 0.0);
        assert_eq!(kept, [1].into_iter().collect());
    }

    #[test]
    fn select_keep_indices_empty_input() {
        let kept = select_keep_indices(&[], &[], 0.5);
        assert!(kept.is_empty());
    }

    #[test]
    fn budget_compression_keeps_critical_lines_exactly() {
        let src = concat!(
            "This ordinary prose has enough words to be shortened substantially.\n",
            "Error: request_id=req_42 at /srv/app.rs:19 must not retry 503\n",
            "See https://example.test/run/42 for details.\n",
            "```rust\nlet stable_id = 9;\n```\n",
            "More ordinary prose which should be compacted under a small budget.\n"
        );
        let result = compress_to_budget_with_options(src, 80, true);
        assert!(result.budget_met, "result: {result:?}");
        for critical in [
            "Error: request_id=req_42 at /srv/app.rs:19 must not retry 503\n",
            "See https://example.test/run/42 for details.\n",
            "```rust\nlet stable_id = 9;\n```\n",
        ] {
            assert!(
                result.text.contains(critical),
                "missing {critical:?}: {}",
                result.text
            );
        }
    }

    #[test]
    fn budget_compression_reports_protected_overflow_without_dropping() {
        let src = "Error: request_id=req_42 at /srv/app.rs:19 must not retry 503\n";
        let result = compress_to_budget_with_options(src, 1, true);
        assert!(!result.budget_met);
        assert_eq!(result.text, src);
        assert!(result.protected_tokens > result.budget_tokens);
    }

    #[test]
    fn rate_api_remains_independent_from_budget_path() {
        let src = "Ordinary prose remains on legacy rate semantics.";
        assert_eq!(compress(src, 1.0), compress_with_options(src, 1.0, false));
    }

    #[test]
    fn drop_manifest_marks_identifier_and_error_loss_high_risk() {
        let manifest = drop_manifest(
            "Error: request_id=req_42 failed at workerTask 503\n",
            "summary\n",
        );
        assert_eq!(manifest.dropped_error_lines, 1);
        assert!(manifest.dropped_identifiers >= 2, "{manifest:?}");
        assert!(manifest.dropped_numeric_literals >= 1, "{manifest:?}");
        assert_eq!(manifest.risk, "high");
        assert!(drop_manifest_meta(&manifest).contains("risk=high"));
    }

    #[test]
    fn resolve_asset_paths_default_dir_under_manifest() {
        // No env vars → both assets resolve to manifest/assets/llmlingua/{name}.
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let (model, tok, default_dir) = resolve_asset_paths(&manifest, None, None);
        assert_eq!(default_dir, manifest.join("assets").join("llmlingua"));
        assert_eq!(model, default_dir.join("model.onnx"));
        assert_eq!(tok, default_dir.join("tokenizer.json"));
        // The default dir is built with PathBuf::join → native separators.
        // The string round-trips identically regardless of platform.
        let manifest_str = manifest.to_str().expect("manifest is valid UTF-8");
        let default_str = default_dir.to_str().expect("default_dir is valid UTF-8");
        assert!(
            default_str.contains(manifest_str),
            "default dir {default_str} should be under manifest {manifest_str}"
        );
    }

    #[test]
    fn resolve_asset_paths_env_var_overrides_default() {
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // POSIX-style override (PathBuf handles separators correctly on both OSes).
        let (model, tok, default_dir) = resolve_asset_paths(
            &manifest,
            Some("/opt/llmlingua/model.onnx"),
            Some("/opt/llmlingua/tok.json"),
        );
        assert_eq!(model, std::path::PathBuf::from("/opt/llmlingua/model.onnx"));
        assert_eq!(tok, std::path::PathBuf::from("/opt/llmlingua/tok.json"));
        // default_dir is unchanged regardless of env override.
        assert_eq!(default_dir, manifest.join("assets").join("llmlingua"));
    }

    #[test]
    fn resolve_asset_paths_windows_style_env_var_preserved() {
        // On Windows, a backslash-separated absolute path must come through
        // verbatim (PathBuf::from doesn't normalize env-var input; the user
        // gets exactly what they put in the env var). PathBuf::join, by
        // contrast, normalizes separators — that's the difference between
        // the "env var override" branch and the "default" branch.
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let win_model = r"C:\Users\test\llmlingua\model.onnx";
        let win_tok = r"C:\Users\test\llmlingua\tokenizer.json";
        let (model, tok, default_dir) =
            resolve_asset_paths(&manifest, Some(win_model), Some(win_tok));
        // The env-var input is preserved exactly. (On POSIX this test still
        // passes because PathBuf::from is a no-op on the input string —
        // there's no implicit separator conversion.)
        assert_eq!(model.as_os_str(), win_model);
        assert_eq!(tok.as_os_str(), win_tok);
        // The default-dir branch still goes through PathBuf::join, which
        // normalizes separators — that's the platform-correct behavior.
        let expected_default = manifest.join("assets").join("llmlingua");
        assert_eq!(default_dir, expected_default);
        // Sanity: the default dir, when stringified, contains the manifest path.
        assert!(
            default_dir
                .to_str()
                .unwrap()
                .contains(manifest.to_str().unwrap()),
            "default dir {} should contain manifest {}",
            default_dir.display(),
            manifest.display()
        );
    }

    #[test]
    fn missing_model_error_mentions_default_dir_and_readme_pointer() {
        // Set the env var to a path that definitely doesn't exist, so we hit
        // the "model not found" branch regardless of whether the local assets
        // dir is populated. (Env mutation in parallel tests is racy; use a
        // marker that no other test uses.)
        let bogus = std::env::temp_dir()
            .join("memright-asset-test-DO-NOT-EXIST")
            .join("model.onnx");
        // SAFETY: this test is the only writer of `MEMRIGHT_LLMLINGUA_MODEL`
        // and it sets/restores it within the test body. cargo's default test
        // runner runs tests on multiple threads but writes to env vars here
        // are best-effort — other tests in this module don't read this var.
        // If running with `--test-threads=1` (CI), it's deterministic.
        unsafe {
            std::env::set_var("MEMRIGHT_LLMLINGUA_MODEL", &bogus);
        }
        let result = llmlingua_asset_paths();
        unsafe {
            std::env::remove_var("MEMRIGHT_LLMLINGUA_MODEL");
        }
        match result {
            Err(msg) => {
                let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                let expected_default = manifest.join("assets").join("llmlingua");
                assert!(
                    msg.contains(expected_default.to_str().unwrap()),
                    "error should mention default dir; got: {msg}"
                );
                assert!(
                    msg.contains("README.md"),
                    "error should point at README.md; got: {msg}"
                );
            }
            Ok((m, _)) => panic!(
                "expected Err for bogus path {}; got Ok with model={}",
                bogus.display(),
                m.display()
            ),
        }
    }

    // ------------------------------------------------------------------------
    // Conditional tests (require the 2.9MB tokenizer.json; skip silently if
    // absent so CI checkouts without the asset still pass).
    // ------------------------------------------------------------------------

    #[test]
    fn tokenizer_loads_when_asset_present() {
        if !assets_present() {
            eprintln!(
                "tokenizer_loads_when_asset_present: assets not present at default path, skipping (CI without model.onnx + tokenizer.json)"
            );
            return;
        }
        let (_model_path, tok_path) = llmlingua_asset_paths().expect("assets present");
        let tok = tokenizers::Tokenizer::from_file(&tok_path).expect("tokenizer loads");
        // Round-trip a tiny input — exercises encode + decode without running
        // the model. Catches path-resolution and tokenizer-config regressions.
        let enc = tok.encode("Hello, world.", false).expect("encode ok");
        assert!(!enc.get_ids().is_empty(), "expected non-empty ids");
        let decoded = tok.decode(enc.get_ids(), true).expect("decode ok");
        assert!(!decoded.is_empty(), "expected non-empty decode");
    }

    // ------------------------------------------------------------------------
    // Parity test (requires both model.onnx + tokenizer.json; skips silently
    // if absent). This is the plan's gate — see `docs/plans/2026-07-01-...md`
    // task 3. Spikes' word-Jaccard results at rate=0.5: min 0.93, mean 0.95
    // (see `spike/SPIKE-RESULT.md`). Threshold is the plan's gate.
    // ------------------------------------------------------------------------

    const THRESHOLD: f64 = 0.90;

    fn word_set(s: &str) -> HashSet<String> {
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|p| !p.is_empty())
            .map(|p| p.to_lowercase())
            .collect()
    }

    fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }
        let inter = a.intersection(b).count() as f64;
        let union = a.union(b).count() as f64;
        if union == 0.0 {
            1.0
        } else {
            inter / union
        }
    }

    #[test]
    fn compress_matches_python_jaccard() {
        if !assets_present() {
            eprintln!(
                "compress_matches_python_jaccard: assets not present at default path, skipping (CI without model.onnx + tokenizer.json)"
            );
            return;
        }

        // The 5 fixtures are embedded to keep the test self-contained —
        // no filesystem dependency at test time (other than the model + tokenizer
        // assets, which we already verified are present above).
        const FIXTURES: &[(&str, &str)] = &[
            (
                "01_meeting.txt",
                include_str!("../../../spike/fixtures/01_meeting.txt"),
            ),
            (
                "02_readme.txt",
                include_str!("../../../spike/fixtures/02_readme.txt"),
            ),
            (
                "03_incident.txt",
                include_str!("../../../spike/fixtures/03_incident.txt"),
            ),
            (
                "04_doc.txt",
                include_str!("../../../spike/fixtures/04_doc.txt"),
            ),
            (
                "05_plan.txt",
                include_str!("../../../spike/fixtures/05_plan.txt"),
            ),
        ];

        let golden_str = include_str!("../tests/llmlingua_golden.json");
        let golden: serde_json::Value =
            serde_json::from_str(golden_str).expect("golden JSON parses");

        let rate = 0.5_f32;
        let mut min_score = f64::INFINITY;
        let mut mean_sum = 0.0_f64;
        let mut n_tested = 0_usize;

        for (name, text) in FIXTURES {
            let rust_out = compress(text, rate);
            let py_compressed = golden[name]["compressed"]
                .as_str()
                .unwrap_or_else(|| panic!("missing golden entry for {name}"));

            let rust_words = word_set(&rust_out);
            let py_words = word_set(py_compressed);
            let score = jaccard(&rust_words, &py_words);
            min_score = min_score.min(score);
            mean_sum += score;
            n_tested += 1;

            assert!(
                score >= THRESHOLD,
                "fixture {name}: word-Jaccard {score:.4} < threshold {THRESHOLD}\n\
                 rust out: {rust_out:?}\n\
                 py out:    {py_compressed:?}",
            );
        }
        let mean = mean_sum / n_tested as f64;
        eprintln!(
            "compress_matches_python_jaccard: min={min_score:.4} mean={mean:.4} threshold={THRESHOLD} (n={n_tested})"
        );
    }
}
