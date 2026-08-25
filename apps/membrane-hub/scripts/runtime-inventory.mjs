// Membrane Hub packages one closed runtime. Blueprint is pre-staged by its
// release builder; native capabilities remain Tauri sidecars, never sources.
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, relative, resolve } from "node:path";
import { createServer } from "node:net";
import { fileURLToPath } from "node:url";

const hub = fileURLToPath(new URL("../", import.meta.url));
const runtime = join(hub, "src-tauri", "runtime");
const axes = ["pull", "push", "cortex", "blueprint", "ledger", "adapt"];
const composition = ["membrane", "cortex", "blueprint", "ledger", "pull", "push", "adapt"];
const retired = /(?:^|[\\/])(crypt(?:-service)?|orthic(?:[_-]manifest)?|product-addons?)(?:[\\/]|$)/i;
const ignored = /(?:^|[\\/])(?:\.git|node_modules|__pycache__|tests?|\.pytest_cache)(?:[\\/]|$)/;
const digest = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");
const MAC_TARGET = "aarch64-apple-darwin";

// `preStagedResource` is emitted by Blueprint's own tested release stager.
// `externalBin` & `tauriBundle` record ownership without copying source or
// duplicating sidecars/icons into Tauri's resource tree.
export const RUNTIME_SPECS = [
  { id: "membrane-command", component: "membrane", delivery: "externalBin", path: "src-tauri/binaries/membrane-{target}" },
  { id: "cortex-cli", component: "cortex", delivery: "externalBin", path: "src-tauri/binaries/cortex-{target}" },
  { id: "blueprint-runtime", component: "blueprint", axis: "blueprint", delivery: "preStagedResource", path: "src-tauri/runtime/blueprint", tree: true },
  { id: "pull-contract", component: "pull", axis: "pull", delivery: "resource", path: "../../schemas/operations/membrane-context.v1.schema.json" },
  { id: "push-contract", component: "push", axis: "push", delivery: "resource", path: "../../schemas/compression-receipt.v1.schema.json" },
  { id: "cortex-contract", component: "cortex", axis: "cortex", delivery: "resource", path: "../../schemas/memory-lifecycle.v1.schema.json" },
  { id: "ledger-contract", component: "ledger", axis: "ledger", delivery: "resource", path: "../../schemas/operations/membrane-source-read.v1.schema.json" },
  // N5 cutover: Hub owns the native Adapt launch seam - scheduled cycles and
  // installed launcher execs bundled binary's `membrane adapt` CLI.
  { id: "adapt-contract", component: "adapt", axis: "adapt", delivery: "resource", path: "../../schemas/operations/membrane-feedback.v1.schema.json", invocation: "hub-native" },
  { id: "runtime-schemas", component: "membrane-schemas", delivery: "resource", path: "../../schemas", tree: true, extensions: [".json", ".yaml", ".yml"] },
  { id: "host-adapters", component: "host-adapters", delivery: "resource", path: "../../mcp/host", tree: true, extensions: [".cjs", ".mjs", ".json"], invocation: "host-bound" },
  { id: "install-workspace", component: "install-workspace", delivery: "resource", path: "../../dist/install/workspace", tree: true, extensions: [".py"] },
  { id: "install-workspace-manifest", component: "install-workspace-manifest", delivery: "resource", path: "../../dist/install/workspace-manifest.json", stageRoot: "resources/install-workspace" },
  { id: "license-membrane", component: "license", delivery: "resource", path: "../../LICENSE" },
  { id: "hub-icons", component: "icons", delivery: "tauriBundle", path: "src-tauri/icons", tree: true },
];

function targetFor(value = process.env.TAURI_ENV_TARGET_TRIPLE) { if (value && value !== MAC_TARGET) throw new Error(`Mac target required: ${MAC_TARGET}`); return MAC_TARGET; }
function concretePath(source, target) { return source.replace("{target}", target); }
function filesAt(root, extensions) {
  if (!existsSync(root)) throw new Error(`runtime source missing: ${root}`);
  const files = statSync(root).isFile() ? [root] : readdirSync(root, { recursive: true }).map((name) => join(root, name)).filter((file) => lstatSync(file).isFile());
  return files.filter((file) => !ignored.test(file)).filter((file) => !extensions || extensions.some((extension) => file.endsWith(extension))).sort((left, right) => left.localeCompare(right));
}
function stagePath(spec, source, sourceRoot, target) {
  const local = statSync(sourceRoot).isFile() ? basename(source) : relative(sourceRoot, source);
  if (spec.delivery === "externalBin") return `external-bin/${spec.component}`;
  if (spec.delivery === "preStagedResource") return `blueprint/${local}`.replaceAll("\\", "/");
  if (spec.delivery === "tauriBundle") return `tauri-assets/${spec.id}/${local}`.replaceAll("\\", "/");
  return `${spec.stageRoot ?? `resources/${spec.id}`}/${local}`.replaceAll("\\", "/");
}

