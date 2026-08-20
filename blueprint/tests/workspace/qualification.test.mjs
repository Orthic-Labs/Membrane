import assert from "node:assert/strict";
import { createXXHash128 } from "hash-wasm";

// XXH3-128, matching the harness and both providers. These expectations are
// RECOMPUTED from the same file bytes, not pasted literals, so they still
// verify normalization rather than restating whatever the code produced.
const xxhasher = await createXXHash128();

function xxh3Hex(bytes) {
  xxhasher.init();
  xxhasher.update(bytes);
  return xxhasher.digest("hex");
}
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  applyBudgetApproval,
  loadTasks,
  makeBlueprintStaticProvider,
  makeFallbackProvider,
  normalizeGitNexusContext,
  qualifyProvider,
  schemaHash,
  selectProvider,
} from "../../evals/run-qualification.mjs";
import { findWorkspaceRoot } from "../_workspace-root.mjs";


const HERE = path.dirname(fileURLToPath(import.meta.url));
const BLUEPRINT = path.resolve(HERE, "../..");
const ROOT = findWorkspaceRoot(HERE, { required: false });
const TASKS = path.join(BLUEPRINT, "evals/graph-tasks.jsonl");
const REPOS = path.join(BLUEPRINT, "evals/fixture-repos");
const SCHEMA = ROOT ? path.join(ROOT, "tools/lib/context-contracts.schema.json") : null;
const workspaceSkip = ROOT ? false : "requires parent monorepo context contracts";

test("GitNexus context is normalized into exact Blueprint evidence", { skip: workspaceSkip }, () => {
  const normalized = normalizeGitNexusContext({
    status: "found",
    symbol: { uid: "Method:src/service.ts:OrderService.placeOrder#1", name: "placeOrder", kind: "Method", filePath: "src/service.ts", startLine: 5, endLine: 8 },
    incoming: { calls: [{ uid: "Function:src/routes.ts:registerOrderRoute", name: "registerOrderRoute", filePath: "src/routes.ts" }] },
    outgoing: { calls: [{ uid: "Method:src/store.ts:OrderStore.save#1", name: "save", filePath: "src/store.ts" }] },
  }, REPOS + "/typescript-commerce");

  assert.deepEqual(normalized.evidence[0], {
    path: "src/service.ts",
    startLine: 6,
    endLine: 9,
    contentHash: xxh3Hex(fs.readFileSync(path.join(REPOS, "typescript-commerce/src/service.ts"))),
  });
  assert.ok(normalized.edges.some((edge) => edge.source.name === "registerOrderRoute" && edge.target.name === "placeOrder"));
  assert.ok(normalized.edges.some((edge) => edge.source.name === "placeOrder" && edge.target.name === "save"));
});


test("locked task rows point to real bounded evidence", { skip: workspaceSkip }, () => {
  const tasks = loadTasks(TASKS);

  assert.equal(tasks.length, 12);
  assert.equal(tasks.filter((item) => item.split === "heldout").length, 3);
  assert.equal(tasks.filter((item) => item.qualificationClass === "mandatory_structural").length, 7);
  assert.ok(tasks.filter((item) => item.qualificationClass === "mandatory_structural" && item.kind !== "symbol_definition")
    .every((item) => item.expectedEdges.length > 0));
  assert.deepEqual(new Set(tasks.filter((item) => item.kind === "semantic_lookup").map((item) => item.repo)), new Set([
    "typescript-commerce", "rust-audio", "python-context",
  ]));
  for (const task of tasks) {
    assert.ok(task.expectedNodes.length > 0 && task.expectedEvidence.length > 0);
    assert.ok(task.maxPacketTokens > 0 && task.protectedAnchors.length > 0);
    for (const evidence of task.expectedEvidence) {
      const file = path.join(REPOS, task.repo, evidence.path);
      assert.ok(fs.existsSync(file), `${task.id}: missing ${file}`);
      const expectedHash = xxh3Hex(fs.readFileSync(file));
      assert.equal(evidence.contentHash, expectedHash, `${task.id}: stale hash for ${evidence.path}`);
      const lines = fs.readFileSync(file, "utf8").split(/\r?\n/);
      assert.ok(evidence.startLine >= 1 && evidence.endLine >= evidence.startLine);
      assert.ok(evidence.endLine <= lines.length, `${task.id}: invalid span ${evidence.path}:${evidence.endLine}`);
    }
  }
  assert.ok(tasks.filter((task) => task.oracle?.kind?.startsWith("scip-")).every((task) => task.oracle.status === "verified"));
});


