//! Native provider registry — port of the legacy `blueprint/src/providers/`
//! plugin-loader/build-pipeline order (see `blueprint/src/providers/build.mjs`
//! `augmentGenerationWithFirstPartyProviders`, and `plugin-loader.mjs` for the
//! untrusted-manifest admission mechanism).
//!
//! Scope of this module (lane P4): the registry scaffold, the built-in
//! provider *order* contract, and the two narrow legacy pieces that do not
//! belong to any single language/framework provider — ranking-provider
//! type vocabulary (`ranking`) and source-disposition auditing
//! (`source_disposition`). Concrete provider implementations (bridges/SCIP/
//! frameworks/IaC) are wired in by their owning lanes, which append their
//! module declarations and registry entries below the marker comments —
//! this file is intentionally append-friendly so concurrent lanes do not
//! collide on the same hunks.

pub mod ranking;
pub mod source_disposition;
pub mod scip;
pub mod frameworks;
pub mod iac_terraform;
pub mod bridges;

use crate::graph::FileRecord;
use crate::model::{GraphEdge, GraphNode};
use std::collections::BTreeMap;

/// Shared read-only context handed to every provider run. Mirrors the
/// `(generation, files, root, options)` argument shape threaded through
/// `build.mjs`'s `augmentGenerationWithFirstPartyProviders`.
pub struct ProviderContext<'a> {
    pub repo_root: &'a std::path::Path,
    pub files: &'a [FileRecord],
    /// path -> file, for O(log n) lookup during resolution (mirrors the
    /// `file_map` already threaded through `graph.rs`'s lexical pass).
    pub file_map: &'a BTreeMap<String, &'a FileRecord>,
}

/// What a single provider run contributes to a generation. Nodes/edges are
/// merged into the generation by the caller using the existing
/// confidence-tier/ranking rules in `confidence_tiers.rs` — a provider never
/// merges itself.
#[derive(Debug, Clone, Default)]
pub struct ProviderOutput {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl ProviderOutput {
    pub fn merge(&mut self, mut other: ProviderOutput) {
        self.nodes.append(&mut other.nodes);
        self.edges.append(&mut other.edges);
    }
}

/// Legacy first-party provider identifiers, in the exact order
/// `build.mjs`'s `augmentGenerationWithFirstPartyProviders` runs them:
/// modules -> frameworks -> sql/terraform -> scip -> bridges, with
/// structural-intelligence / framework-intelligence / portable-identity /
/// conventions run as post-passes over the same generation. `ingestion`
/// (source-disposition) and `plugins` (plugin-loader admission) are
/// accounting/admission side-summaries, not node/edge providers, and are
/// listed first because they run before any provider in the legacy code.
pub const PROVIDER_ORDER: &[&str] = &[
    "ingestion",           // source-disposition.mjs — accounting only
    "plugins",             // plugin-loader.mjs — admission only, never executes
    "blueprint-modules",   // modules/javascript.mjs + python-resolver.mjs
    "blueprint-frameworks",
    "blueprint-sql",
    "blueprint-terraform",
    "scip-python",
    "blueprint-bridges",
    "structural-intelligence",
    "framework-intelligence",
    "portable-identity",
    "conventions",
];

/// A registered provider: a stable id (must appear in [`PROVIDER_ORDER`])
/// paired with its run function.
pub struct ProviderDescriptor {
    pub id: &'static str,
    pub run: fn(&ProviderContext<'_>) -> ProviderOutput,
}

/// Build the provider registry in legacy order. Each owning lane appends
/// exactly one `registry.push(ProviderDescriptor { .. });` call in its own
/// marked section below — never reorders or edits another lane's entry.
/// A descriptor whose `id` is not in [`PROVIDER_ORDER`] is a lane bug; debug
/// builds assert this so the mistake surfaces at test time rather than
/// silently mis-ordering the build pass.
pub fn registry() -> Vec<ProviderDescriptor> {
    let mut registry: Vec<ProviderDescriptor> = Vec::new();

    registry.push(ProviderDescriptor { id: "blueprint-frameworks", run: frameworks::run });
    registry.push(ProviderDescriptor { id: "blueprint-terraform", run: iac_terraform::run });
    registry.push(ProviderDescriptor { id: "scip-python", run: scip::run });
    registry.push(ProviderDescriptor { id: "blueprint-bridges", run: bridges::run });

    debug_assert!(
        registry.iter().all(|d| PROVIDER_ORDER.contains(&d.id)),
        "provider registered with an id outside PROVIDER_ORDER"
    );
    registry.sort_by_ordinal()
}

trait SortByOrdinal {
    fn sort_by_ordinal(self) -> Self;
}

impl SortByOrdinal for Vec<ProviderDescriptor> {
    fn sort_by_ordinal(mut self) -> Self {
        self.sort_by_key(|d| PROVIDER_ORDER.iter().position(|id| *id == d.id).unwrap_or(usize::MAX));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_empty_until_lanes_append_and_stays_sorted() {
        let reg = registry();
        let mut prev = usize::MIN;
        for d in &reg {
            let ord = PROVIDER_ORDER.iter().position(|id| *id == d.id).unwrap();
            assert!(ord >= prev, "provider {} out of legacy order", d.id);
            prev = ord;
        }
    }

    #[test]
    fn provider_order_matches_build_mjs_sequence() {
        assert_eq!(
            PROVIDER_ORDER,
            &[
                "ingestion",
                "plugins",
                "blueprint-modules",
                "blueprint-frameworks",
                "blueprint-sql",
                "blueprint-terraform",
                "scip-python",
                "blueprint-bridges",
                "structural-intelligence",
                "framework-intelligence",
                "portable-identity",
                "conventions",
            ]
        );
    }
}
