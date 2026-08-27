import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveTargetRoot } from "@rightkit/release/cargo-target.mjs";

if (process.platform !== "win32") throw new Error("Windows release build must run on Windows");

const sidecars = [
  "src-tauri/binaries/cortex-x86_64-pc-windows-msvc.exe",
  "src-tauri/binaries/membrane-x86_64-pc-windows-msvc.exe",
  "src-tauri/binaries/membrane-tray-x86_64-pc-windows-msvc.exe",
  "src-tauri/binaries/membrane-daemon-x86_64-pc-windows-msvc.exe",
];

function prepareNativeBinaries() {
  run(["exec", "rightkit", "cargo", "build", "--manifest-path", "../../engine/Cargo.toml", "--release", "--target", "x86_64-pc-windows-msvc", "-p", "membrane-runtime", "--bin", "membrane-daemon"]);
  run(["exec", "rightkit", "cargo", "build", "--manifest-path", "../membrane-tray-windows/Cargo.toml", "--release", "--target", "x86_64-pc-windows-msvc"]);
  const hub = fileURLToPath(new URL("../", import.meta.url));
  const target = "x86_64-pc-windows-msvc";
  const outputs = [
    [join(resolveTargetRoot(join(hub, "../../engine/Cargo.toml")), target, "release", "membrane-daemon.exe"), join(hub, "src-tauri/binaries/membrane-daemon-x86_64-pc-windows-msvc.exe")],
    [join(resolveTargetRoot(join(hub, "../membrane-tray-windows/Cargo.toml")), target, "release", "membrane-tray-windows.exe"), join(hub, "src-tauri/binaries/membrane-tray-x86_64-pc-windows-msvc.exe")],
  ];
  mkdirSync(join(hub, "src-tauri/binaries"), { recursive: true });
  for (const [source, destination] of outputs) {
    if (!existsSync(source)) throw new Error(`native Windows artifact missing: ${source}`);
    cpSync(source, destination);
  }
}

function run(args, env = process.env) {
  const result = spawnSync("pnpm.cmd", args, {
    cwd: new URL("../", import.meta.url),
    encoding: "utf8",
    env,
    shell: true,
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`pnpm exited ${result.status}`);
}

if (process.argv.includes("--prepare-only")) {
  prepareNativeBinaries();
  process.exit(0);
}
prepareNativeBinaries();
run(["exec", "right-release", "sign-windows", ...sidecars]);
run(["exec", "right-release", "sign-windows", "--verify-only", ...sidecars]);
run(["exec", "right-release", "build", "--platform", "win"], {
  ...process.env,
  MEMBRANE_SIGNED_SIDECARS_READY: "1",
});
