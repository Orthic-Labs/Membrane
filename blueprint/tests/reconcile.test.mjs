import assert from "node:assert/strict";
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { reconcile } from "../watchman/reconcile.mjs";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/blueprint.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function makeRepo() {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-reconcile-"));
  cpSync(FIXTURE, repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return repo;
}

test("reconcile applies exactly changed files and leaves unrelated facts intact", async () => {
  const repo = makeRepo();
  try {
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      const unrelated = db.prepare("SELECT path, content_hash FROM files WHERE path='src/config.ts'").all();
      for (const path of ["src/service.ts", "src/routes.ts", "src/store.ts"]) {
        const absolute = join(repo, path);
        writeFileSync(absolute, `${readFileSync(absolute, "utf8")}\nexport const reconcileChange = true;\n`);
      }
      const result = await reconcile(db, repo, { outDir: ".agent" });
      assert.deepEqual(result.changed, ["src/routes.ts", "src/service.ts", "src/store.ts"]);
      assert.equal(result.applied, 3);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=1").get().n, 3);
      assert.deepEqual(db.prepare("SELECT path, content_hash FROM files WHERE path='src/config.ts'").all(), unrelated);
      assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='event_gap'").get().value, "0");
      assert.ok(db.prepare("SELECT value FROM watch_state WHERE key='last_reconcile_ms'").get());
      assert.equal(existsSync(join(repo, ".agent/graph/watch.snapshot")), false);
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("reconcile clears an overflow gap only after applying authority diff", async () => {
  const repo = makeRepo();
  try {
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('event_gap','1') ON CONFLICT(key) DO UPDATE SET value='1'").run();
      writeFileSync(join(repo, "src/service.ts"), `${readFileSync(join(repo, "src/service.ts"), "utf8")}\nexport const recovered = true;\n`);
      const result = await reconcile(db, repo, { outDir: ".agent" });
      assert.deepEqual(result.changed, ["src/service.ts"]);
      assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='event_gap'").get().value, "0");
      assert.equal(existsSync(join(repo, ".agent/graph/watch.snapshot")), true);
      writeFileSync(join(repo, "src/config.ts"), `${readFileSync(join(repo, "src/config.ts"), "utf8")}\nexport const snapshotDiff = true;\n`);
      writeFileSync(join(repo, "src/added.ts"), "export const addedBySnapshotDiff = true;\n");
      rmSync(join(repo, "src/store.ts"));
      const fastResult = await reconcile(db, repo, { outDir: ".agent" });
      assert.deepEqual(fastResult.changed, ["src/config.ts"]);
      assert.deepEqual(fastResult.added, ["src/added.ts"]);
      assert.deepEqual(fastResult.removed, ["src/store.ts"]);
      assert.equal(fastResult.applied, 3);
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("reconcile CLI emits machine-readable result and hook installer pins node", () => {
  const repo = makeRepo();
  try {
    const git = spawnSync("git", ["init", "-q"], { cwd: repo, encoding: "utf8" });
    assert.equal(git.status, 0, git.stderr);
    const reconcileResult = spawnSync(process.execPath, [CLI, "reconcile", "--json", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
    assert.equal(reconcileResult.status, 0, reconcileResult.stderr);
    assert.equal(JSON.parse(reconcileResult.stdout).eventGap, 0);
    const hooks = spawnSync(process.execPath, [CLI, "hooks", "install-git"], { cwd: repo, encoding: "utf8" });
    assert.equal(hooks.status, 0, hooks.stderr);
    const result = JSON.parse(hooks.stdout);
    const posixHook = readFileSync(join(result.hooksDir, "post-checkout"), "utf8");
    assert.ok(posixHook.includes(process.execPath));
    assert.ok(posixHook.includes("blueprint-watch.mjs"));
    assert.match(readFileSync(join(result.hooksDir, "post-checkout.cmd"), "utf8"), /blueprint-watch\.mjs/);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
