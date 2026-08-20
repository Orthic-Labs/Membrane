import { createHash, createPrivateKey, createPublicKey, randomUUID, sign, verify } from "node:crypto";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export const SCOPE_GRANT_TTL_SECONDS = 300;
export const SCOPE_GRANT_SOURCE_BYTES_MAX = 32 * 1024;
export const SCOPE_GRANT_FILES_MAX = 16;
export const SCOPE_GRANT_ISSUER = "rightcontext-gateway";
export const SCOPE_GRANT_ALGORITHM = "Ed25519";
export const SCOPE_GRANT_DOMAIN = "rightstudio.scope-grant.v1\0";
const DEFAULT_STATE_ROOT = process.platform === "win32"
  ? join(process.env.LOCALAPPDATA || join(homedir(), "AppData", "Local"), "RightStudio", "bounded-run")
  : join(homedir(), ".local", "state", "rightstudio", "bounded-run");
const DEFAULT_KEY_FILE = join(DEFAULT_STATE_ROOT, "scope-grant-ed25519-private.pem");

function hash(value) {
  return `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;
}

function canonical(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(",")}}`;
}

export function scopeGrantSigningBytes(grant) {
  if (!grant || typeof grant !== "object" || Array.isArray(grant)) throw new TypeError("ScopeGrantV1 must be an object");
  const { signature, ...unsigned } = grant;
  return Buffer.concat([Buffer.from(SCOPE_GRANT_DOMAIN, "utf8"), Buffer.from(canonical(unsigned), "utf8")]);
}

function scopeGrantKey(signingKey) {
  try {
    const privateKey = signingKey || createPrivateKey(readFileSync(process.env.MEMBRANE_SCOPE_GRANT_PRIVATE_KEY_FILE || DEFAULT_KEY_FILE));
    const rawPublic = createPublicKey(privateKey).export({ type: "spki", format: "der" }).subarray(-32);
    if (rawPublic.length !== 32) return null;
    return { privateKey, publicKey: createPublicKey(privateKey), keyId: `scope-grant-ed25519-v1:${createHash("sha256").update(rawPublic).digest("hex").slice(0, 32)}` };
  } catch {
    return null;
  }
}

export function scopeGrantTrust(signingKey) {
  const key = scopeGrantKey(signingKey);
  if (!key) return null;
  return { keyId: key.keyId, publicKey: createPublicKey(key.privateKey).export({ type: "spki", format: "der" }).subarray(-32).toString("base64") };
}

function isoAfter(seconds, now = Date.now()) {
  return new Date(now + seconds * 1000).toISOString();
}

function exactRange(sourceRef) {
  if (typeof sourceRef !== "string") return null;
  const match = /^(.*):(\d+)-(\d+)$/.exec(sourceRef);
  if (!match) return null;
  const [, path, startRaw, endRaw] = match;
  const startLine = Number(startRaw);
  const endLine = Number(endRaw);
  if (
    !path || path.startsWith("/") || path.includes("\\") || path.split("/").includes("..") ||
    !Number.isSafeInteger(startLine) || !Number.isSafeInteger(endLine) || startLine < 1 || endLine < startLine
  ) return null;
  return { path, startLine, endLine };
}

function blueprintRanges(packet) {
  const raw = Array.isArray(packet?.blocks) ? packet.blocks : [];
  const ranges = raw
    .filter((block) => block?.provider === "blueprint")
    .map((block) => exactRange(block.sourceRef))
    .filter(Boolean);
  const unique = [...new Map(ranges.map((range) => [`${range.path}:${range.startLine}-${range.endLine}`, range])).values()];
  return unique.length > SCOPE_GRANT_FILES_MAX ? null : unique;
}

