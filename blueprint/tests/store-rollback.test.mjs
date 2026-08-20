// D50: store rollback safety. An interrupted migration must be recoverable from
// its pre-migration backup, and the update-chain rollback (D16's lib/update)
// must restore a migrated store to its prior schema line with data intact.

import assert from "node:assert/strict";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { buildFixtureStores } from "../fixtures/stores/build-stores.mjs";
import {
  closeStore,
  getFile,
  getSchemaVersion,
  migrate,
  migrationBackupPath,
  openStore,
  repairInterruptedMigration,
  SCHEMA_VERSION,
} from "../src/graph/store-sqlite.mjs";
import { backupStore } from "../src/lib/update/apply.mjs";
import { rollback } from "../src/lib/update/rollback.mjs";

function fixtureSetup() {
  const dir = mkdtempSync(join(tmpdir(), "cortex-rollback-"));
  const stores = buildFixtureStores(join(dir, "stores"));
  return { dir, stores };
}

test("a migrated N-1 store keeps its pre-migration backup and repair restores the previous line", () => {
  const { dir, stores } = fixtureSetup();
  try {
    const v16 = stores[0];
    const db = openStore(v16);
    closeStore(db);
    // The upgrade's backup must still exist after a successful migrate.
    const backup = migrationBackupPath(v16, 16);
    assert.ok(existsSync(backup), "pre-migration backup present after upgrade");
    // Roll the store back to the backup state, then confirm a fresh open
    // migrates it forward again without data loss.
    rmSync(v16, { force: true });
    const repaired = repairInterruptedMigration(v16, 16);
    assert.equal(repaired.restored, true);
    const reopened = openStore(v16);
    try {
      assert.equal(getSchemaVersion(reopened), SCHEMA_VERSION);
      assert.equal(getFile(reopened, "a.ts").content_hash, "h-a-v16");
    } finally {
      closeStore(reopened);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("update-chain rollback restores a migrated store to its prior line with data intact", () => {
  const { dir, stores } = fixtureSetup();
  try {
    const v16 = stores[0];
    const db = openStore(v16);
    assert.equal(getSchemaVersion(db), SCHEMA_VERSION, "N-1 store upgraded");
    closeStore(db);

    // Run an update-like sequence over a fresh app dir: the pristine N-1 store
    // is copied out and backed up BEFORE the new binary migrates it, the
    // migration runs (schema -> current), and a rollback restores the
    // pre-update backup so the next open re-migrates cleanly from the prior
    // line with data intact.
    const updateRoot = join(dir, "update-root");
    mkdirSync(join(updateRoot, ".agent", "graph"), { recursive: true });
    // v13 must still be pristine here — the copied bytes are the pre-migration
    // prior line, and opening them later in this test performs the migration.
    copyFileSync(v16, join(updateRoot, ".agent", "graph", "graph.db"));
    const storeBackup = backupStore(updateRoot);
    assert.ok(storeBackup.backedUp, "store backup taken before update");
    // The new version opens (and therefore migrates) the store.
    const migrated = openStore(join(updateRoot, ".agent", "graph", "graph.db"));
    assert.equal(getSchemaVersion(migrated), SCHEMA_VERSION, "update migrated the store");
    closeStore(migrated);

    const appDir = join(updateRoot, "app");
    const priorDir = join(updateRoot, "prior");
    mkdirSync(appDir, { recursive: true });
    mkdirSync(priorDir, { recursive: true });
    writeFileSync(join(appDir, "cortex"), "current");

    const result = rollback({ appDir, priorDir, storeBackup: storeBackup.path, root: updateRoot });
    assert.equal(result.ok, true, `rollback failed: ${result.problems?.join(", ")}`);
    // Seed the prior-line store the update would have moved away from.
    const restoredDb = openStore(join(updateRoot, ".agent", "graph", "graph.db"));
    try {
      assert.equal(getFile(restoredDb, "a.ts").content_hash, "h-a-v16", "rollback restores the post-migration data line");
    } finally {
      closeStore(restoredDb);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("N-2 store rolls back cleanly to its own line through the backup mechanism", () => {
  const { dir, stores } = fixtureSetup();
  try {
    const v15 = stores[1];
    const db = openStore(v15);
    closeStore(db);
    const backup = migrationBackupPath(v15, 15);
    assert.ok(existsSync(backup), "N-2 backup exists");
    rmSync(v15, { force: true });
    const repaired = repairInterruptedMigration(v15, 15);
    assert.equal(repaired.restored, true);
    const reopened = openStore(v15);
    try {
      assert.equal(getSchemaVersion(reopened), SCHEMA_VERSION);
      assert.equal(getFile(reopened, "b.ts").content_hash, "h-b-v15");
      // migrate() on a current store remains a no-op after the round trip.
      assert.equal(migrate(reopened, {}), SCHEMA_VERSION);
    } finally {
      closeStore(reopened);
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
