#!/usr/bin/env node
// scripts/qualification/cases/ncl-windows.mjs -- native-cleanup lane case module.
//
// Contract: each export is called by scripts/qualification/run.mjs runOneRegistryCase
// as caseFunction({ row, workspaceRoot, profile, platform, evidencePath }) and must
// return { status: "passed"|<anything else>, evidenceKind, detail, reason } where
// evidenceKind is one of run.mjs EVIDENCE_KINDS (source, component, integration,
// installed, host, task-outcome). A missing/unrecognized evidenceKind is treated by
// the runner as a hard failure, never a soft pass.
//
// Honesty policy for this module (docs/agent-rules.md, membrane.prompt.md native-cleanup
// section, windows-amendment-acceptance.json NCL-01..05): this worker may not run cargo,
// builds, installs, packaging, or process-tree captures. NCL-03 and NCL-05 need an
// installed, interpreter-stripped binary and a live process-tree capture that only the
// integration owner can produce; this module therefore never fabricates a "passed" for
// those two -- it performs the structural checks it CAN do from the live source tree
// (source/component evidenceKind) and reports the remaining gap as a typed
// "insufficient" outcome naming exactly what installed-path evidence would close it.
// NCL-01, NCL-02 and NCL-04 are checkable from the live tree/registry alone and are
// scored pass/fail for real.

import { execFileSync, spawn } from "node:child_process";
import { existsSync, readFileSync, statSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");

const INTERPRETED_EXTENSIONS = [".py", ".mjs", ".cjs", ".js", ".ts", ".sh", ".ps1", ".cmd", ".bat"];

function installedRoot(context) {
  return (context && context.installedRoot) || process.env.MEMBRANE_INSTALLED_ROOT ||
    join(process.env.LOCALAPPDATA || "", "Orthic Labs", "Membrane");
}

function installedExecutables(root) {
  const current = join(root, "current");
  const roots = [current, root];
  const names = new Set(["membrane.exe", "membrane-daemon.exe", "membrane-tray.exe", "membrane-mcp.exe"]);
  const found = [];
  for (const base of roots) {
    if (!existsSync(base)) continue;
    let entries;
    try { entries = readdirSync(base, { withFileTypes: true }); } catch { continue; }
    for (const entry of entries) if (entry.isFile() && names.has(entry.name.toLowerCase())) found.push(join(base, entry.name));
  }
  return [...new Set(found)];
}

function processSnapshot() {
  const script = "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,ExecutablePath,CommandLine | ConvertTo-Json -Compress";
  try {
    const raw = execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", script], { encoding: "utf8", timeout: 15000 }).trim();
    if (!raw) return [];
    const value = JSON.parse(raw);
    return (Array.isArray(value) ? value : [value]).map((p) => ({ pid: p.ProcessId, ppid: p.ParentProcessId, name: p.Name, executable: p.ExecutablePath || null, commandLine: p.CommandLine || null }));
  } catch (error) { return { error: error.message }; }
}

function processSubtree(snapshot, rootPid) {
  if (!Array.isArray(snapshot) || !rootPid) return [];
  const children = new Map();
  for (const process of snapshot) {
    const siblings = children.get(process.ppid) || [];
    siblings.push(process);
    children.set(process.ppid, siblings);
  }
  const out = [];
  const visit = (pid) => { for (const child of children.get(pid) || []) { out.push(child); visit(child.pid); } };
  visit(rootPid);
  return out;
}

function nativeProcessProbe(executables) {
  if (process.platform !== "win32") return { ok: false, reason: "native Windows process probe requires win32" };
  if (executables.length === 0) return { ok: false, reason: "no installed Membrane executable under stable current root" };
  const before = processSnapshot();
  const runs = [];
  for (const executable of executables) {
    const child = spawn(executable, ["--version"], { windowsHide: true, stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, PATH: "C:\\Windows\\System32;C:\\Windows" } });
    const startedAt = new Date().toISOString();
    const snapshot = processSnapshot();
    const during = [{ pid: child.pid, ppid: process.pid, name: executable.split(/[\\/]/).pop(), executable, commandLine: executable }, ...processSubtree(snapshot, child.pid)];
    child.kill();
    runs.push({ executable, startedAt, exitCode: child.exitCode, processTree: during });
  }
  const forbidden = runs.flatMap((r) => Array.isArray(r.processTree) ? r.processTree.filter((p) => /^(node|python|python3|sh|bash)(\.exe)?$/i.test(String(p.name || ""))) : []);
  return { ok: runs.length === executables.length && forbidden.length === 0, before, runs, forbidden };
}

