#!/usr/bin/env node
// MBR-910: binds packaging/oci/release.v1.json to a REAL release.
//
// This script never builds, signs, pushes, or runs a container -- Book-Mode's
// MBR-910 override forbids CI/automation and this task's hard rules forbid
// docker/podman/buildah/cosign invocation from an agent. Everything here is
// offline fs read + sha256 + schema validation.
//
// The binding it proves: the OCI contract's `identity` (tag, commit,
// release_generation) is read verbatim from MBR-903's own immutable
// evidence/releases/<releaseId>/release-generation.json (scripts/release/pipeline/
// multi-platform-release.mjs#writeImmutableReleaseGeneration) -- never
// hand-typed here -- and every evidence artifact (SBOM, Ed25519 signature,
// cosign receipt, rootless-health receipt, secret-scan receipt) and the Linux
// binary baked into the image must already exist on disk with a real,
// independently recomputed sha256. `scripts/release/verify-oci-release.mjs`
// (MBR-910's original source-contract validator, unmodified) is the single
// source of truth for what "ready" means; this script only assembles a
// candidate document and calls that validator before ever writing anything.
// Missing or invalid input -- a release-generation.json that doesn't exist
// yet, a base image that drifted from the Containerfile, a proof file that
// isn't there -- fails closed and packaging/oci/release.v1.json is left
// exactly as it was (state: "unavailable"). Nothing here ever fabricates a
// digest, version, or hash.
import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { isAbsolute, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

import { HEX40, HEX64, TAG } from "../pipeline/identity.mjs";
import { releaseId as computeReleaseId } from "../pipeline/multi-platform-release.mjs";
import { verifyOciRelease } from "../verify-oci-release.mjs";

const RELEASE_GENERATION_SCHEMA = "orthic.membrane.release-generation.v1";
const OCI_SCHEMA = "orthic.membrane.oci-release.v1";

const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };

