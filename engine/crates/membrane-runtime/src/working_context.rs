//! MBR-505: auto-use bounded working context.
//!
//! The runtime selects and renders a working context automatically, but only
//! inside an explicit [`WorkingContextBudgetV1`] cap. The pure
//! [`select_working_context`] helper walks a candidate list in order and
//! stops the moment any of the three caps is hit, marking the resulting
//! [`WorkingContextSelectionV1`] as `exceeded` with a stable lowercase
//! reason (`max_blocks`, `max_bytes`, or `max_recent_turns`). When the
//! selection exceeds the budget the runtime emits a typed
//! `context.budget_exceeded` envelope — see [`render_working_context`] —
//! and the calling layer falls back to metadata-only delivery rather than
//! overflowing the bounded packet.
//!
//! The mirror TypeScript twin lives in `mcp/working-context.mjs`. Both
//! sides must agree byte-for-byte on the canonical digest and on the
//! `kind` label so a downstream Hub or federation gateway can dispatch
//! on a single normalized schema.

use membrane_protocol::canonical::canonical_digest_of;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Schema version of the bounded budget itself. Bumped on incompatible
/// field changes; downstream consumers refuse unknown versions.
pub const WORKING_CONTEXT_BUDGET_SCHEMA_VERSION: u32 = 1;

/// Schema version of the selection result. Bumped when the shape of the
/// returned selection changes; the envelope carries its own version too.
pub const WORKING_CONTEXT_SELECTION_SCHEMA_VERSION: u32 = 1;

/// Schema version of the rendered envelope.
pub const WORKING_CONTEXT_ENVELOPE_SCHEMA_VERSION: u32 = 1;

/// Stable kind label emitted when the selection fit inside the budget.
pub const WORKING_CONTEXT_KIND_NORMAL: &str = "context.working_context";

/// Stable kind label emitted when the selection had to drop blocks to
/// fit the budget. Downstream consumers switch to metadata-only delivery
/// when they observe this kind.
pub const WORKING_CONTEXT_KIND_BUDGET_EXCEEDED: &str = "context.budget_exceeded";

/// The three explicit caps the runtime honors when it auto-selects a
/// working context. Every cap must be positive; a zero is a caller bug
/// and the runtime surfaces it as [`WorkingContextError::BudgetInvalid`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkingContextBudgetV1 {
    /// Maximum number of candidate blocks the selection is allowed to admit.
    pub max_blocks: u32,
    /// Maximum cumulative byte size (UTF-8 or any byte-cost convention the
    /// caller chooses; the runtime treats it as a u64 sum of `candidate_bytes`).
    pub max_bytes: u32,
    /// Maximum number of recent turns the selection is allowed to draw
    /// from. Mirrors the `max_blocks` cap but enforces recency directly:
    /// the Nth candidate is dropped once `n >= max_recent_turns`.
    pub max_recent_turns: u32,
}

/// Result of running [`select_working_context`]: the blocks that fit
/// inside the budget, their cumulative byte cost, and a flag indicating
/// whether the runtime had to drop candidates to fit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkingContextSelectionV1 {
    pub schema_version: u32,
    /// Block ids the runtime admitted, in input order. Empty iff the
    /// first candidate already exceeded a cap.
    pub selected_blocks: Vec<String>,
    /// Cumulative byte size of `selected_blocks`. Equal to the sum of
    /// `candidate_bytes[i]` for every i that was admitted.
    pub selected_bytes: u64,
    /// The budget the caller asked for; echoed back so the rendered
    /// envelope carries the policy alongside the result.
    pub budget: WorkingContextBudgetV1,
    /// True iff the selection had to drop at least one candidate to fit
    /// the budget. The caller is expected to render a
    /// `context.budget_exceeded` envelope and fall back to metadata-only
    /// delivery when this is set.
    pub exceeded: bool,
    /// Stable lowercase reason for `exceeded`. Absent when `exceeded` is
    /// false. One of `"max_blocks"`, `"max_bytes"`, `"max_recent_turns"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exceeded_reason: Option<String>,
}

/// Typed envelope wrapping a [`WorkingContextSelectionV1`]. The `digest`
/// is a `sha256:<hex>` over the canonical-JSON of every other field —
/// the same self-referential pattern `observable_event` uses — so a
/// tampered selection is caught by [`verify_envelope`] before it reaches
/// the durable store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkingContextEnvelopeV1 {
    pub schema_version: u32,
    /// Lowercase UUIDv4, freshly minted by [`render_working_context`].
    pub envelope_id: String,
    /// Either [`WORKING_CONTEXT_KIND_NORMAL`] or
    /// [`WORKING_CONTEXT_KIND_BUDGET_EXCEEDED`], decided by the selection.
    pub kind: String,
    pub selection: WorkingContextSelectionV1,
    /// Unix milliseconds at the producing runtime (RFC-style instant; the
    /// durable store additionally stamps its own `now_unix_ms`).
    pub emitted_at_unix_ms: u64,
    /// `sha256:<hex>` digest of the canonical-JSON of every OTHER field.
    /// See [`verify_envelope`] for the re-computation rule.
    pub digest: String,
}

