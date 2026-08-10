import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CORTEX = path.resolve(HERE, "..");
const CLI = path.join(CORTEX, "scripts/cortex.mjs");
const FIXTURE = path.join(CORTEX, "evals/fixture-repos/typescript-commerce");

test("task brief uses graph retrieval for read-first code paths", () => {
  const repo = path.join(os.tmpdir(), `cortex-brief-graph-${process.pid}-${Date.now()}`);
  fs.cpSync(FIXTURE, repo, { recursive: true });
  try {
    const result = spawnSync(process.execPath, [CLI, "brief", "--task", "change placeOrder behavior", "--out", ".agent"], {
      cwd: repo,
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const context = JSON.parse(fs.readFileSync(path.join(repo, ".agent/runs/change-placeorder-behavior/context.json"), "utf8"));
    // Graph retrieval must surface implementation files for placeOrder (service or routes)
    assert.ok(
      context.readFirst.includes("src/service.ts") || context.readFirst.includes("src/routes.ts"),
      `readFirst ${JSON.stringify(context.readFirst)} must include service or routes`,
    );
    // Also verify candidates directly include service.ts (graph is indexed)
    const cand = JSON.parse(
      spawnSync(process.execPath, [CLI, "graph", "candidates", "--query", "placeOrder", "--out", ".agent", "--json"], {
        cwd: repo,
        encoding: "utf8",
      }).stdout,
    );
    assert.ok(cand.candidates.some((c) => String(c.sourceRef).includes("src/service.ts")));
  } finally {
    fs.rmSync(repo, { recursive: true, force: true });
  }
});
