// Membrane Hub packages native Windows sidecars, typed subsystem contracts, &
// installed Blueprint runtime. Blueprint remains its own subsystem boundary;
// installer makes Blueprint available without separate provisioning.
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { copyFileSync, existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
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
const SHA256_RE = /^[0-9a-f]{64}$/;
function assertInventorySha256(value, label) {
  if (typeof value !== "string" || !SHA256_RE.test(value)) throw new Error(`${label} must be a lowercase SHA-256 digest`);
  return value;
}
function assertNoSymlinks(root, label) {
  const rootStat = lstatSync(root);
  if (rootStat.isSymbolicLink()) throw new Error(label + " may not be a symlink: " + root);
  const pending = [root];
  while (pending.length) {
    const directory = pending.pop();
    if (!lstatSync(directory).isDirectory()) continue;
    for (const name of readdirSync(directory)) {
      const path = join(directory, name);
      const stat = lstatSync(path);
      if (stat.isSymbolicLink()) throw new Error(label + " contains a symlink: " + path);
      if (stat.isDirectory()) pending.push(path);
    }
  }
}
function assertRegularFile(file, label) {
  let stat;
  try { stat = lstatSync(file); } catch { throw new Error(label + " is missing: " + file); }
  if (stat.isSymbolicLink() || !stat.isFile()) throw new Error(label + " is not a regular file: " + file);
  return file;
}
function treeDigest(root) {
  assertNoSymlinks(root, "runtime tree");
  const hash = createHash("sha256");
  const files = filesAt(root, undefined, { includeIgnored: true });
  for (const file of files) {
    hash.update(`${relative(root, file).replaceAll("\\", "/")}\0`).update(readFileSync(file));
  }
  return { sha256: hash.digest("hex"), fileCount: files.length };
}
const WINDOWS_TARGET = "x86_64-pc-windows-msvc";
const EXTERNAL_BINARIES = new Map([
  ["membrane-command", "membrane"],
  ["cortex-cli", "cortex"],
  ["membrane-tray", "membrane-tray"],
  ["membrane-daemon", "membrane-daemon"],
]);

function externalBinaryName(entry) {
  const name = EXTERNAL_BINARIES.get(entry.component);
  if (!name) throw new Error(`runtime external sidecar component invalid: ${entry.component}`);
  return name;
}

// `installedComponent` records files already staged under Tauri runtime.
// `externalBin` & `tauriBundle` record ownership without
// copying source or duplicating sidecars/icons into Tauri's resource tree.
export const RUNTIME_SPECS = [
  { id: "membrane-tray", component: "membrane-tray", delivery: "externalBin", path: "src-tauri/binaries/membrane-tray-{target}.exe" },
  { id: "membrane-daemon", component: "membrane-daemon", delivery: "externalBin", path: "src-tauri/binaries/membrane-daemon-{target}.exe" },
  { id: "membrane-command", component: "membrane", delivery: "externalBin", path: "src-tauri/binaries/membrane-{target}.exe" },
  { id: "cortex-cli", component: "cortex", delivery: "externalBin", path: "src-tauri/binaries/cortex-{target}.exe" },
  { id: "pull-contract", component: "pull", axis: "pull", delivery: "resource", path: "../../schemas/operations/membrane-context.v1.schema.json" },
  { id: "push-contract", component: "push", axis: "push", delivery: "resource", path: "../../schemas/compression-receipt.v1.schema.json" },
  { id: "cortex-contract", component: "cortex", axis: "cortex", delivery: "resource", path: "../../schemas/memory-lifecycle.v1.schema.json" },
  { id: "blueprint-contract", component: "blueprint", axis: "blueprint", delivery: "resource", transport: "named-pipe", path: "../../schemas/operations/membrane-blueprint.v1.schema.json" },
  { id: "ledger-contract", component: "ledger", axis: "ledger", delivery: "resource", path: "../../schemas/operations/membrane-source-read.v1.schema.json" },
  // Architecture B: resident Adapt cycles execute inside tray-owned daemon.
  // On-demand dashboard only projects authenticated daemon state.
  { id: "adapt-contract", component: "adapt", axis: "adapt", delivery: "resource", path: "../../schemas/operations/membrane-feedback.v1.schema.json", invocation: "daemon-native" },
  { id: "runtime-schemas", component: "membrane-schemas", delivery: "resource", path: "../../schemas", tree: true, extensions: [".json", ".yaml", ".yml"] },
  { id: "license-membrane", component: "license", delivery: "resource", path: "../../LICENSE" },
  { id: "hub-icons", component: "icons", delivery: "tauriBundle", path: "src-tauri/icons", tree: true },
];

function targetFor(value = process.env.TAURI_ENV_TARGET_TRIPLE) { if (value && value !== WINDOWS_TARGET) throw new Error(`Windows target required: ${WINDOWS_TARGET}`); return WINDOWS_TARGET; }
function concretePath(source, target) { return source.replace("{target}", target); }
function filesAt(root, extensions, { includeIgnored = false } = {}) {
  if (!existsSync(root)) throw new Error(`runtime source missing: ${root}`);
  const files = statSync(root).isFile() ? [root] : readdirSync(root, { recursive: true }).map((name) => join(root, name)).filter((file) => lstatSync(file).isFile());
  return files.filter((file) => includeIgnored || !ignored.test(file)).filter((file) => !extensions || extensions.some((extension) => file.endsWith(extension))).sort((left, right) => left.localeCompare(right));
}
function stagePath(spec, source, sourceRoot, target) {
  const local = statSync(sourceRoot).isFile() ? basename(source) : relative(sourceRoot, source);
  if (spec.delivery === "externalBin") return `external-bin/${spec.component}`;
  if (spec.delivery === "tauriBundle") return `tauri-assets/${spec.id}/${local}`.replaceAll("\\", "/");
  return `${spec.stageRoot ?? `resources/${spec.id}`}/${local}`.replaceAll("\\", "/");
}

export function runtimeInventory({ hubDir = hub, target, specs = RUNTIME_SPECS } = {}) {
  const runtimeTarget = targetFor(target);
  const seen = new Set(); const entries = [];
  for (const spec of specs) {
    if (seen.has(spec.id)) throw new Error(`duplicate runtime component: ${spec.id}`);
    seen.add(spec.id);
    const sourceRoot = resolve(hubDir, concretePath(spec.path, runtimeTarget));
    if (retired.test(relative(hubDir, sourceRoot))) throw new Error(`retired runtime asset rejected: ${spec.path}`);
    for (const source of filesAt(sourceRoot, spec.extensions)) {
      const staged = stagePath(spec, source, sourceRoot, runtimeTarget);
      if (retired.test(staged)) throw new Error(`retired staged runtime asset rejected: ${staged}`);
      entries.push({ component: spec.id, ...(spec.axis ? { axis: spec.axis } : {}), ...(spec.invocation ? { invocation: spec.invocation } : {}), ...(spec.profile ? { profile: spec.profile } : {}), ...(spec.transport ? { transport: spec.transport } : {}), delivery: spec.delivery, source: relative(hubDir, source).replaceAll("\\", "/"), stagePath: staged, installerPath: spec.delivery === "externalBin" ? `${spec.component}.exe` : staged, sha256: digest(source) });
    }
  }
  entries.sort((left, right) => left.stagePath.localeCompare(right.stagePath));
  if (new Set(entries.map((entry) => entry.stagePath)).size !== entries.length) throw new Error("duplicate staged runtime asset");
  // A tree may contribute many files; an axis is owned once by its component.
  const axisEntries = axes.map((axis) => ({ axis, entries: new Set(entries.filter((entry) => entry.axis === axis).map((entry) => entry.component)).size }));
  if (axisEntries.some(({ entries }) => entries !== 1)) throw new Error(`six-axis runtime ownership ambiguous: ${axisEntries.map(({ axis, entries }) => `${axis}=${entries}`).join(",")}`);
  return { schemaVersion: 3, app: "membrane-hub", target: runtimeTarget, axes: axisEntries, composition, entries };
}

export function verifyStagedInventory({ runtimeDir = runtime, sourceRoot } = {}) {
  const manifestPath = join(runtimeDir, "runtime-inventory.json");
  if (!existsSync(manifestPath)) throw new Error("runtime inventory missing");
  assertNoSymlinks(runtimeDir, "runtime directory");
  const inventory = JSON.parse(readFileSync(manifestPath, "utf8"));
  if (inventory.schemaVersion !== 3 || inventory.app !== "membrane-hub") throw new Error("runtime inventory schema invalid");
  const blueprint = inventory.components?.blueprint;
  if (blueprint) {
    const blueprintRoot = join(runtimeDir, "blueprint");
    const packagePath = join(blueprintRoot, "app", "package", "package.json");
    if (!existsSync(packagePath)) throw new Error("installed Blueprint package manifest missing");
    const packageJson = JSON.parse(readFileSync(packagePath, "utf8"));
    const tree = treeDigest(blueprintRoot);
    if (typeof blueprint.version !== "string" || blueprint.version !== packageJson.version) throw new Error("installed Blueprint version metadata mismatch");
    if (blueprint.treeSha256 !== tree.sha256 || blueprint.fileCount !== tree.fileCount) throw new Error("installed Blueprint tree digest mismatch");
  }
  if (!Array.isArray(inventory.entries) || inventory.entries.length === 0) throw new Error("runtime inventory entries missing");
  const expected = new Set(["runtime-inventory.json"]);
  const seen = new Set();
  for (const entry of inventory.entries) {
    if (!entry || typeof entry !== "object") throw new Error("runtime inventory entry invalid");
    if (typeof entry.component !== "string" || typeof entry.source !== "string" || typeof entry.installerPath !== "string") throw new Error("runtime inventory entry fields invalid");
    const spec = RUNTIME_SPECS.find((candidate) => candidate.id === entry.component);
    if (!spec && entry.component !== "blueprint-runtime") throw new Error("runtime inventory component invalid: " + entry.component);
    const expectedDelivery = spec?.delivery ?? "installedComponent";
    if (entry.delivery !== expectedDelivery) throw new Error("runtime inventory delivery invalid: " + entry.component);
    if (spec?.axis ? entry.axis !== spec.axis : entry.axis !== undefined) throw new Error("runtime inventory axis invalid: " + entry.component);
    if (spec?.transport ? entry.transport !== spec.transport : entry.transport !== undefined) throw new Error("runtime inventory transport invalid: " + entry.component);
    if (spec?.invocation ? entry.invocation !== spec.invocation : entry.invocation !== undefined) throw new Error("runtime inventory invocation invalid: " + entry.component);
    assertInventorySha256(entry.sha256, `runtime inventory ${entry.stagePath ?? entry.component} sha256`);
    if (typeof entry.stagePath !== "string" || !entry.stagePath || entry.stagePath.startsWith("/") || /^[A-Za-z]:[\\/]/.test(entry.stagePath)) throw new Error(`runtime staged path invalid: ${entry.stagePath}`);
    const staged = resolve(runtimeDir, entry.stagePath);
    const relativePath = relative(runtimeDir, staged).replaceAll("\\", "/");
    if (!relativePath || relativePath === ".." || relativePath.startsWith("../")) throw new Error(`runtime staged path escapes runtime: ${entry.stagePath}`);
    if (seen.has(relativePath)) throw new Error(`duplicate runtime staged path: ${relativePath}`);
    seen.add(relativePath); expected.add(relativePath);
    if (entry.delivery === "externalBin") {
      const binary = externalBinaryName(entry);
      if (entry.stagePath !== `external-bin/${binary}` || entry.installerPath !== `${binary}.exe`) throw new Error(`runtime external sidecar mapping invalid: ${entry.component}`);
      const sourceName = entry.source?.replaceAll("\\", "/").split("/").pop();
      if (sourceName !== `${binary}-${inventory.target}.exe`) throw new Error(`runtime external source mapping invalid: ${entry.source}`);
    }
    if (entry.delivery === "resource") {
      if (entry.installerPath !== entry.stagePath || !entry.stagePath.startsWith("resources/" + entry.component + "/")) throw new Error("runtime resource mapping invalid: " + entry.component);
    } else if (entry.delivery === "tauriBundle") {
      if (entry.installerPath !== entry.stagePath || entry.component !== "hub-icons" || !entry.stagePath.startsWith("tauri-assets/hub-icons/")) throw new Error("runtime bundle mapping invalid: " + entry.component);
    } else if (entry.delivery === "installedComponent") {
      if (entry.component !== "blueprint-runtime" || entry.source !== entry.stagePath || entry.installerPath !== entry.stagePath || !entry.stagePath.startsWith("blueprint/")) throw new Error("runtime installed component mapping invalid: " + entry.component);
    }
    if (sourceRoot && entry.delivery === "externalBin") {
      if (typeof entry.source !== "string" || !entry.source || entry.source.startsWith("/") || /^[A-Za-z]:[\\/]/.test(entry.source)) throw new Error(`runtime source path invalid: ${entry.source}`);
      const source = resolve(sourceRoot, entry.source);
      const sourceRelative = relative(sourceRoot, source).replaceAll("\\", "/");
      if (!sourceRelative || sourceRelative === ".." || sourceRelative.startsWith("../")) throw new Error(`runtime source path escapes source root: ${entry.source}`);
      if (digest(assertRegularFile(source, "runtime external source")) !== entry.sha256) throw new Error("runtime external source hash mismatch: " + entry.source);
    }
    if (entry.delivery === "externalBin" || entry.delivery === "tauriBundle") continue;
    if (digest(assertRegularFile(staged, "runtime staged asset")) !== entry.sha256) throw new Error("runtime staged hash mismatch: " + entry.stagePath);
  }
  for (const file of filesAt(runtimeDir, undefined, { includeIgnored: true })) {
    const path = relative(runtimeDir, file).replaceAll("\\", "/");
    if (!expected.has(path)) throw new Error(`unexpected staged runtime asset: ${path}`);
    if (retired.test(path)) throw new Error(`retired staged runtime asset rejected: ${path}`);
  }
  return inventory;
}

export function addInstalledBlueprintInventory({ runtimeDir = runtime } = {}) {
  const manifestPath = join(runtimeDir, "runtime-inventory.json");
  const blueprintRoot = join(runtimeDir, "blueprint");
  if (!existsSync(manifestPath) || !existsSync(blueprintRoot)) throw new Error("installed Blueprint staging missing");
  const inventory = JSON.parse(readFileSync(manifestPath, "utf8"));
  inventory.entries = inventory.entries.filter((entry) => entry.component !== "blueprint-runtime");
  for (const source of filesAt(blueprintRoot, undefined, { includeIgnored: true })) {
    const local = relative(blueprintRoot, source).replaceAll("\\", "/");
    const staged = `blueprint/${local}`;
    inventory.entries.push({ component: "blueprint-runtime", delivery: "installedComponent", source: staged, stagePath: staged, installerPath: staged, sha256: digest(source) });
  }
  const packagePath = join(blueprintRoot, "app", "package", "package.json");
  if (!existsSync(packagePath)) throw new Error("installed Blueprint package manifest missing");
  const packageJson = JSON.parse(readFileSync(packagePath, "utf8"));
  const tree = treeDigest(blueprintRoot);
  inventory.components = { ...(inventory.components ?? {}), blueprint: { version: packageJson.version, treeSha256: tree.sha256, fileCount: tree.fileCount } };
  inventory.entries.sort((left, right) => left.stagePath.localeCompare(right.stagePath));
  writeFileSync(manifestPath, `${JSON.stringify(inventory, null, 2)}\n`);
  return verifyStagedInventory({ runtimeDir });
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
    const membrane = join(sidecarDir, "membrane.exe");
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
    async nativeBootstrap({ sidecarDir }) {
      for (const binary of ["membrane-tray.exe", "membrane-daemon.exe", "membrane.exe"]) await run(join(sidecarDir, binary), ["--help"]);
      return true;
    },
    async dashboardOnDemand() {
      const main = readFileSync(join(hub, "src-tauri", "src", "main.rs"), "utf8").split("#[cfg(test)]")[0];
      if (!main.includes("DashboardConnectionState::from_stdin()") || !main.includes("startup_owned_by_tray") || main.includes("run_hub_runtime") || main.includes("std::thread::spawn") || main.includes("mod supervisor;")) throw new Error("on-demand dashboard topology invalid");
      return true;
    },
    async blueprintInstalled({ runtimeDir, inventory }) {
      const contract = inventory.entries.find((entry) => entry.component === "blueprint-contract");
      const files = inventory.entries.filter((entry) => entry.component === "blueprint-runtime");
      if (!contract || contract.delivery !== "resource" || contract.transport !== "named-pipe") throw new Error("installed Blueprint contract missing");
      if (!files.length || !existsSync(join(runtimeDir, "blueprint", "lib", "node.exe")) || !existsSync(join(runtimeDir, "blueprint", "bin", "blueprint.cmd"))) throw new Error("installed Blueprint runtime missing");
      return true;
    },
    async hubInactive({ sidecarDir }) { return expectHubInactive(sidecarDir); },
  };
}