/// Errors [`select_working_context`] can surface to callers. The runtime
/// never fabricates a synthetic envelope on these errors — the calling
/// layer is responsible for choosing the right fallback.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkingContextError {
    /// The caller passed a zero cap, or the two parallel input slices
    /// had mismatched lengths. Always a caller bug; the runtime cannot
    /// recover.
    #[error("working context budget invalid: {reason}")]
    BudgetInvalid { reason: String },
    /// The candidate list was empty. Surfaced as its own variant so the
    /// runtime can distinguish "nothing to render" from "budget rejected
    /// everything".
    #[error("working context selection has no candidates")]
    NoCandidates,
    /// Reserved for a future bind that reads candidates from the durable
    /// store. Carries the underlying reason verbatim so the operator can
    /// diagnose without grepping logs.
    #[error("working context store error: {reason}")]
    Store { reason: String },
}

/// Pure selection function. Iterates `candidates` in input order,
/// admitting each one as long as all three caps remain satisfied; the
/// moment a cap would be violated, the selection stops and the result
/// is marked `exceeded = true` with a stable lowercase reason. Returns
/// `Err` on caller-bug conditions (length mismatch, zero cap, empty
/// input) — never fabricates a fake selection to paper over them.
pub fn select_working_context(
    candidates: &[String],
    candidate_bytes: &[u64],
    budget: &WorkingContextBudgetV1,
) -> Result<WorkingContextSelectionV1, WorkingContextError> {
    if candidates.len() != candidate_bytes.len() {
        return Err(WorkingContextError::BudgetInvalid {
            reason: format!(
                "candidates.len ({}) must equal candidate_bytes.len ({})",
                candidates.len(),
                candidate_bytes.len()
            ),
        });
    }
    if candidates.is_empty() {
        return Err(WorkingContextError::NoCandidates);
    }
    if budget.max_blocks == 0 || budget.max_bytes == 0 || budget.max_recent_turns == 0 {
        return Err(WorkingContextError::BudgetInvalid {
            reason: format!(
                "budget caps must all be positive (got max_blocks={}, max_bytes={}, max_recent_turns={})",
                budget.max_blocks, budget.max_bytes, budget.max_recent_turns
            ),
        });
    }

    let max_blocks = budget.max_blocks as usize;
    let max_bytes = budget.max_bytes as u64;
    let max_recent_turns = budget.max_recent_turns as usize;

    let mut selected_blocks: Vec<String> = Vec::new();
    let mut selected_bytes: u64 = 0;
    let mut exceeded = false;
    let mut exceeded_reason: Option<String> = None;

    for (index, (block_id, bytes)) in candidates.iter().zip(candidate_bytes.iter()).enumerate() {
        // Recency cap is checked first so a stale block never wins over
        // a fresh one just because the latter happens to be larger.
        if index >= max_recent_turns {
            exceeded = true;
            exceeded_reason = Some("max_recent_turns".to_string());
            break;
        }
        if selected_blocks.len() >= max_blocks {
            exceeded = true;
            exceeded_reason = Some("max_blocks".to_string());
            break;
        }
        // Saturating add guards against a u32 budget overflow when a
        // candidate's bytes alone exceed `max_bytes`; in that case the
        // selection correctly reports `exceeded = max_bytes` and stops
        // without admitting the oversized block.
        let prospective = selected_bytes.saturating_add(*bytes);
        if prospective > max_bytes {
            exceeded = true;
            exceeded_reason = Some("max_bytes".to_string());
            break;
        }
        selected_blocks.push(block_id.clone());
        selected_bytes = prospective;
    }

    Ok(WorkingContextSelectionV1 {
        schema_version: WORKING_CONTEXT_SELECTION_SCHEMA_VERSION,
        selected_blocks,
        selected_bytes,
        budget: budget.clone(),
        exceeded,
        exceeded_reason,
    })
}

/// Wrap a selection into a typed envelope, minting a fresh `envelope_id`
/// and computing the self-referential `digest`. The `kind` label is
/// chosen from the selection's `exceeded` flag so a downstream consumer
/// can dispatch on a single string and never has to inspect the inner
/// payload to decide whether delivery should fall back to metadata-only.
pub fn render_working_context(
    selection: &WorkingContextSelectionV1,
    now_unix_ms: u64,
) -> WorkingContextEnvelopeV1 {
    let kind = if selection.exceeded {
        WORKING_CONTEXT_KIND_BUDGET_EXCEEDED
    } else {
        WORKING_CONTEXT_KIND_NORMAL
    };
    let mut envelope = WorkingContextEnvelopeV1 {
        schema_version: WORKING_CONTEXT_ENVELOPE_SCHEMA_VERSION,
        envelope_id: mint_v4_uuid(),
        kind: kind.to_string(),
        selection: selection.clone(),
        emitted_at_unix_ms: now_unix_ms,
        digest: String::new(),
    };
    envelope.digest = recompute_digest(&envelope);
    envelope
}

