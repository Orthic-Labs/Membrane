#!/usr/bin/env node
// CX-B4: offline Ed25519 update-signing key ceremony. Generates a key pair
// with node:crypto, writes the PKCS8 private key PEM outside the repository
// with owner-only permissions, and prints only the public key PEM and the
// derived keyId. A private key is never printed and never written inside
// the repository.

import { createHash, generateKeyPairSync } from "node:crypto";
import { chmodSync, writeFileSync } from "node:fs";
import { dirname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

// keyId = first 16 lowercase hex chars of sha256 over the SPKI DER export.
// Defined once and reused by the manifest signer so both agree by construction.
export function deriveUpdateKeyId(publicKey) {
  const spkiDer = publicKey.export({ type: "spki", format: "der" });
  return createHash("sha256").update(spkiDer).digest("hex").slice(0, 16);
}

function isInsideRepo(path) {
  const root = ROOT.toLowerCase();
  const candidate = resolve(path).toLowerCase();
  if (candidate === root) return true;
  return candidate.startsWith(root.endsWith(sep) ? root : `${root}${sep}`);
}

export function generateUpdateKeys({ out } = {}) {
  if (!out) throw new Error("generate-update-keys requires --out <path> for the private key");
  if (isInsideRepo(out)) throw new Error("refusing repository-local signing-secret output; choose a path outside the repository root");
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const privatePem = privateKey.export({ type: "pkcs8", format: "pem" });
  writeFileSync(resolve(out), privatePem, { mode: 0o600 });
  try { chmodSync(resolve(out), 0o600); } catch { /* mode not supported on this platform */ }
  const keyId = deriveUpdateKeyId(publicKey);
  return { publicPem: publicKey.export({ type: "spki", format: "pem" }), keyId };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const index = process.argv.indexOf("--out");
  try {
    const { publicPem, keyId } = generateUpdateKeys({ out: index < 0 ? null : process.argv[index + 1] });
    process.stdout.write(publicPem);
    process.stdout.write(`keyId: ${keyId}\n`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
