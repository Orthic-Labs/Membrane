// mcp/workspace-routing.mjs — MBR-004 / SN-NODE-03 bounded workspace target
// selection and fan-out. A no-match task abstains instead of querying every
// repository; learned/catalog relevance belongs to MBR-408 (Wave 0 uses
// explicit aliases plus abstention only).

export const MAX_WORKSPACE_TARGETS = 4;

export function selectWorkspaceTargets({ catalog, task, explicitRepositoryIds = [] }) {
  const repos = [...(catalog.repositories || [])];
  const explicit = new Set(explicitRepositoryIds);
  const text = String(task || "").toLowerCase();
  const scored = repos.map((repo) => {
    const aliases = [repo.repoId, repo.repository_id, ...(repo.aliases || [])].filter(Boolean);
    const explicitMatch = aliases.some((alias) => explicit.has(alias));
    const mentionMatch = aliases.some((alias) => text.includes(String(alias).toLowerCase()));
    return { repo, score: explicitMatch ? 100 : mentionMatch ? 50 : 0 };
  }).filter((row) => row.score > 0)
    .sort((a, b) => b.score - a.score || String(a.repo.repoId).localeCompare(String(b.repo.repoId)));

  if (scored.length === 0) {
    return { status: "abstained", reason: "target_selection_abstained", targets: [], considered: repos.map((r) => r.repoId) };
  }
  return {
    status: "selected",
    targets: scored.slice(0, MAX_WORKSPACE_TARGETS).map((row) => row.repo),
    omitted: scored.slice(MAX_WORKSPACE_TARGETS).map((row) => ({ repoId: row.repo.repoId, reason: "target_limit" })),
  };
}

export async function mapBounded(items, concurrency, fn, signal) {
  const output = new Array(items.length);
  let cursor = 0;
  async function worker() {
    while (true) {
      if (signal?.aborted) throw signal.reason || new Error("cancelled");
      const index = cursor++;
      if (index >= items.length) return;
      output[index] = await fn(items[index], index);
    }
  }
  await Promise.all(Array.from({ length: Math.min(concurrency, items.length) }, worker));
  return output;
}
