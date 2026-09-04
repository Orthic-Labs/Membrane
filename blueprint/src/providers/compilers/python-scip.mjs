// Python SCIP adapter. Reads only out-of-band portable JSON under repo-read;
// it never installs, invokes, or guesses. Transport parsing and occurrence-role
// semantics are shared with every first-party SCIP lane via scip-normalize.mjs.

import { basename, isAbsolute, resolve } from "node:path";

import { defineProvider } from "../index.mjs";
import { EDGE_CONFIDENCE_TIERS, tierConfidence } from "../../graph/confidence-tiers.mjs";
import { PRECISION_TIERS } from "../../graph/precision-tiers.mjs";
import { findScipIndex } from "../../graph/scip-provider.mjs";
import { assertRegisteredRelationshipKinds } from "../../graph/relationship-kinds.mjs";
import { readNormalizedScipIndex, scipOccurrenceEvidence } from "./scip-normalize.mjs";

const PROVIDER_ID = "scip-python";
const ADAPTER_VERSION = "normalized-portable-index-v2";
const SUPPORTED_SCIP_PYTHON_VERSION = "0.6.6";
const DEGRADES_TO = PRECISION_TIERS.AST;

// File-local scip-python symbols (`local 0`, `local 1`, ...) and parameter
// descriptors (`...summarize().(item)`) carry no cross-document meaning.
function isLocalSymbol(symbol) {
  return /^local(?:\s|$)/.test(symbol);
}

function isParameterSymbol(symbol) {
  return /\([A-Za-z_]\w*\)$/.test(symbol);
}

function symbolTail(symbol) {
  return String(symbol).split(/\s+/).slice(4).join(" ");
}

