// Compiler/indexer-grade (SCIP) precision tier — Blueprint B4.
//
// Blueprint never vendors, installs, or invokes a SCIP indexer binary here.
// This module consumes an index produced out-of-band and routes ALL transport
// parsing through the canonical normalizer shared with first-party compiler
// adapters. Policy (edge kinds/confidence) remains here; SCIP shape/roles and
// exact symbol identity do not get reimplemented per consumer.

import { existsSync } from "node:fs";
import { join, resolve } from "node:path";

import { EDGE_CONFIDENCE_TIERS, tierConfidence } from "./confidence-tiers.mjs";
import { PRECISION_TIERS } from "./precision-tiers.mjs";
import { assertRegisteredRelationshipKinds } from "./relationship-kinds.mjs";
import { readNormalizedScipIndex, scipOccurrenceEvidence } from "../providers/compilers/scip-normalize.mjs";

export const PROVIDER = Object.freeze({
  id: "blueprint-scip",
  version: "normalized-index-v2",
  license: "workspace-owned",
});

export function findScipIndex(repoRoot, options = {}) {
  const root = resolve(repoRoot);
  const explicit = options.scipIndexPath ?? process.env.BLUEPRINT_SCIP_INDEX ?? null;
  const candidates = explicit
    ? [resolve(root, explicit)]
    : [join(root, "index.scip.json"), join(root, ".agent", "index.scip.json")];
  for (const candidate of candidates) if (existsSync(candidate)) return candidate;
  return null;
}

function unavailable(reason, code = "scip_index_unavailable", indexPath = null) {
  return {
    provider: PROVIDER,
    precisionTier: PRECISION_TIERS.COMPILER,
    state: "unavailable",
    code,
    reason,
    indexPath,
    degradesTo: PRECISION_TIERS.AST,
  };
}

export function probeScip(repoRoot, options = {}) {
  const indexPath = findScipIndex(repoRoot, options);
  if (!indexPath) {
    return unavailable(
      "no SCIP index found (set BLUEPRINT_SCIP_INDEX, or place index.scip.json / .agent/index.scip.json at repo root)",
      "scip_index_absent",
    );
  }
  let index;
  try {
    index = readNormalizedScipIndex(indexPath);
  } catch (error) {
    return unavailable(String(error?.message ?? error), error?.code ?? "scip_index_unreadable", indexPath);
  }
  const definitionCount = [...index.occurrences].filter((occurrence) => occurrence.roles.has("definition")).length;
  const referenceCount = [...index.occurrences].filter((occurrence) => occurrence.roles.has("reference")).length;
  return {
    provider: PROVIDER,
    precisionTier: PRECISION_TIERS.COMPILER,
    state: "ok",
    indexPath,
    documentCount: index.documents.length,
    definitionCount,
    referenceCount,
    skippedDocuments: index.skippedDocuments,
    skippedOccurrences: index.skippedOccurrences,
    partial: index.skippedDocuments > 0 || index.skippedOccurrences > 0,
  };
}

function exactNodeId(occurrence) {
  return `symbol:${occurrence.documentPath}::${occurrence.symbol}`;
}

function symbolInfo(index, symbol) {
  return index.symbolInformationBySymbol.get(symbol) ?? null;
}

function definitionNode(index, occurrence, fileNode) {
  const info = symbolInfo(index, occurrence.symbol);
  return {
    id: exactNodeId(occurrence),
    kind: "symbol",
    labels: ["Symbol", "CompilerSymbol"],
    name: info?.displayName ?? occurrence.symbol,
    qualifiedName: occurrence.symbol,
    symbol: occurrence.symbol,
    path: occurrence.documentPath,
    precisionTier: PRECISION_TIERS.COMPILER,
    provider: PROVIDER.id,
    confidence: 1,
    ...(info?.documentation?.length ? { documentation: [...info.documentation] } : {}),
    ...(info?.kind !== null && info?.kind !== undefined ? { symbolKind: info.kind } : {}),
    evidence: [scipOccurrenceEvidence(occurrence, { contentHash: fileNode?.evidence?.[0]?.contentHash ?? null })],
  };
}

function referenceEdge(sourceNode, targetNode, occurrence, serial) {
  const resolved = Boolean(targetNode);
  const confidenceTier = resolved
    ? EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION
    : EDGE_CONFIDENCE_TIERS.UNRESOLVED;
  return {
    id: `edge:REFERENCES:${sourceNode.id}->${targetNode?.id ?? `unresolved:${occurrence.symbol}`}:scip:${serial}`,
    kind: "REFERENCES",
    source: sourceNode.id,
    target: targetNode?.id ?? null,
    confidenceTier,
    confidence: tierConfidence(confidenceTier),
    provider: PROVIDER.id,
    precisionTier: PRECISION_TIERS.COMPILER,
    resolved,
    reason: resolved ? null : `no definition for SCIP symbol \"${occurrence.symbol}\" in the normalized index`,
    evidence: [scipOccurrenceEvidence(occurrence, { contentHash: sourceNode.evidence?.[0]?.contentHash ?? null })],
  };
}

// Add exact compiler-semantic definitions/references from the normalized index.
// No path/name fallback exists: if an occurrence's exact SCIP symbol has no
// definition in the index, it remains an explicit unresolved edge.
export async function augmentGenerationWithScip(generation, repoRoot, options = {}) {
  const probe = probeScip(repoRoot, options);
  if (probe.state !== "ok") {
    generation.augmentation = { ...(generation.augmentation ?? {}), scip: probe };
    return probe;
  }

  let index;
  try {
    index = readNormalizedScipIndex(probe.indexPath);
  } catch (error) {
    const failure = unavailable(String(error?.message ?? error), error?.code ?? "scip_index_unreadable", probe.indexPath);
    generation.augmentation = { ...(generation.augmentation ?? {}), scip: failure };
    return failure;
  }

  const nodesById = new Map(generation.nodes.map((node) => [node.id, node]));
  const fileNodes = new Map(generation.nodes.filter((node) => node.kind === "file").map((node) => [node.path, node]));
  let nodesAdded = 0;
  for (const occurrence of index.definitionsBySymbol.values()) {
    const fileNode = fileNodes.get(occurrence.documentPath);
    if (!fileNode) continue;
    const id = exactNodeId(occurrence);
    if (nodesById.has(id)) continue;
    const node = definitionNode(index, occurrence, fileNode);
    generation.nodes.push(node);
    nodesById.set(id, node);
    nodesAdded += 1;
  }

  const edges = [];
  let serial = 0;
  for (const occurrence of index.occurrences) {
    if (!occurrence.roles.has("reference")) continue;
    const sourceNode = fileNodes.get(occurrence.documentPath);
    if (!sourceNode) continue;
    const definition = index.definitionsBySymbol.get(occurrence.symbol) ?? null;
    const targetNode = definition ? nodesById.get(exactNodeId(definition)) ?? null : null;
    edges.push(referenceEdge(sourceNode, targetNode, occurrence, serial));
    serial += 1;
  }
  assertRegisteredRelationshipKinds(edges, PROVIDER.id);
  generation.edges.push(...edges);

  if (generation.manifest?.counts) {
    generation.manifest.counts = {
      ...generation.manifest.counts,
      nodes: generation.nodes.length,
      edges: generation.edges.length,
    };
  }
  const result = {
    ...probe,
    applied: true,
    nodesAdded,
    edgesAdded: edges.length,
    unresolvedReferences: edges.filter((edge) => !edge.resolved).length,
  };
  generation.augmentation = { ...(generation.augmentation ?? {}), scip: result };
  return result;
}
