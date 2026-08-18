// D16: update rollback — current↔previous with fixture stores.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdtempSync, mkdirSync, renameSync, writeFileSync, readFileSync, rmSync, symlinkSync, realpathSync } from "node:fs";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { applySignedLocalArtifactUpdate, backupStore, stageUpdate, applyStaged, recoverInterruptedSwap, recoverPendingUpdate } from "../src/lib/update/apply.mjs";
import { rollback } from "../src/lib/update/rollback.mjs";
import * as manifestModule from "../src/lib/update/manifest.mjs";
import { isConfinedPath, resolvePhysicalPath } from "../src/lib/path-confinement.mjs";

function fixtureRoot() {
  const root = mkdtempSync(join(tmpdir(), "cortex-rollback-"));
  mkdirSync(join(root, "app"), { recursive: true });
  mkdirSync(join(root, "app-v2"), { recursive: true });
  mkdirSync(join(root, ".agent", "graph"), { recursive: true });
  writeFileSync(join(root, "app", "version.txt"), "v1");
  writeFileSync(join(root, "app-v2", "version.txt"), "v2");
  const db = new DatabaseSync(join(root, ".agent", "graph", "graph.db"));
  db.exec("CREATE TABLE state (value TEXT); INSERT INTO state VALUES ('sqlite-v1')");
  db.close();
  return root;
}

