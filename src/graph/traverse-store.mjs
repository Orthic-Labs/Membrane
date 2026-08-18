// Indexed graph retrieval. This module intentionally never calls loadGeneration:
// it reads the slim edge core and hydrates only node/edge ids touched by a query.

import {
  countRows,
  getGenerationEnvelope,
  hydrateEdgesByIds,
  hydrateNodesByIds,
  listEdgeCore,
  traversalNeighbors,
} from "./store-sqlite.mjs";

function providerId(provider) {
  return typeof provider === "string" ? provider : provider?.id ?? "unknown";
}

function indexedMeta(db) {
  const envelope = getGenerationEnvelope(db);
  if (!envelope.manifest) return null;
  const counts = countRows(db);
  const edgeKinds = db.prepare("SELECT kind, COUNT(*) AS n FROM edges GROUP BY kind ORDER BY kind").all();
  return {
    schemaVersion: envelope.schemaVersion ?? 1,
    provider: envelope.provider,
    manifest: envelope.manifest,
    repoRoot: envelope.repoRoot,
    counts,
    edgeKinds: Object.fromEntries(edgeKinds.map((row) => [row.kind, row.n])),
  };
}

export function readIndexedMeta(db) {
  return indexedMeta(db);
}

function hydrateNodeMap(db, ids) {
  return new Map(hydrateNodesByIds(db, ids).map((node) => [node.id, node]));
}

function hydrateEdgeMap(db, ids) {
  return new Map(hydrateEdgesByIds(db, ids).map((edge) => [edge.id, edge]));
}

export function indexedResolve(db, nodeId, options = {}) {
  const meta = indexedMeta(db);
  const node = hydrateNodesByIds(db, [nodeId])[0];
  if (!node) return null;
  return {
    schemaVersion: 1,
    provider: providerId(meta.provider),
    node: { ...node, fresh: options.sourceState !== "dirty_overlay" },
  };
}

function queryTerms(query) {
  const stop = new Set(["a", "an", "and", "for", "in", "of", "the", "to", "where"]);
  return [...new Set(String(query)
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .toLowerCase()
    .split(/[^a-z0-9_]+/)
    .filter((term) => term.length > 1 && !stop.has(term)))];
}

function scalarNodeScore(node, terms) {
  if (!terms.length) return 1;
  const haystack = `${node.kind} ${node.name} ${node.qualifiedName} ${node.path}`.toLowerCase();
  return terms.reduce((sum, term) => sum + (haystack.includes(term) ? 1 : 0), 0);
}

/** Return a bounded, hydrated node subset suitable for lexical consumers. */
export function indexedQueryGeneration(db, query, options = {}) {
  const meta = indexedMeta(db);
  const terms = queryTerms(query);
  const limit = Math.max(1, Number(options.limit ?? 20));
  const poolLimit = Math.max(limit * 3, limit + 20);
  const ids = [];
  const addRows = (table, idColumn, kind) => {
    const clauses = [];
    const params = [];
    for (const term of terms) {
      clauses.push("lower(COALESCE(name, '') || ' ' || COALESCE(qualified_name, '') || ' ' || path) LIKE ?");
      params.push(`%${term}%`);
    }
    const where = clauses.length ? `WHERE ${clauses.join(" OR ")}` : "";
    const rows = db.prepare(`SELECT ${idColumn} AS id, ? AS kind FROM ${table} ${where} ORDER BY rowid LIMIT ?`)
      .all(kind, ...params, poolLimit);
    ids.push(...rows.map((row) => row.id));
  };
  addRows("files", "node_id", "file");
  addRows("symbols", "id", "symbol");
  for (const anchor of options.anchors ?? []) {
    const row = db.prepare("SELECT node_id AS id FROM files WHERE path = ?").get(String(anchor).replaceAll("\\", "/"));
    if (row?.id) ids.push(row.id);
  }
  const nodes = hydrateNodesByIds(db, ids)
    .map((node) => ({ node, score: scalarNodeScore(node, terms) }))
    .filter((item) => item.score > 0 || (options.anchors ?? []).includes(item.node.path))
    .sort((a, b) => b.score - a.score || a.node.id.localeCompare(b.node.id))
    .slice(0, poolLimit)
    .map((item) => item.node);
  return {
    schemaVersion: meta.schemaVersion,
    provider: meta.provider,
    manifest: meta.manifest,
    nodes,
    edges: [],
  };
}

