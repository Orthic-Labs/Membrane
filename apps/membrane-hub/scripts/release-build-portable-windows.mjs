import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveTargetRoot } from "@rightkit/release/cargo-target.mjs";

if (process.platform !== "win32") throw new Error("portable Windows release must run on Windows");

const hub = fileURLToPath(new URL("../", import.meta.url));
const target = "x86_64-pc-windows-msvc";
const sidecars = [
  "src-tauri/binaries/cortex-x86_64-pc-windows-msvc.exe",
  "src-tauri/binaries/membrane-x86_64-pc-windows-msvc.exe",
  "src-tauri/binaries/membrane-tray-x86_64-pc-windows-msvc.exe",
  "src-tauri/binaries/membrane-daemon-x86_64-pc-windows-msvc.exe",
];

function run(args, env = process.env) {
  const result = spawnSync("pnpm.cmd", args, {
    cwd: hub,
    encoding: "utf8",
    env,
    shell: true,
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`pnpm exited ${result.status}`);
}

// Reuse existing native build & workspace signing capability. Portable lane
// stops after signed raw app creation; it never enters NSIS packaging.
run(["run", "release:prepare:sidecars:win"]);
run(["exec", "right-release", "sign-windows", ...sidecars]);
run(["exec", "right-release", "sign-windows", "--verify-only", ...sidecars]);
run(["run", "rightkit:package:win", "--", "raw"], {
  ...process.env,
  MEMBRANE_SIGNED_SIDECARS_READY: "1",
});

const hubExe = join(
  resolveTargetRoot(join(hub, "src-tauri", "Cargo.toml")),
  target,
  "release",
  "membrane-hub.exe",
);
run(["exec", "right-release", "sign-windows", hubExe]);
run(["exec", "right-release", "sign-windows", "--verify-only", hubExe]);
run(["exec", "right-release", "hardening", hubExe, ...sidecars]);
run(["exec", "node", "scripts/package-portable-windows.mjs", "--hub-exe", hubExe]);
