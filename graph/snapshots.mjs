import { execFileSync } from "node:child_process";
import { realpathSync } from "node:fs";
import { resolve } from "node:path";

function fail(code) { const error = new Error(code); error.code = code; return error; }
function rootOf(root) { try { return realpathSync(resolve(root)); } catch { return resolve(root); } }

export function currentGitIdentity(root) {
  const repoRoot = rootOf(root);
  try {
    const head = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
    const status = execFileSync("git", ["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", ".", ":(exclude).agent", ":(exclude).agent/**"], { cwd: repoRoot, encoding: "buffer", stdio: ["ignore", "pipe", "ignore"] });
    return { head, dirty: status.length > 0 };
  } catch { return null; }
}

function identityFromStore(db, repoRoot) {
  let rows;
  try { rows = db.prepare("SELECT key,value FROM generation").all(); } catch { throw fail("snapshot_missing"); }
  const envelope = Object.fromEntries(rows.map((row) => { try { return [row.key, JSON.parse(row.value)]; } catch { throw fail("snapshot_malformed"); } }));
  const manifest = envelope.manifest;
  if (!manifest || manifest.complete !== true || typeof manifest.generationId !== "string" || !manifest.generationId || typeof manifest.manifestDigest !== "string" || !manifest.manifestDigest) throw fail("snapshot_incomplete");
  if (!envelope.sourceObservation || typeof envelope.sourceObservation.head !== "string" || !envelope.sourceObservation.head || typeof envelope.sourceObservation.dirty !== "boolean") throw fail("snapshot_malformed");
  if (envelope.sourceObservation.dirty === true) throw fail("snapshot_dirty");
  const leaves = db.prepare("SELECT path,digest FROM generation_leaf WHERE kind='file' ORDER BY path").all();
  if (leaves.some((leaf) => typeof leaf.path !== "string" || typeof leaf.digest !== "string" || !leaf.digest)) throw fail("snapshot_malformed");
  return { repoRoot: rootOf(repoRoot), generationId: String(manifest.generationId), manifestDigest: String(manifest.manifestDigest), sourceObservation: envelope.sourceObservation, leaves };
}

export function createSnapshot(db, name, repoRoot, { current = currentGitIdentity(repoRoot) } = {}) {
  const snapshotName = String(name ?? "").trim();
  if (!snapshotName || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(snapshotName)) throw fail("snapshot_invalid_name");
  const identity = identityFromStore(db, repoRoot);
  if (!current || current.dirty) throw fail("snapshot_current_dirty");
  if (identity.repoRoot !== rootOf(repoRoot) || identity.sourceObservation.head !== current.head || identity.sourceObservation.dirty !== false) throw fail("snapshot_stale");
  const existing = db.prepare("SELECT identity_json FROM named_snapshot WHERE name=?").get(snapshotName);
  const json = JSON.stringify(identity);
  if (existing) {
    try {
      if (JSON.stringify(JSON.parse(existing.identity_json)) === json) return { ...identity, name: snapshotName, idempotent: true };
    } catch { throw fail("snapshot_malformed"); }
    throw fail("snapshot_conflict");
  }
  db.prepare("INSERT INTO named_snapshot(name,repo_root,generation_id,manifest_digest,identity_json,created_ms) VALUES (?,?,?,?,?,?)").run(snapshotName, identity.repoRoot, identity.generationId, identity.manifestDigest, json, Date.now());
  return { ...identity, name: snapshotName, idempotent: false };
}

export function getSnapshot(db, name) {
  const snapshotName = String(name ?? "").trim();
  if (!snapshotName || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(snapshotName)) throw fail("snapshot_invalid_name");
  const row = db.prepare("SELECT identity_json FROM named_snapshot WHERE name=?").get(snapshotName);
  if (!row) throw fail("snapshot_missing");
  try { return { ...JSON.parse(row.identity_json), name: snapshotName }; } catch { throw fail("snapshot_malformed"); }
}

export function listSnapshots(db) {
  return db.prepare("SELECT name,repo_root AS repoRoot,generation_id AS generationId,manifest_digest AS manifestDigest,created_ms AS createdMs FROM named_snapshot ORDER BY name").all();
}

export function changesSince(db, name, { limit = 100 } = {}) {
  if (limit !== undefined && limit !== null && (typeof limit === "string" && limit.trim() === "" || !Number.isInteger(Number(limit)) || Number(limit) < 1)) throw fail("snapshot_invalid_limit");
  const snapshot = getSnapshot(db, name);
  const current = identityFromStore(db, snapshot.repoRoot);
  const before = new Map(snapshot.leaves.map((leaf) => [leaf.path, leaf.digest]));
  const after = new Map(current.leaves.map((leaf) => [leaf.path, leaf.digest]));
  const changes = [];
  for (const path of new Set([...before.keys(), ...after.keys()])) {
    if (!before.has(path)) changes.push({ path, kind: "added" });
    else if (!after.has(path)) changes.push({ path, kind: "deleted" });
    else if (before.get(path) !== after.get(path)) changes.push({ path, kind: "modified" });
  }
  changes.sort((a, b) => a.path.localeCompare(b.path) || a.kind.localeCompare(b.kind));
  const cap = Math.min(10000, Number(limit));
  return { name: snapshot.name, base: { generationId: snapshot.generationId, manifestDigest: snapshot.manifestDigest }, head: { generationId: current.generationId, manifestDigest: current.manifestDigest }, changes: changes.slice(0, cap), receipt: { total: changes.length, limit: cap, truncated: changes.length > cap } };
}
