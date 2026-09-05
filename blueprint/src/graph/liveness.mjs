import { buildEntryPointRegistry } from "./entry-points.mjs";

export const LIVENESS_STATES = Object.freeze(["LIVE", "UNREACHED", "UNKNOWN"]);

function citations(values) {
  const rows = [], seen = new Set();
  for (const value of values) for (const item of value?.evidence ?? []) {
    const row = { path: item.path ?? null, startLine: item.startLine ?? null, endLine: item.endLine ?? null, contentHash: item.contentHash ?? null };
    const key = JSON.stringify(row);
    if (!seen.has(key)) { seen.add(key); rows.push(row); }
  }
  return rows;
}

export function buildLivenessProjection(generation, options = {}) {
  const generationId = generation?.manifest?.generationId ?? null;
  const maxNodes = Math.max(1, Math.min(20000, Number(options.maxNodes ?? 5000) || 5000));
  const maxEdges = Math.max(1, Math.min(100000, Number(options.maxEdges ?? 20000) || 20000));
  const maxHops = Math.max(1, Math.min(64, Number(options.maxHops ?? 24) || 24));
  const sourceState = options.sourceState ?? "clean";
  const allNodes = generation?.nodes ?? [];
  const selected = allNodes.slice(0, maxNodes);
  const boundedOut = allNodes.length > selected.length;
  const complete = generation?.manifest?.complete === true && !generation?.manifest?.truncated && !boundedOut;
  const registry = buildEntryPointRegistry(generation, { includeStructuralCandidates: true });
  const admitted = registry.filter((entry) => entry.authority === "explicit" && entry.evidence.length > 0);
  const trustworthy = complete && sourceState === "clean" && admitted.length > 0;

  if (!trustworthy) {
    const reason = sourceState !== "clean" ? "source_not_current" : !complete ? "generation_incomplete_or_bounded" : "no_explicit_entrypoint_evidence";
    return Object.freeze({
      schemaVersion: 1, kind: "liveness", generationId, sourceState,
      entryPoints: registry.map(({ node, ...entry }) => ({ ...entry, nodeId: node.id })),
      results: selected.map((node) => ({ nodeId: node.id, path: node.path ?? null, state: "UNKNOWN", reason, evidence: citations([node]), reachabilityPath: [] })),
      counts: { LIVE: 0, UNREACHED: 0, UNKNOWN: selected.length },
      omissions: boundedOut ? [{ reason: "node_ceiling", count: allNodes.length - selected.length }] : [],
      truncated: boundedOut,
    });
  }

  const allowed = new Set(selected.map((node) => node.id));
  const edges = (generation?.edges ?? []).filter((edge) => edge.target && edge.resolved !== false && edge.confidenceTier !== "UNRESOLVED" && allowed.has(edge.source) && allowed.has(edge.target)).slice(0, maxEdges);
  const adjacency = new Map();
  for (const edge of edges) {
    if (!adjacency.has(edge.source)) adjacency.set(edge.source, []);
    adjacency.get(edge.source).push(edge);
  }
  for (const rows of adjacency.values()) rows.sort((a, b) => String(a.id).localeCompare(String(b.id)));
  const reached = new Map();
  const queue = admitted.filter((entry) => allowed.has(entry.id)).map((entry) => ({ id: entry.id, hops: 0 }));
  for (const entry of admitted) if (allowed.has(entry.id)) reached.set(entry.id, { parent: null, edge: null, root: entry.id });
  while (queue.length) {
    const current = queue.shift();
    if (current.hops >= maxHops) continue;
    for (const edge of adjacency.get(current.id) ?? []) {
      if (reached.has(edge.target)) continue;
      const root = reached.get(current.id)?.root ?? current.id;
      reached.set(edge.target, { parent: current.id, edge, root });
      queue.push({ id: edge.target, hops: current.hops + 1 });
    }
  }
  const byId = new Map(selected.map((node) => [node.id, node]));
  const livePath = (id) => {
    const ids = [id], edgeRows = [];
    let cursor = id;
    while (reached.get(cursor)?.parent) {
      const state = reached.get(cursor);
      edgeRows.push(state.edge);
      cursor = state.parent;
      ids.push(cursor);
    }
    ids.reverse(); edgeRows.reverse();
    return { ids, evidence: citations([...ids.map((nodeId) => byId.get(nodeId)), ...edgeRows]) };
  };
  const results = selected.map((node) => {
    if (!reached.has(node.id)) return { nodeId: node.id, path: node.path ?? null, state: "UNREACHED", reason: "no_path_from_admitted_entrypoint", evidence: citations([node]), reachabilityPath: [] };
    const path = livePath(node.id);
    return { nodeId: node.id, path: node.path ?? null, state: "LIVE", reason: "evidence_backed_path_from_admitted_entrypoint", evidence: path.evidence, reachabilityPath: path.ids };
  });
  return Object.freeze({
    schemaVersion: 1, kind: "liveness", generationId, sourceState,
    entryPoints: registry.map(({ node, ...entry }) => ({ ...entry, nodeId: node.id })),
    results,
    counts: Object.fromEntries(LIVENESS_STATES.map((state) => [state, results.filter((row) => row.state === state).length])),
    omissions: (generation?.edges?.length ?? 0) > edges.length ? [{ reason: "edge_ceiling_or_out_of_scope", count: (generation?.edges?.length ?? 0) - edges.length }] : [],
    truncated: (generation?.edges?.length ?? 0) > maxEdges,
  });
}