function receiptProbe(context, schema) {
  const path = context && context.evidencePath || process.env.MEMBRANE_NATIVE_OBSERVATION;
  if (!path || !existsSync(path)) return { ok: false, reason: `missing ${schema} observation receipt` };
  try {
    const value = JSON.parse(readFileSync(path, "utf8"));
    const valid = value.schema === schema && value.platform === "windows" && Array.isArray(value.processTree) && value.generatedAt;
    return valid ? { ok: true, value } : { ok: false, reason: `invalid ${schema} observation shape` };
  } catch (error) { return { ok: false, reason: `cannot read observation receipt: ${error.message}` }; }
}

function resolveRoot(context) {
  return (context && context.workspaceRoot) || (context && context.root) || REPO_ROOT;
}

function readJson(root, relPath) {
  const abs = join(root, relPath);
  if (!existsSync(abs)) return { ok: false, error: `not found: ${relPath}` };
  try {
    return { ok: true, value: JSON.parse(readFileSync(abs, "utf8")) };
  } catch (error) {
    return { ok: false, error: `parse error in ${relPath}: ${error.message}` };
  }
}

function hasExt(p) {
  return INTERPRETED_EXTENSIONS.some((ext) => p.toLowerCase().endsWith(ext));
}

// Real git-tracked + untracked-but-interpreted file listing. Untracked files that are
// git-ignored are excluded (they are not part of the live shippable tree); this mirrors
// "git ls-files + untracked interpreted files" from the NCL-01 acceptance text.
function listLiveInterpretedFiles(root) {
  const tracked = execFileSync("git", ["ls-files"], { cwd: root, encoding: "utf8" })
    .split(/\r?\n/)
    .filter(Boolean);
  let untracked = [];
  try {
    untracked = execFileSync(
      "git",
      ["ls-files", "--others", "--exclude-standard"],
      { cwd: root, encoding: "utf8" },
    )
      .split(/\r?\n/)
      .filter(Boolean);
  } catch {
    untracked = [];
  }
  const all = [...new Set([...tracked, ...untracked])];
  return all.filter(hasExt).map((p) => p.replace(/\\/g, "/")).sort();
}

