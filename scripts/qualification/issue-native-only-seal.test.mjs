import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { issueNativeOnlySeal } from "./issue-native-only-seal.mjs";

const source = readFileSync(new URL("./issue-native-only-seal.mjs", import.meta.url), "utf8");

test("native-only seal issuer is receipt-bound & fail-closed", () => {
  for (const term of [
    "membrane.native-only-seal.v1",
    "membrane.release-evidence.v1",
    "membrane.windows-installed-qualification.v1",
    "signed-version-liveness-durable-state-v1",
    "full-native-upgrade-uninstall-v1",
    "productionInterpreterRows",
    "boundedExternalInterpreterRows",
    "nativeOnlyProcessTree",
    "installedCurrent",
    "artifactSha256",
    "lifecycleObservations",
    "qualification evidence is stale",
    "platform_trust",
    "authenticode",
    "Object.fromEntries(inputs",
    "renameSync",
    "lstatSync",
  ]) assert.ok(source.includes(term), term);
  // The issuer must require zero bounded-external interpreter rows: no
  // bundled-package (e.g. bundled `blueprint/`) allowlist may reintroduce a
  // bounded-external carve-out. blueprint/ ships as a live installable
  // Node package (package.json bin entries, fixtures, capability
  // inventory, and release SBOM all enumerate it) so any nonzero bounded
  // count is a real Node payload in the sealed artifact.
  assert.match(source, /boundedExternalInterpreterRows\s*!==\s*0/);
  assert.doesNotMatch(source, /blueprint-bundled-runtime-blueprint|blueprint-bundled-launchers-blueprint/,
    "issuer must not carry a bundled-blueprint bounded-external allowlist");
  assert.match(source, /releaseHash !== qualificationHash/);
  assert.match(source, /previous installer is not older than current installer/);
  assert.match(source, /seal already exists/);
  assert.doesNotMatch(source, /spawnSync|execFileSync|cargo|pnpm|tauri/);
});

// A 64-hex-char stand-in artifact digest shared by the release and
// qualification fixtures below (the issuer requires them to match).
const ARTIFACT_SHA256 = "a".repeat(64);

function validReleaseManifest() {
  return {
    schema: "membrane.release-evidence.v1",
    release: { target: "windows-x86_64", artifact_sha256: ARTIFACT_SHA256 },
    event_history: { status: "sealed" },
    artifact: { sha256: ARTIFACT_SHA256 },
    platform_trust: { kind: "authenticode", subject_sha256: ARTIFACT_SHA256 },
    signatures: [{ kind: "ed25519", subject_sha256: ARTIFACT_SHA256 }],
    install_receipts: [{ os: "windows" }],
  };
}

function validQualification() {
  const REQUIRED_LIFECYCLE = [
    "install", "startup", "hubHealth", "tray", "popup", "renderer", "mcp17",
    "nativeHostCutover", "blueprintHubHosted", "blueprintHubOffOneShot", "downgrade",
    "upgrade", "stateContinuity", "uninstall", "residue", "nativeOnlyProcessTree",
    "runtimeInventory", "currentRootActivation", "doctor", "hubOffManualBlueprint",
    "residentFileChangeRefresh", "zeroInterpreterProcessTree",
  ];
  const lifecycle = Object.fromEntries(REQUIRED_LIFECYCLE.map((field) => [field, "pass"]));
  return {
    schema: "membrane.windows-installed-qualification.v1",
    generatedAt: new Date().toISOString(),
    platform: "windows-x86_64",
    profile: "installed-local",
    artifact: {
      path: "candidate.exe",
      sha256: ARTIFACT_SHA256,
      version: "2.0.0",
      authenticode: "valid",
      signerSubject: "s", signerThumbprint: "t", timestampSubject: "s", timestampThumbprint: "t",
    },
    lifecycle,
    installedCurrent: {
      root: "c:\\installed\\current",
      artifactSha256: ARTIFACT_SHA256,
      files: [
        { path: "membrane.exe", size: 1, sha256: "c".repeat(64) },
      ],
    },
    downgradeContract: "signed-version-liveness-durable-state-v1",
    previousArtifact: {
      sha256: "b".repeat(64),
      version: "1.0.0",
      authenticode: "valid",
      path: "prev.exe",
      signerSubject: "s", signerThumbprint: "t", timestampSubject: "s", timestampThumbprint: "t",
    },
    downgrade: {
      version: "1.0.0",
      durableState: "preserved",
      processTree: [{ name: "membrane-hub.exe", executablePath: "c:\\hub.exe", executableSha256: "c".repeat(64) }],
      installedContent: [{ path: "c:\\hub.exe", sha256: "c".repeat(64) }],
    },
    upgradeContract: "full-native-upgrade-uninstall-v1",
    upgrade: {
      Version: "2.0.0",
      Health: { serviceId: "membrane-hub", nativeOnly: true },
      McpTools: Array.from({ length: 17 }, (_, index) => `tool-${index}`),
      ProcessTree: [{ name: "membrane-hub.exe", executablePath: "c:\\hub.exe" }],
      Assets: [{ name: "renderer" }],
      Blueprint: {},
    },
    uninstallEvidence: {
      installRootRemoved: true, processesRemoved: true, shortcutsRemoved: true,
      registryRemoved: true, durableStatePreserved: true,
    },
    environment: { developmentCheckoutRequired: false, networkInterpreterFetch: false },
    runtime: { blueprint: { nativeOnly: true }, lifecycleObservations: [{ id: "install", observed: true }] },
    processTree: [],
  };
}

