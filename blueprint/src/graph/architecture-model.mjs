// D40: architecture/layer model — components, layers, coupling, and hotspots
// derived from the graph, reproducible and generation-bound.

import { createHash } from "node:crypto";
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

function evidence(value) {
  return (Array.isArray(value?.evidence) ? value.evidence : []).filter(Boolean).map((item) => ({
    path: item.path ?? value.path ?? null,
    startLine: item.startLine ?? null,
    endLine: item.endLine ?? null,
    contentHash: item.contentHash ?? null,
  }));
}

function componentKey(node) {
  const path = String(node.path ?? evidence(node)[0]?.path ?? "").replaceAll("\\", "/");
  if (!path) return "unlocated";
  const parts = path.split("/");
  return parts.length > 1 ? parts.slice(0, Math.min(2, parts.length - 1)).join("/") : ".";
}

/** Disposable, generation-bound cited architecture view. It is deliberately
 * not persisted as graph truth & never grants planner authority. */
export function buildDisposableArchitectureProjection({ nodes = [], edges = [], generationId = null, maxComponents = 100, maxFlows = 200 } = {}) {
  const nodeById = new Map(nodes.map((node) => [node.id, node]));
  const groups = new Map();
  for (const node of nodes) {
    const key = componentKey(node);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(node);
  }
  const components = [...groups.entries()].sort(([left], [right]) => left.localeCompare(right)).slice(0, maxComponents).map(([id, members]) => ({
    id: `component:${id}`,
    label: id,
    nodeIds: members.map((node) => node.id).sort(),
    citations: members.flatMap(evidence).sort((left, right) => String(left.path).localeCompare(String(right.path))).slice(0, 50),
    inferred: true,
  }));
  const componentForNode = new Map(components.flatMap((component) => component.nodeIds.map((id) => [id, component.id])));
  const flows = [];
  for (const edge of [...edges].sort((left, right) => String(left.id).localeCompare(String(right.id)))) {
    const from = componentForNode.get(edge.source);
    const to = componentForNode.get(edge.target);
    if (!from || !to || from === to) continue;
    flows.push({
      id: `flow:${edge.id}`,
      from,
      to,
      edgeId: edge.id,
      kind: edge.kind,
      citations: [...evidence(edge), ...evidence(nodeById.get(edge.source)), ...evidence(nodeById.get(edge.target))],
      inferred: true,
    });
    if (flows.length >= maxFlows) break;
  }
  const visible = { generationId, components, flows };
  return {
    schemaVersion: 1,
    kind: "DisposableArchitectureProjection",
    id: `sha256:${createHash("sha256").update(JSON.stringify(visible)).digest("hex")}`,
    generationId,
    authority: "disposable_cited_view",
    plannerAuthority: "none",
    components,
    flows,
    omissions: [
      ...(groups.size > components.length ? [{ reason: "component_ceiling", count: groups.size - components.length }] : []),
      ...(flows.length >= maxFlows ? [{ reason: "flow_ceiling" }] : []),
    ],
  };
}
