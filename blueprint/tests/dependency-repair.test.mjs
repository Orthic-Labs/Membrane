import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/blueprint.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function makeRepo() {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-repair-"));
  cpSync(FIXTURE, repo, { recursive: true });
  writeFileSync(join(repo, "src/third-dependent.ts"), "import { OrderService } from './service.js';\nexport const third = (service: OrderService) => service;\n");
  const build = spawnSync(process.execPath, [CLI, "graph", "build", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
  assert.equal(build.status, 0, build.stderr || build.stdout);
  return repo;
}

function run(repo, args) {
  const result = spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr || result.stdout);
  return JSON.parse(result.stdout);
}

test("capped dependency repair persists remaining paths and resume clears them", () => {
  const repo = makeRepo();
  try {
    const service = join(repo, "src/service.ts");
    writeFileSync(service, `${readFileSync(service, "utf8")}\nexport const repairProbe = true;\n`);

    const capped = run(repo, ["delta", "src/service.ts", "--max-dependent-files", "2", "--out", ".agent"]);
    assert.equal(capped[0].truncated, true);
    assert.deepEqual(capped[0].remaining, ["test/service.test.ts"]);

    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      const state = JSON.parse(db.prepare("SELECT value FROM watch_state WHERE key='repair_truncated'").get().value);
      assert.deepEqual(state.remaining, ["test/service.test.ts"]);
      const manifest = JSON.parse(db.prepare("SELECT value FROM generation WHERE key='manifest'").get().value);
      assert.equal(Object.hasOwn(manifest.counts, "files"), false);
      assert.equal(Object.hasOwn(manifest.counts, "symbols"), false);
      assert.equal(typeof manifest.counts.nodes, "number");
      assert.equal(typeof manifest.counts.edges, "number");
    } finally { closeStore(db); }

    const resumed = run(repo, ["delta", "--resume-repair", "--out", ".agent"]);
    assert.deepEqual(resumed, [{ resumed: true, truncated: false, remaining: [] }]);

    const reopened = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      assert.equal(reopened.prepare("SELECT COUNT(*) AS n FROM watch_state WHERE key='repair_truncated'").get().n, 0);
      assert.equal(reopened.prepare("SELECT resolved FROM edges WHERE source='file:src/routes.ts' AND target='file:src/service.ts'").get().resolved, 1);
    } finally { closeStore(reopened); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
