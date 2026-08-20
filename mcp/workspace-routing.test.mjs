import assert from "node:assert/strict";
import test from "node:test";
import { MAX_WORKSPACE_TARGETS, selectWorkspaceTargets, mapBounded } from "./workspace-routing.mjs";

function repo(id, aliases = []) {
  return { repoId: id, aliases };
}

const catalog = {
  catalog_digest: "sha256:catalog",
  repositories: [
    repo("srv-api", ["api", "srv-api"]),
    repo("srv-billing", ["billing", "srv-billing"]),
    repo("srv-cache", ["cache", "srv-cache"]),
    repo("srv-promo", ["promo", "srv-promo"]),
    repo("srv-meta", ["meta", "srv-meta"]),
    repo("srv-cron", ["cron", "srv-cron"]),
  ],
};

test("selectWorkspaceTargets selects an exact explicit repository id", () => {
  const result = selectWorkspaceTargets({ catalog, task: "unrelated task", explicitRepositoryIds: ["srv-billing"] });
  assert.equal(result.status, "selected");
  assert.deepEqual(result.targets.map((r) => r.repoId), ["srv-billing"]);
});

test("selectWorkspaceTargets selects repos mentioned by alias in the task", () => {
  const result = selectWorkspaceTargets({ catalog, task: "investigate the api and billing latency" });
  assert.equal(result.status, "selected");
  assert.deepEqual(result.targets.map((r) => r.repoId).sort(), ["srv-api", "srv-billing"]);
});

test("explicit ids dominate whole-token aliases and substring collisions abstain", () => {
  const result = selectWorkspaceTargets({ catalog, task: "billing incident", explicitRepositoryIds: ["srv-cache"] });
  assert.deepEqual(result.targets.map((r) => r.repoId), ["srv-cache", "srv-billing"]);
  assert.deepEqual(result.receipt.selected.map((row) => row.score), [[1, 0, 0], [0, 1, 0]]);
  const collision = selectWorkspaceTargets({ catalog, task: "investigate apiary capacity" });
  assert.equal(collision.status, "abstained");
});

test("catalog-owned relevance terms route without exposing task content", () => {
  const relevant = { ...catalog, repositories: catalog.repositories.map((entry) =>
    entry.repoId === "srv-cache" ? { ...entry, relevanceTerms: ["redis eviction", "cache pressure"] } : entry) };
  const result = selectWorkspaceTargets({ catalog: relevant, task: "debug redis eviction immediately" });
  assert.deepEqual(result.targets.map((r) => r.repoId), ["srv-cache"]);
  assert.deepEqual(result.receipt.selected[0], { repoId: "srv-cache", score: [0, 0, 1], evidence: ["catalog_relevance"] });
  assert.equal(result.receipt.catalogDigest, catalog.catalog_digest);
  assert.equal(JSON.stringify(result.receipt).includes("redis"), false);
});

test("routing is permutation-stable and returns exact catalog objects", () => {
  const left = selectWorkspaceTargets({ catalog, task: "api billing" });
  const reversed = { ...catalog, repositories: [...catalog.repositories].reverse() };
  const right = selectWorkspaceTargets({ catalog: reversed, task: "api billing" });
  assert.deepEqual(left.receipt, right.receipt);
  assert.deepEqual(left.targets.map((repo) => repo.repoId), ["srv-api", "srv-billing"]);
  assert.equal(left.targets[0], catalog.repositories[0]);
});

test("selectWorkspaceTargets caps the fan-out at MAX_WORKSPACE_TARGETS and reports the omitted limit", () => {
  const matching = { ...catalog, repositories: catalog.repositories.map((r) => ({ ...r, aliases: ["match"] })) };
  const result = selectWorkspaceTargets({ catalog: matching, task: "match" });
  assert.equal(result.status, "selected");
  assert.equal(result.targets.length, MAX_WORKSPACE_TARGETS);
  assert.equal(result.omitted.length, matching.repositories.length - MAX_WORKSPACE_TARGETS);
  assert.ok(result.omitted.every((row) => row.reason === "target_limit"));
});

test("MBR-004: a task with no explicit or mentioned target abstains instead of querying every repository", () => {
  const result = selectWorkspaceTargets({ catalog, task: "do the dishes", explicitRepositoryIds: [] });
  assert.equal(result.status, "abstained");
  assert.equal(result.reason, "target_selection_abstained");
  assert.deepEqual(result.targets, []);
  assert.equal(result.considered.length, catalog.repositories.length);
});

test("unknown and malformed explicit ids are typed omissions", () => {
  const malformed = { ...catalog, repositories: [...catalog.repositories, repo("srv-api")] };
  const result = selectWorkspaceTargets({ catalog: malformed, task: "none", explicitRepositoryIds: ["missing", "", 7, "srv-api"] });
  assert.equal(result.status, "abstained");
  assert.deepEqual(result.omitted.map((row) => row.reason), [
    "unknown_explicit_repository", "invalid_explicit_repository_id",
    "invalid_explicit_repository_id", "unknown_explicit_repository",
  ]);
  assert.equal(result.considered.includes("srv-api"), false);
  assert.equal(selectWorkspaceTargets({ catalog, task: "none", explicitRepositoryIds: Array(257).fill("missing") }).omitted[0].reason, "explicit_repository_limit");
});

test("mapBounded runs concurrently up to the bound and preserves input order", async () => {
  const inputs = [10, 20, 30, 40, 50];
  const signal = new AbortController().signal;
  const out = await mapBounded(inputs, 2, async (value) => value * 2, signal);
  assert.deepEqual(out, [20, 40, 60, 80, 100]);
});

test("mapBounded aborts and throws on cancellation", async () => {
  const controller = new AbortController();
  controller.abort(new Error("cancelled"));
  await assert.rejects(
    mapBounded([1, 2, 3], 2, async (value) => value, controller.signal),
    /cancelled/,
  );
});
