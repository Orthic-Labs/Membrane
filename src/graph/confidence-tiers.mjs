// Per-edge confidence TIER vocabulary (cortex B3).
//
// Every edge emitted by any graph provider (graph/static-provider.mjs,
// graph/treesitter-provider.mjs, graph/scip-provider.mjs) carries a
// `confidenceTier` alongside its numeric `confidence`. The tier is DERIVED
// from how the edge was actually resolved at the call site that built it —
// never hardcoded per provider, never a subjective score. The numeric
// `confidence` is a pure function of the tier (see tierConfidence below), so
// two edges in the same tier always carry the same number.
//
// Ordered MOST -> LEAST certain:
//   EXACT_RESOLUTION      — deterministic resolution: an import specifier that
//                           matched a real file by path-resolution rules, a
//                           structural AST relationship (file contains its own
//                           top-level declaration; a class defines its own
//                           method), or a compiler/indexer-backed symbol
//                           reference (SCIP). There is no ambiguity to resolve.
//   SAME_FILE_LEXICAL     — a call resolved to a symbol by NAME MATCH, but
//                           bounded to the calling file's own symbol table.
//                           Not a resolved binding (no scope/type analysis),
//                           but the file boundary rules out cross-module
//                           collisions.
//   CROSS_FILE_HEURISTIC  — a call or reference resolved by a heuristic that
//                           crosses file boundaries without compiler-grade
//                           binding info: a name match through an already-
//                           resolved import link, a repo-wide unique-name
//                           fallback (exactly one candidate exists anywhere),
//                           or a string-literal-to-filename match (CONFIGURES).
//   UNRESOLVED            — no target could be resolved (an import specifier
//                           that matched no file; a call name that matched
//                           more than one candidate with no way to prefer one).
//                           The edge is still emitted with target=null —
//                           NEVER silently dropped — and NEVER promoted to a
//                           higher tier just because it was observed.
export const EDGE_CONFIDENCE_TIERS = Object.freeze({
  EXACT_RESOLUTION: "EXACT_RESOLUTION",
  SAME_FILE_LEXICAL: "SAME_FILE_LEXICAL",
  CROSS_FILE_HEURISTIC: "CROSS_FILE_HEURISTIC",
  UNRESOLVED: "UNRESOLVED",
});

// Most -> least certain, index 0 is the highest tier.
export const EDGE_CONFIDENCE_TIER_ORDER = Object.freeze([
  EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION,
  EDGE_CONFIDENCE_TIERS.SAME_FILE_LEXICAL,
  EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC,
  EDGE_CONFIDENCE_TIERS.UNRESOLVED,
]);

export const EDGE_CONFIDENCE_TIER_DESCRIPTIONS = Object.freeze({
  [EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION]:
    "Deterministic resolution — resolved import path, structural AST relationship, or compiler/indexer-backed (SCIP) symbol reference.",
  [EDGE_CONFIDENCE_TIERS.SAME_FILE_LEXICAL]:
    "Name match bounded to the calling file's own symbol table — not a resolved binding, but the file boundary rules out cross-module collisions.",
  [EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC]:
    "Name match that crosses file boundaries via an already-resolved import link, a repo-wide unique-name fallback, or a string-literal-to-filename match.",
  [EDGE_CONFIDENCE_TIERS.UNRESOLVED]:
    "No target could be resolved. Tagged and kept (target=null) — never silently dropped, never promoted.",
});

// Numeric confidence is a pure function of tier. These are NOT subjective
// scores tuned per finding — they exist only so numeric ranking/filtering
// still works for consumers that pre-date the tier field.
const EDGE_CONFIDENCE_TIER_SCORE = Object.freeze({
  [EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION]: 1,
  [EDGE_CONFIDENCE_TIERS.SAME_FILE_LEXICAL]: 0.75,
  [EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC]: 0.5,
  [EDGE_CONFIDENCE_TIERS.UNRESOLVED]: 0,
});

export function tierConfidence(tier) {
  if (!(tier in EDGE_CONFIDENCE_TIER_SCORE)) {
    throw new Error(`unknown edge confidence tier: ${String(tier)}`);
  }
  return EDGE_CONFIDENCE_TIER_SCORE[tier];
}

// True when `tier` is at least as certain as `minTier` (lower index = more certain).
export function isTierAtLeast(tier, minTier) {
  const tierIndex = EDGE_CONFIDENCE_TIER_ORDER.indexOf(tier);
  const minIndex = EDGE_CONFIDENCE_TIER_ORDER.indexOf(minTier);
  if (tierIndex === -1) throw new Error(`unknown edge confidence tier: ${String(tier)}`);
  if (minIndex === -1) throw new Error(`unknown edge confidence tier: ${String(minTier)}`);
  return tierIndex <= minIndex;
}

export function filterEdgesByMinTier(edges, minTier) {
  return edges.filter((e) => e.confidenceTier && isTierAtLeast(e.confidenceTier, minTier));
}