// -----------------------------------------------------------------------------------
// NCL-01 -- interpreter inventory equals live tree.
// -----------------------------------------------------------------------------------
export function NCL_01(context) {
  const root = resolveRoot(context);
  const disp = readJson(root, "docs/../../../review/windows-r5/interpreter-dispositions.json");
  // interpreter-dispositions.json lives outside the repo (D:/Claude/review/windows-r5);
  // resolve it from an absolute path override when the relative repo-join misses, rather
  // than fabricate a pass over a file we never actually read.
  let dispositions = disp;
  if (!dispositions.ok) {
    const absOverride = (context && context.interpreterDispositionsPath) ||
      "D:/Claude/review/windows-r5/interpreter-dispositions.json";
    if (existsSync(absOverride)) {
      try {
        dispositions = { ok: true, value: JSON.parse(readFileSync(absOverride, "utf8")) };
      } catch (error) {
        dispositions = { ok: false, error: error.message };
      }
    }
  }
  if (!dispositions.ok) {
    return {
      status: "failed",
      evidenceKind: "source",
      detail: { error: dispositions.error },
      reason: `NCL-01: could not load interpreter-dispositions.json: ${dispositions.error}`,
    };
  }
  const rows = dispositions.value;
  if (!Array.isArray(rows)) {
    return {
      status: "failed",
      evidenceKind: "source",
      detail: {},
      reason: "NCL-01: interpreter-dispositions.json is not an array of rows",
    };
  }

  const missingFields = rows.filter((r) => !r.path || !r.action || !r.owner || !r.requiredProof);
  const inventoryPaths = new Set(rows.map((r) => r.path));
  const live = listLiveInterpretedFiles(root);

  const unclassified = live.filter((p) => !inventoryPaths.has(p));
  // A "dead" entry is an inventory path absent from disk -- a plain filesystem existence
  // check, independent of the interpreted-extension filter used for liveSet. liveSet is
  // extension-filtered (INTERPRETED_EXTENSIONS), so it wrongly excludes extensionless
  // launchers (blueprint, blueprint-mcp) and non-listed extensions (.bash, .rb), making
  // every such inventory row look "dead" even though the file exists on disk. The
  // unclassified check above is correctly extension-filtered (it only cares about live
  // interpreted files missing from the inventory); the dead check below must not be.
  const deadEntries = rows.filter((r) => r.path && !existsSync(join(root, r.path))).map((r) => r.path);
  const windowsLaunchers = live.filter((p) => p.endsWith(".ps1") || p.endsWith(".cmd"));
  const missingLaunchers = windowsLaunchers.filter((p) => !inventoryPaths.has(p));

  const problems = [];
  if (missingFields.length > 0) {
    problems.push(`${missingFields.length} row(s) missing action/owner/requiredProof`);
  }
  if (unclassified.length > 0) {
    problems.push(`${unclassified.length} live interpreted file(s) absent from inventory: ${unclassified.slice(0, 10).join(", ")}${unclassified.length > 10 ? ", ..." : ""}`);
  }
  if (deadEntries.length > 0) {
    problems.push(`${deadEntries.length} inventory entr(y/ies) reference file(s) absent on disk: ${deadEntries.slice(0, 10).join(", ")}${deadEntries.length > 10 ? ", ..." : ""}`);
  }
  if (missingLaunchers.length > 0) {
    problems.push(`${missingLaunchers.length} Windows launcher(s) (.ps1/.cmd) missing from inventory: ${missingLaunchers.join(", ")}`);
  }

  const detail = {
    inventoryCount: rows.length,
    liveCount: live.length,
    unclassifiedCount: unclassified.length,
    deadEntryCount: deadEntries.length,
    missingLauncherCount: missingLaunchers.length,
    missingFieldCount: missingFields.length,
  };

  if (problems.length > 0) {
    return {
      status: "failed",
      evidenceKind: "source",
      detail,
      reason: `NCL-01: ${problems.join("; ")}`,
    };
  }
  return {
    status: "passed",
    evidenceKind: "source",
    detail,
    reason: "NCL-01: interpreter-dispositions.json path set equals live tracked+untracked interpreted files, and every entry carries action/owner/requiredProof",
  };
}

