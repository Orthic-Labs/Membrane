// Windows phase two only. RightKit has already built, marker-patched, &
// Authenticode-signed raw EXE before this asks Tauri to create NSIS.
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";

const version = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).version;

const result = spawnSync("pnpm", ["exec", "tauri", "bundle", "--bundles", "nsis"], {
  stdio: "inherit",
  windowsHide: true,
});
if (result.error) throw result.error;
if (result.status !== 0) throw new Error(`Windows NSIS bundle failed with exit ${result.status}`);
const installer = `src-tauri/target/release/bundle/nsis/Membrane_${version}_x64-setup.exe`;
const probe = spawnSync("node", ["scripts/verify-unpacked-artifact.mjs", "--nsis", installer], { stdio: "inherit", windowsHide: true });
if (probe.error) throw probe.error;
if (probe.status !== 0) throw new Error(`Windows unpacked artifact probe failed with exit ${probe.status}`);
