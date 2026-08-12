import assert from "node:assert/strict";
import test from "node:test";
import { REPORT_SCHEMA, isCanonicalCorePath, selectCanonicalCoreFiles, validateCanonicalCoreReport } from "../scripts/benchmark-canonical-core.mjs";

test("canonical core selection retains tracked runtime sources & excludes fixtures, generated output, and vendor research", () => {
  assert.deepEqual(selectCanonicalCoreFiles(["scripts/cortex.mjs", ".agent/manifest.json", "vendor-research/notes.md", "evals/fixture-repos/sample/index.js", "evals/performance-fixtures/sample.json", "graph/store-sqlite.mjs"]), ["graph/store-sqlite.mjs", "scripts/cortex.mjs"]);
  assert.equal(isCanonicalCorePath("tests/canonical-core-benchmark.test.mjs"), true);
});

test("canonical core report requires source, exact inventory, isolated snapshot, raw timings, and RSS", () => {
  const sample = { elapsedMs: 1, peakRssBytes: 1024 };
  const timed = { ...sample, stageTimings: { state: "measured", records: [{ stage: "source_scan", elapsedMs: 1 }] } };
  const report = { schema: REPORT_SCHEMA, commands: { inventory: "git ls-files --cached -z", snapshot: "git init", smoke: "node scripts/cortex.mjs build" }, source: { head: "abc", worktree: { state: "clean" } }, inventory: { selectedFiles: 1, selectionSha256: "a", trackedContentSha256: "b" }, isolation: { temporarySnapshot: true, measuredContentSha256: "c", liveAgentMutated: false }, host: { node: "v26", platform: "darwin" }, metrics: { smoke: { samples: [timed] }, coldBuild: { samples: [timed] }, noChangeBuild: { samples: [timed] } } };
  assert.deepEqual(validateCanonicalCoreReport(report), { ok: true, problems: [] });
  assert.equal(validateCanonicalCoreReport({}).ok, false);
});
