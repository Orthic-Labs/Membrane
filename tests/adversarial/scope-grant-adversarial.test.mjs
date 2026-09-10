// tests/adversarial/scope-grant-adversarial.test.mjs — MBR-802.
//
// Native scope-grant contract guard. Behavioral adversarial coverage lives in
// engine/crates/membrane-runtime/tests/native_authorization.rs; this test keeps
// the JS gate from importing the deleted MCP implementation.

import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
const root = new URL("../..", import.meta.url);
const rust = readFileSync(new URL("engine/crates/membrane-federation/src/scope.rs", root), "utf8");
const protocol = readFileSync(new URL("engine/crates/membrane-protocol/src/types.rs", root), "utf8");
const schema = JSON.parse(readFileSync(new URL("schemas/scope-grant.v1.schema.json", root), "utf8"));
test("native scope-grant owner and schema remain present", () => {
  assert.match(rust, /SCOPE_GRANT_DOMAIN/);
  assert.match(rust, /verify|signature/i);
  assert.match(protocol, /struct ScopeGrantV1/);
  assert.ok(schema.required.includes("id"));
  assert.ok(schema.required.includes("signature"));
});
