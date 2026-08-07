import assert from "node:assert/strict";
import test from "node:test";
import { parseToolsetConfig, toolsetNames } from "../../mcp/toolsets.mjs";

const groups = {
  default: ["membrane_context"], memory: ["membrane_context", "membrane_feedback"],
  cortex: ["membrane_source_read"], diagnostic: [],
};
const params = (value) => ({ _meta: { "membrane.toolsets.v1": value } });

test("toolsets negotiate valid groups & conservative metadata fallback", () => {
  assert.deepEqual(toolsetNames({}, groups), ["membrane_context"]);
  assert.deepEqual(toolsetNames(params(["memory"]), groups), ["membrane_context", "membrane_feedback"]);
  assert.deepEqual(toolsetNames(params(["cortex"]), groups), ["membrane_context", "membrane_source_read"]);
  assert.deepEqual(toolsetNames(params(["diagnostic"]), groups), ["membrane_context"]);
  assert.deepEqual(toolsetNames(params(["memory", "cortex"]), groups), ["membrane_context", "membrane_feedback", "membrane_source_read"]);
  for (const value of ["memory", ["unknown"], ["memory", "memory"], { memory: true }]) assert.deepEqual(toolsetNames(params(value), groups), ["membrane_context"]);
});

test("invalid configuration falls back", () => {
  assert.equal(parseToolsetConfig('{"version":"membrane.toolsets.v1","groups":{}}'), null);
  assert.equal(parseToolsetConfig('{"version":"wrong","groups":{}}'), null);
  for (const defaultGroup of [[], ["membrane_source_read"], ["membrane_context", "membrane_source_read"]]) {
    assert.equal(parseToolsetConfig(JSON.stringify({ version: "membrane.toolsets.v1", groups: { ...groups, default: defaultGroup } })), null);
  }
  assert.deepEqual(toolsetNames(params(["memory"]), null), ["membrane_context"]);
});
