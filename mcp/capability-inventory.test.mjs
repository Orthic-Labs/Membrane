import assert from "node:assert/strict";
import test from "node:test";
import { buildCapabilityInventory } from "./capability-inventory.mjs";

test("capability inventory is generated from live MCP, adapter, and contract surfaces", async () => {
  const inventory = await buildCapabilityInventory();
  assert.equal(inventory.schema, "membrane.capability-inventory.v1");
  assert.deepEqual(inventory.labels, ["shipped", "partial", "unwired", "design", "deprecated"]);
  assert.ok(inventory.mcp.tools.some((tool) => tool.name === "membrane_context"));
  assert.equal(inventory.adapters.codex_cli.level, "L2");
  assert.equal(inventory.adapters.generic_mcp.level, "L0");
  assert.match(inventory.contract_freeze["context-contracts.schema.json"], /^[a-f0-9]{64}$/);
  assert.ok(inventory.capabilities.length > inventory.mcp.tools.length);
  assert.ok(inventory.capabilities.filter(({ status }) => status === "shipped").every(({ test_ids, artifact, platforms }) => test_ids.length && artifact && platforms.length));
  assert.equal(inventory.capabilities.find(({ capability }) => capability === "adapter.ccx").status, "partial");
});

test("MBR-016: no capability is shipped unless all five gates for its claim class pass", async () => {
  const inventory = await buildCapabilityInventory();
  // The five-gate model is exposed and stable.
  assert.deepEqual(inventory.gate_model.gates, ["exercised_test", "artifact", "platforms", "documented", "contract"]);
  assert.equal(inventory.gate_model.required, 5);
  for (const capability of inventory.capabilities) {
    if (capability.status === "shipped") {
      assert.equal(capability.gates_passed, capability.gates_required, `${capability.capability} shipped without all gates`);
      assert.ok(Object.values(capability.gates).every(Boolean), `${capability.capability} shipped with a failed gate`);
    } else {
      assert.ok(capability.gates_passed < capability.gates_required, `${capability.capability} not shipped yet passes all gates`);
    }
  }
});

test("MBR-016: test-presence alone no longer yields shipped (missing gates drop to partial)", async () => {
  const inventory = await buildCapabilityInventory();
  // ccx has a source artifact and is documented but lacks an exercised-path test
  // and platform support — under test-presence it would still be claimable; the
  // five-gate model forces it to partial.
  const ccx = inventory.capabilities.find(({ capability }) => capability === "adapter.ccx");
  assert.equal(ccx.status, "partial");
  assert.equal(ccx.gates.exercised_test, false);
  assert.equal(ccx.gates.platforms, false);
  assert.ok(ccx.gates_passed < 5);
  // generic_mcp passes all five gates and stays shipped.
  const generic = inventory.capabilities.find(({ capability }) => capability === "adapter.generic_mcp");
  assert.equal(generic.status, "shipped");
  assert.equal(generic.gates_passed, 5);
});
