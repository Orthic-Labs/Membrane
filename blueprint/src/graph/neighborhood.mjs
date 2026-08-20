import { EDGE_CONFIDENCE_TIERS } from "./confidence-tiers.mjs";

const MAX_HOPS = 2;
const DIRECTIONAL_KINDS = new Set(["IMPORTS", "CALLS", "TESTS", "CONFIGURES"]);
const UNDIRECTIONAL_KINDS = new Set(["CONTAINS", "REFERENCES", "DEFINES", "DOCS_LINK"]);
const TIER_RANK = Object.freeze({ EXACT_RESOLUTION: 0, SAME_FILE_LEXICAL: 1, CROSS_FILE_HEURISTIC: 2, UNRESOLVED: 3 });

function nodeSpan(node) {
  const evidence = Array.isArray(node?.evidence) ? node.evidence[0] : null;
  return [Number(evidence?.startLine ?? 1), Number(evidence?.endLine ?? evidence?.startLine ?? 1)];
}

function nodeCost(node) {
  return Math.max(1, Math.ceil(JSON.stringify({ id: node?.id, name: node?.name, path: node?.path, span: nodeSpan(node) }).length / 4));
}

function resolveAnchors(generation, anchors) {
  const nodes = generation.nodes ?? [];
  return (anchors ?? []).map((raw) => {
    const value = String(raw);
    const exact = nodes.find((node) => node.id === value)
      ?? nodes.find((node) => node.path === value && node.kind === "file")
      ?? nodes.find((node) => node.path === value);
    return { path: exact?.path ?? value, symbol: exact?.kind === "file" ? null : exact?.name ?? null, protected: true, node: exact ?? null };
  });
}

// Wire weight is an evidence-tier projection, never a ranking signal.
function edgeWeight(edge) {
  if (edge.resolved === false) return 0;
  const tier = edge.confidenceTier;
  if (!tier) return 0.5;
  switch (tier) {
    case EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION: return 1;
    case EDGE_CONFIDENCE_TIERS.SAME_FILE_LEXICAL: return 0.75;
    case EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC: return 0.5;
    case EDGE_CONFIDENCE_TIERS.UNRESOLVED: return 0;
    default: return 0.5;
  }
}

function nodeEvidenceRank(nodeId, edges) {
  let rank = 4;
  for (const edge of edges) {
    if (edge.source !== nodeId && edge.target !== nodeId) continue;
    rank = Math.min(rank, TIER_RANK[edge.confidenceTier] ?? 4);
  }
  return rank;
}

// Phase 7.2 — expand the LOCAL subgraph around the anchors.
//
// Walks the edges up to `localRadius` hops from the anchor set. Directional
// kinds (IMPORTS, CALLS, TESTS, CONFIGURES) are followed forward only — they
// describe a directed dependency. Symmetric kinds (CONTAINS, REFERENCES,
// DEFINES, DOCS_LINK) are walked both ways. UNRESOLVED edges are skipped
// outright (they have no target and were already filtered from the rank
// graph).
//
// Returns the BFS-distance map keyed by node id. `Infinity` means "not in the
// local subgraph"; `0` is an anchor.
export function localSubgraphHops(nodes, edges, anchorIds, { localRadius = 2 } = {}) {
  const nodeIds = new Set(nodes.map((node) => node.id));
  const outgoing = new Map();
  const incoming = new Map();
  for (const edge of edges) {
    if (edge.resolved === false || !edge.target) continue;
    if (!nodeIds.has(edge.source) || !nodeIds.has(edge.target)) continue;
    if (DIRECTIONAL_KINDS.has(edge.kind)) {
      if (!outgoing.has(edge.source)) outgoing.set(edge.source, []);
      outgoing.get(edge.source).push(edge.target);
    } else if (UNDIRECTIONAL_KINDS.has(edge.kind) || !DIRECTIONAL_KINDS.has(edge.kind)) {
      // Structural / unknown kinds: walk both directions.
      if (!outgoing.has(edge.source)) outgoing.set(edge.source, []);
      outgoing.get(edge.source).push(edge.target);
      if (!incoming.has(edge.target)) incoming.set(edge.target, []);
      incoming.get(edge.target).push(edge.source);
    }
  }
  const distances = new Map(anchorIds.filter((id) => nodeIds.has(id)).map((id) => [id, 0]));
  let frontier = [...distances.keys()];
  let depth = 0;
  while (frontier.length && depth < localRadius) {
    depth += 1;
    const next = [];
    for (const id of frontier) {
      const reach = [...(outgoing.get(id) ?? []), ...(incoming.get(id) ?? [])];
      for (const target of reach) {
        if (distances.has(target)) continue;
        distances.set(target, depth);
        next.push(target);
      }
    }
    frontier = next;
  }
  return distances;
}

function hopsFrom(nodes, edges, anchorIds) {
  const adjacent = new Map(nodes.map((node) => [node.id, []]));
  for (const edge of edges) {
    if (!adjacent.has(edge.source) || !adjacent.has(edge.target)) continue;
    adjacent.get(edge.source).push(edge.target);
    adjacent.get(edge.target).push(edge.source);
  }
  const distances = new Map(anchorIds.map((id) => [id, 0]));
  let frontier = [...anchorIds];
  while (frontier.length) {
    const next = [];
    for (const id of frontier) {
      const distance = distances.get(id);
      for (const target of adjacent.get(id) ?? []) {
        if (distances.has(target)) continue;
        distances.set(target, distance + 1);
        next.push(target);
      }
    }
    frontier = next;
  }
  return distances;
}

