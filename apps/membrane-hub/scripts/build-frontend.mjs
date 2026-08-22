import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { engineReleaseIdentity } from "./release-identity.mjs";
import { resolveManagedCargoTarget } from "./lib/target-root.mjs";

const output = new URL("../dist/", import.meta.url);
mkdirSync(output, { recursive: true });
for (const name of readdirSync(output)) {
  if (name !== "release-identity.json") rmSync(join(fileURLToPath(output), name), { recursive: true, force: true });
}
for (const name of ["index.html", "popover.html", "src"]) {
  cpSync(new URL(`../${name}`, import.meta.url), new URL(name, output), { recursive: true });
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
};
const target = process.env.TAURI_ENV_TARGET_TRIPLE || targets[`${process.platform}-${process.arch}`];
if (!target) throw new Error(`unsupported sidecar target: ${process.platform}-${process.arch}`);
const repo = fileURLToPath(new URL("../../../", import.meta.url));
const engine = join(repo, "engine", "Cargo.toml");
// A managed build owns the target root; process.env.CARGO_TARGET_DIR can be
// set but stale, so it is never trusted to LOCATE output. cargo metadata
// against the engine manifest is the sole source of truth here.
const engineTarget = resolveManagedCargoTarget(engine);
// Bake the release identity in. `release_identity.rs` reads these through
// `option_env!`, so a build that omits them produces a binary permanently
// reporting `sha256:unknown` — which the gateway treats as an unverifiable
// release and answers with zero candidates. Every packet then ships empty
// while the hook still reports enforcement.
const identity = engineReleaseIdentity(repo);
console.log(
  `[membrane] release generation ${identity.releaseGeneration} `
  + `(commit ${identity.commit.slice(0, 8)}${identity.dirty ? ", working tree" : ""}, ${identity.fileCount} files)`,
);
// RightKit intentionally strips arbitrary ambient environment from managed
// Cargo requests. Write one build input outside the hashed engine subtree;
// membrane-runtime/build.rs validates it & emits compile-time identity.
const identityPath = new URL("../dist/release-identity.json", import.meta.url);
const identityText = `${JSON.stringify(identity, null, 2)}\n`;
if (!existsSync(identityPath) || readFileSync(identityPath, "utf8") !== identityText) {
  writeFileSync(identityPath, identityText);
}
const binaries = fileURLToPath(new URL("../src-tauri/binaries/", import.meta.url));
mkdirSync(binaries, { recursive: true });

if (process.env.MEMBRANE_SIDECARS_READY === "1") {
  for (const name of ["cortex", "membrane"]) {
    const ready = join(binaries, `${name}-${target}`);
    if (!existsSync(ready)) throw new Error(`prepared sidecar missing: ${ready}`);
  }
} else if (target === "universal-apple-darwin") {
  const architectureTargets = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
  for (const architectureTarget of architectureTargets) buildSidecars(architectureTarget);
  for (const name of ["cortex", "membrane"]) {
    const inputs = architectureTargets.map((architectureTarget) => stageSidecar(name, architectureTarget));
    const destination = join(binaries, `${name}-${target}`);
    run("lipo", ["-create", "-output", destination, ...inputs]);
    run("lipo", [destination, "-verify_arch", "x86_64", "arm64"]);
    chmodSync(destination, 0o755);
  }
} else {
  buildSidecars(target);
  for (const name of ["cortex", "membrane"]) stageSidecar(name, target);
}

function buildSidecars(architectureTarget) {
  const result = spawnSync("rightkit", ["cargo", "build", "--manifest-path", engine, "--release", "--target", architectureTarget, "-p", "cortex", "-p", "membrane", "--bin", "cortex", "--bin", "membrane"], { cwd: repo, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Membrane sidecar build failed for ${architectureTarget} with exit ${result.status}`);
}

function stageSidecar(name, architectureTarget) {
  const source = join(engineTarget, architectureTarget, "release", name);
  if (!existsSync(source)) throw new Error(`missing sidecar: ${source}`);
  const destination = join(binaries, `${name}-${architectureTarget}`);
  cpSync(source, destination);
  chmodSync(destination, 0o755);
  return destination;
}

function run(command, args) {
  const result = spawnSync(command, args, { cwd: repo, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with exit ${result.status}`);
}
