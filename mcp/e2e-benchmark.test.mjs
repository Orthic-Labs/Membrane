import assert from "node:assert/strict";
import test from "node:test";
import { runBenchmark } from "./e2e-benchmark.mjs";

test("E1 archives whole-path stages and fails closed when model/tool behavior is absent", () => {
  const result = runBenchmark({ workspaceRoot: new URL("../../", import.meta.url).pathname });
  assert.equal(result.schema, "orthic.e2e-benchmark-result.v1");
  assert.equal(result.records.length, 10);
  assert.equal(result.status, "blocked_incomplete_path");
  assert.equal(result.exit_gate.isolated_ann_claim, false);
  assert.ok(result.records.every((record) => record.stages.includes("outcome_feedback")));
});

test("E1 accepts an exercised model/tool callback for every scenario", () => {
  const result = runBenchmark({ workspaceRoot: new URL("../../", import.meta.url).pathname, scenarioRunner: () => ({ model_tool_exercised: true }) });
  assert.equal(result.status, "complete");
  assert.equal(result.exit_gate.model_tool_behavior_exercised, true);
});
