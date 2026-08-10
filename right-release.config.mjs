import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));
const version = pkg.version;
const mac = `apps/cortex-tray/src-tauri/target/release/bundle/dmg/Cortex_${version}_aarch64.dmg`;
const win = `apps/cortex-tray/src-tauri/target/release/bundle/nsis/Cortex_${version}_x64-setup.exe`;

export default {
  schema: 1,
  app: "cortex",
  version,
  packageManager: "pnpm",
  checks: ["test:all"],
  buildInputs: {
    include: [
      "package.json", "pnpm-lock.yaml", "right-release.config.mjs",
      "apps/cortex-tray/**", "scripts/**", "lib/**", "graph/**", "schemas/**",
      "sdk/**", "service/**", "watchman/**", "release/**",
    ],
    exclude: [
      "**/node_modules/**", "**/target/**", "apps/cortex-tray/dist/**",
      "apps/cortex-tray/src-tauri/icons/**", "release/candidates/**",
    ],
  },
  targets: {
    mac: {
      signed: true,
      package: { cmd: "pnpm", args: ["--dir", "apps/cortex-tray", "run", "rightkit:package:mac"] },
      artifacts: [mac], hardening: [mac],
      installer: { artifacts: [{ file: mac, key: "cortex/installers/mac/current/Cortex.dmg" }] },
    },
    win: {
      signed: true,
      package: { cmd: "pnpm", args: ["--dir", "apps/cortex-tray", "run", "rightkit:package:win"] },
      artifacts: [win], sign: { files: [win] }, hardening: [win],
      installer: { artifacts: [{ file: win, key: "cortex/installers/windows/current/Cortex_x64-setup.exe" }] },
      updater: { artifacts: [{ file: win, signature: `${win}.sig`, platform: "windows-x86_64", key: "cortex/updates/windows/current/Cortex_x64-setup.exe" }] },
    },
  },
};
