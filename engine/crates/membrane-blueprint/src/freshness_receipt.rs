//! Native port of the full-receipt assembly rules from
//! `blueprint/src/graph/freshness-receipt.mjs` (`buildFreshnessReceipt`).
//!
//! [`crate::freshness`] already owns the pure decision axes -- deliberately
//! with no process/shell dependency: [`crate::freshness::evaluate_freshness`]
//! is the freshness axis (`fresh | changed_since_generation | unknown |
//! unavailable`) and [`crate::freshness::assert_generation_coherence`] is the
//! orthogonal generation-coherence axis, which fails closed on a pin mismatch
//! regardless of what freshness reports -- including `fresh`.
//!
//! This module owns the remaining deterministic rule from the legacy
//! module: given an already-observed generation basis, an already-observed
//! current source state, and (only when freshness is
//! `changed_since_generation`) an already-enumerated changed-path set, derive
//! the full `BlueprintFreshnessReceiptV1` shape -- `staleSources` filtered to
//! paths actually present in the generation, and the `suppression`
//! required/mode fields. Git enumeration and store I/O stay caller
//! responsibilities, consistent with the "no process/shell dependency"
//! design of [`crate::freshness`].

use serde::{Deserialize, Serialize};

use crate::freshness::{evaluate_freshness, CurrentSourceState, FreshnessState, GenerationFreshnessBasis};

pub const FRESHNESS_RECEIPT_SCHEMA: &str = "BlueprintFreshnessReceiptV1";

/// The result of `changedPathsSinceGeneration`: a caller-performed git
/// enumeration (committed diff + worktree diff + untracked files), or a
/// typed reason it could not be produced. `complete: false` requires the
/// caller to suppress every source-backed row instead of guessing freshness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedPaths {
    pub complete: bool,
    pub paths: Vec<String>,
    pub reason: Option<String>,
}

impl ChangedPaths {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self { complete: false, paths: Vec::new(), reason: Some(reason.into()) }
    }

    pub fn complete(paths: Vec<String>) -> Self {
        Self { complete: true, paths, reason: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuppressionMode {
    None,
    ChangedPaths,
    WholeGeneration,
}

impl SuppressionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ChangedPaths => "changed_paths",
            Self::WholeGeneration => "whole_generation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suppression {
    pub required: bool,
    pub mode: SuppressionMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreshnessReceipt {
    pub schema: &'static str,
    #[serde(rename = "generationId")]
    pub generation_id: Option<String>,
    #[serde(rename = "manifestDigest")]
    pub manifest_digest: Option<String>,
    pub generation: GenerationFreshnessBasis,
    pub current: CurrentSourceState,
    pub freshness: FreshnessState,
    #[serde(rename = "staleSources")]
    pub stale_sources: ChangedPaths,
    pub suppression: Suppression,
}

/// Build the full receipt from already-observed inputs.
///
/// `enumerate_changed` is invoked lazily, and only when freshness is exactly
/// `changed_since_generation` -- mirroring the legacy
/// `freshness === "changed_since_generation" ? enumerateChanged(...) : ...`
/// ternary, so a caller need not perform git enumeration on the `fresh`
/// path. `is_indexed_path` mirrors the `indexedPaths` filter drawn from
/// `generation_leaf` in the legacy module: only changed paths the generation
/// actually indexed are reported as stale sources.
pub fn build_freshness_receipt(
    generation_id: Option<String>,
    manifest_digest: Option<String>,
    generation: GenerationFreshnessBasis,
    current: CurrentSourceState,
    enumerate_changed: impl FnOnce() -> ChangedPaths,
    is_indexed_path: impl Fn(&str) -> bool,
) -> FreshnessReceipt {
    let freshness = evaluate_freshness(&generation, &current);
    let changed_raw = if freshness == FreshnessState::ChangedSinceGeneration {
        enumerate_changed()
    } else if freshness == FreshnessState::Fresh {
        ChangedPaths::complete(Vec::new())
    } else {
        ChangedPaths::unavailable(freshness.as_str())
    };
    let stale_sources = ChangedPaths {
        complete: changed_raw.complete,
        paths: changed_raw.paths.into_iter().filter(|path| is_indexed_path(path)).collect(),
        reason: changed_raw.reason,
    };
    let suppression = Suppression {
        required: freshness == FreshnessState::ChangedSinceGeneration,
        mode: if freshness != FreshnessState::ChangedSinceGeneration {
            SuppressionMode::None
        } else if stale_sources.complete {
            SuppressionMode::ChangedPaths
        } else {
            SuppressionMode::WholeGeneration
        },
    };
    FreshnessReceipt {
        schema: FRESHNESS_RECEIPT_SCHEMA,
        generation_id,
        manifest_digest,
        generation,
        current,
        freshness,
        stale_sources,
        suppression,
    }
}
