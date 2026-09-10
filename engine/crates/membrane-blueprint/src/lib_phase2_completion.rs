//! Native Rust port of `blueprint/src/lib/phase2-completion.mjs`.
//!
//! Lane LIB4 (r5 closure): this module has no prior native equivalent.
//!
//! `completePendingDocDomain` in the legacy module is a database
//! orchestration function: it reads a live `better-sqlite3` handle's
//! `watch_state`/generation-envelope tables, calls into
//! `../graph/delta-store.mjs` (`readPendingDomains`, `clearDomainPending`),
//! `../graph/static-provider.mjs` (`readGeneration`),
//! `../graph/store-sqlite.mjs` (`getGenerationEnvelope`), and
//! `./incremental-phase2.mjs` (`buildIncrementalPhase2Plan`,
//! `sealPhase2Artifacts`), and runs a `BEGIN IMMEDIATE` transaction against
//! that handle. None of those five sibling modules are in this lane's
//! 11-file scope, and the generation-envelope/watch_state schema they read
//! lives in `store.rs`/`graph.rs`, which lane rules forbid touching. A
//! faithful port of the orchestration therefore cannot land in this pass
//! without inventing unowned schema access or a second SQLite entry point.
//!
//! What IS ported here, faithfully and as pure functions with no DB/file
//! I/O, is every piece of decision logic that does not require the live
//! handle: the generation-fencing state machine (`noop` /
//! `superseded` / `complete` / `doc_current_phase2_pending` /
//! `pending_judgment`), the pending verdict/dimension counts derived from a
//! plan, and the on-disk artifact path layout under `outDir`. A caller that
//! already holds the store handle (once delta-store/store-sqlite are
//! themselves ported) can drive these pure functions from real DB state
//! without re-deriving this logic.

use std::path::{Path, PathBuf};

/// On-disk artifact paths for phase-2 completion, rooted at `root/outDir`.
/// Mirrors the path joins used throughout `phase2Inputs` /
/// `completePendingDocDomain` (`queue.json`, `verdicts.json`,
/// `understanding.json`, `phase2-plan.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase2Paths {
    pub base: PathBuf,
    pub queue: PathBuf,
    pub verdicts: PathBuf,
    pub understanding: PathBuf,
    pub plan: PathBuf,
}

/// Compute the phase-2 artifact paths under `root/out_dir`. Mirrors the
/// `resolve(root, outDir)` + `join(base, ...)` calls in `phase2Inputs` and
/// `completePendingDocDomain`.
pub fn phase2_paths(root: &Path, out_dir: &str) -> Phase2Paths {
    let base = if root.is_absolute() {
        root.join(out_dir)
    } else {
        std::env::current_dir()
            .unwrap_or_default()
            .join(root)
            .join(out_dir)
    };
    Phase2Paths {
        queue: base.join("queue.json"),
        verdicts: base.join("verdicts.json"),
        understanding: base.join("understanding.json"),
        plan: base.join("phase2-plan.json"),
        base,
    }
}

/// Pending counts derived from an incremental phase-2 plan. Mirrors the
/// `plan.verdicts?.verify?.length ?? 0` / `plan.dimensions?.synthesize?.length ?? 0`
/// expressions in `writeCompletionState` and the final result object.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PendingCounts {
    pub verify: usize,
    pub synthesize: usize,
}

/// Minimal shape of the incremental phase-2 plan this module needs.
/// `buildIncrementalPhase2Plan`'s full shape lives in
/// `incremental-phase2.mjs` (out of scope); this is the subset
/// `completePendingDocDomain` actually reads.
#[derive(Debug, Clone, Default)]
pub struct Phase2Plan {
    pub complete: bool,
    pub pending: PendingCounts,
}

/// The generation-fenced completion state, mirroring every distinct
/// `state` string the legacy function can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionState {
    /// `doc` was not pending; nothing to do. Mirrors `{ state: "noop" }`.
    Noop,
    /// The source generation advanced past the one this attempt was
    /// planning/sealing/committing against. Mirrors
    /// `{ state: "superseded" }` (returned from any of the three fencing
    /// checkpoints: before planning, after sealing, or inside the
    /// transaction).
    Superseded,
    /// No graph generation id could be determined at all. Mirrors the
    /// `!generationId` half of the `if (!generationId || ...)` guard,
    /// which also yields `"superseded"` in the legacy code (kept distinct
    /// here only for caller clarity about *why*; both map to the same JS
    /// string).
    MissingGeneration,
    /// The plan was fully reusable; verdicts/understanding were resealed
    /// and both `doc` and `semantic` pending marks were cleared. Mirrors
    /// `{ state: "complete" }`.
    Complete,
    /// The plan required fresh judgment work; `doc` was cleared but
    /// `semantic` stays pending, with an explicit `phase2-plan.json`
    /// receipt. Mirrors `{ state: "doc_current_phase2_pending" }`.
    DocCurrentPhase2Pending,
}