// -----------------------------------------------------------------------------------
// NCL-02 -- parity before delete, per cluster.
//
// This module structurally checks, per delete-after-parity row, whether a named native
// destination module exists on disk and whether the row's requiredProof fields are
// present in the dispositions/effective-ownership records. It cannot itself observe
// another lane's ported-assertion test run (that is task-outcome evidence produced by
// the integration owner's sweep), so a cluster with no recorded native test evidence
// hash is reported "insufficient", never "passed".
// -----------------------------------------------------------------------------------
export function NCL_02(context) {
  const root = resolveRoot(context);
  const absOverride = (context && context.interpreterDispositionsPath) ||
    "D:/Claude/review/windows-r5/interpreter-dispositions.json";
  if (!existsSync(absOverride)) {
    return {
      status: "failed",
      evidenceKind: "source",
      detail: {},
      reason: "NCL-02: interpreter-dispositions.json not found",
    };
  }
  let rows;
  try {
    rows = JSON.parse(readFileSync(absOverride, "utf8"));
  } catch (error) {
    return { status: "failed", evidenceKind: "source", detail: { error: error.message }, reason: `NCL-02: parse error: ${error.message}` };
  }
  const clusters = rows.filter((r) => r.action === "delete-after-parity");
  const withDestinationOwner = clusters.filter((r) => r.nativeDestinationOwner);
  const withoutDestinationOwner = clusters.filter((r) => !r.nativeDestinationOwner);

  // Vacuous-test detection is out of scope for a source-only check (it requires reading
  // and semantically evaluating another lane's Rust test bodies for assertion-free
  // scaffolding, e.g. the packet's own cited empty_dimension_dependencies_cannot_be_sealed
  // example) -- this module records that as an explicit unproven gap rather than a pass.
  return {
    status: "insufficient",
    evidenceKind: "source",
    detail: {
      clusterCount: clusters.length,
      withDestinationOwner: withDestinationOwner.length,
      withoutDestinationOwner: withoutDestinationOwner.map((r) => r.path),
    },
    reason: `NCL-02: ${clusters.length} delete-after-parity row(s) identified from interpreter-dispositions.json; ${withoutDestinationOwner.length} lack a nativeDestinationOwner and cannot even be candidates. Per-cluster ported-assertion test names, native test evidence hash freshness, and vacuous-assertion detection require reading each collaborator lane's actual test bodies and the I1 sweep evidence, which this worker\'s allowlist and no-build/no-test constraint do not permit it to execute or fabricate; reported insufficient pending that per-cluster evidence.`,
  };
}

// -----------------------------------------------------------------------------------
// NCL-03 -- native-only installed runtime proof. Requires an installed, interpreter-
// stripped build and a live process-tree capture -- both installed-path facts this
// worker cannot produce (no builds/installs/packaging permitted). Reported insufficient,
// naming exactly the installed-path evidence that would close it.
// -----------------------------------------------------------------------------------
export function NCL_03(context) {
  const root = resolveRoot(context);
  const rootPath = installedRoot(context);
  const executables = installedExecutables(rootPath);
  const probe = nativeProcessProbe(executables);
  const payloadInterpreters = [];
  const scan = (dir, depth = 0) => {
    if (depth > 8 || !existsSync(dir)) return;
    let entries; try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const entry of entries) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) scan(path, depth + 1);
      else if (/^(node|nodejs|python|python3|sh|bash)(\.exe)?$/i.test(entry.name)) payloadInterpreters.push(path);
    }
  };
  scan(rootPath);
  const payloadOk = payloadInterpreters.length === 0;
  const passed = probe.ok && payloadOk;
  return {
    status: passed ? "passed" : "insufficient",
    evidenceKind: "installed",
    detail: { root, installedRoot: rootPath, executables, payloadInterpreters, probe },
    reason: passed
      ? "NCL-03: installed native executables ran with restricted PATH, live process snapshots contained no interpreter children, and payload contained no interpreter executable"
      : `NCL-03: installed native probe incomplete: ${probe.reason || (probe.forbidden?.length ? "interpreter child observed" : "process probe failed")}${payloadOk ? "" : `; bundled interpreter executable(s): ${payloadInterpreters.join(", ")}`}`,
  };
}

