import { spawnSync } from "node:child_process";
import { renameSync, rmSync, readFileSync } from "node:fs";

const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;
const directory = `${process.env.CARGO_TARGET_DIR || "src-tauri/target"}/release/bundle/nsis/`;
const source = `${directory}Membrane Hub_${version}_x64-setup.exe`;
const destination = `${directory}Membrane_${version}_x64-setup.exe`;

run("pnpm", ["exec", "tauri", "build", "--bundles", "nsis"]);
run("node", ["scripts/write-release-manifest.mjs", "--identity", "dist/release-identity.json"]);
rmSync(destination, { force: true });
renameSync(source, destination);

function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with exit ${result.status}`);
}
