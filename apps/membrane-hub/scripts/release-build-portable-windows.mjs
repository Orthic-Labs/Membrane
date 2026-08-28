import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { resolveTargetRoot } from "@rightkit/release/cargo-target.mjs";
import { materializeHardeningEvidence } from "@rightkit/release/hardening-evidence.mjs";
import { mkdirSync } from "node:fs";

if (process.platform !== "win32") throw new Error("portable Windows release must run on Windows");

const hub = fileURLToPath(new URL("../", import.meta.url));
const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const startedAt = new Date().toISOString();
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

function runRightReleaseAtRepoRoot(args) {
  const result = spawnSync(join(hub, "node_modules", ".bin", "right-release.cmd"), args, {
    cwd: repoRoot,
    encoding: "utf8",
    env: process.env,
    shell: true,
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`right-release exited ${result.status}`);
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
const hardeningEvidence = join(hub, "dist", "portable", "windows-hardening-evidence.json");
mkdirSync(join(hub, "dist", "portable"), { recursive: true });
materializeHardeningEvidence({
  root: repoRoot,
  outputPath: hardeningEvidence,
  allowances: sidecars.slice(0, 2).map((file) => ({
    rule: "system-prompt-marker",
    exact: "system_prompt",
    file: join(hub, file),
    sourceEvidence: "engine/crates/membrane-adapt/src/remediation.rs:74",
    rationale: "Public remediation scope enum serialization token; no prompt content is embedded.",
  })),
});
runRightReleaseAtRepoRoot(["hardening", "--allow-evidence", hardeningEvidence, hubExe, ...sidecars.map((file) => join(hub, file))]);
run(["exec", "node", "scripts/package-portable-windows.mjs", "--hub-exe", hubExe, "--started-at", startedAt]);
run(["exec", "node", "scripts/finalize-portable-release.mjs"]);
