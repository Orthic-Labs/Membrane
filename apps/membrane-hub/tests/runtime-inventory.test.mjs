import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { addInstalledBlueprintInventory, RUNTIME_SPECS, runtimeInventory, runtimeTarget, verifyStagedInventory, verifyUnpackedArtifact, writeRuntimeInventory } from "../scripts/runtime-inventory.mjs";

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "membrane-hub-runtime-"));
  const make = (file, text = file) => { mkdirSync(join(file, ".."), { recursive: true }); writeFileSync(file, text); };
  for (const name of ["pull", "push", "cortex", "ledger", "adapt"]) make(join(root, `${name}.txt`));
  make(join(root, "blueprint-contract.json"), '{"operation":"membrane_blueprint"}\n');
  for (const name of ["membrane-x86_64-pc-windows-msvc.exe", "cortex-x86_64-pc-windows-msvc.exe"]) make(join(root, name));
  return { root, make, runtime: join(root, "src-tauri", "runtime") };
}
function specs() {
  return [
    { id: "membrane-command", component: "membrane", delivery: "externalBin", path: "membrane-{target}.exe" },
    { id: "cortex-cli", component: "cortex", delivery: "externalBin", path: "cortex-{target}.exe" },
    { id: "cortex-contract", component: "cortex", axis: "cortex", delivery: "resource", path: "cortex.txt" },
    { id: "blueprint-contract", component: "blueprint", axis: "blueprint", delivery: "resource", transport: "named-pipe", path: "blueprint-contract.json" },
    { id: "pull-contract", component: "pull", axis: "pull", delivery: "resource", path: "pull.txt" },
    { id: "push-contract", component: "push", axis: "push", delivery: "resource", path: "push.txt" },
    { id: "ledger-contract", component: "ledger", axis: "ledger", delivery: "resource", path: "ledger.txt" },
    { id: "adapt-contract", component: "adapt", axis: "adapt", delivery: "resource", path: "adapt.txt", invocation: "daemon-native" },
  ];
}

function installBlueprint(runtime, make) {
  make(join(runtime, "blueprint", "lib", "node.exe"));
  make(join(runtime, "blueprint", "bin", "blueprint.cmd"));
  make(join(runtime, "blueprint", "app", "package", "package.json"), '{"name":"@membrane/blueprint","version":"0.2.0"}\n');
  make(join(runtime, "blueprint", "app", "package", "scripts", "blueprint.mjs"));
  make(join(runtime, "blueprint", "app", "package", "scripts", "blueprint-watch.mjs"));
  addInstalledBlueprintInventory({ runtimeDir: runtime });
}

