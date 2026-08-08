// Bounded, catalog-authoritative workspace relevance routing.

export const MAX_WORKSPACE_TARGETS = 4;
const MAX_TASK_CHARS = 8192;
const MAX_TASK_TOKENS = 256;
const MAX_SIGNALS = 64;
const MAX_REPOSITORIES = 4096;
const MAX_EXPLICIT_IDS = 256;

const bytewise = (left, right) => left < right ? -1 : left > right ? 1 : 0;
const words = (value, maxLength = MAX_TASK_CHARS) => {
  if (typeof value !== "string" || value.length === 0 || value.length > maxLength) return [];
  return (value.normalize("NFKC").toLowerCase().match(/[\p{L}\p{N}]+/gu) || []).slice(0, MAX_TASK_TOKENS);
};
const containsPhrase = (haystack, needle) => needle.length > 0 && haystack.some((_, index) =>
  needle.every((token, offset) => haystack[index + offset] === token));
const signals = (values, maxLength) => [...new Set((Array.isArray(values) ? values : [])
  .filter((value) => typeof value === "string" && value.length > 0 && value.length <= maxLength))]
  .sort(bytewise).slice(0, MAX_SIGNALS);
const repoId = (repo) => typeof repo?.repoId === "string" && repo.repoId.length > 0 && repo.repoId.length <= 256
  ? repo.repoId : typeof repo?.repository_id === "string" && repo.repository_id.length > 0 && repo.repository_id.length <= 256
    ? repo.repository_id : null;
const scoreRepo = (repo, taskTokens, explicit) => {
  const id = repoId(repo);
  const aliases = signals([id, repo.repository_id, ...(Array.isArray(repo.aliases) ? repo.aliases : [])], 256);
  const relevanceTerms = signals(repo.relevanceTerms, 80);
  const aliasHits = aliases.filter((alias) => containsPhrase(taskTokens, words(alias, 256))).length;
  const relevanceHits = relevanceTerms.filter((term) => containsPhrase(taskTokens, words(term, 80))).length;
  const tuple = [explicit.has(id) ? 1 : 0, aliasHits, relevanceHits];
  return { repo, repoId: id, score: tuple, evidence: [tuple[0] && "explicit_id", aliasHits && "alias", relevanceHits && "catalog_relevance"].filter(Boolean) };
};

export function selectWorkspaceTargets({ catalog, task, explicitRepositoryIds = [] }) {
  const rawRepos = Array.isArray(catalog?.repositories) ? catalog.repositories : [];
  const taskTokens = words(task);
  if (rawRepos.length > MAX_REPOSITORIES) {
    const receipt = { policy: "catalog-relevance-v1", catalogDigest: typeof catalog?.catalog_digest === "string" ? catalog.catalog_digest : null,
      taskTokenCount: taskTokens.length, selected: [], omitted: [{ reason: "catalog_limit_exceeded" }] };
    return { status: "abstained", reason: "catalog_limit_exceeded", targets: [], considered: [], omitted: receipt.omitted, receipt };
  }
  const counts = new Map();
  for (const repo of rawRepos) if (repoId(repo)) counts.set(repoId(repo), (counts.get(repoId(repo)) || 0) + 1);
  const repos = rawRepos.filter((repo) => repoId(repo) && counts.get(repoId(repo)) === 1);
  const allExplicitValues = Array.isArray(explicitRepositoryIds) ? explicitRepositoryIds : [];
  const explicitValues = allExplicitValues.slice(0, MAX_EXPLICIT_IDS);
  const explicit = new Set(explicitValues.filter((id) => typeof id === "string" && id.length > 0 && id.length <= 256));
  const known = new Set(repos.map(repoId));
  const explicitOmissions = [
    ...(allExplicitValues.length > MAX_EXPLICIT_IDS ? [{ reason: "explicit_repository_limit", count: allExplicitValues.length - MAX_EXPLICIT_IDS }] : []),
    ...explicitValues.flatMap((id, requestIndex) => {
    if (typeof id !== "string" || id.length === 0 || id.length > 256) return [{ requestIndex, reason: "invalid_explicit_repository_id" }];
    return known.has(id) ? [] : [{ requestIndex, reason: "unknown_explicit_repository" }];
    }),
  ];
  const scored = repos.map((repo) => scoreRepo(repo, taskTokens, explicit))
    .filter((row) => row.score.some((value) => value > 0))
    .sort((left, right) => right.score[0] - left.score[0]
      || right.score[1] - left.score[1] || right.score[2] - left.score[2]
      || bytewise(left.repoId, right.repoId));
  const selected = scored.slice(0, MAX_WORKSPACE_TARGETS);
  const limited = scored.slice(MAX_WORKSPACE_TARGETS).map((row) => ({ repoId: row.repoId, reason: "target_limit" }));
  const receipt = {
    policy: "catalog-relevance-v1",
    catalogDigest: typeof catalog?.catalog_digest === "string" ? catalog.catalog_digest : null,
    taskTokenCount: taskTokens.length,
    selected: selected.map(({ repoId, score, evidence }) => ({ repoId, score, evidence })),
    omitted: [...explicitOmissions, ...limited],
  };

  if (scored.length === 0) {
    return { status: "abstained", reason: "target_selection_abstained", targets: [], considered: [...known].sort(bytewise), omitted: explicitOmissions, receipt };
  }
  return {
    status: "selected",
    targets: selected.map((row) => row.repo),
    omitted: [...explicitOmissions, ...limited],
    receipt,
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
