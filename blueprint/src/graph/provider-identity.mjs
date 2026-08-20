// Provider identities, in one side-effect-free module.
//
// Why this exists: graphStatus must decide whether a persisted graph is still
// readable by the installed extractors. After tree-sitter's promotion to the
// SELECTED provider, a graph carries two layers (AST primary + lexical
// fallback), and a version bump in EITHER must invalidate it — otherwise a
// fixed extractor silently leaves every existing graph stale with no rebuild
// prompt, which is the exact silent-staleness class Cortex exists to catch.
//
// static-provider.mjs cannot import treesitter-provider.mjs for this: that
// module pulls in web-tree-sitter and hash-wasm at load, and static-provider is
// imported by every consumer including ones that never touch the AST layer.
// Hoisting the two identity records here keeps the version check honest at zero
// load cost — this file imports nothing but the tier vocabulary.

import { PRECISION_TIERS } from "./precision-tiers.mjs";

export const STATIC_PROVIDER = {
  id: "cortex-static",
  version: "repo-local-deterministic-v4",
  license: "workspace-owned",
  precisionTier: PRECISION_TIERS.LEXICAL,
};

export const TREESITTER_PROVIDER = {
  id: "cortex-treesitter",
  version: "standalone-v1",
  license: "workspace-owned",
  precisionTier: PRECISION_TIERS.AST,
};

export const providerAliases = Object.freeze({
  "cortex-static": "lexical",
  lexical: "lexical",
  "cortex-treesitter": "treesitter",
  treesitter: "treesitter",
  "cortex-scip": "scip",
  scip: "scip",
});

export function canonicalProviderId(id) {
  const value = String(id ?? "");
  return providerAliases[value] ?? value;
}
