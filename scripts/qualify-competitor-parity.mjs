#!/usr/bin/env node
/**
 * Finding-bound parity qualification.  This runner intentionally treats absent
 * platform receipts as open gates; it never projects a source test onto an
 * installed host or another operating system.
 */
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";

const ACCEPTED = [
  ...Array.from({ length: 21 }, (_, index) => `F${String(index + 1).padStart(2, "0")}`),
  ...Array.from({ length: 17 }, (_, index) => `C${String(index + 1).padStart(2, "0")}`),
];
const DELETED = 6;
const PHASES = new Set(["baseline", "source", "mac", "windows", "final"]);
const SOURCE_PROOF = {
  F01: ["membrane"], F02: ["membrane"], F03: ["membrane", "cortex"], F04: ["cortex"], F05: ["adapt"],
  F06: ["engine"], F07: ["engine"], F08: ["membrane", "engine"], F09: ["membrane", "engine"], F10: ["engine"],
  F11: ["engine", "cortex"], F12: ["membrane", "engine", "sync"], F13: ["m0"], F14: ["ccx"], F15: ["sentinel"], F16: ["sentinel"], F17: ["sentinel"], F18: ["engine"],
  F19: ["engine"], F20: ["engine"], F21: ["membrane"], C01: ["engine"], C02: ["cortex"], C03: ["benchmark"],
  C04: ["engine", "cortex"], C05: ["engine"], C06: ["engine", "cortex"], C07: ["engine"], C08: ["membrane"],
  C09: ["membrane"], C10: ["capability"], C11: ["rollout"], C12: ["membrane"], C13: ["membrane", "engine"], C14: ["morph"],
  C15: ["benchmark"], C16: ["sync"], C17: ["membrane"],
};
const args = process.argv.slice(2);
const membraneRoot = resolve(dirname(new URL(import.meta.url).pathname), "..");
const workspaceRoot = resolve(membraneRoot, "..");
const valueFor = (flag) => {
  const index = args.indexOf(flag);
  return index < 0 ? undefined : args[index + 1];
};
const has = (flag) => args.includes(flag);
const phase = valueFor("--phase") || "baseline";
const evidenceRoot = resolve(valueFor("--evidence-root") || join(workspaceRoot, "tasks", "evidence", "membrane-competitor-parity-completion"));
const findingsPath = resolve(valueFor("--findings") || join(workspaceRoot, "tasks", "dispatches", "2026-08-02", "membrane-competitor-parity-completion.findings.md"));
const requestedRange = valueFor("--finding-range");

