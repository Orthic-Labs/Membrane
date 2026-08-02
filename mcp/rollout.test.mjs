import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { advanceRollout, STAGES } from "./rollout.mjs";

const digest = (bytes) => `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
function artifact(kind, status) {
  const path = join(mkdtempSync(join(tmpdir(), "membrane-rollout-")), `${kind}.json`);
  const bytes = Buffer.from(JSON.stringify({ schema: "membrane.competitor-parity.v1", phase: kind, status, open: [] }));
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
});

test("E2 context enforcement requires hash-bound Tier 1 receipts", () => {
  const mac = artifact("mac", "mac_host_passed");
  const windows = artifact("windows", "windows_host_passed");
  assert.throws(() => advanceRollout("advisory", "context_enforced", { artifacts: { mac } }), /windows/);
  const tampered = { ...windows, sha256: `sha256:${"0".repeat(64)}` };
  assert.throws(() => advanceRollout("advisory", "context_enforced", { artifacts: { mac, windows: tampered } }), /digest mismatch/);
  assert.equal(advanceRollout("advisory", "context_enforced", { artifacts: { mac, windows } }).to, "context_enforced");
});