export function runtimeInventory({ hubDir = hub, target = targetFor(), specs = RUNTIME_SPECS } = {}) {
  const seen = new Set(); const entries = [];
  for (const spec of specs) {
    if (seen.has(spec.id)) throw new Error(`duplicate runtime component: ${spec.id}`);
    seen.add(spec.id);
    const sourceRoot = resolve(hubDir, concretePath(spec.path, target));
    if (retired.test(relative(hubDir, sourceRoot))) throw new Error(`retired runtime asset rejected: ${spec.path}`);
    for (const source of filesAt(sourceRoot, spec.extensions)) {
      const staged = stagePath(spec, source, sourceRoot, target);
      if (retired.test(staged)) throw new Error(`retired staged runtime asset rejected: ${staged}`);
      entries.push({ component: spec.id, ...(spec.axis ? { axis: spec.axis } : {}), ...(spec.invocation ? { invocation: spec.invocation } : {}), delivery: spec.delivery, source: relative(hubDir, source).replaceAll("\\", "/"), stagePath: staged, installerPath: spec.delivery === "externalBin" ? `sidecars/${spec.component}` : staged, sha256: digest(source) });
    }
  }
  entries.sort((left, right) => left.stagePath.localeCompare(right.stagePath));
  if (new Set(entries.map((entry) => entry.stagePath)).size !== entries.length) throw new Error("duplicate staged runtime asset");
  // A tree may contribute many files; an axis is owned once by its component.
  const axisEntries = axes.map((axis) => ({ axis, entries: new Set(entries.filter((entry) => entry.axis === axis).map((entry) => entry.component)).size }));
  if (axisEntries.some(({ entries }) => entries !== 1)) throw new Error(`six-axis runtime ownership ambiguous: ${axisEntries.map(({ axis, entries }) => `${axis}=${entries}`).join(",")}`);
  return { schemaVersion: 3, app: "membrane-hub", target, axes: axisEntries, composition, entries };
}

function installWorkspaceDigest(files, runtimeRequirements) {
  return createHash("sha256").update(JSON.stringify({ schemaVersion: "membrane-install-workspace-v1", packageVersion: "1.0.0", files, runtime: runtimeRequirements })).digest("hex");
}
function verifyInstallWorkspaceManifest(inventory, runtimeDir) {
  const manifestEntry = inventory.entries.find((entry) => entry.component === "install-workspace-manifest");
  if (!manifestEntry) throw new Error("install workspace manifest missing");
  const listed = JSON.parse(readFileSync(join(runtimeDir, manifestEntry.stagePath), "utf8"));
  if (listed.schemaVersion !== "membrane-install-workspace-v1" || listed.packageVersion !== "1.0.0" || listed.source !== "membrane/install/workspace" || listed.generated !== "source-to-dist") throw new Error("install workspace manifest invalid");
  if (listed.runtime?.python !== ">=3.11" || !Array.isArray(listed.runtime.dependencies) || !Array.isArray(listed.files)) throw new Error("install workspace runtime requirements invalid");
  const expected = new Map(inventory.entries.filter((entry) => entry.component === "install-workspace").map((entry) => [entry.stagePath.replace("resources/install-workspace/", ""), entry]));
  if (expected.size !== listed.files.length) throw new Error("install workspace manifest file count mismatch");
  for (const member of listed.files) {
    const entry = expected.get(member.path); const staged = entry && join(runtimeDir, entry.stagePath);
    if (!entry || member.sha256 !== entry.sha256 || member.bytes !== statSync(staged).size) throw new Error(`install workspace manifest mismatch: ${member.path}`);
  }
  if (listed.packageSha256 !== installWorkspaceDigest(listed.files, listed.runtime)) throw new Error("install workspace manifest digest mismatch");
}

