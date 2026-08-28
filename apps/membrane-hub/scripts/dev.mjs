import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";

const hubRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workspaceRoot = resolve(hubRoot, "../../..");
const checkoutId = createHash("sha256").update(hubRoot.toLowerCase()).digest("hex").slice(0, 12);
const platformRoot = process.platform === "win32"
  ? process.env.LOCALAPPDATA
  : process.platform === "darwin"
    ? join(homedir(), "Library", "Application Support")
    : process.env.XDG_DATA_HOME ?? join(homedir(), ".local", "share");
if (!platformRoot) throw new Error("development data root is unavailable");
const devRoot = process.env.MEMBRANE_DEV_ROOT ?? join(platformRoot, "Orthic Labs", "Membrane Dev", checkoutId);
const devPort = String(process.env.MEMBRANE_DEV_PORT ?? (48_000 + (Number.parseInt(checkoutId.slice(0, 4), 16) % 1_000)));
const pnpm = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
const child = spawn(pnpm, ["exec", "tauri", "dev", "--no-watch"], {
  cwd: hubRoot,
  env: {
    ...process.env,
    MEMBRANE_RUNTIME_ORIGIN: "development",
    MEMBRANE_WORKSPACE_ROOT: workspaceRoot,
    MEMBRANE_PORT: devPort,
    MEMBRANE_CONFIG_ROOT: join(devRoot, "config"),
    MEMBRANE_DATA_ROOT: join(devRoot, "data"),
    MEMBRANE_CACHE_ROOT: join(devRoot, "cache"),
    MEMBRANE_LOG_ROOT: join(devRoot, "log"),
  },
  stdio: "inherit",
});

child.once("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  process.exitCode = code ?? 1;
});
