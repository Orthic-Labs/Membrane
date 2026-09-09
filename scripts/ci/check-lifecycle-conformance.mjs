// Gate: assert Architecture B (visible native tray parent, headless
// membrane-daemon child, OS-enforced lifetime coupling; Hub dashboard is an
// on-demand client with no resident runtime) against CODE facts, not doc
// prose. See docs/architecture/adr/tray-daemon-process.md
// (decided 2026-08-27) and docs/agent-rules.md.
//
// Every assertion below is pinned to an exact string/path verified present
// in the current tree. If any of these files move or the string they carry
// changes, this gate must be updated in the same change — that is the
// point: a topology regression (or an unnoticed drift back to the retired
// in-process model) fails CI instead of silently going green.
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const failures = [];

function read(relativePath) {
  return readFileSync(join(root, relativePath), "utf8");
}

function check(label, condition) {
  if (!condition) failures.push(label);
}

// Installed qualification must prove native Blueprint operations across both
// resident & Hub-off paths; these names are intentionally stable evidence keys.
const qualificationEvidence = [
  "migration/native-rust/native-only-seal.json",
  "migration/native-rust/runtime-language-manifest.json",
];
for (const path of qualificationEvidence) {
  check(`${path} must be present for installed lifecycle qualification`, existsSync(join(root, path)));
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

// 5. Retained JS HTTP adapter never starts a resident process. Installed native
// MCP/CLI owns bounded explicit execution with Hub on or off. Resident process
// authority remains tray-owned.
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

// Agent-facing current doctrine must not reintroduce blanket Hub-off refusal.
// Historical/archive documents deliberately remain outside this check.
function currentMarkdown(directory) {
  return readdirSync(join(root, directory), { withFileTypes: true }).flatMap((entry) => {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory()) return entry.name === "archive" ? [] : currentMarkdown(path);
    return entry.isFile() && entry.name.endsWith(".md") ? [path] : [];
  });
}
for (const path of ["README.md", ...["docs/product", "docs/architecture", "docs/canon", "docs/reference"].flatMap(currentMarkdown)]) {
  const prose = read(path).replace(/\s+/g, " ");
  for (const forbidden of [
    /tray off means Membrane is unavailable/i,
    /tray-off access returns typed unavailability/i,
    /canonical state remains daemon-bound/i,
    /Blueprint's special one-shot exception/i,
    /Operational Ledger CLI is a daemon client/i,
    /Membrane remains unavailable/i,
  ]) check(`${path} contradicts explicit execution boundary: ${forbidden}`, !forbidden.test(prose));
}

const boundary = read("docs/architecture/execution-lifecycle-boundary.md");
check("execution contract must separate Hub-off explicit operations from automatic Hub-owned work",
  boundary.includes("Explicit operations available with Hub off") && boundary.includes("Automatic work requires Hub"));
for (const subsystem of ["Pull", "Blueprint", "Cortex", "Ledger", "Adapt", "Push"]) {
  check(`execution contract must cover ${subsystem}`, boundary.includes(`| ${subsystem} |`));
}
for (const path of ["README.md", "docs/architecture/membrane.md", "docs/canon/membrane.md",
  "docs/reference/cli/README.md", "docs/architecture/integrations/coderight.md",
  "docs/architecture/security/mcp-threat-model.md"]) {
  check(`${path} must retain explicit Hub-off availability doctrine`,
    /explicit[^.\n]{0,240}Hub(?: on or)? off/i.test(read(path)));
}

// Native closure policy: no packaged Blueprint interpreter, launcher, or
// bounded external interpreter allowance may survive sealed qualification.
try {
  const policy = JSON.parse(read("migration/native-rust/runtime-policy.json"));
  check("sealed runtime policy must allow zero bounded external interpreter rows",
    policy.enforcementMode === "sealed" && (policy.sealedExternalInterpreterRows ?? []).length === 0);
  check("sealed runtime policy must have no Blueprint interpreter exception",
    !(policy.exceptions ?? []).some((entry) => /blueprint/i.test(JSON.stringify(entry))));
  check("sealed runtime policy must reject retired Blueprint interpreter selectors",
    (policy.deletedSelectors ?? []).some((entry) => String(entry).startsWith("blueprint/")));
} catch (error) {
  failures.push(`runtime policy could not be read: ${error.message}`);
}

if (failures.length) {
  console.error("lifecycle conformance check failed (Architecture B violated):");
  for (const failure of failures) console.error(`  - ${failure}`);
  process.exit(1);
}
