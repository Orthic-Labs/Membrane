import assert from "node:assert/strict";
import test from "node:test";
import { buildCapabilityInventory } from "./capability-inventory.mjs";

test("capability inventory is generated from live MCP, adapter, and contract surfaces", async () => {
  const inventory = await buildCapabilityInventory();
  assert.equal(inventory.schema, "orthic.capability-inventory.v1");
  assert.deepEqual(inventory.labels, ["shipped", "partial", "unwired", "design", "deprecated"]);
  assert.ok(inventory.mcp.tools.some((tool) => tool.name === "membrane_context"));
  assert.equal(inventory.adapters.codex_cli.level, "L2");
  assert.equal(inventory.adapters.generic_mcp.level, "L0");
  assert.match(inventory.contract_freeze["context-contracts.schema.json"], /^[a-f0-9]{64}$/);
});
