import { spawnSync } from "node:child_process";

const result = spawnSync(process.env.ComSpec || process.env.COMSPEC || "cmd.exe", ["/d", "/s", "/c", "pnpm exec tauri bundle --bundles nsis"], { stdio: "inherit", windowsHide: true });
if (result.error) throw result.error;
if (result.status !== 0) throw new Error(`Windows NSIS bundle failed with exit ${result.status}`);
