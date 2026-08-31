import assert from "node:assert/strict";
import test from "node:test";
import { closeStore, openStore } from "../src/graph/store-sqlite.mjs";
import { createSnapshot, getSnapshot, listSnapshots, changesSince, changesSinceReference } from "../src/graph/snapshots.mjs";

function seeded() {
  const db = openStore(":memory:");
  db.prepare("INSERT INTO generation(key,value) VALUES (?,?)").run("manifest", JSON.stringify({ complete: true, generationId: "g1", manifestDigest: "m1" }));
  db.prepare("INSERT INTO generation(key,value) VALUES (?,?)").run("sourceObservation", JSON.stringify({ head: "h1", dirty: false }));
  db.prepare("INSERT INTO generation_leaf(path,kind,digest) VALUES (?,?,?)").run("a.ts", "file", "d1");
  return db;
}

test("named snapshots are idempotent and changes are deterministic/bounded", () => {
  const db = seeded();
  try {
    const first = createSnapshot(db, "base", "/tmp", { current: { head: "h1", dirty: false } });
    assert.equal(first.idempotent, false);
    assert.equal(createSnapshot(db, "base", "/tmp", { current: { head: "h1", dirty: false } }).idempotent, true);
    db.prepare("UPDATE generation SET value=? WHERE key='manifest'").run(JSON.stringify({ complete: true, generationId: "g2", manifestDigest: "m2" }));
    db.prepare("UPDATE generation_leaf SET digest=? WHERE path=?").run("d2", "a.ts");
    db.prepare("INSERT INTO generation_leaf(path,kind,digest) VALUES (?,?,?)").run("b.ts", "file", "d3");
    const delta = changesSince(db, "base", { limit: 1 });
    assert.deepEqual(delta.changes, [{ path: "a.ts", kind: "modified" }]);
    assert.deepEqual(delta.base.sourceObservation, { head: "h1", dirty: false });
    assert.deepEqual(delta.head.sourceObservation, { head: "h1", dirty: false });
    assert.equal(delta.receipt.truncated, true);
    assert.equal(getSnapshot(db, "base").generationId, "g1");
    assert.equal(listSnapshots(db).length, 1);
    const semantic = changesSinceReference(db, "/tmp", { generation: "g1" });
    assert.equal(semantic.authority, "history_reference_only");
    assert.equal(semantic.currentTruth.generationId, "g2");
    assert.equal(semantic.changes[0].path, "a.ts");
    assert.ok(semantic.semanticDelta);
    assert.equal(semantic.schemaVersion, 2);
  } finally { closeStore(db); }
});

test("conflicting identity is rejected", () => {
  const db = seeded();
  try {
    createSnapshot(db, "base", "/tmp", { current: { head: "h1", dirty: false } });
    assert.throws(() => createSnapshot(db, "base", "/tmp", { current: { head: "h2", dirty: false } }), { code: "snapshot_stale" });
  } finally { closeStore(db); }
});