export function mintScopeGrantV1({ binding, args, packet, freshness, signingKey, now = Date.now() }) {
  if (!binding || !args?.taskId || !args?.session || !packet || !freshness) return null;
  if (!binding.root || !binding.repository_id) return null;
  if (!["clean", "dirty_overlay"].includes(freshness.graphState)) return null;
  if (!freshness.blueprintGeneration || !freshness.manifestDigest) return null;
  if (freshness.providers?.blueprint?.usable !== true) return null;
  const readPaths = blueprintRanges(packet);
  if (!readPaths?.length) return null;
  const key = scopeGrantKey(signingKey);
  if (!key) return null;

  const request = {
    repositoryId: binding.repository_id,
    repositoryRoot: binding.root,
    scopeId: binding.scope_id,
    task: args.task,
    taskId: args.taskId,
    session: args.session,
    intent: args.intent || "",
    anchors: args.anchors || "",
    budget: args.budget ?? null,
  };
  const cryptAdmitted = (packet.blocks || []).some((block) => block?.provider === "crypt");
  const issuedAt = new Date(now).toISOString();
  const grant = {
    schemaVersion: 1,
    id: `sgv1-${randomUUID()}`,
    issuer: SCOPE_GRANT_ISSUER,
    client: "mcp",
    repositoryIds: [binding.root],
    permittedEdgeTypes: ["source_read"],
    taskId: args.taskId,
    sessionId: args.session,
    issuedAt,
    expiresAt: isoAfter(SCOPE_GRANT_TTL_SECONDS, now),
    status: "active",
    nonce: randomUUID().replaceAll("-", ""),
    manifestDigest: freshness.manifestDigest,
    repositoryId: binding.repository_id,
    repositoryRoot: binding.root,
    blueprintGeneration: freshness.blueprintGeneration,
    blueprintFreshness: freshness.graphState,
    requestHash: hash(request),
    contextPacketHash: hash(packet),
    readPaths,
    sourceReadBytesMax: SCOPE_GRANT_SOURCE_BYTES_MAX,
    uniqueFilesMax: readPaths.length,
    resultsMax: readPaths.length,
    cryptStatus: cryptAdmitted ? "available" : "degraded",
    degraded: !cryptAdmitted,
    signatureAlgorithm: SCOPE_GRANT_ALGORITHM,
    keyId: key.keyId,
  };
  return { ...grant, signature: sign(null, scopeGrantSigningBytes(grant), key.privateKey).toString("base64") };
}

export function validateScopeGrantV1(grant, { binding, args, packet, freshness, signingKey, trustedPublicKey, trustedKeyId, now = Date.now() }) {
  const key = scopeGrantKey(signingKey);
  const publicKey = trustedPublicKey ? Buffer.from(trustedPublicKey, "base64") : key && createPublicKey(key.privateKey).export({ type: "spki", format: "der" }).subarray(-32);
  const keyId = trustedKeyId || key?.keyId;
  const expected = mintScopeGrantV1({ binding, args, packet, freshness, signingKey, now });
  if (!grant || !expected) return false;
  if (!publicKey || publicKey.length !== 32 || grant.signatureAlgorithm !== SCOPE_GRANT_ALGORITHM || grant.keyId !== keyId || typeof grant.signature !== "string") return false;
  let signatureValid = false;
  try {
    const der = Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), publicKey]);
    signatureValid = verify(null, scopeGrantSigningBytes(grant), { key: der, format: "der", type: "spki" }, Buffer.from(grant.signature, "base64"));
  } catch { return false; }
  if (!signatureValid) return false;
  if (Date.parse(grant.expiresAt) <= now || grant.status !== "active") return false;
  const immutableFields = [
    "schemaVersion", "issuer", "client", "taskId", "sessionId", "repositoryId", "repositoryRoot", "blueprintGeneration",
    "blueprintFreshness", "requestHash", "contextPacketHash", "manifestDigest", "sourceReadBytesMax",
    "uniqueFilesMax", "resultsMax", "cryptStatus", "degraded",
  ];
  return immutableFields.every((field) => grant[field] === expected[field]) &&
    JSON.stringify(grant.repositoryIds) === JSON.stringify(expected.repositoryIds) &&
    JSON.stringify(grant.readPaths) === JSON.stringify(expected.readPaths) &&
    typeof grant.id === "string" && grant.id.startsWith("sgv1-") &&
    typeof grant.nonce === "string" && grant.nonce.length >= 8;
}
