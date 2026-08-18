// Host-owned orientation receipt store — data, not enforcement.
//
// Receipts live outside the agent-writable repository tree by default
// (~/.agent/receipts or CORTEX_RECEIPT_STORE). The store records
// orientation decisions keyed by session / task / repo / generation; it does
// not block tools or classify shell commands.

import {
  existsSync,
  chmodSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createHmac, randomBytes, randomUUID, timingSafeEqual } from "node:crypto";

export const RECEIPT_SCHEMA_VERSION = 1;

const GRANT_KEY_NAME = "grant.key";
const GRANT_DIR_NAME = "grants";

function grantRoot(repoRoot, outDir = ".agent") {
  return join(resolve(repoRoot), outDir, "graph");
}

function grantKeyPath(repoRoot, outDir = ".agent") { return join(grantRoot(repoRoot, outDir), GRANT_KEY_NAME); }
function grantDir(repoRoot, outDir = ".agent") { return join(grantRoot(repoRoot, outDir), GRANT_DIR_NAME); }

function ensureGrantKey(repoRoot, outDir = ".agent") {
  const path = grantKeyPath(repoRoot, outDir);
  mkdirSync(dirname(path), { recursive: true });
  if (!existsSync(path)) {
    writeFileSync(path, randomBytes(32).toString("hex"), { encoding: "utf8", mode: 0o600 });
    try { chmodSync(path, 0o600); } catch {}
  }
  return readFileSync(path, "utf8").trim();
}

function grantPayload(grant) {
  return {
    taskId: String(grant.taskId),
    repoRoot: String(grant.repoRoot),
    generationId: grant.generationId ?? null,
    receiptId: String(grant.receiptId),
    paths: [...(grant.paths ?? [])].map((path) => String(path)),
    issuedMs: Number(grant.issuedMs),
    ttlMs: Number(grant.ttlMs),
  };
}

function signGrant(grant, key) {
  return createHmac("sha256", key).update(JSON.stringify(grantPayload(grant))).digest("hex");
}

function grantFilePath(repoRoot, receiptId, outDir = ".agent") {
  return join(grantDir(repoRoot, outDir), `${receiptId}.json`);
}

function globRegex(glob) {
  let pattern = "";
  const value = String(glob).replace(/\\/g, "/");
  for (let index = 0; index < value.length; index += 1) {
    const char = value[index];
    if (char === "*" && value[index + 1] === "*") { pattern += ".*"; index += 1; }
    else if (char === "*") pattern += "[^/]*";
    else if (char === "?") pattern += "[^/]";
    else pattern += char.replace(/[.+^${}()|[\]\\]/g, "\\$&");
  }
  return new RegExp(`^${pattern}$`);
}

