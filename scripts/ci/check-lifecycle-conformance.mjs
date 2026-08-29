// Gate: assert Architecture B (visible native tray parent, headless
// membrane-daemon child, OS-enforced lifetime coupling; Hub dashboard is an
// on-demand client with no resident runtime) against CODE facts, not doc
// prose. See docs/design/hub-redesign/DECISION-PROCESS-ARCHITECTURE.md
// (decided 2026-08-27) and docs/agent-rules.md.
//
// Every assertion below is pinned to an exact string/path verified present
// in the current tree. If any of these files move or the string they carry
// changes, this gate must be updated in the same change — that is the
// point: a topology regression (or an unnoticed drift back to the retired
// in-process model) fails CI instead of silently going green.
import { readFileSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const failures = [];

function read(relativePath) {
  return readFileSync(join(root, relativePath), "utf8");
}

function check(label, condition) {
  if (!condition) failures.push(label);
}

// 1. The Hub dashboard app carries no resident runtime dependency. It is an
// on-demand client (see apps/membrane-hub/src-tauri/src/main.rs), not a
// second Membrane planner/runtime host.
{
  const hubCargoToml = read("apps/membrane-hub/src-tauri/Cargo.toml");
  check(
    "apps/membrane-hub/src-tauri/Cargo.toml must not depend on membrane-runtime (Hub dashboard is an on-demand client, not a resident runtime host)",
    !/membrane-runtime/.test(hubCargoToml),
  );
}

// 2. The Membrane runtime crate declares the headless membrane-daemon
// binary that the tray spawns and supervises.
{
  const runtimeCargoToml = read("engine/crates/membrane-runtime/Cargo.toml");
  check(
    'engine/crates/membrane-runtime/Cargo.toml must declare [[bin]] name = "membrane-daemon"',
    /\[\[bin\]\]\s*\nname = "membrane-daemon"/.test(runtimeCargoToml),
  );
}

// 3. The Windows tray enforces kernel lifetime coupling via a Job Object
// with KILL_ON_JOB_CLOSE, not cooperative process tracking.
{
  const windowsProcess = read("apps/membrane-tray-windows/src/process.rs");
  check(
    "apps/membrane-tray-windows/src/process.rs must reference JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE (kernel-enforced daemon lifetime coupling)",
    windowsProcess.includes("JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE"),
  );
}

// 4. The macOS tray has a daemon supervisor (kqueue-based lifetime
// coupling; see DECISION-PROCESS-ARCHITECTURE.md §2).
{
  const macSupervisorPath = "apps/membrane-tray-macos/Sources/MembraneTrayMacOS/DaemonSupervisor.swift";
  check(
    `${macSupervisorPath} must exist (macOS daemon supervisor)`,
    existsSync(join(root, macSupervisorPath)),
  );
}

// 5. Stateless MCP/CLI clients never spawn a runtime process — they are
// thin HTTP clients against the loopback service the tray-owned daemon
// hosts, and fail typed when it is absent.
{
  const mcpClient = read("mcp/client.mjs");
  check(
    "mcp/client.mjs must not spawn a child process (stateless clients never start the runtime)",
    !/child_process|\bspawn\s*\(|\bexecFile\s*\(/.test(mcpClient),
  );
}

// 6. The Hub dashboard's production source (excluding #[cfg(test)] blocks)
// must not run the runtime in-process: no run_hub_runtime call, no
// std::thread::spawn, and no supervisor module — those are retired
// single-process-model constructs. It must instead proxy an inherited
// dashboard connection.
{
  const mainRs = read("apps/membrane-hub/src-tauri/src/main.rs");
  const production = mainRs.split("#[cfg(test)]")[0];
  check(
    "apps/membrane-hub/src-tauri/src/main.rs production source must not call run_hub_runtime (retired in-process runtime host)",
    !production.includes("run_hub_runtime"),
  );
  check(
    "apps/membrane-hub/src-tauri/src/main.rs production source must not use std::thread::spawn (retired in-process runtime host)",
    !production.includes("std::thread::spawn"),
  );
  check(
    "apps/membrane-hub/src-tauri/src/main.rs production source must not declare `mod supervisor;` (crash-loop supervision now lives in the native tray, not the Hub dashboard)",
    !/mod\s+supervisor;/.test(production),
  );
  check(
    "apps/membrane-hub/src-tauri/src/main.rs production source must use DashboardConnectionState::from_stdin() (on-demand dashboard proxies an inherited connection)",
    production.includes("DashboardConnectionState::from_stdin()"),
  );
}

if (failures.length) {
  console.error("lifecycle conformance check failed (Architecture B violated):");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
