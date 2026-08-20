import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { constants as sqliteConstants } from "node:sqlite";
import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { adoptRebuiltGeneration, closeStore, maintainStore, openStore, resetWatchStateAfterRebuild } from "../src/graph/store-sqlite.mjs";

test("store maintenance checkpoints WAL and bounds applied watcher history", () => {
  const db = openStore(join(mkdtempSync(join(tmpdir(), "blueprint-store-maintenance-")), "graph.db"));
  try {
    const insert = db.prepare("INSERT INTO event_journal(observed_ms,event_kind,path,rename_to,source_clock,applied) VALUES (1,'modify',?,NULL,?,1)");
    for (let index = 0; index < 5; index += 1) insert.run(`src/${index}.ts`, index + 1);
    const result = maintainStore(db, { retainedJournalRows: 2 });
    assert.equal(result.checkpoint, "passive");
    assert.equal(result.pruned, 3);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal").get().n, 2);
  } finally { closeStore(db); }
});

test("rebuild adoption removes stale watcher journal and state atomically", () => {
  const db = openStore(join(mkdtempSync(join(tmpdir(), "blueprint-store-rebuild-reset-")), "graph.db"));
  try {
    db.prepare("INSERT INTO event_journal(observed_ms,event_kind,path,rename_to,source_clock,applied) VALUES (1,'modify','node_modules/noise.js',NULL,1,0)").run();
    db.prepare("INSERT INTO watch_state(key,value) VALUES ('source_clock','1')").run();
    resetWatchStateAfterRebuild(db);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal").get().n, 0);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM watch_state").get().n, 0);
  } finally { closeStore(db); }
});

test("fresh generation bulk-load clears FTS once instead of once per symbol", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-store-bulk-search-"));
  writeFileSync(join(root, "many.mjs"), Array.from({ length: 200 }, (_, index) => `export function symbol${index}() { return ${index}; }`).join("\n"));
  const db = openStore(join(root, "graph.db"));
  let searchDeletes = 0;
  db.setAuthorizer((action, table) => {
    if (action === sqliteConstants.SQLITE_DELETE && table === "symbol_search") searchDeletes += 1;
    return sqliteConstants.SQLITE_OK;
  });
  try {
    adoptRebuiltGeneration(db, buildGraphGeneration(root), { populateState: true });
    assert.equal(searchDeletes, 1);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM symbol_search").get().n, 200);
  } finally { closeStore(db); }
});