function verifySidecars(sidecarDir, inventory) {
  assertNoSymlinks(sidecarDir, "sidecar directory");
  const expected = new Map();
  for (const entry of inventory.entries.filter((candidate) => candidate.delivery === "externalBin")) {
    assertInventorySha256(entry.sha256, `external sidecar ${entry.component} sha256`);
    if (typeof entry.installerPath !== "string") throw new Error(`external sidecar installer path invalid: ${entry.installerPath}`);
    const path = entry.installerPath;
    if (path !== `${externalBinaryName(entry)}.exe`) throw new Error(`external sidecar installer path invalid: ${entry.installerPath}`);
    if (expected.has(path)) throw new Error(`duplicate external sidecar installer path: ${entry.installerPath}`);
    expected.set(path, entry);
  }
  for (const [path, entry] of expected) {
    const file = resolve(sidecarDir, path);
    const relativePath = relative(sidecarDir, file).replaceAll("\\", "/");
    if (relativePath !== path) throw new Error("unpacked sidecar missing: " + path);
    if (digest(assertRegularFile(file, "unpacked sidecar")) !== entry.sha256) throw new Error("unpacked sidecar hash mismatch: " + path);
  }
  for (const file of filesAt(sidecarDir, undefined, { includeIgnored: true })) {
    const path = relative(sidecarDir, file).replaceAll("\\", "/");
    if (retired.test(path)) throw new Error(`retired unpacked sidecar rejected: ${path}`);
    if (!expected.has(path)) throw new Error(`unexpected unpacked sidecar: ${path}`);
  }
}

