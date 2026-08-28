import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveTargetRoot } from "@rightkit/release/cargo-target.mjs";

const hubRoot = fileURLToPath(new URL("./", import.meta.url));
const packageJson = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));
const version = packageJson.version;
const cargoTargetRoot = resolveTargetRoot(join(hubRoot, "src-tauri", "Cargo.toml"));
const cargoTriple = "x86_64-pc-windows-msvc";
const releaseRoot = join(cargoTargetRoot, cargoTriple, "release");
const buildInputs = {
  include: [
    "index.html", "popover.html", "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", "scripts/**", "src/**", "assets/**",
    "src-tauri/Cargo.toml", "src-tauri/Cargo.lock", "src-tauri/build.rs", "src-tauri/tauri.conf.json", "src-tauri/tauri.windows.conf.json", "src-tauri/windows/**", "src-tauri/src/**",
    "../membrane-tray-windows/Cargo.toml", "../membrane-tray-windows/Cargo.lock", "../membrane-tray-windows/build.rs", "../membrane-tray-windows/src/**", "../membrane-tray-windows/ui/**",
    "../../engine/Cargo.toml", "../../engine/Cargo.lock", "../../engine/crates/**",
    // Blueprint runtime is staged into Tauri resources before packaging.
    "../../schemas/**", "../../blueprint/package.json", "../../blueprint/pnpm-lock.yaml", "../../blueprint/scripts/**", "../../blueprint/src/**", "../../blueprint/release/**", "../../blueprint/LICENSE",
  ],
  exclude: ["**/tests/**", "dist/**", "node_modules/**", "src-tauri/target/**", "src-tauri/gen/**"],
};
// RightKit's Windows command runner uses cmd.exe. Keep artifact path free of
// spaces so signing receives one exact argv value rather than a split path.
const winInstaller = join(releaseRoot, "bundle", "nsis", `Membrane_Hub_${version}_x64-setup.exe`);
const winRawExe = join(releaseRoot, "membrane-hub.exe");

export default {
  schema: 1,
  app: "membrane-hub",
  version,
  packageManager: "pnpm",
  checks: ["test"],
  buildInputs,
  targets: {
    // RightKit owns Azure Authenticode + updater signing.  This target only
    // supplies its native Windows package command & exact files to seal.
    win: {
      signed: true,
      cargoTarget: cargoTriple,
      // RightKit patches Tauri's bundle marker & signs this raw EXE before the
      // package phase.  `tauri bundle` then embeds those immutable signed bytes.
      signingContract: "windows-raw-exe-authenticode-before-nsis-v1",
      prePackage: { cmd: "pnpm", args: ["run", "rightkit:package:win", "--", "raw"] },
      package: { cmd: "pnpm", args: ["run", "rightkit:package:win", "--", "package"] },
      artifacts: [winInstaller],
      sign: { prePackageFiles: [winRawExe], files: [winInstaller] },
      hardening: [winInstaller],
      installer: { artifacts: [{ file: winInstaller, key: "membrane/installers/windows/current/Membrane_x64-setup.exe" }] },
      updater: { artifacts: [{ file: winInstaller, signature: `${winInstaller}.sig`, platform: "windows-x86_64", key: "membrane/updates/windows/current/Membrane_x64-setup.exe" }] },
      nsisUpgradeContract: { windowsTauriConfig: "src-tauri/tauri.windows.conf.json" },
    },
  },
};
