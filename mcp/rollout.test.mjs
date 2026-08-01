import assert from "node:assert/strict";
import test from "node:test";
import { advanceRollout, STAGES } from "./rollout.mjs";

test("E2 promotion is ordered and evidence-gated", () => {
  assert.deepEqual(STAGES, ["shadow", "advisory", "context_enforced", "tool_enforced", "learning"]);
  assert.throws(() => advanceRollout("shadow", "context_enforced", { current_commit: true }), /not ordered/);
  const receipt = advanceRollout("shadow", "advisory", { genuine_tasks: true });
  assert.equal(receipt.status, "promoted");
});

test("E2 context enforcement requires both Tier 1 platforms", () => {
  assert.throws(() => advanceRollout("advisory", "context_enforced", { macOS: true }), /Windows/);
  assert.equal(advanceRollout("advisory", "context_enforced", { macOS: true, Windows: true }).to, "context_enforced");
});