function validInvocationGraph() {
  return { schemaVersion: 2, artifact: "membrane.invocation-graph" };
}

function validNativeContractManifest() {
  return {
    schemaVersion: 1,
    artifact: "membrane.native-contract-fixtures",
    immutable: true,
    contracts: ["contract-1"],
  };
}

test("issuer refuses to seal while bundled-blueprint (or any) bounded external interpreter rows remain", () => {
  const dir = mkdtempSync(join(tmpdir(), "native-only-seal-"));
  try {
    const write = (name, value) => {
      const path = join(dir, name);
      writeFileSync(path, JSON.stringify(value));
      return path;
    };
    const releaseManifest = write("release.json", validReleaseManifest());
    const qualification = write("qualification.json", validQualification());
    const invocationGraph = write("invocation-graph.json", validInvocationGraph());
    const nativeContractManifest = write("native-contract-fixtures.json", validNativeContractManifest());

    // Control: with zero bounded-external rows (and zero production rows),
    // sealing succeeds — proving the fixtures above are otherwise valid.
    const runtimeLanguageManifestOk = write("runtime-language-manifest-ok.json", {
      schemaVersion: 1,
      artifact: "membrane.runtime-language-manifest",
      enforcementMode: "sealed",
      totals: { productionInterpreterRows: 0, boundedExternalInterpreterRows: 0 },
      rows: [],
    });
    const seal = issueNativeOnlySeal({
      releaseManifest, qualification, runtimeLanguageManifest: runtimeLanguageManifestOk,
      invocationGraph, nativeContractManifest, out: join(dir, "seal-ok.json"),
    });
    assert.equal(seal.status, "sealed");

    // The actual proof: nonzero bounded-external rows attributed to the
    // bundled `blueprint/` package (a live installable Node package per the
    // NCL-02 parity audit — package.json bin entries, qualification
    // fixtures, capability inventory, and release SBOM all enumerate it)
    // must still fail closed. No path-based allowlist may waive this.
    const runtimeLanguageManifestBundledBlueprint = write("runtime-language-manifest-bundled.json", {
      schemaVersion: 1,
      artifact: "membrane.runtime-language-manifest",
      enforcementMode: "sealed",
      totals: { productionInterpreterRows: 0, boundedExternalInterpreterRows: 4 },
      rows: [
        { runtime: "node", production_reachable: false, path: "blueprint/scripts/x.mjs" },
        { runtime: "node", production_reachable: false, path: "blueprint/src/x.mjs" },
        { runtime: "node", production_reachable: false, path: "blueprint/watchman/x.mjs" },
        { runtime: "node", production_reachable: false, path: "blueprint/release/x.mjs" },
      ],
    });
    assert.throws(() => issueNativeOnlySeal({
      releaseManifest, qualification, runtimeLanguageManifest: runtimeLanguageManifestBundledBlueprint,
      invocationGraph, nativeContractManifest, out: join(dir, "seal-bundled.json"),
    }), /FAIL CLOSED: runtime-language manifest has bounded external interpreter rows/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("issuer rejects stale qualification or a current-root artifact mismatch", () => {
  const dir = mkdtempSync(join(tmpdir(), "native-only-seal-freshness-"));
  try {
    const write = (name, value) => {
      const path = join(dir, name);
      writeFileSync(path, JSON.stringify(value));
      return path;
    };
    const releaseManifest = write("release.json", validReleaseManifest());
    const invocationGraph = write("invocation-graph.json", validInvocationGraph());
    const nativeContractManifest = write("native-contract-fixtures.json", validNativeContractManifest());
    const runtimeLanguageManifest = write("runtime-language-manifest.json", {
      schemaVersion: 1, artifact: "membrane.runtime-language-manifest", enforcementMode: "sealed",
      totals: { productionInterpreterRows: 0, boundedExternalInterpreterRows: 0 }, rows: [],
    });
    const stale = validQualification();
    stale.generatedAt = new Date(Date.now() - (25 * 60 * 60 * 1000)).toISOString();
    assert.throws(() => issueNativeOnlySeal({
      releaseManifest, qualification: write("qualification-stale.json", stale), runtimeLanguageManifest,
      invocationGraph, nativeContractManifest, out: join(dir, "seal-stale.json"),
    }), /FAIL CLOSED: qualification evidence is stale/);

    const mismatched = validQualification();
    mismatched.installedCurrent.artifactSha256 = "d".repeat(64);
    assert.throws(() => issueNativeOnlySeal({
      releaseManifest, qualification: write("qualification-mismatch.json", mismatched), runtimeLanguageManifest,
      invocationGraph, nativeContractManifest, out: join(dir, "seal-mismatch.json"),
    }), /FAIL CLOSED: qualification installedCurrent\.artifactSha256 does not match installer artifact/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("issuer accepts internal-unsigned candidate/install evidence without Authenticode", () => {
  const dir = mkdtempSync(join(tmpdir(), "native-only-seal-unsigned-"));
  try {
    const write = (name, value) => {
      const path = join(dir, name);
      writeFileSync(path, JSON.stringify(value));
      return path;
    };
    const qualificationValue = validQualification();
    qualificationValue.profile = "internal-unsigned";
    qualificationValue.certification = "unsigned-functional";
    qualificationValue.artifact.authenticode = "NotSigned";
    delete qualificationValue.artifact.signerSubject;
    delete qualificationValue.artifact.signerThumbprint;
    delete qualificationValue.artifact.timestampSubject;
    delete qualificationValue.artifact.timestampThumbprint;
    qualificationValue.previousArtifact.authenticode = "NotSigned";
    delete qualificationValue.previousArtifact.signerSubject;
    delete qualificationValue.previousArtifact.signerThumbprint;
    delete qualificationValue.previousArtifact.timestampSubject;
    delete qualificationValue.previousArtifact.timestampThumbprint;
    const releaseValue = validReleaseManifest();
    delete releaseValue.event_history;
    delete releaseValue.platform_trust;
    delete releaseValue.signatures;
    releaseValue.signing = { status: "unsigned" };
    releaseValue.release.generation = "b".repeat(64);
    releaseValue.artifact.path = qualificationValue.artifact.path;
    const releaseManifest = write("candidate.json", releaseValue);
    const qualification = write("qualification.json", qualificationValue);
    const invocationGraph = write("invocation-graph.json", validInvocationGraph());
    const nativeContractManifest = write("native-contract-fixtures.json", validNativeContractManifest());
    const runtimeLanguageManifest = write("runtime-language-manifest.json", {
      schemaVersion: 1, artifact: "membrane.runtime-language-manifest", enforcementMode: "sealed",
      totals: { productionInterpreterRows: 0, boundedExternalInterpreterRows: 0 }, rows: [],
    });
    const seal = issueNativeOnlySeal({
      releaseManifest, qualification, runtimeLanguageManifest, invocationGraph,
      nativeContractManifest, out: join(dir, "seal.json"),
    });
    assert.equal(seal.status, "sealed");
    assert.equal(seal.qualificationProfile, "internal-unsigned");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("internal-unsigned first stable repair may omit previous installer", () => {
  const dir = mkdtempSync(join(tmpdir(), "native-only-seal-unsigned-repair-"));
  try {
    const write = (name, value) => {
      const path = join(dir, name);
      writeFileSync(path, JSON.stringify(value));
      return path;
    };
    const qualificationValue = validQualification();
    qualificationValue.profile = "internal-unsigned";
    qualificationValue.certification = "unsigned-functional";
    qualificationValue.artifact.authenticode = "NotSigned";
    delete qualificationValue.artifact.signerSubject;
    delete qualificationValue.artifact.signerThumbprint;
    delete qualificationValue.artifact.timestampSubject;
    delete qualificationValue.artifact.timestampThumbprint;
    qualificationValue.previousArtifact = null;
    qualificationValue.downgradeContract = "first-stable-layout-repair-v1";
    qualificationValue.downgrade = { status: "not_applicable", durableState: "preserved" };
    qualificationValue.lifecycle.downgrade = "not_applicable";
    const releaseValue = validReleaseManifest();
    delete releaseValue.event_history;
    delete releaseValue.platform_trust;
    delete releaseValue.signatures;
    releaseValue.signing = { status: "unsigned" };
    releaseValue.release.generation = "e".repeat(64);
    releaseValue.artifact.path = qualificationValue.artifact.path;
    const seal = issueNativeOnlySeal({
      releaseManifest: write("candidate.json", releaseValue),
      qualification: write("qualification.json", qualificationValue),
      runtimeLanguageManifest: write("runtime-language-manifest.json", {
        schemaVersion: 1, artifact: "membrane.runtime-language-manifest", enforcementMode: "sealed",
        totals: { productionInterpreterRows: 0, boundedExternalInterpreterRows: 0 }, rows: [],
      }),
      invocationGraph: write("invocation-graph.json", validInvocationGraph()),
      nativeContractManifest: write("native-contract-fixtures.json", validNativeContractManifest()),
      out: join(dir, "seal.json"),
    });
    assert.equal(seal.qualificationProfile, "internal-unsigned");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
