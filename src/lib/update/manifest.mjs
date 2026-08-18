// D16: signed release manifest validation. Portable/native updates require a
// signed manifest (UpdateManifestV1) and a matching checksum before staging;
// unsigned, downgrade, and replay manifests are rejected.

import { createHash, verify } from "node:crypto";
import { existsSync, lstatSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { validateContract } from "../contracts/validate.mjs";

export function loadUpdateManifest(path) {
  const raw = JSON.parse(readFileSync(path, "utf8"));
  return validateContract("UpdateManifestV1", raw);
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  return value;
}

// Signature bytes are deterministic and never include the detached signature.
export function canonicalManifestPayload(manifest) {
  const { signature, ...unsigned } = manifest ?? {};
  return JSON.stringify(canonical(unsigned));
}

export function verifySignedManifest(manifest, { trustedKeys } = {}) {
  if (!trustedKeys || typeof trustedKeys !== "object") return { ok: false, reason: "update_trust_root_missing" };
  if (manifest?.signatureAlgorithm !== "Ed25519" || typeof manifest?.keyId !== "string") return { ok: false, reason: "invalid_signature_metadata" };
  const publicKey = trustedKeys[manifest.keyId];
  if (typeof publicKey !== "string" || !publicKey) return { ok: false, reason: "untrusted_key_id" };
  if (typeof manifest?.signature !== "string" || !/^[A-Za-z0-9+/]+={0,2}$/.test(manifest.signature)) return { ok: false, reason: "invalid_signature" };
  try {
    const signature = Buffer.from(manifest.signature, "base64");
    if (!signature.length || signature.toString("base64") !== manifest.signature) return { ok: false, reason: "invalid_signature" };
    return verify(null, Buffer.from(canonicalManifestPayload(manifest)), publicKey, signature)
      ? { ok: true, algorithm: "Ed25519" }
      : { ok: false, reason: "invalid_signature" };
  } catch { return { ok: false, reason: "invalid_signature" }; }
}

export function loadTrustedUpdateKeys(path = join(dirname(fileURLToPath(import.meta.url)), "trusted-update-keys.json")) {
  if (!existsSync(path)) return null;
  try {
    const stat = lstatSync(path);
    if (stat.isSymbolicLink() || !stat.isFile()) throw new Error("invalid root file");
    const parsed = JSON.parse(readFileSync(path, "utf8"));
    if (!parsed || Object.keys(parsed).length !== 2 || parsed.schemaVersion !== 1 || !Array.isArray(parsed.keys)) throw new Error("invalid root schema");
    const keys = {};
    for (const entry of parsed.keys) {
      if (!entry || Object.keys(entry).sort().join(",") !== "algorithm,keyId,publicKey" || !/^[A-Za-z0-9._-]+$/.test(entry.keyId) || entry.algorithm !== "Ed25519" || !/^-----BEGIN PUBLIC KEY-----\r?\n[\s\S]+\r?\n-----END PUBLIC KEY-----\r?\n?$/.test(entry.publicKey) || keys[entry.keyId]) throw new Error("invalid root key");
      keys[entry.keyId] = entry.publicKey;
    }
    return keys;
  } catch { throw new Error("update_trust_root_corrupt"); }
}

// Hash each safe relative name and exact file bytes, in lexical order.
export function treeDigest(root) {
  const base = resolve(root);
  const hasher = createHash("sha256");
  const visit = (dir) => {
    const entries = readdirSync(dir).sort();
    for (const name of entries) {
      const path = join(dir, name);
      const stat = lstatSync(path);
      if (stat.isSymbolicLink() || (!stat.isDirectory() && !stat.isFile())) throw new Error(`unsafe artifact entry: ${path}`);
      if (stat.isDirectory()) visit(path);
      else {
        const rel = relative(base, path).replaceAll("\\", "/");
        if (!rel || rel.startsWith("../") || rel.includes("/../")) throw new Error(`unsafe artifact path: ${path}`);
        hasher.update(rel).update("\0").update(readFileSync(path)).update("\0");
      }
    }
  };
  if (!lstatSync(base).isDirectory()) throw new Error("artifact must be a directory");
  visit(base);
  return hasher.digest("hex");
}

export function verifyArtifactChecksum(manifest, artifactName, actualSha256) {
  const artifact = (manifest.artifacts ?? []).find((entry) => entry.name === artifactName);
  if (!artifact) return { ok: false, reason: "artifact_not_in_manifest" };
  if (artifact.sha256 !== actualSha256) return { ok: false, reason: "checksum_mismatch" };
  return { ok: true };
}

export function rejectDowngrade(candidateVersion, currentVersion) {
  const current = parseVersion(currentVersion);
  const candidate = parseVersion(candidateVersion);
  if (candidate.major < current.major) return { ok: false, reason: "downgrade_major" };
  if (candidate.major === current.major && candidate.minor < current.minor) return { ok: false, reason: "downgrade_minor" };
  if (candidate.major === current.major && candidate.minor === current.minor && candidate.patch < current.patch) return { ok: false, reason: "downgrade_patch" };
  if (candidate.major === current.major && candidate.minor === current.minor && candidate.patch === current.patch) return { ok: false, reason: "replay_version" };
  return { ok: true };
}

export function rejectReplay(manifest, seenManifests = []) {
  if (seenManifests.includes(manifest.commit)) return { ok: false, reason: "replay_commit" };
  return { ok: true };
}

function parseVersion(version) {
  const [major = 0, minor = 0, patch = 0] = String(version).split(".").map((part) => Number.parseInt(part, 10) || 0);
  return { major, minor, patch };
}