function fail(message) { process.stderr.write(`${message}\n`); process.exitCode = 1; }
function sha256(value) { return `sha256:${createHash("sha256").update(value).digest("hex")}`; }
function fileHash(path) { return existsSync(path) ? sha256(readFileSync(path)) : null; }
function git(root, command) {
  try { return execFileSync("git", command, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim(); }
  catch { return null; }
}
function atomicJson(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  const temp = `${path}.${process.pid}.tmp`;
  writeFileSync(temp, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  renameSync(temp, path);
}
function parseFindings(text) {
  const rows = [...text.matchAll(/^\|\s*((?:F\d{2})|(?:C\d{2}))\s*\|\s*([^|]+)\|/gm)];
  const ids = rows.map((row) => row[1]);
  const deleted = [...text.matchAll(/^- DELETE:/gm)].length;
  return { ids, deleted };
}
function expandRange(range) {
  if (!range) return ACCEPTED;
  const selected = new Set();
  for (const part of range.split(",")) {
    const [start, end] = part.trim().split(":");
    if (!end) { selected.add(start); continue; }
    const prefix = start[0];
    const first = Number(start.slice(1));
    const last = Number(end.replace(/^[A-Z]/, ""));
    for (let number = first; number <= last; number += 1) selected.add(`${prefix}${String(number).padStart(2, "0")}`);
  }
  return ACCEPTED.filter((id) => selected.has(id));
}
function fingerprint() {
  const root = workspaceRoot;
  return {
    root: git(root, ["rev-parse", "HEAD"]),
    membrane: git(membraneRoot, ["rev-parse", "HEAD"]),
    runner: fileHash(new URL(import.meta.url).pathname),
    findings: fileHash(findingsPath),
  };
}
function checkpointPath() { return join(evidenceRoot, "checkpoint.json"); }
function writeCheckpoint(result) { atomicJson(checkpointPath(), { schema: "membrane.parity-checkpoint.v1", updated_at: new Date().toISOString(), fingerprint: result.fingerprint, phase, result: basename(result.evidence_path) }); }
function platformReceipt(platform) { return join(evidenceRoot, `${platform}.json`); }
function currentEvidence() {
  const source = join(evidenceRoot, "source.json");
  const mac = platformReceipt("mac");
  const windows = platformReceipt("windows");
  return { source: existsSync(source) ? JSON.parse(readFileSync(source, "utf8")) : null, mac: existsSync(mac) ? JSON.parse(readFileSync(mac, "utf8")) : null, windows: existsSync(windows) ? JSON.parse(readFileSync(windows, "utf8")) : null };
}
function runCommand(command, commandArgs, cwd, timeout = 20 * 60_000) {
  const execution = spawnSync(command, commandArgs, {
    cwd, encoding: "utf8", timeout, windowsHide: true,
    env: { ...process.env, CI: "1" },
  });
  const output = `${execution.stdout || ""}${execution.stderr || ""}`.slice(-8_000);
  return {
    command: [command, ...commandArgs].join(" "), cwd, status: execution.status,
    signal: execution.signal || null, timed_out: execution.error?.code === "ETIMEDOUT",
    output_sha256: sha256(output), output_tail: output,
  };
}
function sourceSuites(ids) {
  const membraneRoot = resolve(dirname(new URL(import.meta.url).pathname), "..");
  const root = resolve(membraneRoot, "..");
  const suites = [];
  const add = (key, command, commandArgs, cwd) => {
    if (!suites.some((suite) => suite.key === key)) suites.push({ key, command, commandArgs, cwd });
  };
  add("membrane", "pnpm", ["--dir", membraneRoot, "test"] , root);
  if (ids.includes("F13")) add("m0", "node", [join(membraneRoot, "scripts", "run-m0-baseline-current.mjs")], root);
  if (ids.includes("F14")) add("ccx", "node", [join(membraneRoot, "scripts", "run-ccx-live.mjs")], root);
  if (ids.some((id) => ["C03", "C15"].includes(id))) add("benchmark", "node", ["--test", join(membraneRoot, "mcp", "e2e-benchmark.test.mjs")], root);
  if (ids.includes("C10")) add("capability", "node", ["--test", join(membraneRoot, "mcp", "capability-inventory.test.mjs")], root);
  if (ids.includes("C11")) add("rollout", "node", ["--test", join(membraneRoot, "mcp", "rollout.test.mjs")], root);
  if (ids.includes("C16")) add("sync", join(root, ".venv-tools", "bin", "python"), ["-m", "unittest", "tools.pipelines.memory.test_sync"], root);
  if (ids.includes("C14")) add("morph", "node", [join(membraneRoot, "scripts", "run-morph-installed-current.mjs")], root);
  if (ids.some((id) => /^F1[5-8]$/.test(id))) add("sentinel", "pnpm", ["--dir", join(root, "tether"), "test"], root);
  if (ids.some((id) => ["F07", "F08", "F09", "F10", "F11", "F18", "F19", "F20", "C01", "C04", "C05", "C06", "C07", "C13"].includes(id))) add("engine", "cargo", ["check", "--manifest-path", join(membraneRoot, "engine", "Cargo.toml"), "--workspace", "--all-targets"], root);
  if (ids.some((id) => ["F03", "F04", "F11", "C02", "C04", "C05", "C06", "C07"].includes(id))) add("cortex", "pnpm", ["--dir", join(root, "cortex"), "test:all"], root);
  if (ids.some((id) => ["F05", "F12", "C14"].includes(id))) add("adapt", join(root, ".venv-tools", "bin", "python"), ["-m", "pytest", join(root, "adapt")], root);
  return suites;
}

if (!PHASES.has(phase)) fail(`unsupported phase: ${phase}`);
if (!existsSync(findingsPath)) fail(`findings missing: ${findingsPath}`);
const ledger = parseFindings(readFileSync(findingsPath, "utf8"));
const ledgerIds = new Set(ledger.ids);
const unmapped = ACCEPTED.filter((id) => !ledgerIds.has(id));
const selected = expandRange(requestedRange);
const base = {
  schema: "membrane.competitor-parity.v1",
  phase,
  generated_at: new Date().toISOString(),
  dry_run: has("--dry-run"),
  finding_counts: { accepted: ledger.ids.length, deleted: ledger.deleted, selected: selected.length, unmapped: unmapped.length },
  fingerprint: fingerprint(),
  flags: Object.fromEntries(["--require-live", "--require-zero-open", "--require-restart", "--require-rollback", "--require-encrypted-sync"].map((flag) => [flag.slice(2), has(flag)])),
};
if (ledger.ids.length !== ACCEPTED.length || ledger.deleted !== DELETED || unmapped.length) fail(`invalid findings ledger: accepted=${ledger.ids.length} deleted=${ledger.deleted} unmapped=${unmapped.join(",")}`);

if (has("--dry-run")) {
  const result = { ...base, mutations: 0, max_jobs: 52, phases: [...PHASES], status: "dry_run" };
  process.stdout.write(`${JSON.stringify(result)}\n`);
  process.exit(process.exitCode || 0);
}

let result;
if (phase === "baseline") {
  result = { ...base, status: "characterized", finding_results: selected.map((id) => ({ id, status: "baseline_open", reason: "current implementation has not passed a finding-bound current gate" })), open: selected };
} else if (phase === "source") {
  const checks = sourceSuites(selected).map(({ key, command, commandArgs, cwd }) => ({ key, ...runCommand(command, commandArgs, cwd) }));
  const byKey = new Map(checks.map((check) => [check.key, check]));
  const findingResults = selected.map((id) => {
    const required = SOURCE_PROOF[id];
    if (!required) return { id, status: "open", reason: "missing_finding_bound_source_check", checks: [] };
    const failed = required.filter((key) => byKey.get(key)?.status !== 0);
    return { id, status: failed.length ? "open" : "source_passed", reason: failed.length ? "finding_bound_source_check_failed" : "finding_bound_source_checks_passed", checks: required };
  });
  const open = findingResults.filter(({ status }) => status !== "source_passed").map(({ id }) => id);
  result = {
    ...base,
    status: open.length ? "source_failed" : "source_passed",
    checks,
    finding_results: findingResults,
    open,
  };
} else if (phase === "mac") {
  const source = currentEvidence().source;
  const installed = runCommand(join(workspaceRoot, "tools", "bin", "memright"), ["build-info"], workspaceRoot, 10_000);
  const health = runCommand("curl", ["--fail", "--silent", "--show-error", "--max-time", "5", "http://127.0.0.1:47851/health"], workspaceRoot, 10_000);
  const service = runCommand("launchctl", ["print", `gui/${process.getuid()}/com.adrian.memright-serve`], workspaceRoot, 10_000);
  const db = process.env.MEMRIGHT_DB || join(workspaceRoot, "tools", ".cache", "memory", "memright-engine.db");
  const hostEvents = runCommand("sqlite3", ["-json", db, "SELECT client, COUNT(*) AS events, MAX(ts) AS latest_ts FROM context_event_log WHERE client IN ('claude_code', 'codex', 'ccx') GROUP BY client ORDER BY client"], workspaceRoot, 10_000);
  let hostCoverage = [];
  let identity = null;
  let healthJson = null;
  try {
    identity = JSON.parse(installed.output_tail);
    healthJson = JSON.parse(health.output_tail);
    hostCoverage = JSON.parse(hostEvents.output_tail);
  } catch { /* receipt remains open */ }
  const expectedCommit = git(membraneRoot, ["rev-parse", "HEAD"]);
  const engineCurrent = identity?.memright_source_commit
    ? spawnSync("git", ["diff", "--quiet", identity.memright_source_commit, "HEAD", "--", "engine"], { cwd: membraneRoot }).status === 0
    : false;
  const coveredClients = new Set(hostCoverage.filter((row) => Number(row.events) > 0).map((row) => row.client));
  const identityMatch = source?.status === "source_passed"
    && (identity?.memright_source_commit === expectedCommit || engineCurrent)
    && healthJson?.releaseGeneration === identity?.release_generation
    && /state = running/.test(service.output_tail)
    && ["claude_code", "codex", "ccx"].every((client) => coveredClients.has(client));
  result = {
    ...base, platform: "mac", checks: [installed, health, service, hostEvents], identity, engine_source_current: engineCurrent, host_coverage: hostCoverage,
    status: identityMatch ? "mac_host_passed" : "mac_host_failed",
    finding_results: selected.map((id) => ({ id, status: identityMatch ? "mac_host_passed" : "open", reason: identityMatch ? "installed identity, service, & Claude/Codex/ccx event receipts match" : "installed identity, service, or host receipt mismatch" })),
    open: identityMatch ? [] : selected,
  };
} else if (phase === "windows") {
  result = { ...base, status: "blocked_missing_installed_receipt", platform: phase, finding_results: selected.map((id) => ({ id, status: "open", reason: "no installed windows receipt" })), open: selected };
} else {
  const evidence = currentEvidence();
  const platformOpen = ["source", "mac", "windows"].filter((name) => !evidence[name]);
  result = { ...base, status: platformOpen.length ? "blocked_missing_phase_receipts" : "blocked_unvalidated_receipts", phase_receipts: Object.fromEntries(Object.entries(evidence).map(([name, receipt]) => [name, receipt?.status || "missing"])), open: ACCEPTED };
}
const name = phase === "final" ? "final-verification.json" : `${phase}.json`;
result.evidence_path = join(evidenceRoot, name);
atomicJson(result.evidence_path, result);
writeCheckpoint(result);
process.stdout.write(`${JSON.stringify({ phase, status: result.status, accepted: ledger.ids.length, deleted: ledger.deleted, open: result.open.length, evidence: result.evidence_path })}\n`);
if (result.open.length) process.exitCode = 1;
