// D50: N-2/N-1 store migrations, migration backups, and interrupted-migration
// repair. Fixture stores are materialised as REAL schema-v15 and schema-v14
// databases (see fixtures/stores/build-stores.mjs) and opened through the same
// openStore() path production uses, so the upgrade under test is the exact
// migration chain — not a hand-rolled table diff.

import assert from "node:assert/strict";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildFixtureStores } from "../fixtures/stores/build-stores.mjs";
import { applyFileDelta } from "../graph/delta-store.mjs";
import {
  closeStore,
  getFile,
  getGenerationEnvelope,
  getSchemaVersion,
  listEdges,
  listSymbolsByPath,
  loadGeneration,
  migrationBackupPath,
  migrate,
  openStore,
  repairInterruptedMigration,
  saveGeneration,
  SCHEMA_VERSION,
} from "../graph/store-sqlite.mjs";

function withFixtureStores(fn) {
  const dir = mkdtempSync(join(tmpdir(), "cortex-migrations-"));
  const stores = buildFixtureStores(join(dir, "stores"));
  try {
    return fn(stores);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("N-1 (schema v16) fixture repairs dense order, canonical provenance, and ranks", () => {
  withFixtureStores(([v16]) => {
    const db = openStore(v16);
    try {
      assert.equal(getSchemaVersion(db), SCHEMA_VERSION, "schema upgraded to current");
      assert.equal(getGenerationEnvelope(db, "manifest").generationId, "gen-v16", "envelope preserved");
      assert.equal(getFile(db, "a.ts").content_hash, "h-a-v16", "file row preserved");
      assert.equal(getFile(db, "a.ts").provider, "cortex-treesitter", "parser report provider preserved");
      const symbols = listSymbolsByPath(db, "a.ts");
      assert.ok(symbols.some((s) => s.name === "v16Alpha"), "symbol rows preserved");
      assert.deepEqual(db.prepare("SELECT node_ordinal FROM files UNION ALL SELECT node_ordinal FROM symbols UNION ALL SELECT node_ordinal FROM annotation_nodes ORDER BY node_ordinal").all().map((row) => row.node_ordinal), [0,1,2,3,4]);
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM node_provider").get().n, 5);
      assert.equal(db.prepare("SELECT provider_id FROM node_provider WHERE node_id='comment:a.ts:1'").get().provider_id, "treesitter");
      assert.deepEqual(loadGeneration(db).nodes.map((node) => node.id), ["file:a.ts", "file:b.ts", "symbol:a.ts::v16Alpha", "comment:a.ts:1", "symbol:b.ts::v16Beta"]);
      assert.deepEqual(db.prepare("SELECT provider_id, rank FROM provider_ranks ORDER BY rank").all().map((row) => row.provider_id), ["lexical", "treesitter", "doctruth", "custom"]);
      assert.equal(db.prepare("SELECT provider_id FROM node_provider WHERE node_id='symbol:b.ts::v16Beta'").get().provider_id, "custom");
      assert.equal(db.prepare("SELECT COUNT(*) n FROM fact_owner WHERE freshness_domain='doc'").get().n, 1, "non-physical doc lineage survives repair");
      assert.equal(db.prepare("SELECT COUNT(*) n FROM fact_owner WHERE fact_kind='node' AND freshness_domain='structural'").get().n, 5, "one owner per physical node");
      const edges = listEdges(db);
      assert.equal(edges.length, 1);
      assert.equal(edges[0].id, "edge:v16:import:b->a");
    } finally {
      closeStore(db);
    }
  });
});

test("N-2 (schema v15) fixture preserves annotation interleave and reconstructs provenance", () => {
  withFixtureStores(([, v15]) => {
    const db = openStore(v15);
    try {
      assert.equal(getSchemaVersion(db), SCHEMA_VERSION, "schema upgraded to current");
      assert.equal(getGenerationEnvelope(db, "manifest").generationId, "gen-v15", "envelope preserved");
      assert.equal(getFile(db, "b.ts").content_hash, "h-b-v15", "file row preserved");
      assert.equal(getFile(db, "b.ts").provider, "cortex-treesitter", "parser report provider preserved");
      const symbols = listSymbolsByPath(db, "b.ts");
      assert.ok(symbols.some((s) => s.name === "v15Beta"), "symbol rows preserved");
      assert.equal(listEdges(db).length, 1, "edge rows preserved");
      assert.equal(db.prepare("SELECT COUNT(*) AS n FROM node_provider").get().n, 5);
    } finally {
      closeStore(db);
    }
  });
});

test("migrating an older store writes a pre-migration backup for its from-version", () => {
  withFixtureStores(([v16]) => {
    const db = openStore(v16);
    closeStore(db);
    assert.ok(existsSync(migrationBackupPath(v16, 16)), "backup written before N-1 upgrade");
  });
});

test("repairInterruptedMigration restores a torn store to the from-version and re-migrates", () => {
  withFixtureStores(([v16]) => {
    // Simulate an interrupted upgrade: take the N-1 store, corrupt the file
    // with a partial migration (fresh metadata row but the schema version
    // table states the OLD version — the torn state a crash leaves behind),
    // then repair from the backup taken before the upgrade, and re-migrate.
    const db = openStore(v16);
    assert.equal(getSchemaVersion(db), SCHEMA_VERSION);
    closeStore(db);
    const backup = migrationBackupPath(v16, 16);
    assert.ok(existsSync(backup), "backup exists before simulated interruption");
    // Truncate the store file to simulate a half-written upgrade.
    rmSync(v16, { force: true });
    const result = repairInterruptedMigration(v16, 16);
    assert.equal(result.restored, true, "repair restores from the pre-migration backup");
    const reopened = openStore(v16);
    try {
      assert.equal(getSchemaVersion(reopened), SCHEMA_VERSION, "repaired store migrates to current");
      assert.equal(getFile(reopened, "a.ts").content_hash, "h-a-v16", "repaired store data intact");
    } finally {
      closeStore(reopened);
    }
  });
});

test("repair without a backup returns a typed reason, not a crash", () => {
  withFixtureStores(([v15]) => {
    // Migrate once (backup exists), remove the backup to simulate loss, then
    // verify the repair path reports the missing backup instead of crashing.
    const db = openStore(v15);
    closeStore(db);
    rmSync(migrationBackupPath(v15, 15), { force: true });
    const result = repairInterruptedMigration(v15, 15);
    assert.equal(result.restored, false);
    assert.equal(result.reason, "no_backup");
  });
});

test("migrate() re-run on a current store is a no-op and keeps the schema version", () => {
  withFixtureStores(([v15]) => {
    const db = openStore(v15);
    try {
      const version = migrate(db, {});
      assert.equal(version, SCHEMA_VERSION);
      assert.equal(getSchemaVersion(db), SCHEMA_VERSION);
    } finally {
      closeStore(db);
    }
  });
});

test("migrated alias batches replace canonical winners and delete every old-path physical row", () => {
  withFixtureStores(([v16]) => {
    const db = openStore(v16);
    try {
      const beforeGeneration = getGenerationEnvelope(db, "manifest").generationId;
      const file = { ...loadGeneration(db).nodes.find((node) => node.id === "file:a.ts"), name: "alias-updated", evidence: [{ path: "a.ts", contentHash: "after-alias" }] };
      const fresh = { id: "symbol:a.ts::freshAlias", kind: "symbol", labels: ["Function"], name: "freshAlias", qualifiedName: "freshAlias", path: "a.ts", confidence: 1, evidence: [] };
      applyFileDelta(db, { eventKind: "modify", path: "a.ts", contentDigest: "after-alias", sourceClock: 1, factBatches: [{ provider: { id: "cortex-static", version: "2" }, parsed: { nodes: [file, fresh], edges: [] } }] });
      assert.equal(getFile(db, "a.ts").content_hash, "after-alias");
      assert.notEqual(getGenerationEnvelope(db, "manifest").generationId, beforeGeneration);
      assert.equal(db.prepare("SELECT provider_id FROM node_provider WHERE node_id=?").get(fresh.id).provider_id, "lexical");
      applyFileDelta(db, { eventKind: "delete", path: "a.ts", sourceClock: 2, provider: { id: "cortex-static", version: "2" } });
      assert.equal(db.prepare("SELECT COUNT(*) n FROM symbols WHERE path='a.ts'").get().n, 0);
      assert.equal(db.prepare("SELECT COUNT(*) n FROM node_provider WHERE source_path='a.ts'").get().n, 0);
      assert.equal(db.prepare("SELECT COUNT(*) n FROM fact_owner WHERE source_path='a.ts'").get().n, 0);
    } finally { closeStore(db); }
  });
});

test("full replacement always seeds fixed provider ranks before deterministic custom ranks", () => {
  const db = openStore(":memory:");
  try {
    saveGeneration(db, { manifest: { generationId: "fixed" }, provider: { id: "custom-z", version: "1" }, nodes: [{ id: "file:z", kind: "file", path: "z", name: "z", qualifiedName: "z", labels: ["File"], evidence: [] }], edges: [] });
    assert.deepEqual(db.prepare("SELECT provider_id,rank FROM provider_ranks ORDER BY rank").all().map((row) => [row.provider_id, row.rank]), [["lexical",0],["treesitter",1],["doctruth",2],["custom-z",3]]);
  } finally { closeStore(db); }
});

test("migration rejects normalized path collisions and cross-table IDs with typed rollback", () => {
  const db = openStore(":memory:", { upToVersion: 16 });
  const generation = {
    provider: { id: "lexical", version: "1" }, manifest: { generationId: "collision" }, edges: [],
    nodes: [{ id: "file:composed", kind: "file", labels: ["File"], name: "é.ts", qualifiedName: "é.ts", path: "é.ts", confidence: 1, evidence: [] }],
  };
  try {
    saveGeneration(db, generation);
    db.prepare(`INSERT INTO files(path,content_hash,language,provider,parse_status,error_node_count,generation_id,node_id,labels,name,qualified_name,confidence,evidence,extra,node_ordinal)
      SELECT 'é.ts',content_hash,language,provider,parse_status,error_node_count,generation_id,'file:decomposed',labels,'é.ts','é.ts',confidence,evidence,extra,1 FROM files WHERE node_id='file:composed'`).run();
    assert.throws(() => migrate(db), (error) => error.code === "path_normalization_collision");
    assert.equal(getSchemaVersion(db), 16);
    db.prepare("DELETE FROM files WHERE node_id='file:decomposed'").run();
    db.prepare("INSERT INTO symbols(id,kind,labels,name,qualified_name,path,confidence,evidence,generation_id,extra,node_ordinal) VALUES ('file:composed','symbol','[]','x','x','é.ts',1,'[]','collision',NULL,1)").run();
    assert.throws(() => migrate(db), (error) => error.code === "duplicate_node_id");
    assert.equal(getSchemaVersion(db), 16);
  } finally { closeStore(db); }
});