export function verifyStagedInventory({ runtimeDir = runtime } = {}) {
  const manifestPath = join(runtimeDir, "runtime-inventory.json");
  if (!existsSync(manifestPath)) throw new Error("runtime inventory missing");
  const inventory = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (inventory.schemaVersion !== 3 || inventory.app !== "membrane-hub") throw new Error("runtime inventory schema invalid");
  const expected = new Set(["runtime-inventory.json"]);
  for (const entry of inventory.entries) {
    if (entry.delivery === "externalBin" || entry.delivery === "tauriBundle") continue;
    expected.add(entry.stagePath);
    const staged = join(runtimeDir, entry.stagePath);
    if (!existsSync(staged) || digest(staged) !== entry.sha256) throw new Error(`runtime staged hash mismatch: ${entry.stagePath}`);
  }
  verifyInstallWorkspaceManifest(inventory, runtimeDir);
  for (const file of filesAt(runtimeDir)) {
    const path = relative(runtimeDir, file).replaceAll("\\", "/");
    if (!expected.has(path)) throw new Error(`unexpected staged runtime asset: ${path}`);
    if (retired.test(path)) throw new Error(`retired staged runtime asset rejected: ${path}`);
  }
  return inventory;
}

function run(command, args, { cwd, env, timeout = 12_000 } = {}) {
  return new Promise((resolveRun, rejectRun) => {
    const child = spawn(command, args, { cwd, env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "", stderr = "";
    child.stdout.on("data", (chunk) => { stdout = `${stdout}${chunk}`.slice(-16_384); });
    child.stderr.on("data", (chunk) => { stderr = `${stderr}${chunk}`.slice(-16_384); });
    const timer = setTimeout(() => { child.kill(); rejectRun(new Error(`probe timeout: ${basename(command)}`)); }, timeout);
    child.once("error", (error) => { clearTimeout(timer); rejectRun(error); });
    child.once("close", (status) => { clearTimeout(timer); status === 0 ? resolveRun({ stdout, stderr }) : rejectRun(new Error(`probe failed: ${basename(command)} (${status}): ${stderr || stdout}`)); });
  });
}
function freePort() {
  return new Promise((resolvePort, rejectPort) => {
    const server = createServer(); server.once("error", rejectPort);
    server.listen(0, "127.0.0.1", () => { const port = server.address().port; server.close((error) => error ? rejectPort(error) : resolvePort(port)); });
  });
}
function expectHubInactive(sidecarDir) {
  return new Promise(async (resolveProbe, rejectProbe) => {
    const port = await freePort();
    const membrane = join(sidecarDir, "membrane");
    const child = spawn(membrane, ["cli", "hub-snapshot"], { env: { ...process.env, MEMBRANE_PORT: String(port) }, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "", stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.once("error", rejectProbe);
    child.once("close", (status) => {
      try {
        if (status === 0) throw new Error("Hub-inactive client unexpectedly succeeded");
        const payload = JSON.parse(stdout);
        if (payload.kind !== "membrane_unavailable" || payload.reason !== "hub_inactive" || payload.retryable !== true) throw new Error("Hub-inactive response invalid");
        if (!stderr.includes("hub inactive")) throw new Error("Hub-inactive exit reason missing");
        resolveProbe(true);
      } catch (error) { rejectProbe(error); }
    });
  });
}
function nativeUnpackedProbes() {
  return {
    async bootstrapImport({ runtimeDir }) {
      const init = join(runtimeDir, "resources", "install-workspace", "__init__.py");
      const command = "python3";
      const args = ["-I", "-c"];
      args.push("import importlib.util,pathlib,sys; p=pathlib.Path(sys.argv[1]); s=importlib.util.spec_from_file_location('membrane_install_workspace',p,submodule_search_locations=[str(p.parent)]); m=importlib.util.module_from_spec(s); sys.modules[s.name]=m; s.loader.exec_module(m)", init);
      await run(command, args); return true;
    },
    async hubRuntimeInProcess() {
      const supervisor = readFileSync(join(hub, "src-tauri", "src", "supervisor.rs"), "utf8");
      const retiredChildMode = ["supervisor", "child"].join("-");
      if (!supervisor.includes("run_hub_runtime") || supervisor.includes("std::process::Command") || supervisor.includes(retiredChildMode)) throw new Error("Hub runtime topology invalid");
      return true;
    },
    async blueprintRecall({ runtimeDir }) {
      const root = mkdtempSync(join(tmpdir(), "membrane-hub-blueprint-probe-")); writeFileSync(join(root, "probe.mjs"), "export const membraneHubProbe = true;\n");
      const launcher = join(runtimeDir, "blueprint", "bin", "blueprint");
      try { await run(launcher, ["build", "--root", root, "--out", ".agent"], { cwd: root, env: { BLUEPRINT_NO_UPDATE_CHECK: "1" }, timeout: 30_000 }); await run(launcher, ["recall", "--root", root, "--out", ".agent", "--query", "membrane hub probe"], { cwd: root, env: { BLUEPRINT_NO_UPDATE_CHECK: "1" }, timeout: 15_000 }); return true; }
      finally { rmSync(root, { recursive: true, force: true }); }
    },
    async hubInactive({ sidecarDir }) { return expectHubInactive(sidecarDir); },
  };
}

function verifySidecars(sidecarDir, inventory) {
  const expected = new Set(inventory.entries.filter((entry) => entry.delivery === "externalBin").map((entry) => basename(entry.installerPath)));
  for (const name of expected) {
    const file = join(sidecarDir, name);
    if (!existsSync(file) || !statSync(file).isFile()) throw new Error(`unpacked sidecar missing: ${name}`);
  }
  for (const name of readdirSync(sidecarDir)) {
    if (retired.test(name)) throw new Error(`retired unpacked sidecar rejected: ${name}`);
    if (!expected.has(name) && [...expected].some((family) => name === family || name.startsWith(`${family}-`))) {
      throw new Error(`unexpected unpacked sidecar: ${name}`);
    }
  }
}

// Native defaults exercise packaged bytes. Injected probes remain available for
// deterministic tests or product-specific artifact harnesses.
export async function verifyUnpackedArtifact({ runtimeDir = runtime, sidecarDir, probes = {} } = {}) {
  const inventory = verifyStagedInventory({ runtimeDir });
  if (JSON.stringify(inventory.composition) !== JSON.stringify(composition)) throw new Error("runtime composition invalid");
  if (inventory.axes?.length !== axes.length || inventory.axes.some(({ axis, entries }) => !axes.includes(axis) || !Number.isInteger(entries) || entries < 1)) throw new Error("six-axis unpacked runtime invalid");
  if (!inventory.entries.some((entry) => entry.component === "blueprint-runtime" && entry.delivery === "preStagedResource")) throw new Error("packaged Blueprint runtime missing");
  if (!inventory.entries.some((entry) => entry.component === "adapt-contract" && entry.invocation === "hub-native")) throw new Error("Adapt invocation seam invalid");
  if (!sidecarDir) throw new Error("unpacked sidecar directory required");
  verifySidecars(sidecarDir, inventory);
  const activeProbes = Object.keys(probes).length ? probes : nativeUnpackedProbes();
  try {
    for (const name of ["bootstrapImport", "hubRuntimeInProcess", "blueprintRecall", "hubInactive"]) {
      if (typeof activeProbes[name] !== "function") throw new Error(`unpacked executable probe required: ${name}`);
      if (!(await activeProbes[name]({ runtimeDir, sidecarDir, inventory }))) throw new Error(`unpacked executable probe failed: ${name}`);
    }
    return inventory;
  } finally { await activeProbes.cleanup?.(); }
}

export function writeRuntimeInventory({ hubDir = hub, runtimeDir = runtime, target, specs } = {}) {
  const inventory = runtimeInventory({ hubDir, target, specs }); const resources = join(runtimeDir, "resources");
  rmSync(resources, { recursive: true, force: true }); mkdirSync(resources, { recursive: true });
  for (const entry of inventory.entries.filter((entry) => entry.delivery === "resource")) {
    const destination = join(runtimeDir, entry.stagePath); mkdirSync(dirname(destination), { recursive: true }); copyFileSync(resolve(hubDir, entry.source), destination);
  }
  writeFileSync(join(runtimeDir, "runtime-inventory.json"), `${JSON.stringify(inventory, null, 2)}\n`); verifyStagedInventory({ runtimeDir }); return inventory;
}

if (fileURLToPath(import.meta.url) === resolve(process.argv[1] || "")) {
  const action = process.argv[2] || "check";
  if (action === "write") writeRuntimeInventory(); else if (action === "check") runtimeInventory(); else if (action === "verify") verifyUnpackedArtifact().catch((error) => { console.error(error.message); process.exitCode = 1; }); else throw new Error("usage: runtime-inventory.mjs <check|write|verify>");
}