export function indexedNeighbors(db, options = {}) {
  const meta = indexedMeta(db);
  const nodeId = String(options.nodeId ?? "");
  const direction = options.direction ?? "both";
  const maxDepth = Math.max(1, Number(options.depth ?? 1));
  // SQL frontier expansion (Phase 7.1): the recursive CTE walks the edges
  // table once per direction, bounded by `maxDepth` and `generation_id`. The
  // JS frontier loop that used to scan every edge in the generation is gone.
  // Behavior is preserved: same `root`, same `nodes`/`edges`, same `depths`.
  const generationId = meta?.manifest?.generationId ?? null;
  const frontier = generationId
    ? traversalNeighbors(db, { seedIds: [nodeId], maxDepth, direction, generationId })
    : null;
  let seenNodes;
  let seenEdges;
  let depths;
  if (frontier) {
    seenNodes = new Set(frontier.seenNodes);
    seenEdges = new Set(frontier.seenEdges);
    depths = new Map(frontier.depths);
  } else {
    // No generation envelope (rare — empty/in-memory stores): fall back to
    // scanning the slim edge core in JS so the contract is preserved on a
    // fresh ":memory:" handle.
    seenNodes = new Set([nodeId]);
    seenEdges = new Set();
    depths = new Map([[nodeId, 0]]);
    let frontierSet = new Set([nodeId]);
    for (let depth = 0; depth < maxDepth; depth += 1) {
      const next = new Set();
      for (const edge of listEdgeCore(db)) {
        const out = direction !== "in" && frontierSet.has(edge.source);
        const incoming = direction !== "out" && frontierSet.has(edge.target);
        if (!out && !incoming) continue;
        seenEdges.add(edge.id);
        const other = out ? edge.target : edge.source;
        if (!seenNodes.has(other)) {
          next.add(other);
          depths.set(other, depth + 1);
        }
        seenNodes.add(other);
      }
      frontierSet = next;
      if (!frontierSet.size) break;
    }
  }
  depths.set(nodeId, 0);
  const hydratedNodes = hydrateNodesByIds(db, [...seenNodes]);
  const hydratedEdges = hydrateEdgesByIds(db, [...seenEdges]);
  const result = {
    schemaVersion: 1,
    provider: providerId(meta.provider),
    root: nodeId,
    nodes: hydratedNodes,
    edges: hydratedEdges,
    truncated: false,
  };
  Object.defineProperties(result, {
    depths: { value: depths },
    nodeMap: { value: new Map(hydratedNodes.map((node) => [node.id, node])) },
    edgeMap: { value: new Map(hydratedEdges.map((edge) => [edge.id, edge])) },
  });
  return result;
}

export function indexedPath(db, options = {}) {
  const meta = indexedMeta(db);
  const from = String(options.from ?? "");
  const to = String(options.to ?? "");
  const maxDepth = Math.max(1, Number(options.maxDepth ?? 5));
  // Phase 7.1: indexedPath previously loaded every edge into JS and BFS-walked
  // the outgoing map. The SQL frontier still drives the BFS, but each
  // `listEdgeCore` call now passes `source` + `generation_id`, so the
  // (generation_id, source, kind) compound index carries the lookup. The JS
  // frontier scan over all edges is gone.
  const generationId = meta?.manifest?.generationId ?? null;
  const queue = [[from]];
  const visited = new Set([from]);
  while (queue.length) {
    const path = queue.shift();
    const current = path.at(-1);
    if (current === to) {
      const edgeIds = [];
      for (let index = 0; index < path.length - 1; index += 1) {
        const edge = findEdgeBetween(db, path[index], path[index + 1], generationId);
        if (edge) edgeIds.push(edge.id);
      }
      const nodes = hydrateNodeMap(db, path);
      const edges = hydrateEdgeMap(db, edgeIds);
      return {
        schemaVersion: 1,
        provider: providerId(meta.provider),
        found: true,
        path: path.map((id) => nodes.get(id)).filter(Boolean),
        edges: edgeIds.map((id) => edges.get(id)).filter(Boolean),
      };
    }
    if (path.length > maxDepth) continue;
    const nextEdges = generationId
      ? listEdgeCore(db, { source: current, generationId })
      : listEdgeCore(db);
    for (const edge of nextEdges) {
      if (!edge.target) continue;
      if (visited.has(edge.target)) continue;
      visited.add(edge.target);
      queue.push([...path, edge.target]);
    }
  }
  return { schemaVersion: 1, provider: providerId(meta.provider), from, to, path: [], edges: [], found: false };
}

