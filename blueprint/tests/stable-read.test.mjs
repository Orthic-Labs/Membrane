import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { contentDigest } from "../src/graph/generation-identity.mjs";
import { stableRead } from "../src/graph/stable-read.mjs";

test("stableRead returns exact bytes, digest, identity, and stats", () => {
  const dir = mkdtempSync(join(tmpdir(), "blueprint-stable-read-"));
  const path = join(dir, "sample.ts");
  const bytes = Buffer.from("export const exact = 'bytes';\n", "utf8");
  writeFileSync(path, bytes);
  try {
    const result = stableRead(path);
    assert.deepEqual(result.bytes, bytes);
    assert.equal(result.contentDigest, contentDigest(bytes));
    assert.ok(result.fileIdentity);
    assert.equal(result.unstable, false);
    assert.equal(result.statBefore.size, bytes.length);
    assert.equal(result.statAfter.size, bytes.length);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