test("fallback reports unsupported graph capabilities instead of empty success", { skip: workspaceSkip }, async () => {
  const task = loadTasks(TASKS).find((item) => item.kind === "call_path");
  const report = await qualifyProvider(makeFallbackProvider(), [task], REPOS);

  assert.equal(report.status, "failed");
  assert.equal(report.tasks[0].state, "unsupported");
  assert.equal(report.gates.correctness, false);
});


test("fallback measures lexical semantic retrieval instead of reporting blanket unsupported", { skip: workspaceSkip }, async () => {
  const task = loadTasks(TASKS).find((item) => item.id === "semantic-python-context");
  const report = await qualifyProvider(makeFallbackProvider(), [task], REPOS);

  assert.equal(report.tasks[0].state, "passed");
  assert.ok(report.tasks[0].primaryRank >= 1 && report.tasks[0].primaryRank <= 10);
  assert.equal(report.semantic.tasks, 1);
  assert.equal(report.semantic.macroRecallAt10, 1);
});

test("blueprint-static earns mandatory structural evidence without external provider state", { skip: workspaceSkip }, async () => {
  const tasks = loadTasks(TASKS).filter((item) => item.qualificationClass === "mandatory_structural");
  const report = await qualifyProvider(makeBlueprintStaticProvider({ schemaPath: SCHEMA }), tasks, REPOS);

  assert.equal(report.status, "passed");
  assert.equal(report.gates.correctness, true);
  assert.ok(report.tasks.every((item) => item.state === "passed"));
  assert.ok(report.tasks.find((item) => item.id === "ts-config-resource").evidence.some((item) => item.path === "resources/orders.json"));
  assert.ok(report.tasks.find((item) => item.id === "ts-test-coverage").evidence.some((item) => item.path === "test/service.test.ts"));
});


test("provider cannot pass by reading the answer key instead of returning evidence", { skip: workspaceSkip }, async () => {
  const task = loadTasks(TASKS).find((item) => item.kind === "symbol_definition");
  let calls = 0;
  const dishonest = {
    id: "dishonest",
    kind: "codebase-memory",
    capabilities: new Set(["symbol_definition"]),
    async probe() { return { available: true }; },
    async execute() { calls += 1; return { state: "ok", evidenceRefs: [] }; },
    async close() {},
  };

  const report = await qualifyProvider(dishonest, [task], REPOS);

  assert.equal(calls, 1);
  assert.equal(report.tasks[0].state, "failed");
  assert.ok(report.tasks[0].missingEvidence.some((item) => item.path === "src/service.ts"));
});


test("provider must return exact span and current hash evidence", { skip: workspaceSkip }, async () => {
  const task = loadTasks(TASKS).find((item) => item.kind === "symbol_definition");
  const wrongSpan = {
    id: "wrong-span",
    kind: "codebase-memory",
    capabilities: new Set(["symbol_definition"]),
    async probe() { return { available: true }; },
    async execute() {
      return {
        state: "ok",
        evidence: [{
          path: "src/service.ts",
          startLine: 1,
          endLine: 1,
          contentHash: task.expectedEvidence[0].contentHash,
        }],
        nodes: [{
          path: "src/service.ts",
          name: "placeOrder",
          qualifiedName: "fixture.OrderService.placeOrder",
          labels: ["Method"],
        }],
        edges: [],
      };
    },
    async close() {},
  };

  const report = await qualifyProvider(wrongSpan, [task], REPOS);

  assert.equal(report.tasks[0].state, "failed");
  assert.match(report.tasks[0].reason, /evidence_mismatch/);
});