/// Recompute the self-referential digest by stripping the `digest`
/// field, canonicalizing the rest, and hashing the canonical bytes.
/// Mirrors the pattern `canonical_digest_of` uses for the other protocol
/// shapes so the TypeScript twin can reproduce it byte-for-byte.
fn recompute_digest(envelope: &WorkingContextEnvelopeV1) -> String {
    let mut without_digest =
        serde_json::to_value(envelope).expect("working context envelope serializes to JSON");
    if let Some(object) = without_digest.as_object_mut() {
        object.remove("digest");
    }
    canonical_digest_of(&without_digest)
}

/// Validate envelope self-integrity. Rejects unknown schema versions so
/// a future v2 envelope cannot be silently parsed as v1, and re-runs the
/// digest check so a tampered selection is caught before the envelope
/// reaches the durable store.
pub fn verify_envelope(env: &WorkingContextEnvelopeV1) -> Result<(), String> {
    if env.schema_version != WORKING_CONTEXT_ENVELOPE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported working_context envelope schema_version {} (expected {})",
            env.schema_version, WORKING_CONTEXT_ENVELOPE_SCHEMA_VERSION
        ));
    }
    let expected = recompute_digest(env);
    if expected != env.digest {
        return Err(format!(
            "working_context envelope digest mismatch: expected {expected}, got {}",
            env.digest
        ));
    }
    Ok(())
}

