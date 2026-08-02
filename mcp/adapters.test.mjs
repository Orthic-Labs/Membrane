import assert from "node:assert/strict";
import test from "node:test";
import { adapterManifest, requestContext } from "./adapters.mjs";

test("X2 adapters share Membrane context and honest levels", () => {
  assert.equal(adapterManifest("cursor").max_honest_level, "L1");
  assert.equal(adapterManifest("windsurf").max_honest_level, "L1");
  assert.equal(adapterManifest("generic_mcp").max_honest_level, "L0");
  assert.equal(adapterManifest("generic_mcp").tool_receipts, false);
  assert.equal(adapterManifest("cursor").install, undefined);
  assert.equal(adapterManifest("windsurf").uninstall, undefined);
});

test("X2 adapter reports degraded state when shared client is unavailable", () => {
  const result = requestContext("cursor", { task: "x" }, { client: "/tmp/no-such-membrane-client.mjs" });
  assert.equal(result.state, "degraded");
});
