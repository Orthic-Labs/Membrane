#!/usr/bin/env node
// Issue the native-only seal only after independent release & installed gates pass.
// This command validates receipts; it never builds, signs, installs, or runs tests.
import { createHash, randomUUID } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SHA256 = /^[a-f0-9]{64}$/u;
const TARGET = "windows-x86_64";
const SEAL_SCHEMA = "membrane.native-only-seal.v1";
const RELEASE_SCHEMA = "membrane.release-evidence.v1";
const QUALIFICATION_SCHEMA = "membrane.windows-installed-qualification.v1";
// Qualification is an observation of one installed current root, not a
// durable entitlement. Keep its validity window bounded so an old receipt
// cannot certify a later install merely because its installer hash still
// happens to match.
const MAX_QUALIFICATION_AGE_MS = 24 * 60 * 60 * 1000;
const REQUIRED_LIFECYCLE = [
  "install", "startup", "hubHealth", "tray", "popup", "renderer", "mcp17",
  "nativeHostCutover", "blueprintHubHosted", "blueprintHubOffOneShot", "downgrade",
  "upgrade", "stateContinuity", "uninstall", "residue", "nativeOnlyProcessTree",
  "runtimeInventory", "currentRootActivation", "doctor", "hubOffManualBlueprint",
  "residentFileChangeRefresh", "zeroInterpreterProcessTree",
];

const fail = (message) => { throw new Error(`FAIL CLOSED: ${message}`); };

