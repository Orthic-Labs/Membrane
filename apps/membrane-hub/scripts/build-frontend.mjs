import { chmodSync, cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";

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

const targets = {
  "darwin-arm64": "aarch64-apple-darwin",
  "darwin-x64": "x86_64-apple-darwin",
  "win32-x64": "x86_64-pc-windows-msvc",
  "win32-arm64": "aarch64-pc-windows-msvc",
};
const target = process.env.TAURI_ENV_TARGET_TRIPLE || targets[`${process.platform}-${process.arch}`];
if (!target) throw new Error(`unsupported sidecar target: ${process.platform}-${process.arch}`);
const repo = new URL("../../../", import.meta.url);
const engine = new URL("engine/Cargo.toml", repo);
const engineTarget = new URL("engine/target/", repo);
const result = spawnSync("cargo", ["build", "--manifest-path", engine.pathname, "--release", "--target", target, "-p", "crypt", "-p", "membrane", "--bin", "crypt-service", "--bin", "membrane"], { cwd: repo, stdio: "inherit", env: { ...process.env, CARGO_TARGET_DIR: engineTarget.pathname } });
if (result.error) throw result.error;
if (result.status !== 0) throw new Error(`Membrane sidecar build failed with exit ${result.status}`);
const binaries = new URL("../src-tauri/binaries/", import.meta.url);
mkdirSync(binaries, { recursive: true });
for (const name of ["crypt-service", "membrane"]) {
  const suffix = target.includes("windows") ? ".exe" : "";
  const source = new URL(`${target}/release/${name}${suffix}`, engineTarget);
  if (!existsSync(source)) throw new Error(`missing sidecar: ${source.pathname}`);
  const destination = new URL(`${name}-${target}${suffix}`, binaries);
  cpSync(source, destination);
  if (process.platform !== "win32") chmodSync(destination, 0o755);
}