test("runtime closure records native sidecars, installed Blueprint & six axes", () => {
  const ids = new Set(RUNTIME_SPECS.map((spec) => spec.id));
  for (const id of ["membrane-tray", "membrane-daemon", "membrane-command", "cortex-cli", "blueprint-contract", "pull-contract", "push-contract", "ledger-contract", "adapt-contract", "runtime-schemas", "hub-icons"]) assert.ok(ids.has(id), id);
  for (const retired of ["host-adapters", "install-workspace", "install-workspace-manifest"]) assert.ok(!ids.has(retired), retired);
  const blueprint = RUNTIME_SPECS.find((spec) => spec.id === "blueprint-contract");
  assert.equal(blueprint.delivery, "resource"); assert.equal(blueprint.transport, "named-pipe");
  for (const forbidden of ["cortex-store/src", "membrane-runtime/src/pull", "membrane-runtime/src/push", "../../blueprint", "../../adapt/src/adapt", "node"]) assert.ok(!RUNTIME_SPECS.some((spec) => spec.path.includes(forbidden)), forbidden);
  const tauri = readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8");
  assert.match(tauri, /"resources": \["runtime"\]/);
  assert.match(tauri, /"externalBin": \["binaries\/cortex", "binaries\/membrane"\]/);
  const windowsTauri = JSON.parse(readFileSync(new URL("../src-tauri/tauri.windows.conf.json", import.meta.url), "utf8"));
  assert.deepEqual(windowsTauri.bundle.externalBin, ["binaries/cortex", "binaries/membrane", "binaries/membrane-tray", "binaries/membrane-daemon"]);
  assert.doesNotMatch(tauri, /cortex-service/);
  const main = readFileSync(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8");
  const production = main.split("#[cfg(test)]")[0];
  assert.match(production, /DashboardConnectionState::from_stdin\(\)/);
  assert.match(production, /connection\.get\("\/hub\/snapshot"/);
  assert.match(production, /connection\.get\("\/health"/);
  assert.match(production, /startup_owned_by_tray/);
  assert.match(production, /let _ = show_dashboard\(app\.handle\(\)/);
  assert.match(production, /app\.exit\(0\)/);
  assert.doesNotMatch(production, /bundled_binary\("membrane"\)/);
  assert.doesNotMatch(production, /mod supervisor;|mod blueprint;|mod adapt_launch;/);
  assert.doesNotMatch(production, /run_hub_runtime|std::thread::spawn/);
  assert.doesNotMatch(production, /LaunchAgents|current_app_bundle|set_platform_startup/);
  assert.doesNotMatch(production, /stop_membrane_service|stop_blueprint_service/);
  assert.doesNotMatch(production, /fn (?:open_dashboard|hide_popover)/);
  assert.doesNotMatch(readFileSync(new URL("../src-tauri/Cargo.toml", import.meta.url), "utf8"), /membrane-supervisor/);
  assert.match(tauri, /"identifier": "com\.membrane\.hub"/);
  assert.doesNotMatch(production, /args\(\["cli", "hub-snapshot"\]\)/);
  assert.doesNotMatch(production, /startup\.json/);
  // Native tray owns resident sidecar launch; Hub inventory keeps only
  // on-demand command artifacts at its external-bin boundary.
  const membraneSidecar = RUNTIME_SPECS.find((spec) => spec.id === "membrane-command");
  assert.equal(membraneSidecar.delivery, "externalBin");
  assert.equal(membraneSidecar.component, "membrane");
  const probes = readFileSync(new URL("../scripts/runtime-inventory.mjs", import.meta.url), "utf8");
  assert.match(probes, /MEMBRANE_PORT/);
  assert.match(probes, /membrane_unavailable/);
  assert.match(probes, /hub_inactive/);
  assert.doesNotMatch(probes, /MEMBRANE_LIFECYCLE_STDIO|supervisor-child|\["cli", "build-info"\]/);
  assert.doesNotMatch(probes, /MEMBRANE_OWNER_PIPE/);
  assert.doesNotMatch(probes, /CORTEX_(?:PORT|API_TOKEN_FILE|INSTALLATION_ID|SERVICE_INSTANCE_ID)/);
  assert.match(probes, /WINDOWS_TARGET/);
  assert.match(probes, /blueprintInstalled/);
  assert.match(probes, /blueprint.*lib.*node\.exe/s);
  assert.doesNotMatch(probes, /externalContract|preStagedResource/);
  const frontendBuild = readFileSync(new URL("../scripts/build-frontend.mjs", import.meta.url), "utf8");
  assert.match(frontendBuild, /dist\/release-identity\.json/);
  assert.match(frontendBuild, /x86_64-pc-windows-msvc/);
  assert.match(frontendBuild, /MEMBRANE_SIDECARS_READY/);
  const runtimeBuild = readFileSync(new URL("../../../engine/crates/membrane-runtime/build.rs", import.meta.url), "utf8");
  assert.match(runtimeBuild, /cargo:rustc-env=MEMBRANE_SOURCE_COMMIT/);
  assert.match(runtimeBuild, /cargo:rustc-env=MEMBRANE_SOURCE_TREE_SHA256/);
  assert.doesNotMatch(frontendBuild, /CORTEX_SOURCE_(?:COMMIT|TREE_SHA256)/);
  const stager = readFileSync(new URL("../scripts/stage-runtime.mjs", import.meta.url), "utf8");
  assert.match(stager, /writeRuntimeInventory/);
  assert.match(stager, /blueprint.*stage-runtime\.mjs/s);
  assert.match(stager, /addInstalledBlueprintInventory/);
  assert.doesNotMatch(stager, /profile-b|external-blueprint/);
  const releaseConfig = readFileSync(new URL("../right-release.config.mjs", import.meta.url), "utf8");
  assert.doesNotMatch(releaseConfig, /src-tauri\/runtime\/\*\*/);
  assert.match(releaseConfig, /\.\.\/\.\.\/blueprint\/scripts\/\*\*/);
});

test("runtime inventory accepts Windows x64 & macOS arm64 targets, rejecting mismatches", () => {
  assert.equal(runtimeTarget("x86_64-pc-windows-msvc"), "x86_64-pc-windows-msvc");
  assert.equal(runtimeTarget("aarch64-apple-darwin"), "aarch64-apple-darwin");
  assert.throws(() => runtimeTarget("x86_64-unknown-linux-gnu"), /unsupported runtime target/);

  const { root, make } = fixture();
  try {
    make(join(root, "membrane-aarch64-apple-darwin"));
    make(join(root, "cortex-aarch64-apple-darwin"));
    const macSpecs = specs().filter((spec) => spec.delivery !== "externalBin").concat([
      { id: "membrane-command", component: "membrane", delivery: "externalBin", path: "membrane-{target}" },
      { id: "cortex-cli", component: "cortex", delivery: "externalBin", path: "cortex-{target}" },
    ]);
    const inventory = runtimeInventory({ hubDir: root, target: "aarch64-apple-darwin", specs: macSpecs });
    assert.equal(inventory.target, "aarch64-apple-darwin");
    assert.deepEqual(inventory.entries.filter((entry) => entry.delivery === "externalBin").map((entry) => entry.installerPath), ["cortex", "membrane"]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("runtime inventory hashes native runtime & rejects missing/extra/retired", () => {
  const { root, make, runtime } = fixture();
  try {
    writeRuntimeInventory({ hubDir: root, runtimeDir: runtime, specs: specs(), target: "x86_64-pc-windows-msvc" });
    installBlueprint(runtime, make);
    const manifest = verifyStagedInventory({ runtimeDir: runtime });
    assert.equal(manifest.schemaVersion, 3); assert.ok(manifest.axes.every(({ entries }) => entries === 1));
    assert.equal(manifest.components.blueprint.version, "0.2.0"); assert.match(manifest.components.blueprint.treeSha256, /^[a-f0-9]{64}$/); assert.ok(manifest.components.blueprint.fileCount > 0);
    assert.ok(!manifest.entries.some((entry) => entry.source.endsWith(".py") || entry.source.includes("mcp/host")));
    writeFileSync(join(runtime, "resources", "blueprint-contract", "blueprint-contract.json"), "mutated");
    assert.throws(() => verifyStagedInventory({ runtimeDir: runtime }), /hash mismatch/);
    make(join(runtime, "resources", "blueprint-contract", "blueprint-contract.json")); rmSync(join(runtime, "blueprint"), { recursive: true, force: true }); writeRuntimeInventory({ hubDir: root, runtimeDir: runtime, specs: specs(), target: "x86_64-pc-windows-msvc" }); installBlueprint(runtime, make);
    make(join(runtime, "resources", "extra.txt")); assert.throws(() => verifyStagedInventory({ runtimeDir: runtime }), /unexpected staged/);
    assert.throws(() => runtimeInventory({ hubDir: root, specs: [...specs(), { id: "retired", axis: "pull", component: "retired", delivery: "resource", path: "orthic/crypt-service" }] }), /retired runtime asset/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("unpacked artifact requires native bootstrap, on-demand dashboard, installed Blueprint & Hub-off probes", async () => {
  const { root, make, runtime } = fixture(); const sidecars = join(root, "sidecars");
  try {
    writeRuntimeInventory({ hubDir: root, runtimeDir: runtime, specs: specs(), target: "x86_64-pc-windows-msvc" });
    installBlueprint(runtime, make);
    make(join(sidecars, "membrane.exe"), readFileSync(join(root, "membrane-x86_64-pc-windows-msvc.exe")));
    make(join(sidecars, "cortex.exe"), readFileSync(join(root, "cortex-x86_64-pc-windows-msvc.exe")));
    assert.match(readFileSync(new URL("../scripts/runtime-inventory.mjs", import.meta.url), "utf8"), /function nativeUnpackedProbes/);
    const called = [];
    await verifyUnpackedArtifact({ runtimeDir: runtime, sidecarDir: sidecars, probes: Object.fromEntries(["nativeBootstrap", "dashboardOnDemand", "blueprintInstalled", "hubInactive"].map((name) => [name, async () => { called.push(name); return true; }])) });
    assert.deepEqual(called, ["nativeBootstrap", "dashboardOnDemand", "blueprintInstalled", "hubInactive"]);
    make(join(sidecars, "Membrane Hub"));
    await assert.rejects(() => verifyUnpackedArtifact({ runtimeDir: runtime, sidecarDir: sidecars, probes: { nativeBootstrap: () => true, dashboardOnDemand: () => true, blueprintInstalled: () => true, hubInactive: () => true } }), /unexpected unpacked sidecar/);
    rmSync(join(sidecars, "Membrane Hub"));
    make(join(sidecars, "cortex.exe-debug"));
    await assert.rejects(() => verifyUnpackedArtifact({ runtimeDir: runtime, sidecarDir: sidecars, probes: { nativeBootstrap: () => true, dashboardOnDemand: () => true, blueprintInstalled: () => true, hubInactive: () => true } }), /unexpected unpacked sidecar/);
    rmSync(join(sidecars, "cortex.exe-debug"));
    make(join(sidecars, "crypt-service")); await assert.rejects(() => verifyUnpackedArtifact({ runtimeDir: runtime, sidecarDir: sidecars, probes: { nativeBootstrap: () => true, dashboardOnDemand: () => true, blueprintInstalled: () => true, hubInactive: () => true } }), /retired unpacked sidecar/);
    rmSync(join(sidecars, "crypt-service"));
    writeFileSync(join(sidecars, "membrane.exe"), "tampered");
    await assert.rejects(() => verifyUnpackedArtifact({ runtimeDir: runtime, sidecarDir: sidecars, probes: { nativeBootstrap: () => true, dashboardOnDemand: () => true, blueprintInstalled: () => true, hubInactive: () => true } }), /sidecar hash mismatch/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("runtime inventory validates every digest & external source binding", () => {
  const { root, make, runtime } = fixture();
  try {
    writeRuntimeInventory({ hubDir: root, runtimeDir: runtime, specs: specs(), target: "x86_64-pc-windows-msvc" });
    installBlueprint(runtime, make);
    const manifestPath = join(runtime, "runtime-inventory.json");
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    manifest.entries.find((entry) => entry.delivery === "externalBin").sha256 = "BAD";
    writeFileSync(manifestPath, `${JSON.stringify(manifest)}\n`);
    assert.throws(() => verifyStagedInventory({ runtimeDir: runtime }), /lowercase SHA-256/);

    rmSync(join(runtime, "blueprint"), { recursive: true, force: true });
    writeRuntimeInventory({ hubDir: root, runtimeDir: runtime, specs: specs(), target: "x86_64-pc-windows-msvc" });
    installBlueprint(runtime, make);
    make(join(root, "membrane-x86_64-pc-windows-msvc.exe"), "changed at source");
    assert.throws(() => verifyStagedInventory({ runtimeDir: runtime, sourceRoot: root }), /external source hash mismatch/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test("runtime inventory rejects delivery and ownership tuple tampering", () => {
  const { root, make, runtime } = fixture();
  try {
    writeRuntimeInventory({ hubDir: root, runtimeDir: runtime, specs: specs(), target: "x86_64-pc-windows-msvc" });
    installBlueprint(runtime, make);
    const manifestPath = join(runtime, "runtime-inventory.json");
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    const resource = manifest.entries.find((entry) => entry.component === "pull-contract");
    resource.delivery = "tauriBundle";
    writeFileSync(manifestPath, JSON.stringify(manifest) + "\n");
    assert.throws(() => verifyStagedInventory({ runtimeDir: runtime }), /delivery invalid/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