function findEdgeBetween(db, source, target, generationId) {
  const candidates = generationId
    ? listEdgeCore(db, { source, generationId })
    : listEdgeCore(db).filter((edge) => edge.source === source);
  return candidates.find((edge) => edge.target === target) ?? null;
}

export function indexedImpact(db, options = {}) {
  const nodeId = String(options.nodeId ?? "");
  const depth = Math.max(1, Number(options.depth ?? 3));
  const neighborhood = indexedNeighbors(db, { nodeId, direction: "in", depth });
  const result = {
    schemaVersion: 1,
    provider: neighborhood.provider,
    target: neighborhood.nodeMap.get(nodeId) ?? { id: nodeId },
    direction: "upstream",
    impacted: neighborhood.nodes.filter((node) => node.id !== nodeId),
    edges: neighborhood.edges,
    truncated: neighborhood.truncated,
  };
  Object.defineProperty(result, "depths", { value: neighborhood.depths });
  return result;
}

function nodeReference(node) {
  const evidence = node?.evidence?.[0] ?? {};
  return {
    id: node.id,
    kind: node.kind,
    path: node.path ?? evidence.path ?? null,
    startLine: evidence.startLine ?? null,
    endLine: evidence.endLine ?? null,
  };
}

function edgeReference(edge) {
  return {
    id: edge.id,
    kind: edge.kind,
    source: edge.source,
    target: edge.target,
    confidenceTier: edge.confidenceTier ?? null,
  };
}

function decodeCursor(cursor) {
  if (!cursor) return { node: 0, edge: 0 };
  try {
    const value = JSON.parse(Buffer.from(String(cursor), "base64url").toString("utf8"));
    return { node: Math.max(0, Number(value.node ?? 0)), edge: Math.max(0, Number(value.edge ?? 0)) };
  } catch {
    return { node: 0, edge: 0 };
  }
}

function encodeCursor(node, edge) {
  return Buffer.from(JSON.stringify({ node, edge })).toString("base64url");
}

function tierRank(edge) {
  return ({ EXACT_RESOLUTION: 0, SAME_FILE_LEXICAL: 1, CROSS_FILE_HEURISTIC: 2, UNRESOLVED: 3 })[edge.confidenceTier] ?? 4;
}

function rowTokens(row) {
  return Math.max(1, Math.ceil(JSON.stringify(row).length / 4));
}

function safeBudget(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? Math.floor(parsed) : 2000;
}

function rankedReferences(nodes, edges, depths, root) {
  const degree = new Map();
  for (const edge of edges) {
    degree.set(edge.source, (degree.get(edge.source) ?? 0) + 1);
    if (edge.target !== null) degree.set(edge.target, (degree.get(edge.target) ?? 0) + 1);
  }
  const nodeTier = new Map();
  for (const edge of edges) {
    for (const id of [edge.source, edge.target]) {
      if (id === null) continue;
      nodeTier.set(id, Math.min(nodeTier.get(id) ?? 99, tierRank(edge)));
    }
  }
  const rankedNodes = [...nodes].sort((a, b) =>
    (a.id === root ? -1 : 0) - (b.id === root ? -1 : 0)
    || (nodeTier.get(a.id) ?? 99) - (nodeTier.get(b.id) ?? 99)
    || (degree.get(b.id) ?? 0) - (degree.get(a.id) ?? 0)
    || (depths?.get(a.id) ?? 99) - (depths?.get(b.id) ?? 99)
    || a.id.localeCompare(b.id));
  const rankedEdges = [...edges].sort((a, b) =>
    tierRank(a) - tierRank(b)
    || Math.min(depths?.get(a.source) ?? 99, depths?.get(a.target) ?? 99) - Math.min(depths?.get(b.source) ?? 99, depths?.get(b.target) ?? 99)
    || a.id.localeCompare(b.id));
  return { rankedNodes, rankedEdges };
}

