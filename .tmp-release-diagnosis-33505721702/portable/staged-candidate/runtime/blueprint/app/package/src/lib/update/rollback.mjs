// D16: receipt-bound rollback restores one verified app/store pair.

import { copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync } from "node:fs";
import { createHash } from "node:crypto";
import { join } from "node:path";
import { durableJson, isConfinedUpdatePath, recoverInterruptedSwap, recoverPendingUpdate } from "./apply.mjs";
import { treeDigest } from "./manifest.mjs";
import { sealLocalState, verifyLocalState } from "../init/state-integrity.mjs";

function fileDigest(path) { return createHash("sha256").update(readFileSync(path)).digest("hex"); }
function validDigest(value) { return typeof value === "string" && /^[a-f0-9]{64}$/.test(value); }

export function rollback({ appDir, priorDir, storeBackup, receiptPath, outDir = ".agent", root }) {
  for (const [name, value] of [["appDir", appDir], ["priorDir", priorDir]]) {
    if (typeof value !== "string" || value.length === 0) return { ok: false, problems: [`rollback_missing_argument: ${name}`] };
  }
  const problems = [], dbPath = join(root ?? "", outDir, "graph", "graph.db");
  try { if (root) recoverPendingUpdate(root); else recoverInterruptedSwap({ appDir, priorDir }); }
  catch (error) { return { ok: false, problems: [String(error.message ?? error)] }; }
  if (!root || !isConfinedUpdatePath(root, appDir) || !isConfinedUpdatePath(root, priorDir) || !isConfinedUpdatePath(root, dbPath) || (receiptPath && appDir === priorDir)) problems.push("unsafe rollback target");
  let receipt = null;
  if (receiptPath) {
    try {
      if (!isConfinedUpdatePath(root, receiptPath) || !existsSync(receiptPath)) throw new Error("missing receipt");
      receipt = verifyLocalState(root, JSON.parse(readFileSync(receiptPath, "utf8")), { namespace: "update" });
      if (!validDigest(receipt.currentAppDigest) || !validDigest(receipt.priorAppDigest) || !/^[0-9]+\.[0-9]+\.[0-9]+/.test(receipt.currentPackageVersion ?? "") || !/^[0-9]+\.[0-9]+\.[0-9]+/.test(receipt.priorPackageVersion ?? "")) throw new Error("invalid receipt binding");
      if (receipt.storeBackup && (!isConfinedUpdatePath(root, receipt.storeBackup) || !validDigest(receipt.storeBackupDigest) || !existsSync(receipt.storeBackup) || fileDigest(receipt.storeBackup) !== receipt.storeBackupDigest)) throw new Error("invalid receipt backup");
      storeBackup = receipt.storeBackup ?? storeBackup;
      if (!existsSync(appDir) || treeDigest(appDir) !== receipt.currentAppDigest || !existsSync(priorDir) || treeDigest(priorDir) !== receipt.priorAppDigest) throw new Error("receipt app binding mismatch");
    } catch { problems.push("no verified accepted update receipt"); }
  }
  if (!existsSync(priorDir)) problems.push("no prior version retained");
  if (storeBackup && !existsSync(storeBackup)) problems.push("no store backup");
  if (existsSync(dbPath) && !storeBackup && receiptPath) problems.push("no compatible store backup");
  if (problems.length) return { ok: false, problems };

  const next = `${appDir}.rollback-${Date.now()}`, failed = `${appDir}.failed-${Date.now()}`, failedDb = `${dbPath}.failed-${Date.now()}`;
  const journalPath = join(root, ".agent", "update", "transaction.json");
  const journal = { schemaVersion: 1, operation: "rollback", phase: "prepared", appDir, priorDir, dbPath, failedApp: failed, failedDb, hadDb: existsSync(dbPath), preAppDigest: treeDigest(appDir), preDbDigest: existsSync(dbPath) ? fileDigest(dbPath) : null, targetAppDigest: treeDigest(priorDir), targetDbDigest: storeBackup ? fileDigest(storeBackup) : null, receiptPath };
  try {
    durableJson(journalPath, sealLocalState(root, journal, { namespace: "update" }));
    copyRecursive(priorDir, next);
    renameSync(appDir, failed); renameSync(next, appDir);
    durableJson(journalPath, sealLocalState(root, { ...journal, phase: "app-swapped" }, { namespace: "update" }));
    if (storeBackup) {
      const staged = `${dbPath}.restore-${Date.now()}`;
      mkdirSync(join(root, outDir, "graph"), { recursive: true }); copyFileSync(storeBackup, staged);
      if (existsSync(dbPath)) renameSync(dbPath, failedDb);
      renameSync(staged, dbPath); rmSync(`${dbPath}-wal`, { force: true }); rmSync(`${dbPath}-shm`, { force: true });
      durableJson(journalPath, sealLocalState(root, { ...journal, phase: "db-swapped" }, { namespace: "update" }));
    }
    if (treeDigest(appDir) !== journal.targetAppDigest || (journal.targetDbDigest && fileDigest(dbPath) !== journal.targetDbDigest)) throw new Error("rollback_target_digest_mismatch");
    durableJson(journalPath, sealLocalState(root, { ...journal, phase: "committed" }, { namespace: "update" }));
    rmSync(failed, { recursive: true, force: true }); rmSync(failedDb, { force: true }); rmSync(journalPath, { force: true }); if (receiptPath) rmSync(receiptPath, { force: true });
    return { ok: true };
  } catch (error) {
    try { recoverPendingUpdate(root); } catch {}
    rmSync(next, { recursive: true, force: true });
    return { ok: false, problems: [String(error.message ?? error)] };
  }
}

function copyRecursive(source, target) {
  mkdirSync(target, { recursive: true });
  for (const name of readdirSync(source)) {
    const src = join(source, name), dst = join(target, name), stat = lstatSync(src);
    if (stat.isSymbolicLink() || (!stat.isDirectory() && !stat.isFile())) throw new Error(`unsafe rollback entry: ${src}`);
    if (stat.isDirectory()) copyRecursive(src, dst); else copyFileSync(src, dst);
  }
}
