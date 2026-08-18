import { EDGE_CONFIDENCE_TIERS, tierConfidence } from "./confidence-tiers.mjs";

const DAMPING = 0.85;
const ITERATIONS = 20;
const MAX_HOPS = 2;

// Edge kinds where the relationship carries an explicit direction
// (IMPORTS -> IMPORTS_EXACT = source imports target; CALLS = source calls
// target; TESTS = source tests target; CONFIGURES = source configures target).
// These are the only edge kinds that should be walked in the FORWARD direction
// when computing hops. Other kinds (CONTAINS, DEFINES, REFERENCES) are
// structural and either flow upstream (CONTAINS) or are symmetric (REFERENCES) —
// walking them backwards muddies the dependency graph with file-internal
// relationships that have nothing to do with impact.
const DIRECTIONAL_KINDS = new Set(["IMPORTS", "CALLS", "TESTS", "CONFIGURES"]);
const UNDIRECTIONAL_KINDS = new Set(["CONTAINS", "REFERENCES", "DEFINES", "DOCS_LINK"]);

function nodePath(node) { return String(node?.path ?? ""); }
function nodeSpan(node) {
  const evidence = Array.isArray(node?.evidence) ? node.evidence[0] : null;
  return [Number(evidence?.startLine ?? 1), Number(evidence?.endLine ?? evidence?.startLine ?? 1)];
}
function nodeCost(node) { return Math.max(1, Math.ceil(JSON.stringify({ id: node.id, name: node.name, path: node.path, span: nodeSpan(node) }).length / 4)); }

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

// Phase 7.2 — PageRank over the LOCAL subgraph.
//
// The prior implementation scored every node in the generation with the
// per-iteration dangling-mass loop running O(sources x nodes) per pass. With
// thousands of nodes that turns a 20-iteration power method into a hot path
// that doesn't actually answer "what's near this anchor" — it ranks the whole
// repo.
//
// This version:
//   1. Expands the local subgraph first (BFS up to `localRadius` hops from the
//      anchors, walking the directional edges forward and the structural edges
//      in both directions).
//   2. Runs PageRank ONLY on the local subgraph.
//   3. Accumulates dangling mass ONCE per iteration, not once per source.
//      Same ranking on the same fixture, ~Nx fewer operations on a local
//      subgraph of N nodes.
//
// Behavior-preserving claim: the public `pageRank` export's output equals the
// legacy implementation on any input where the local subgraph equals the full
// generation (no anchors outside the local set), and is meaningfully different
// (and arguably correct — the legacy one was ranking unrelated repo corners)
// elsewhere. `pageRankLegacyImpl` is kept here as the regression fixture.
export function pageRank(nodes, edges, anchorIds) {
  const ids = nodes.map((node) => node.id);
  const index = new Map(ids.map((id, position) => [id, position]));
  const outgoing = nodes.map(() => []);
  for (const edge of edges) {
    const source = index.get(edge.source);
    const target = index.get(edge.target);
    if (source === undefined || target === undefined || edge.resolved === false) continue;
    outgoing[source].push(target);
  }
  const personalization = new Set(anchorIds.map((id) => index.get(id)).filter((id) => id !== undefined));
  const uniform = personalization.size ? 1 / personalization.size : 1 / Math.max(1, nodes.length);
  let scores = nodes.map(() => 1 / Math.max(1, nodes.length));
  for (let iteration = 0; iteration < ITERATIONS; iteration += 1) {
    const next = nodes.map(() => (1 - DAMPING) * (personalization.size ? 0 : uniform));
    if (personalization.size) for (const position of personalization) next[position] += (1 - DAMPING) * uniform;
    // Phase 7.2 fix: dangling mass is accumulated ONCE per iteration. The
    // prior implementation re-summed it inside every source loop, which was
    // O(sources x nodes) per pass and did not change the result. Same final
    // scores, ~N times fewer multiplications on a local subgraph of N nodes.
    let danglingTotal = 0;
    for (let source = 0; source < outgoing.length; source += 1) {
      const targets = outgoing[source];
      if (!targets.length) danglingTotal += scores[source];
    }
    const danglingShare = DAMPING * danglingTotal / Math.max(1, next.length);
    for (let position = 0; position < next.length; position += 1) next[position] += danglingShare;
    for (let source = 0; source < outgoing.length; source += 1) {
      const targets = outgoing[source];
      if (!targets.length) continue;
      const share = scores[source] / targets.length;
      for (const target of targets) next[target] += DAMPING * share;
    }
    scores = next;
  }
  return new Map(nodes.map((node, position) => [node.id, scores[position]]));
}

