import assert from "node:assert/strict";
import test from "node:test";
import { MAX_WORKSPACE_TARGETS, selectWorkspaceTargets, mapBounded } from "./workspace-routing.mjs";

function repo(id, aliases = []) {
  return { repoId: id, repository_id: id, aliases };
}

const catalog = {
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
