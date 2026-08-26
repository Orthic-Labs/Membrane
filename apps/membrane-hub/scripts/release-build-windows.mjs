import { spawnSync } from "node:child_process";

if (process.platform !== "win32") throw new Error("Windows release build must run on Windows");

const sidecars = [
  "src-tauri/binaries/cortex-x86_64-pc-windows-msvc.exe",
  "src-tauri/binaries/membrane-x86_64-pc-windows-msvc.exe",
];

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

run(["exec", "right-release", "sign-windows", "--verify-only", ...sidecars]);
run(["exec", "right-release", "build", "--platform", "win"], {
  ...process.env,
  MEMBRANE_SIGNED_SIDECARS_READY: "1",
});
