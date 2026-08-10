import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8"));
const version = pkg.version;
const mac = `src-tauri/target/release/bundle/dmg/Cortex_${version}_aarch64.dmg`;
const win = `src-tauri/target/release/bundle/nsis/Cortex_${version}_x64-setup.exe`;

export default {
  schema: 1,
  app: "cortex",
  version,
  packageManager: "pnpm",
  checks: ["test"],
  buildInputs: {
    include: ["index.html", "package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml", "scripts/**", "src/**", "src-tauri/**"],
    exclude: ["dist/**", "node_modules/**", "src-tauri/target/**", "src-tauri/gen/**"],
  },
  targets: {
    mac: {
      signed: true,
      package: { cmd: "pnpm", args: ["run", "rightkit:package:mac"] },
      artifacts: [mac], hardening: [mac],
      installer: { artifacts: [{ file: mac, key: "cortex/installers/mac/current/Cortex.dmg" }] },
    },
    win: {
      signed: true,
      package: { cmd: "pnpm", args: ["run", "rightkit:package:win"] },
      artifacts: [win], sign: { files: [win] }, hardening: [win],
      installer: { artifacts: [{ file: win, key: "cortex/installers/windows/current/Cortex_x64-setup.exe" }] },
      updater: { artifacts: [{ file: win, signature: `${win}.sig`, platform: "windows-x86_64", key: "cortex/updates/windows/current/Cortex_x64-setup.exe" }] },
    },
  },
};