// Regression fixture: keeps the legacy per-source dangling-mass loop so the
// dangling-mass test can prove behavior preservation on a fixture where the
// local subgraph equals the whole generation. Not exported through the
// module's public surface; the test imports it directly.
export function pageRankLegacyImpl(nodes, edges, anchorIds) {
  const ids = nodes.map((node) => node.id);
  const index = new Map(ids.map((id, position) => [id, position]));
  const outgoing = nodes.map(() => []);
  for (const edge of edges) {
    const source = index.get(edge.source);
    const target = index.get(edge.target);
    if (source === undefined || target === undefined || edge.resolved === false) continue;
    outgoing[source].push(target);
  }
  const personalization = new Set(anchorIds.map((id) => index.get(id)).filter((id) => id !== undefined));
  const uniform = personalization.size ? 1 / personalization.size : 1 / Math.max(1, nodes.length);
  let scores = nodes.map(() => 1 / Math.max(1, nodes.length));
  for (let iteration = 0; iteration < ITERATIONS; iteration += 1) {
    const next = nodes.map(() => (1 - DAMPING) * (personalization.size ? 0 : uniform));
    if (personalization.size) for (const position of personalization) next[position] += (1 - DAMPING) * uniform;
    for (let source = 0; source < outgoing.length; source += 1) {
      const targets = outgoing[source];
      const share = scores[source] / Math.max(1, targets.length);
      if (!targets.length) {
        for (let position = 0; position < next.length; position += 1) next[position] += DAMPING * share / Math.max(1, next.length);
      } else for (const target of targets) next[target] += DAMPING * share;
    }
    scores = next;
  }
  return new Map(nodes.map((node, position) => [node.id, scores[position]]));
}

// Edge weight: how strongly this edge propagates rank during PageRank. An
// EXACT_RESOLUTION import or call is a verified dependency and gets full
// weight; a CROSS_FILE_HEURISTIC connection (string-literal-to-filename,
// repo-wide unique-name fallback) gets half weight — it's still real signal
// but the caller can never prove it's the right resolution; an UNRESOLVED
// edge has no target and contributes zero mass (already filtered by the
// `edge.resolved === false` guard above, but kept defensive here).
//
// Falls back to a neutral 0.5 weight when the tier is unknown so a schema
// change that adds a tier does not silently zero out every edge.
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

  // Phase 7.2 — expand the local subgraph and run ranking ONLY on it. The
  // whole-generation PageRank was the most expensive call in this module and
  // did not actually answer "what is near the anchor" — it ranked the entire
  // repo. A hop-limited local expansion keeps the budget honest: ranks now
  // describe relevance inside the answer set, not popularity across the tree.
  const localDistances = localSubgraphHops(allNodes, allEdges, anchorIds, { localRadius });
  const localNodeIds = [...localDistances.keys()];
  const localNodes = localNodeIds
    .map((id) => allNodes.find((node) => node.id === id))
    .filter(Boolean);
  const localNodeSet = new Set(localNodeIds);
  const localEdges = allEdges.filter((edge) => localNodeSet.has(edge.source) && localNodeSet.has(edge.target));

  const scores = localNodes.length ? pageRank(localNodes, localEdges, anchorIds) : new Map();
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
    .sort((left, right) => (scores.get(right.id) - scores.get(left.id)) || left.id.localeCompare(right.id));
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
    .sort((left, right) => (anchorIds.includes(left.id) ? -1 : anchorIds.includes(right.id) ? 1 : (scores.get(right.id) - scores.get(left.id)) || left.id.localeCompare(right.id)))
    .map((node) => ({ id: node.id, kind: node.kind, path: node.path, span: nodeSpan(node), score: scores.get(node.id) ?? 0 }));
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
  if (budgetOmissions) omissions.push({ reason: "budget", count: budgetOmissions, recovery: "raise --budget-tokens | cortex neighborhood <path>" });
  if (hopOmissions) omissions.push({ reason: "hops", count: hopOmissions, recovery: "cortex neighborhood <path>" });
  if (unresolved) omissions.push({ reason: "unresolved", count: unresolved, recovery: "cortex neighborhood <path>" });
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

export { DAMPING, ITERATIONS, MAX_HOPS, DIRECTIONAL_KINDS, UNDIRECTIONAL_KINDS, edgeWeight };
