import assert from "node:assert/strict";
import test from "node:test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { closeStore, maintainStore, openStore, resetWatchStateAfterRebuild } from "../graph/store-sqlite.mjs";

test("store maintenance checkpoints WAL and bounds applied watcher history", () => {
  const db = openStore(join(mkdtempSync(join(tmpdir(), "cortex-store-maintenance-")), "graph.db"));
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
  const db = openStore(join(mkdtempSync(join(tmpdir(), "cortex-store-rebuild-reset-")), "graph.db"));
  try {
    db.prepare("INSERT INTO event_journal(observed_ms,event_kind,path,rename_to,source_clock,applied) VALUES (1,'modify','node_modules/noise.js',NULL,1,0)").run();
    db.prepare("INSERT INTO watch_state(key,value) VALUES ('source_clock','1')").run();
    resetWatchStateAfterRebuild(db);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM event_journal").get().n, 0);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM watch_state").get().n, 0);
  } finally { closeStore(db); }
});