function freshnessFields(freshness = {}) {
  return {
    generationId: freshness.generationId ?? null,
    sourceState: freshness.sourceState ?? "clean",
    dirtyFileCount: Number(freshness.dirtyFileCount ?? 0),
  };
}

function boundedPayload({ provider, freshness, kind, nodes, edges, depths, root, budget = 2000, cursor }) {
  budget = safeBudget(budget);
  const { rankedNodes, rankedEdges } = rankedReferences(nodes, edges, depths, root);
  const start = decodeCursor(cursor);
  const selectedNodes = [];
  const selectedEdges = [];
  let used = 0;
  let nodeIndex = start.node;
  let edgeIndex = start.edge;
  for (; nodeIndex < rankedNodes.length; nodeIndex += 1) {
    const row = nodeReference(rankedNodes[nodeIndex]);
    const cost = rowTokens(row);
    if (used + cost > budget && selectedNodes.length + selectedEdges.length > 0) break;
    selectedNodes.push(row);
    used += cost;
  }
  for (; edgeIndex < rankedEdges.length; edgeIndex += 1) {
    const row = edgeReference(rankedEdges[edgeIndex]);
    const cost = rowTokens(row);
    if (used + cost > budget && selectedNodes.length + selectedEdges.length > 0) break;
    selectedEdges.push(row);
    used += cost;
  }
  const truncated = nodeIndex < rankedNodes.length || edgeIndex < rankedEdges.length;
  const nextCursor = truncated ? encodeCursor(nodeIndex, edgeIndex) : null;
  return {
    schemaVersion: 2,
    provider,
    kind,
    ...freshnessFields(freshness),
    root: root ?? undefined,
    counts: {
      nodes: nodes.length,
      edges: edges.length,
      edgesByKind: Object.fromEntries([...new Set(edges.map((edge) => edge.kind))].sort().map((edgeKind) => [edgeKind, edges.filter((edge) => edge.kind === edgeKind).length])),
    },
    nodes: selectedNodes,
    edges: selectedEdges,
    truncated,
    continuationCursor: nextCursor,
    budget: Number(budget),
  };
}

export function boundedNeighbors(db, options = {}) {
  const raw = indexedNeighbors(db, options);
  const meta = indexedMeta(db);
  return boundedPayload({
    provider: providerId(meta.provider),
    freshness: options.freshness,
    kind: "neighbors",
    nodes: raw.nodes,
    edges: raw.edges,
    depths: raw.depths,
    root: raw.root,
    budget: options.budget,
    cursor: options.cursor,
  });
}

export function boundedImpact(db, options = {}) {
  const raw = indexedImpact(db, options);
  const meta = indexedMeta(db);
  const payload = boundedPayload({
    provider: providerId(meta.provider),
    freshness: options.freshness,
    kind: "impact",
    nodes: [raw.target, ...raw.impacted.filter((node) => node.id !== raw.target.id)],
    edges: raw.edges,
    depths: raw.depths,
    root: options.nodeId,
    budget: options.budget,
    cursor: options.cursor,
  });
  const target = nodeReference(raw.target);
  return {
    ...payload,
    target,
    impacted: payload.nodes.filter((node) => node.id !== target.id),
    nodes: undefined,
  };
}

export function boundedPath(db, options = {}) {
  const raw = indexedPath(db, options);
  const meta = indexedMeta(db);
  const payload = boundedPayload({
    provider: providerId(meta.provider),
    freshness: options.freshness,
    kind: "path",
    nodes: raw.path,
    edges: raw.edges,
    depths: new Map(raw.path.map((node, index) => [node.id, index])),
    root: options.from,
    budget: options.budget,
    cursor: options.cursor,
  });
  return {
    ...payload,
    path: payload.nodes,
    nodes: undefined,
    from: options.from ?? "",
    to: options.to ?? "",
    found: raw.found,
  };
}

