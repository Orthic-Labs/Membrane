import assert from "node:assert/strict";
import test from "node:test";
import { checkRuntime, RUNTIME_CAPABILITIES } from "../src/lib/runtime-capabilities.mjs";

test("runtime capability check returns the running Node version and an explicit supported flag", () => {
  const result = checkRuntime();
  assert.equal(typeof result.ok, "boolean");
  assert.equal(result.capabilities.nodeVersion, process.version);
  assert.match(result.capabilities.supportedRange, />=\d+/);
  if (result.ok) {
    assert.equal(result.error, undefined);
  } else {
    assert.equal(result.error.code, "unsupported_node_runtime");
    assert.equal(result.error.nodeVersion, process.version);
    assert.ok(result.error.remediation.includes("Node"));
  }
});

test("runtime capability record is frozen and exposes the expected capability flags", () => {
  assert.equal(RUNTIME_CAPABILITIES.schemaVersion, 1);
  assert.equal(typeof RUNTIME_CAPABILITIES.capabilities.nodeSqlite, "boolean");
  assert.equal(typeof RUNTIME_CAPABILITIES.capabilities.topLevelAwait, "boolean");
  assert.equal(typeof RUNTIME_CAPABILITIES.capabilities.sqliteWAL, "boolean");
  assert.throws(() => { RUNTIME_CAPABILITIES.schemaVersion = 2; }, TypeError);
});