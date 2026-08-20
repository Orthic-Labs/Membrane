import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { adoptFileAtomically } from "../src/graph/atomic-store-adoption.mjs";

test("fresh store replaces prior file only after validation", () => {
  const dir = mkdtempSync(join(tmpdir(), "cortex-adopt-"));
  try {
    const target = join(dir, "graph.db"), source = join(dir, "fresh.db");
    writeFileSync(target, "old");
    writeFileSync(source, "fresh");
    adoptFileAtomically(source, target, (path) => assert.equal(readFileSync(path, "utf8"), "fresh"));
    assert.equal(readFileSync(target, "utf8"), "fresh");
    assert.equal(existsSync(source), false);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test("failed validation restores prior file", () => {
  const dir = mkdtempSync(join(tmpdir(), "cortex-adopt-"));
  try {
    const target = join(dir, "graph.db"), source = join(dir, "fresh.db");
    writeFileSync(target, "old");
    writeFileSync(source, "broken");
    assert.throws(() => adoptFileAtomically(source, target, () => { throw new Error("invalid"); }), /invalid/);
    assert.equal(readFileSync(target, "utf8"), "old");
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
