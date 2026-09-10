import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import { NCL_02, validateNativeOnlySeal, validateNcl05Observation } from "./ncl-windows.mjs";

function fixture(action = "delete-after-parity") {
  const root = mkdtempSync(join(tmpdir(), "ncl02-"));
  const native = join(root, "native", "replacement.rs");
  const legacy = join(root, "legacy", "old.mjs");
  const i1 = join(root, "i1-tests.json");
  mkdirSync(join(root, "native"), { recursive: true });
  mkdirSync(join(root, "legacy"), { recursive: true });
  writeFileSync(native, "pub fn replacement() -> bool { true }\n");
  writeFileSync(legacy, "export const old = true;\n");
  writeFileSync(i1, JSON.stringify({ schema: "membrane.i1-test-evidence.v1", tests: ["parity_replacement"] }));
  const hash = createHash("sha256").update(readFileSync(i1)).digest("hex");
  const sourceRevision = execFileSync("git", ["rev-parse", "HEAD"], { cwd: resolve(new URL("../../../", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1")), encoding: "utf8" }).trim();
  const inventory = join(root, "interpreter-dispositions.json");
  writeFileSync(inventory, JSON.stringify([{ path: "legacy/old.mjs", owner: "native-cleanup", action, requiredProof: "proof", nativeDestinationOwner: "owner" }]));
  const ledger = join(root, "clusters.json");
  writeFileSync(ledger, JSON.stringify([{ owner: "owner", cluster: "replacement", legacyFiles: ["legacy/old.mjs"], nativeDestinationPaths: ["native/replacement.rs"], portedTestNames: ["parity_replacement"], callerCutover: true, i1EvidencePath: i1, i1EvidenceHash: hash, sourceRevision, ...(action === "commit-deletion" ? { deleted: true } : {}) }]));
  return { root, native, legacy, i1, inventory, ledger, sourceRevision };
}

test("NCL-02 explicitly passes zero pending rows while preserving terminal commit-deletion state", () => {
  const fx = fixture("commit-deletion");
  try {
    const result = NCL_02({ workspaceRoot: fx.root, interpreterDispositionsPath: fx.inventory, clusterLedgerPath: fx.ledger, i1EvidencePath: fx.i1, sourceRevision: fx.sourceRevision });
    assert.equal(result.status, "passed", result.reason);
    assert.equal(result.detail.pendingCount, 0);
    assert.equal(result.detail.committedCount, 1);
  } finally { rmSync(fx.root, { recursive: true, force: true }); }
});

test("NCL-02 passes an empty deletion scope without requiring unrelated ledger or I1 inputs", () => {
  const root = mkdtempSync(join(tmpdir(), "ncl02-empty-"));
  const inventory = join(root, "interpreter-dispositions.json");
  writeFileSync(inventory, JSON.stringify([{ path: "kept.mjs", owner: "native-cleanup", action: "retain-with-reachability-proof", requiredProof: "proof" }]));
  try {
    const result = NCL_02({ workspaceRoot: root, interpreterDispositionsPath: inventory, clusterLedgerPath: join(root, "missing-clusters.json") });
    assert.equal(result.status, "passed", result.reason);
    assert.match(result.reason, /no delete-after-parity rows/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("NCL-02 accepts a pending row only when cluster, assertion, cutover, destination, and fresh I1 hash join", () => {
  const fx = fixture();
  try {
    const result = NCL_02({ workspaceRoot: fx.root, interpreterDispositionsPath: fx.inventory, clusterLedgerPath: fx.ledger, i1EvidencePath: fx.i1, sourceRevision: fx.sourceRevision });
    assert.equal(result.status, "passed", result.reason);
    assert.equal(result.detail.checked[0].testNames[0], "parity_replacement");
  } finally { rmSync(fx.root, { recursive: true, force: true }); }
});

test("NCL-02 rejects stale I1 bytes even when cluster metadata claims parity", () => {
  const fx = fixture();
  try {
    const ledger = JSON.parse(readFileSync(fx.ledger, "utf8"));
    ledger[0].i1EvidenceHash = "0".repeat(64);
    writeFileSync(fx.ledger, JSON.stringify(ledger));
    const result = NCL_02({ workspaceRoot: fx.root, interpreterDispositionsPath: fx.inventory, clusterLedgerPath: fx.ledger, i1EvidencePath: fx.i1, sourceRevision: fx.sourceRevision });
    assert.notEqual(result.status, "passed");
    assert.match(result.reason, /I1 evidence hash/);
  } finally { rmSync(fx.root, { recursive: true, force: true }); }
});

test("NCL-02 rejects a delete-after-parity row without a unique cluster mapping", () => {
  const fx = fixture();
  try {
    writeFileSync(fx.ledger, JSON.stringify([{ cluster: "different", legacyFiles: ["legacy/other.mjs"] }]));
    const result = NCL_02({ workspaceRoot: fx.root, interpreterDispositionsPath: fx.inventory, clusterLedgerPath: fx.ledger, i1EvidencePath: fx.i1, sourceRevision: fx.sourceRevision });
    assert.notEqual(result.status, "passed");
    assert.match(result.reason, /unique cluster-ledger mapping/);
  } finally { rmSync(fx.root, { recursive: true, force: true }); }
});

const NCL_ROOT = "C:/Users/test/AppData/Local/Orthic Labs/Membrane/current";
const NCL_EXE = `${NCL_ROOT}/membrane.exe`;
const NCL_HASH = "a".repeat(64);
const NCL_IDENTITY = { root: NCL_ROOT, executable: NCL_EXE, executableSha256: NCL_HASH, releaseGeneration: `sha256:${"b".repeat(64)}`, mtimeMs: Date.now() - 1000 };
const owner = () => ({ pid: 10, ppid: null, name: "membrane.exe", executable: NCL_EXE, commandLine: NCL_EXE });
const action = (stdout, extra = {}) => ({ executable: NCL_EXE, sha256: NCL_HASH, nativeEvidence: true, terminal: true, timedOut: false, exitCode: 0, stdout, stderr: "", processTree: [owner()], forbiddenChildren: [], ...extra });
const binding = { schemaVersion: 1, mode: "bounded_explicit", installationId: "install-1", cortexStoreId: "sha256:store", releaseGeneration: NCL_IDENTITY.releaseGeneration, startupGeneration: 1, stableInstallRoot: NCL_ROOT, protocolVersion: 1, nativeOnly: true, subsystems: ["pull"], capabilities: ["memory", "explicit-call"], embedderDim: 384 };

function validNcl05Observation() {
  const bindingResponse = JSON.stringify({ schemaVersion: 1, binding, status: 200, data: {} });
  const sdkResponse = JSON.stringify({ schemaVersion: 1, binding, status: 200, data: [] });
  const federationResponse = JSON.stringify({ transport: "native", packet: { blocks: [], omissions: [] }, receipts: [] });
  const mcp = `${JSON.stringify({ jsonrpc: "2.0", id: 1, result: { protocolVersion: "2025-03-26", capabilities: {} } })}\n${JSON.stringify({ jsonrpc: "2.0", id: 2, result: { tools: [{ name: "membrane_context" }] } })}`;
  return {
    schema: "membrane.windows-native-observation.v1", platform: "windows", generatedAt: new Date().toISOString(), installedRoot: NCL_ROOT,
    buildIdentity: { root: NCL_ROOT, generation: NCL_IDENTITY.releaseGeneration, membraneSha256: NCL_HASH }, payloadInterpreters: [],
    surfaces: [
      { name: "cli", status: "passed", actions: [action(JSON.stringify({ schemaVersion: "LiveDiagnosticsServiceV1", surface: "membrane-live-diagnostics", audit: { schemaVersion: "live-diagnostics-audit.v1" }, endpoints: ["GET /diagnostics/capabilities"] }))] },
      { name: "mcp", status: "passed", actions: [action(mcp)] },
      { name: "sdk", status: "passed", actions: [action(bindingResponse), action(sdkResponse)] },
      { name: "federation", status: "passed", actions: [action(federationResponse)] },
    ],
  };
}

test("NCL-05 accepts only four exact successful installed surface calls", () => {
  const result = validateNcl05Observation(validNcl05Observation(), NCL_IDENTITY, { maxAgeMs: 60_000 });
  assert.equal(result.ok, true, result.reason);
});

test("NCL-05 rejects SDK typed/nonzero failures even when row status says passed", () => {
  const observation = validNcl05Observation();
  observation.surfaces.find((surface) => surface.name === "sdk").actions[1].exitCode = 2;
  observation.surfaces.find((surface) => surface.name === "sdk").actions[1].stdout = "";
  assert.equal(validateNcl05Observation(observation, NCL_IDENTITY, { maxAgeMs: 60_000 }).ok, false);
});

test("NCL-05 rejects stale or mismatched installed identity and interpreter descendants", () => {
  const stale = validNcl05Observation();
  stale.installedRoot = "C:/other/Membrane/current";
  assert.match(validateNcl05Observation(stale, NCL_IDENTITY, { maxAgeMs: 60_000 }).reason, /identity/iu);
  const interpreter = validNcl05Observation();
  interpreter.surfaces[0].surfaces = undefined;
  interpreter.surfaces[0].actions[0].processTree.push({ pid: 11, ppid: 10, name: "node.exe", executable: "C:/node.exe" });
  assert.match(validateNcl05Observation(interpreter, NCL_IDENTITY, { maxAgeMs: 60_000 }).reason, /interpreter/iu);
});

test("NCL-05 preserves MCP IDs/schema & requires SDK binding plus operation", () => {
  const badMcp = validNcl05Observation();
  badMcp.surfaces.find((surface) => surface.name === "mcp").actions[0].stdout = badMcp.surfaces.find((surface) => surface.name === "mcp").actions[0].stdout.replace('"id":2', '"id":3');
  assert.match(validateNcl05Observation(badMcp, NCL_IDENTITY, { maxAgeMs: 60_000 }).reason, /MCP/iu);
  const badSdk = validNcl05Observation();
  badSdk.surfaces.find((surface) => surface.name === "sdk").actions.pop();
  assert.match(validateNcl05Observation(badSdk, NCL_IDENTITY, { maxAgeMs: 60_000 }).reason, /SDK|binding/iu);
});

test("NCL-03 binds internal-unsigned seal to exact live current-root bytes & fresh qualification", () => {
  const root = mkdtempSync(join(tmpdir(), "ncl03-seal-"));
  try {
    const current = join(root, "current");
    mkdirSync(current);
    const binary = join(current, "membrane.exe");
    writeFileSync(binary, "native-candidate");
    const binaryHash = createHash("sha256").update(readFileSync(binary)).digest("hex");
    const generatedAt = new Date().toISOString();
    const qualification = {
      schema: "membrane.windows-installed-qualification.v1", platform: "windows-x86_64",
      profile: "internal-unsigned", certification: "unsigned-functional", generatedAt,
      artifact: { path: join(root, "candidate.exe"), sha256: "a".repeat(64), version: "2.0.0" },
      installedCurrent: { root: current, artifactSha256: "a".repeat(64), files: [{ path: "membrane.exe", size: 15, sha256: binaryHash }] },
      lifecycle: Object.fromEntries([
        "install", "startup", "hubHealth", "tray", "popup", "renderer", "mcp17", "nativeHostCutover",
        "blueprintHubHosted", "blueprintHubOffOneShot", "downgrade", "upgrade", "stateContinuity", "uninstall",
        "residue", "nativeOnlyProcessTree", "runtimeInventory", "currentRootActivation", "doctor",
        "hubOffManualBlueprint", "residentFileChangeRefresh", "zeroInterpreterProcessTree",
      ].map((field) => [field, "pass"])),
      runtime: { lifecycleObservations: [{ id: "native-only-process-tree", observed: true }] },
    };
    const qualificationPath = join(root, "qualification.json");
    writeFileSync(qualificationPath, JSON.stringify(qualification));
    const qualificationHash = createHash("sha256").update(readFileSync(qualificationPath)).digest("hex");
    const seal = {
      schema: "membrane.native-only-seal.v1", status: "sealed", target: "windows-x86_64",
      artifact_sha256: qualification.artifact.sha256, generatedAt,
      qualificationProfile: "internal-unsigned",
      installedCurrent: { root: current, binaryCount: 1 },
      inputs: { installedQualification: { path: qualificationPath, sha256: qualificationHash } },
    };
    assert.equal(validateNativeOnlySeal(seal, qualificationPath, root).ok, true);
    writeFileSync(binary, "tampered-candidate");
    assert.match(validateNativeOnlySeal(seal, qualificationPath, root).reason, /digest mismatch/iu);
    seal.generatedAt = new Date(Date.now() - (25 * 60 * 60 * 1000)).toISOString();
    assert.match(validateNativeOnlySeal(seal, qualificationPath, root).reason, /stale/iu);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
