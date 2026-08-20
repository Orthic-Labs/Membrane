#!/usr/bin/env node
// Binds SBOM, provenance, and toolchain evidence to one sealed RightKit
// headless add-on manifest. This module never builds, signs, uploads, or
// publishes; absent receipts stay explicit gaps.
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { DEFAULT_LOCKFILES, generateSbom } from "./sbom.mjs";
import { verifyReleaseEvidence } from "../verify-release-evidence.mjs";

const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };
const SHA256 = /^[0-9a-f]{64}$/;
const SAFE_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
const ROLES = new Set(["command", "service", "icon", "license", "eula", "privacy", "third-party-notices"]);
export const RELEASE_EVIDENCE_SCHEMA = "membrane.release-evidence.v1";

function exact(value, keys, label) {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).sort().join("\0") !== [...keys].sort().join("\0")) fail(`${label} has unsupported fields`);
}
function sha256(bytes) { return createHash("sha256").update(bytes).digest("hex"); }
function git(cwd, args) {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  if (result.status !== 0) fail(`git ${args.join(" ")} failed: ${(result.stderr ?? "").trim() || `exit ${result.status}`}`);
  return result.stdout.trim();
}
function writeEvidenceFile(dir, name, content) {
  mkdirSync(dir, { recursive: true });
  const path = resolve(dir, name);
  writeFileSync(path, content);
  return { path, sha256: sha256(content) };
}

/** Reads a sealed add-on manifest and verifies every adjacent component. */
export function readAddonManifest({ manifestPath }) {
  if (!manifestPath || !existsSync(manifestPath)) fail(`addon-manifest.json not found: ${manifestPath}`);
  const raw = readFileSync(manifestPath);
  let manifest;
  try { manifest = JSON.parse(raw); } catch (error) { fail(`addon-manifest.json is not valid JSON: ${error.message}`); }
  exact(manifest, ["schema", "kind", "addon", "version", "commit", "platform", "targetTriple", "consumer", "files", "signedAt"], "add-on manifest");
  if (manifest.schema !== 1 || manifest.kind !== "headless-addon" || manifest.addon !== "membrane" || !SAFE_NAME.test(manifest.version)) fail("invalid add-on manifest identity");
  if (!/^[0-9a-f]{40}$/i.test(manifest.commit) || !["mac", "win"].includes(manifest.platform) || typeof manifest.targetTriple !== "string" || !manifest.targetTriple || !manifest.consumer || typeof manifest.consumer !== "object" || Array.isArray(manifest.consumer) || !Array.isArray(manifest.files) || typeof manifest.signedAt !== "string" || !manifest.signedAt) fail("invalid add-on manifest fields");
  const root = dirname(resolve(manifestPath));
  const roles = new Set();
  const names = new Set();
  for (const file of manifest.files) {
    exact(file, ["role", "name", "sha256", "sizeBytes", "executable", "signing"], "add-on file");
    if (!ROLES.has(file.role) || roles.has(file.role) || !SAFE_NAME.test(file.name) || names.has(file.name) || !SHA256.test(file.sha256) || !Number.isSafeInteger(file.sizeBytes) || file.sizeBytes < 0 || typeof file.executable !== "boolean") fail("invalid add-on component");
    if (file.executable && (!file.signing || file.signing.status !== "verified")) fail(`unsigned executable role: ${file.role}`);
    const path = resolve(root, file.name);
    if (relative(root, path).startsWith(`..${sep}`) || !existsSync(path) || !statSync(path).isFile()) fail(`sealed component missing: ${file.name}`);
    const bytes = readFileSync(path);
    if (bytes.length !== file.sizeBytes || sha256(bytes) !== file.sha256) fail(`sealed component hash mismatch: ${file.name}`);
    roles.add(file.role); names.add(file.name);
  }
  for (const role of ROLES) if (!roles.has(role)) fail(`missing add-on role: ${role}`);
  return { manifest, manifestPath: resolve(manifestPath), manifestSha256: sha256(raw) };
}

export function buildProvenance({ repoRoot, commit }) {
  const fieldSep = "\x1f";
  const raw = git(repoRoot, ["show", "-s", `--format=${["%H", "%T", "%P", "%an <%ae>", "%aI", "%cn <%ce>", "%cI", "%s"].join(fieldSep)}`, commit]);
  const [hash, tree, parents, author, authorDate, committer, committerDate, subject] = raw.split(fieldSep);
  if (hash !== commit) fail(`git show returned commit ${hash}, expected ${commit}`);
  return { schema: "membrane.provenance.v1", commit: hash, tree, parents: parents ? parents.split(" ").filter(Boolean) : [], author, authorDate, committer, committerDate, subject };
}

