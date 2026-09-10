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
// builds, installs, or packaging. It MAY run already-installed, already-signed executables
// read-only (e.g. `membrane.exe cli doctor`, `--version`) to capture real process trees --
// that is not a build/install/package action -- and does so for NCL-03/NCL-04/NCL-05.
//
// NCL-03 needs an installed, interpreter-stripped binary. The installed 0.1.24 product
// root still bundles an interpreter (runtime/blueprint/lib/node.exe, a sibling of
// versions/ outside the stable "current" tree) -- this module's payload scan finds it
// truthfully, so NCL-03 reports "insufficient" for real, not fabricated: the gate needs a
// rebuilt/repackaged installed product with that interpreter actually removed, which is
// outside this worker's no-build/no-install/no-package allowlist.
//
// NCL-04 reads a real measurement receipt (audit/qualification/windows-r5/receipts/
// native-cleanup__waveB.json by default, overridable via context.receiptPath or
// MEMBRANE_NATIVE_CLEANUP_RECEIPT) containing on-this-machine storage/vector-scale/
// package-size measurements and interpreter-disposition accounting reconciled against
// D:/Claude/review/windows-r5/interpreter-dispositions.json, and scores pass/fail for real.
//
// NCL-05 reads a real process-tree observation receipt (audit/qualification/windows-r5/
// receipts/native-windows-observation.json by default, overridable via
// context.observationReceiptPath or MEMBRANE_NATIVE_OBSERVATION) captured by the installed
// membrane.exe. CLI diagnostics, stdio MCP JSON-RPC, identity-fenced explicit SDK binding,
// and native Pull federation are all direct installed modes; no standalone helper process
// or interpreter is invented for SDK/federation.
//
// NCL-01 and NCL-02 are checkable from the live tree/registry alone and are scored
// pass/fail (NCL-01) or a typed cross-lane "insufficient" (NCL-02) for real.

import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, statSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, "../../../");

const INTERPRETED_EXTENSIONS = [".py", ".mjs", ".cjs", ".js", ".ts", ".sh", ".ps1", ".cmd", ".bat"];
const NCL05_SURFACES = Object.freeze(["cli", "mcp", "sdk", "federation"]);
const FORBIDDEN_PROCESS = /^(node|node_repl|python|python3|sh|bash)(\.exe)?$/iu;
const NATIVE_ONLY_SEAL_SCHEMA = "membrane.native-only-seal.v1";
const QUALIFICATION_SCHEMA = "membrane.windows-installed-qualification.v1";
const MAX_EVIDENCE_AGE_MS = 24 * 60 * 60 * 1000;
const REQUIRED_LIFECYCLE = [
  "install", "startup", "hubHealth", "tray", "popup", "renderer", "mcp17",
  "nativeHostCutover", "blueprintHubHosted", "blueprintHubOffOneShot", "downgrade",
  "upgrade", "stateContinuity", "uninstall", "residue", "nativeOnlyProcessTree",
  "runtimeInventory", "currentRootActivation", "doctor", "hubOffManualBlueprint",
  "residentFileChangeRefresh", "zeroInterpreterProcessTree",
];

function installedRoot(context) {
  const configured = (context && context.installedRoot) || process.env.MEMBRANE_INSTALLED_ROOT ||
    process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT ||
    join(process.env.LOCALAPPDATA || "", "Orthic Labs", "Membrane");
  // Qualification commonly supplies stable `current` as its installed root for
  // executable probes. NCL-03 payload proof covers product-root residue too, so
  // normalize that input to its parent before scanning.
  return configured.replace(/[\\/]current$/i, "");
}

function installedCurrentRoot(context) {
  return resolve(join(installedRoot(context), "current"));
}

function comparablePath(value) {
  if (typeof value !== "string" || value.trim().length === 0) return "";
  return value.replace(/^\\\\\?\\/u, "").replace(/[\\/]+$/u, "").replace(/\\/gu, "/").toLowerCase();
}

function sha256File(path) {
  try { return createHash("sha256").update(readFileSync(path)).digest("hex"); } catch { return null; }
}

