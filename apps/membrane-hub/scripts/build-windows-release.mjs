// Windows phase two only. RightKit has already built, marker-patched, &
// Authenticode-signed raw EXE before this asks Tauri to create NSIS.
import { spawnSync } from "node:child_process";

const result = spawnSync("pnpm", ["exec", "tauri", "bundle", "--bundles", "nsis"], {
  stdio: "inherit",
  windowsHide: true,
});
if (result.error) throw result.error;
if (result.status !== 0) throw new Error(`Windows NSIS bundle failed with exit ${result.status}`);