test("probe metadata cannot self-certify executable gates", { skip: workspaceSkip }, async () => {
  const task = loadTasks(TASKS).find((item) => item.kind === "symbol_definition");
  const provider = {
    id: "metadata-only",
    kind: "codebase-memory",
    capabilities: new Set(["symbol_definition"]),
    async probe() {
      return {
        available: true,
        securityVerified: true,
        freshnessVerified: true,
        operabilityVerified: true,
        portability: ["win32", "darwin"],
      };
    },
    async execute() {
      return {
        state: "ok",
        evidence: task.expectedEvidence,
        nodes: [{
          path: "src/service.ts",
          name: "placeOrder",
          qualifiedName: "fixture.OrderService.placeOrder",
          labels: ["Method"],
        }],
        edges: [],
      };
    },
    async close() {},
  };

  const report = await qualifyProvider(provider, [task], REPOS);

  assert.equal(report.gates.freshness, false);
  assert.equal(report.gates.security, false);
  assert.equal(report.gates.operability, false);
  assert.equal(report.gates.portability, false);
});


test("optional semantic failure does not redefine mandatory structural correctness", { skip: workspaceSkip }, async () => {
  const tasks = loadTasks(TASKS);
  const symbol = tasks.find((item) => item.kind === "symbol_definition");
  const semantic = tasks.find((item) => item.kind === "semantic_lookup");
  const provider = {
    id: "structural-only",
    kind: "codebase-memory",
    capabilities: new Set(["symbol_definition", "semantic_lookup"]),
    async probe() { return { available: true }; },
    async execute(task) {
      if (task.id === symbol.id) return {
        state: "ok",
        evidence: symbol.expectedEvidence,
        nodes: [{
          path: "src/service.ts",
          name: "placeOrder",
          qualifiedName: "fixture.OrderService.placeOrder",
          labels: ["Method"],
        }],
        edges: [],
      };
      return { state: "unsupported", reason: "semantic_not_promoted" };
    },
    async close() {},
  };

  const report = await qualifyProvider(provider, [symbol, semantic], REPOS);

  assert.equal(report.status, "passed");
  assert.equal(report.gates.correctness, true);
  assert.equal(report.tasks.find((item) => item.id === semantic.id).state, "unsupported");
});


test("selection is blocked unless one provider passes every mandatory gate and budgets are approved", { skip: workspaceSkip }, () => {
  const failed = { id: "fallback", status: "failed", gates: { correctness: false } };
  const passing = {
    id: "provider-a",
    status: "passed",
    gates: { correctness: true, freshness: true, security: true, contract: true, portability: true, operability: true },
    metrics: { fullMs: 10, incrementalMs: 2, queryP95Ms: 1, peakRssBytes: 100, indexBytes: 50 },
    budgetApproval: "approved",
  };

  assert.deepEqual(selectProvider([failed]), { outcome: "no_provider_passed", selectedProvider: null });
  assert.deepEqual(
    selectProvider([{ ...passing, budgetApproval: "pending" }]),
    { outcome: "budget_approval_pending", selectedProvider: null },
  );
  assert.deepEqual(selectProvider([failed, passing]), { outcome: "selected", selectedProvider: "provider-a" });
});

test("budget approval can be applied to provider reports", { skip: workspaceSkip }, () => {
  const report = { id: "blueprint-static", budgetApproval: "pending" };
  assert.deepEqual(applyBudgetApproval([report], "approved"), [{ id: "blueprint-static", budgetApproval: "approved" }]);
  assert.deepEqual(applyBudgetApproval([report], "pending"), [report]);
});


test("schema hash covers exact canonical bytes", { skip: workspaceSkip }, () => {
  const expected = xxh3Hex(fs.readFileSync(SCHEMA));
  assert.equal(schemaHash(SCHEMA), expected);
});
