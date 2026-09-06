import { createHash } from "node:crypto";

import { buildEntryPointRegistry } from "./entry-points.mjs";

const FLOW_KINDS = new Set(["CALLS", "ROUTES_TO", "HANDLES", "PRODUCES", "CONSUMES", "READS", "WRITES", "USES", "DEPLOYS"]);
function digest(value) { return createHash("sha256").update(String(value)).digest("hex"); }

function nodeRef(node) {
  return { id: node.id, kind: node.kind, name: node.name ?? null, path: node.path ?? node.evidence?.[0]?.path ?? null, evidence: node.evidence ?? [] };
}

export function buildProcessProjection(generation, { maxProcesses = 64, maxDepth = 12, maxSteps = 256 } = {}) {
  const byId = new Map((generation?.nodes ?? []).map((node) => [node.id, node]));
  const outgoing = new Map();
  const frontiers = [];
  for (const edge of generation?.edges ?? []) {
    if (!FLOW_KINDS.has(edge.kind)) continue;
    if (!edge.target) {
      frontiers.push({ source: edge.source ?? null, relation: edge.kind, reason: edge.reason ?? "unresolved_flow_edge", evidence: edge.evidence ?? [] });
      continue;
    }
    if (!outgoing.has(edge.source)) outgoing.set(edge.source, []);
    outgoing.get(edge.source).push(edge);
  }
  for (const edges of outgoing.values()) edges.sort((a, b) => a.id.localeCompare(b.id));

  const entries = buildEntryPointRegistry(generation, { includeStructuralCandidates: false }).slice(0, maxProcesses);
  const processes = [];
  for (const entry of entries) {
    const root = entry.node;
    const queue = [{ id: root.id, depth: 0, via: null, parentStepId: null }];
    const seen = new Set();
    const steps = [];
    let truncated = false;
    while (queue.length && steps.length < maxSteps) {
      const current = queue.shift();
      if (seen.has(current.id)) continue;
      seen.add(current.id);
      const node = byId.get(current.id);
      if (!node) continue;
      const stepId = `step:${digest(`${root.id}\0${current.id}\0${current.depth}`)}`;
      steps.push({
        stepId,
        ordinal: steps.length,
        node: nodeRef(node),
        viaRelation: current.via ? { id: current.via.id, kind: current.via.kind } : null,
        parentStepId: current.parentStepId,
        terminal: !(outgoing.get(current.id)?.length),
      });
      if (current.depth >= maxDepth) {
        if (outgoing.get(current.id)?.length) truncated = true;
        continue;
      }
      for (const edge of outgoing.get(current.id) ?? []) queue.push({ id: edge.target, depth: current.depth + 1, via: edge, parentStepId: stepId });
    }
    if (queue.length) truncated = true;
    processes.push({
      processId: `process:${digest(root.id)}`,
      entryPoint: { id: root.id, kind: entry.kind, confidence: entry.confidence, evidence: entry.evidence },
      steps,
      truncated,
      omissions: truncated ? [{ reason: "process_projection_bound", maxDepth, maxSteps }] : [],
    });
  }
  return Object.freeze({
    schemaVersion: 1,
    kind: "process-projection",
    generationId: generation?.manifest?.generationId ?? null,
    processes: Object.freeze(processes),
    frontiers: Object.freeze(frontiers),
    truncated: entries.length >= maxProcesses,
  });
}
