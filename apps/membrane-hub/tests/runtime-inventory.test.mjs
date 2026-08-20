import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { RUNTIME_SPECS, runtimeInventory, verifyStagedInventory, verifyUnpackedArtifact, writeRuntimeInventory } from "../scripts/runtime-inventory.mjs";

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "membrane-hub-runtime-"));
  const make = (file, text = file) => { mkdirSync(join(file, ".."), { recursive: true }); writeFileSync(file, text); };
  for (const name of ["pull", "push", "cortex", "guide", "adapt", "install"]) make(join(root, `${name}.txt`));
  const workspaceFile = join(root, "dist", "install", "workspace", "__init__.py"); make(workspaceFile, "PACKAGE_SCHEMA = 'membrane-install-workspace-v1'\n");
  const files = [{ path: "__init__.py", sha256: createHash("sha256").update(readFileSync(workspaceFile)).digest("hex"), bytes: readFileSync(workspaceFile).byteLength }];
  const runtime = { python: ">=3.11", dependencies: [] };
  const packageSha256 = createHash("sha256").update(JSON.stringify({ schemaVersion: "membrane-install-workspace-v1", packageVersion: "1.0.0", files, runtime })).digest("hex");
  make(join(root, "dist", "install", "workspace-manifest.json"), `${JSON.stringify({ schemaVersion: "membrane-install-workspace-v1", packageVersion: "1.0.0", source: "membrane/install/workspace", generated: "source-to-dist", runtime, files, packageSha256 })}\n`);
  make(join(root, "src-tauri", "runtime", "blueprint", "bin", "blueprint"));
  make(join(root, "src-tauri", "runtime", "blueprint", "lib", "node"));
  for (const name of ["membrane-aarch64-apple-darwin", "cortex-service-aarch64-apple-darwin", "cortex-aarch64-apple-darwin"]) make(join(root, name));
  return { root, make, runtime: join(root, "src-tauri", "runtime") };
}
function specs() {
  return [
    { id: "membrane-command", component: "membrane", delivery: "externalBin", path: "membrane-{target}" },
    { id: "cortex-service", component: "cortex-service", delivery: "externalBin", path: "cortex-service-{target}" },
    { id: "cortex-cli", component: "cortex", delivery: "externalBin", path: "cortex-{target}" },
    { id: "cortex-contract", component: "cortex", axis: "cortex", delivery: "resource", path: "cortex.txt" },
    { id: "blueprint-runtime", component: "blueprint", axis: "blueprint", delivery: "preStagedResource", path: "src-tauri/runtime/blueprint", tree: true },
    { id: "pull-contract", component: "pull", axis: "pull", delivery: "resource", path: "pull.txt" },
    { id: "push-contract", component: "push", axis: "push", delivery: "resource", path: "push.txt" },
    { id: "guide-contract", component: "guide", axis: "guide", delivery: "resource", path: "guide.txt" },
    { id: "adapt-contract", component: "adapt", axis: "adapt", delivery: "resource", path: "adapt.txt", invocation: "membrane-sidecar" },
    { id: "install-workspace", component: "install-workspace", delivery: "resource", path: "dist/install/workspace", tree: true, extensions: [".py"] },
    { id: "install-workspace-manifest", component: "install-workspace-manifest", delivery: "resource", path: "dist/install/workspace-manifest.json", stageRoot: "resources/install-workspace" },
  ];
}

test("runtime closure records generated Blueprint, compiled sidecars & six axes", () => {
  const ids = new Set(RUNTIME_SPECS.map((spec) => spec.id));
  for (const id of ["membrane-command", "cortex-service", "blueprint-runtime", "pull-contract", "push-contract", "guide-contract", "adapt-contract", "runtime-schemas", "host-adapters", "install-workspace", "install-workspace-manifest", "hub-icons"]) assert.ok(ids.has(id), id);
  assert.equal(RUNTIME_SPECS.find((spec) => spec.id === "blueprint-runtime").delivery, "preStagedResource");
  for (const forbidden of ["cortex-store/src", "membrane-runtime/src/pull", "membrane-runtime/src/push", "../../blueprint/src", "../../adapt/src/adapt"]) assert.ok(!RUNTIME_SPECS.some((spec) => spec.path.includes(forbidden)), forbidden);
  const tauri = readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8");
  assert.match(tauri, /"resources": \["runtime"\]/);
  assert.match(tauri, /"externalBin": \["binaries\/cortex", "binaries\/cortex-service", "binaries\/membrane"\]/);
  const stager = readFileSync(new URL("../scripts/stage-runtime.mjs", import.meta.url), "utf8");
  assert.match(stager, /blueprint\/scripts\/release\/stage-runtime\.mjs/);
  assert.match(stager, /writeRuntimeInventory/);
});

