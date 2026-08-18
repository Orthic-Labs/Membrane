// Python SCIP adapter. Reads only out-of-band portable JSON under repo-read;
// it never installs, invokes, or guesses. Exact symbol identity produces
// COMPILER definitions/references/types. Absent, unreadable, unsupported
// version/shape, or partial indexes degrade explicitly to AST without throws.

import { existsSync, readFileSync } from "node:fs";
import { basename, isAbsolute, resolve } from "node:path";

import { defineProvider } from "../index.mjs";
import { EDGE_CONFIDENCE_TIERS, tierConfidence } from "../../graph/confidence-tiers.mjs";
import { PRECISION_TIERS } from "../../graph/precision-tiers.mjs";
import { findScipIndex } from "../../graph/scip-provider.mjs";

const PROVIDER_ID = "scip-python";
const ADAPTER_VERSION = "portable-index-v1";
const SUPPORTED_SCIP_PYTHON_VERSION = "0.6.6";
const DEGRADES_TO = PRECISION_TIERS.AST;

// Normalizes SCIP roles from either portable form: an array of role-name
// strings (the repo's documented portable shape) or the standard numeric
// bitmask (1=definition, 2=reference, 4=read, 8=write).
function roleNames(roles) {
  if (Array.isArray(roles)) return new Set(roles.map((role) => String(role)));
  const value = Number(roles);
  if (Number.isNaN(value)) return new Set();
  const names = new Set();
  if ((value & 1) === 1) names.add("definition");
  if ((value & 2) === 2) names.add("reference");
  if ((value & 4) === 4) names.add("read");
  if ((value & 8) === 8) names.add("write");
  return names;
}

// A usable occurrence carries a non-empty symbol, a roles field that names at
// least one of definition/reference, and a range. Anything else is
// structurally incomplete: skipped and reported (partial), never guessed at.
function isUsableOccurrence(occ) {
  if (occ === null || typeof occ !== "object") return false;
  if (typeof occ.symbol !== "string" || occ.symbol.length === 0) return false;
  if (!Array.isArray(occ.range) || occ.range.length < 2) return false;
  const roles = roleNames(occ.roles);
  if (roles.size === 0) return false;
  return roles.has("definition") || roles.has("reference");
}

// File-local scip-python symbols (`local 0`, `local 1`, ...) and parameter
// descriptors (`...summarize().(item)`) carry no cross-document meaning.
function isLocalSymbol(symbol) {
  return /^local(?:\s|$)/.test(symbol);
}

function isParameterSymbol(symbol) {
  return /\([A-Za-z_]\w*\)$/.test(symbol);
}

// Drop the scheme/manager/package/version prefix; what remains is the module
// path plus the descriptor chain, e.g. `pkg/models Item#total().`.
function symbolTail(symbol) {
  return String(symbol).split(/\s+/).slice(4).join(" ");
}

// Display-name extraction from literal descriptor text — a presentation
// layer only, never a resolution heuristic (resolution is symbol identity).
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

function occurrenceEvidence(path, occ) {
  const startLine = Number(occ.range[0] ?? 0) + 1;
  const endLine = Number(occ.range[2] ?? occ.range[0] ?? 0) + 1;
  return [{ path, startLine, endLine, symbol: occ.symbol }];
}

