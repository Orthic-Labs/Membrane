// D40: reproducible graph analytics — deterministic views with algorithm/
// version/input generation. Derived scores never become source truth.

import { createHash } from "node:crypto";

export const ANALYTICS_VERSION = "1.0.0";

// Tarjan SCC — deterministic order with stable tie-breaking.
export function findSccs(edges = []) {
  const graph = new Map();
  for (const edge of edges) {
    if (!graph.has(edge.source)) graph.set(edge.source, []);
    graph.get(edge.source).push(edge.target);
    if (!graph.has(edge.target)) graph.set(edge.target, []);
  }
  for (const list of graph.values()) list.sort();
  const index = new Map();
  const lowlink = new Map();
  const onStack = new Set();
  const stack = [];
  const sccs = [];
  let counter = 0;
  const strongconnect = (node) => {
    index.set(node, counter);
    lowlink.set(node, counter);
    counter += 1;
    stack.push(node);
    onStack.add(node);
    for (const neighbor of graph.get(node) ?? []) {
      if (!index.has(neighbor)) {
        strongconnect(neighbor);
        lowlink.set(node, Math.min(lowlink.get(node), lowlink.get(neighbor)));
      } else if (onStack.has(neighbor)) {
        lowlink.set(node, Math.min(lowlink.get(node), index.get(neighbor)));
      }
    }
    if (lowlink.get(node) === index.get(node)) {
      const component = [];
      let member;
      do {
        member = stack.pop();
        onStack.delete(member);
        component.push(member);
      } while (member !== node);
      component.sort();
      sccs.push(component);
    }
  };
  for (const node of [...graph.keys()].sort()) {
    if (!index.has(node)) strongconnect(node);
  }
  return sccs;
}

// Cycle detection = SCCs of size > 1 (or self-loops).
export function findCycles(edges = []) {
  return findSccs(edges).filter((component) => component.length > 1 || (component.length === 1 && edges.some((e) => e.source === component[0] && e.target === component[0])));
}

// Dead-code candidates: nodes with no incoming edges (except roots), never
// proven dead — candidates only.
export function deadCodeCandidates({ nodes = [], edges = [] } = {}) {
  const referenced = new Set(edges.map((e) => e.target));
  return nodes.filter((node) => !referenced.has(node.id)).map((node) => ({ id: node.id, candidate: true, reason: "no_incoming_edges" }));
}

// Deterministic layer clustering: assign layers by longest path from roots,
// stable tie-breaking by node id.
export function assignLayers({ nodes = [], edges = [] } = {}) {
  const incoming = new Map(nodes.map((node) => [node.id, []]));
  for (const edge of edges) {
    if (!incoming.has(edge.target)) continue;
    incoming.get(edge.target).push(edge.source);
  }
  const layers = new Map();
  const visit = (nodeId, depth) => {
    const current = layers.get(nodeId) ?? 0;
    layers.set(nodeId, Math.max(current, depth));
    for (const [target, sources] of incoming) {
      if (sources.includes(nodeId)) visit(target, depth + 1);
    }
  };
  for (const node of [...nodes].sort((a, b) => a.id.localeCompare(b.id))) {
    if ((incoming.get(node.id) ?? []).length === 0) visit(node.id, 0);
  }
  return nodes.map((node) => ({ id: node.id, layer: layers.get(node.id) ?? 0 }));
}

export function analyticsDigest({ algorithm, inputGenerationId, params }) {
  return createHash("sha256")
    .update(`${algorithm}:${inputGenerationId}:${JSON.stringify(params ?? {})}`)
    .digest("hex")
    .slice(0, 16);
}

function clamp(value, minimum = 0, maximum = 1) {
  return Math.max(minimum, Math.min(maximum, Number(value) || 0));
}

/** Inspectable risk decomposition. Co-change is deliberately low-authority:
 * it can refine review priority but can never make a structurally safe change
 * high risk or cancel graph/evidence uncertainty. */
export function decomposeChangeRisk({
  changedPaths = [], impacted = [], edges = [], truncated = false,
  ambiguousSeeds = 0, stale = false, cochangeScore = 0,
} = {}) {
  const heuristicEdges = edges.filter((edge) => ["CROSS_FILE_HEURISTIC", "UNRESOLVED"].includes(edge.confidenceTier));
  const structural = clamp(Math.log2(1 + new Set(changedPaths).size) / 5);
  const reach = clamp(Math.log2(1 + new Set(impacted.map((item) => item.id ?? item.path ?? String(item))).size) / 7);
  const uncertainty = clamp((edges.length ? heuristicEdges.length / edges.length : 0) + (truncated ? 0.35 : 0) + Math.min(0.35, ambiguousSeeds * 0.1) + (stale ? 0.35 : 0));
  const historical = clamp(cochangeScore) * 0.15;
  const score = clamp(structural * 0.3 + reach * 0.35 + uncertainty * 0.35 + historical);
  return Object.freeze({
    schemaVersion: 1,
    kind: "ChangeRiskDecomposition",
    score,
    band: score >= 0.67 ? "high" : score >= 0.34 ? "medium" : "low",
    factors: Object.freeze([
      { id: "change_breadth", authority: "structural", value: structural, evidence: { changedPathCount: new Set(changedPaths).size } },
      { id: "impact_reach", authority: "structural", value: reach, evidence: { impactedCount: impacted.length } },
      { id: "evidence_uncertainty", authority: "graph", value: uncertainty, evidence: { heuristicEdgeCount: heuristicEdges.length, edgeCount: edges.length, truncated, ambiguousSeeds, stale } },
      { id: "cochange", authority: "historical_low", value: historical, evidence: { suppliedScore: clamp(cochangeScore), maximumContribution: 0.15 } },
    ]),
    authority: "advisory_not_truth",
  });
}