impl CompletionState {
    /// The `watch_state.phase2_completion_state` value `writeCompletionState`
    /// would persist for this outcome (`Complete` -> `"reused"`,
    /// `DocCurrentPhase2Pending` -> `"pending_judgment"`; the other
    /// variants never reach `writeCompletionState`, since they return
    /// before the commit).
    pub fn watch_state_value(&self) -> Option<&'static str> {
        match self {
            CompletionState::Complete => Some("reused"),
            CompletionState::DocCurrentPhase2Pending => Some("pending_judgment"),
            _ => None,
        }
    }
}

/// The full completion outcome, mirroring the frozen result object shape.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionOutcome {
    pub state: CompletionState,
    pub generation_id: Option<String>,
    pub phase2_complete: Option<bool>,
    pub verify: usize,
    pub synthesize: usize,
}

/// Decide the generation-fenced completion outcome given the doc-pending
/// flag, the graph's generation id, the store's *current* generation id at
/// each fencing checkpoint, and (once computed) the plan. This is the pure
/// core of `completePendingDocDomain`'s control flow, with all DB/file I/O
/// factored out to the caller.
///
/// `current_generation_at_commit` models the final `BEGIN IMMEDIATE`
/// fencing check; pass the same value as `current_generation_after_plan`
/// when the caller has no intermediate re-check point (i.e. collapses the
/// three legacy fencing checks into one, which is always at least as
/// strict).
pub fn decide_completion(
    doc_domain_pending: bool,
    current_generation_before_plan: Option<&str>,
    graph_generation_id: Option<&str>,
    current_generation_after_plan: Option<&str>,
    current_generation_at_commit: Option<&str>,
    plan: Option<&Phase2Plan>,
) -> CompletionOutcome {
    if !doc_domain_pending {
        return CompletionOutcome {
            state: CompletionState::Noop,
            generation_id: current_generation_before_plan.map(|s| s.to_string()),
            phase2_complete: None,
            verify: 0,
            synthesize: 0,
        };
    }

    let generation_id = match graph_generation_id {
        Some(g) if !g.is_empty() => g,
        _ => {
            return CompletionOutcome {
                state: CompletionState::MissingGeneration,
                generation_id: None,
                phase2_complete: None,
                verify: 0,
                synthesize: 0,
            }
        }
    };

    if current_generation_before_plan != Some(generation_id) {
        return CompletionOutcome {
            state: CompletionState::Superseded,
            generation_id: Some(generation_id.to_string()),
            phase2_complete: None,
            verify: 0,
            synthesize: 0,
        };
    }

    let plan = match plan {
        Some(p) => p,
        None => {
            // Plan not yet built by the caller -- nothing further to
            // decide; this mirrors the point in the JS function right
            // before `buildIncrementalPhase2Plan` is invoked.
            return CompletionOutcome {
                state: CompletionState::Superseded,
                generation_id: Some(generation_id.to_string()),
                phase2_complete: None,
                verify: 0,
                synthesize: 0,
            };
        }
    };

    if current_generation_after_plan != Some(generation_id) {
        return CompletionOutcome {
            state: CompletionState::Superseded,
            generation_id: Some(generation_id.to_string()),
            phase2_complete: Some(plan.complete),
            verify: 0,
            synthesize: 0,
        };
    }

    if current_generation_at_commit != Some(generation_id) {
        return CompletionOutcome {
            state: CompletionState::Superseded,
            generation_id: Some(generation_id.to_string()),
            phase2_complete: Some(plan.complete),
            verify: 0,
            synthesize: 0,
        };
    }

    let state = if plan.complete {
        CompletionState::Complete
    } else {
        CompletionState::DocCurrentPhase2Pending
    };

    CompletionOutcome {
        state,
        generation_id: Some(generation_id.to_string()),
        phase2_complete: Some(plan.complete),
        verify: plan.pending.verify,
        synthesize: plan.pending.synthesize,
    }
}