// Native defaults exercise packaged bytes. Injected probes remain available for
// deterministic tests or product-specific artifact harnesses.
export async function verifyUnpackedArtifact({ runtimeDir = runtime, sidecarDir, probes = {} } = {}) {
  const inventory = verifyStagedInventory({ runtimeDir });
  if (JSON.stringify(inventory.composition) !== JSON.stringify(composition)) throw new Error("runtime composition invalid");
  if (inventory.axes?.length !== axes.length || inventory.axes.some(({ axis, entries }, index) => axis !== axes[index] || !Number.isInteger(entries) || entries !== 1)) throw new Error("six-axis unpacked runtime invalid");
  if (!inventory.entries.some((entry) => entry.component === "blueprint-runtime" && entry.delivery === "installedComponent")) throw new Error("installed Blueprint runtime missing");
  if (!inventory.components?.blueprint?.treeSha256 || !Number.isInteger(inventory.components.blueprint.fileCount)) throw new Error("installed Blueprint inventory metadata missing");
  if (!inventory.entries.some((entry) => entry.component === "blueprint-contract" && entry.delivery === "resource")) throw new Error("installed Blueprint contract missing");
  if (!inventory.entries.some((entry) => entry.component === "adapt-contract" && entry.invocation === "daemon-native")) throw new Error("Adapt invocation seam invalid");
  if (!sidecarDir) throw new Error("unpacked sidecar directory required");
  verifySidecars(sidecarDir, inventory);
  const activeProbes = Object.keys(probes).length ? probes : nativeUnpackedProbes();
  try {
    for (const name of ["nativeBootstrap", "dashboardOnDemand", "blueprintInstalled", "hubInactive"]) {
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
  writeFileSync(join(runtimeDir, "runtime-inventory.json"), `${JSON.stringify(inventory, null, 2)}\n`); verifyStagedInventory({ runtimeDir, sourceRoot: hubDir }); return inventory;
}

if (fileURLToPath(import.meta.url) === resolve(process.argv[1] || "")) {
  const action = process.argv[2] || "check";
  if (action === "write") writeRuntimeInventory(); else if (action === "check") runtimeInventory(); else if (action === "verify") verifyUnpackedArtifact().catch((error) => { console.error(error.message); process.exitCode = 1; }); else throw new Error("usage: runtime-inventory.mjs <check|write|verify>");
}
