import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BLUEPRINT = path.resolve(HERE, "..");
const CLI = path.join(BLUEPRINT, "scripts/blueprint.mjs");
const FIXTURE = path.join(BLUEPRINT, "evals/fixture-repos/typescript-commerce");

function copyFixture() {
  const dir = path.join(os.tmpdir(), `blueprint-graph-query-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}`);
  fs.cpSync(FIXTURE, dir, { recursive: true });
  return dir;
}

function run(args, cwd) {
  const result = spawnSync(process.execPath, [CLI, ...args], { cwd, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  return JSON.parse(result.stdout);
}

test("graph query primitives return typed, evidence-backed JSON", () => {
  const repo = copyFixture();
  try {
    spawnSync(process.execPath, [CLI, "graph", "build", "--out", ".agent"], { cwd: repo, encoding: "utf8" });

    const resolve = run(["graph", "resolve", "--node", "symbol:src/service.ts::OrderService.placeOrder", "--out", ".agent"], repo);
    assert.equal(resolve.node.evidence[0].path, "src/service.ts");
    assert.equal(resolve.node.evidence[0].startLine, 6);

    const neighbors = run(["graph", "neighbors", "--node", "symbol:src/service.ts::OrderService.placeOrder", "--direction", "both", "--depth", "1", "--json", "--out", ".agent"], repo);
    assert.ok(neighbors.edges.some((edge) => edge.kind === "CALLS" && edge.target === "symbol:src/service.ts::OrderService.placeOrder"));
    assert.ok(neighbors.edges.some((edge) => edge.kind === "CALLS" && edge.source === "symbol:src/service.ts::OrderService.placeOrder"));

    const pathResult = run(["graph", "path", "--from", "symbol:src/routes.ts::registerOrderRoute", "--to", "symbol:src/store.ts::OrderStore.save", "--json", "--out", ".agent"], repo);
    assert.deepEqual(pathResult.path.map((step) => step.id), [
      "symbol:src/routes.ts::registerOrderRoute",
      "symbol:src/service.ts::OrderService.placeOrder",
      "symbol:src/store.ts::OrderStore.save",
    ]);

    const architecture = run(["graph", "architecture", "--json", "--out", ".agent"], repo);
    assert.ok(architecture.counts.symbols >= 5);
    assert.ok(architecture.examples.some((node) => node.id.includes("registerOrderRoute")));

    const impact = run(["graph", "impact", "--node", "symbol:src/store.ts::OrderStore.save", "--json", "--out", ".agent"], repo);
    assert.ok(impact.impacted.some((node) => node.id === "symbol:src/service.ts::OrderService.placeOrder"));

    const candidates = run(["graph", "candidates", "--query", "placeOrder", "--out", ".agent", "--limit", "2"], repo);
    assert.equal(candidates.provider, "blueprint-static");
    assert.equal(candidates.candidates[0].sourceKind, "repo_code");

    const build = spawnSync(process.execPath, [CLI, "build", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
    assert.equal(build.status, 0, build.stderr || build.stdout);
    const docTruth = run(["graph", "doc-truth", "--out", ".agent"], repo);
    assert.equal(docTruth.provider, "blueprint-treesitter");
    assert.ok(Array.isArray(docTruth.joins));
    assert.ok(docTruth.sourceDocMap.docs >= 1);

    const mermaid = spawnSync(process.execPath, [CLI, "graph", "mermaid", "--view", "neighbors", "--node", "symbol:src/service.ts::OrderService.placeOrder", "--out", ".agent", "--limit", "8"], { cwd: repo, encoding: "utf8" });
    assert.equal(mermaid.status, 0, mermaid.stderr || mermaid.stdout);
    assert.match(mermaid.stdout, /^flowchart LR/);
    assert.match(mermaid.stdout, /OrderService\.placeOrder/);

    const planner = run(["graph", "planner-status", "--query", "placeOrder", "--out", ".agent", "--limit", "2"], repo);
    assert.equal(planner.provider, "blueprint-treesitter");
    assert.ok(["ready", "missing_command", "unavailable", "broken"].includes(planner.planner.state));
    // A "ready" verdict must be earned by actually round-tripping a ContextPacket
    // through `cortex plan-context`, not by grepping help text (the old probe).
    if (planner.planner.state === "ready") {
      assert.match(planner.planner.evidence, /round-tripped a ContextPacket/);
    }
    assert.equal(planner.candidateSet.candidates[0].sourceKind, "repo_code");
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});
