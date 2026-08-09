import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { stableRead } from "../graph/stable-read.mjs";
import { applyFileDelta, clearDomainPending, DOC_PROVIDER, markDomainPending, readPendingDomains, STRUCTURAL_PROVIDER } from "../graph/delta-store.mjs";
import { computeFullLedger, updateLeafChain } from "../graph/merkle-ledger.mjs";
import { buildGraphGeneration, parseFileFacts } from "../graph/static-provider.mjs";
import { closeStore, listFileMetadata, openStore, openStoreReadOnly } from "../graph/store-sqlite.mjs";
import { CortexRepositoryWorker, RepositoryActor } from "../graph/watchman.mjs";
import { normalizeEvents } from "../watchman/adapter.mjs";
import { reconcile } from "../watchman/reconcile.mjs";

const ROOT = join(import.meta.dirname, "..");
const CLI = join(ROOT, "scripts/cortex.mjs");
const FIXTURE = join(ROOT, "evals/fixture-repos/typescript-commerce");

function makeRepo(prefix = "cortex-freshness-") {
  const repo = mkdtempSync(join(tmpdir(), prefix));
  cpSync(FIXTURE, repo, { recursive: true });
  return repo;
}

function dbPath(repo) {
  return join(repo, ".agent/graph/graph.db");
}

function run(repo, args) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: repo, encoding: "utf8", timeout: 20_000 });
}

function canonicalGraphRows(db) {
  return {
    files: db.prepare("SELECT path,content_hash,language,provider,parse_status,error_node_count FROM files ORDER BY path").all(),
    symbols: db.prepare("SELECT id,kind,labels,name,qualified_name,path,confidence,evidence,extra FROM symbols ORDER BY id").all(),
    edges: db.prepare("SELECT id,kind,source,target,confidence,resolved,specifier,evidence,confidence_tier,extra FROM edges ORDER BY id").all(),
    owners: db.prepare("SELECT fact_id,fact_kind,source_path,source_digest,provider_id,provider_version,freshness_domain,fact_kind_detail FROM fact_owner ORDER BY provider_id,fact_kind,fact_id").all(),
    dependencies: db.prepare("SELECT source_path,dependent_path,reason FROM dependency_index ORDER BY source_path,dependent_path,reason").all(),
  };
}

