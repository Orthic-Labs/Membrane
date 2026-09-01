// D43: preregistered benchmark metrics and contract check. Shared by the
// retrieval and parser benchmark harnesses.

export const BENCHMARK_METRICS = Object.freeze([
  "exact_correctness", "partial_correctness", "no_answer_abstention",
  "precision_by_edge_kind", "recall_by_edge_kind", "mrr", "ndcg",
  "false_confidence", "stale_answer", "latency_ms", "rss_bytes",
  "db_size_bytes", "mcp_bytes", "mcp_tokens", "truncation_receipts",
  "setup_success",
]);

export const REPO_CLASSES = Object.freeze(["small", "medium", "large", "very-large"]);

export function checkBenchmarkContract(spec) {
  const problems = [];
  if (!spec || !Array.isArray(spec.tasks)) problems.push("spec.tasks must be an array");
  if (!spec.metrics || !BENCHMARK_METRICS.every((m) => spec.metrics.includes(m))) {
    problems.push("spec.metrics must include all preregistered metrics");
  }
  if (!spec.repoClasses || !REPO_CLASSES.every((c) => spec.repoClasses.includes(c))) {
    problems.push("spec.repoClasses must include small/medium/large/very-large");
  }
  if (spec.tasks) {
    for (const task of spec.tasks) {
      if (!task.id || !task.repoClass || !task.kind) problems.push(`task missing id/repoClass/kind: ${JSON.stringify(task)}`);
    }
  }
  return { ok: problems.length === 0, problems };
}
