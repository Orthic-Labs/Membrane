#!/usr/bin/env node
// Assemble Windows release evidence from already-produced, already-signed
// inputs. This command never builds, signs, installs, or contacts services.
import { createHash } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { basename, dirname, isAbsolute, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { verifyPair } from "../../../scripts/release/verify-platform-artifacts.mjs";

export const SCHEMA = "membrane.release-evidence.v1";
export const TARGET = "windows-x86_64";
export const VECTOR_DISPATCH = "CORTEX_VECTOR_DISPATCH_V2";

const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };
const SHA256 = /^[a-f0-9]{64}$/;
const COMMIT = /^[a-f0-9]{40}$/;
const TREE = /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/;
const TAG = /^v\d+\.\d+\.\d+$/;

function requiredString(value, label) {
  if (typeof value !== "string" || value.length === 0) fail(`${label} is required`);
  return value;
}

function pathValue(value, label) {
  if (typeof value === "string") return value;
  if (value && typeof value.path === "string") return value.path;
  fail(`${label} path is required`);
}

function rootFile(root, value, label) {
  const rootPath = resolve(root);
  const supplied = requiredString(pathValue(value, label), label);
  const candidate = resolve(rootPath, supplied);
  const rel = relative(rootPath, candidate);
  if (!rel || rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel)) fail(`${label} must be inside evidence root`);
  if (!existsSync(candidate)) fail(`${label} missing: ${supplied}`);
  const stat = lstatSync(candidate);
  if (stat.isSymbolicLink() || !stat.isFile()) fail(`${label} must be a regular file: ${supplied}`);
  const bytes = readFileSync(candidate);
  return {
    path: rel.replaceAll("\\", "/"),
    sha256: createHash("sha256").update(bytes).digest("hex"),
  };
}

function readJson(root, value, label) {
  const receipt = rootFile(root, value, label);
  try {
    return { receipt, value: JSON.parse(readFileSync(resolve(root, receipt.path), "utf8")) };
  } catch (error) {
    fail(`${label} must be valid JSON: ${error.message}`);
  }
}

function releaseIdentity(value, artifactSha256) {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail("release identity is required");
  const tag = requiredString(value.tag, "release.tag");
  const commit = requiredString(value.commit, "release.commit");
  const tree = requiredString(value.tree, "release.tree");
  const generation = requiredString(value.generation, "release.generation");
  if (!TAG.test(tag)) fail("release.tag must be immutable semver");
  if (!COMMIT.test(commit)) fail("release.commit must be lowercase 40-character SHA-1");
  if (!TREE.test(tree)) fail("release.tree must be lowercase 40- or 64-character digest");
  if (!SHA256.test(generation)) fail("release.generation must be lowercase SHA-256");
  if (value.target !== TARGET) fail(`release.target must be ${TARGET}`);
  if (value.vector_dispatch !== VECTOR_DISPATCH) fail(`release.vector_dispatch must be ${VECTOR_DISPATCH}`);
  if (value.artifact_sha256 !== undefined && value.artifact_sha256 !== artifactSha256) fail("release.artifact_sha256 does not match signed installer");
  return { tag, commit, tree, generation, target: TARGET, artifact_sha256: artifactSha256, vector_dispatch: VECTOR_DISPATCH };
}

function normalizedVersion(value, label) {
  const version = requiredString(value, label);
  if (!/^v?\d+\.\d+\.\d+$/.test(version)) fail(`${label} must be semver`);
  return `v${version.replace(/^v/, "")}`;
}