function sha256OfFile(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

/**
 * Reads and validates one already-written MBR-903 release-generation record.
 * Refuses anything that is not exactly that schema, is a publish-intent
 * document (publish must stay false), or whose releaseId does not match the
 * app/version/commit it also carries -- catching a hand-edited or
 * cross-pasted record instead of trusting it verbatim.
 */
export function readReleaseGeneration(path) {
  if (!existsSync(path)) fail(`release-generation record not found: ${path} (run scripts/release/pipeline/multi-platform-release.mjs --write first)`);
  let doc;
  try { doc = JSON.parse(readFileSync(path, "utf8")); }
  catch (error) { fail(`release-generation record is not valid JSON: ${path}: ${error.message}`); }
  if (doc.schema !== RELEASE_GENERATION_SCHEMA) fail(`release-generation record has unsupported schema: ${path}`);
  if (doc.publish !== false) fail(`release-generation record must stay publish:false: ${path}`);
  const rel = doc.release ?? {};
  if (!TAG.test(rel.tag ?? "")) fail(`release-generation record tag invalid: ${path}`);
  if (!HEX40.test(rel.commit ?? "")) fail(`release-generation record commit invalid: ${path}`);
  if (!HEX64.test(rel.release_generation ?? "")) fail(`release-generation record release_generation invalid: ${path}`);
  const expectedId = computeReleaseId({ app: rel.app, version: rel.version, commit: rel.commit });
  if (expectedId !== rel.releaseId) fail(`release-generation record releaseId does not match its own app/version/commit: ${path}`);
  return doc;
}

function relativeProof(repoRoot, absolutePath, name) {
  if (!existsSync(absolutePath)) fail(`${name} evidence file not found: ${absolutePath}`);
  const stat = lstatSync(absolutePath);
  if (!stat.isFile() || stat.isSymbolicLink()) fail(`${name} evidence must be a regular file: ${absolutePath}`);
  const rel = relative(repoRoot, absolutePath);
  if (isAbsolute(rel) || rel.startsWith(`..${sep}`)) fail(`${name} evidence must live inside the repository: ${absolutePath}`);
  return { path: rel.split(sep).join("/"), sha256: sha256OfFile(absolutePath) };
}

function readContainerfileBase(repoRoot) {
  const path = resolve(repoRoot, "packaging", "oci", "Containerfile");
  const text = readFileSync(path, "utf8");
  const match = text.match(/^FROM\s+(\S+)/m);
  if (!match) fail(`Containerfile has no FROM line: ${path}`);
  return match[1];
}

/**
 * Pure assembly: builds the candidate ready-state OCI release document from
 * real, already-on-disk inputs and validates it with verifyOciRelease before
 * returning it. Never touches packaging/oci/release.v1.json itself -- callers
 * decide whether/where to write.
 */
export function buildOciRelease({
  repoRoot, releaseGenerationPath, image, base, binaryPath,
  sbomPath, ed25519Path, cosignPath, rootlessHealthPath, secretScanPath,
}) {
  const generation = readReleaseGeneration(resolve(repoRoot, releaseGenerationPath));
  const containerfileBase = readContainerfileBase(repoRoot);
  if (base !== containerfileBase) fail(`--base (${base}) does not match packaging/oci/Containerfile's FROM line (${containerfileBase}); the image actually built must match the source contract`);
  if (!/^[a-z0-9./_-]+@sha256:[a-f0-9]{64}$/.test(image ?? "")) fail("--image must be an exact digest-pinned reference (repo@sha256:<64 hex>)");
  if (!existsSync(resolve(repoRoot, binaryPath))) fail(`--binary not found: ${binaryPath}`);
  const artifactSha256 = sha256OfFile(resolve(repoRoot, binaryPath));

  const candidate = {
    schema: OCI_SCHEMA,
    state: "ready",
    publish: false,
    image,
    base,
    identity: {
      tag: generation.release.tag,
      commit: generation.release.commit,
      release_generation: generation.release.release_generation,
      artifact_sha256: artifactSha256,
    },
    evidence: {
      sbom: relativeProof(repoRoot, resolve(repoRoot, sbomPath), "sbom"),
      ed25519: relativeProof(repoRoot, resolve(repoRoot, ed25519Path), "ed25519"),
      cosign: relativeProof(repoRoot, resolve(repoRoot, cosignPath), "cosign"),
      rootlessHealth: relativeProof(repoRoot, resolve(repoRoot, rootlessHealthPath), "rootlessHealth"),
      secretScan: relativeProof(repoRoot, resolve(repoRoot, secretScanPath), "secretScan"),
    },
  };
  verifyOciRelease(candidate, repoRoot); // throws FAIL CLOSED on any defect; single source of truth.
  return candidate;
}

/**
 * Writes packaging/oci/release.v1.json only after buildOciRelease succeeds.
 * Idempotent: an identical candidate written twice succeeds silently; a
 * candidate that would change the currently-recorded ready release throws,
 * mirroring MBR-903's writeImmutableReleaseGeneration -- a bound release
 * identity is never silently overwritten.
 */
export function writeOciRelease(options) {
  const candidate = buildOciRelease(options);
  const path = resolve(options.repoRoot, "packaging", "oci", "release.v1.json");
  const next = `${JSON.stringify(candidate, null, 2)}\n`;
  if (existsSync(path)) {
    const existing = readFileSync(path, "utf8");
    const existingDoc = JSON.parse(existing);
    if (existingDoc.state === "ready" && existing.trim() !== next.trim()) {
      fail(`packaging/oci/release.v1.json already records a different ready release; a sealed OCI release identity must never be rewritten (${path})`);
    }
  }
  writeFileSync(path, next);
  return { path, candidate };
}

function parseArgs(argv) {
  const args = { repoRoot: process.cwd(), write: false };
  const need = (flag) => { const value = argv[++i]; if (value === undefined) fail(`${flag} requires a value`); return value; };
  for (var i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--release-generation") args.releaseGenerationPath = need(arg);
    else if (arg === "--image") args.image = need(arg);
    else if (arg === "--base") args.base = need(arg);
    else if (arg === "--binary") args.binaryPath = need(arg);
    else if (arg === "--sbom") args.sbomPath = need(arg);
    else if (arg === "--ed25519") args.ed25519Path = need(arg);
    else if (arg === "--cosign") args.cosignPath = need(arg);
    else if (arg === "--rootless-health") args.rootlessHealthPath = need(arg);
    else if (arg === "--secret-scan") args.secretScanPath = need(arg);
    else if (arg === "--repo-root") args.repoRoot = need(arg);
    else if (arg === "--write") args.write = true;
    else if (arg === "-h" || arg === "--help") args.help = true;
    else fail(`unknown argument: ${arg}`);
  }
  return args;
}

function usage() {
  console.log(
    "usage: generate-oci-release.mjs --release-generation PATH --image REPO@sha256:HEX --base REF@sha256:HEX\n" +
    "  --binary PATH --sbom PATH --ed25519 PATH --cosign PATH --rootless-health PATH --secret-scan PATH\n" +
    "  [--repo-root PATH] [--write]\n\n" +
    "Binds packaging/oci/release.v1.json to a real MBR-903 release-generation.json plus real,\n" +
    "already-produced evidence files. Every input must already exist on disk -- this command\n" +
    "never builds, signs, pushes, or runs an image. Without --write, prints the candidate\n" +
    "document only. Fails closed (no write) if any input is missing or invalid.",
  );
}

function cli() {
  const args = parseArgs(process.argv.slice(2));
  const required = ["releaseGenerationPath", "image", "base", "binaryPath", "sbomPath", "ed25519Path", "cosignPath", "rootlessHealthPath", "secretScanPath"];
  if (args.help || required.some((key) => !args[key])) { usage(); process.exit(args.help ? 0 : 2); }
  try {
    if (args.write) {
      const result = writeOciRelease(args);
      console.error(`[generate-oci-release] wrote: ${result.path}`);
      process.stdout.write(`${JSON.stringify(result.candidate, null, 2)}\n`);
    } else {
      process.stdout.write(`${JSON.stringify(buildOciRelease(args), null, 2)}\n`);
    }
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli();