test("full production build records lexical and Tree-sitter fact ownership", () => {
  const repo = makeRepo("cortex-provider-owner-");
  try {
    const built = run(repo, ["build", "--out", ".agent"]);
    assert.equal(built.status, 0, built.stderr);
    const db = openStore(dbPath(repo));
    try {
      const providers = db.prepare("SELECT DISTINCT provider_id FROM fact_owner WHERE freshness_domain='structural' ORDER BY provider_id").all().map((row) => row.provider_id);
      assert.deepEqual(providers, ["lexical", "treesitter"]);
      assert.ok(db.prepare("SELECT 1 FROM fact_owner WHERE provider_id='treesitter' AND source_path='src/service.ts'").get());
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("same-content watcher event advances durable clocks without changing generation", async () => {
  const repo = makeRepo("cortex-noop-clock-");
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const db = openStore(dbPath(repo));
    const before = db.prepare("SELECT value FROM generation WHERE key='manifest'").get().value;
    closeStore(db);
    const result = await new CortexRepositoryWorker({ root: repo }).ingest("src/service.ts");
    const afterDb = openStore(dbPath(repo));
    try {
      assert.equal(result.applied, true);
      assert.equal(afterDb.prepare("SELECT value FROM watch_state WHERE key='source_clock'").get().value, "1");
      assert.equal(afterDb.prepare("SELECT value FROM watch_state WHERE key='applied_clock'").get().value, "1");
      assert.equal(afterDb.prepare("SELECT applied_clock FROM event_journal WHERE seq=?").get(result.journalSeq).applied_clock, 1);
      assert.equal(afterDb.prepare("SELECT value FROM generation WHERE key='manifest'").get().value, before);
    } finally { closeStore(afterDb); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("same-basename move normalizes to one rename event", () => {
  const root = makeRepo("cortex-rename-normalize-");
  try {
    mkdirSync(join(root, "moved"), { recursive: true });
    const events = normalizeEvents(root, [
      { type: "create", path: join(root, "moved") },
      { type: "delete", path: join(root, "src/service.ts") },
      { type: "create", path: join(root, "moved/service.ts") },
    ], 1000);
    assert.deepEqual(events, [{ eventKind: "rename", path: "src/service.ts", renameTo: "moved/service.ts", observedMs: 1000 }]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("snapshot reconcile journals one atomic rename", async () => {
  const repo = makeRepo("cortex-reconcile-rename-");
  try {
    assert.equal(run(repo, ["build", "--out", ".agent"]).status, 0);
    mkdirSync(join(repo, "moved"), { recursive: true });
    renameSync(join(repo, "src/service.ts"), join(repo, "moved/service.ts"));
    const snapshot = join(repo, ".agent/graph/watch.snapshot");
    writeFileSync(snapshot, "fixture");
    const db = openStore(dbPath(repo));
    try {
      const result = await reconcile(db, repo, {
        snapshotPath: snapshot,
        adapter: {
          eventsSince: async () => [{ eventKind: "rename", path: "src/service.ts", renameTo: "moved/service.ts", observedMs: 1 }],
          writeSnapshot: async () => {},
        },
      });
      assert.equal(result.queued, 1);
      assert.equal(result.applied, 1);
      assert.deepEqual(db.prepare("SELECT event_kind,path,rename_to FROM event_journal").all().map((row) => ({ ...row })), [
        { event_kind: "rename", path: "src/service.ts", rename_to: "moved/service.ts" },
      ]);
      assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='source_clock'").get().value, "1");
      assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='applied_clock'").get().value, "1");
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("snapshot reconcile durably acknowledges same-content event", async () => {
  const repo = makeRepo("cortex-reconcile-noop-");
  try {
    assert.equal(run(repo, ["build", "--out", ".agent"]).status, 0);
    const source = join(repo, "src/service.ts");
    writeFileSync(source, readFileSync(source));
    const snapshot = join(repo, ".agent/graph/watch.snapshot");
    writeFileSync(snapshot, "fixture");
    const db = openStore(dbPath(repo));
    const generation = db.prepare("SELECT value FROM generation WHERE key='manifest'").get().value;
    try {
      const result = await reconcile(db, repo, {
        snapshotPath: snapshot,
        adapter: {
          eventsSince: async () => [{ eventKind: "modify", path: "src/service.ts", observedMs: 1 }],
          writeSnapshot: async () => {},
        },
      });
      assert.equal(result.queued, 1);
      assert.equal(result.applied, 1);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=1 AND applied_clock=1").get().n, 1);
      assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='source_clock'").get().value, "1");
      assert.equal(db.prepare("SELECT value FROM watch_state WHERE key='applied_clock'").get().value, "1");
      assert.equal(db.prepare("SELECT value FROM generation WHERE key='manifest'").get().value, generation);
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("journal rename removes old path, owners, state, and Merkle leaf", async () => {
  const repo = makeRepo("cortex-rename-journal-");
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    mkdirSync(join(repo, "moved"), { recursive: true });
    renameSync(join(repo, "src/service.ts"), join(repo, "moved/service.ts"));
    const result = await new CortexRepositoryWorker({ root: repo }).ingest("src/service.ts", "rename", "moved/service.ts");
    assert.equal(result.applied, true);
    const db = openStore(dbPath(repo));
    try {
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM files WHERE path='src/service.ts'").get().n, 0);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM file_state WHERE path='src/service.ts'").get().n, 0);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM fact_owner WHERE source_path='src/service.ts'").get().n, 0);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM generation_leaf WHERE path='src/service.ts'").get().n, 0);
      assert.equal(db.prepare("SELECT event_kind FROM event_journal WHERE seq=?").get(result.journalSeq).event_kind, "rename");
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM files WHERE path='moved/service.ts'").get().n, 1);
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("document delete removes derived claims, queue rows, owners, state, and leaf", () => {
  const repo = makeRepo("cortex-doc-delete-");
  try {
    mkdirSync(join(repo, "docs"), { recursive: true });
    writeFileSync(join(repo, "docs/SECOND.md"), "# Secondary\n\nThe service is implemented in `src/service.ts`.\n");
    const built = run(repo, ["build", "--out", ".agent"]);
    assert.equal(built.status, 0, built.stderr);
    rmSync(join(repo, "docs/SECOND.md"));
    const delta = run(repo, ["delta", "docs/SECOND.md", "--out", ".agent"]);
    assert.equal(delta.status, 0, delta.stderr);
    const claims = JSON.parse(readFileSync(join(repo, ".agent/claims.json"), "utf8"));
    const queue = JSON.parse(readFileSync(join(repo, ".agent/queue.json"), "utf8"));
    assert.equal(claims.some((claim) => claim.source === "docs/SECOND.md"), false);
    assert.equal((queue.claims ?? []).some((claim) => claim.source === "docs/SECOND.md"), false);
    const db = openStore(dbPath(repo));
    try {
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM fact_owner WHERE source_path='docs/SECOND.md'").get().n, 0);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM file_state WHERE path='docs/SECOND.md'").get().n, 0);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM generation_leaf WHERE path='docs/SECOND.md'").get().n, 0);
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("document rename rehydrates owners, state, leaf, and artifacts at new path", async () => {
  const repo = makeRepo("cortex-doc-rename-");
  const coldRepo = makeRepo("cortex-doc-rename-cold-");
  try {
    mkdirSync(join(repo, "docs"), { recursive: true });
    const content = "# Secondary\n\nThe service is implemented in `src/service.ts`.\n";
    writeFileSync(join(repo, "docs/SECOND.md"), content);
    assert.equal(run(repo, ["build", "--out", ".agent"]).status, 0);
    mkdirSync(join(repo, "docs/moved"), { recursive: true });
    renameSync(join(repo, "docs/SECOND.md"), join(repo, "docs/moved/SECOND.md"));
    mkdirSync(join(coldRepo, "docs/moved"), { recursive: true });
    writeFileSync(join(coldRepo, "docs/moved/SECOND.md"), content);
    assert.equal(run(coldRepo, ["build", "--out", ".agent"]).status, 0);
    const result = await new CortexRepositoryWorker({ root: repo }).ingest("docs/SECOND.md", "rename", "docs/moved/SECOND.md");
    assert.equal(result.applied, true);
    const claims = JSON.parse(readFileSync(join(repo, ".agent/claims.json"), "utf8"));
    const queue = JSON.parse(readFileSync(join(repo, ".agent/queue.json"), "utf8"));
    assert.equal(claims.some((claim) => claim.source === "docs/SECOND.md"), false);
    assert.ok(claims.some((claim) => claim.source === "docs/moved/SECOND.md"));
    assert.equal((queue.claims ?? []).some((claim) => claim.source === "docs/SECOND.md"), false);
    assert.ok((queue.claims ?? []).some((claim) => claim.source === "docs/moved/SECOND.md"));
    const db = openStore(dbPath(repo));
    try {
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM fact_owner WHERE source_path='docs/SECOND.md'").get().n, 0);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM file_state WHERE path='docs/SECOND.md'").get().n, 0);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM generation_leaf WHERE path='docs/SECOND.md'").get().n, 0);
      assert.ok(db.prepare("SELECT 1 FROM fact_owner WHERE source_path='docs/moved/SECOND.md' AND provider_id='doctruth'").get());
      assert.ok(db.prepare("SELECT 1 FROM files WHERE path='docs/moved/SECOND.md'").get());
      assert.ok(db.prepare("SELECT 1 FROM file_state WHERE path='docs/moved/SECOND.md'").get());
      assert.ok(db.prepare("SELECT 1 FROM generation_leaf WHERE path='docs/moved/SECOND.md' AND kind='file'").get());
      const coldDb = openStore(dbPath(coldRepo));
      try {
        const deltaRows = canonicalGraphRows(db);
        const coldRows = canonicalGraphRows(coldDb);
        assert.deepEqual({ ...deltaRows, owners: deltaRows.owners.filter((owner) => owner.provider_id !== "doctruth") }, coldRows);
      }
      finally { closeStore(coldDb); }
    } finally { closeStore(db); }
  } finally {
    rmSync(repo, { recursive: true, force: true });
    rmSync(coldRepo, { recursive: true, force: true });
  }
});

test("document artifact write failure restores every JSON artifact and SQLite row", () => {
  const repo = makeRepo("cortex-doc-atomic-");
  try {
    mkdirSync(join(repo, "docs"), { recursive: true });
    const relativePath = "docs/SECOND.md";
    const absolutePath = join(repo, relativePath);
    writeFileSync(absolutePath, "# Secondary\n\nInitial.\n");
    assert.equal(run(repo, ["build", "--out", ".agent"]).status, 0);
    writeFileSync(absolutePath, "# Secondary\n\nUpdated.\n");
    const read = stableRead(absolutePath);
    const descriptor = {
      absolutePath,
      path: relativePath,
      text: read.bytes.toString("utf8"),
      lines: read.bytes.toString("utf8").split(/\r?\n/),
      contentHash: read.contentDigest.replace(/^xxh128:/, ""),
      size: read.bytes.length,
    };
    const db = openStore(dbPath(repo));
    try {
      const files = listFileMetadata(db).filter((file) => file.path !== relativePath).concat(descriptor);
      const parsed = parseFileFacts(repo, descriptor, { files, symbols: [] });
      const beforeRows = canonicalGraphRows(db);
      const artifactPaths = ["claims.json", "stale.json", "queue.json"].map((name) => join(repo, ".agent", name));
      const beforeArtifacts = artifactPaths.map((path) => readFileSync(path, "utf8"));
      let writes = 0;
      assert.throws(() => applyFileDelta(db, {
        eventKind: "modify",
        path: relativePath,
        provider: DOC_PROVIDER,
        domain: "doc",
        parsed,
        factBatches: [{ provider: STRUCTURAL_PROVIDER, parsed }],
        contentDigest: read.contentDigest,
        size: read.bytes.length,
        mtimeMs: read.statAfter.mtimeMs,
        document: {
          claims: [{ id: "claim.atomic", source: relativePath, line: 3, status: "current", text: "Updated." }],
          codeRefs: [],
          lifecycle: null,
        },
      }, {
        repoRoot: repo,
        outDir: ".agent",
        writeJsonAtomic: (path, value) => {
          writes += 1;
          writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
          if (writes === 2) throw new Error("injected artifact write failure");
        },
      }), /injected artifact write failure/);
      assert.deepEqual(artifactPaths.map((path) => readFileSync(path, "utf8")), beforeArtifacts);
      assert.deepEqual(canonicalGraphRows(db), beforeRows);
    } finally { closeStore(db); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("stableRead retries with a fresh stat-read-stat pair", () => {
  const first = { size: 1, mtimeMs: 1, dev: 1, ino: 1 };
  const changed = { size: 2, mtimeMs: 2, dev: 1, ino: 1 };
  const stable = { size: 3, mtimeMs: 3, dev: 1, ino: 1 };
  const stats = [first, changed, stable, stable];
  const reads = [Buffer.from("x"), Buffer.from("new")];
  const result = stableRead("virtual.ts", {
    statSync: () => stats.shift(),
    readFileSync: () => reads.shift(),
  });
  assert.equal(result.bytes.toString("utf8"), "new");
  assert.equal(result.unstable, false);
  assert.equal(result.statBefore, stable);
  assert.equal(result.statAfter, stable);
});

test("Merkle schema supports indexed parent-local updates", () => {
  const db = openStore(":memory:");
  try {
    const columns = db.prepare("PRAGMA table_info(generation_leaf)").all().map((row) => row.name);
    assert.ok(columns.includes("parent_path"));
    assert.ok(columns.includes("name"));
    const indexes = db.prepare("PRAGMA index_list(generation_leaf)").all().map((row) => row.name);
    assert.ok(indexes.includes("idx_generation_leaf_parent"));
  } finally { closeStore(db); }
});

test("Merkle delta touches only changed file and ancestor directories", () => {
  const db = openStore(":memory:");
  try {
    computeFullLedger(db, [
      { path: "src/a/file.ts", contentDigest: "xxh128:11111111111111111111111111111111" },
      { path: "src/b/other.ts", contentDigest: "xxh128:22222222222222222222222222222222" },
      { path: "test/untouched.ts", contentDigest: "xxh128:33333333333333333333333333333333" },
    ]);
    db.exec("CREATE TEMP TABLE touched(path TEXT); CREATE TEMP TRIGGER leaf_update AFTER UPDATE ON generation_leaf BEGIN INSERT INTO touched VALUES (NEW.path); END; CREATE TEMP TRIGGER leaf_insert AFTER INSERT ON generation_leaf BEGIN INSERT INTO touched VALUES (NEW.path); END; CREATE TEMP TRIGGER leaf_delete AFTER DELETE ON generation_leaf BEGIN INSERT INTO touched VALUES (OLD.path); END;");
    updateLeafChain(db, "src/a/file.ts", "xxh128:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert.deepEqual(db.prepare("SELECT DISTINCT path FROM touched ORDER BY path").all().map((row) => row.path), ["", "src", "src/a", "src/a/file.ts"]);
  } finally { closeStore(db); }
});

test("ordinary actor path cannot call whole-repository source scan", () => {
  const source = readFileSync(join(ROOT, "watchman/repo-actor.mjs"), "utf8");
  assert.doesNotMatch(source, /\bscanSourcesPublic\b/);
});

test("production Tree-sitter modify removes stale facts and equals a cold build", async () => {
  const deltaRepo = makeRepo("cortex-ast-delta-");
  const coldRepo = makeRepo("cortex-ast-cold-");
  try {
    const helper = "export function importedHelper(value: number) { return value + 1; }\n";
    writeFileSync(join(deltaRepo, "src/helper.ts"), helper);
    writeFileSync(join(coldRepo, "src/helper.ts"), helper);
    assert.equal(run(deltaRepo, ["build", "--out", ".agent"]).status, 0);
    const replacement = "import { importedHelper } from './helper.js';\nexport function replacementService(value: number) { return importedHelper(value); }\n";
    writeFileSync(join(deltaRepo, "src/service.ts"), replacement);
    writeFileSync(join(coldRepo, "src/service.ts"), replacement);
    const beforeDb = openStore(dbPath(deltaRepo));
    const staleFacts = beforeDb.prepare("SELECT fact_id,provider_id FROM fact_owner WHERE source_path='src/service.ts' AND fact_kind_detail!='file'").all();
    closeStore(beforeDb);
    await new CortexRepositoryWorker({ root: deltaRepo }).ingest("src/service.ts");
    assert.equal(run(coldRepo, ["build", "--out", ".agent"]).status, 0);
    const deltaDb = openStore(dbPath(deltaRepo));
    const coldDb = openStore(dbPath(coldRepo));
    try {
      for (const fact of staleFacts.filter((fact) => !fact.fact_id.includes("replacementService"))) {
        assert.equal(deltaDb.prepare("SELECT 1 FROM fact_owner WHERE fact_id=? AND provider_id=?").get(fact.fact_id, fact.provider_id), undefined);
      }
      assert.deepEqual(canonicalGraphRows(deltaDb), canonicalGraphRows(coldDb));
    } finally { closeStore(deltaDb); closeStore(coldDb); }
  } finally {
    rmSync(deltaRepo, { recursive: true, force: true });
    rmSync(coldRepo, { recursive: true, force: true });
  }
});

test("10k-file actor delta reads only changed path content", async () => {
  const repo = makeRepo("cortex-read-bound-");
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const db = openStore(dbPath(repo));
    const generationId = db.prepare("SELECT json_extract(value,'$.generationId') AS id FROM generation WHERE key='manifest'").get().id;
    const insert = db.prepare("INSERT INTO files(path,generation_id) VALUES (?,?)");
    db.exec("BEGIN IMMEDIATE");
    try {
      for (let index = 0; index < 10_000; index += 1) insert.run(`virtual/f${index}.ts`, generationId);
      db.exec("COMMIT");
    } catch (error) { db.exec("ROLLBACK"); throw error; }
    closeStore(db);
    writeFileSync(join(repo, "src/config.ts"), "export const boundedRead = true;\n");
    let reads = 0;
    const worker = new CortexRepositoryWorker({
      root: repo,
      readStable: (path) => { reads += 1; return stableRead(path); },
    });
    const result = await worker.ingest("src/config.ts");
    assert.equal(result.applied, true);
    assert.equal(reads, 1);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("pending domains clear selectively after successful work", () => {
  const db = openStore(":memory:");
  try {
    markDomainPending(db, "semantic");
    markDomainPending(db, "doc");
    assert.deepEqual(readPendingDomains(db), ["doc", "semantic"]);
    clearDomainPending(db, "doc");
    assert.deepEqual(readPendingDomains(db), ["semantic"]);
    clearDomainPending(db, "semantic");
    assert.deepEqual(readPendingDomains(db), []);
  } finally { closeStore(db); }
});

test("WAL readers observe old or new complete delta state, never a mixed generation", () => {
  const repo = makeRepo("cortex-wal-delta-");
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    const path = join(repo, "src/service.ts");
    writeFileSync(path, "export function walReplacement() { return true; }\n");
    const read = stableRead(path);
    const descriptor = {
      absolutePath: path,
      path: "src/service.ts",
      text: read.bytes.toString("utf8"),
      lines: read.bytes.toString("utf8").split(/\r?\n/),
      contentHash: read.contentDigest.replace(/^xxh128:/, ""),
      size: read.bytes.length,
    };
    const writer = openStore(dbPath(repo));
    const reader = openStoreReadOnly(dbPath(repo));
    try {
      const files = listFileMetadata(writer).filter((file) => file.path !== "src/service.ts").concat(descriptor);
      const parsed = parseFileFacts(repo, descriptor, { files });
      applyFileDelta(writer, {
        eventKind: "modify",
        path: "src/service.ts",
        parsed,
        provider: STRUCTURAL_PROVIDER,
        contentDigest: read.contentDigest,
        size: read.bytes.length,
        mtimeMs: read.statAfter.mtimeMs,
      }, {
        beforeCommit: () => {
          assert.ok(reader.prepare("SELECT 1 FROM symbols WHERE name='OrderService'").get());
          assert.equal(reader.prepare("SELECT 1 FROM symbols WHERE name='walReplacement'").get(), undefined);
        },
      });
      assert.equal(reader.prepare("SELECT 1 FROM symbols WHERE name='OrderService'").get(), undefined);
      assert.ok(reader.prepare("SELECT 1 FROM symbols WHERE name='walReplacement'").get());
    } finally { closeStore(reader); closeStore(writer); }
  } finally { rmSync(repo, { recursive: true, force: true }); }
});

test("restart drains journal row appended before process death exactly once", async () => {
  const repo = makeRepo("cortex-restart-drain-");
  let actor;
  try {
    buildGraphGeneration(repo, { outDir: ".agent", persist: true });
    writeFileSync(join(repo, "src/service.ts"), "export const restartRecovered = true;\n");
    const child = spawnSync(process.execPath, ["--input-type=module", "-e", `
      import { openStore, closeStore } from ${JSON.stringify(new URL("../graph/store-sqlite.mjs", import.meta.url).href)};
      import { appendWatchEvents } from ${JSON.stringify(new URL("../watchman/repo-actor.mjs", import.meta.url).href)};
      const db = openStore(${JSON.stringify(dbPath(repo))});
      appendWatchEvents(db, [{ eventKind: "modify", path: "src/service.ts", observedMs: 1 }]);
      closeStore(db);
      process.exit(17);
    `], { encoding: "utf8" });
    assert.equal(child.status, 17, child.stderr);
    actor = new RepositoryActor({ root: repo });
    assert.equal(await actor.flush(true), 1);
    assert.equal(await actor.flush(true), 0);
    const db = openStore(dbPath(repo));
    try {
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=1").get().n, 1);
      assert.ok(db.prepare("SELECT 1 FROM symbols WHERE name='restartRecovered'").get());
    } finally { closeStore(db); }
  } finally { await actor?.stop(); rmSync(repo, { recursive: true, force: true }); }
});

test("35k-file no-change reconcile stays below hard five-second ceiling", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-reconcile-35k-"));
  try {
    const db = openStore(":memory:");
    const insertState = db.prepare(`INSERT INTO file_state(path,content_digest,size,mtime_ms,file_identity,last_event_seq,applied_clock)
      VALUES (?,?,?,?,?,NULL,0)`);
    const metadata = [];
    db.exec("BEGIN IMMEDIATE");
    try {
      for (let index = 0; index < 35_000; index += 1) {
        const relativePath = `src/d${String(Math.floor(index / 100)).padStart(3, "0")}/f${String(index % 100).padStart(3, "0")}.ts`;
        const identity = `1:${index + 1}`;
        insertState.run(relativePath, "xxh128:00000000000000000000000000000000", 1, 1, identity);
        metadata.push({ path: relativePath, size: 1, mtimeMs: 1, identity });
      }
      db.exec("COMMIT");
    } catch (error) {
      db.exec("ROLLBACK");
      throw error;
    }
    const snapshot = join(repo, "watch.snapshot");
    writeFileSync(snapshot, "fixture");
    const started = performance.now();
    const result = await reconcile(db, repo, {
      outDir: ".agent",
      snapshotPath: snapshot,
      adapter: { eventsSince: async () => [], writeSnapshot: async () => {} },
      scanSourceMetadata: () => ({ files: metadata }),
    });
    const elapsedMs = performance.now() - started;
    assert.equal(result.queued, 0);
    assert.equal(result.applied, 0);
    assert.ok(elapsedMs < 5000, `35k no-change reconcile took ${elapsedMs.toFixed(1)}ms`);
    console.log(`reconcile-35k-ms ${elapsedMs.toFixed(1)}`);
    closeStore(db);
  } finally { rmSync(repo, { recursive: true, force: true }); }
});
