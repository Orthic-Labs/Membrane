import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { openStore, openStoreReadOnly, closeStore } from "../src/graph/store-sqlite.mjs";

// Regression guard for the production fleet-watcher death: WAL without busy_timeout turns any
// writer/writer race into an instant "database is locked" throw. Serial tests can never catch
// the race itself, so this pins the configuration that makes racers queue instead of die.
test("writable and read-only stores both wait out contention instead of failing instantly", () => {
  const dir = mkdtempSync(join(tmpdir(), "cortex-busy-"));
  const dbPath = join(dir, "graph.db");
  // Writers wait longer than readers on purpose: a blocked reader can retry cheaply,
  // while a blocked writer loses a whole build generation.
  const writer = openStore(dbPath);
  assert.equal(writer.prepare("PRAGMA busy_timeout").get().timeout, 30000);
  const reader = openStoreReadOnly(dbPath);
  assert.equal(reader.prepare("PRAGMA busy_timeout").get().timeout, 5000);
  reader.close();
  closeStore(writer);
});