// -----------------------------------------------------------------------------------
// NCL-04 -- accounting and measurements are fresh Windows data.
// -----------------------------------------------------------------------------------
export function NCL_04(context) {
  const root = resolveRoot(context);
  const absOverride = (context && context.interpreterDispositionsPath) ||
    "D:/Claude/review/windows-r5/interpreter-dispositions.json";
  if (!existsSync(absOverride)) {
    return { status: "failed", evidenceKind: "source", detail: {}, reason: "NCL-04: interpreter-dispositions.json not found" };
  }
  let rows;
  try {
    rows = JSON.parse(readFileSync(absOverride, "utf8"));
  } catch (error) {
    return { status: "failed", evidenceKind: "source", detail: { error: error.message }, reason: `NCL-04: parse error: ${error.message}` };
  }
  const actions = { "delete-after-parity": 0, "retain-with-reachability-proof": 0, "commit-deletion": 0 };
  for (const r of rows) {
    if (r.action in actions) actions[r.action] += 1;
  }
  const receiptPath = (context && context.receiptPath) ||
    join(dirname(dirname(dirname(root))), "scratchpad", "lanes", "receipts", "native-cleanup__waveB.json");
  const receiptExists = existsSync(receiptPath);
  let receipt = null;
  if (receiptExists) { try { receipt = JSON.parse(readFileSync(receiptPath, "utf8")); } catch {} }
  const measured = receipt?.platform === "windows" && receipt?.buildIdentity && receipt?.generatedAt &&
    receipt?.accounting && receipt?.storage?.compatibility && receipt.storage.compatibility !== "unmeasured" &&
    receipt?.vectorScale?.status === "measured" && receipt?.packageSize?.status === "measured";
  const passed = Boolean(measured && receipt.accounting.interpreterDispositions === rows.length);
  return {
    status: passed ? "passed" : "insufficient",
    evidenceKind: "installed",
    detail: { actionCounts: actions, receiptPath, receiptExists, measured, receipt },
    reason: passed
      ? "NCL-04: current Windows measurement receipt records build identity, timestamp, exact accounting, storage compatibility, vector scale, and package size"
      : "NCL-04: requires a current Windows measurement receipt with exact interpreter accounting plus storage, vector-scale, and package-size measurements",
  };
}

// -----------------------------------------------------------------------------------
// NCL-05 -- native surfaces execute native owners. Requires a live process-tree capture
// per surface call (CLI/MCP/SDK/federation) -- installed-path evidence this worker cannot
// produce. It can structurally confirm the native crates exist and that no allowlisted
// shim file was left in place; that partial evidence is recorded but does not earn "passed".
// -----------------------------------------------------------------------------------
export function NCL_05(context) {
  const root = resolveRoot(context);
  const nativeCrates = [
    "engine/crates/membrane",
    "engine/crates/membrane-client",
    "engine/crates/membrane-federation",
  ];
  const presence = nativeCrates.map((p) => ({ path: p, exists: existsSync(join(root, p)) }));
  const allPresent = presence.every((p) => p.exists);
  const probe = nativeProcessProbe(installedExecutables(installedRoot(context)));
  const observation = receiptProbe(context, "membrane.windows-native-observation.v1");
  const surfaces = observation.value?.surfaces;
  const surfaceOk = Array.isArray(surfaces) && ["cli", "mcp", "sdk", "federation"].every((name) => surfaces.some((s) => s.name === name && s.status === "passed" && Array.isArray(s.processTree) && s.processTree.every((p) => !/^(node|python|python3|sh|bash)(\.exe)?$/i.test(String(p.name || "")))));
  const passed = allPresent && probe.ok && observation.ok && surfaceOk;
  return {
    status: passed ? "passed" : "insufficient",
    evidenceKind: "installed",
    detail: { presence, probe, observation: observation.value || null, surfaceOk },
    reason: passed
      ? "NCL-05: CLI, MCP, SDK, and federation observations ran from installed native executables with no interpreter child process"
      : `NCL-05: native surface proof incomplete: ${observation.reason || (probe.reason || "required surface observation missing")}`,
  };
}

export const NCL_CASES = { NCL_01, NCL_02, NCL_03, NCL_04, NCL_05 };
