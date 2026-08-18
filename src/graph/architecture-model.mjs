// D40: architecture/layer model — components, layers, coupling, and hotspots
// derived from the graph, reproducible and generation-bound.

import { assignLayers } from "./analytics/index.mjs";

export function buildArchitectureModel({ nodes = [], edges = [], generationId = null } = {}) {
  const layers = assignLayers({ nodes, edges });
  const byLayer = new Map();
  for (const entry of layers) {
    if (!byLayer.has(entry.layer)) byLayer.set(entry.layer, []);
    byLayer.get(entry.layer).push(entry.id);
  }
  const coupling = new Map();
  for (const edge of edges) {
    coupling.set(edge.source, (coupling.get(edge.source) ?? 0) + 1);
  }
  const hotspots = [...coupling.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, 20)
    .map(([id, count]) => ({ id, degree: count, candidate: true }));
  return {
    schemaVersion: 1,
    generationId,
    algorithm: { name: "layer-clustering", version: "1.0.0" },
    layers: [...byLayer.entries()].sort((a, b) => a[0] - b[0]).map(([layer, ids]) => ({ layer, nodeIds: ids })),
    coupling: [...coupling.entries()].map(([id, degree]) => ({ id, degree })).sort((a, b) => b.degree - a.degree),
    hotspots,
    nodeCount: nodes.length,
    edgeCount: edges.length,
  };
}
