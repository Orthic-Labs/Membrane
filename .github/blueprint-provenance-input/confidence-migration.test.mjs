import assert from "node:assert/strict";
import test from "node:test";
import { DatabaseSync } from "node:sqlite";
import { migrateNullableFactConfidence } from "../src/graph/confidence-migration.mjs";

function fixture() {
  const db = new DatabaseSync(":memory:");
  db.exec(`
    CREATE TABLE symbols (id TEXT PRIMARY KEY, confidence REAL NOT NULL, extra TEXT, node_ordinal INTEGER);
    CREATE TABLE edges (id TEXT PRIMARY KEY, confidence REAL NOT NULL, extra TEXT);
    CREATE INDEX idx_symbols_confidence_fixture ON symbols(node_ordinal);
    CREATE INDEX idx_edges_confidence_fixture ON edges(id);
    CREATE TABLE audit (id TEXT);
    CREATE TRIGGER symbol_confidence_audit AFTER INSERT ON symbols BEGIN INSERT INTO audit VALUES (new.id); END;
    INSERT INTO symbols(rowid,id,confidence,extra,node_ordinal) VALUES (7,'legacy',0.78,'{"provider":"fixture"}',2);
    INSERT INTO edges(rowid,id,confidence,extra) VALUES (11,'legacy-edge',1,'{"generationId":"old"}');
  `);
  return db;
}

function transaction(db, work) {
  db.exec("BEGIN");
  try { work(); db.exec("COMMIT"); }
  catch (error) { db.exec("ROLLBACK"); throw error; }
}

test("nullable-confidence migration preserves data, rowids, indexes and triggers", () => {
  const db = fixture();
  try {
    const before = ["symbols", "edges"].map((table) => db.prepare(`SELECT rowid,* FROM ${table}`).all());
    transaction(db, () => migrateNullableFactConfidence(db));
    for (const [index, table] of ["symbols", "edges"].entries()) {
      assert.deepEqual(db.prepare(`SELECT rowid,* FROM ${table}`).all(), before[index]);
      assert.equal(db.prepare(`PRAGMA table_info(${table})`).all().find((column) => column.name === "confidence").notnull, 0);
    }
    db.prepare("INSERT INTO symbols(id,confidence,node_ordinal) VALUES (?,?,?)").run("authoritative", null, 3);
    db.prepare("INSERT INTO edges(id,confidence) VALUES (?,?)").run("authoritative-edge", null);
    assert.equal(db.prepare("SELECT confidence FROM symbols WHERE id='authoritative'").get().confidence, null);
    assert.equal(db.prepare("SELECT COUNT(*) n FROM audit WHERE id='authoritative'").get().n, 1);
    assert.equal(db.prepare("SELECT COUNT(*) n FROM sqlite_master WHERE name IN ('idx_symbols_confidence_fixture','idx_edges_confidence_fixture')").get().n, 2);
    assert.deepEqual(db.prepare("PRAGMA foreign_key_check").all(), []);
  } finally { db.close(); }
});

test("nullable-confidence migration is idempotent", () => {
  const db = fixture();
  try {
    transaction(db, () => migrateNullableFactConfidence(db));
    const before = db.prepare("SELECT type,name,sql FROM sqlite_master ORDER BY name").all();
    transaction(db, () => migrateNullableFactConfidence(db));
    assert.deepEqual(db.prepare("SELECT type,name,sql FROM sqlite_master ORDER BY name").all(), before);
  } finally { db.close(); }
});

test("failure on the second table rolls back the first table and its indexes", () => {
  const db = fixture();
  try {
    db.exec("CREATE TABLE edges_nullable_confidence_v19 (collision TEXT)");
    const before = db.prepare("SELECT type,name,sql FROM sqlite_master ORDER BY name").all();
    assert.throws(() => transaction(db, () => migrateNullableFactConfidence(db)), /already exists/);
    assert.deepEqual(db.prepare("SELECT type,name,sql FROM sqlite_master ORDER BY name").all(), before);
    assert.equal(db.prepare("SELECT rowid FROM symbols WHERE id='legacy'").get().rowid, 7);
    assert.throws(() => db.prepare("INSERT INTO symbols(id,confidence) VALUES (?,?)").run("still-required", null), /NOT NULL/);
  } finally { db.close(); }
});

test("unexpected schema aborts with a typed migration failure", () => {
  const db = new DatabaseSync(":memory:");
  try {
    db.exec("CREATE TABLE symbols (id TEXT PRIMARY KEY)");
    assert.throws(() => transaction(db, () => migrateNullableFactConfidence(db)), { code: "confidence_migration_schema_mismatch" });
  } finally { db.close(); }
});
