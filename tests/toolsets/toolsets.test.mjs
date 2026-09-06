import assert from "node:assert/strict";
import test from "node:test";
import { parseToolsetConfig, toolsetNames } from "../../mcp/toolsets.mjs";

const safeDefault = ["membrane_context", "membrane_source_read", "membrane_ledger", "membrane_knowledge_propose", "membrane_memory", "membrane_checkpoint_save", "membrane_checkpoint_load"];
const groups = {
  default: safeDefault,
  memory: ["membrane_context", "membrane_knowledge_propose", "membrane_memory", "membrane_checkpoint_save", "membrane_checkpoint_load", "membrane_working_context", "membrane_temporal_fact", "membrane_scratchpad", "membrane_feedback"],
  blueprint: ["membrane_source_read", "membrane_blueprint"], ledger: ["membrane_source_read", "membrane_ledger"], diagnostic: [], adapt: ["membrane_adapt_inspect"], push: ["membrane_push_prepare", "membrane_push_resolve"], operator: ["membrane_knowledge_review"],
};
const params = (value) => ({ _meta: { "membrane.toolsets.v1": value } });

test("toolsets negotiate valid groups & conservative metadata fallback", () => {
  assert.deepEqual(toolsetNames({}, groups), safeDefault);
  assert.deepEqual(toolsetNames(params(["memory"]), groups), [...safeDefault, "membrane_working_context", "membrane_temporal_fact", "membrane_scratchpad", "membrane_feedback"]);
  assert.deepEqual(toolsetNames(params(["blueprint"]), groups), [...safeDefault, "membrane_blueprint"]);
  assert.deepEqual(toolsetNames(params(["operator"]), groups), [...safeDefault, "membrane_knowledge_review"]);
  for (const value of ["memory", ["unknown"], ["memory", "memory"], { memory: true }]) assert.deepEqual(toolsetNames(params(value), groups), safeDefault);
});

test("invalid configuration falls back", () => {
  assert.equal(parseToolsetConfig('{"version":"membrane.toolsets.v1","groups":{}}'), null);
  assert.equal(parseToolsetConfig('{"version":"wrong","groups":{}}'), null);
  assert.equal(parseToolsetConfig(JSON.stringify({ version: "membrane.toolsets.v1", groups: { ...groups, default: ["membrane_context"] } })), null);
  assert.deepEqual(toolsetNames(params(["memory"]), null), safeDefault);
});
