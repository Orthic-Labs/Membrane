import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { graphFlowInventory } from "../src/graph/static-provider.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CORTEX = path.resolve(HERE, "..");
const CLI = path.join(CORTEX, "scripts/cortex.mjs");
const FIXTURE = path.join(CORTEX, "evals/fixture-repos/typescript-commerce");

test("graph flows emits deterministic entry-to-terminal inventory", () => {
  const repo = path.join(os.tmpdir(), `cortex-flow-${process.pid}-${Date.now()}`);
  fs.cpSync(FIXTURE, repo, { recursive: true });
  try {
    const build = spawnSync(process.execPath, [CLI, "graph", "build", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
    assert.equal(build.status, 0, build.stderr || build.stdout);
    const result = spawnSync(process.execPath, [CLI, "graph", "flows", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const payload = JSON.parse(result.stdout);
    assert.equal(payload.provider, "cortex-static");
    assert.ok(payload.flows.some((flow) => (
      flow.entry.id === "symbol:src/routes.ts::registerOrderRoute"
      && flow.status === "complete"
      && flow.path.some((node) => node.id === "symbol:src/store.ts::OrderStore.save")
    )));
    assert.ok(fs.existsSync(path.join(repo, ".agent/flows.json")));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});

test("flow inventory is capped on branching graphs", () => {
  const nodes = [{ id: "symbol:entry", kind: "symbol", labels: ["Function"], evidence: [{ path: "a.ts", startLine: 1, endLine: 1, contentHash: "x" }] }];
  const edges = [];
  for (let index = 0; index < 200; index += 1) {
    const mid = { id: `symbol:mid${index}`, kind: "symbol", labels: ["Function"], evidence: [{ path: "a.ts", startLine: 1, endLine: 1, contentHash: "x" }] };
    const leaf = { id: `symbol:leaf${index}`, kind: "symbol", labels: ["Function"], evidence: [{ path: "a.ts", startLine: 1, endLine: 1, contentHash: "x" }] };
    nodes.push(mid, leaf);
    edges.push({ id: `e1-${index}`, kind: "CALLS", source: "symbol:entry", target: mid.id, evidence: [] });
    edges.push({ id: `e2-${index}`, kind: "CALLS", source: mid.id, target: leaf.id, evidence: [] });
  }

  const inventory = graphFlowInventory({ provider: { id: "cortex-static" }, nodes, edges }, { maxFlows: 25 });

  assert.equal(inventory.flows.length, 25);
  assert.equal(inventory.truncated, true);
  assert.equal(inventory.mode, "bounded");
  assert.match(inventory.truncationReason, /maxFlows=25/);

  const complete = graphFlowInventory({ provider: { id: "cortex-static" }, nodes, edges }, { complete: true, maxFlows: 500 });
  assert.equal(complete.flows.length, 200);
  assert.equal(complete.truncated, false);
  assert.equal(complete.mode, "complete");
});
