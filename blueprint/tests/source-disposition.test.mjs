import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { auditSourceDispositions } from "../src/providers/source-disposition.mjs";

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "blueprint-dispositions-"));
  execFileSync("git", ["init", "-q"], { cwd: root });
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(join(root, "src", "ok.ts"), "export const ok = 1;\n");
  writeFileSync(join(root, "src", "opaque.png"), Buffer.from([1, 2, 3]));
  writeFileSync(join(root, "src", "nul.ts"), Buffer.from([0x65, 0x78, 0, 0x70]));
  writeFileSync(join(root, "src", "unknown.zzzblueprint"), "unsupported tracked source\n");
  execFileSync("git", ["add", "."], { cwd: root });
  return root;
}

test("tracked source audit assigns every tracked path one terminal outcome", () => {
  const root = fixture();
  try {
    const admitted = [
      { path: "src/ok.ts" },
      { path: "src/opaque.png" },
    ];
    const result = auditSourceDispositions(root, admitted);
    assert.equal(result.scope, "git_tracked");
    assert.equal(result.complete, true);
    assert.equal(result.considered, 4);
    assert.equal(result.terminal, 4);
    assert.equal(result.indexed, 2);
    assert.ok(result.exceptions.some((entry) => entry.path === "src/nul.ts" && entry.disposition === "rejected"));
    assert.ok(result.exceptions.some((entry) => entry.path === "src/unknown.zzzblueprint" && entry.disposition === "unsupported"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("production graph augmentation carries the ingestion disposition receipt", () => {
  const root = fixture();
  try {
    const generation = buildGraphGeneration(root);
    const receipt = generation.augmentation?.providers?.ingestion;
    assert.ok(receipt, "build must persist ingestion accounting in provider augmentation");
    assert.equal(receipt.scope, "git_tracked");
    assert.equal(receipt.complete, true);
    assert.equal(receipt.considered, 4);
    assert.equal(receipt.terminal, 4);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
