import assert from "node:assert/strict";
import test from "node:test";
import { buildAdapterManifest } from "./adapter-manifest.mjs";

test("X1 runtime adapter manifest derives tool inventory and capability matrix", async () => {
  const manifest = await buildAdapterManifest();
  assert.equal(manifest.schema, "membrane.adapter-manifest.v1");
  assert.deepEqual(manifest.mcp.tools.map((tool) => tool.name), [
    "membrane_context", "membrane_source_read", "membrane_blueprint",
    "membrane_knowledge_propose", "membrane_checkpoint_save", "membrane_checkpoint_load",
    "membrane_working_context", "membrane_temporal_fact", "membrane_scratchpad",
    "membrane_feedback",
    // Live Diagnostics operational surface (design §12).
    "membrane_diagnostic_workspace", "membrane_diagnostic_mutation",
    "membrane_diagnostic_snapshot", "membrane_diagnostic_fence",
    "membrane_diagnostic_capabilities", "membrane_diagnostic_baseline",
    "membrane_diagnostic_provider",
  ]);
  assert.equal(manifest.adapters.ccx.inherits, "claude_code");
  assert.equal(manifest.adapters.generic_mcp.max_honest_level, "L0");
  assert.equal(manifest.deprecations, undefined);
});
