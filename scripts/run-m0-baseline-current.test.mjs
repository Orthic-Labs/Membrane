import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import test from "node:test";
import { fixturePath, probes, runM0 } from "./run-m0-baseline-current.mjs";

test("F13 fixture path resolves after the Roundtable-to-Citadel rename (regression for the ENOENT crash)", () => {
  assert.ok(existsSync(fixturePath), `fixture missing at ${fixturePath}`);
});

test("F13 every probe's documented baseline value fails validation and current value passes", () => {
  for (const [id, probe] of Object.entries(probes)) {
    assert.equal(probe.valid(probe.baseline), false, `${id}: documented baseline unexpectedly validates`);
    assert.equal(probe.valid(probe.current), true, `${id}: current literal unexpectedly fails validation`);
  }
});

test("F13 runM0 fails a case when its owning suite fails, even if the literal is valid", () => {
  const result = runM0({ runSuiteImpl: (owner) => ({ owner, status: owner === "cortex" ? 1 : 0, signal: null, output_sha256: "sha256:0", output_tail: "" }) });
  assert.equal(result.passed, false);
  const cortex = result.cases.find((entry) => entry.id === "mcp-write-noop");
  assert.equal(cortex.current.status, "fail");
  assert.equal(cortex.current.suite_exit_status, 1);
  const other = result.cases.find((entry) => entry.owner !== "cortex");
  assert.equal(other.current.status, "pass");
});

test("F13 runM0 discloses every case as baseline-undrivable rather than silently asserting a full dual-commit proof", () => {
  const result = runM0({ runSuiteImpl: (owner) => ({ owner, status: 0, signal: null, output_sha256: "sha256:0", output_tail: "" }) });
  assert.equal(result.passed, true);
  assert.equal(result.baseline_red, result.cases.length);
  assert.equal(result.current_green, result.cases.length);
  assert.equal(result.undrivable_fixtures.length, result.cases.length);
  assert.ok(result.undrivable_fixtures.every((entry) => entry.side === "baseline" && /historical commit/.test(entry.reason)));
  assert.ok(result.cases.every((entry) => entry.baseline.live_reproduced === false && entry.current.live_reproduced === true));
});

// Slow, unmocked integration proof: actually spawn the real cortex/membrane-host/membrane/blueprint/
// forge/adapt suites at current HEAD, on this machine, exactly as the CLI entrypoint does.
// This is the test that makes "current_green" a live claim instead of a mocked one. It is slower
// than the rest of this file (cross-repo pnpm/pytest runs) — that cost buys real evidence.
test("F13 runM0 end-to-end: real cross-repo suites at current HEAD (slow, unmocked)", { timeout: 9 * 60_000 }, () => {
  const result = runM0();
  assert.equal(result.schema, "membrane.m0-run.v1");
  assert.equal(result.cases.length, 10);
  for (const suite of result.suites) {
    assert.equal(suite.status, 0, `${suite.owner} suite failed at current HEAD: ${suite.output_tail.slice(-400)}`);
  }
  assert.equal(result.current_green, 10);
  assert.equal(result.passed, true);
});
