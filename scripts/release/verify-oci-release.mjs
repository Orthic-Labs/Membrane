#!/usr/bin/env node
import { createHash } from "node:crypto";
import { lstatSync, readFileSync, realpathSync } from "node:fs";
import { isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HEX40 = /^[a-f0-9]{40}$/;
const HEX64 = /^[a-f0-9]{64}$/;
const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };
const text = (value, name) => { if (typeof value !== "string" || !value) fail(`missing ${name}`); return value; };
const nonzero = value => HEX64.test(value) && !/^0+$/.test(value);
const evidence = (root, value, name) => {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).some(key => !["path", "sha256"].includes(key)) || isAbsolute(value.path) || relative(root, resolve(root, value.path)).startsWith(`..${sep}`) || !nonzero(value.sha256)) fail(`${name} evidence invalid`);
  const candidate = resolve(root, value.path); const rootReal = realpathSync(root); const real = realpathSync(candidate); const stat = lstatSync(candidate);
  if (!stat.isFile() || stat.isSymbolicLink() || !real.startsWith(`${rootReal}${sep}`)) fail(`${name} evidence invalid`);
  if (createHash("sha256").update(readFileSync(candidate)).digest("hex") !== value.sha256) fail(`${name} evidence hash mismatch`);
};

export function verifyOciRelease(value, root = process.cwd()) {
  if (value?.schema !== "membrane.oci-release.v1") fail("unsupported schema");
  if (!['unavailable', 'ready'].includes(value.state) || value.publish !== false) fail("OCI source contract must remain non-publishing");
  if (!/^[a-z0-9./_-]+@sha256:[a-f0-9]{64}$/.test(text(value.base, "base"))) fail("base must be exact digest-pinned reference");
  const identity = value.identity || {};
  if (!HEX40.test(text(identity.commit, "identity.commit")) || !HEX64.test(text(identity.release_generation, "identity.release_generation")) || !HEX64.test(text(identity.artifact_sha256, "identity.artifact_sha256"))) fail("identity hashes invalid");
  if (value.state === "unavailable") {
    if (!/^registry\.invalid\/[a-z0-9][a-z0-9/_-]*$/.test(text(value.image, "image")) || identity.tag !== "UNAVAILABLE") fail("unavailable identity must prevent release use");
    for (const name of ["sbom", "ed25519", "cosign", "rootlessHealth", "secretScan"]) if (value.evidence?.[name] !== null) fail(`${name} evidence must be unavailable`);
  } else {
    if (!/^[a-z0-9.-]+\/[a-z0-9/_-]+@sha256:[a-f0-9]{64}$/.test(text(value.image, "image")) || !/^v\d+\.\d+\.\d+$/.test(identity.tag) || !nonzero(identity.release_generation) || !nonzero(identity.artifact_sha256) || /@sha256:0{64}$/.test(value.base)) fail("ready OCI identity invalid");
    for (const name of ["sbom", "ed25519", "cosign", "rootlessHealth", "secretScan"]) evidence(root, value.evidence?.[name], name);
  }
  return { verified: true, state: value.state, publish: value.publish };
}

function cli() {
  const path = process.argv[process.argv.indexOf("--manifest") + 1];
  if (!path) { console.error("usage: verify-oci-release.mjs --manifest PATH"); process.exit(2); }
  try { process.stdout.write(`${JSON.stringify(verifyOciRelease(JSON.parse(readFileSync(resolve(process.cwd(), path), "utf8")), process.cwd()))}\n`); }
  catch (error) { console.error(error.message); process.exit(1); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli();
