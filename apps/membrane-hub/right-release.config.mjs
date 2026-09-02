import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveTargetRoot } from "@rightkit/release/cargo-target.mjs";

const hubRoot = fileURLToPath(new URL("./", import.meta.url));
const packageJson = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));
const version = packageJson.version;
// RIGHT_RELEASE_OFFLINE=1 lets CI and tests import this config for its shape
// without shelling out to `cargo metadata` (RightKit forbids cargo on this
// public repo). The placeholder target root is never used to locate real bytes
// in that mode — finalize/sign always run online on a native host.
const cargoTargetRoot = process.env.RIGHT_RELEASE_OFFLINE === "1"
  ? join(hubRoot, "target")
  : resolveTargetRoot(join(hubRoot, "src-tauri", "Cargo.toml"));
const cargoTriple = "x86_64-pc-windows-msvc";
const releaseRoot = join(cargoTargetRoot, cargoTriple, "release");
const macCargoTriple = "aarch64-apple-darwin";
const macReleaseRoot = join(cargoTargetRoot, macCargoTriple, "release");
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
const macDmg = join(macReleaseRoot, "bundle", "dmg", `Membrane Hub_${version}_aarch64.dmg`);

export default {
  schema: 1,
  app: "membrane-hub",
  version,
  distribution: { provider: "github-releases", repository: "Orthic-Labs/Membrane" },
  packageManager: "pnpm",
  // Tests belong to the one CI gate; finalize only packages and signs bytes CI already proved.
  checks: [],
  buildInputs,
  targets: {
    // Tauri seals nested app contents while assembling the DMG. RightKit then
    // owns the outer Developer ID signature, notarization & release evidence.
    mac: {
      signed: true,
      cargoTarget: macCargoTriple,
      signingContract: "macos-developer-id-notarized-portable-v1",
      prePackage: { cmd: "pnpm", args: ["run", "rightkit:prepackage:mac"] },
      package: { cmd: "pnpm", args: ["run", "rightkit:package:mac"] },
      artifacts: [macDmg],
      sign: { prePackageFiles: [macDmg] },
      notarize: { file: macDmg },
      hardening: [macDmg],
      installer: { artifacts: [{ file: macDmg, key: "membrane/installers/mac/current/Membrane_Hub.dmg" }] },
    },
    // RightKit owns Azure Authenticode signing. This target only
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

      nsisUpgradeContract: { windowsTauriConfig: "src-tauri/tauri.windows.conf.json" },
    },
  },
};
