// D16: atomic update application. Backs up the database before any
// incompatible migration, performs atomic app replacement, and retains one
// prior version plus a compatible store backup.

import { closeSync, existsSync, fsyncSync, mkdirSync, openSync, copyFileSync, readFileSync, rmSync, readdirSync, lstatSync, renameSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { fileURLToPath } from "node:url";
import { canonicalManifestPayload, loadTrustedUpdateKeys, loadUpdateManifest, rejectDowngrade, treeDigest, verifyArtifactChecksum, verifySignedManifest } from "./manifest.mjs";
import { sealLocalState, verifyLocalState } from "../init/state-integrity.mjs";
import { isConfinedPath } from "../path-confinement.mjs";

export function backupStore(root, { outDir = ".agent" } = {}) {
  const dbPath = join(root, outDir, "graph", "graph.db");
  if (!existsSync(dbPath)) return { backedUp: false, path: null };
  const backupDir = join(root, outDir, "backups");
  mkdirSync(backupDir, { recursive: true });
  const target = join(backupDir, `graph.db.before-update-${Date.now()}`);
  let db;
  try {
    db = new DatabaseSync(dbPath);
    db.exec(`VACUUM INTO '${target.replaceAll("'", "''")}'`);
    return { backedUp: true, path: target };
  } catch (error) { return { backedUp: false, path: null, reason: "store_backup_failed", error: String(error.message ?? error) }; }
  finally { db?.close(); }
}

export function stageUpdate({ fromDir, toDir, backup }) {
  // Atomic app replacement: copy the new payload beside the old, then swap.
  const stagingDir = `${toDir}.staging-${Date.now()}`;
  mkdirSync(stagingDir, { recursive: true });
  copyRecursive(fromDir, stagingDir);
  return stagingDir;
}

function swapJournalPath(appDir) { return join(dirname(appDir), ".blueprint-swap-journal.json"); }
export function durableJson(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  const temp = `${path}.tmp-${process.pid}-${Date.now()}`;
  writeFileSync(temp, JSON.stringify(value));
  let fd; try { fd = openSync(temp, "r+"); fsyncSync(fd); } finally { if (fd !== undefined) closeSync(fd); fd = undefined; }
  renameSync(temp, path);
  try { fd = openSync(dirname(path), "r"); fsyncSync(fd); } catch {} finally { if (fd !== undefined) closeSync(fd); }
}
export function recoverInterruptedSwap({ appDir, priorDir, journalPath = swapJournalPath(appDir) }) {
  if (!existsSync(journalPath)) return { recovered: false };
  try { JSON.parse(readFileSync(journalPath, "utf8")); } catch { throw new Error("update_journal_corrupt"); }
  if (!existsSync(appDir) && existsSync(priorDir)) renameSync(priorDir, appDir);
  if (!existsSync(appDir)) throw new Error("update_journal_no_working_app");
  rmSync(journalPath, { force: true });
  return { recovered: true };
}

export function applyStaged({ stagingDir, appDir, priorDir, journalPath = swapJournalPath(appDir), repoRoot = null, journal = null, keepJournal = false }) {
  recoverInterruptedSwap({ appDir, priorDir, journalPath });
  const next = `${appDir}.next-${Date.now()}`;
  const retiredPrior = existsSync(priorDir) ? `${priorDir}.retired-${Date.now()}` : null;
  copyRecursive(stagingDir, next);
  const transaction = { schemaVersion: 1, phase: "prepared", appDir, priorDir, next, retiredPrior, preAppDigest: existsSync(appDir) ? treeDigest(appDir) : null, ...journal };
  const save = (value) => durableJson(journalPath, repoRoot ? sealLocalState(repoRoot, value, { namespace: "update" }) : value);
  save(transaction);
  let movedCurrent = false;
  try {
    if (retiredPrior) renameSync(priorDir, retiredPrior);
    if (existsSync(appDir)) { renameSync(appDir, priorDir); movedCurrent = true; }
    renameSync(next, appDir);
    save({ ...transaction, phase: "app-swapped" });
  } catch (error) {
    if (movedCurrent && !existsSync(appDir) && existsSync(priorDir)) renameSync(priorDir, appDir);
    if (retiredPrior && existsSync(retiredPrior) && !existsSync(priorDir)) renameSync(retiredPrior, priorDir);
    if (existsSync(appDir)) rmSync(journalPath, { force: true });
    rmSync(next, { recursive: true, force: true });
    throw error;
  }
  rmSync(stagingDir, { recursive: true, force: true });
  if (!keepJournal) { if (retiredPrior) rmSync(retiredPrior, { recursive: true, force: true }); rmSync(journalPath, { force: true }); }
  return { applied: true, priorDir, retiredPrior };
}

function fileDigest(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }

function restoreRollbackPair(journal) {
  const restore = (active, failed, expected, directory) => {
    if (!expected) return;
    if (existsSync(failed) && (directory ? treeDigest(failed) : fileDigest(failed)) !== expected) throw new Error("update_journal_digest_mismatch");
    if (!existsSync(failed)) {
      if (!existsSync(active) || (directory ? treeDigest(active) : fileDigest(active)) !== expected) throw new Error("update_journal_digest_mismatch");
      return;
    }
    const hold = `${active}.recovery-${Date.now()}`;
    if (existsSync(active)) renameSync(active, hold);
    try { renameSync(failed, active); }
    catch (error) { if (existsSync(hold) && !existsSync(active)) renameSync(hold, active); throw error; }
    if ((directory ? treeDigest(active) : fileDigest(active)) !== expected) throw new Error("update_journal_digest_mismatch");
    rmSync(hold, { recursive: directory, force: true });
  };
  restore(journal.appDir, journal.failedApp, journal.preAppDigest, true);
  restore(journal.dbPath, journal.failedDb, journal.preDbDigest, false);
}

export function recoverPendingUpdate(repoRoot) {
  const journalPath = join(repoRoot, ".agent", "update", "transaction.json");
  if (!existsSync(journalPath)) return { recovered: false };
  const journal = verifyLocalState(repoRoot, JSON.parse(readFileSync(journalPath, "utf8")), { namespace: "update" });
  for (const path of [journal.appDir, journal.priorDir, journal.dbPath, journal.failedApp, journal.failedDb, journal.retiredPrior, journal.receiptPath, journal.next].filter(Boolean)) {
    if (!isConfinedUpdatePath(repoRoot, path)) throw new Error("unsafe update journal target");
  }
  if (journal.operation === "rollback" || journal.phase === "rollback-prepared") {
    if (!/^[a-f0-9]{64}$/.test(journal.preAppDigest ?? "") || (journal.hadDb && !/^[a-f0-9]{64}$/.test(journal.preDbDigest ?? ""))) throw new Error("update_journal_digest_missing");
    if (journal.phase === "committed") {
      if (treeDigest(journal.appDir) !== journal.targetAppDigest || (journal.targetDbDigest && fileDigest(journal.dbPath) !== journal.targetDbDigest)) throw new Error("update_journal_digest_mismatch");
      rmSync(journal.failedApp, { recursive: true, force: true }); rmSync(journal.failedDb, { force: true }); if (journal.receiptPath) rmSync(journal.receiptPath, { force: true }); rmSync(journalPath, { force: true });
      return { recovered: true, committed: true };
    }
    restoreRollbackPair(journal);
    rmSync(journalPath, { force: true });
    return { recovered: true, rolledBack: true };
  }
  if (journal.artifactDigest && existsSync(journal.appDir) && treeDigest(journal.appDir) === journal.artifactDigest && journal.receipt) {
    durableJson(join(repoRoot, ".agent", "update", "accepted-manifest.json"), sealLocalState(repoRoot, journal.receipt, { namespace: "update" }));
    if (journal.retiredPrior) rmSync(journal.retiredPrior, { recursive: true, force: true });
    rmSync(journalPath, { force: true });
    return { recovered: true, finalized: true };
  }
  if (journal.operation === "apply" && /^[a-f0-9]{64}$/.test(journal.preAppDigest ?? "")) {
    if (!existsSync(journal.appDir) || treeDigest(journal.appDir) !== journal.preAppDigest) {
      if (!existsSync(journal.priorDir) || treeDigest(journal.priorDir) !== journal.preAppDigest) throw new Error("update_journal_digest_mismatch");
      const abandoned = `${journal.appDir}.aborted-${Date.now()}`;
      if (existsSync(journal.appDir)) renameSync(journal.appDir, abandoned);
      renameSync(journal.priorDir, journal.appDir); rmSync(abandoned, { recursive: true, force: true });
    }
    if (journal.retiredPrior) {
      if (!/^[a-f0-9]{64}$/.test(journal.prePriorDigest ?? "")) throw new Error("update_journal_digest_mismatch");
      if (existsSync(journal.retiredPrior)) {
        if (treeDigest(journal.retiredPrior) !== journal.prePriorDigest || existsSync(journal.priorDir)) throw new Error("update_journal_digest_mismatch");
        renameSync(journal.retiredPrior, journal.priorDir);
      } else if (!existsSync(journal.priorDir) || treeDigest(journal.priorDir) !== journal.prePriorDigest) throw new Error("update_journal_digest_mismatch");
    }
    if (journal.next) rmSync(journal.next, { recursive: true, force: true }); rmSync(journalPath, { force: true });
    return { recovered: true, aborted: true };
  }
  const result = recoverInterruptedSwap({ appDir: journal.appDir, priorDir: journal.priorDir, journalPath });
  return { ...result, phase: journal.phase };
}

export const isConfinedUpdatePath = isConfinedPath;

export function applySignedLocalArtifactUpdate({ manifestPath, artifactDir, artifactName, appDir, priorDir, repoRoot } = {}) {
  const required = { manifestPath, artifactDir, artifactName, appDir, priorDir, repoRoot };
  const missing = Object.entries(required).filter(([, value]) => !value).map(([name]) => name);
  if (missing.length) return { ok: false, reason: "missing_update_inputs", missing };
  try {
    if (!isConfinedUpdatePath(repoRoot, appDir) || !isConfinedUpdatePath(repoRoot, priorDir) || resolve(appDir) === resolve(priorDir)) throw new Error("unsafe update target");
    recoverPendingUpdate(repoRoot);
    let trustedKeys;
    try { trustedKeys = loadTrustedUpdateKeys(); } catch { return { ok: false, reason: "update_trust_root_corrupt" }; }
    if (!trustedKeys) return { ok: false, reason: "update_trust_root_missing" };
    const manifest = loadUpdateManifest(manifestPath);
    const signature = verifySignedManifest(manifest, { trustedKeys });
    if (!signature.ok) return signature;
    const current = JSON.parse(readFileSync(join(appDir, "package.json"), "utf8"));
    const artifact = (manifest.artifacts ?? []).find((entry) => entry.name === artifactName);
    if (!artifact || artifact.platform !== process.platform || artifact.arch !== process.arch) return { ok: false, reason: "artifact_platform_mismatch" };
    const candidatePackage = JSON.parse(readFileSync(join(artifactDir, "package.json"), "utf8"));
    if (!artifact.packageName || current.name !== artifact.packageName || candidatePackage.name !== artifact.packageName || candidatePackage.version !== manifest.version) return { ok: false, reason: "artifact_package_identity_mismatch" };
    const digest = treeDigest(artifactDir);
    const checksum = verifyArtifactChecksum(manifest, artifactName, digest);
    if (!checksum.ok) return checksum;
    const version = rejectDowngrade(manifest.version, current.version);
    if (!version.ok) return version;
    const receiptPath = join(repoRoot, ".agent", "update", "accepted-manifest.json");
    if (existsSync(receiptPath)) {
      let receipt;
      try { receipt = verifyLocalState(repoRoot, JSON.parse(readFileSync(receiptPath, "utf8")), { namespace: "update" }); } catch { return { ok: false, reason: "update_receipt_corrupt" }; }
      if (receipt.channel === manifest.channel && (receipt.version === manifest.version || receipt.commit === manifest.commit)) return { ok: false, reason: "replay_manifest" };
    }
    const dbPath = join(repoRoot, ".agent", "graph", "graph.db");
    const store = backupStore(repoRoot);
    if (existsSync(dbPath) && !store.backedUp) return { ok: false, reason: "store_backup_failed", error: store.error };
    const stagingDir = stageUpdate({ fromDir: artifactDir, toDir: appDir });
    if (treeDigest(stagingDir) !== digest) { rmSync(stagingDir, { recursive: true, force: true }); return { ok: false, reason: "staging_checksum_mismatch" }; }
    const preAppDigest = treeDigest(appDir), prePriorDigest = existsSync(priorDir) ? treeDigest(priorDir) : null;
    const receipt = { digest: createHash("sha256").update(canonicalManifestPayload(manifest)).digest("hex"), channel: manifest.channel, version: manifest.version, commit: manifest.commit, currentPackageVersion: manifest.version, priorPackageVersion: current.version, currentAppDigest: digest, priorAppDigest: preAppDigest, storeBackup: store.path, storeBackupDigest: store.path ? fileDigest(store.path) : null, store: store.backedUp ? "backed_up" : "no_store" };
    const journalPath = join(repoRoot, ".agent", "update", "transaction.json");
    const result = applyStaged({ stagingDir, appDir, priorDir, repoRoot, journalPath, keepJournal: true, journal: { operation: "apply", dbPath, storeBackup: store.path, storeBackupDigest: receipt.storeBackupDigest, artifactDigest: digest, preAppDigest, prePriorDigest, receipt } });
    durableJson(receiptPath, sealLocalState(repoRoot, receipt, { namespace: "update" }));
    if (result.retiredPrior) rmSync(result.retiredPrior, { recursive: true, force: true });
    rmSync(journalPath, { force: true });
    return { ok: true, manifest, storeBackup: store.path, ...result };
  } catch (error) { return { ok: false, reason: "update_validation_failed", error: String(error.message ?? error) }; }
}

function copyRecursive(source, target) {
  mkdirSync(target, { recursive: true });
  for (const name of readdirSync(source)) {
    const src = join(source, name);
    const dst = join(target, name);
    const stat = lstatSync(src);
    if (stat.isSymbolicLink() || (!stat.isDirectory() && !stat.isFile())) throw new Error(`unsafe update entry: ${src}`);
    if (stat.isDirectory()) copyRecursive(src, dst);
    else copyFileSync(src, dst);
  }
}