test("runtime inventory hashes generated runtime, install manifest & rejects missing/extra/retired", () => {
  const { root, make, runtime } = fixture();
  try {
    writeRuntimeInventory({ hubDir: root, runtimeDir: runtime, specs: specs(), target: "aarch64-apple-darwin" });
    const manifest = verifyStagedInventory({ runtimeDir: runtime });
    assert.equal(manifest.schemaVersion, 3); assert.ok(manifest.axes.every(({ entries }) => entries === 1));
    assert.ok(manifest.entries.some((entry) => entry.component === "install-workspace-manifest"));
    assert.match(readFileSync(join(runtime, "resources", "install-workspace", "workspace-manifest.json"), "utf8"), /membrane-install-workspace-v1/);
    writeFileSync(join(runtime, "blueprint", "lib", "node"), "mutated");
    assert.throws(() => verifyStagedInventory({ runtimeDir: runtime }), /hash mismatch/);
    make(join(runtime, "blueprint", "lib", "node")); writeRuntimeInventory({ hubDir: root, runtimeDir: runtime, specs: specs(), target: "aarch64-apple-darwin" });
    make(join(runtime, "resources", "extra.txt")); assert.throws(() => verifyStagedInventory({ runtimeDir: runtime }), /unexpected staged/);
    assert.throws(() => runtimeInventory({ hubDir: root, specs: [...specs(), { id: "retired", axis: "pull", component: "retired", delivery: "resource", path: "orthic/crypt-service" }] }), /retired runtime asset/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("unpacked artifact requires executable bootstrap, Cortex, Blueprint & Hub probes", async () => {
  const { root, make, runtime } = fixture(); const sidecars = join(root, "sidecars");
  try {
    writeRuntimeInventory({ hubDir: root, runtimeDir: runtime, specs: specs(), target: "aarch64-apple-darwin" });
    for (const name of ["membrane", "cortex-service", "cortex"]) make(join(sidecars, name));
    assert.match(readFileSync(new URL("../scripts/runtime-inventory.mjs", import.meta.url), "utf8"), /function nativeUnpackedProbes/);
    const called = [];
    await verifyUnpackedArtifact({ runtimeDir: runtime, sidecarDir: sidecars, probes: Object.fromEntries(["bootstrapImport", "cortexService", "blueprintRecall", "hubSnapshot"].map((name) => [name, async () => { called.push(name); return true; }])) });
    assert.deepEqual(called, ["bootstrapImport", "cortexService", "blueprintRecall", "hubSnapshot"]);
    make(join(sidecars, "Membrane Hub"));
    await verifyUnpackedArtifact({ runtimeDir: runtime, sidecarDir: sidecars, probes: { bootstrapImport: () => true, cortexService: () => true, blueprintRecall: () => true, hubSnapshot: () => true } });
    make(join(sidecars, "cortex-debug"));
    await assert.rejects(() => verifyUnpackedArtifact({ runtimeDir: runtime, sidecarDir: sidecars, probes: { bootstrapImport: () => true, cortexService: () => true, blueprintRecall: () => true, hubSnapshot: () => true } }), /unexpected unpacked sidecar/);
    rmSync(join(sidecars, "cortex-debug"));
    make(join(sidecars, "crypt-service")); await assert.rejects(() => verifyUnpackedArtifact({ runtimeDir: runtime, sidecarDir: sidecars, probes: { bootstrapImport: () => true, cortexService: () => true, blueprintRecall: () => true, hubSnapshot: () => true } }), /retired unpacked sidecar/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
