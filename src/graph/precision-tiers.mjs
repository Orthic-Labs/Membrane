// Provider PRECISION tier vocabulary (cortex B4).
//
// A precision tier is a property of a PROVIDER (what class of analysis it can
// possibly achieve), distinct from EDGE_CONFIDENCE_TIERS (graph/confidence-tiers.mjs),
// which is a property of an individual EDGE (how that one edge was resolved).
// A COMPILER-precision provider can still emit an UNRESOLVED-confidence edge
// (e.g. a SCIP index with a dangling reference); the two axes are orthogonal.
//
// Ordered MOST -> LEAST precise:
//   COMPILER — backed by a real compiler/type-checker/indexer (SCIP or
//              equivalent). Symbol resolution reflects actual bindings, not
//              a name-match heuristic. Requires an index to exist; cortex
//              never vendors or invokes a SCIP indexer itself (graph/scip-provider.mjs
//              only READS an index if the repo already produced one).
//   AST      — backed by a real parser (tree-sitter). Structural relationships
//              (containment, definition) are exact; cross-reference resolution
//              (imports/calls) is still name-match heuristic, same ceiling as
//              LEXICAL for those edges.
//   LEXICAL  — regex/text-pattern extraction (graph/static-provider.mjs). No
//              parser, no compiler. The baseline every repo gets for free.
export const PRECISION_TIERS = Object.freeze({
  COMPILER: "COMPILER",
  AST: "AST",
  LEXICAL: "LEXICAL",
});

// Most -> least precise, index 0 is the highest tier.
export const PRECISION_TIER_ORDER = Object.freeze([
  PRECISION_TIERS.COMPILER,
  PRECISION_TIERS.AST,
  PRECISION_TIERS.LEXICAL,
]);

export const PRECISION_TIER_DESCRIPTIONS = Object.freeze({
  [PRECISION_TIERS.COMPILER]:
    "Compiler/indexer-backed (SCIP or equivalent) symbol resolution. Reads an externally produced index; cortex never vendors or runs an indexer itself.",
  [PRECISION_TIERS.AST]:
    "Real-parser (tree-sitter) structural extraction. Containment/definition edges are exact; cross-reference resolution is still name-match heuristic.",
  [PRECISION_TIERS.LEXICAL]:
    "Regex/text-pattern extraction with no parser and no compiler. The deterministic baseline every repo gets.",
});
