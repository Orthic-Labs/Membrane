import { readFileSync } from "node:fs";

const packageJson = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));
const version = packageJson.version;
const buildInputs = {
  include: [
    "index.html", "popover.html", "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", "scripts/**", "src/**", "assets/**",
    "src-tauri/Cargo.toml", "src-tauri/Cargo.lock", "src-tauri/build.rs", "src-tauri/tauri.conf.json", "src-tauri/src/**", "src-tauri/runtime/**",
    "../../engine/Cargo.toml", "../../engine/Cargo.lock", "../../engine/crates/**",
    // Blueprint's own canonical stager derives runtime from this complete
    // source tree; no raw Blueprint or Adapt source is a Tauri resource.
    "../../blueprint/**", "../../schemas/**", "../../mcp/host/**", "../../dist/install/workspace/**", "../../dist/install/workspace-manifest.json",
  ],
  exclude: ["**/tests/**", "dist/**", "node_modules/**", "src-tauri/target/**", "src-tauri/gen/**"],
};
const macDmg = `src-tauri/target/release/bundle/dmg/Membrane Hub_${version}_aarch64.dmg`;

export default {
  schema: 1,
  app: "membrane-hub",
  version,
  packageManager: "pnpm",
  checks: ["test"],
  buildInputs,
  targets: {
    mac: {
      signed: true,
      package: { cmd: "pnpm", args: ["run", "rightkit:package:mac"] },
      artifacts: [macDmg],
      hardening: [macDmg],
      installer: { artifacts: [{ file: macDmg, key: "membrane/installers/mac/current/Membrane_Hub.dmg" }] },
    },
  },
};
