import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { advanceRollout, commit, STAGES } from "./rollout.mjs";

const MEMBRANE_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const WORKSPACE_ROOT = resolve(MEMBRANE_ROOT, "..");
const REAL_MEMBRANE_COMMIT = commit(MEMBRANE_ROOT);
const REAL_WORKSPACE_COMMIT = commit(WORKSPACE_ROOT);

const digest = (bytes) => `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
function artifact(kind, status, { fingerprintCommit = REAL_MEMBRANE_COMMIT, omitFingerprint = false } = {}) {
  const path = join(mkdtempSync(join(tmpdir(), "membrane-rollout-")), `${kind}.json`);
  const body = { schema: "membrane.competitor-parity.v1", phase: kind, status, open: [] };
  if (!omitFingerprint) body.fingerprint = { membrane: fingerprintCommit };
  const bytes = Buffer.from(JSON.stringify(body));
  writeFileSync(path, bytes);
  return { path, sha256: digest(bytes) };
}
function benchmarkArtifact(workspaceCommit) {
  const path = join(mkdtempSync(join(tmpdir(), "membrane-rollout-")), "benchmark.json");
  const bytes = Buffer.from(JSON.stringify({ schema: "membrane.e2e-benchmark-result.v1", status: "complete", manifest: { workspace_commit: workspaceCommit } }));
  writeFileSync(path, bytes);
  return { path, sha256: digest(bytes) };
}

test("E2 promotion is ordered and rejects caller booleans", () => {
  assert.deepEqual(STAGES, ["shadow", "advisory", "context_enforced", "tool_enforced", "learning"]);
  assert.throws(() => advanceRollout("shadow", "context_enforced", { current_commit: true }), /not ordered/);
  assert.throws(() => advanceRollout("shadow", "advisory", { genuine_tasks: true }), /source/);
  const receipt = advanceRollout("shadow", "advisory", { artifacts: { source: artifact("source", "source_passed") } });
  assert.equal(receipt.status, "promoted");
  assert.match(receipt.evidence[0].sha256, /^sha256:[a-f0-9]{64}$/);
  assert.equal(receipt.evidence[0].commit, REAL_MEMBRANE_COMMIT);
});

test("E2 context enforcement requires hash-bound Tier 1 receipts", () => {
  const mac = artifact("mac", "mac_host_passed");
  const windows = artifact("windows", "windows_host_passed");
  assert.throws(() => advanceRollout("advisory", "context_enforced", { artifacts: { mac } }), /windows/);
  const tampered = { ...windows, sha256: `sha256:${"0".repeat(64)}` };
  assert.throws(() => advanceRollout("advisory", "context_enforced", { artifacts: { mac, windows: tampered } }), /digest mismatch/);
  assert.equal(advanceRollout("advisory", "context_enforced", { artifacts: { mac, windows } }).to, "context_enforced");
});

test("E2 rejects a qualification artifact with no commit binding", () => {
  const source = artifact("source", "source_passed", { omitFingerprint: true });
  assert.throws(() => advanceRollout("shadow", "advisory", { artifacts: { source } }), /missing commit binding/);
});

test("E2 rejects a stale qualification artifact bound to a commit that is not HEAD", () => {
  const source = artifact("source", "source_passed", { fingerprintCommit: "0".repeat(40) });
  assert.throws(() => advanceRollout("shadow", "advisory", { artifacts: { source } }), /stale/);
});

test("E2 learning stage binds the benchmark receipt to the live workspace commit, not the membrane commit", () => {
  const stale = benchmarkArtifact("0".repeat(40));
  assert.throws(() => advanceRollout("tool_enforced", "learning", { artifacts: { benchmark: stale } }), /stale/);
  const fresh = benchmarkArtifact(REAL_WORKSPACE_COMMIT);
  const receipt = advanceRollout("tool_enforced", "learning", { artifacts: { benchmark: fresh } });
  assert.equal(receipt.to, "learning");
  assert.equal(receipt.evidence[0].commit, REAL_WORKSPACE_COMMIT);
});