export function buildNeighborhood(generation, anchors, { budgetTokens = 8000, receiptId = null, repoId = null, repoRoot = null, maxHops = MAX_HOPS, localRadius = maxHops + 1 } = {}) {
  const allNodes = generation.nodes ?? [];
  const allEdges = generation.edges ?? [];
  const resolvedAnchors = resolveAnchors(generation, anchors);
  const anchorIds = resolvedAnchors.map((anchor) => anchor.node?.id).filter(Boolean);

  // Expand only bounded local evidence.
  const localDistances = localSubgraphHops(allNodes, allEdges, anchorIds, { localRadius });
  const localNodeIds = [...localDistances.keys()];
  const localNodes = localNodeIds
    .map((id) => allNodes.find((node) => node.id === id))
    .filter(Boolean);
  const localNodeSet = new Set(localNodeIds);
  const localEdges = allEdges.filter((edge) => localNodeSet.has(edge.source) && localNodeSet.has(edge.target));

  // `distances` is still useful for the budget hop gate — keep it as a Map
  // the rest of the function reads. Anchors are at distance 0; everything
  // outside the local subgraph is Infinity.
  const distances = localDistances;
  // Nodes outside the local subgraph get a synthetic distance so the hop gate
  // does not include them; this is equivalent to the prior whole-generation
  // computation when every anchor's reachable set is within `maxHops` of the
  // anchor set, and otherwise excludes repo-wide noise from the candidate set.
  for (const node of allNodes) {
    if (!distances.has(node.id)) distances.set(node.id, Infinity);
  }
  const selected = new Set(anchorIds);
  let usedTokens = anchorIds.reduce((sum, id) => sum + nodeCost(localNodes.find((node) => node.id === id) ?? allNodes.find((node) => node.id === id)), 0);
  const ranked = localNodes
    .filter((node) => !selected.has(node.id) && (distances.get(node.id) ?? Infinity) <= maxHops)
    .sort((left, right) => nodeEvidenceRank(left.id, localEdges) - nodeEvidenceRank(right.id, localEdges)
      || (distances.get(left.id) ?? Infinity) - (distances.get(right.id) ?? Infinity)
      || left.id.localeCompare(right.id));
  let budgetOmissions = 0;
  for (const node of ranked) {
    const cost = nodeCost(node);
    if (usedTokens + cost > Number(budgetTokens)) { budgetOmissions += 1; continue; }
    selected.add(node.id);
    usedTokens += cost;
  }
  const hopOmissions = localNodes.filter((node) => !selected.has(node.id) && !anchorIds.includes(node.id) && (distances.get(node.id) ?? Infinity) > maxHops).length;
  const unresolved = allEdges.filter((edge) => edge.resolved === false || !edge.target).length;
  const neurons = [...selected]
    .map((id) => localNodes.find((node) => node.id === id) ?? allNodes.find((node) => node.id === id))
    .filter(Boolean)
    .sort((left, right) => (anchorIds.includes(left.id) ? -1 : anchorIds.includes(right.id) ? 1
      : nodeEvidenceRank(left.id, localEdges) - nodeEvidenceRank(right.id, localEdges)
        || (distances.get(left.id) ?? Infinity) - (distances.get(right.id) ?? Infinity)
        || left.id.localeCompare(right.id)))
    .map((node) => ({ id: node.id, kind: node.kind, path: node.path, span: nodeSpan(node), evidenceTier: nodeEvidenceRank(node.id, localEdges) }));
  const synapses = localEdges.filter((edge) => selected.has(edge.source) && selected.has(edge.target)).map((edge) => ({
    id: edge.id,
    kind: edge.kind,
    source: edge.source,
    target: edge.target,
    confidenceTier: edge.confidenceTier ?? null,
    weight: edgeWeight(edge),
    resolved: edge.resolved !== false,
  })).sort((left, right) => left.id.localeCompare(right.id));
  const omissions = [];
  if (budgetOmissions) omissions.push({ reason: "budget", count: budgetOmissions, recovery: "raise --budget-tokens | blueprint neighborhood <path>" });
  if (hopOmissions) omissions.push({ reason: "hops", count: hopOmissions, recovery: "blueprint neighborhood <path>" });
  if (unresolved) omissions.push({ reason: "unresolved", count: unresolved, recovery: "blueprint neighborhood <path>" });
  const selectedIds = neurons.map((neuron) => neuron.id);
  return {
    schemaVersion: 1,
    kind: "RepositoryNeighborhoodV1",
    repoId: repoId ?? generation.repoId ?? null,
    repoRoot: repoRoot ?? generation.repoRoot ?? null,
    generationId: generation.manifest?.generationId ?? null,
    receiptId,
    anchors: resolvedAnchors.map(({ path, symbol, protected: isProtected }) => ({ path, symbol, protected: isProtected })),
    neurons,
    synapses,
    circuits: selectedIds.length ? [{ name: "repository-neighborhood", neuronIds: selectedIds }] : [],
    bounds: { budgetTokens: Number(budgetTokens), usedTokens, maxHops, localRadius },
    omissions,
  };
}

export { MAX_HOPS, DIRECTIONAL_KINDS, UNDIRECTIONAL_KINDS, edgeWeight };
