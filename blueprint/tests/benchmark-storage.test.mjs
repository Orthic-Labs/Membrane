import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fixtureFilePath, fixtureIdentity, generateBenchmarkFixture } from "../scripts/generate-benchmark-fixture.mjs";
import { REPORT_SCHEMA, validateStorageReport } from "../scripts/benchmark-storage.mjs";

test("storage benchmark fixture generator is deterministic and marked", () => {
  const dir = mkdtempSync(join(tmpdir(), "blueprint-bench-fixture-"));
  try {
    const identity = generateBenchmarkFixture({ outDir: dir, files: 12 });
    assert.deepEqual(identity, fixtureIdentity(12));
    assert.ok(existsSync(join(dir, fixtureFilePath(11))));
    assert.match(readFileSync(join(dir, fixtureFilePath(11)), "utf8"), /benchmarkSymbol00011/);
    assert.deepEqual(generateBenchmarkFixture({ outDir: dir, files: 12 }), identity);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("storage report validator requires raw timings plus observed peak RSS for every build mode", () => {
  const sample = { elapsedMs: 1, peakRssBytes: 1024 };
  const valid = { schema: REPORT_SCHEMA, source: { commit: "abc", worktree: "clean" }, fixture: { id: "fixture", files: 550 }, host: { node: "v26", platform: "darwin" }, metrics: { coldBuild: { samples: [sample], peakRssBytes: 1024 }, noChange: { samples: [sample], peakRssBytes: 1024 }, oneFileDelta: { samples: [sample], peakRssBytes: 1024 }, walBlockedReader: {}, allocation: {} } };
  assert.deepEqual(validateStorageReport(valid), { ok: true, problems: [] });
  assert.equal(validateStorageReport({}).ok, false);
});