export function buildToolchain({ repoRoot }) {
  const pkg = JSON.parse(readFileSync(resolve(repoRoot, "package.json"), "utf8"));
  const cargo = readFileSync(resolve(repoRoot, "engine/Cargo.toml"), "utf8");
  return {
    schema: "membrane.toolchain.v1",
    node: { engines: pkg.engines ?? null, packageManager: pkg.packageManager ?? null },
    addon: { packageManager: pkg.packageManager ?? null, checks: ["test", "test:all"] },
    rust: { edition: cargo.match(/\nedition = "([^"]+)"/)?.[1] ?? null, rustToolchainToml: existsSync(resolve(repoRoot, "rust-toolchain.toml")) },
  };
}

export function buildAddonEvidence({ repoRoot, manifestPath, sbomLockfiles = DEFAULT_LOCKFILES, evidenceRoot }) {
  if (!repoRoot) fail("repoRoot is required");
  const addon = readAddonManifest({ manifestPath });
  const evidenceDir = evidenceRoot ?? resolve(repoRoot, "evidence", "addons", addon.manifestSha256);
  const sbom = generateSbom({ repoRoot, lockfiles: sbomLockfiles });
  const sbomFile = writeEvidenceFile(evidenceDir, "sbom.json", `${JSON.stringify(sbom, null, 2)}\n`);
  const provenanceFile = writeEvidenceFile(evidenceDir, "provenance.json", `${JSON.stringify(buildProvenance({ repoRoot, commit: addon.manifest.commit }), null, 2)}\n`);
  const toolchainFile = writeEvidenceFile(evidenceDir, "toolchain.json", `${JSON.stringify(buildToolchain({ repoRoot }), null, 2)}\n`);
  const gaps = sbom.gaps.map((gap) => ({ kind: "sbom-component", ...gap }));
  gaps.push(
    { kind: "tests", reason: "release-evidence.v1 requires >=1 real test receipt; no release test receipt was supplied" },
    { kind: "signatures", reason: "sealed component signing is verified, but no detached release-evidence signature receipt was supplied" },
    { kind: "platform_trust", reason: `no platform-trust receipt was supplied for ${addon.manifest.platform}` },
    { kind: "install_receipts", reason: "no installed Membrane package receipt was supplied" },
    { kind: "event_history", reason: "no sealed-vs-legacy event-history receipt was supplied" },
  );
  return {
    addonManifest: { path: addon.manifestPath, sha256: addon.manifestSha256, files: addon.manifest.files },
    release: { addon: addon.manifest.addon, version: addon.manifest.version, commit: addon.manifest.commit, platform: addon.manifest.platform, targetTriple: addon.manifest.targetTriple, consumer: addon.manifest.consumer },
    artifact: { name: "addon-manifest.json", sha256: addon.manifestSha256, path: addon.manifestPath },
    sbom: sbomFile, provenance: provenanceFile, toolchain: toolchainFile, status: "incomplete", gaps,
  };
}

export function assembleReleaseEvidenceManifest({ release, artifact, sbom, provenance, toolchain, tests, signatures, platform_trust, compatibility, install_receipts, event_history }) {
  for (const [name, value] of Object.entries({ release, artifact, sbom, provenance, toolchain, tests, signatures, platform_trust, compatibility, install_receipts, event_history })) if (value === undefined) fail(`assembleReleaseEvidenceManifest: missing ${name}`);
  return { schema: RELEASE_EVIDENCE_SCHEMA, release, artifact, sbom, provenance, toolchain, tests, signatures, platform_trust, compatibility, install_receipts, event_history };
}

function parseArgs(argv) {
  const args = { repoRoot: process.cwd() };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--addon-manifest") args.manifestPath = argv[++i];
    else if (arg === "--repo-root") args.repoRoot = argv[++i];
    else if (arg === "--evidence-root") args.evidenceRoot = argv[++i];
    else if (arg === "-h" || arg === "--help") args.help = true;
    else fail(`unknown argument: ${arg}`);
  }
  return args;
}
function usage() { console.log("usage: manifest.mjs --addon-manifest PATH [--repo-root PATH]\n\nVerifies sealed add-on bytes, writes SBOM/provenance/toolchain evidence, and reports remaining receipts."); }
function cli() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help || !args.manifestPath) { usage(); process.exit(args.help ? 0 : 2); }
  try { process.stdout.write(`${JSON.stringify(buildAddonEvidence({ repoRoot: resolve(args.repoRoot), manifestPath: resolve(args.manifestPath), evidenceRoot: args.evidenceRoot ? resolve(args.evidenceRoot) : undefined }), null, 2)}\n`); }
  catch (error) { console.error(error.message); process.exit(1); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli();