function trustReceipt(root, value, label, artifactSha256, expectedKind) {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${label} is required`);
  const kind = requiredString(value.kind, `${label}.kind`);
  if (!["ed25519", "authenticode"].includes(kind)) fail(`${label}.kind is unsupported`);
  if (expectedKind && kind !== expectedKind) fail(`${label}.kind must be ${expectedKind}`);
  const identity = requiredString(value.identity, `${label}.identity`);
  const subject = requiredString(value.subject_sha256, `${label}.subject_sha256`);
  if (!SHA256.test(subject) || subject !== artifactSha256) fail(`${label}.subject_sha256 must bind signed installer`);
  return { kind, identity, subject_sha256: subject, receipt: rootFile(root, value.receipt, `${label}.receipt`) };
}

function platformAcceptance(root, value, artifactSha256, release, inputArtifactPath) {
  if (!value || typeof value !== "object") fail("Windows platform contract & receipt are required");
  const contract = readJson(root, value.contract, "Windows platform contract");
  const receipt = readJson(root, value.receipt, "Windows platform receipt");
  verifyPair(contract.value, receipt.value);
  const platform = receipt.value;
  if (platform.platform !== "windows" || platform.version === undefined) fail("Windows receipt is not platform evidence");
  if (normalizedVersion(platform.version, "Windows receipt.version") !== release.tag) fail("Windows receipt.version does not match release.tag");
  if (platform.commit !== release.commit || platform.releaseGeneration !== release.generation) fail("Windows receipt identity does not match release");
  if (platform.artifact?.name !== basename(inputArtifactPath)) fail("Windows receipt artifact name does not match signed installer");
  if (platform.artifact?.sha256 !== artifactSha256) fail("Windows receipt artifact does not match signed installer");
  if (contract.value.commit !== release.commit || contract.value.releaseGeneration !== release.generation || contract.value.platform !== "windows") fail("Windows contract identity does not match release");
  if (contract.value.artifact?.sha256 !== artifactSha256) fail("Windows contract artifact does not match signed installer");
  return { contract: contract.receipt, receipt: receipt.receipt };
}

export function assembleWindowsReleaseEvidence(input = {}) {
  const root = resolve(requiredString(input.root ?? input.evidenceRoot, "root"));
  const artifact = rootFile(root, input.artifact ?? input.installer, "signed installer");
  const release = releaseIdentity(input.release, artifact.sha256);
  const platformAcceptanceInput = input.platformAcceptance ?? { contract: input.platformContract, receipt: input.platformReceipt };
  const install = platformAcceptance(root, platformAcceptanceInput, artifact.sha256, release, artifact.path);
  const signatures = input.signatures ?? input.signatureReceipts;
  if (!Array.isArray(signatures) || signatures.length === 0) fail("signature receipts are required");
  const normalizedSignatures = signatures.map((value, index) => trustReceipt(root, value, `signatures[${index}]`, artifact.sha256));
  if (!normalizedSignatures.some(({ kind }) => kind === "ed25519")) fail("ed25519 signature receipt is required");
  const platformTrust = trustReceipt(root, input.platformTrust, "platform_trust", artifact.sha256, "authenticode");
  const history = input.eventHistory;
  if (!history || !["sealed", "legacy-unsealed"].includes(history.status)) fail("event_history.status must be sealed or legacy-unsealed");
  const eventHistory = { status: history.status, receipt: rootFile(root, history.receipt, "event_history.receipt") };
  const manifest = {
    schema: SCHEMA,
    release,
    artifact,
    sbom: rootFile(root, input.sbom, "SBOM"),
    provenance: rootFile(root, input.provenance, "provenance"),
    toolchain: rootFile(root, input.toolchain, "toolchain"),
    tests: (Array.isArray(input.tests) ? input.tests : []).map((value, index) => rootFile(root, value, `tests[${index}]`)),
    signatures: normalizedSignatures,
    platform_trust: platformTrust,
    compatibility: rootFile(root, input.compatibility, "compatibility"),
    install_receipts: [{ os: "windows", vector_dispatch: VECTOR_DISPATCH, contract: install.contract, receipt: install.receipt }],
    event_history: eventHistory,
  };
  if (manifest.tests.length === 0) fail("test receipts are required");
  return manifest;
}

export function writeWindowsReleaseEvidence(input = {}) {
  const root = resolve(requiredString(input.root ?? input.evidenceRoot, "root"));
  const manifest = assembleWindowsReleaseEvidence({ ...input, root });
  const output = input.output ?? "docs/evidence/releases/RELEASE.json";
  const outputPath = resolve(root, requiredString(pathValue(output, "output")));
  const rel = relative(root, outputPath);
  if (!rel || rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel)) fail("output must be inside evidence root");
  mkdirSync(dirname(outputPath), { recursive: true });
  const staged = `${outputPath}.staged`;
  const bytes = Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`);
  writeFileSync(staged, bytes);
  if (!readFileSync(staged).equals(bytes)) fail("manifest staging mismatch");
  renameSync(staged, outputPath);
  return { manifest, path: rel.replaceAll("\\", "/") };
}

function cli(argv) {
  const args = [...argv];
  const get = (name) => { const index = args.indexOf(name); return index >= 0 ? args[index + 1] : undefined; };
  if (args.includes("--help") || !get("--input")) {
    console.log("usage: write-windows-release-evidence.mjs --input CONFIG.json [--root ROOT] [--out PATH]");
    return 0;
  }
  const configPath = resolve(get("--input"));
  const config = JSON.parse(readFileSync(configPath, "utf8"));
  const root = resolve(get("--root") ?? config.root ?? config.evidenceRoot ?? dirname(configPath));
  const output = get("--out") ?? config.output;
  const result = writeWindowsReleaseEvidence({ ...config, root, ...(output ? { output } : {}) });
  process.stdout.write(`${JSON.stringify(result.manifest, null, 2)}\n`);
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { process.exitCode = cli(process.argv.slice(2)); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
