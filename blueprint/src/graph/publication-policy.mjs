// Completeness-safe publication policy for Blueprint generations.
//
// This module is deliberately storage-agnostic. The SQLite transaction already
// prevents torn writes; this policy answers the separate question: is a
// candidate generation complete enough to supersede the last known complete
// generation at all? INV-014 requires both checks.

function normalizePath(value) {
  return String(value ?? "").replaceAll("\\", "/");
}

function factPath(fact, nodesById = null) {
  const direct = normalizePath(fact?.path ?? fact?.evidence?.[0]?.path);
  if (direct) return direct;
  if (nodesById && fact?.source) return normalizePath(nodesById.get(fact.source)?.path ?? nodesById.get(fact.source)?.evidence?.[0]?.path);
  return "";
}

function candidateCompletenessProblems(generation) {
  const problems = [];
  if (!generation || !Array.isArray(generation.nodes) || !Array.isArray(generation.edges)) {
    return ["generation_shape_invalid"];
  }
  const manifest = generation.manifest ?? {};
  if (manifest.complete !== true) problems.push("manifest_not_complete");
  if (generation.truncated === true) problems.push("generation_truncated");
  if (generation.docTruth?.truncated === true) problems.push("doc_truth_truncated");
  if (manifest.repo?.traversalTruncated === true || manifest.traversalTruncated === true) problems.push("source_traversal_truncated");

  const counts = manifest.counts ?? null;
  if (counts) {
    if (Number.isFinite(counts.nodes) && Number(counts.nodes) !== generation.nodes.length) problems.push("manifest_node_count_mismatch");
    if (Number.isFinite(counts.edges) && Number(counts.edges) !== generation.edges.length) problems.push("manifest_edge_count_mismatch");
  }
  return problems;
}

function removedFactsOutsideChangedPaths(prior, candidate, changedPaths) {
  if (!prior) return [];
  const changed = new Set((changedPaths ?? []).map(normalizePath));
  if (changed.size === 0) return [];

  const priorNodesById = new Map((prior.nodes ?? []).map((node) => [node.id, node]));
  const candidateNodeIds = new Set((candidate.nodes ?? []).map((node) => node.id));
  const candidateEdgeIds = new Set((candidate.edges ?? []).map((edge) => edge.id));
  const removed = [];

  for (const node of prior.nodes ?? []) {
    if (candidateNodeIds.has(node.id)) continue;
    const path = factPath(node, priorNodesById);
    if (!changed.has(path)) removed.push({ factKind: "node", factId: node.id, path });
  }
  for (const edge of prior.edges ?? []) {
    if (candidateEdgeIds.has(edge.id)) continue;
    const path = factPath(edge, priorNodesById);
    if (!changed.has(path)) removed.push({ factKind: "edge", factId: edge.id, path });
  }
  return removed;
}

export function evaluatePublicationCandidate(generation, options = {}) {
  const problems = candidateCompletenessProblems(generation);
  const unexpectedShrink = removedFactsOutsideChangedPaths(options.priorGeneration ?? null, generation, options.changedPaths ?? []);
  if (unexpectedShrink.length) problems.push("unexpected_unrelated_fact_shrink");
  return Object.freeze({
    schemaVersion: 1,
    kind: "BlueprintPublicationDecision",
    action: problems.length ? "block" : "allow",
    reasonCode: problems.length ? problems[0] : "complete_generation",
    generationId: generation?.manifest?.generationId ?? null,
    problems: Object.freeze(problems),
    unexpectedShrink: Object.freeze(unexpectedShrink),
  });
}

export function assertPublicationCandidate(generation, options = {}) {
  const decision = evaluatePublicationCandidate(generation, options);
  if (decision.action === "block") {
    const error = new Error(`blueprint publication blocked: ${decision.problems.join(", ")}`);
    error.code = "publication_incomplete";
    error.decision = decision;
    throw error;
  }
  return decision;
}