test("apply then rollback restores the prior app version", () => {
  const root = fixtureRoot();
  try {
    const appDir = join(root, "app");
    const priorDir = join(root, "app-prior");
    const staging = stageUpdate({ fromDir: join(root, "app-v2"), toDir: appDir });
    const applied = applyStaged({ stagingDir: staging, appDir, priorDir });
    assert.equal(applied.applied, true);
    assert.equal(readFileSync(join(appDir, "version.txt"), "utf8"), "v2");
    assert.equal(readFileSync(join(priorDir, "version.txt"), "utf8"), "v1");

    const result = rollback({ appDir, priorDir, root });
    assert.equal(result.ok, true);
    assert.equal(readFileSync(join(appDir, "version.txt"), "utf8"), "v1");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rollback with a store backup restores the database", () => {
  const root = fixtureRoot();
  try {
    const backup = backupStore(root);
    assert.equal(backup.backedUp, true);
    const db = new DatabaseSync(join(root, ".agent", "graph", "graph.db")); db.exec("UPDATE state SET value = 'sqlite-v2'"); db.close();
    const result = rollback({ appDir: join(root, "app"), priorDir: join(root, "app"), storeBackup: backup.path, root });
    assert.equal(result.ok, true);
    const restored = new DatabaseSync(join(root, ".agent", "graph", "graph.db"));
    assert.equal(restored.prepare("SELECT value FROM state").get().value, "sqlite-v1"); restored.close();
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rollback fails when no prior version is retained", () => {
  const root = fixtureRoot();
  try {
    const result = rollback({ appDir: join(root, "app"), priorDir: join(root, "no-prior"), root });
    assert.equal(result.ok, false);
    assert.ok(result.problems.length > 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("one prior version is retained after apply", () => {
  const root = fixtureRoot();
  try {
    const appDir = join(root, "app");
    const priorDir = join(root, "app-prior");
    const staging = stageUpdate({ fromDir: join(root, "app-v2"), toDir: appDir });
    applyStaged({ stagingDir: staging, appDir, priorDir });
    // Applying again replaces prior with the previous current.
    const staging2 = stageUpdate({ fromDir: join(root, "app-v2"), toDir: appDir });
    applyStaged({ stagingDir: staging2, appDir, priorDir });
    assert.equal(readFileSync(join(priorDir, "version.txt"), "utf8"), "v2");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("apply recovery restores a journaled last working app", () => {
  const root = fixtureRoot();
  try {
    const appDir = join(root, "app"), priorDir = join(root, "app-prior");
    mkdirSync(priorDir); writeFileSync(join(priorDir, "version.txt"), "v1");
    rmSync(appDir, { recursive: true });
    writeFileSync(join(root, ".cortex-swap-journal.json"), JSON.stringify({ appDir, priorDir }));
    assert.equal(recoverInterruptedSwap({ appDir, priorDir }).recovered, true);
    assert.equal(readFileSync(join(appDir, "version.txt"), "utf8"), "v1");
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("local signed update rejects a signer absent from shipped trust root", () => {
  const root = fixtureRoot();
  try {
    assert.equal(typeof manifestModule.treeDigest, "function");
    const artifact = join(root, "artifact");
    mkdirSync(artifact); writeFileSync(join(artifact, "version.txt"), "v2");
    const keys = generateKeyPairSync("ed25519");
    const manifest = { schemaVersion: 1, channel: "stable", version: "0.3.0", commit: "a".repeat(40), publishedAt: "2026-08-08T00:00:00Z", artifacts: [{ name: "local", platform: process.platform, arch: process.arch, sha256: typeof manifestModule.treeDigest === "function" ? manifestModule.treeDigest(artifact) : "missing" }], signatureAlgorithm: "Ed25519", keyId: "ephemeral", signature: "" };
    if (typeof manifestModule.canonicalManifestPayload !== "function") return;
    manifest.signature = sign(null, Buffer.from(manifestModule.canonicalManifestPayload(manifest)), keys.privateKey).toString("base64");
    const manifestPath = join(root, "manifest.json"); writeFileSync(manifestPath, JSON.stringify(manifest));
    const cli = join(import.meta.dirname, "..", "scripts", "cortex.mjs");
    const apply = spawnSync(process.execPath, [cli, "update", "apply", "--manifest", manifestPath, "--artifact", artifact, "--artifact-name", "local", "--app-dir", join(root, "app"), "--prior-dir", join(root, "prior"), "--repo-root", root, "--json"], { encoding: "utf8" });
    assert.notEqual(apply.status, 0, apply.stderr); assert.equal(JSON.parse(apply.stdout).reason, "untrusted_key_id");
    assert.equal(readFileSync(join(root, "app", "version.txt"), "utf8"), "v1");
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("local update rejects a symlinked app target before trust lookup", () => {
  const root = fixtureRoot(), outside = mkdtempSync(join(tmpdir(), "cortex-outside-"));
  try {
    rmSync(join(root, "app"), { recursive: true });
    try { symlinkSync(outside, join(root, "app"), "junction"); } catch { return; }
    const result = applySignedLocalArtifactUpdate({ manifestPath: "missing", artifactDir: outside, artifactName: "local", appDir: join(root, "app"), priorDir: join(root, "prior"), repoRoot: root });
    assert.equal(result.reason, "update_validation_failed");
    assert.match(result.error, /unsafe update target/);
  } finally { rmSync(root, { recursive: true, force: true }); rmSync(outside, { recursive: true, force: true }); }
});

test("update receipt integrity uses a separate external key namespace", async () => {
  const root = fixtureRoot(), keyDir = mkdtempSync(join(tmpdir(), "cortex-update-keys-"));
  try {
    const integrity = await import("../src/lib/init/state-integrity.mjs");
    const receipt = integrity.sealLocalState(root, { version: "0.3.0", commit: "a".repeat(40) }, { namespace: "update", stateKeyDir: keyDir });
    assert.doesNotThrow(() => integrity.verifyLocalState(root, receipt, { namespace: "update", stateKeyDir: keyDir }));
    receipt.version = "9.9.9";
    assert.throws(() => integrity.verifyLocalState(root, receipt, { namespace: "update", stateKeyDir: keyDir }), /state_integrity_invalid/);
  } finally { rmSync(root, { recursive: true, force: true }); rmSync(keyDir, { recursive: true, force: true }); }
});

test("pending rollback transaction restores both failed app and database", async () => {
  const root = fixtureRoot();
  try {
    const integrity = await import("../src/lib/init/state-integrity.mjs"), appDir = join(root, "app"), priorDir = join(root, "app-prior"), failedApp = join(root, "app.failed"), dbPath = join(root, ".agent", "graph", "graph.db"), failedDb = `${dbPath}.failed`;
    mkdirSync(failedApp); writeFileSync(join(failedApp, "version.txt"), "v2");
    const db = new DatabaseSync(dbPath); db.exec("UPDATE state SET value = 'sqlite-v2'"); db.close(); copyFileSync(dbPath, failedDb);
    const reset = new DatabaseSync(dbPath); reset.exec("UPDATE state SET value = 'sqlite-v1'"); reset.close();
    mkdirSync(join(root, ".agent", "update"), { recursive: true });
    writeFileSync(join(root, ".agent", "update", "transaction.json"), JSON.stringify(integrity.sealLocalState(root, { operation: "rollback", phase: "rollback-prepared", appDir, priorDir, dbPath, failedApp, failedDb, hadDb: true, preAppDigest: manifestModule.treeDigest(failedApp), preDbDigest: createHash("sha256").update(readFileSync(failedDb)).digest("hex") }, { namespace: "update" })));
    assert.equal(recoverPendingUpdate(root).recovered, true);
    assert.equal(readFileSync(join(appDir, "version.txt"), "utf8"), "v2");
    const restored = new DatabaseSync(dbPath); assert.equal(restored.prepare("SELECT value FROM state").get().value, "sqlite-v2"); restored.close();
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("direct rollback recovers a pending apply journal before receipt handling", async () => {
  const root = fixtureRoot();
  try {
    const integrity = await import("../src/lib/init/state-integrity.mjs"), appDir = join(root, "app"), priorDir = join(root, "app-prior"), journalPath = join(root, ".agent", "update", "transaction.json");
    const backup = backupStore(root); renameSync(appDir, priorDir); mkdirSync(appDir); writeFileSync(join(appDir, "version.txt"), "v2");
    const receipt = { channel: "stable", version: "0.2.0", commit: "a".repeat(40), storeBackup: backup.path, storeBackupDigest: createHash("sha256").update(readFileSync(backup.path)).digest("hex") };
    mkdirSync(join(root, ".agent", "update"), { recursive: true });
    writeFileSync(journalPath, JSON.stringify(integrity.sealLocalState(root, { phase: "app-swapped", appDir, priorDir, artifactDigest: manifestModule.treeDigest(appDir), receipt }, { namespace: "update" })));
    assert.equal(rollback({ appDir, priorDir, root }).ok, true);
    assert.equal(existsSync(journalPath), false);
    assert.equal(readFileSync(join(appDir, "version.txt"), "utf8"), "v1");
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("rollback returns a structured failure for missing path arguments instead of throwing", () => {
  const root = fixtureRoot();
  try {
    assert.doesNotThrow(() => rollback({ root }));
    const missingBoth = rollback({ root });
    assert.equal(missingBoth.ok, false);
    assert.match(missingBoth.problems.join(" "), /rollback_missing_argument: appDir/);

    const missingPrior = rollback({ appDir: join(root, "app"), root });
    assert.equal(missingPrior.ok, false);
    assert.match(missingPrior.problems.join(" "), /rollback_missing_argument: priorDir/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rollback rejects a retained app outside its repository root", () => {
  const root = fixtureRoot(), outside = mkdtempSync(join(tmpdir(), "cortex-outside-prior-"));
  try {
    writeFileSync(join(outside, "version.txt"), "outside");
    const result = rollback({ appDir: join(root, "app"), priorDir: outside, root });
    assert.equal(result.ok, false);
    assert.match(result.problems.join(" "), /unsafe rollback target/);
  } finally { rmSync(root, { recursive: true, force: true }); rmSync(outside, { recursive: true, force: true }); }
});

test("apply recovery restores a retired prior before receipt commit", async () => {
  const root = fixtureRoot();
  try {
    const integrity = await import("../src/lib/init/state-integrity.mjs"), appDir = join(root, "app"), priorDir = join(root, "app-prior"), retiredPrior = `${priorDir}.retired`;
    mkdirSync(priorDir); writeFileSync(join(priorDir, "version.txt"), "v0"); renameSync(priorDir, retiredPrior); renameSync(appDir, priorDir);
    mkdirSync(join(root, ".agent", "update"), { recursive: true });
    writeFileSync(join(root, ".agent", "update", "transaction.json"), JSON.stringify(integrity.sealLocalState(root, { operation: "apply", phase: "prepared", appDir, priorDir, retiredPrior, preAppDigest: manifestModule.treeDigest(priorDir), prePriorDigest: manifestModule.treeDigest(retiredPrior), artifactDigest: "a".repeat(64) }, { namespace: "update" })));
    assert.equal(recoverPendingUpdate(root).recovered, true);
    assert.equal(readFileSync(join(appDir, "version.txt"), "utf8"), "v1");
    assert.equal(readFileSync(join(priorDir, "version.txt"), "utf8"), "v0");
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("prepared apply recovery keeps pre-state before retiring prior", async () => {
  const root = fixtureRoot();
  try {
    const integrity = await import("../src/lib/init/state-integrity.mjs"), appDir = join(root, "app"), priorDir = join(root, "app-prior"), next = `${appDir}.next`;
    mkdirSync(priorDir); writeFileSync(join(priorDir, "version.txt"), "v0"); mkdirSync(next); writeFileSync(join(next, "version.txt"), "v2");
    mkdirSync(join(root, ".agent", "update"), { recursive: true });
    writeFileSync(join(root, ".agent", "update", "transaction.json"), JSON.stringify(integrity.sealLocalState(root, { operation: "apply", phase: "prepared", appDir, priorDir, next, retiredPrior: `${priorDir}.retired`, preAppDigest: manifestModule.treeDigest(appDir), prePriorDigest: manifestModule.treeDigest(priorDir), artifactDigest: manifestModule.treeDigest(next) }, { namespace: "update" })));
    assert.equal(recoverPendingUpdate(root).recovered, true);
    assert.equal(existsSync(next), false); assert.equal(readFileSync(join(appDir, "version.txt"), "utf8"), "v1"); assert.equal(readFileSync(join(priorDir, "version.txt"), "utf8"), "v0");
    const staged = stageUpdate({ fromDir: join(root, "app-v2"), toDir: appDir }); applyStaged({ stagingDir: staged, appDir, priorDir });
    assert.equal(rollback({ appDir, priorDir, root }).ok, true);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("path confinement follows a symlinked repo root but rejects symlink escape", () => {
  const base = mkdtempSync(join(tmpdir(), "cortex-confine-"));
  try {
    const real = join(base, "real");
    mkdirSync(join(real, "sub"), { recursive: true });
    writeFileSync(join(real, "sub", "file.txt"), "x");
    const link = join(base, "link");
    symlinkSync(real, link, "dir");

    // A legitimately symlinked repo root must still confine its own paths.
    assert.equal(isConfinedPath(link, join(link, "sub", "file.txt")), true);
    assert.equal(isConfinedPath(link, join(link, "sub", "new.txt")), true);
    // The canonical root is accepted through the symlink, and the physical
    // resolution returns the real (canonicalised) location.
    assert.equal(resolvePhysicalPath(join(link, "sub", "file.txt")), realpathSync(join(real, "sub", "file.txt")));

    // An internal symlink stays confined when it resolves inside the root.
    symlinkSync(join(real, "sub"), join(real, "alias"), "dir");
    assert.equal(isConfinedPath(real, join(real, "alias", "file.txt")), true);

    // A symlink (or junction) target resolving outside the root is rejected.
    const outside = mkdtempSync(join(tmpdir(), "cortex-confine-out-"));
    try {
      symlinkSync(outside, join(real, "escape"), "dir");
      assert.equal(isConfinedPath(real, join(real, "escape", "secret.txt")), false);
      assert.equal(isConfinedPath(link, join(link, "escape", "secret.txt")), false);
    } finally {
      rmSync(outside, { recursive: true, force: true });
    }

    // Non-existent tail under the root remains confined; a path outside the
    // root and a relative path are not.
    assert.equal(isConfinedPath(real, join(real, "a", "b", "c.txt")), true);
    assert.equal(isConfinedPath(real, join(base, "elsewhere.txt")), false);
    assert.equal(isConfinedPath(real, "relative.txt"), false);
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
});
