import { chmodSync, cpSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { writeEngineReleaseIdentity } from "./release-identity.mjs";
import { PRESENTATION_ASSETS } from "./presentation-assets.mjs";

const output = new URL("../dist/", import.meta.url);
mkdirSync(output, { recursive: true });
for (const name of readdirSync(output)) {
  if (name !== "release-identity.json") rmSync(join(fileURLToPath(output), name), { recursive: true, force: true });
}
for (const name of PRESENTATION_ASSETS) {
  const destination = new URL(name, output);
  mkdirSync(new URL("./", destination), { recursive: true });
  cpSync(new URL(`../${name}`, import.meta.url), destination);
}
cpSync(new URL("../assets/tray", import.meta.url), new URL("assets/tray", output), { recursive: true });
cpSync(
  new URL("../node_modules/@tauri-apps/api", import.meta.url),
  new URL("vendor/@tauri-apps/api", output),
  { recursive: true, dereference: true },
);
cpSync(
  new URL("../node_modules/@tauri-apps/plugin-os/dist-js", import.meta.url),
  new URL("vendor/@tauri-apps/plugin-os", output),
  { recursive: true },
);
cpSync(
  new URL("../node_modules/@rightkit/platform-ui/dist", import.meta.url),
  new URL("vendor/@rightkit/platform-ui", output),
  { recursive: true },
);

const targets = {
  "darwin-arm64": "aarch64-apple-darwin",
  "darwin-x64": "x86_64-apple-darwin",
  "win32-x64": "x86_64-pc-windows-msvc",
};
const target = process.env.TAURI_ENV_TARGET_TRIPLE || targets[`${process.platform}-${process.arch}`];
if (!target) throw new Error(`unsupported sidecar target: ${process.platform}-${process.arch}`);
const repo = fileURLToPath(new URL("../../../", import.meta.url));
const engine = join(repo, "engine", "Cargo.toml");
// Bake the release identity in. `release_identity.rs` reads these through
// `option_env!`, so a build that omits them produces a binary permanently
// reporting `sha256:unknown` — which the gateway treats as an unverifiable
// release and answers with zero candidates. Every packet then ships empty
// while the hook still reports enforcement.
const identityPath = new URL("../dist/release-identity.json", import.meta.url);
const identity = writeEngineReleaseIdentity(repo, fileURLToPath(identityPath));
console.log(
  `[membrane] release generation ${identity.releaseGeneration} `
  + `(commit ${identity.commit.slice(0, 8)}${identity.dirty ? ", working tree" : ""}, ${identity.fileCount} files)`,
);
// RightKit intentionally strips arbitrary ambient environment from managed
// Cargo requests. Write one build input outside the hashed engine subtree;
// membrane-runtime/build.rs validates it & emits compile-time identity.
const binaries = fileURLToPath(new URL("../src-tauri/binaries/", import.meta.url));
mkdirSync(binaries, { recursive: true });

if (process.env.MEMBRANE_SIDECARS_READY === "1") {
  for (const name of ["cortex", "membrane"]) {
    const ready = join(binaries, `${name}-${target}${target.includes("windows") ? ".exe" : ""}`);
    if (!existsSync(ready)) throw new Error(`prepared sidecar missing: ${ready}`);
  }
} else if (target === "universal-apple-darwin") {
  const architectureTargets = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
  const artifacts = new Map(architectureTargets.map((architectureTarget) => [architectureTarget, buildSidecars(architectureTarget)]));
  for (const name of ["cortex", "membrane"]) {
    const inputs = architectureTargets.map((architectureTarget) => stageSidecar(name, architectureTarget, artifacts.get(architectureTarget).get(name)));
    const destination = join(binaries, `${name}-${target}`);
    run("lipo", ["-create", "-output", destination, ...inputs]);
    run("lipo", [destination, "-verify_arch", "x86_64", "arm64"]);
    chmodSync(destination, 0o755);
  }
} else {
  const artifacts = buildSidecars(target);
  for (const name of ["cortex", "membrane"]) stageSidecar(name, target, artifacts.get(name));
}

function buildSidecars(architectureTarget) {
  const rightkit = process.env.RIGHTKIT || "rightkit";
  const result = spawnSync(rightkit, ["cargo", "build", "--manifest-path", engine, "--release", "--target", architectureTarget, "-p", "cortex", "-p", "membrane", "--bin", "cortex", "--bin", "membrane", "--message-format=json-render-diagnostics"], { cwd: repo, encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.stdout) process.stdout.write(result.stdout);
  if (result.stderr) process.stderr.write(result.stderr);
  if (result.status !== 0) throw new Error(`Membrane sidecar RightKit build failed for ${architectureTarget} with exit ${result.status}`);
  const artifacts = new Map();
  for (const line of String(result.stdout ?? "").split(/\r?\n/)) {
    let message;
    try { message = JSON.parse(line); } catch { continue; }
    if (message.reason !== "compiler-artifact" || !message.executable) continue;
    const name = message.target?.name;
    if (["cortex", "membrane"].includes(name) && message.target?.kind?.includes("bin")) artifacts.set(name, message.executable);
  }
  for (const name of ["cortex", "membrane"]) {
    if (!artifacts.has(name)) throw new Error(`RightKit emitted no compiler artifact for ${name} (${architectureTarget})`);
  }
  return artifacts;
}

function stageSidecar(name, architectureTarget, source) {
  const suffix = architectureTarget.includes("windows") ? ".exe" : "";
  if (!existsSync(source)) throw new Error(`missing sidecar: ${source}`);
  const destination = join(binaries, `${name}-${architectureTarget}${suffix}`);
  cpSync(source, destination);
  chmodSync(destination, 0o755);
  return destination;
}

function run(command, args) {
  const result = spawnSync(command, args, { cwd: repo, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with exit ${result.status}`);
}