function descriptorNames(symbol) {
  const pattern = /([A-Za-z_]\w*)(?=#|\(\)\.|\(|\.|$)/g;
  return [...symbolTail(symbol).matchAll(pattern)].map((match) => match[1]);
}

function leafName(symbol) {
  const names = descriptorNames(symbol);
  return names.length ? names.at(-1) : symbol;
}

function symbolLabels(symbol) {
  const tail = symbolTail(symbol);
  if (tail.endsWith("().")) return tail.includes("#") ? ["Method"] : ["Function"];
  if (tail.endsWith("#")) return ["Class"];
  return ["Symbol"];
}

function occurrenceEvidence(occurrence) {
  return [scipOccurrenceEvidence(occurrence)];
}

function definitionNode(occurrence, info = null) {
  const symbol = occurrence.symbol;
  return {
    id: `symbol:${occurrence.documentPath}::${symbol}`,
    kind: "symbol",
    labels: symbolLabels(symbol),
    name: info?.displayName ?? leafName(symbol),
    qualifiedName: descriptorNames(symbol).join(".") || symbol,
    symbol,
    path: occurrence.documentPath,
    precisionTier: PRECISION_TIERS.COMPILER,
    provider: PROVIDER_ID,
    confidence: 1,
    ...(info?.documentation?.length ? { documentation: [...info.documentation] } : {}),
    ...(info?.kind !== null && info?.kind !== undefined ? { symbolKind: info.kind } : {}),
    evidence: occurrenceEvidence(occurrence),
  };
}

function referenceEdge(kind, sourceId, target, evidence, reason, serial) {
  const resolved = target !== null && typeof target.id === "string";
  const targetId = resolved ? target.id : null;
  const tier = resolved ? EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION : EDGE_CONFIDENCE_TIERS.UNRESOLVED;
  return {
    id: `edge:${kind}:${sourceId}->${targetId ?? `unresolved:${evidence[0].symbol}`}:scip:${serial}`,
    kind,
    source: sourceId,
    target: targetId,
    confidenceTier: tier,
    confidence: tierConfidence(tier),
    provider: PROVIDER_ID,
    precisionTier: PRECISION_TIERS.COMPILER,
    resolved,
    reason: reason ?? null,
    evidence,
  };
}

function includedOccurrence(occurrence) {
  return !isLocalSymbol(occurrence.symbol) && !isParameterSymbol(occurrence.symbol);
}

function buildFromIndex(index) {
  const nodes = [];
  const edges = [];
  const definitionsBySymbol = new Map();
  let definitionCount = 0;
  let referenceCount = 0;

  for (const doc of index.documents) {
    const path = doc.path;
    nodes.push({
      id: `file:${path}`,
      kind: "file",
      labels: ["File"],
      name: basename(path),
      qualifiedName: path,
      path,
      precisionTier: PRECISION_TIERS.COMPILER,
      provider: PROVIDER_ID,
      confidence: 1,
      evidence: [{ path, startLine: 1, endLine: 1 }],
    });
    for (const occurrence of doc.occurrences) {
      if (!includedOccurrence(occurrence)) continue;
      if (occurrence.roles.has("definition")) {
        definitionCount += 1;
        if (!definitionsBySymbol.has(occurrence.symbol)) {
          const node = definitionNode(occurrence, index.symbolInformationBySymbol.get(occurrence.symbol) ?? null);
          definitionsBySymbol.set(occurrence.symbol, node);
          nodes.push(node);
        }
      }
    }
  }

  let serial = 0;
  for (const doc of index.documents) {
    const sourceId = `file:${doc.path}`;
    for (const occurrence of doc.occurrences) {
      if (!includedOccurrence(occurrence) || !occurrence.roles.has("reference")) continue;
      referenceCount += 1;
      const target = definitionsBySymbol.get(occurrence.symbol) ?? null;
      const evidence = occurrenceEvidence(occurrence);
      edges.push(referenceEdge(
        "REFERENCES",
        sourceId,
        target,
        evidence,
        target ? null : `no definition for symbol "${occurrence.symbol}" in the index; no name-match fallback`,
        serial,
      ));
      serial += 1;
      if (target?.labels?.includes("Class")) {
        edges.push(referenceEdge("TYPED", sourceId, target, evidence, null, serial));
        serial += 1;
      }
    }
  }

  return { nodes, edges, definitionCount, referenceCount };
}

function degradationReport(probe) {
  return {
    kind: probe.state,
    code: probe.code,
    reason: probe.reason,
    degradesTo: probe.degradesTo,
    provider: probe.provider,
    precisionTier: probe.precisionTier,
    indexPath: probe.indexPath ?? null,
    ...(probe.skippedDocuments !== undefined ? { skippedDocuments: probe.skippedDocuments } : {}),
    ...(probe.skippedOccurrences !== undefined ? { skippedOccurrences: probe.skippedOccurrences } : {}),
  };
}

function probeScipIndex(context = {}) {
  const explicit = context.scipIndexPath ?? process.env.BLUEPRINT_SCIP_INDEX ?? null;
  const indexPath = explicit && isAbsolute(explicit)
    ? explicit
    : findScipIndex(context.repoRoot ?? process.cwd(), { scipIndexPath: explicit });
  if (!indexPath) {
    return {
      state: "unavailable",
      code: "scip_index_absent",
      provider: PROVIDER_ID,
      precisionTier: PRECISION_TIERS.COMPILER,
      degradesTo: DEGRADES_TO,
      reason: "no SCIP index found (set BLUEPRINT_SCIP_INDEX or pass scipIndexPath, or place index.scip.json / .agent/index.scip.json at the repo root)",
    };
  }

  let index;
  try {
    index = readNormalizedScipIndex(indexPath);
  } catch (error) {
    return {
      state: "unavailable",
      code: error?.code ?? "scip_index_unreadable",
      provider: PROVIDER_ID,
      precisionTier: PRECISION_TIERS.COMPILER,
      degradesTo: DEGRADES_TO,
      indexPath,
      reason: String(error?.message ?? error),
    };
  }

  const indexVersion = String(index.metadata?.version ?? "");
  if (indexVersion && indexVersion !== SUPPORTED_SCIP_PYTHON_VERSION) {
    return {
      state: "unavailable",
      code: "scip_index_version_incompatible",
      provider: PROVIDER_ID,
      precisionTier: PRECISION_TIERS.COMPILER,
      degradesTo: DEGRADES_TO,
      indexPath,
      indexVersion,
      reason: `SCIP index at ${indexPath} uses scip-python ${indexVersion}; supported version is ${SUPPORTED_SCIP_PYTHON_VERSION}`,
    };
  }

  let definitionCount = 0;
  let referenceCount = 0;
  for (const occurrence of index.occurrences) {
    if (!includedOccurrence(occurrence)) continue;
    if (occurrence.roles.has("definition")) definitionCount += 1;
    if (occurrence.roles.has("reference")) referenceCount += 1;
  }
  const partialReasons = [];
  if (index.skippedDocuments > 0) partialReasons.push(`${index.skippedDocuments} document(s) missing relativePath`);
  if (index.skippedOccurrences > 0) partialReasons.push(`${index.skippedOccurrences} structurally incomplete occurrence(s)`);
  if (definitionCount === 0) partialReasons.push("index declares no definitions");
  if (partialReasons.length > 0) {
    return {
      state: "partial",
      code: "scip_index_partial",
      provider: PROVIDER_ID,
      precisionTier: PRECISION_TIERS.COMPILER,
      degradesTo: DEGRADES_TO,
      indexPath,
      indexVersion,
      reason: `SCIP index at ${indexPath} is partial: ${partialReasons.join("; ")}. Affected entries are skipped; no edges are fabricated for them.`,
      skippedDocuments: index.skippedDocuments,
      skippedOccurrences: index.skippedOccurrences,
      definitionCount,
      referenceCount,
    };
  }
  return {
    state: "ok",
    provider: PROVIDER_ID,
    precisionTier: PRECISION_TIERS.COMPILER,
    indexPath,
    indexVersion,
    documentCount: index.documents.length,
    definitionCount,
    referenceCount,
  };
}

export const pythonScipProvider = defineProvider({
  id: PROVIDER_ID,
  version: ADAPTER_VERSION,
  kind: "compiler",
  protocolRange: ">=1 <2",
  capabilities: ["definitions", "references", "types"],
  permissions: { filesystem: "repo-read", network: "none", process: "none" },
  probe(context = {}) {
    return probeScipIndex(context);
  },
  collect(context = {}) {
    const probe = probeScipIndex(context);
    if (probe.state === "unavailable") {
      return { nodes: [], edges: [], reports: [degradationReport(probe)], index: probe };
    }
    let index;
    try {
      index = readNormalizedScipIndex(probe.indexPath);
    } catch (error) {
      const unavailable = {
        ...probe,
        state: "unavailable",
        code: error?.code ?? "scip_index_unreadable",
        degradesTo: DEGRADES_TO,
        reason: String(error?.message ?? error),
      };
      return { nodes: [], edges: [], reports: [degradationReport(unavailable)], index: unavailable };
    }
    const built = buildFromIndex(index);
    assertRegisteredRelationshipKinds(built.edges, PROVIDER_ID);
    const reports = probe.state === "partial" ? [degradationReport(probe)] : [];
    return {
      nodes: built.nodes,
      edges: built.edges,
      reports,
      index: {
        provider: PROVIDER_ID,
        indexer: index.metadata?.indexer ?? "scip-python",
        version: String(index.metadata?.version ?? ""),
        path: probe.indexPath,
        documentCount: index.documents.length,
        definitionCount: built.definitionCount,
        referenceCount: built.referenceCount,
        state: probe.state,
      },
    };
  },
});

export { PROVIDER_ID, ADAPTER_VERSION };