export function boundedArchitecture(db, options = {}) {
  const meta = indexedMeta(db);
  const generationId = meta?.manifest?.generationId ?? null;
  const edges = generationId ? listEdgeCore(db, { generationId }) : listEdgeCore(db);
  const incoming = new Set(edges.map((edge) => edge.target));
  const outgoing = new Set(edges.map((edge) => edge.source));
  const symbolIds = db.prepare("SELECT id FROM symbols ORDER BY rowid").all().map((row) => row.id);
  const entryIds = symbolIds.filter((id) => outgoing.has(id) && !incoming.has(id));
  const terminalIds = [...new Set(edges.map((edge) => edge.target).filter(Boolean))].filter((id) => !outgoing.has(id));
  const sampleIds = [...new Set([...entryIds, ...terminalIds])];
  const samples = hydrateNodesByIds(db, sampleIds);
  const counts = countRows(db);
  const edgesByKind = Object.fromEntries([...new Set(edges.map((edge) => edge.kind))].sort().map((kind) => [kind, edges.filter((edge) => edge.kind === kind).length]));
  const budget = safeBudget(options.budget ?? 2000);
  const ranked = rankedReferences(samples, [], new Map(), null).rankedNodes;
  let used = 0;
  const examples = [];
  for (const node of ranked) {
    const row = nodeReference(node);
    const cost = rowTokens(row);
    if (used + cost > budget && examples.length) break;
    examples.push(row);
    used += cost;
  }
  return {
    schemaVersion: 2,
    provider: providerId(meta.provider),
    kind: "architecture",
    ...freshnessFields(options.freshness),
    counts: { nodes: counts.files + counts.symbols, files: counts.files, symbols: counts.symbols, edges: counts.edges, edgesByKind },
    entryPoints: entryIds.length,
    terminals: terminalIds.length,
    examples,
    truncated: examples.length < samples.length,
    continuationCursor: examples.length < samples.length ? encodeCursor(examples.length, 0) : null,
    budget,
  };
}

export function encodeTabular(payload) {
  const lines = [
    `schemaVersion=${payload.schemaVersion} provider=${payload.provider} kind=${payload.kind}`,
    `generationId=${payload.generationId ?? ""} sourceState=${payload.sourceState ?? "clean"} dirtyFileCount=${payload.dirtyFileCount ?? 0}`,
    `truncated=${Boolean(payload.truncated)} continuationCursor=${payload.continuationCursor ?? ""}`,
  ];
  for (const [key, value] of Object.entries(payload.counts ?? {})) {
    lines.push(`count.${key}=${typeof value === "object" ? JSON.stringify(value) : value}`);
  }
  if (payload.root !== undefined) lines.push(`root=${payload.root}`);
  if (payload.from !== undefined) lines.push(`from=${payload.from} to=${payload.to} found=${Boolean(payload.found)}`);
  if (payload.examples) {
    lines.push("examples[id kind path startLine endLine]");
    for (const row of payload.examples) lines.push([row.id, row.kind, row.path ?? "", row.startLine ?? "", row.endLine ?? ""].map(String).join("\t"));
  }
  const nodeRows = payload.nodes ?? payload.path ?? (payload.impacted ? [payload.target, ...payload.impacted].filter(Boolean) : null);
  if (nodeRows) {
    lines.push("nodes[id kind path startLine endLine]");
    for (const row of nodeRows) lines.push([row.id, row.kind, row.path ?? "", row.startLine ?? "", row.endLine ?? ""].map(String).join("\t"));
  }
  if (payload.edges) {
    lines.push("edges[id kind source target confidenceTier]");
    for (const row of payload.edges) lines.push([row.id, row.kind, row.source, row.target ?? "", row.confidenceTier ?? ""].map(String).join("\t"));
  }
  return `${lines.join("\n")}\n`;
}

export const _internals = { decodeCursor, encodeCursor, nodeReference, edgeReference, rowTokens };
