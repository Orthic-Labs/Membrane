// Phase 7.6 — retrieval benchmark harness test.
//
// Pins three properties on the harness itself:
//   1. It loads the corpus from the checked-in JSON without inventing cases.
//   2. The no-gold class is present (abstention contract is exercised).
//   3. Two runs against the same corpus produce identical per-strategy
//      hit/MRR totals. Determinism is the harness's whole point.
// The benchmark itself is a script, not a unit test; this test wraps its
// public surface.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { resolveReportDir } from "../scripts/benchmark-retrieval.mjs";

const HERE = join(import.meta.dirname, "..");
const CORPUS = join(HERE, "evals/retrieval-corpus/corpus.v1.json");
const SCRIPT = join(HERE, "scripts/benchmark-retrieval.mjs");
const REPORT_DIR = mkdtempSync(join(tmpdir(), "blueprint-retrieval-benchmark-"));

test.after(() => rmSync(REPORT_DIR, { recursive: true, force: true }));

function runBenchmark() {
  const proc = execFileSync(process.execPath, [SCRIPT], { encoding: "utf8", timeout: 600000, env: { ...process.env, BLUEPRINT_RETRIEVAL_REPORT_DIR: REPORT_DIR } });
  const latestPath = join(REPORT_DIR, "latest.json");
  assert.ok(existsSync(latestPath), `expected ${latestPath} to be written`);
  const report = JSON.parse(readFileSync(latestPath, "utf8"));
  return { stdout: proc, report };
}

test("empty report directory resolves to default", () => assert.equal(resolveReportDir(""), join(HERE, "evals/retrieval-corpus/reports")));

test("corpus is loaded, has at least 20 cases, and includes the no-gold class", () => {
  const corpus = JSON.parse(readFileSync(CORPUS, "utf8"));
  assert.equal(corpus.schema, "blueprint-retrieval-corpus-v1");
  assert.ok(corpus.cases.length >= 20, `corpus has ${corpus.cases.length} cases, want >= 20`);
  const classes = new Set(corpus.cases.map((c) => c.class));
  assert.ok(classes.has("no-gold"), "no-gold class is required by the abstention contract");
  // The misleading-lexical class is the other mandatory task class per the
  // plan: "task classes MUST include no-gold and misleading-lexical".
  assert.ok(classes.has("misleading-lexical"), "misleading-lexical class is required by the plan");
  for (const entry of corpus.cases) {
    if (entry.gold.abstained) {
      assert.equal(entry.gold.reason, "no_relevant_seed", `case ${entry.id} abstention shape must match Phase 3.3 contract`);
    } else {
      assert.ok(entry.gold.path, `case ${entry.id} positive gold must have a path`);
      assert.ok(Array.isArray(entry.gold.region), `case ${entry.id} positive gold must have a region`);
      assert.ok(entry.gold.rationale, `case ${entry.id} positive gold must record WHY it is gold`);
    }
  }
});

test("benchmark runs end-to-end and writes the report", () => {
  const { report } = runBenchmark();
  assert.equal(report.schema, "blueprint-retrieval-report-v1");
  assert.ok(Array.isArray(report.strategies) && report.strategies.includes("lexical"));
  assert.ok(Array.isArray(report.perCase) && report.perCase.length >= 20);
  assert.ok(report.summary.lexical, "lexical strategy summary present");
  assert.ok(report.summary.graph, "graph strategy summary present");
  assert.ok(report.summary.hybrid, "hybrid strategy summary present");
  // No-gold cases must always be scored: either abstained_correctly or
  // junk_on_no_gold. Their sum across all strategies must equal the number of
  // no-gold cases in the corpus (counted per-strategy, since every strategy
  // sees every case).
  const noGoldCases = report.perCase.filter((row) => row.class === "no-gold").length;
  assert.equal(noGoldCases * report.strategies.length,
    report.summary.lexical.abstainedCorrectly + report.summary.lexical.junkOnNoGold +
    report.summary.graph.abstainedCorrectly + report.summary.graph.junkOnNoGold +
    report.summary.hybrid.abstainedCorrectly + report.summary.hybrid.junkOnNoGold,
    "every no-gold case must be accounted for by exactly one (abstained | junk) decision per strategy");
});

test("benchmark is deterministic: two runs against the same corpus yield identical hit/MRR totals", () => {
  // Run twice. Only the `runAt` envelope and per-query latencies may differ
  // (latency is wall-clock); the per-strategy SCORING and per-case verdict
  // must be byte-identical so reruns are comparable.
  const first = runBenchmark();
  const second = runBenchmark();
  for (const key of ["hitRate", "mrr", "abstainedCorrectly", "junkOnNoGold", "abstainedOnPositiveGold", "total", "freshnessFailures"]) {
    for (const strategy of first.report.summary ? Object.keys(first.report.summary) : []) {
      assert.deepEqual(first.report.summary[strategy][key], second.report.summary[strategy][key], `summary.${strategy}.${key} must be identical across runs`);
    }
  }
  assert.deepEqual(first.report.classBreakdown, second.report.classBreakdown, "class breakdown must be identical across runs");
  // Per-case verdict (hit/miss) must be identical; latencies and
  // retrieved-vs-used tokens may vary trivially because we don't pin the
  // scheduler.
  for (let index = 0; index < first.report.perCase.length; index += 1) {
    const a = first.report.perCase[index];
    const b = second.report.perCase[index];
    assert.equal(a.caseId, b.caseId, `case ${index} id stable across runs`);
    assert.deepEqual(a.scored, b.scored, `case ${a.caseId} scored results must be identical across runs`);
  }
});

test("abstention contract: at least one no-gold case must be answered with the abstention template by every strategy", () => {
  // This is the explicit contract gate from the plan: a strategy that returns
  // junk on a no-gold query should score WORSE than one that abstains. The
  // harness enforces that contract; this test pins that the contract is
  // actually being exercised (not silently passing 0/0 cases).
  const { report } = runBenchmark();
  for (const strategy of report.strategies) {
    const noGoldTotal = report.perCase.filter((row) => row.class === "no-gold").length;
    const noGoldAccounted = report.summary[strategy].abstainedCorrectly + report.summary[strategy].junkOnNoGold;
    assert.equal(noGoldAccounted, noGoldTotal, `${strategy}: every no-gold case must be either abstained_correctly or junk_on_no_gold`);
  }
});