function definitionNode(path, occ) {
  const symbol = occ.symbol;
  return {
    id: `symbol:${path}::${symbol}`,
    kind: "symbol",
    labels: symbolLabels(symbol),
    name: leafName(symbol),
    qualifiedName: descriptorNames(symbol).join(".") || symbol,
    symbol,
    path,
    precisionTier: PRECISION_TIERS.COMPILER,
    provider: PROVIDER_ID,
    confidence: 1,
    evidence: occurrenceEvidence(path, occ),
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

// Single pass over the whole index builds a symbol -> definition-node map, so
// a reference in one document resolves to a definition in ANOTHER document by
// exact symbol identity — the cross-document channel. No name matching.
function buildFromIndex(parsed) {
  const nodes = [];
  const edges = [];
  const definitionsBySymbol = new Map();
  let skippedOccurrences = 0;
  let definitionCount = 0;
  let referenceCount = 0;

  for (const doc of parsed.documents) {
    const path = String(doc.relativePath ?? doc.path ?? "");
    if (!path) continue;
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
    for (const occ of doc.occurrences ?? []) {
      if (!isUsableOccurrence(occ)) {
        skippedOccurrences += 1;
        continue;
      }
      if (isLocalSymbol(occ.symbol) || isParameterSymbol(occ.symbol)) continue;
      const roles = roleNames(occ.roles);
      if (roles.has("definition")) {
        definitionCount += 1;
        if (!definitionsBySymbol.has(occ.symbol)) {
          const node = definitionNode(path, occ);
          definitionsBySymbol.set(occ.symbol, node);
          nodes.push(node);
        }
      }
    }
  }

  let serial = 0;
  for (const doc of parsed.documents) {
    const path = String(doc.relativePath ?? doc.path ?? "");
    if (!path) continue;
    const sourceId = `file:${path}`;
    for (const occ of doc.occurrences ?? []) {
      if (!isUsableOccurrence(occ)) continue;
      if (isLocalSymbol(occ.symbol) || isParameterSymbol(occ.symbol)) continue;
      const roles = roleNames(occ.roles);
      if (!roles.has("reference")) continue;
      referenceCount += 1;
      const target = definitionsBySymbol.get(occ.symbol) ?? null;
      const evidence = occurrenceEvidence(path, occ);
      edges.push(referenceEdge(
        "REFERENCES",
        sourceId,
        target,
        evidence,
        target ? null : `no definition for symbol "${occ.symbol}" in the index; no name-match fallback`,
        serial,
      ));
      serial += 1;
      // A reference whose target symbol is a class IS a type usage — exact
      // type resolution at COMPILER precision, same symbol identity rule.
      if (target?.labels?.includes("Class")) {
      edges.push(referenceEdge("TYPES", sourceId, target, evidence, null, serial));
        serial += 1;
      }
    }
  }

  return { nodes, edges, skippedOccurrences, definitionCount, referenceCount };
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
  const explicit = context.scipIndexPath ?? process.env.CORTEX_SCIP_INDEX ?? null;
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
      reason: "no SCIP index found (set CORTEX_SCIP_INDEX or pass scipIndexPath, or place index.scip.json / .agent/index.scip.json at the repo root)",
    };
  }
  let parsed;
  try {
    parsed = JSON.parse(readFileSync(indexPath, "utf8"));
  } catch (err) {
    return {
      state: "unavailable",
      code: "scip_index_unreadable",
      provider: PROVIDER_ID,
      precisionTier: PRECISION_TIERS.COMPILER,
      degradesTo: DEGRADES_TO,
      indexPath,
      reason: `SCIP index at ${indexPath} could not be read/parsed as JSON: ${String(err?.message ?? err)}`,
    };
  }
  if (!Array.isArray(parsed?.documents)) {
    return {
      state: "unavailable",
      code: "scip_index_incompatible",
      provider: PROVIDER_ID,
      precisionTier: PRECISION_TIERS.COMPILER,
      degradesTo: DEGRADES_TO,
      indexPath,
      reason: `SCIP index at ${indexPath} has no "documents" array — not a recognized portable-SCIP-JSON shape`,
    };
  }
  const indexVersion = String(parsed.metadata?.version ?? "");
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
  let skippedDocuments = 0;
  let skippedOccurrences = 0;
  let definitionCount = 0;
  let referenceCount = 0;
  for (const doc of parsed.documents) {
    if (doc === null || typeof doc !== "object") {
      skippedDocuments += 1;
      continue;
    }
    const path = String(doc.relativePath ?? doc.path ?? "");
    if (!path) {
      skippedDocuments += 1;
      continue;
    }
    for (const occ of doc.occurrences ?? []) {
      if (!isUsableOccurrence(occ)) {
        skippedOccurrences += 1;
        continue;
      }
      if (isLocalSymbol(occ.symbol) || isParameterSymbol(occ.symbol)) continue;
      const roles = roleNames(occ.roles);
      if (roles.has("definition")) definitionCount += 1;
      if (roles.has("reference")) referenceCount += 1;
    }
  }
  const partialReasons = [];
  if (skippedDocuments > 0) partialReasons.push(`${skippedDocuments} document(s) missing relativePath`);
  if (skippedOccurrences > 0) partialReasons.push(`${skippedOccurrences} structurally incomplete occurrence(s)`);
  if (definitionCount === 0) partialReasons.push("index declares no definitions");
  if (partialReasons.length > 0) {
    return {
      state: "partial",
      code: "scip_index_partial",
      provider: PROVIDER_ID,
      precisionTier: PRECISION_TIERS.COMPILER,
      degradesTo: DEGRADES_TO,
      indexPath,
      indexVersion: String(parsed.metadata?.version ?? ""),
      reason: `SCIP index at ${indexPath} is partial: ${partialReasons.join("; ")}. Affected entries are skipped; no edges are fabricated for them.`,
      skippedDocuments,
      skippedOccurrences,
      definitionCount,
      referenceCount,
    };
  }
  return {
    state: "ok",
    provider: PROVIDER_ID,
    precisionTier: PRECISION_TIERS.COMPILER,
    indexPath,
    indexVersion: String(parsed.metadata?.version ?? ""),
    documentCount: parsed.documents.length,
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
  async probe(context = {}) {
    return probeScipIndex(context);
  },
  async collect(context = {}) {
    const probe = probeScipIndex(context);
    if (probe.state === "unavailable") {
      return { nodes: [], edges: [], reports: [degradationReport(probe)], index: probe };
    }
    let parsed;
    try {
      parsed = JSON.parse(readFileSync(probe.indexPath, "utf8"));
    } catch (err) {
      // The index changed between probe and read — same typed degradation.
      const unavailable = {
        ...probe,
        state: "unavailable",
        code: "scip_index_unreadable",
        degradesTo: DEGRADES_TO,
        reason: `SCIP index at ${probe.indexPath} could not be read/parsed as JSON: ${String(err?.message ?? err)}`,
      };
      return { nodes: [], edges: [], reports: [degradationReport(unavailable)], index: unavailable };
    }
    const built = buildFromIndex(parsed);
    const reports = probe.state === "partial" ? [degradationReport(probe)] : [];
    return {
      nodes: built.nodes,
      edges: built.edges,
      reports,
      index: {
        provider: PROVIDER_ID,
        indexer: parsed.metadata?.indexer ?? "scip-python",
        version: String(parsed.metadata?.version ?? ""),
        path: probe.indexPath,
        documentCount: parsed.documents.length,
        definitionCount: built.definitionCount,
        referenceCount: built.referenceCount,
        state: probe.state,
      },
    };
  },
});

export { PROVIDER_ID, ADAPTER_VERSION };
