import { chmodSync, cpSync, existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { engineReleaseIdentity } from "./release-identity.mjs";
import { resolveManagedCargoTarget } from "./lib/target-root.mjs";

const output = new URL("../dist/", import.meta.url);
rmSync(output, { recursive: true, force: true });
mkdirSync(output, { recursive: true });
for (const name of ["index.html", "popover.html", "src"]) {
  cpSync(new URL(`../${name}`, import.meta.url), new URL(name, output), { recursive: true });
}
cpSync(new URL("../assets/tray", import.meta.url), new URL("assets/tray", output), { recursive: true });
cpSync(
  new URL("../node_modules/@tauri-apps/api", import.meta.url),
  new URL("vendor/@tauri-apps/api", output),
  { recursive: true },
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
  "win32-arm64": "aarch64-pc-windows-msvc",
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
const result = spawnSync("rightkit", ["cargo", "build", "--manifest-path", engine, "--release", "--target", target, "-p", "cortex", "-p", "membrane", "--bin", "cortex", "--bin", "cortex-service", "--bin", "membrane"], { cwd: repo, stdio: "inherit", env: { ...process.env, CORTEX_SOURCE_COMMIT: identity.commit, CORTEX_SOURCE_TREE_SHA256: identity.sourceTreeSha256 } });
if (result.error) throw result.error;
if (result.status !== 0) throw new Error(`Membrane sidecar build failed with exit ${result.status}`);
// The consumer of the baked value is the workspace release manifest; writing it
// beside the binaries keeps expected and observed in one step instead of two.
writeFileSync(new URL("../dist/release-identity.json", import.meta.url), `${JSON.stringify(identity, null, 2)}\n`);
const binaries = fileURLToPath(new URL("../src-tauri/binaries/", import.meta.url));
mkdirSync(binaries, { recursive: true });
for (const name of ["cortex", "cortex-service", "membrane"]) {
  const suffix = target.includes("windows") ? ".exe" : "";
  const source = join(engineTarget, target, "release", `${name}${suffix}`);
  if (!existsSync(source)) throw new Error(`missing sidecar: ${source}`);
  const destination = join(binaries, `${name}-${target}${suffix}`);
  cpSync(source, destination);
  if (process.platform !== "win32") chmodSync(destination, 0o755);
}