function installedIdentity(context) {
  if (process.platform !== "win32") return { ok: false, reason: "native Windows installed identity requires win32" };
  const root = installedCurrentRoot(context);
  const executable = join(root, "membrane.exe");
  const releasePath = join(root, "release.json");
  if (!existsSync(executable) || !existsSync(releasePath)) return { ok: false, reason: "installed current membrane.exe or release.json is missing" };
  let release;
  try { release = JSON.parse(readFileSync(releasePath, "utf8")); } catch (error) { return { ok: false, reason: `installed release.json is invalid: ${error.message}` }; }
  const releaseGeneration = String(release.releaseGeneration || "");
  if (release.product !== "membrane" || release.os !== "windows" || release.arch !== "x64" || !/^sha256:[0-9a-f]{64}$/iu.test(releaseGeneration)) {
    return { ok: false, reason: "installed release.json lacks canonical Windows identity" };
  }
  const expectedHash = String(release.files?.["membrane.exe"] || "").replace(/^sha256:/iu, "");
  const executableSha256 = sha256File(executable);
  if (!/^[0-9a-f]{64}$/iu.test(expectedHash) || executableSha256?.toLowerCase() !== expectedHash.toLowerCase()) {
    return { ok: false, reason: "installed membrane.exe hash does not match release.json" };
  }
  let build;
  try {
    const raw = execFileSync(executable, ["cli", "build-info"], { cwd: root, encoding: "utf8", timeout: 15000, windowsHide: true, env: { ...process.env, PATH: "C:\\Windows\\System32;C:\\Windows" } });
    build = JSON.parse(raw);
  } catch (error) { return { ok: false, reason: `installed build-info failed: ${error.message}` }; }
  const buildGeneration = String(build.release_generation || build.releaseGeneration || "");
  if (build.target !== "x86_64-pc-windows-msvc" || buildGeneration !== releaseGeneration) return { ok: false, reason: "installed build-info does not match release identity" };
  return {
    ok: true, value: {
      root, executable, releaseGeneration, executableSha256: executableSha256.toLowerCase(),
      manifestSha256: sha256File(releasePath), mtimeMs: statSync(executable).mtimeMs,
    },
  };
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
  // run.mjs's runOneRegistryCase never populates a per-case receipt-input field on
  // context (its "evidencePath" is the runner's own --evidence OUTPUT file, not an
  // input observation receipt), so context.evidencePath is intentionally not treated
  // as the observation source here. MEMBRANE_NATIVE_OBSERVATION lets a caller point at
  // an ad-hoc capture; absent that, fall back to the sanctioned windows-r5 receipt path
  // under this repository's audit/qualification/windows-r5/receipts/.
  const root = resolveRoot(context);
  const path = (context && context.observationReceiptPath) || process.env.MEMBRANE_NATIVE_OBSERVATION ||
    join(root, "audit", "qualification", "windows-r5", "receipts", "native-windows-observation.json");
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

function digestFile(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function samePath(left, right) {
  return resolve(left).replace(/[\\/]$/u, "").toLowerCase() === resolve(right).replace(/[\\/]$/u, "").toLowerCase();
}

// Validate the seal's complete qualification pointer, then compare every
// recorded current-root file (especially native binaries) with the live
// installed bytes. A seal over a valid but older receipt is not current proof.
export function validateNativeOnlySeal(seal, qualificationPath, installedRoot) {
  const fail = (reason, detail = {}) => ({ ok: false, reason, detail });
  if (!seal || seal.schema !== NATIVE_ONLY_SEAL_SCHEMA || seal.status !== "sealed" || seal.target !== "windows-x86_64") {
    return fail("native-only seal schema/status/target is invalid");
  }
  if (!/^[a-f0-9]{64}$/u.test(String(seal.artifact_sha256 || ""))) return fail("native-only seal artifact digest is invalid");
  const sealTime = Date.parse(String(seal.generatedAt || ""));
  if (!Number.isFinite(sealTime) || Date.now() - sealTime > MAX_EVIDENCE_AGE_MS || sealTime > Date.now() + 5 * 60 * 1000) {
    return fail("native-only seal evidence is stale or has invalid generatedAt");
  }
  const qualificationInput = seal.inputs?.installedQualification;
  if (!qualificationInput || typeof qualificationInput.path !== "string" || typeof qualificationPath !== "string" ||
    !samePath(qualificationInput.path, qualificationPath) || !/^[a-f0-9]{64}$/u.test(String(qualificationInput.sha256 || ""))) {
    return fail("native-only seal does not point to its installed qualification input");
  }
  if (!existsSync(qualificationInput.path) || digestFile(qualificationInput.path) !== qualificationInput.sha256) {
    return fail("native-only seal installed qualification input hash is stale");
  }
  let qualification;
  try { qualification = JSON.parse(readFileSync(qualificationInput.path, "utf8")); }
  catch (error) { return fail(`installed qualification cannot be read: ${error.message}`); }
  if (qualification.schema !== QUALIFICATION_SCHEMA || qualification.platform !== "windows-x86_64" ||
    !["installed-local", "internal-unsigned"].includes(qualification.profile) ||
    (qualification.profile === "internal-unsigned" && qualification.certification !== "unsigned-functional") ||
    (qualification.profile === "installed-local" && qualification.certification === "unsigned-functional")) {
    return fail("installed qualification identity is invalid");
  }
  const qualificationTime = Date.parse(String(qualification.generatedAt || ""));
  if (!Number.isFinite(qualificationTime) || Date.now() - qualificationTime > MAX_EVIDENCE_AGE_MS || qualificationTime > Date.now() + 5 * 60 * 1000) {
    return fail("installed qualification evidence is stale or has invalid generatedAt");
  }
  const artifactHash = qualification.artifact?.sha256;
  if (artifactHash !== seal.artifact_sha256 || qualification.installedCurrent?.artifactSha256 !== artifactHash) {
    return fail("installed qualification artifact is not bound to native-only seal");
  }
  const currentRoot = join(installedRoot, "current");
  const recordedRoot = qualification.installedCurrent?.root;
  if (!recordedRoot || !samePath(recordedRoot, currentRoot)) return fail("installed qualification current root differs from live current root");
  const files = qualification.installedCurrent?.files;
  if (!Array.isArray(files) || files.length === 0) return fail("installed qualification has no current-root file manifest");
  const seen = new Set();
  let binaryCount = 0;
  for (const [index, entry] of files.entries()) {
    const relative = String(entry?.path || "").replaceAll("\\", "/");
    if (!relative || relative.startsWith("/") || relative === ".." || relative.includes("../") || seen.has(relative)) {
      return fail(`installed qualification file manifest path is invalid at index ${index}`);
    }
    seen.add(relative);
    const expected = String(entry?.sha256 || "");
    if (!/^[a-f0-9]{64}$/u.test(expected)) return fail(`installed qualification file digest is invalid at index ${index}`);
    const actualPath = join(currentRoot, relative);
    if (!existsSync(actualPath) || !lstatSync(actualPath).isFile() || lstatSync(actualPath).isSymbolicLink()) return fail(`installed qualification file is absent or not regular: ${relative}`);
    if (digestFile(actualPath) !== expected) return fail(`installed qualification file digest mismatch: ${relative}`);
    if (/\.exe$/iu.test(relative)) binaryCount += 1;
  }
  if (binaryCount === 0 || seal.installedCurrent?.binaryCount !== binaryCount || !samePath(seal.installedCurrent.root, currentRoot) ||
    seal.qualificationProfile !== qualification.profile) {
    return fail("native-only seal does not bind exact installed executable manifest");
  }
  for (const field of REQUIRED_LIFECYCLE) {
    const lifecycleValue = String(qualification.lifecycle?.[field]).toLowerCase();
    const repairOnly = qualification.downgradeContract === "first-stable-layout-repair-v1";
    if (lifecycleValue !== "pass" && !(qualification.profile === "internal-unsigned" && field === "downgrade" && repairOnly && lifecycleValue === "not_applicable")) {
      return fail(`installed qualification lifecycle.${field} is not pass`);
    }
  }
  const observations = qualification.runtime?.lifecycleObservations;
  if (!Array.isArray(observations) || observations.length === 0 || observations.some((entry) => entry?.observed !== true)) {
    return fail("installed qualification lifecycle observations are missing or not observed");
  }
  return { ok: true, qualification, binaryCount, currentRoot };
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
export function NCL_02(context = {}) {
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
  const pending = rows.filter((r) => r.action === "delete-after-parity");
  const committed = rows.filter((r) => r.action === "commit-deletion");
  // commit-deletion is a post-parity terminal disposition. NCL-02's acceptance
  // quantifies only rows still marked delete-after-parity; never re-open or
  // reinterpret committed rows as pending clusters.
  const candidates = pending;
  const ledgerPath = context.clusterLedgerPath || process.env.MEMBRANE_NCL02_CLUSTER_LEDGER || join(root, "audit", "qualification", "windows-r5", "ncl-02", "clusters.json");
  let ledger = null;
  if (existsSync(ledgerPath)) { try { const value = JSON.parse(readFileSync(ledgerPath, "utf8")); ledger = Array.isArray(value) ? value : value.clusters; } catch {} }
  const i1Path = context.i1EvidencePath || process.env.MEMBRANE_NCL02_I1_EVIDENCE ||
    join(root, "audit", "qualification", "windows-r5", "ncl-02", "i1-evidence.json");
  let i1 = null;
  if (i1Path && existsSync(i1Path)) { try { i1 = JSON.parse(readFileSync(i1Path, "utf8")); } catch {} }
  const currentRevision = context.sourceRevision || process.env.MEMBRANE_QUALIFICATION_SOURCE_REVISION || (() => { try { return execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim(); } catch { return null; } })();
  const hashPattern = /^(?:sha256:)?[0-9a-f]{64}$/iu;
  const filesOf = (cluster) => (cluster?.legacyFiles ?? cluster?.files ?? cluster?.filePaths ?? cluster?.paths ?? [])
    .map((entry) => typeof entry === "string" ? entry : entry?.path ?? entry?.legacyPath ?? entry?.legacyFile).filter(Boolean)
    .map((entry) => entry.replace(/\\/gu, "/"));
  const namesOf = (cluster) => {
    const values = [];
    const collect = (value) => { if (typeof value === "string") values.push(value.trim()); else if (Array.isArray(value)) value.forEach(collect); else if (value && typeof value === "object") Object.values(value).forEach(collect); };
    collect(cluster?.portedTestNames);
    return [...new Set(values.filter(Boolean))];
  };
  const clusterFor = new Map();
  for (const cluster of Array.isArray(ledger) ? ledger : []) for (const file of filesOf(cluster)) clusterFor.set(file, clusterFor.has(file) ? null : cluster);
  const failures = [];
  if (!Array.isArray(ledger)) failures.push("cluster ledger missing or malformed");
  // An empty pending set is an explicit, non-vacuous terminal result: inventory
  // has no clusters awaiting the parity-before-delete gate. Ledger/I1 inputs are
  // intentionally required only when at least one pending row exists.
  if (candidates.length === 0) {
    const detail = { inventoryPath: absOverride, ledgerPath, i1EvidencePath: i1Path || null, candidateCount: 0, pendingCount: 0, committedCount: committed.length, checked: [], failures: [] };
    return { status: "passed", evidenceKind: "source", detail, reason: "NCL-02: interpreter-dispositions.json contains no delete-after-parity rows; no pending parity cluster remains (commit-deletion rows are terminal dispositions)" };
  }
  if (!i1) failures.push("fresh I1 evidence receipt missing");
  const checked = [];
  for (const row of candidates) {
    const key = String(row.path || "").replace(/\\/gu, "/");
    const cluster = clusterFor.get(key);
    if (!cluster) { failures.push(`${key || "<missing path>"} has no unique cluster-ledger mapping`); continue; }
    const names = namesOf(cluster);
    if (names.length === 0) failures.push(`${key}: no named non-vacuous ported assertion`);
    const destination = cluster.nativeDestinationPaths ?? cluster.nativeDestinationPath ?? cluster.nativeDestination;
    const destinations = Array.isArray(destination) ? destination : [destination];
    const destinationExists = destinations.some((value) => typeof value === "string" && /[/\\]/u.test(value) && !/[,:]/u.test(value) && existsSync(resolve(root, value)));
    if (!destinationExists) failures.push(`${key}: native destination is missing`);
    const cutover = cluster.callerCutover === true || cluster.callerCutover?.cutover === true || cluster.callerCutover?.status === "passed" || cluster.callerCutover?.verdict === "cutover";
    if (!cutover) failures.push(`${key}: caller cutover is not explicitly proven`);
    const evidence = cluster.i1Evidence ?? cluster.i1 ?? cluster.integrationEvidence ?? {};
    const declaredHash = cluster.i1EvidenceHash ?? cluster.i1TestEvidenceHash ?? cluster.nativeTestEvidenceHash ?? evidence.sha256 ?? evidence.hash;
    const evidencePath = cluster.i1EvidencePath ?? cluster.i1TestEvidencePath ?? evidence.path ?? evidence.evidencePath;
    const revision = cluster.sourceRevision ?? evidence.sourceRevision ?? i1?.sourceRevision ?? i1?.sourceRevisionAtStart;
    const absoluteEvidencePath = evidencePath ? resolve(dirname(i1Path || root), evidencePath) : null;
    if (!hashPattern.test(String(declaredHash || "")) || !absoluteEvidencePath || !existsSync(absoluteEvidencePath)) failures.push(`${key}: missing I1 evidence hash/path`);
    else {
      const actual = createHash("sha256").update(readFileSync(absoluteEvidencePath)).digest("hex");
      if (actual !== String(declaredHash).replace(/^sha256:/iu, "").toLowerCase()) failures.push(`${key}: I1 evidence hash does not match bytes`);
      if (!/^[0-9a-f]{40}$/iu.test(String(revision || "")) || (currentRevision && String(revision).toLowerCase() !== String(currentRevision).toLowerCase())) failures.push(`${key}: I1 evidence source revision is stale or missing`);
    }
    checked.push({ path: key, cluster: cluster.cluster ?? cluster.id ?? cluster.name ?? null, action: row.action, testNames: names });
  }
  const detail = { inventoryPath: absOverride, ledgerPath, i1EvidencePath: i1Path || null, candidateCount: candidates.length, pendingCount: pending.length, committedCount: committed.length, checked, failures };
  return failures.length === 0
    ? { status: "passed", evidenceKind: "source", detail, reason: "NCL-02: every deletion row has unique cluster mapping, named non-vacuous assertion, native destination, caller cutover, fresh I1 evidence, and deletion-state proof" }
    : { status: "insufficient", evidenceKind: "source", detail, reason: `NCL-02: ${failures.join("; ")}` };
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
  // The registry acceptance requires an issued native-only seal in addition to
  // process/payload observations.  Do not let a clean smoke probe masquerade as
  // a sealed release when the evidence chain is absent or malformed.
  const sealPath = (context && context.nativeOnlySealPath) ||
    process.env.MEMBRANE_NATIVE_ONLY_SEAL ||
    join(root, "audit", "qualification", "windows-r5", "native-only-seal.json");
  let seal = null;
  let sealError = null;
  if (existsSync(sealPath)) {
    try { seal = JSON.parse(readFileSync(sealPath, "utf8")); }
    catch (error) { sealError = `cannot read seal: ${error.message}`; }
  } else {
    sealError = `missing native-only seal: ${sealPath}`;
  }
  const sealValidation = seal ? validateNativeOnlySeal(seal, seal?.inputs?.installedQualification?.path, rootPath) : { ok: false, reason: sealError };
  const sealOk = sealValidation.ok;
  if (!sealOk && !sealError) sealError = sealValidation.reason;
  const passed = probe.ok && payloadOk && sealOk;
  return {
    status: passed ? "passed" : "insufficient",
    evidenceKind: "installed",
    detail: { root, installedRoot: rootPath, executables, payloadInterpreters, probe, sealPath, sealOk, sealError, sealValidation, seal },
    reason: passed
      ? "NCL-03: installed native executables ran with restricted PATH, live process snapshots contained no interpreter children, payload contained no interpreter executable, and native-only seal is present"
      : `NCL-03: installed native probe incomplete: ${probe.reason || (probe.forbidden?.length ? "interpreter child observed" : "process probe failed")}${payloadOk ? "" : `; bundled interpreter executable(s): ${payloadInterpreters.join(", ")}`}${sealOk ? "" : `; ${sealError || "native-only seal is invalid"}`}`,
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
  // The prior default pointed at a session-scoped scratchpad path
  // (D:/scratchpad/lanes/receipts/...) that no runner-supplied context ever
  // populates and that is not a sanctioned evidence location. The sanctioned
  // default lives under this repository's audit/qualification/windows-r5/
  // receipts/ directory, alongside the other windows-r5 acceptance evidence.
  const receiptPath = (context && context.receiptPath) ||
    process.env.MEMBRANE_NATIVE_CLEANUP_RECEIPT ||
    join(root, "audit", "qualification", "windows-r5", "receipts", "native-cleanup__waveB.json");
  const receiptExists = existsSync(receiptPath);
  let receipt = null;
  if (receiptExists) { try { receipt = JSON.parse(readFileSync(receiptPath, "utf8")); } catch {} }
  const generatedAtMs = receipt?.generatedAt ? Date.parse(receipt.generatedAt) : Number.NaN;
  const receiptFresh = Number.isFinite(generatedAtMs) && generatedAtMs <= Date.now() + 5 * 60 * 1000 &&
    Date.now() - generatedAtMs <= 48 * 60 * 60 * 1000;
  const expectedActions = Object.fromEntries(Object.keys(actions).map((key) => [key, actions[key]]));
  const accountingExact = receipt?.accounting?.interpreterDispositions === rows.length &&
    receipt.accounting.interpretedFiles === rows.length &&
    JSON.stringify(receipt.accounting.actionCounts) === JSON.stringify(expectedActions);
  const configuredRoot = context?.installedRoot || process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT;
  const installedExe = configuredRoot ? join(installedCurrentRoot(context), "membrane.exe") : null;
  let identityBound = true;
  if (installedExe && existsSync(installedExe)) {
    try {
      const currentSha = execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", `(Get-FileHash -LiteralPath '${installedExe.replace(/'/g, "''")}' -Algorithm SHA256).Hash.ToLowerInvariant()`], { encoding: "utf8", timeout: 15000 }).trim();
      identityBound = String(receipt?.buildIdentity?.membraneSha256 || "").toLowerCase() === currentSha;
    } catch { identityBound = false; }
  }
  const measured = receipt?.schema === "membrane.windows-lifecycle-observation.v1" && receipt?.platform === "windows" &&
    receipt?.buildIdentity?.root && receipt?.buildIdentity?.membraneSha256 && receipt?.generatedAt && receiptFresh &&
    receipt?.accounting && accountingExact && receipt?.storage?.status === "measured" && receipt.storage.compatible === true &&
    receipt.storage.compatibility === "measured-windows-ntfs" && receipt?.vectorScale?.status === "measured" &&
    receipt?.vectorScale?.performance?.status === "measured" && receipt?.packageSize?.status === "measured" && identityBound;
  const passed = Boolean(measured);
  return {
    status: passed ? "passed" : "insufficient",
    evidenceKind: "installed",
    detail: { actionCounts: actions, receiptPath, receiptExists, measured, receiptFresh, accountingExact, identityBound, receipt },
    reason: passed
      ? "NCL-04: current Windows measurement receipt records build identity, timestamp, exact accounting, storage compatibility, vector scale, and package size"
      : "NCL-04: requires a current Windows measurement receipt with exact interpreter accounting plus storage, vector-scale, and package-size measurements",
  };
}

// -----------------------------------------------------------------------------------
// NCL-05 -- native surfaces execute native owners. The Windows observer captures each
// installed membrane.exe mode (CLI, stdio MCP, explicit SDK, and native federation),
// including executable hash, terminal result, and live descendant process tree.
// -----------------------------------------------------------------------------------
function parseJsonText(text) {
  try { return { ok: true, value: JSON.parse(String(text || "")) }; }
  catch (error) { return { ok: false, reason: `invalid JSON output: ${error.message}` }; }
}

function actionList(surface) {
  if (Array.isArray(surface?.actions)) return surface.actions;
  if (surface?.action && typeof surface.action === "object") return [surface.action];
  // Older receipts flattened one action into the surface row. Treat that form as
  // evidence only when it carries all terminal/native fields; never infer success
  // from status/processTree alone.
  return surface?.exitCode !== undefined || surface?.stdout !== undefined ? [surface] : [];
}

function validateSurfaceResponse(name, actions) {
  const fail = (reason) => ({ ok: false, reason });
  if (actions.length === 0) return fail(`${name} has no real installed call action`);
  const final = actions[actions.length - 1];
  if (name === "mcp") {
    const lines = String(final.stdout).trim().split(/\r?\n/u).filter(Boolean);
    let rows;
    try { rows = lines.map((line) => JSON.parse(line)); } catch { return fail("MCP response contains malformed JSON-RPC output"); }
    if (rows.length !== 2 || rows.some((row) => row?.jsonrpc !== "2.0" || row?.error) ||
      rows.map((row) => row.id).join(",") !== "1,2" || rows[0]?.result?.protocolVersion !== "2025-03-26" ||
      !Array.isArray(rows[1]?.result?.tools) || rows[1].result.tools.length === 0) {
      return fail("MCP initialize/tools-list schema, IDs, or typed result is invalid");
    }
    return { ok: true };
  }
  const parsed = parseJsonText(final.stdout);
  if (!parsed.ok) return fail(`${name} ${parsed.reason}`);
  const value = parsed.value;
  if (name === "cli") {
    if (value?.schemaVersion !== "LiveDiagnosticsServiceV1" || value?.surface !== "membrane-live-diagnostics" ||
      value?.audit?.schemaVersion !== "live-diagnostics-audit.v1" || !Array.isArray(value?.endpoints) || value.endpoints.length === 0) {
      return fail("CLI diagnostics response schema/identity is invalid");
    }
    return { ok: true };
  }
  // Native Pull federation is also exposed by the installed `cli pull federate`
  // command.  It has no ExplicitResponseV1 binding wrapper, so validate its
  // unchanged packet/receipt envelope directly while still requiring a real
  // terminal action (checked by validateNcl05Observation).
  if (name === "federation" && actions.length === 1) {
    if (value?.packet && Array.isArray(value.receipts) && value?.transport === "native") return { ok: true };
    return fail("federation response does not contain native packet and receipts");
  }
  const binding = actions[0];
  const bindingJson = parseJsonText(binding.stdout);
  if (!bindingJson.ok || bindingJson.value?.schemaVersion !== 1 || bindingJson.value?.status !== 200 ||
    bindingJson.value?.binding?.schemaVersion !== 1 || bindingJson.value.binding.mode !== "bounded_explicit" ||
    bindingJson.value.binding.nativeOnly !== true || typeof bindingJson.value.binding.installationId !== "string" ||
    typeof bindingJson.value.binding.cortexStoreId !== "string" || typeof bindingJson.value.binding.releaseGeneration !== "string" ||
    typeof bindingJson.value.binding.stableInstallRoot !== "string" || bindingJson.value.binding.protocolVersion !== 1) {
    return fail(`${name} binding response schema/identity is invalid`);
  }
  if (actions.length < 2) return fail(`${name} has no bound operation call after binding`);
  if (value?.schemaVersion !== 1 || typeof value?.status !== "number" || value.status < 200 || value.status >= 300 ||
    value?.binding?.installationId !== bindingJson.value.binding.installationId || value?.binding?.releaseGeneration !== bindingJson.value.binding.releaseGeneration ||
    value?.data === undefined || value?.data?.error !== undefined || value?.data?.kind === "error") {
    return fail(`${name} bound operation returned a non-success or typed error response`);
  }
  if (name === "sdk" && !Array.isArray(value.data)) return fail("SDK list response data is not an array");
  if (name === "federation" && (!value.data || typeof value.data !== "object" || !value.data.packet || !Array.isArray(value.data.receipts))) {
    return fail("federation response does not contain packet and receipts");
  }
  return { ok: true };
}

export function validateNcl05Observation(observation, expectedIdentity, options = {}) {
  const fail = (reason, detail = {}) => ({ ok: false, reason, detail });
  if (!observation || observation.schema !== "membrane.windows-native-observation.v1" || observation.platform !== "windows") {
    return fail("native observation schema/platform is invalid");
  }
  const generatedMs = Date.parse(String(observation.generatedAt || ""));
  const now = Date.now();
  const maxAge = Number.isFinite(options.maxAgeMs) ? options.maxAgeMs : MAX_EVIDENCE_AGE_MS;
  if (!Number.isFinite(generatedMs) || generatedMs > now + 5 * 60 * 1000 || now - generatedMs > maxAge) return fail("native observation is stale or has invalid generatedAt");
  const identity = expectedIdentity?.root ? expectedIdentity : null;
  const recorded = observation.buildIdentity;
  if (!identity || !recorded || !samePath(observation.installedRoot, identity.root) || !samePath(recorded.root, identity.root) ||
    comparablePath(observation.installedRoot) !== comparablePath(recorded.root) ||
    String(recorded.generation || recorded.releaseGeneration || "") !== identity.releaseGeneration ||
    String(recorded.membraneSha256 || "").toLowerCase() !== identity.executableSha256) {
    return fail("native observation installed identity is not an exact current-root match", { expected: identity, recorded });
  }
  if (Array.isArray(observation.payloadInterpreters) && observation.payloadInterpreters.length > 0) return fail("native observation payload contains interpreter executable(s)");
  if (Date.parse(String(observation.generatedAt)) + 5 * 60 * 1000 < Number(identity.mtimeMs || 0)) return fail("native observation predates installed executable identity");
  const surfaces = observation.surfaces;
  if (!Array.isArray(surfaces) || surfaces.length !== NCL05_SURFACES.length || new Set(surfaces.map((surface) => surface?.name)).size !== NCL05_SURFACES.length ||
    surfaces.some((surface) => !NCL05_SURFACES.includes(surface?.name) || surface.status !== "passed")) return fail("native observation surface IDs/status are incomplete");
  const surfaceResults = {};
  for (const name of NCL05_SURFACES) {
    const surface = surfaces.find((item) => item.name === name);
    const actions = actionList(surface);
    if (actions.some((action) => action.status === "failed" || action.terminal !== true || action.nativeEvidence !== true || action.timedOut === true || action.exitCode !== 0 ||
      typeof action.stdout !== "string" || action.stdout.trim().length === 0 || comparablePath(action.executable) !== comparablePath(identity.executable) ||
      String(action.sha256 || "").toLowerCase() !== identity.executableSha256 || !Array.isArray(action.processTree) || action.processTree.length === 0 ||
      !Array.isArray(action.forbiddenChildren) || action.forbiddenChildren.length > 0 || action.processTree.some((process) => FORBIDDEN_PROCESS.test(String(process?.name || "")) || FORBIDDEN_PROCESS.test(String(process?.executable || "").split(/[\\/]/u).pop() || "")))) {
      return fail(`${name} did not produce a successful real installed native call with interpreter-free process tree`);
    }
    if (!actions.some((action) => action.processTree.some((process) => comparablePath(process?.executable) === comparablePath(identity.executable)))) return fail(`${name} process tree does not contain its installed owner`);
    const response = validateSurfaceResponse(name, actions);
    if (!response.ok) return fail(response.reason);
    surfaceResults[name] = { actions: actions.length };
  }
  return { ok: true, detail: { generatedAt: observation.generatedAt, identity, surfaces: surfaceResults } };
}

export function NCL_05(context) {
  const root = resolveRoot(context);
  const nativeCrates = [
    "engine/crates/membrane",
    "engine/crates/membrane-client",
    "engine/crates/membrane-federation",
  ];
  const presence = nativeCrates.map((p) => ({ path: p, exists: existsSync(join(root, p)) }));
  const allPresent = presence.every((p) => p.exists);
  const identity = installedIdentity(context);
  const probe = nativeProcessProbe(installedExecutables(installedRoot(context)));
  const observation = receiptProbe(context, "membrane.windows-native-observation.v1");
  const validation = observation.ok && identity.ok ? validateNcl05Observation(observation.value, identity.value) : { ok: false, reason: observation.reason || identity.reason };
  const passed = allPresent && probe.ok && observation.ok && identity.ok && validation.ok;
  return {
    status: passed ? "passed" : "insufficient",
    evidenceKind: "installed",
    detail: { presence, identity, probe, observation: observation.value || null, validation },
    reason: passed
      ? "NCL-05: CLI, MCP, SDK, and federation observations ran from installed native executables with no interpreter child process"
      : `NCL-05: native surface proof incomplete: ${validation.reason || identity.reason || observation.reason || (probe.reason || "required surface observation missing")}`,
  };
}

export const NCL_CASES = { NCL_01, NCL_02, NCL_03, NCL_04, NCL_05 };
