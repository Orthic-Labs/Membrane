import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { finishEntityRenames, reconcile } from "../watchman/reconcile.mjs";
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

test("reconcile carries entity identity across a detected file rename (BPT-013)", async () => {
  const repo = makeRepo();
  try {
    const db = openStore(join(repo, ".agent/graph/graph.db"));
    try {
      const beforeIds = db.prepare("SELECT id FROM symbols WHERE path='src/service.ts' ORDER BY id").all().map((row) => row.id);
      assert.ok(beforeIds.length > 0);
      mkdirSync(join(repo, "moved"), { recursive: true });
      renameSync(join(repo, "src/service.ts"), join(repo, "moved/service.ts"));
      const snapshot = join(repo, ".agent/graph/watch.snapshot");
      writeFileSync(snapshot, "fixture");
      const result = await reconcile(db, repo, {
        snapshotPath: snapshot,
        adapter: {
          eventsSince: async () => [{ eventKind: "rename", path: "src/service.ts", renameTo: "moved/service.ts", observedMs: 1 }],
          writeSnapshot: async () => {},
        },
      });
      // Unchanged content moving to a new path: every entity (the file node
      // and each of its symbols) must reconcile deterministically, and none
      // may be left unresolved.
      assert.equal(result.renameReconciliation.length, 1);
      const [reconciliation] = result.renameReconciliation;
      assert.equal(reconciliation.oldPath, "src/service.ts");
      assert.equal(reconciliation.newPath, "moved/service.ts");
      assert.deepEqual(reconciliation.unresolved, []);
      assert.deepEqual(reconciliation.aliases.map((alias) => alias.from).sort(), [...beforeIds, "file:src/service.ts"].sort());
      const afterIds = new Set(db.prepare("SELECT id FROM symbols WHERE path='moved/service.ts'").all().map((row) => row.id));
      for (const alias of reconciliation.aliases) {
        assert.ok(alias.to === "file:moved/service.ts" || afterIds.has(alias.to), `unexpected alias target ${alias.to}`);
      }
      // Persisted so a caller that only re-reads the store (not this return
      // value) can still see the reconciliation.
      const stored = JSON.parse(db.prepare("SELECT value FROM watch_state WHERE key='rename_reconciliation'").get().value);
      assert.deepEqual(stored, result.renameReconciliation);
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("reconcile leaves an ambiguous rename unresolved instead of guessing a winner", () => {
  // Direct unit coverage of the wiring seam itself (finishEntityRenames +
  // renameFact), independent of what a real parser happens to produce: two
  // candidates at the same new path share the same qualified name as the one
  // prior entity, so no tier yields a unique winner and the rename must come
  // back typed ambiguous with zero aliases, exactly as reanchorEvidence's own
  // conservative posture requires (see src/graph/reanchor.mjs).
  const before = { nodes: [{ id: "old:dup", kind: "symbol", path: "src/dup.ts", qualifiedName: "dup" }] };
  const after = {
    nodes: [
      { id: "new:dup:a", kind: "symbol", path: "moved/dup.ts", qualifiedName: "dup" },
      { id: "new:dup:b", kind: "symbol", path: "moved/dup.ts", qualifiedName: "dup" },
    ],
  };
  const [result] = finishEntityRenames([{ path: "src/dup.ts", renameTo: "moved/dup.ts" }], before, after);
  assert.deepEqual(result.aliases, []);
  assert.equal(result.unresolved.length, 1);
  assert.equal(result.unresolved[0].state, "ambiguous");
  assert.deepEqual(result.unresolved[0].candidates, ["new:dup:a", "new:dup:b"]);
});

test("reconcile CLI emits machine-readable pending-domain result and hook installer pins node", () => {
  const repo = makeRepo();
  try {
    const git = spawnSync("git", ["init", "-q"], { cwd: repo, encoding: "utf8" });
    assert.equal(git.status, 0, git.stderr);
    const reconcileResult = spawnSync(process.execPath, [CLI, "reconcile", "--json", "--out", ".agent"], { cwd: repo, encoding: "utf8" });
    assert.equal(reconcileResult.status, 0, reconcileResult.stderr);
    const reconcilePayload = JSON.parse(reconcileResult.stdout);
    assert.equal(reconcilePayload.eventGap, 1);
    assert.deepEqual(reconcilePayload.convergence.domainsPending, ["doc"]);
    const hooks = spawnSync(process.execPath, [CLI, "hooks", "install-git"], { cwd: repo, encoding: "utf8" });
    assert.equal(hooks.status, 0, hooks.stderr);
    const result = JSON.parse(hooks.stdout);
    const posixHook = readFileSync(join(result.hooksDir, "post-checkout"), "utf8");
    assert.ok(posixHook.includes(process.execPath));
    assert.ok(posixHook.includes("blueprint-watch.mjs"));
    assert.match(readFileSync(join(result.hooksDir, "post-checkout.cmd"), "utf8"), /blueprint-watch\.mjs/);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
