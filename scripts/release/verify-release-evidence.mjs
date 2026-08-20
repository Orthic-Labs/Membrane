#!/usr/bin/env node
import { createHash } from "node:crypto";
import { lstatSync, readFileSync, realpathSync } from "node:fs";
import { isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { verifyPair } from "./verify-platform-artifacts.mjs";

const HEX40 = /^[a-f0-9]{40}$/;
const HEX64 = /^[a-f0-9]{64}$/;
const TARGETS = new Set(["mac-arm64", "mac-x64"]);
const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };
const field = (object, name) => { if (typeof object?.[name] !== "string" || !object[name]) fail(`missing ${name}`); return object[name]; };
const confined = (root, path) => {
  if (isAbsolute(path) || relative(root, resolve(root, path)).startsWith(`..${sep}`)) fail("evidence path escapes root");
  const candidate = resolve(root, path); const rootReal = realpathSync(root); const real = realpathSync(candidate); const stat = lstatSync(candidate);
  if (stat.isSymbolicLink() || !stat.isFile() || !real.startsWith(`${rootReal}${sep}`)) fail("evidence path invalid");
  return candidate;
};
const hash = (root, path) => createHash("sha256").update(readFileSync(confined(root, path))).digest("hex");
const receipt = (root, value, label) => {
  const path = field(value, "path"); const expected = field(value, "sha256");
  if (!HEX64.test(expected)) fail(`${label} sha256 must be lowercase SHA-256`);
  const actual = hash(root, path);
  if (actual !== expected) fail(`${label} hash mismatch`);
};
const json = (root, value, label) => { receipt(root, value, label); return JSON.parse(readFileSync(confined(root, value.path), "utf8")); };

export function verifyReleaseEvidence(manifest, root = process.cwd()) {
  if (!manifest || manifest.schema !== "membrane.release-evidence.v1") fail("unsupported schema");
  const release = manifest.release || {};
  if (!/^v\d+\.\d+\.\d+$/.test(field(release, "tag"))) fail("tag must be immutable semver");
  if (!HEX40.test(field(release, "commit")) || !/^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(field(release, "tree"))) fail("release source identity invalid");
  if (!HEX64.test(field(release, "generation")) || !HEX64.test(field(release, "artifact_sha256"))) fail("release generation or artifact hash invalid");
  if (!TARGETS.has(field(release, "target")) || release.vector_dispatch !== "CORTEX_VECTOR_DISPATCH_V2") fail("unsupported target or vector dispatch");
  for (const [name, value] of Object.entries({ artifact: manifest.artifact, sbom: manifest.sbom, provenance: manifest.provenance, toolchain: manifest.toolchain, compatibility: manifest.compatibility, event_history: manifest.event_history?.receipt })) receipt(root, value, name);
  if (manifest.artifact.sha256 !== release.artifact_sha256) fail("artifact must bind release artifact_sha256");
  if (!Array.isArray(manifest.tests) || !manifest.tests.length) fail("test receipts required");
  manifest.tests.forEach((value, index) => receipt(root, value, `test ${index}`));
  if (!Array.isArray(manifest.signatures) || !manifest.signatures.length) fail("signature receipt required");
  const trusts = [...manifest.signatures, manifest.platform_trust];
  for (const trust of trusts) {
    if (!trust || !["ed25519", "apple-notary"].includes(trust.kind) || !field(trust, "identity")) fail("untrusted signing identity");
    if (trust.subject_sha256 !== release.artifact_sha256) fail("trust receipt must bind artifact hash");
    receipt(root, trust.receipt, `${trust.kind} receipt`);
  }
  if (!manifest.signatures.some((trust) => trust.kind === "ed25519")) fail("ed25519 artifact signature receipt required");
  if (manifest.platform_trust?.kind !== "apple-notary") fail(`${release.target} requires apple-notary receipt`);
  if (!Array.isArray(manifest.install_receipts)) fail("installed platform receipts required");
  const installed = new Set();
  for (const item of manifest.install_receipts) {
    if (!item || item.os !== "macos" || item.vector_dispatch !== "CORTEX_VECTOR_DISPATCH_V2") fail("invalid installed Mac vector-dispatch receipt");
    const result = verifyPair(json(root, item.contract, `${item.os} platform contract`), json(root, item.receipt, `${item.os} installed receipt`));
    if (result.platform !== item.os) fail("installed receipt platform mismatch");
    installed.add(item.os);
  }
  if (!installed.has("macos") || installed.size !== 1) fail("one installed macOS receipt required");
  if (!manifest.event_history || !["sealed", "legacy-unsealed"].includes(manifest.event_history.status)) fail("event history must state sealed or legacy-unsealed");
  return { verified: true, release: { tag: release.tag, commit: release.commit, tree: release.tree, generation: release.generation, target: release.target }, event_history: manifest.event_history.status };
}

function cli() {
  const args = process.argv.slice(2); const path = args[args.indexOf("--manifest") + 1];
  if (!path || args.includes("--help")) { console.error("usage: verify-release-evidence.mjs --manifest PATH"); process.exit(path ? 0 : 2); }
  try { process.stdout.write(`${JSON.stringify(verifyReleaseEvidence(JSON.parse(readFileSync(resolve(process.cwd(), path), "utf8")), process.cwd()), null, 2)}\n`); }
  catch (error) { console.error(error.message); process.exit(1); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli();
