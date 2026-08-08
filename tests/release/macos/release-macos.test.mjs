import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const script = resolve("scripts/release/macos/release-macos.mjs");
test("release script rejects missing command and artifact inputs", () => {
  const result = spawnSync(process.execPath, [script, "verify"], { encoding: "utf8" });
  assert.equal(result.status, 2); assert.match(result.stderr, /--app and --dmg are required/);
});
test("receipt command requires identity fields before touching output", () => {
  const result = spawnSync(process.execPath, [script, "receipt", "--app", "x.app", "--dmg", "x.dmg"], { encoding: "utf8" });
  assert.equal(result.status, 2); assert.match(result.stderr, /missing app/);
});
