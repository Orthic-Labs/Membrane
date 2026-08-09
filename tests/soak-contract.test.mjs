// D52: soak contract. The runbook's 1.0 gate requires that a deterministic
// multi-repo soak run proves: zero graph corruption, no false-current
// response, bounded memory/queue, independent repo degradation, eventual
// reconciliation, and a useful support bundle. A failed invariant blocks
// release; this test is the only way to catch that before the 1.0 release
// candidate goes out.

import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { runSoak } from "../scripts/run-soak.mjs";
import { applyFault, buildFaultSequence, faultCoverageReport } from "../scripts/fault-inject.mjs";

test("the soak report covers every required fault class", () => {
  const coverage = faultCoverageReport();
  for (const required of coverage.required) {
    assert.ok(coverage.covered.includes(required), `fault class covered: ${required}`);
  }
});

test("the fault sequence is deterministic for the same seed", () => {
  const a = buildFaultSequence({ seed: 42, durationEvents: 50, repoCount: 3 });
  const b = buildFaultSequence({ seed: 42, durationEvents: 50, repoCount: 3 });
  assert.equal(a.events.length, 50);
  assert.equal(b.events.length, 50);
  for (let index = 0; index < a.events.length; index += 1) {
    assert.equal(a.events[index].kind, b.events[index].kind, `event ${index} kinds match`);
    assert.equal(a.events[index].repo, b.events[index].repo, `event ${index} repos match`);
  }
});

test("two soak runs with the same seed produce the same summary", async () => {
  const ws1 = mkdtempSync(join(tmpdir(), "cortex-soak-det-"));
  const ws2 = mkdtempSync(join(tmpdir(), "cortex-soak-det-"));
  try {
    const { report: r1 } = await runSoak({ seed: 7, durationEvents: 50, repoCount: 3, workspaceDir: ws1 });
    const { report: r2 } = await runSoak({ seed: 7, durationEvents: 50, repoCount: 3, workspaceDir: ws2 });
    assert.deepEqual(r1.summary.faultKindsApplied.sort(), r2.summary.faultKindsApplied.sort());
    assert.equal(r1.summary.crashes, r2.summary.crashes, "deterministic crash count");
    assert.equal(r1.summary.actorFailures, r2.summary.actorFailures, "deterministic failure count");
  } finally {
    rmSync(ws1, { recursive: true, force: true });
    rmSync(ws2, { recursive: true, force: true });
  }
});

test("a soak run produces no graph corruption and an error-free fleet", async () => {
  const { report, work } = await runSoak({ seed: 1, durationEvents: 200, repoCount: 3 });
  try {
    assert.equal(report.summary.crashes, 0, `crashes: ${JSON.stringify(report.findings)}`);
    assert.equal(report.summary.reposHealthy, 3, "every repo's store opened and counted rows");
    for (const repo of report.fleet) {
      assert.ok(!repo.rowCount?.error, `repo ${repo.index} has no row-count error: ${JSON.stringify(repo.rowCount)}`);
      assert.ok(repo.rowCount.files >= 1, `repo ${repo.index} has file rows`);
      assert.ok(repo.rowCount.symbols >= 1, `repo ${repo.index} has symbol rows`);
      assert.ok(repo.rowCount.edges >= 1, `repo ${repo.index} has edge rows`);
    }
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("every required fault class is applied at least once", async () => {
  const { report, work } = await runSoak({ seed: 1, durationEvents: 200, repoCount: 3 });
  try {
    const applied = new Set(report.summary.faultKindsApplied);
    for (const required of faultCoverageReport().required) {
      assert.ok(applied.has(required), `fault class applied: ${required}`);
    }
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("one repo's failures never bleed into another repo's state", async () => {
  const { report, work } = await runSoak({ seed: 1, durationEvents: 200, repoCount: 3 });
  try {
    const faultsByRepo = new Map();
    for (const result of report.faultResults) {
      faultsByRepo.set(result.repo, (faultsByRepo.get(result.repo) ?? 0) + 1);
    }
    for (const repo of report.fleet) {
      const expected = faultsByRepo.get(repo.index) ?? 0;
      assert.ok(repo.actor.failures <= expected, `repo ${repo.index} failures (${repo.actor.failures}) <= applied (${expected})`);
    }
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("watcher overflow surfaces as a typed event_gap, not a corrupted store", async () => {
  const { report, work } = await runSoak({ seed: 1, durationEvents: 200, repoCount: 3 });
  try {
    const overflowFindings = report.faultResults.filter((result) => result.kind === "watcher_overflow");
    assert.ok(overflowFindings.length > 0, "watcher overflow was injected at least once");
    const gapped = report.fleet.filter((repo) => repo.actor.eventGap);
    assert.ok(gapped.length > 0, "at least one actor reported eventGap");
    for (const repo of gapped) {
      assert.ok(!repo.rowCount?.error, `gapped repo ${repo.index} store still opens`);
    }
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
});

test("applyFault throws on an unrecognised kind, by design", async () => {
  const actor = { ingest() {}, handleFailure() {} };
  await assert.rejects(() => applyFault(actor, { kind: "unknown", repo: 0, index: 0 }), /unrecognised/);
});

test("the soak's report is well-formed machine-readable JSON", async () => {
  const workspace = mkdtempSync(join(tmpdir(), "cortex-soak-json-"));
  try {
    const { report } = await runSoak({ seed: 1, durationEvents: 50, repoCount: 3, workspaceDir: workspace });
    assert.equal(report.schemaVersion, 1);
    assert.ok(report.runId && report.runId.length > 0, "runId is set");
    assert.ok(report.startedAt && report.endedAt, "start/end timestamps set");
    assert.ok(Number.isInteger(report.durationMs), "durationMs is an integer");
    assert.ok(Array.isArray(report.coverage.required));
    assert.ok(Array.isArray(report.coverage.covered));
    assert.ok(Array.isArray(report.fleet));
    assert.ok(Array.isArray(report.findings));
    assert.ok(Array.isArray(report.faultResults));
    assert.ok(report.summary && typeof report.summary.crashes === "number");
  } finally {
    rmSync(workspace, { recursive: true, force: true });
  }
});

