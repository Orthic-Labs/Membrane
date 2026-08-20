import assert from "node:assert/strict";
import { chmodSync, cpSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/cortex.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

test("read commands never open graph.db writable", () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-read-only-"));
  cpSync(FIXTURE, repo, { recursive: true });
  try {
    const build = spawnSync(process.execPath, [CLI, "graph", "build", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
    assert.equal(build.status, 0, build.stderr || build.stdout);
    const dbPath = join(repo, ".agent/graph/graph.db");
    chmodSync(dbPath, 0o444);
    const commands = [
      ["graph", "status", "--out", ".agent"],
      ["graph", "search", "--query", "placeOrder", "--out", ".agent"],
      ["graph", "neighbors", "--node", "file:src/routes.ts", "--out", ".agent"],
      ["graph", "path", "--from", "file:src/routes.ts", "--to", "file:src/service.ts", "--out", ".agent"],
      ["graph", "impact", "--node", "file:src/routes.ts", "--out", ".agent"],
      ["graph", "architecture", "--out", ".agent"],
      ["graph", "flows", "--out", ".agent"],
      ["graph", "candidates", "--query", "placeOrder", "--out", ".agent"],
      ["orient", "--query", "placeOrder", "--out", ".agent"],
    ];
    for (const args of commands) {
      const result = spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
      assert.equal(result.status, 0, `${args.join(" ")} attempted a writable open: ${result.stderr || result.stdout}`);
    }
  } finally {
    chmodSync(join(repo, ".agent/graph/graph.db"), 0o644);
    rmSync(repo, { recursive: true, force: true });
  }
});
