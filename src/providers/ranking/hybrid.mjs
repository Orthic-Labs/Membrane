// D44: hybrid reranker — exposes each score component separately (exact
// anchor, structural proximity, evidence confidence, claim relevance, change
// relevance, lexical score, semantic score, centrality, diversity penalty,
// budget cost). Never collapses to an unexplained final score.

export const SCORE_COMPONENTS = Object.freeze([
  "exactAnchor", "structuralProximity", "evidenceConfidence", "claimRelevance",
  "changeRelevance", "lexicalScore", "semanticScore", "centrality",
  "diversityPenalty", "budgetCost",
]);

export function hybridRank({ candidates = [], semanticCandidates = [], weights = {} } = {}) {
  const semanticById = new Map(semanticCandidates.map((c) => [c.id, c]));
  return candidates.map((candidate) => {
    const semantic = semanticById.get(candidate.id);
    const components = {
      exactAnchor: candidate.exactAnchor ?? 0,
      structuralProximity: candidate.structuralProximity ?? 0,
      evidenceConfidence: candidate.evidenceConfidence ?? candidate.confidence ?? 0,
      claimRelevance: candidate.claimRelevance ?? 0,
      changeRelevance: candidate.changeRelevance ?? 0,
      lexicalScore: candidate.lexicalScore ?? candidate.score ?? 0,
      semanticScore: semantic?.score ?? 0,
      centrality: candidate.centrality ?? 0,
      diversityPenalty: candidate.diversityPenalty ?? 0,
      budgetCost: candidate.budgetCost ?? 0,
    };
    const weighted = Object.entries(components).reduce((sum, [key, value]) => sum + (weights[key] ?? 1) * value, 0);
    return { id: candidate.id, score: weighted, components, provider: "hybrid" };
  }).sort((a, b) => b.score - a.score || a.id.localeCompare(b.id));
}
