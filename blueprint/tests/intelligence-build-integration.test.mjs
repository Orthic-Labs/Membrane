import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { createHash } from "node:crypto";

import { buildGraphGeneration } from "../src/graph/static-provider.mjs";
import { defineProvider } from "../src/providers/index.mjs";
import { pythonScipProvider } from "../src/providers/compilers/python-scip.mjs";
import {
  ALLOWED_PROVIDER_LICENSES,
  PROVIDER_MANIFEST_DIR,
  loadFirstPartyProviderManifest,
  readProviderArtifactBytes,
} from "../src/providers/semantic-orchestrator.mjs";

test("production graph build runs structural/framework/portable/convention layers", () => {
  const root = mkdtempSync(join(tmpdir(), "blueprint-intelligence-build-"));
  try {
    mkdirSync(join(root, "src"), { recursive: true });
    writeFileSync(join(root, "src", "base.ts"), "export class Base { run() {} }\n");
    writeFileSync(join(root, "src", "child.ts"), `import { Base } from './base.js';\nexport class Child extends Base { run() {} }\nconst endpoint = process.env.API_URL;\n`);
    writeFileSync(join(root, "src", "tools.ts"), `export function ping() {}\nmcp.tool("ping", ping);\n`);
    const generation = buildGraphGeneration(root);
    const providers = generation.augmentation?.providers;
    assert.ok(providers?.structuralIntelligence);
    assert.ok(providers?.frameworkIntelligence);
    assert.ok(providers?.portableIdentity);
    assert.equal(providers?.conventions?.policyAuthority, false);
    assert.ok(generation.nodes.some((node) => node.labels?.includes("ConfigKey") && node.name === "API_URL"));
    assert.ok(generation.nodes.some((node) => node.labels?.includes("ToolContract") && node.name === "ping"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// ---------------------------------------------------------------------------
// BPT-012 / BPT-010: the isolation contract and provider registration must be
// enforced on the PRODUCTION build path, not only in the async orchestrator.
// Every test below drives a real `buildGraphGeneration`.
// ---------------------------------------------------------------------------

function withRepo(run) {
  const root = mkdtempSync(join(tmpdir(), "blueprint-provider-bounds-"));
  try {
    mkdirSync(join(root, "src"), { recursive: true });
    writeFileSync(join(root, "src", "a.ts"), "export function a() {}\n");
    return run(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function scipDisposition(generation) {
  return generation.augmentation?.providers?.scip?.disposition ?? null;
}

test("build refuses a provider declaring network access before it can run", () => {
  let ran = false;
  const networked = {
    id: "test.networked", version: "1.0.0", kind: "compiler", protocolRange: ">=1 <2",
    capabilities: ["definitions"],
    permissions: { filesystem: "repo-read", network: "fetch-only", process: "none" },
    probe() { ran = true; return { state: "available" }; },
    collect() { ran = true; return { nodes: [], edges: [], reports: [] }; },
  };
  withRepo((root) => {
    assert.throws(() => buildGraphGeneration(root, { semanticProviders: [networked] }), { code: "provider_permissions_invalid" });
  });
  assert.equal(ran, false);
});

test("build refuses a provider declaring arbitrary process execution", () => {
  const commanding = {
    id: "test.commanding", version: "1.0.0", kind: "compiler", protocolRange: ">=1 <2",
    capabilities: ["definitions"],
    permissions: { filesystem: "repo-read", network: "none", process: "any-command" },
    probe() { return { state: "available" }; },
    collect() { return { nodes: [], edges: [], reports: [] }; },
  };
  withRepo((root) => {
    assert.throws(() => buildGraphGeneration(root, { semanticProviders: [commanding] }), { code: "provider_permissions_invalid" });
  });
});

test("build types an untyped provider probe crash as provider_crash", () => {
  const crashingProbe = defineProvider({
    id: "test.crashing-probe", version: "1.0.0", kind: "compiler", protocolRange: ">=1 <2",
    capabilities: ["definitions"],
    permissions: { filesystem: "repo-read", network: "none", process: "none" },
    probe() { throw new Error("probe boom"); },
    collect() { return { nodes: [], edges: [], reports: [] }; },
  });
  withRepo((root) => {
    const disposition = scipDisposition(buildGraphGeneration(root, { semanticProviders: [crashingProbe] }));
    assert.equal(disposition.disposition, "failed");
    assert.equal(disposition.code, "provider_crash");
  });
});

test("build gates opt-in process providers and never runs them unauthorized", () => {
  let probed = false;
  let collected = false;
  const optIn = defineProvider({
    id: "test.optin", version: "1.0.0", kind: "compiler", protocolRange: ">=1 <2",
    capabilities: ["definitions"],
    permissions: { filesystem: "repo-read", network: "none", process: "opt-in" },
    probe() { probed = true; return { state: "available" }; },
    collect() { collected = true; return { nodes: [], edges: [], reports: [] }; },
  });
  withRepo((root) => {
    const generation = buildGraphGeneration(root, { semanticProviders: [optIn] });
    const disposition = scipDisposition(generation);
    assert.equal(disposition.disposition, "failed");
    assert.equal(disposition.code, "provider_process_not_authorized");
  });
  assert.equal(probed, false, "process gating must refuse before the probe runs");
  assert.equal(collected, false);
});

test("build types an untyped provider crash as provider_crash", () => {
  const crashing = defineProvider({
    id: "test.crashing", version: "1.0.0", kind: "compiler", protocolRange: ">=1 <2",
    capabilities: ["definitions"],
    permissions: { filesystem: "repo-read", network: "none", process: "none" },
    probe() { return { state: "available" }; },
    collect() { throw new Error("boom"); },
  });
  withRepo((root) => {
    const generation = buildGraphGeneration(root, { semanticProviders: [crashing] });
    const disposition = scipDisposition(generation);
    assert.equal(disposition.disposition, "failed");
    assert.equal(disposition.code, "provider_crash");
  });
});

test("build refuses a provider whose declared identity drifts from its committed manifest", () => {
  const drifted = defineProvider({
    id: pythonScipProvider.id,
    version: "9.9.9-not-the-manifest",
    kind: "compiler",
    protocolRange: pythonScipProvider.protocolRange,
    capabilities: [...pythonScipProvider.capabilities],
    permissions: { ...pythonScipProvider.permissions },
    probe() { return { state: "available" }; },
    collect() { return { nodes: [], edges: [], reports: [] }; },
  });
  withRepo((root) => {
    assert.throws(() => buildGraphGeneration(root, { semanticProviders: [drifted] }), { code: "provider_manifest_identity_mismatch" });
  });
});

function withCommittedManifest(manifest, run) {
  const path = join(PROVIDER_MANIFEST_DIR, `${manifest.id}.json`);
  writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`);
  try {
    return run();
  } finally {
    rmSync(path, { force: true });
  }
}

function manifestTestProvider() {
  return defineProvider({
    id: "test.manifested", version: "1.0.0", kind: "compiler", protocolRange: ">=1 <2",
    capabilities: ["definitions"],
    permissions: { filesystem: "repo-read", network: "none", process: "none" },
    probe() { return { state: "available" }; },
    collect() { return { nodes: [], edges: [], reports: [] }; },
  });
}

test("build refuses a provider whose module bytes do not match its manifest integrity", () => {
  const provider = manifestTestProvider();
  withCommittedManifest({
    id: provider.id, version: provider.version, license: ALLOWED_PROVIDER_LICENSES[0],
    integrity: `sha256:${"0".repeat(64)}`,
    entry: "src/providers/compilers/python-scip.mjs",
  }, () => withRepo((root) => {
    assert.throws(() => buildGraphGeneration(root, { semanticProviders: [provider] }), { code: "provider_integrity_mismatch" });
  }));
});

test("build refuses a provider whose manifest licence is outside the allowlist", () => {
  const provider = manifestTestProvider();
  const entry = "src/providers/compilers/python-scip.mjs";
  const integrity = `sha256:${createHash("sha256").update(readProviderArtifactBytes({ entry })).digest("hex")}`;
  withCommittedManifest({
    id: provider.id, version: provider.version, license: "GPL-3.0-only", integrity, entry,
  }, () => withRepo((root) => {
    assert.throws(() => buildGraphGeneration(root, { semanticProviders: [provider] }), { code: "provider_license_rejected" });
  }));
});

test("build admits a provider whose committed manifest matches its real module bytes", () => {
  const provider = manifestTestProvider();
  const entry = "src/providers/compilers/python-scip.mjs";
  const integrity = `sha256:${createHash("sha256").update(readProviderArtifactBytes({ entry })).digest("hex")}`;
  withCommittedManifest({
    id: provider.id, version: provider.version, license: ALLOWED_PROVIDER_LICENSES[0], integrity, entry,
  }, () => withRepo((root) => {
    const generation = buildGraphGeneration(root, { semanticProviders: [provider] });
    assert.equal(scipDisposition(generation).disposition, "indexed");
  }));
});

test("the committed first-party manifest matches the shipped provider module", () => {
  const manifest = loadFirstPartyProviderManifest(pythonScipProvider);
  assert.equal(manifest.id, pythonScipProvider.id);
  assert.equal(manifest.version, pythonScipProvider.version);
  assert.ok(ALLOWED_PROVIDER_LICENSES.includes(manifest.license));
  const observed = `sha256:${createHash("sha256").update(readProviderArtifactBytes(manifest)).digest("hex")}`;
  assert.equal(observed, manifest.integrity);
  withRepo((root) => {
    // Default (first-party) build path still succeeds with registration live.
    assert.ok(buildGraphGeneration(root).augmentation?.providers?.scip);
  });
});
