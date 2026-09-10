// tests/adversarial/authorization-adversarial.test.mjs — MBR-802.
//
// Native authorization owner guard. Full adversarial behavior is covered by
// engine/crates/membrane-runtime/tests/native_authorization.rs.

import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
const root = new URL("../..", import.meta.url);
const rust = readFileSync(new URL("engine/crates/membrane-runtime/src/authorization.rs", root), "utf8");
test("native authorization owner exposes bounded decision gates", () => {
  for (const symbol of ["authorize", "intersect", "permits", "can_reach", "read-only", "write-proposed"]) {
    assert.match(rust, new RegExp(symbol.replace("-", "[-_]"), "i"), `missing native authorization symbol ${symbol}`);
  }
});
