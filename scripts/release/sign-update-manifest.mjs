#!/usr/bin/env node
// CX-B4: sign an UpdateManifestV1 in place with the UPDATE_SIGNING_KEY_PEM
// private key. Uses the identical canonicalManifestPayload the verifier
// uses; canonicalization is never reimplemented here.

import { createPrivateKey, createPublicKey, sign } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { canonicalManifestPayload } from "../../lib/update/manifest.mjs";
import { deriveUpdateKeyId } from "./generate-update-keys.mjs";

export function signUpdateManifest(manifest, { privateKeyPem } = {}) {
  if (!privateKeyPem) throw new Error("UPDATE_SIGNING_KEY_PEM missing: cannot sign update manifest");
  const privateKey = createPrivateKey(privateKeyPem);
  const publicKey = createPublicKey(privateKey);
  manifest.keyId = deriveUpdateKeyId(publicKey);
  manifest.signatureAlgorithm = "Ed25519";
  manifest.signature = sign(null, Buffer.from(canonicalManifestPayload(manifest)), privateKey).toString("base64");
  return manifest;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  const index = process.argv.indexOf("--manifest");
  const manifestPath = index < 0 ? null : process.argv[index + 1];
  try {
    if (!manifestPath) throw new Error("sign-update-manifest requires --manifest <path>");
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    signUpdateManifest(manifest, { privateKeyPem: process.env.UPDATE_SIGNING_KEY_PEM });
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(`signed ${manifestPath} (keyId ${manifest.keyId})`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
