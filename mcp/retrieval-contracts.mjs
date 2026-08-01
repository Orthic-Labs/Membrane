import { createHash } from "node:crypto";

const digest = (value) => `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;
const sortById = (left, right) => String(left.id || left.node_id || left.candidate_id).localeCompare(String(right.id || right.node_id || right.candidate_id));

function bounded(value, bounds) {
  return value.filter((entry) => (entry.depth ?? 0) <= bounds.depth).slice(0, bounds.nodes);
}

export function buildNeighborhood({ providerId, repositoryId, sourceGenerationId, seeds = [], nodes = [], edges = [], evidencePaths = [], sourceResolutions = [], bounds = {} }) {
  const limit = { depth: bounds.depth ?? 2, nodes: bounds.nodes ?? 64, edges: bounds.edges ?? 128, elapsed_ms: bounds.elapsed_ms ?? 250, estimated_tokens: bounds.estimated_tokens ?? 4096 };
  const seedIds = new Set(seeds.map((seed) => seed.id || seed.node_id));
  const selected = bounded([...nodes].sort(sortById), limit);
  const selectedIds = new Set(selected.map((node) => node.id || node.node_id));
  const typedEdges = edges.filter((edge) => selectedIds.has(edge.from) && selectedIds.has(edge.to)).sort((a, b) => `${a.from}:${a.to}:${a.type}`.localeCompare(`${b.from}:${b.to}:${b.type}`)).slice(0, limit.edges);
  const reachable = new Set(seedIds);
  let changed = true;
  while (changed) {
    changed = false;
    for (const edge of typedEdges) if (reachable.has(edge.from) && !reachable.has(edge.to)) { reachable.add(edge.to); changed = true; }
  }
  const omissions = selected.filter((node) => !reachable.has(node.id || node.node_id)).map((node) => ({ node_id: node.id || node.node_id, reason: "no_seed_path", recovery: "add_anchor_or_allow_relation" }));
  return {
    schema: "orthic.context-neighborhood.v1", provider_id: providerId, repository_id: repositoryId,
    source_generation_id: sourceGenerationId, seed_nodes: [...seeds], nodes: selected.filter((node) => reachable.has(node.id || node.node_id)),
    typed_edges: typedEdges.filter((edge) => reachable.has(edge.from) && reachable.has(edge.to)), evidence_paths: [...evidencePaths].sort(),
    source_resolutions: [...sourceResolutions].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))), bounds: limit, omissions,
  };
}

export function fixedFusion(candidates, { providerQuotas = {}, rrfK = 60, maxItems = 32, policy = "membrane-fusion-fixed-v1" } = {}) {
  const eligible = candidates.filter((candidate) => candidate.eligible !== false && candidate.scope_ok !== false && candidate.fresh !== false);
  const byProvider = new Map();
  for (const candidate of eligible) {
    const list = byProvider.get(candidate.provider_id) || [];
    list.push(candidate);
    byProvider.set(candidate.provider_id, list);
  }
  const ranked = [];
  for (const [provider, list] of byProvider) {
    const quota = providerQuotas[provider] ?? list.length;
    [...list].sort((a, b) => (a.rank ?? 0) - (b.rank ?? 0) || sortById(a, b)).slice(0, quota).forEach((candidate, index) => {
      ranked.push({ ...candidate, rrf_score: 1 / (rrfK + index + 1), diagnostics: { provider, rank: index + 1, raw_score: candidate.score ?? null } });
    });
  }
  const selected = [];
  const seen = new Set();
  for (const candidate of ranked.sort((a, b) => b.rrf_score - a.rrf_score || sortById(a, b))) {
    const key = candidate.redundancy_key || candidate.id || candidate.candidate_id;
    if (seen.has(key) && candidate.protected !== true) continue;
    seen.add(key);
    selected.push(candidate);
    if (selected.length >= maxItems) break;
  }
  return { policy, fallback_policy: "fixed-lanes-v1", candidates: selected, diagnostics: { eligible: eligible.length, selected: selected.length, raw_scores_are_not_probabilities: true } };
}

export function providerReadiness({ providerId, modelId, configDigest, capabilities = {}, supported = true, reason = null, observedAt = new Date().toISOString() }) {
  return { schema: "orthic.provider-readiness.v1", provider_id: providerId, status: supported ? "ready" : "degraded", observed_at: observedAt, details: { model_id: modelId, config_digest: configDigest, capabilities, ...(reason ? { reason } : {}) } };
}

export function conformanceCheck(provider, fixture = {}) {
  const errors = [];
  if (!provider?.provider_id || !provider?.details?.model_id || !provider?.details?.config_digest) errors.push("identity_missing");
  if (provider?.status === "ready" && provider?.details?.capabilities?.live_resolution && fixture.live_resolution !== true) errors.push("live_resolution_unexercised");
  if (fixture.scope_leak === true) errors.push("scope_leak");
  if (fixture.nondeterministic === true) errors.push("nondeterministic_output");
  return { provider_id: provider?.provider_id || "unknown", pass: errors.length === 0, errors };
}

function scoreEvaluation(cases, policy) {
  let expected = 0, recovered = 0, utility = 0, selected = 0, scopeRegressions = 0, freshnessRegressions = 0;
  for (const item of cases) {
    const result = policy(item.candidates || [], item.options || {});
    const ids = new Set(result.candidates.map((candidate) => candidate.id || candidate.candidate_id));
    const wanted = new Set(item.expected_ids || []);
    expected += wanted.size;
    recovered += [...wanted].filter((id) => ids.has(id)).length;
    selected += result.candidates.length;
    utility += result.candidates.reduce((sum, candidate) => sum + (Number(candidate.utility) || 0), 0);
    scopeRegressions += result.candidates.filter((candidate) => candidate.scope_ok === false).length;
    freshnessRegressions += result.candidates.filter((candidate) => candidate.fresh === false).length;
  }
  return { evidence_completeness: expected ? recovered / expected : 1, utility_per_token: selected ? utility / selected : 0, selected, scope_regressions: scopeRegressions, freshness_regressions: freshnessRegressions };
}

export function calibrationHarness({ training = [], heldout = [], candidatePolicy = fixedFusion, baselinePolicy = fixedFusion } = {}) {
  const baseline = scoreEvaluation(heldout, baselinePolicy);
  const candidate = scoreEvaluation(heldout, candidatePolicy);
  const improvement = candidate.evidence_completeness > baseline.evidence_completeness || candidate.utility_per_token > baseline.utility_per_token;
  const noRegression = candidate.scope_regressions === 0 && candidate.freshness_regressions === 0;
  return {
    schema: "orthic.retrieval-calibration.v1",
    policy: "offline-heldout-only",
    training_cases: training.length,
    heldout_cases: heldout.length,
    baseline,
    candidate,
    deployable: improvement && noRegression,
    fallback: "membrane-fusion-fixed-v1",
    diagnostics: { raw_scores_are_not_probabilities: true, ranking_mutation: "disabled_until_heldout_validation" },
  };
}
