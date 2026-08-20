// Compiler/indexer-grade (SCIP) precision tier — blueprint B4.
//
// blueprint NEVER vendors, installs, or invokes a SCIP indexer binary. This
// module only READS an index if the repo already produced one out-of-band
// (scip-typescript, scip-python, rust-analyzer's scip backend, ...) and
// exported it to a portable JSON shape (e.g. `scip print --json`, or any
// `{ documents: [{ relativePath, occurrences: [{ symbol, roles, range }] }] }`
// shape). Absence — no index file, unreadable JSON, or a JSON file missing
// the expected shape — is reported as `state: "unavailable"` with an honest
// `reason`, exactly like augmentGenerationWithTreeSitter's WASM-load failure
// path in static-provider.mjs. The graph then degrades to whatever the AST
// tier (graph/treesitter-provider.mjs) already produced; this module never
// invents edges to compensate for a missing index.
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { EDGE_CONFIDENCE_TIERS, tierConfidence } from "./confidence-tiers.mjs";
import { PRECISION_TIERS } from "./precision-tiers.mjs";

export const PROVIDER = {
  id: "blueprint-scip",
  version: "index-optional-v1",
  license: "workspace-owned",
};

export function findScipIndex(repoRoot, options = {}) {
  const root = resolve(repoRoot);
  const explicit = options.scipIndexPath ?? process.env.BLUEPRINT_SCIP_INDEX ?? null;
  const candidates = explicit
    ? [resolve(root, explicit)]
    : [
        join(root, "index.scip.json"),
        join(root, ".agent", "index.scip.json"),
      ];
  for (const candidate of candidates) {
    if (existsSync(candidate)) return candidate;
  }
  return null;
}

// Reports whether a SCIP index is present and readable, WITHOUT mutating a
// generation. Mirrors the shape of the other providers' probe/capability
// output so a consumer can inspect precision-tier availability up front.
export function probeScip(repoRoot, options = {}) {
  const indexPath = findScipIndex(repoRoot, options);
  if (!indexPath) {
    return {
      provider: PROVIDER,
      precisionTier: PRECISION_TIERS.COMPILER,
      state: "unavailable",
      reason: "no SCIP index found (set BLUEPRINT_SCIP_INDEX, or place index.scip.json / .agent/index.scip.json at repo root)",
      degradesTo: PRECISION_TIERS.AST,
    };
  }
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(indexPath, "utf8"));
  } catch (err) {
    return {
      provider: PROVIDER,
      precisionTier: PRECISION_TIERS.COMPILER,
      state: "unavailable",
      reason: `SCIP index at ${indexPath} could not be parsed as JSON: ${String(err?.message ?? err)}`,
      degradesTo: PRECISION_TIERS.AST,
    };
  }
  if (!Array.isArray(parsed?.documents)) {
    return {
      provider: PROVIDER,
      precisionTier: PRECISION_TIERS.COMPILER,
      state: "unavailable",
      reason: `SCIP index at ${indexPath} has no "documents" array — not a recognized portable-SCIP-JSON shape`,
      degradesTo: PRECISION_TIERS.AST,
    };
  }
  return {
    provider: PROVIDER,
    precisionTier: PRECISION_TIERS.COMPILER,
    state: "ok",
    indexPath,
    documentCount: parsed.documents.length,
  };
}

// Additive augmentation, same contract shape as
// static-provider.mjs::augmentGenerationWithTreeSitter: async at the build
// boundary only, degrades honestly on any absence/failure, and only ever adds
// REFERENCES edges that are literally present in the index's occurrence list
// (roles including "reference") joined against nodes already in the
// generation — it never fabricates a node or a reference that isn't in the
// index.
export async function augmentGenerationWithScip(generation, repoRoot, options = {}) {
  const probe = probeScip(repoRoot, options);
  if (probe.state !== "ok") {
    generation.augmentation = { ...(generation.augmentation ?? {}), scip: probe };
    return probe;
  }

  const parsed = JSON.parse(readFileSync(probe.indexPath, "utf8"));
  const nodesById = new Map(generation.nodes.map((node) => [node.id, node]));
  const nodesByPathAndName = new Map(
    generation.nodes
      .filter((node) => node.kind === "symbol")
      .map((node) => [`${node.path}::${node.qualifiedName}`, node]),
  );
  let edgesAdded = 0;
  for (const doc of parsed.documents) {
    const path = String(doc.relativePath ?? doc.path ?? "");
    if (!path) continue;
    const sourceNode = nodesById.get(`file:${path}`);
    if (!sourceNode) continue;
    for (const occ of doc.occurrences ?? []) {
      if (!occ.symbol || !Array.isArray(occ.roles) || !occ.roles.includes("reference")) continue;
      const targetNode = nodesByPathAndName.get(`${path}::${occ.symbol}`);
      if (!targetNode) continue;
      const range = Array.isArray(occ.range) ? occ.range : [1, 0, 1, 0];
      generation.edges.push({
        id: `edge:REFERENCES:${sourceNode.id}->${targetNode.id}:scip:${edgesAdded}`,
        kind: "REFERENCES",
        source: sourceNode.id,
        target: targetNode.id,
        confidenceTier: EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION,
        confidence: tierConfidence(EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION),
        provider: PROVIDER.id,
        evidence: [{
          path,
          startLine: (range[0] ?? 0) + 1,
          endLine: (range[2] ?? range[0] ?? 0) + 1,
          contentHash: sourceNode.evidence?.[0]?.contentHash ?? null,
        }],
      });
      edgesAdded += 1;
    }
  }
  const result = { ...probe, applied: true, edgesAdded };
  generation.augmentation = { ...(generation.augmentation ?? {}), scip: result };
  return result;
}
