import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const CORTEX = join(import.meta.dirname, "..");
const CLI = join(CORTEX, "scripts", "cortex.mjs");
const LARGE_REPO = process.env.CORTEX_LARGE_REPO;

test("large Git file listings build without recursive-walk fallback", { skip: !LARGE_REPO }, () => {
  const result = spawnSync(process.execPath, [CLI, "build", "--out", ".agent", "--check"], {
    cwd: LARGE_REPO,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.equal(existsSync(join(LARGE_REPO, ".agent", "graph", "graph.db")), true);
});
