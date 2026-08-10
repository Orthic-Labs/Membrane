import { readFileSync } from "node:fs";

const packageJson = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));
const version = packageJson.version;
const buildInputs = {
  include: [
    "index.html", "popover.html", "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", "scripts/**", "src/**", "assets/**",
    "src-tauri/Cargo.toml", "src-tauri/Cargo.lock", "src-tauri/build.rs", "src-tauri/tauri.conf.json", "src-tauri/src/**",
    "brand.json",
    "schema/**",
  ],
  exclude: ["**/tests/**", "dist/**", "node_modules/**", "src-tauri/target/**", "src-tauri/gen/**"],
};
const macDmg = `src-tauri/target/release/bundle/dmg/Orthic_${version}_aarch64.dmg`;
const windowsInstaller = `src-tauri/target/release/bundle/nsis/Orthic_${version}_x64-setup.exe`;

export default {
  schema: 1,
  app: "orthic",
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
      installer: { artifacts: [{ file: macDmg, key: "membrane/installers/mac/current/Membrane_Hub.dmg" }, { file: macDmg, key: "cortex/installers/mac/current/Orthic.dmg" }] },
    },
    win: {
      signed: true,
      package: { cmd: "pnpm", args: ["run", "rightkit:package:win"] },
      artifacts: ["src-tauri/target/release/bundle/nsis"],
      sign: { files: [windowsInstaller] },
      hardening: ["src-tauri/target/release/bundle/nsis"],
      installer: { artifacts: [{ file: windowsInstaller, key: "membrane/installers/windows/current/Membrane_x64-setup.exe" }, { file: windowsInstaller, key: "cortex/installers/windows/current/Orthic_x64-setup.exe" }] },
      updater: { artifacts: [{ file: windowsInstaller, signature: `${windowsInstaller}.sig`, platform: "windows-x86_64", key: "membrane/updates/windows/current/Membrane_x64-setup.exe" }, { file: windowsInstaller, signature: `${windowsInstaller}.sig`, platform: "windows-x86_64", key: "cortex/updates/windows/current/Orthic_x64-setup.exe" }] },
    },
  },
};
