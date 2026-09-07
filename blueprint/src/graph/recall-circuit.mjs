import { createHash } from "node:crypto";
import { hydrateEdgesByIds, hydrateNodesByIds, traversalNeighbors } from "./store-sqlite.mjs";
import { semanticAuthorityForFact, semanticAuthorityRankForFact } from "./evidence-authority.mjs";
import { resolveSeeds } from "./seed-resolver.mjs";
import { selectTraversalPolicy } from "./traversal-policy.mjs";

const TIER_RANK = Object.freeze({ EXACT_RESOLUTION: 0, SAME_FILE_LEXICAL: 1, CROSS_FILE_HEURISTIC: 2, UNRESOLVED: 3 });
const tierRank = (tier) => TIER_RANK[tier] ?? 4;
const evidenceFor = (value) => Array.isArray(value?.evidence) ? value.evidence.filter(Boolean) : [];
const stableDigest = (value) => `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;

function adjacency(edgeRows, direction) {
  const result = new Map();
  const add = (from, to, edge) => {
    if (!from || !to) return;
    if (!result.has(from)) result.set(from, []);
    result.get(from).push({ to, edge });
  };
  for (const edge of edgeRows) {
    if (direction !== "in") add(edge.source, edge.target, edge);
    if (direction !== "out") add(edge.target, edge.source, edge);
  }
  // Resolution specificity is structural traversal order only. It is not a
  // scalar confidence competition between producers.
  for (const entries of result.values()) entries.sort((a, b) => tierRank(a.edge.confidence_tier) - tierRank(b.edge.confidence_tier) || a.edge.id.localeCompare(b.edge.id));
  return result;
}

function makePath(seed, nodeIds, edgeIds, nodeMap, edgeMap, complete, generationId) {
  const nodes = nodeIds.map((id) => nodeMap.get(id)).filter(Boolean);
  const edges = edgeIds.map((id) => edgeMap.get(id)).filter(Boolean);
  const evidence = [...nodes.flatMap(evidenceFor), ...edges.flatMap(evidenceFor)];
  const edgeTiers = edges.map((edge) => edge.confidenceTier).filter(Boolean);
  const authorityRanks = edges.map((edge) => semanticAuthorityRankForFact(edge));
  const weakestAuthorityRank = authorityRanks.length ? Math.max(...authorityRanks) : 0;
  const weakestAuthorityEdge = edges.find((edge) => semanticAuthorityRankForFact(edge) === weakestAuthorityRank) ?? null;
  const minimumSemanticAuthority = weakestAuthorityEdge ? semanticAuthorityForFact(weakestAuthorityEdge) : null;
  const projection = { seed: seed.id, terminal: nodeIds.at(-1), nodeIds, edgeIds };
  const id = stableDigest(projection);
  const omissionReasons = complete ? [] : ["bound_reached"];
  const evidenceEnvelope = Object.freeze({
    schemaVersion: 1,
    kind: "AtomicEvidencePath",
    id,
    generationId,
    completeness: complete ? "exact" : "lower_bound",
    nodeEvidence: nodes.map((node) => ({ nodeId: node.id, evidence: evidenceFor(node) })),
    edgeEvidence: edges.map((edge) => ({ edgeId: edge.id, source: edge.source, target: edge.target, evidence: evidenceFor(edge) })),
    omissions: omissionReasons.map((reason) => ({ reason })),
  });
  return {
    id,
    seedId: seed.id,
    terminalId: nodeIds.at(-1),
    nodes,
    edges,
    evidence,
    minimumEdgeTier: edgeTiers.sort((a, b) => tierRank(b) - tierRank(a))[0] ?? "EXACT_RESOLUTION",
    minimumSemanticAuthority,
    semanticAuthorityRank: weakestAuthorityRank,
    seedExactness: seed.exactness,
    evidenceCoverage: (nodes.length + edges.length) ? evidence.length / (nodes.length + edges.length) : 0,
    hopCount: edgeIds.length,
    state: complete ? "complete" : "partial",
    omissionReasons,
    evidenceEnvelope,
  };
}

/**
 * The non-compensatory Recall ordering (BPT-026). Exported so the contract can
 * be asserted where it is actually decided: the candidate-set conversion below
 * deliberately does not sort, so asserting order there proves nothing about
 * this comparator.
 */
export function comparePaths(left, right) {
  // The application freshness barrier has already established one served
  // generation for this circuit. Within that source-coherent generation,
  // semantic authority precedes resolution specificity. Scalar confidence is
  // deliberately absent: it cannot compensate for a weaker evidence class.
  return (left.state === "complete" ? 0 : 1) - (right.state === "complete" ? 0 : 1)
    || left.semanticAuthorityRank - right.semanticAuthorityRank
    || tierRank(left.minimumEdgeTier) - tierRank(right.minimumEdgeTier)
    || left.seedExactness - right.seedExactness
    || right.evidenceCoverage - left.evidenceCoverage
    || left.hopCount - right.hopCount
    || left.id.localeCompare(right.id);
}

export function executeRecallCircuit(db, task, options = {}) {
  const generationId = String(options.generationId ?? "");
  const policy = selectTraversalPolicy(task, options.policy, options.limits);
  const resolution = resolveSeeds(db, task, {
    generationId,
    seedIds: options.seedIds,
    anchors: options.anchors,
    maxSeeds: policy.maxSeeds,
    allowAmbiguousTaskSeeds: true,
  });
  if (!resolution.seeds.length) {
    const visible = { generationId, task: String(task), policy: policy.family, paths: [], omissions: [{ reason: resolution.reason }] };
    return { schemaVersion: 1, kind: "RecallCircuit", id: stableDigest(visible), ...visible, seeds: [], resolution, state: resolution.state === "ambiguous" ? "ambiguous" : "abstained", bounds: policy };
  }

  const frontier = traversalNeighbors(db, {
    seedIds: resolution.seeds.map((seed) => seed.id),
    generationId,
    direction: policy.direction,
    maxDepth: policy.maxHops,
    kinds: policy.kinds,
  });
  const allowedNodes = new Set(frontier.seenNodes.slice(0, policy.maxNodes));
  const edgeRows = frontier.edgeRows.filter((edge) => allowedNodes.has(edge.source) && allowedNodes.has(edge.target)).slice(0, policy.maxEdges);
  const nodeMap = new Map(hydrateNodesByIds(db, [...allowedNodes]).map((node) => [node.id, node]));
  const edgeMap = new Map(hydrateEdgesByIds(db, edgeRows.map((edge) => edge.id)).map((edge) => [edge.id, edge]));
  const graph = adjacency(edgeRows, policy.direction);
  const paths = [];
  const seenPathIds = new Set();
  for (const seed of resolution.seeds) {
    const queue = [{ nodeId: seed.id, nodeIds: [seed.id], edgeIds: [], visited: new Set([seed.id]) }];
    while (queue.length && paths.length < policy.maxPaths) {
      const current = queue.shift();
      const next = (graph.get(current.nodeId) ?? []).filter((item) => !current.visited.has(item.to));
      const isTerminal = current.edgeIds.length > 0 && (next.length === 0 || current.edgeIds.length >= policy.maxHops);
      if (isTerminal) {
        const path = makePath(seed, current.nodeIds, current.edgeIds, nodeMap, edgeMap, next.length === 0, generationId);
        if ((!policy.evidenceRequired || path.evidenceCoverage > 0) && !seenPathIds.has(path.id)) {
          seenPathIds.add(path.id);
          paths.push(path);
        }
      }
      if (current.edgeIds.length >= policy.maxHops) continue;
      for (const item of next) {
        queue.push({
          nodeId: item.to,
          nodeIds: [...current.nodeIds, item.to],
          edgeIds: [...current.edgeIds, item.edge.id],
          visited: new Set([...current.visited, item.to]),
        });
      }
    }
  }
  if (!paths.length) {
    for (const seed of resolution.seeds) paths.push(makePath(seed, [seed.id], [], nodeMap, edgeMap, true, generationId));
  }
  paths.sort(comparePaths);
  const omissions = [];
  if (frontier.seenNodes.length > policy.maxNodes) omissions.push({ reason: "node_ceiling", count: frontier.seenNodes.length - policy.maxNodes });
  if (frontier.edgeRows.length > policy.maxEdges) omissions.push({ reason: "edge_ceiling", count: frontier.edgeRows.length - policy.maxEdges });
  if (paths.length >= policy.maxPaths) omissions.push({ reason: "path_ceiling" });
  const visible = { generationId, task: String(task), policy: policy.family, paths, omissions };
  return {
    schemaVersion: 1,
    kind: "RecallCircuit",
    id: stableDigest({ generationId, policy: policy.family, paths: paths.map((path) => path.id), omissions }),
    ...visible,
    seeds: resolution.seeds.map((seed) => ({ id: seed.id, exactness: seed.exactness, reason: seed.reason, evidence: seed.evidence })),
    resolution,
    state: "complete",
    bounds: policy,
    accounting: { visited: frontier.seenNodes.length, hydrated: nodeMap.size, returnedPaths: paths.length },
  };
}

export function canonicalCandidateSet(candidateSet) {
  return {
    schemaVersion: candidateSet.schemaVersion,
    traceId: candidateSet.traceId,
    indexedAt: candidateSet.indexedAt,
    task: candidateSet.task,
    mode: candidateSet.mode,
    provider: candidateSet.provider,
    freshness: candidateSet.freshness,
    providerCeiling: candidateSet.providerCeiling,
    candidates: (candidateSet.candidates ?? []).map((candidate) => ({
      id: candidate.id,
      layer: candidate.layer,
      ...(candidate.provider === undefined ? {} : { provider: candidate.provider }),
      sourceKind: candidate.sourceKind,
      sourceRef: candidate.sourceRef,
      sourceHash: candidate.sourceHash,
      trustClass: candidate.trustClass,
      instructionPolicy: candidate.instructionPolicy,
      providerScore: candidate.providerScore,
      ...(candidate.scoreComponents === undefined ? {} : { scoreComponents: candidate.scoreComponents }),
      ...(candidate.baseCommit === undefined ? {} : { baseCommit: candidate.baseCommit }),
      ...(candidate.overlayDigest === undefined ? {} : { overlayDigest: candidate.overlayDigest }),
      ...(candidate.freshnessClass === undefined ? {} : { freshnessClass: candidate.freshnessClass }),
      ...(candidate.snapshotId === undefined ? {} : { snapshotId: candidate.snapshotId }),
      estimatedTokens: candidate.estimatedTokens,
      protected: candidate.protected,
      exact: candidate.exact,
      recoverable: candidate.recoverable,
      resolver: candidate.resolver,
      text: candidate.text,
      // V1 is deliberately strict. Rich path provenance belongs to the
      // containing RecallCircuit response, not undeclared CandidateV1 keys.
    })),
    omissions: (candidateSet.omissions ?? []).map((omission, index) => ({
      id: omission.id ?? `${candidateSet.traceId}:omission:${index}`,
      ...(omission.layer === undefined ? {} : { layer: omission.layer }),
      reason: omission.reason,
    })),
  };
}

export function recallCircuitToCandidateSet(circuit, options = {}) {
  const indexedAt = options.indexedAt ?? circuit.indexedAt ?? new Date().toISOString();
  const candidates = [];
  const seen = new Set();
  for (const path of circuit.paths ?? []) {
    const terminal = path.nodes.at(-1);
    const evidence = evidenceFor(terminal)[0] ?? path.evidence[0];
    if (!terminal || !evidence || seen.has(terminal.id)) continue;
    seen.add(terminal.id);
    const estimatedTokens = Math.max(1, Math.ceil((Number(evidence.endLine ?? evidence.startLine ?? 1) - Number(evidence.startLine ?? 1) + 1) * 12));
    candidates.push({
      id: terminal.id,
      layer: 3,
      sourceKind: "repo_code",
      sourceRef: `${evidence.path}:${evidence.startLine ?? 1}-${evidence.endLine ?? evidence.startLine ?? 1}`,
      sourceHash: `xxh128:${evidence.contentHash ?? "0".repeat(32)}`,
      trustClass: "workspace_tracked",
      instructionPolicy: "data_only",
      // BPT-026: `providerScore` and `scoreComponents` are PRESENTATIONAL
      // only. Ordering is already final by the time this loop runs — it was
      // decided upstream by `comparePaths`' non-compensatory lexicographic
      // chain (state, semanticAuthorityRank, edge tier, seedExactness,
      // evidenceCoverage, hopCount, id), which never sums or weighs those
      // fields against each other. These emitted numbers exist only so a V1
      // consumer has a human-readable score to display; they must never be
      // summed, weighted, or otherwise fed back into re-ranking — doing so
      // can silently recover a compensatory ordering the comparator
      // deliberately forbids (see tests/recall-candidate-contract.test.mjs).
      providerScore: 1 / (candidates.length + 1),
      scoreComponents: {
        semanticAuthority: 1 / ((path.semanticAuthorityRank ?? 0) + 1),
        evidenceTier: 1 / (tierRank(path.minimumEdgeTier) + 1),
        pathCompleteness: path.state === "complete" ? 1 : 0,
        evidenceCoverage: path.evidenceCoverage,
      },
      estimatedTokens,
      actualTokens: null,
      truncation: null,
      protected: path.seedId === terminal.id,
      exact: path.seedExactness <= 2,
      recoverable: true,
      resolver: `blueprint graph resolve --node ${terminal.id}`,
      text: terminal.qualifiedName ?? terminal.name ?? terminal.id,
      recallCircuitId: circuit.id,
      evidencePathId: path.id,
      evidenceEnvelope: path.evidenceEnvelope,
    });
  }
  const candidateSet = {
    schemaVersion: 1,
    traceId: options.traceId ?? circuit.id,
    indexedAt,
    repoId: options.repoId ?? null,
    repoRoot: options.repoRoot ?? null,
    receiptId: options.receiptId ?? null,
    task: circuit.task,
    mode: options.mode ?? "survey",
    provider: typeof options.provider === "string" ? options.provider : options.provider?.id ?? "blueprint-static",
    freshness: options.freshness ?? { revision: circuit.generationId, indexedAt, stale: false },
    providerCeiling: { maxCandidates: circuit.bounds?.maxPaths ?? candidates.length, maxEstimatedTokens: options.maxEstimatedTokens ?? 8000 },
    candidates,
    omissions: circuit.omissions ?? [],
    recallCircuit: { id: circuit.id, state: circuit.state, policy: circuit.policy },
  };
  return options.canonical ? canonicalCandidateSet(candidateSet) : candidateSet;
}