function grantMatchesPath(grant, path) {
  const normalized = String(path).replace(/\\/g, "/").replace(/^\.\//, "");
  return grant.paths.some((glob) => globRegex(glob).test(normalized));
}

export function issueScopeGrant({ repoRoot, generationId, taskId, paths, ttlMinutes = 120, outDir = ".agent", receiptId = randomUUID(), issuedMs = Date.now() }) {
  const normalizedPaths = [...new Set((paths ?? []).map((path) => String(path).replace(/\\/g, "/").replace(/^\.\//, "")).filter(Boolean))].sort();
  if (!String(taskId ?? "").trim()) throw new Error("grant taskId is required");
  if (!normalizedPaths.length) throw new Error("grant paths are required");
  const ttlMs = Math.max(1, Number(ttlMinutes) * 60 * 1000);
  const grant = {
    ...grantPayload({ taskId, repoRoot: resolve(repoRoot), generationId, receiptId, paths: normalizedPaths, issuedMs, ttlMs }),
  };
  const key = ensureGrantKey(repoRoot, outDir);
  grant.signature = signGrant(grant, key);
  const path = grantFilePath(repoRoot, grant.receiptId, outDir);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(grant, null, 2)}\n`, "utf8");
  return { grant, path };
}

export function checkScopeGrant({ repoRoot, generationId, taskId, path: requestedPath, outDir = ".agent", nowMs = Date.now() }) {
  const root = resolve(repoRoot);
  const keyPath = grantKeyPath(root, outDir);
  if (!existsSync(keyPath)) return { allowed: false, reason: "grant_key_missing" };
  const key = readFileSync(keyPath, "utf8").trim();
  const directory = grantDir(root, outDir);
  if (!existsSync(directory)) return { allowed: false, reason: "grant_missing" };
  const grants = readdirSync(directory).filter((name) => name.endsWith(".json"));
  for (const name of grants) {
    let grant;
    try { grant = JSON.parse(readFileSync(join(directory, name), "utf8")); } catch { continue; }
    const expected = signGrant(grant, key);
    const actual = String(grant.signature ?? "");
    const valid = actual.length === expected.length && timingSafeEqual(Buffer.from(actual), Buffer.from(expected));
    if (!valid) continue;
    if (grant.taskId !== String(taskId) || (generationId != null && grant.generationId !== generationId)) continue;
    if (Number(grant.issuedMs) + Number(grant.ttlMs) <= Number(nowMs)) continue;
    if (!grantMatchesPath(grant, requestedPath)) continue;
    return { allowed: true, reason: "grant_match", grant };
  }
  return { allowed: false, reason: "grant_miss" };
}

export function defaultReceiptStoreDir() {
  const fromEnv = process.env.CORTEX_RECEIPT_STORE;
  if (fromEnv && String(fromEnv).trim()) return resolve(String(fromEnv).trim());
  return join(homedir(), ".agent", "receipts");
}

function ensureDir(dir) {
  mkdirSync(dir, { recursive: true });
  return dir;
}

function receiptPath(storeDir, receiptId) {
  return join(storeDir, `${receiptId}.json`);
}

function indexPath(storeDir) {
  return join(storeDir, "index.json");
}

function readIndex(storeDir) {
  const path = indexPath(storeDir);
  if (!existsSync(path)) return { schemaVersion: 1, byKey: {}, byReceipt: {} };
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return { schemaVersion: 1, byKey: {}, byReceipt: {} };
  }
}

function writeIndexAtomic(storeDir, index) {
  const path = indexPath(storeDir);
  const tmp = `${path}.${process.pid}.${Date.now()}.tmp`;
  writeFileSync(tmp, `${JSON.stringify(index, null, 2)}\n`, "utf8");
  renameSync(tmp, path);
}

function writeReceiptAtomic(storeDir, receipt) {
  const path = receiptPath(storeDir, receipt.receiptId);
  const tmp = `${path}.${process.pid}.${Date.now()}.tmp`;
  writeFileSync(tmp, `${JSON.stringify(receipt, null, 2)}\n`, "utf8");
  renameSync(tmp, path);
}

/** Composite lookup key: session + task + repo + generation. */
export function receiptLookupKey({ sessionId, taskId, repoIdentity, generationId }) {
  return [
    String(sessionId ?? ""),
    String(taskId ?? ""),
    String(repoIdentity ?? ""),
    String(generationId ?? ""),
  ].join("\u001f");
}

/**
 * Create a host-owned receipt store rooted at `storeDir`.
 * @param {{ storeDir?: string }} [options]
 */
export function createReceiptStore(options = {}) {
  const storeDir = ensureDir(resolve(options.storeDir ?? defaultReceiptStoreDir()));

  function put(receipt) {
    if (!receipt?.receiptId) throw new Error("receipt.receiptId is required");
    writeReceiptAtomic(storeDir, receipt);
    const index = readIndex(storeDir);
    const key = receiptLookupKey(receipt);
    index.byKey[key] = receipt.receiptId;
    index.byReceipt[receipt.receiptId] = {
      key,
      status: receipt.status ?? "active",
      updatedAt: receipt.updatedAt ?? receipt.issuedAt ?? null,
    };
    writeIndexAtomic(storeDir, index);
    return receipt;
  }

  function get(receiptId) {
    if (!receiptId) return null;
    const path = receiptPath(storeDir, receiptId);
    if (!existsSync(path)) return null;
    try {
      return JSON.parse(readFileSync(path, "utf8"));
    } catch {
      return null;
    }
  }

  function findActive(query) {
    const index = readIndex(storeDir);
    const key = receiptLookupKey(query);
    const receiptId = index.byKey[key];
    if (!receiptId) return null;
    const receipt = get(receiptId);
    if (!receipt || receipt.status === "revoked") return null;
    return receipt;
  }

  function list({ includeRevoked = false } = {}) {
    const files = existsSync(storeDir)
      ? readdirSync(storeDir).filter((name) => name.endsWith(".json") && name !== "index.json")
      : [];
    const out = [];
    for (const name of files) {
      try {
        const receipt = JSON.parse(readFileSync(join(storeDir, name), "utf8"));
        if (!includeRevoked && receipt.status === "revoked") continue;
        out.push(receipt);
      } catch {
        // skip corrupt rows; host can reclaim later
      }
    }
    return out.sort((a, b) => String(a.issuedAt ?? "").localeCompare(String(b.issuedAt ?? "")));
  }

  function revoke(receiptId, { reason = "explicit_revoke", at = new Date().toISOString() } = {}) {
    const receipt = get(receiptId);
    if (!receipt) return null;
    if (receipt.status === "revoked") return receipt;
    const updated = {
      ...receipt,
      status: "revoked",
      revokedAt: at,
      revokeReason: reason,
      updatedAt: at,
    };
    return put(updated);
  }

  function clear() {
    if (!existsSync(storeDir)) return;
    for (const name of readdirSync(storeDir)) {
      rmSync(join(storeDir, name), { force: true });
    }
  }

  return {
    storeDir,
    put,
    get,
    findActive,
    list,
    revoke,
    clear,
    nextId: () => randomUUID(),
  };
}

/**
 * Build a durable orientation receipt record (not yet persisted).
 */
export function buildOrientationReceipt(fields) {
  const issuedAt = fields.issuedAt ?? new Date().toISOString();
  return {
    schemaVersion: RECEIPT_SCHEMA_VERSION,
    kind: "cortex_orientation_receipt",
    receiptId: fields.receiptId ?? randomUUID(),
    status: fields.status ?? "active",
    sessionId: String(fields.sessionId ?? ""),
    taskId: String(fields.taskId ?? fields.task ?? ""),
    turnDigest: fields.turnDigest ?? null,
    repoIdentity: String(fields.repoIdentity ?? fields.repoRoot ?? ""),
    worktreeIdentity: fields.worktreeIdentity ?? null,
    generationId: fields.generationId ?? null,
    manifestDigest: fields.manifestDigest ?? null,
    sourceObservationDigest: fields.sourceObservationDigest ?? null,
    candidateSetDigest: fields.candidateSetDigest ?? null,
    explicitAnchors: [...(fields.explicitAnchors ?? [])],
    allowedPaths: [...(fields.allowedPaths ?? [])],
    allowedDirectoryScopes: [...(fields.allowedDirectoryScopes ?? [])],
    allowedOperations: [...(fields.allowedOperations ?? ["read", "search", "test", "edit"])],
    overlayRevision: Number(fields.overlayRevision ?? 0),
    omissions: [...(fields.omissions ?? [])],
    issuedAt,
    updatedAt: fields.updatedAt ?? issuedAt,
  };
}
