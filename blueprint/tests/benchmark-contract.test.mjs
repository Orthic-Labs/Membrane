// D43: benchmark contract — preregistered metrics, repo classes, and the
// retrieval/parser harness entry points.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { checkBenchmarkContract, BENCHMARK_METRICS, REPO_CLASSES } from "../scripts/benchmark-contract.mjs";
import { runParserBenchmark } from "../scripts/benchmark-parsers.mjs";

const ROOT = join(import.meta.dirname, "..");
const spec = JSON.parse(readFileSync(join(ROOT, "evals", "benchmark-spec.json"), "utf8"));

test("benchmark spec satisfies the preregistered contract", () => {
  const result = checkBenchmarkContract(spec);
  assert.equal(result.ok, true, result.problems?.join("; "));
});

test("all metrics are preregistered", () => {
  assert.equal(BENCHMARK_METRICS.length, 16);
  for (const metric of ["exact_correctness", "no_answer_abstention", "false_confidence", "stale_answer", "mrr", "ndcg"]) {
    assert.ok(BENCHMARK_METRICS.includes(metric));
  }
});

test("repo classes cover small to very-large", () => {
  assert.deepEqual(REPO_CLASSES, ["small", "medium", "large", "very-large"]);
});

test("parser benchmark smoke run records hardware, versions, and failures", async () => {
  const result = await runParserBenchmark();
  assert.equal(result.schemaVersion, 1);
  assert.ok(result.hardware);
  assert.ok(result.node);
  assert.ok(result.rows.length > 0);
  for (const row of result.rows) {
    if (row.state === "ok") {
      assert.ok(row.elapsedMs >= 0);
      assert.ok(row.memoryBytes > 0);
      assert.ok(row.parseFailures >= 0);
    }
  }
});
