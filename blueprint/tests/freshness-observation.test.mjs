import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { observeRepositoryFreshness } from "../src/sources/freshness-observation.mjs";

function git(root, args) {
  const result = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

test("Blueprint owns bounded commit and dirty-overlay freshness evidence", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-freshness-"));
  try {
    git(root, ["init", "--quiet"]);
    git(root, ["config", "user.email", "test@example.invalid"]);
    git(root, ["config", "user.name", "Test"]);
    writeFileSync(join(root, "app.js"), "export const value = 1;\n");
    git(root, ["add", "app.js"]);
    git(root, ["commit", "--quiet", "-m", "fixture"]);
    const head = git(root, ["rev-parse", "HEAD"]);

    const clean = observeRepositoryFreshness(root, { baseCommit: head });
    assert.equal(clean.available, true);
    assert.equal(clean.stable, true);
    assert.equal(clean.revision, head);
    assert.equal(clean.commitDistance, 0);
    assert.deepEqual(clean.entries, []);

    writeFileSync(join(root, "app.js"), "export const value = 2;\n");
    const dirty = observeRepositoryFreshness(root, { baseCommit: head });
    assert.equal(dirty.available, true);
    assert.equal(dirty.stable, true);
    assert.equal(dirty.entries.length, 1);
    assert.deepEqual(
      { path: dirty.entries[0].path, status: dirty.entries[0].status },
      { path: "app.js", status: " M" },
    );
    assert.match(dirty.entries[0].contentHash, /^sha256:[0-9a-f]{64}$/);
    assert.ok(Number.isInteger(dirty.stageElapsedMs.git_status));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