function regularFile(path, label) {
  const full = resolve(path ?? "");
  if (!path || !existsSync(full)) fail(`${label} is missing: ${path ?? ""}`);
  const stat = lstatSync(full);
  if (stat.isSymbolicLink() || !stat.isFile()) fail(`${label} must be a regular file: ${path}`);
  return full;
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function readInput(path, label) {
  const full = regularFile(path, label);
  let value;
  try { value = JSON.parse(readFileSync(full, "utf8")); }
  catch (error) { fail(`${label} is not valid JSON: ${error.message}`); }
  return { path: full, sha256: sha256(full), value };
}

function object(value, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${label} must be an object`);
  return value;
}

function requiredString(value, label) {
  if (typeof value !== "string" || value.length === 0) fail(`${label} is required`);
  return value;
}

function digest(value, label) {
  if (typeof value !== "string" || !SHA256.test(value)) fail(`${label} must be lowercase SHA-256`);
  return value;
}

function semver(value, label) {
  const text = requiredString(value, label);
  if (!/^v?\d+\.\d+\.\d+$/u.test(text)) fail(`${label} must be v?x.y.z`);
  return text.replace(/^v/u, "").split(".").map(Number);
}

function olderVersion(previous, current) {
  for (let index = 0; index < 3; index += 1) {
    if (previous[index] !== current[index]) return previous[index] < current[index];
  }
  return false;
}

function releaseArtifact(value) {
  const release = object(value.release, "release evidence release");
  if (value.schema !== RELEASE_SCHEMA) fail(`release evidence schema must be ${RELEASE_SCHEMA}`);
  if (release.target !== TARGET) fail(`release evidence target must be ${TARGET}`);
  const artifact = object(value.artifact, "release evidence artifact");
  const artifactHash = digest(artifact.sha256, "release evidence artifact.sha256");
  if (digest(release.artifact_sha256, "release evidence release.artifact_sha256") !== artifactHash) {
    fail("release evidence artifact digest does not match release identity");
  }
  // Internal qualification binds an unsigned local candidate by its exact
  // artifact hash & source/build generation. It is independent of
  // Authenticode/release-channel trust, which is outside this NCL-03 lane.
  if (value.signing?.status === "unsigned" || value.profile === "internal-unsigned") {
    if (value.signing?.status === "release-signed" || value.certification === "signed-release") {
      fail("internal-unsigned release evidence claims signed-release identity");
    }
    if (!/^[a-f0-9]{64}$/u.test(String(release.generation || ""))) {
      fail("internal-unsigned release evidence lacks source/build generation");
    }
    requiredString(artifact.path, "release evidence artifact.path");
    return artifactHash;
  }
  if (value.event_history?.status !== "sealed") fail("release evidence event history is not sealed");
  const trust = object(value.platform_trust, "release evidence platform_trust");
  if (trust.kind !== "authenticode" || digest(trust.subject_sha256, "platform_trust.subject_sha256") !== artifactHash) {
    fail("release evidence lacks installer-bound Authenticode evidence");
  }
  if (!Array.isArray(value.signatures) || !value.signatures.some((entry) => entry?.kind === "ed25519" && entry.subject_sha256 === artifactHash)) {
    fail("release evidence lacks installer-bound release signature");
  }
  if (!Array.isArray(value.install_receipts) || value.install_receipts.length !== 1 || value.install_receipts[0]?.os !== "windows") {
    fail("release evidence requires one Windows installed receipt");
  }
  return artifactHash;
}

function qualificationArtifact(value, { now = new Date() } = {}) {
  if (value.schema !== QUALIFICATION_SCHEMA) fail(`qualification schema must be ${QUALIFICATION_SCHEMA}`);
  const internalUnsigned = value.profile === "internal-unsigned";
  if (value.platform !== TARGET || !["installed-local", "internal-unsigned"].includes(value.profile)) fail("qualification is not Windows installed-local evidence");
  if (internalUnsigned && value.certification !== "unsigned-functional") fail("internal-unsigned qualification certification is invalid");
  if (!internalUnsigned && value.certification === "unsigned-functional") fail("installed-local qualification cannot claim unsigned-functional certification");
  const generatedAt = requiredString(value.generatedAt, "qualification generatedAt");
  const generatedTime = Date.parse(generatedAt);
  const nowTime = now instanceof Date ? now.getTime() : Date.parse(String(now));
  if (!Number.isFinite(generatedTime) || !Number.isFinite(nowTime)) fail("qualification generatedAt is not an ISO timestamp");
  if (generatedTime > nowTime + 5 * 60 * 1000) fail("qualification generatedAt is in the future");
  if (nowTime - generatedTime > MAX_QUALIFICATION_AGE_MS) fail("qualification evidence is stale");
  const artifact = object(value.artifact, "qualification artifact");
  const artifactHash = digest(artifact.sha256, "qualification artifact.sha256");
  requiredString(artifact.path, "qualification artifact.path");
  const artifactVersion = requiredString(artifact.version, "qualification artifact.version");
  if (!internalUnsigned) {
    if (String(artifact.authenticode).toLowerCase() !== "valid") fail("qualification installer Authenticode is not valid");
    for (const field of ["signerSubject", "signerThumbprint", "timestampSubject", "timestampThumbprint"]) {
      requiredString(artifact[field], `qualification artifact.${field}`);
    }
  }
  const lifecycle = object(value.lifecycle, "qualification lifecycle");
  for (const field of REQUIRED_LIFECYCLE) {
    const lifecycleValue = String(lifecycle[field]).toLowerCase();
    const firstStableRepair = value.downgradeContract === "first-stable-layout-repair-v1";
    if (lifecycleValue !== "pass" && !(internalUnsigned && field === "downgrade" && firstStableRepair && lifecycleValue === "not_applicable")) {
      fail(`qualification lifecycle.${field} is not pass`);
    }
  }
  const installedCurrent = object(value.installedCurrent, "qualification installedCurrent");
  const installedRoot = requiredString(installedCurrent.root, "qualification installedCurrent.root");
  if (!/[\\/]current$/iu.test(installedRoot)) fail("qualification installedCurrent.root must be the stable current root");
  if (digest(installedCurrent.artifactSha256, "qualification installedCurrent.artifactSha256") !== artifactHash) {
    fail("qualification installedCurrent.artifactSha256 does not match installer artifact");
  }
  if (!Array.isArray(installedCurrent.files) || installedCurrent.files.length === 0) fail("qualification installedCurrent.files is missing");
  const installedPaths = new Set();
  let binaryCount = 0;
  for (const [index, entry] of installedCurrent.files.entries()) {
    const row = object(entry, `qualification installedCurrent.files[${index}]`);
    const path = requiredString(row.path, `qualification installedCurrent.files[${index}].path`).replaceAll("\\", "/");
    if (path.startsWith("/") || path.includes("../") || path === ".." || installedPaths.has(path)) {
      fail(`qualification installedCurrent.files[${index}].path is not a unique relative path`);
    }
    installedPaths.add(path);
    digest(row.sha256, `qualification installedCurrent.files[${index}].sha256`);
    if (!Number.isSafeInteger(row.size) || row.size < 0) fail(`qualification installedCurrent.files[${index}].size is invalid`);
    if (/\.exe$/iu.test(path)) binaryCount += 1;
  }
  if (binaryCount === 0) fail("qualification installedCurrent.files has no native executable");
  const observations = value.runtime?.lifecycleObservations;
  if (!Array.isArray(observations) || observations.length === 0 || observations.some((entry) => entry?.observed !== true)) {
    fail("qualification runtime.lifecycleObservations are missing or not observed");
  }
  if (value.downgradeContract !== "signed-version-liveness-durable-state-v1" &&
      !(internalUnsigned && value.downgradeContract === "first-stable-layout-repair-v1")) {
    fail("qualification downgrade contract is invalid");
  }
  const downgrade = object(value.downgrade, "qualification downgrade");
  const previousArtifact = value.previousArtifact == null ? null : object(value.previousArtifact, "qualification previousArtifact");
  if (previousArtifact == null) {
    if (!(internalUnsigned && value.downgradeContract === "first-stable-layout-repair-v1" && downgrade.status === "not_applicable")) {
      fail("qualification previousArtifact is missing");
    }
  } else {
    digest(previousArtifact.sha256, "qualification previousArtifact.sha256");
    if (!internalUnsigned && String(previousArtifact.authenticode).toLowerCase() !== "valid") {
      fail("qualification previous installer Authenticode is not valid");
    }
    requiredString(previousArtifact.path, "qualification previousArtifact.path");
    if (!internalUnsigned) {
      for (const field of ["signerSubject", "signerThumbprint", "timestampSubject", "timestampThumbprint"]) {
        requiredString(previousArtifact[field], `qualification previousArtifact.${field}`);
      }
    }
    if (requiredString(downgrade.version, "qualification downgrade.version")
        !== requiredString(previousArtifact.version, "qualification previousArtifact.version")) {
      fail("qualification downgrade version does not match previous installer");
    }
    if (!olderVersion(
      semver(previousArtifact.version, "qualification previousArtifact.version"),
      semver(artifactVersion, "qualification artifact.version"),
    )) fail("qualification previous installer is not older than current installer");
    if (downgrade.durableState !== "preserved") fail("qualification downgrade durable state was not preserved");
    if (!Array.isArray(downgrade.processTree) || downgrade.processTree.length < 1) {
      fail("qualification downgrade process tree is missing");
    }
    for (const [index, entry] of downgrade.processTree.entries()) {
      const row = object(entry, `qualification downgrade.processTree[${index}]`);
      requiredString(row.name, `qualification downgrade.processTree[${index}].name`);
      requiredString(row.executablePath, `qualification downgrade.processTree[${index}].executablePath`);
      digest(row.executableSha256, `qualification downgrade.processTree[${index}].executableSha256`);
    }
    if (!Array.isArray(downgrade.installedContent) || downgrade.installedContent.length < 1) {
      fail("qualification downgrade installed content is missing");
    }
    for (const [index, entry] of downgrade.installedContent.entries()) {
      const row = object(entry, `qualification downgrade.installedContent[${index}]`);
      requiredString(row.path, `qualification downgrade.installedContent[${index}].path`);
      digest(row.sha256, `qualification downgrade.installedContent[${index}].sha256`);
    }
  }
  if (value.upgradeContract !== "full-native-upgrade-uninstall-v1") {
    fail("qualification upgrade/uninstall contract is invalid");
  }
  const upgrade = object(value.upgrade, "qualification upgrade");
  if (requiredString(upgrade.Version, "qualification upgrade.Version") !== artifactVersion) {
    fail("qualification upgrade version does not match current installer");
  }
  const upgradeHealth = object(upgrade.Health, "qualification upgrade.Health");
  if (upgradeHealth.serviceId !== "membrane-hub" || upgradeHealth.nativeOnly !== true) {
    fail("qualification upgrade native Hub health is invalid");
  }
  if (!Array.isArray(upgrade.McpTools) || upgrade.McpTools.length !== 17) {
    fail("qualification upgrade MCP surface is not exactly 17 tools");
  }
  if (!Array.isArray(upgrade.ProcessTree) || upgrade.ProcessTree.length < 1
      || !Array.isArray(upgrade.Assets) || upgrade.Assets.length < 1) {
    fail("qualification upgrade process/renderer evidence is missing");
  }
  object(upgrade.Blueprint, "qualification upgrade.Blueprint");
  const uninstall = object(value.uninstallEvidence, "qualification uninstallEvidence");
  for (const field of ["installRootRemoved", "processesRemoved", "shortcutsRemoved", "registryRemoved", "durableStatePreserved"]) {
    if (uninstall[field] !== true) fail(`qualification uninstallEvidence.${field} is not true`);
  }
  const environment = object(value.environment, "qualification environment");
  if (environment.developmentCheckoutRequired !== false || environment.networkInterpreterFetch !== false) {
    fail("qualification environment is not isolated from checkout/interpreter fetch");
  }
  if (value.runtime?.blueprint?.hubOwned !== true && value.runtime?.blueprint?.nativeOnly !== true) {
    fail("qualification does not prove installed Blueprint lifecycle");
  }
  const processTrees = [downgrade.processTree, upgrade.ProcessTree, value.processTree, value.runtime?.processTree]
    .filter(Array.isArray).flat();
  for (const entry of processTrees) {
    const text = `${entry.name ?? ""} ${entry.executablePath ?? ""}`.toLowerCase();
    if (/node(?:\.exe)?|python(?:\.exe)?|(?:^|[\\/])(?:sh|bash)(?:\.exe)?$|blueprint[\\/]?(?:scripts|src|watchman|release)/i.test(text)) {
      fail("qualification process tree contains Blueprint interpreter runtime");
    }
  }
  return artifactHash;
}

function verifyRuntimeLanguage(value) {
  if (value.schemaVersion !== 1 || value.artifact !== "membrane.runtime-language-manifest") fail("runtime-language manifest schema/artifact invalid");
  if (value.enforcementMode !== "sealed") fail("runtime-language enforcement is not sealed");
  if (value.totals?.productionInterpreterRows !== 0) fail("runtime-language manifest has production interpreter rows");
  if (value.totals?.boundedExternalInterpreterRows !== 0) fail("runtime-language manifest has bounded external interpreter rows");
  if ((value.rows ?? []).some((row) => row.production_reachable && ["node", "python", "shell"].includes(row.runtime))) {
    fail("runtime-language manifest has a production interpreter row");
  }
  for (const field of ["errors", "blockers", "sealBlockers"]) {
    if (Array.isArray(value[field]) && value[field].length > 0) fail(`runtime-language manifest has ${field}`);
  }
  if ((value.rows ?? []).some((row) => row.seal_blocker_note != null)) fail("runtime-language manifest contains a seal blocker note");
}

function verifyGraph(value) {
  if (value.schemaVersion !== 2 || value.artifact !== "membrane.invocation-graph") fail("invocation graph schema/artifact invalid");
}

function verifyFixtures(value) {
  if (value.schemaVersion !== 1 || value.artifact !== "membrane.native-contract-fixtures" || value.immutable !== true) {
    fail("native contract fixture manifest schema/artifact invalid");
  }
  if (!Array.isArray(value.contracts) || value.contracts.length === 0) fail("native contract fixture manifest has no contracts");
}

export function issueNativeOnlySeal({ releaseManifest, qualification, runtimeLanguageManifest, invocationGraph, nativeContractManifest, out, now = new Date() }) {
  const inputs = [
    ["releaseEvidence", releaseManifest],
    ["installedQualification", qualification],
    ["runtimeLanguageManifest", runtimeLanguageManifest],
    ["invocationGraph", invocationGraph],
    ["nativeContractManifest", nativeContractManifest],
  ].map(([kind, path]) => ({ kind, ...readInput(path, kind) }));
  const output = resolve(out ?? "");
  if (!out) fail("output path is required");
  if (existsSync(output)) {
    const stat = lstatSync(output);
    if (stat.isSymbolicLink() || !stat.isFile()) fail("output must not be a symlink or non-file");
    fail(`seal already exists: ${output}`);
  }
  if (inputs.some((input) => input.path === output)) fail("output must differ from every input");
  const releaseHash = releaseArtifact(inputs[0].value);
  const qualificationHash = qualificationArtifact(inputs[1].value, { now });
  if (releaseHash !== qualificationHash) fail("release artifact and installed qualification digests differ");
  const internalUnsignedRelease = inputs[0].value.signing?.status === "unsigned" || inputs[0].value.profile === "internal-unsigned";
  const internalUnsignedQualification = inputs[1].value.profile === "internal-unsigned";
  if (internalUnsignedRelease !== internalUnsignedQualification) {
    fail("release and installed qualification profiles are not aligned");
  }
  if (internalUnsignedRelease) {
    const releaseArtifactPath = requiredString(inputs[0].value.artifact.path, "release evidence artifact.path");
    const qualificationArtifactPath = requiredString(inputs[1].value.artifact.path, "qualification artifact.path");
    if (resolve(releaseArtifactPath).toLowerCase() !== resolve(qualificationArtifactPath).toLowerCase()) {
      fail("internal-unsigned candidate path differs from installed qualification artifact");
    }
  }
  verifyRuntimeLanguage(inputs[2].value);
  verifyGraph(inputs[3].value);
  verifyFixtures(inputs[4].value);
  mkdirSync(dirname(output), { recursive: true });
  const seal = {
    schema: SEAL_SCHEMA,
    status: "sealed",
    target: TARGET,
    artifact_sha256: releaseHash,
    inputs: Object.fromEntries(inputs.map(({ kind, path, sha256: hash }) => [kind, { path, sha256: hash }])),
    generatedAt: now instanceof Date ? now.toISOString() : new Date(now).toISOString(),
    qualificationProfile: inputs[1].value.profile,
    installedCurrent: {
      root: inputs[1].value.installedCurrent.root,
      artifactSha256: inputs[1].value.installedCurrent.artifactSha256,
      binaryCount: inputs[1].value.installedCurrent.files.filter((entry) => /\.exe$/iu.test(entry.path)).length,
      qualificationGeneratedAt: inputs[1].value.generatedAt,
    },
  };
  const temporary = `${output}.tmp-${randomUUID()}`;
  try {
    writeFileSync(temporary, `${JSON.stringify(seal, null, 2)}\n`, { encoding: "utf8", flag: "wx" });
    renameSync(temporary, output);
  } finally {
    if (existsSync(temporary)) unlinkSync(temporary);
  }
  return seal;
}

function parseArgs(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const key = {
      "--release-manifest": "releaseManifest",
      "--qualification": "qualification",
      "--runtime-language-manifest": "runtimeLanguageManifest",
      "--invocation-graph": "invocationGraph",
      "--native-contract-manifest": "nativeContractManifest",
      "--out": "out",
    }[flag];
    if (!key || index + 1 >= argv.length || argv[index + 1].startsWith("--")) fail(`unknown or incomplete argument: ${flag}`);
    values[key] = argv[++index];
  }
  for (const key of ["releaseManifest", "qualification", "runtimeLanguageManifest", "invocationGraph", "nativeContractManifest", "out"]) {
    if (!values[key]) fail(`--${key.replace(/[A-Z]/gu, (letter) => `-${letter.toLowerCase()}`)} is required`);
  }
  return values;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const seal = issueNativeOnlySeal(parseArgs(process.argv.slice(2)));
    process.stdout.write(`${JSON.stringify(seal, null, 2)}\n`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
