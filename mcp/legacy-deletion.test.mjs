import assert from "node:assert/strict";
import test from "node:test";
import { deletionPlan, LEGACY_PATHS, n1dEvidenceGate } from "./legacy-deletion.mjs";

test("E3 keeps legacy paths until measured activation and records all deletion targets", () => {
  const plan = deletionPlan();
  assert.equal(plan.status, "blocked_until_stable_activation");
  assert.equal(plan.paths.length, 11);
  assert.ok(plan.paths.includes("n1d_aliases"));
  assert.equal(plan.durable_ids_rekeyed, false);
  assert.equal(LEGACY_PATHS.length, 11);
});

test("N1d requires clean Mac and Windows cycles plus rollback evidence", () => {
  const blocked = n1dEvidenceGate({ macosCycle: { clean_daily_cycle: true, alias_readback: true } });
  assert.equal(blocked.status, "blocked");
  assert.ok(blocked.missing.includes("Windows clean daily cycle"));

  const ready = n1dEvidenceGate({
    macosCycle: { clean_daily_cycle: true, alias_readback: true },
    windowsCycle: { clean_daily_cycle: true, alias_readback: true },
    compatibilityReceipt: "compat-sha",
    rollbackReceipt: "rollback-sha",
  });
  assert.equal(ready.status, "ready_to_delete");
  assert.equal(ready.durable_ids_rekeyed, false);
  assert.equal(ready.delete_targets.length, 3);
});
