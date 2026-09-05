import { hydrateNodesByIds } from "./store-sqlite.mjs";

function parseEvidence(value) {
  if (Array.isArray(value)) return value;
  try { return JSON.parse(value ?? "[]"); } catch { return []; }
}

function publicEvidence(node, edges) {
  const rows = [], seen = new Set();
  for (const item of [...(node?.evidence ?? []), ...edges.flatMap((edge) => edge.evidence ?? [])]) {
    if (!item) continue;
    const row = { path: item.path ?? null, startLine: item.startLine ?? null, endLine: item.endLine ?? null, contentHash: item.contentHash ?? null };
    const key = JSON.stringify(row);
    if (!seen.has(key)) { seen.add(key); rows.push(row); }
  }
  return rows;
}

export function recommendTestsForImpact(db, { generationId, impactedIds = [], maxRecommendations = 50 } = {}) {
  const targets = [...new Set((impactedIds ?? []).map(String).filter(Boolean))].slice(0, 500);
  const cap = Math.max(1, Math.min(200, Number(maxRecommendations) || 50));
  if (!generationId || !targets.length) return Object.freeze({
    schemaVersion: 1, kind: "test-recommendations", generationId: generationId ?? null,
    recommendations: [], uncoveredImpact: targets, coverage: { impacted: targets.length, covered: 0, ratio: targets.length ? 0 : null },
    omissions: targets.length ? [{ reason: "generation_missing" }] : [{ reason: "no_impacted_symbols" }], minimality: "not_proven", truncated: false,
  });
  const placeholders = targets.map(() => "?").join(",");
  const rows = db.prepare(`SELECT id, source, target, evidence, confidence_tier AS confidenceTier
    FROM edges WHERE generation_id=? AND kind='TESTS' AND target IN (${placeholders}) AND target IS NOT NULL
    ORDER BY source,target,id LIMIT 5000`).all(String(generationId), ...targets)
    .map((row) => ({ ...row, evidence: parseEvidence(row.evidence) }));
  const grouped = new Map();
  for (const edge of rows) {
    if (!grouped.has(edge.source)) grouped.set(edge.source, []);
    grouped.get(edge.source).push(edge);
  }
  const testNodes = new Map(hydrateNodesByIds(db, [...grouped.keys()]).map((node) => [node.id, node]));
  const all = [...grouped.entries()].map(([testId, edges]) => {
    const node = testNodes.get(testId);
    const coveredTargets = [...new Set(edges.map((edge) => edge.target))].sort();
    return {
      testId,
      path: node?.path ?? null,
      name: node?.qualifiedName ?? node?.name ?? testId,
      reason: "first_class_TESTS_edge_covers_impacted_symbol",
      coveredTargets,
      coverageCount: coveredTargets.length,
      evidence: publicEvidence(node, edges),
    };
  }).sort((a, b) => b.coverageCount - a.coverageCount || String(a.path ?? "").localeCompare(String(b.path ?? "")) || a.testId.localeCompare(b.testId));
  const recommendations = all.slice(0, cap);
  const covered = new Set(all.flatMap((row) => row.coveredTargets));
  const uncoveredImpact = targets.filter((id) => !covered.has(id));
  const omissions = [];
  if (!rows.length) omissions.push({ reason: "no_static_test_reachability_evidence" });
  if (all.length > recommendations.length) omissions.push({ reason: "recommendation_ceiling", count: all.length - recommendations.length });
  if (targets.length >= 500 && impactedIds.length > 500) omissions.push({ reason: "impact_target_ceiling", count: impactedIds.length - 500 });
  return Object.freeze({
    schemaVersion: 1, kind: "test-recommendations", generationId,
    recommendations, uncoveredImpact,
    coverage: { impacted: targets.length, covered: covered.size, ratio: targets.length ? covered.size / targets.length : null },
    omissions, minimality: "not_proven", truncated: all.length > recommendations.length || impactedIds.length > 500,
  });
}