/// Mint a fresh lowercase UUIDv4. Wraps `getrandom` so the call site is
/// infallible at the type level (we retry once on the rare zero-byte
/// case, matching `observable_event::mint_v4_uuid`).
fn mint_v4_uuid() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).unwrap_or_else(|_| {
        let _ = getrandom::fill(&mut bytes);
    });
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(max_blocks: u32, max_bytes: u32, max_recent_turns: u32) -> WorkingContextBudgetV1 {
        WorkingContextBudgetV1 {
            max_blocks,
            max_bytes,
            max_recent_turns,
        }
    }

    #[test]
    fn empty_candidates_returns_no_candidates_error() {
        let candidates: Vec<String> = Vec::new();
        let bytes: Vec<u64> = Vec::new();
        let budget = budget(4, 1024, 4);
        let err = select_working_context(&candidates, &bytes, &budget).unwrap_err();
        assert_eq!(err, WorkingContextError::NoCandidates);
    }

    #[test]
    fn zero_budget_returns_budget_invalid_error() {
        let candidates = vec!["block-a".to_string()];
        let bytes = vec![100u64];
        // max_blocks = 0
        let err = select_working_context(&candidates, &bytes, &budget(0, 1024, 4)).unwrap_err();
        assert!(matches!(err, WorkingContextError::BudgetInvalid { .. }));
        // max_bytes = 0
        let err = select_working_context(&candidates, &bytes, &budget(4, 0, 4)).unwrap_err();
        assert!(matches!(err, WorkingContextError::BudgetInvalid { .. }));
        // max_recent_turns = 0
        let err = select_working_context(&candidates, &bytes, &budget(4, 1024, 0)).unwrap_err();
        assert!(matches!(err, WorkingContextError::BudgetInvalid { .. }));
    }

    #[test]
    fn selection_within_budget_marks_exceeded_false() {
        let candidates = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let bytes = vec![100u64, 100, 100];
        let selection = select_working_context(&candidates, &bytes, &budget(8, 1024, 8)).unwrap();
        assert!(!selection.exceeded);
        assert!(selection.exceeded_reason.is_none());
        assert_eq!(selection.selected_blocks, candidates);
        assert_eq!(selection.selected_bytes, 300);
        assert_eq!(
            selection.schema_version,
            WORKING_CONTEXT_SELECTION_SCHEMA_VERSION
        );
    }

    #[test]
    fn selection_over_max_blocks_marks_exceeded_with_max_blocks_reason() {
        let candidates: Vec<String> = (0..10).map(|i| format!("block-{i}")).collect();
        let bytes = vec![1u64; 10];
        let selection = select_working_context(&candidates, &bytes, &budget(3, 1024, 10)).unwrap();
        assert!(selection.exceeded);
        assert_eq!(selection.exceeded_reason.as_deref(), Some("max_blocks"));
        assert_eq!(selection.selected_blocks.len(), 3);
        assert_eq!(selection.selected_bytes, 3);
    }

    #[test]
    fn selection_over_max_bytes_marks_exceeded_with_max_bytes_reason() {
        let candidates: Vec<String> = (0..5).map(|i| format!("block-{i}")).collect();
        let bytes = vec![1000u64, 1000, 1000, 1000, 1000]; // 5000 total
        let selection = select_working_context(&candidates, &bytes, &budget(8, 1024, 8)).unwrap();
        assert!(selection.exceeded);
        assert_eq!(selection.exceeded_reason.as_deref(), Some("max_bytes"));
        // The first candidate (1000) fits; the second (cumulative 2000)
        // exceeds 1024 and is rejected, so only the first block is admitted.
        assert_eq!(selection.selected_blocks, vec!["block-0".to_string()]);
        assert_eq!(selection.selected_bytes, 1000);
    }

    #[test]
    fn envelope_digest_is_stable_for_equalized_envelopes() {
        let candidates = vec!["a".to_string(), "b".to_string()];
        let bytes = vec![10u64, 20];
        let selection = select_working_context(&candidates, &bytes, &budget(8, 1024, 8)).unwrap();

        // Build two envelopes with the SAME envelope_id and emitted_at
        // so the digest can be equalized, then recompute on both.
        let mut a = render_working_context(&selection, 1_700_000_000_000);
        let mut b = render_working_context(&selection, 1_700_000_000_000);
        a.envelope_id = "11111111-1111-4111-8111-111111111111".to_string();
        b.envelope_id = "11111111-1111-4111-8111-111111111111".to_string();
        a.emitted_at_unix_ms = 1_700_000_000_000;
        b.emitted_at_unix_ms = 1_700_000_000_000;
        a.digest = recompute_digest(&a);
        b.digest = recompute_digest(&b);
        assert_eq!(a.digest, b.digest);
        // And the resulting digests must match the envelopes' own digest
        // field after recompute.
        assert_eq!(a.digest, recompute_digest(&a));
        assert_eq!(b.digest, recompute_digest(&b));
    }

    #[test]
    fn verify_envelope_passes_on_a_fresh_envelope_and_rejects_tampering() {
        let candidates = vec!["a".to_string()];
        let bytes = vec![1u64];
        let selection = select_working_context(&candidates, &bytes, &budget(4, 1024, 4)).unwrap();
        let envelope = render_working_context(&selection, 1_700_000_000_001);
        verify_envelope(&envelope).expect("fresh envelope must verify");

        // Tamper with the selection; the digest must mismatch.
        let mut tampered = envelope.clone();
        tampered.selection.selected_blocks.push("rogue".to_string());
        tampered.selection.selected_bytes += 1;
        let err = verify_envelope(&tampered).expect_err("tampered selection must not verify");
        assert!(err.contains("digest mismatch"), "unexpected error: {err}");
    }

    #[test]
    fn render_chooses_kind_based_on_exceeded_flag() {
        // Within budget → context.working_context
        let within =
            select_working_context(&["a".to_string()], &[10u64], &budget(4, 1024, 4)).unwrap();
        let env_within = render_working_context(&within, 1_700_000_000_002);
        assert_eq!(env_within.kind, WORKING_CONTEXT_KIND_NORMAL);
        assert_eq!(env_within.kind, "context.working_context");

        // Over budget → context.budget_exceeded
        let over = select_working_context(
            &(0..10).map(|i| format!("b-{i}")).collect::<Vec<_>>(),
            &vec![1u64; 10],
            &budget(2, 1024, 10),
        )
        .unwrap();
        let env_over = render_working_context(&over, 1_700_000_000_003);
        assert_eq!(env_over.kind, WORKING_CONTEXT_KIND_BUDGET_EXCEEDED);
        assert_eq!(env_over.kind, "context.budget_exceeded");
    }

    #[test]
    fn mismatched_candidate_and_byte_lengths_return_budget_invalid() {
        let candidates = vec!["a".to_string(), "b".to_string()];
        let bytes = vec![1u64];
        let err = select_working_context(&candidates, &bytes, &budget(4, 1024, 4)).unwrap_err();
        assert!(matches!(err, WorkingContextError::BudgetInvalid { .. }));
    }

    #[test]
    fn selection_over_max_recent_turns_marks_exceeded_with_recency_reason() {
        let candidates: Vec<String> = (0..5).map(|i| format!("turn-{i}")).collect();
        let bytes = vec![1u64; 5];
        let selection = select_working_context(&candidates, &bytes, &budget(8, 1024, 2)).unwrap();
        assert!(selection.exceeded);
        assert_eq!(
            selection.exceeded_reason.as_deref(),
            Some("max_recent_turns")
        );
        assert_eq!(selection.selected_blocks.len(), 2);
    }
}
