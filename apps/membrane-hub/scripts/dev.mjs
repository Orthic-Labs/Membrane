import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const hubRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workspaceRoot = resolve(hubRoot, "../../..");
const pnpm = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
const child = spawn(pnpm, ["exec", "tauri", "dev", "--no-watch"], {
  cwd: hubRoot,
  env: { ...process.env, MEMBRANE_WORKSPACE_ROOT: workspaceRoot },
  stdio: "inherit",
});

child.once("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  process.exitCode = code ?? 1;
});
