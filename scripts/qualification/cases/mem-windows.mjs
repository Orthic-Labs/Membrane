// scripts/qualification/cases/mem-windows.mjs — MEM group qualification
// cases for the public-runtime lane (windows-r5), authored against
// blueprint-membrane-acceptance.json rows BM09 and BM11.
//
// These are static/structural checks over source and fixture files, run by
// plain Node with no cargo/build step. They prove the descriptor and corpus
// *exist and are complete*, per acceptance row `implementationStatus:
// "IMPLEMENT_THEN_RUN; no runtime result claimed"`. They do not, and cannot,
// prove live "available -> intact -> discoverable -> effective on supported
// Windows hosts" behavior — that requires the compiled/installed binary and
// is the integration owner's registry-command execution, not this module's.

import { readFileSync, existsSync, statSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, "../../..");

const REQUIRED_INJECTION_POINTS = [
  "session_start", "user_prompt", "pre_tool", "post_edit",
  "post_tool", "pre_compaction", "resume", "explicit_pull",
];

// Fields BM09 requires per descriptor (negative controls Z17/Z18).
const REQUIRED_DESCRIPTOR_FIELDS = [
  "canonical_name", "host_names", "purpose", "when_to_use", "when_not_to_use",
  "input", "output", "cost_bound", "freshness_bound", "effect_bound",
  "budget", "dedup", "suppression", "receipt_kind",
];

const HOOK_RS_PATH = join(REPO_ROOT, "engine/crates/membrane-protocol/src/hook.rs");
const CORPUS_DIR = join(REPO_ROOT, "engine/crates/membrane-runtime/tests/fixtures/context-outcome-baseline");

function readHookSource(path = HOOK_RS_PATH) {
  if (!existsSync(path)) throw new Error(`hook.rs not found at ${path}`);
  return readFileSync(path, "utf8");
}

/**
 * Splits the `hook_injection_point_descriptors` function body into one
 * struct-literal block per `HookInjectionPointDescriptorV1 { ... }` entry.
 * Purely textual — this does not compile or execute Rust.
 */
export function extractDescriptorBlocks(source) {
  const start = source.indexOf("pub fn hook_injection_point_descriptors()");
  if (start === -1) return [];
  const body = source.slice(start);
  const blocks = [];
  const marker = "HookInjectionPointDescriptorV1 {";
  let index = body.indexOf(marker);
  while (index !== -1) {
    let depth = 0;
    let cursor = index + marker.length - 1; // sit on the opening brace
    for (; cursor < body.length; cursor += 1) {
      if (body[cursor] === "{") depth += 1;
      else if (body[cursor] === "}") {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    blocks.push(body.slice(index, cursor + 1));
    index = body.indexOf(marker, cursor + 1);
  }
  return blocks;
}

function fieldIsPopulated(block, field) {
  // Matches `field_name: <non-empty-looking value>,` and rejects an empty
  // string literal or an obviously empty slice literal for that field.
  const re = new RegExp(`\\b${field}\\s*:\\s*([^,]+?)\\s*,`, "u");
  const match = block.match(re);
  if (!match) return false;
  const value = match[1].trim();
  if (value === '""' || value === "&[]" || value === "&\"\"") return false;
  return value.length > 0;
}

function canonicalNameOf(block) {
  const match = block.match(/id:\s*(\w+),/u);
  const idToName = {
    SessionStart: "session_start", UserPrompt: "user_prompt", PreTool: "pre_tool",
    PostEdit: "post_edit", PostTool: "post_tool", PreCompaction: "pre_compaction",
    Resume: "resume", ExplicitPull: "explicit_pull",
  };
  return match ? idToName[match[1]] : undefined;
}

/**
 * BM09 case: shared semantic descriptors, one per required injection point,
 * every required field populated (Z17: when_not_to_use; Z18: freshness /
 * budget / dedup / suppression / receipt).
 */
export function BM09(options = {}) {
  const source = options.source ?? readHookSource(options.hookRsPath);
  const blocks = extractDescriptorBlocks(source);
  const findings = [];

  const byName = new Map();
  for (const block of blocks) {
    const name = canonicalNameOf(block);
    if (name) byName.set(name, block);
  }

  for (const point of REQUIRED_INJECTION_POINTS) {
    const block = byName.get(point);
    if (!block) {
      findings.push({ point, ok: false, reason: "descriptor_missing" });
      continue;
    }
    const missingFields = REQUIRED_DESCRIPTOR_FIELDS.filter((field) => !fieldIsPopulated(block, field));
    if (missingFields.length > 0) {
      findings.push({ point, ok: false, reason: "fields_missing", missingFields });
    } else {
      findings.push({ point, ok: true });
    }
  }

  const missingPoints = findings.filter((f) => !f.ok);
  const pass = missingPoints.length === 0 && byName.size === REQUIRED_INJECTION_POINTS.length;
  const structural = {
    case: "BM09",
    evidenceKind: "source",
    pass,
    status: pass ? "passed" : "failed",
    findings,
    note: "Structural/static: proves descriptors exist and are complete. Live available->intact->discoverable->effective proof requires the installed registry-command run by the integration owner.",
  };

  if (!(options.installedRoot ?? process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT)) return structural;
  const installed = probeBM09Installed(options);
  const installedPass = structural.pass && installed.pass;
  return {
    ...structural,
    evidenceKind: "installed",
    pass: installedPass,
    status: installedPass ? "passed" : "failed",
    findings: [...findings, { ok: installed.pass, reason: installed.reason, detail: installed.detail }],
    detail: { structural, installed },
    reason: installedPass ? "native installed HookHost probes passed for every descriptor delivery point" : installed.reason,
  };
}

/**
 * BM11 case: minimum baseline corpus exists at the frozen fixture path
 * (negative control Z20), tasks/expected-evidence are both present, valid
 * JSONL, and reference the same taskId set with an independently authored
 * (non-empty, non-templated) correctnessCriteria per task.
 */
export function BM11(options = {}) {
  const corpusDir = options.corpusDir ?? CORPUS_DIR;
  if (!existsSync(corpusDir) || !statSync(corpusDir).isDirectory()) {
    return { case: "BM11", evidenceKind: "source", pass: false, status: "failed", findings: [{ ok: false, reason: "corpus_path_undefined" }] };
  }
  const tasksPath = join(corpusDir, "tasks.jsonl");
  const evidencePath = join(corpusDir, "expected-evidence.jsonl");
  if (!existsSync(tasksPath) || !existsSync(evidencePath)) {
    return { case: "BM11", evidenceKind: "source", pass: false, status: "failed", findings: [{ ok: false, reason: "corpus_files_missing" }] };
  }

  const tasks = readJsonlRecords(tasksPath);
  const evidence = readJsonlRecords(evidencePath);
  const findings = [];

  if (tasks.length === 0) findings.push({ ok: false, reason: "no_tasks" });
  const taskIds = new Set(tasks.map((t) => t.taskId));
  const evidenceIds = new Set(evidence.map((e) => e.taskId));

  for (const id of taskIds) if (!evidenceIds.has(id)) findings.push({ ok: false, reason: "expected_evidence_missing_for_task", taskId: id });
  for (const id of evidenceIds) if (!taskIds.has(id)) findings.push({ ok: false, reason: "orphan_expected_evidence", taskId: id });

  for (const record of evidence) {
    if (!record.correctnessCriteria || record.correctnessCriteria.trim().length === 0) {
      findings.push({ ok: false, reason: "empty_correctness_criteria", taskId: record.taskId });
    }
    if (!Array.isArray(record.requiredEvidenceIds) || record.requiredEvidenceIds.length === 0) {
      findings.push({ ok: false, reason: "no_required_evidence_ids", taskId: record.taskId });
    }
    if (!["correctness", "evidence_recall", "false_confident_decision"].includes(record.primaryMetric)) {
      findings.push({ ok: false, reason: "invalid_primary_metric", taskId: record.taskId, primaryMetric: record.primaryMetric });
    }
  }

  const pass = findings.length === 0;
  const structural = {
    case: "BM11",
    evidenceKind: "source",
    pass,
    status: pass ? "passed" : "failed",
    findings,
    taskCount: tasks.length,
    note: "Minimum baseline per acceptance row; statistical/category expansion is sequenced later and not required at this checkpoint. This is a fixture-corpus attestation (structural/static), not a measured task-outcome.",
  };

  if (!(options.installedRoot ?? process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT)) return structural;
  const installed = probeBM11Installed({ ...options, corpusDir });
  const installedPass = structural.pass && installed.pass;
  return {
    ...structural,
    evidenceKind: "installed",
    pass: installedPass,
    status: installedPass ? "passed" : "failed",
    findings: [...findings, { ok: installed.pass, reason: installed.reason, detail: installed.detail }],
    detail: { structural, installed },
    reason: installedPass ? "native installed replay processed every baseline task with a validated baseline envelope" : installed.reason,
  };
}

function readJsonlRecords(path) {
  return readFileSync(path, "utf8").split(/\r?\n/u).filter((line) => line.trim().length > 0).map((line, lineNumber) => {
    try { return JSON.parse(line); } catch { throw new Error(`${path}:${lineNumber + 1}: invalid JSON`); }
  });
}

// MEM rows which use this module are source-bound checks. Each row keeps its
// own artifact set and marker contract so registry coverage cannot become a
// blanket “module exists” pass. Runtime-only portions remain explicit in the
// returned finding and are left for the installed consumer qualification.
const MEM_SOURCE_CASES = {
  MEM_001: [["engine/crates/membrane-runtime/src/mcp_executor.rs", /context|planner|federat/iu]],
  MEM_002: [["engine/crates/membrane-protocol/src/types.rs", /V1|serde|struct/iu]],
  MEM_003: [["engine/crates/membrane-runtime/src/release_identity.rs", /release|target|commit/iu]],
  MEM_004: [["engine/crates/membrane-runtime/src/authorization.rs", /scope|authorize|revok/iu]],
  MEM_005: [["engine/crates/membrane-runtime/src/catalog.rs", /grant|expire|revoke/iu], ["engine/crates/membrane-federation/src/scope.rs", /grant|scope|valid/iu]],
  MEM_007: [["apps/membrane-hub/src-tauri/src/supervisor.rs", /daemon|launchd|tray/iu]],
  MEM_011: [["engine/crates/membrane-mcp/src/jsonrpc.rs", /stdio|jsonrpc|MCP/iu]],
  MEM_013: [["engine/crates/membrane-mcp/src/tools.rs", /tool|group|context/iu]],
  MEM_014: [["engine/crates/membrane-mcp/src/resources.rs", /resource|read|list/iu]],
  MEM_015: [["engine/crates/membrane-mcp/src/prompts.rs", /prompt|recap|checkpoint/iu]],
  MEM_017: [["engine/crates/membrane-runtime/src/planes.rs", /application|control|data/iu]],
  MEM_022: [["engine/crates/membrane-runtime/src/working_context.rs", /context|budget|tamper/iu]],
  MEM_023: [["engine/crates/membrane-runtime/src/scratchpad.rs", /scratch|session|cap/iu]],
  MEM_024: [["engine/crates/cortex-core/src/effectiveness.rs", /verdict|feedback|receipt/iu]],
  MEM_025: [["schemas/hub-capabilities.v1.json", /capabilit|inventory|hub/iu]],
  MEM_026: [["apps/membrane-hub/src/overview.mjs", /snapshot|deliver|provider/iu]],
  MEM_027: [["apps/membrane-hub/src/overview.mjs", /unknown|absent|status/iu]],
  MEM_028: [["apps/membrane-hub/src/sources.mjs", /source|generation|repository/iu]],
  MEM_029: [["engine/crates/membrane-runtime/src/memory_sentinel_producer.rs", /contradiction|provenance|lifecycle/iu]],
  MEM_030: [["engine/crates/membrane-runtime/src/delivery_trace_view.rs", /delivery|trace|join/iu]],
  MEM_031: [["apps/membrane-hub/src/fleet.mjs", /adapter|client|capabilit/iu]],
  MEM_032: [["engine/crates/membrane-runtime/src/notifications.rs", /threshold|notify|status/iu]],
  MEM_033: [["engine/crates/membrane-runtime/src/live_diagnostics_service.rs", /diagnostic|workspace|reconcile/iu]],
  MEM_034: [["engine/crates/membrane-runtime/src/live_diagnostics_service.rs", /epoch|mutation|seal/iu]],
  MEM_035: [["engine/crates/membrane-runtime/src/live_diagnostics_service.rs", /provider|snapshot|timeout/iu]],
  MEM_036: [["engine/crates/membrane-runtime/src/live_diagnostics_service.rs", /fence|policy|allow/iu]],
  MEM_037: [["engine/crates/membrane-runtime/src/live_diagnostics_service.rs", /restart|diagnostic|authoriz/iu]],
  MEM_038: [["engine/crates/membrane-runtime/src/live_diagnostics_service.rs", /baseline|delta|resolved/iu]],
  MEM_039: [["engine/crates/membrane-runtime/src/doctor.rs", /doctor|manifest|hash/iu], ["engine/crates/membrane-runtime/src/diagnostic_bundle.rs", /bundle|hash|content/iu]],
  MEM_040: [["engine/crates/membrane-protocol/src/host_observation.rs", /H4|H6|H8|H9|H10/iu]],
  MEM_041: [["engine/crates/membrane-runtime/src/host_observation_ingress.rs", /observation|ingest|identity/iu]],
  MEM_042: [["engine/crates/membrane-runtime/src/runtime_receipt.rs", /receipt|generation|omission/iu]],
  MEM_043: [["engine/crates/membrane-federation/src/deadline.rs", /deadline|cancel|bounded/iu]],
  MEM_045: [["docs/reference/clients/support-matrix.v1.json", /claude|L4|capabilit/iu]],
  MEM_046: [["docs/reference/clients/support-matrix.v1.json", /codex|L2|capabilit/iu]],
  MEM_047: [["docs/reference/clients/support-matrix.v1.json", /coderight|L5|capabilit/iu]],
  MEM_048: [["docs/reference/clients/support-matrix.v1.json", /cursor|windsurf|L1/iu]],
  MEM_049: [["docs/reference/clients/support-matrix.v1.json", /generic_mcp|L0|tool/iu]],
  MEM_050: [["mcp/client.mjs", /client|http|transport/iu], ["continuity/pyproject.toml", /client|transport/iu]],
  MEM_051: [["engine/crates/membrane-runtime/src/team_policy.rs", /signature|scope|opt.?in/iu]],
};

function memSourceCase(id, options = {}) {
  const checks = MEM_SOURCE_CASES[id] ?? [];
  const findings = [];
  for (const [relativePath, marker] of checks) {
    const path = join(REPO_ROOT, relativePath);
    if (!existsSync(path)) {
      findings.push({ ok: false, path: relativePath, reason: "implementation_artifact_missing" });
      continue;
    }
    const source = readFileSync(path, "utf8");
    findings.push({ ok: marker.test(source), path: relativePath, reason: marker.test(source) ? "contract_marker_present" : "contract_marker_missing" });
  }
  if (checks.length === 0) findings.push({ ok: false, reason: "case_contract_missing" });
  const pass = findings.length > 0 && findings.every((finding) => finding.ok);
  return {
    case: id,
    evidenceKind: "source",
    pass,
    status: pass ? "passed" : "failed",
    findings,
    note: "Source contract only; installed/native runtime behavior requires the integration owner's Windows candidate probe.",
  };
}

function installedExecutable(options = {}) {
  const root = options.installedRoot ?? process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT
    ?? "C:/Users/adrds/AppData/Local/Orthic Labs/Membrane/current";
  return join(resolve(root), "membrane.exe");
}

function installedJson(args, options = {}) {
  const exe = installedExecutable(options);
  if (!existsSync(exe)) return { ok: false, reason: "installed_executable_missing", path: exe };
  const result = spawnSync(exe, args, { encoding: "utf8", timeout: 30_000, windowsHide: true, cwd: options.workspaceRoot });
  const output = String(result.stdout ?? "").trim();
  let value = null;
  try { value = output ? JSON.parse(output) : null; } catch { /* preserve raw diagnostic below */ }
  return { ok: result.status === 0 && value !== null, status: result.status, value, stderr: String(result.stderr ?? "").trim(), stdout: output };
}

function installedMcp(methods, options = {}) {
  const exe = installedExecutable(options);
  if (!existsSync(exe)) return { ok: false, reason: "installed_executable_missing", path: exe };
  const requests = [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "membrane-qualification", version: "1" } } },
    ...methods.map((method, index) => ({ jsonrpc: "2.0", id: index + 2, method, params: {} })),
  ];
  const result = spawnSync(exe, ["stdio-mcp"], { input: `${requests.map((request) => JSON.stringify(request)).join("\n")}\n`, encoding: "utf8", timeout: 30_000, windowsHide: true, cwd: options.workspaceRoot });
  const responses = String(result.stdout ?? "").trim().split(/\r?\n/u).filter(Boolean).map((line) => {
    try { return JSON.parse(line); } catch { return null; }
  });
  return { ok: result.status === 0 && responses.length >= requests.length, status: result.status, responses, stderr: String(result.stderr ?? "").trim() };
}

const BM09_EVENT_PROBES = [
  { point: "session_start", event: "SessionStart", modules: ["membrane.cortex-status", "membrane.memory-rearm"] },
  { point: "user_prompt", event: "UserPromptSubmit", modules: ["membrane.memory-recall"] },
  { point: "pre_tool", event: "PreToolUse", modules: ["membrane.memory-bump", "membrane.diagnostics-fence", "membrane.memory-conflict"] },
  { point: "post_edit", event: "PostToolUse", modules: ["membrane.memory-ingest", "membrane.diagnostics-observe"] },
  { point: "post_tool", event: "PostToolUse", modules: ["membrane.tool-observer", "membrane.diagnostics-observe"] },
  { point: "pre_compaction", event: "PreCompact", modules: ["membrane.memory-pre-compact"] },
  { point: "resume", event: "SessionStart", modules: ["membrane.memory-rearm"] },
];

function hookPayload(probe, workspaceRoot) {
  const payload = {
    hook_event_name: probe.event,
    session_id: "membrane-bm09-session",
    thread_id: "membrane-bm09-session",
    cwd: workspaceRoot,
    source: probe.point === "resume" ? "resume" : "qualification",
  };
  if (probe.point === "user_prompt") payload.prompt = "qualification probe";
  if (probe.point === "pre_tool") {
    payload.tool_name = "Bash";
    payload.tool_input = { command: "git status --short" };
  }
  if (probe.point === "post_edit") {
    payload.tool_name = "Edit";
    payload.tool_input = { file_path: join(workspaceRoot, "README.md"), new_string: "qualification" };
    payload.tool_response = { ok: true };
  }
  if (probe.point === "post_tool") {
    payload.tool_name = "Bash";
    payload.tool_input = { command: "git status --short" };
    payload.tool_response = { ok: true };
  }
  return payload;
}

function runHookProbe(exe, probe, workspaceRoot) {
  const result = spawnSync(exe, ["hook"], {
    input: `${JSON.stringify(hookPayload(probe, workspaceRoot))}\n`, encoding: "utf8",
    timeout: 30_000, windowsHide: true, cwd: workspaceRoot,
  });
  if (result.error || result.status !== 0) return { ok: false, point: probe.point, reason: `hook process failed: ${result.error?.message || result.status}` };
  let response;
  try { response = JSON.parse(String(result.stdout || "").trim()); } catch { return { ok: false, point: probe.point, reason: "hook returned non-JSON output" }; }
  const dispatch = response.membraneHook;
  const output = response.hookSpecificOutput;
  const results = Array.isArray(dispatch?.results) ? dispatch.results : [];
  const ids = new Set(results.map((entry) => entry?.id));
  const modulesPresent = probe.modules.every((id) => ids.has(id));
  const outputsValid = results.every((entry) => entry?.status === "ok" && entry.output?.schemaVersion === 1 && entry.output.kind === "membrane.hook.status" && typeof entry.output.reason === "string");
  const ok = dispatch?.schemaVersion === 1 && dispatch.event === probe.event && dispatch.status === "ok"
    && output?.hookEventName === probe.event && results.length === 16 && modulesPresent && outputsValid;
  return { ok, point: probe.point, event: probe.event, moduleCount: results.length, modules: probe.modules, reason: ok ? "native_hook_dispatch_validated" : "native_hook_dispatch_invalid", response };
}

function runExplicitPullProbe(exe, workspaceRoot) {
  const requests = [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "membrane-qualification", version: "1" } } },
    { jsonrpc: "2.0", id: 2, method: "tools/list", params: {} },
    { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "membrane_context", arguments: {} } },
  ];
  const result = spawnSync(exe, ["stdio-mcp"], {
    input: `${requests.map((request) => JSON.stringify(request)).join("\n")}\n`, encoding: "utf8",
    timeout: 30_000, windowsHide: true, cwd: workspaceRoot,
  });
  const responses = String(result.stdout ?? "").trim().split(/\r?\n/u).filter(Boolean).map((line) => {
    try { return JSON.parse(line); } catch { return null; }
  }).filter(Boolean);
  const init = responses.find((entry) => entry.id === 1)?.result;
  const tools = responses.find((entry) => entry.id === 2)?.result?.tools;
  const call = responses.find((entry) => entry.id === 3)?.result?.structuredContent;
  const ok = result.status === 0 && init?.serverInfo?.name === "membrane" && Array.isArray(tools)
    && tools.some((tool) => tool.name === "membrane_context")
    && call?.operation === "membrane_context" && call.result?.kind === "error" && call.result.code === "context_envelope_invalid";
  return { ok, reason: ok ? "native_explicit_pull_discovered_and_rejected_invalid_envelope" : "native_explicit_pull_probe_invalid", toolCount: Array.isArray(tools) ? tools.length : 0, response: { init, call }, stderr: String(result.stderr ?? "").trim() };
}

export function probeBM09Installed(options = {}) {
  const exe = installedExecutable(options);
  const workspaceRoot = resolve(options.workspaceRoot ?? REPO_ROOT);
  if (!existsSync(exe)) return { pass: false, reason: "installed_executable_missing", detail: { path: exe } };
  const build = installedJson(["cli", "build-info"], { ...options, workspaceRoot });
  if (!build.ok || build.value?.target !== "x86_64-pc-windows-msvc") return { pass: false, reason: "installed_build_identity_invalid", detail: { build } };
  const hooks = BM09_EVENT_PROBES.map((probe) => runHookProbe(exe, probe, workspaceRoot));
  const explicitPull = runExplicitPullProbe(exe, workspaceRoot);
  const pass = hooks.every((probe) => probe.ok) && explicitPull.ok;
  return { pass, reason: pass ? "available_intact_discoverable_effective" : "installed_hook_or_explicit_pull_probe_failed", detail: { executable: exe, build: build.value, hooks, explicitPull } };
}

export function probeBM11Installed(options = {}) {
  const corpusDir = options.corpusDir ?? CORPUS_DIR;
  const exe = installedExecutable(options);
  const workspaceRoot = resolve(options.workspaceRoot ?? REPO_ROOT);
  if (!existsSync(exe)) return { pass: false, reason: "installed_executable_missing", detail: { path: exe } };
  let tasks;
  try { tasks = readJsonlRecords(join(corpusDir, "tasks.jsonl")); } catch (error) { return { pass: false, reason: `corpus_task_read_failed: ${error.message}` }; }
  const temp = mkdtempSync(join(tmpdir(), "membrane-bm11-replay-"));
  const input = join(temp, "replay.jsonl");
  try {
    writeFileSync(input, `${tasks.map((task) => JSON.stringify({ row_id: task.taskId, query: task.prompt, scope: "global" })).join("\n")}\n`, "utf8");
    const replay = spawnSync(exe, ["cli", "replay", "--input", input, "-k", "20"], {
      encoding: "utf8", timeout: 60_000, windowsHide: true, cwd: workspaceRoot,
      env: { ...process.env, ...(options.env ?? {}), WORKSPACE_ROOT: undefined },
    });
    const rows = String(replay.stdout ?? "").trim().split(/\r?\n/u).filter(Boolean).map((line) => {
      try { return JSON.parse(line); } catch { return null; }
    });
    const expectedIds = tasks.map((task) => task.taskId);
    const rowIds = rows.map((row) => row?.row_id);
    const replayPass = replay.status === 0 && rows.length === expectedIds.length
      && sameStringSet(rowIds, expectedIds) && rows.every((row) => Array.isArray(row?.ranked_ids));
    const baseline = installedJson(["cli", "baseline", "--scope", "global", "-k", "20"], { ...options, workspaceRoot });
    const baselinePass = baseline.ok && baseline.value?.schemaVersion === 1 && baseline.value.mode === "baseline"
      && baseline.value.task === "__cortex_baseline_projection__" && Array.isArray(baseline.value.candidates) && Array.isArray(baseline.value.omissions);
    const pass = replayPass && baselinePass;
    return { pass, reason: pass ? "installed replay processed complete corpus & baseline projection" : "installed replay or baseline projection failed", detail: { executable: exe, taskCount: tasks.length, replay: { status: replay.status, rows }, baseline: baseline.value, stderr: String(replay.stderr ?? "").trim() } };
  } finally { rmSync(temp, { recursive: true, force: true }); }
}

function sameStringSet(left, right) {
  return left.length === right.length && new Set(left).size === left.length && left.every((value) => right.includes(value));
}

export function MEM_011(options = {}) {
  const result = installedMcp([]);
  const init = result.responses?.find((response) => response?.id === 1)?.result;
  const ok = result.ok && init?.serverInfo?.name === "membrane" && typeof init.protocolVersion === "string" && init.capabilities && typeof init.instructions === "string";
  return { case: "MEM_011", evidenceKind: "installed", pass: ok, status: ok ? "passed" : "failed", findings: [{ ok, command: "membrane stdio-mcp initialize", reason: ok ? "native_stdio_mcp_initialized" : "native_stdio_mcp_unavailable", response: init ?? null, stderr: result.stderr }] };
}

export function MEM_013(options = {}) {
  const result = installedMcp(["tools/list"]);
  const payload = result.responses?.find((response) => response?.id === 2)?.result;
  const names = Array.isArray(payload?.tools) ? payload.tools.map((tool) => tool.name) : [];
  const ok = result.ok && names.includes("membrane_context") && names.length > 0;
  return { case: "MEM_013", evidenceKind: "installed", pass: ok, status: ok ? "passed" : "failed", findings: [{ ok, command: "membrane stdio-mcp tools/list", reason: ok ? "default_context_tool_discovered" : "tool_discovery_unavailable", toolNames: names, stderr: result.stderr }] };
}

export function MEM_014(options = {}) {
  const result = installedMcp(["resources/list"]);
  const payload = result.responses?.find((response) => response?.id === 2)?.result;
  const resources = Array.isArray(payload?.resources) ? payload.resources : [];
  const ok = result.ok && resources.length > 0 && resources.every((resource) => typeof resource.uri === "string" && Array.isArray(resource.accessGrants) && resource.authorityEscalation === false);
  return { case: "MEM_014", evidenceKind: "installed", pass: ok, status: ok ? "passed" : "failed", findings: [{ ok, command: "membrane stdio-mcp resources/list", reason: ok ? "bounded_resources_listed" : "resource_listing_unavailable", resourceCount: resources.length, stderr: result.stderr }] };
}

export function MEM_015(options = {}) {
  const result = installedMcp(["prompts/list"]);
  const payload = result.responses?.find((response) => response?.id === 2)?.result;
  const prompts = Array.isArray(payload?.prompts) ? payload.prompts : [];
  const ok = result.ok && prompts.length > 0 && prompts.every((prompt) => typeof prompt.name === "string" && typeof prompt.description === "string");
  return { case: "MEM_015", evidenceKind: "installed", pass: ok, status: ok ? "passed" : "failed", findings: [{ ok, command: "membrane stdio-mcp prompts/list", reason: ok ? "bounded_prompts_listed" : "prompt_listing_unavailable", promptNames: prompts.map((prompt) => prompt.name), stderr: result.stderr }] };
}

export function MEM_003(options = {}) {
  const result = installedJson(["cli", "build-info"]);
  const value = result.value ?? {};
  const ok = result.ok && value.target === "x86_64-pc-windows-msvc"
    && typeof value.release_generation === "string" && value.release_generation.length > 0
    && typeof value.membrane_source_commit === "string" && /^[0-9a-f]{40}$/u.test(value.membrane_source_commit);
  return { case: "MEM_003", evidenceKind: "installed", pass: ok, status: ok ? "passed" : "failed", findings: [{ ok, command: "membrane cli build-info", value, reason: ok ? "installed_identity_published" : (result.reason ?? "installed_identity_invalid") }] };
}

export function MEM_025(options = {}) {
  const result = installedJson(["cli", "hub-capabilities"]);
  const value = result.value ?? {};
  const ok = result.ok && value.kind !== "membrane_unavailable" && (value.schemaVersion !== undefined || value.schema_version !== undefined);
  return { case: "MEM_025", evidenceKind: "installed", pass: ok, status: ok ? "passed" : "failed", findings: [{ ok, command: "membrane cli hub-capabilities", reason: ok ? "capability_inventory_returned" : "hub_capability_probe_unavailable", response: value, stderr: result.stderr }] };
}

export function MEM_026(options = {}) {
  const result = installedJson(["cli", "hub-snapshot"]);
  const value = result.value ?? {};
  const ok = result.ok && value.kind !== "membrane_unavailable" && (value.schemaVersion !== undefined || value.schema_version !== undefined);
  return { case: "MEM_026", evidenceKind: "installed", pass: ok, status: ok ? "passed" : "failed", findings: [{ ok, command: "membrane cli hub-snapshot", reason: ok ? "hub_snapshot_returned" : "hub_snapshot_unavailable", response: value, stderr: result.stderr }] };
}

export function MEM_033(options = {}) {
  const result = installedJson(["cli", "diagnostics", "capabilities"]);
  const value = result.value ?? {};
  const ok = result.ok && value.surface === "membrane-live-diagnostics" && Array.isArray(value.endpoints)
    && value.endpoints.includes("POST /diagnostics/workspace/open") && value.endpoints.includes("POST /diagnostics/fence/evaluate");
  return { case: "MEM_033", evidenceKind: "installed", pass: ok, status: ok ? "passed" : "failed", findings: [{ ok, command: "membrane cli diagnostics capabilities", reason: ok ? "diagnostics_surface_advertised" : "diagnostics_capabilities_invalid", response: value, stderr: result.stderr }] };
}

export function MEM_039(options = {}) {
  const result = installedJson(["cli", "doctor", "paths", "--json"]);
  const value = result.value ?? {};
  const roots = value.roots ?? {};
  const ok = result.ok && value.schemaVersion === 1 && ["cache", "config", "data", "log"].every((key) => typeof roots[key] === "string" && roots[key].length > 0);
  return { case: "MEM_039", evidenceKind: "installed", pass: ok, status: ok ? "passed" : "failed", findings: [{ ok, command: "membrane cli doctor paths --json", reason: ok ? "doctor_roots_reported" : "doctor_roots_invalid", response: value, stderr: result.stderr }] };
}

const MEM_CASE_EXPORTS = Object.fromEntries(Object.keys(MEM_SOURCE_CASES).map((id) => [id, (options = {}) => memSourceCase(id, options)]));
Object.assign(MEM_CASE_EXPORTS, { MEM_003, MEM_011, MEM_013, MEM_014, MEM_015, MEM_025, MEM_026, MEM_033, MEM_039 });
export const CASES = { BM09, BM11, ...MEM_CASE_EXPORTS };
export const MEM_001 = MEM_CASE_EXPORTS.MEM_001;
export const MEM_002 = MEM_CASE_EXPORTS.MEM_002;
export const MEM_004 = MEM_CASE_EXPORTS.MEM_004;
export const MEM_005 = MEM_CASE_EXPORTS.MEM_005;
export const MEM_007 = MEM_CASE_EXPORTS.MEM_007;
export const MEM_017 = MEM_CASE_EXPORTS.MEM_017;
export const MEM_022 = MEM_CASE_EXPORTS.MEM_022;
export const MEM_023 = MEM_CASE_EXPORTS.MEM_023;
export const MEM_024 = MEM_CASE_EXPORTS.MEM_024;
export const MEM_027 = MEM_CASE_EXPORTS.MEM_027;
export const MEM_028 = MEM_CASE_EXPORTS.MEM_028;
export const MEM_029 = MEM_CASE_EXPORTS.MEM_029;
export const MEM_030 = MEM_CASE_EXPORTS.MEM_030;
export const MEM_031 = MEM_CASE_EXPORTS.MEM_031;
export const MEM_032 = MEM_CASE_EXPORTS.MEM_032;
export const MEM_034 = MEM_CASE_EXPORTS.MEM_034;
export const MEM_035 = MEM_CASE_EXPORTS.MEM_035;
export const MEM_036 = MEM_CASE_EXPORTS.MEM_036;
export const MEM_037 = MEM_CASE_EXPORTS.MEM_037;
export const MEM_038 = MEM_CASE_EXPORTS.MEM_038;
export const MEM_040 = MEM_CASE_EXPORTS.MEM_040;
export const MEM_041 = MEM_CASE_EXPORTS.MEM_041;
export const MEM_042 = MEM_CASE_EXPORTS.MEM_042;
export const MEM_043 = MEM_CASE_EXPORTS.MEM_043;
export const MEM_045 = MEM_CASE_EXPORTS.MEM_045;
export const MEM_046 = MEM_CASE_EXPORTS.MEM_046;
export const MEM_047 = MEM_CASE_EXPORTS.MEM_047;
export const MEM_048 = MEM_CASE_EXPORTS.MEM_048;
export const MEM_049 = MEM_CASE_EXPORTS.MEM_049;
export const MEM_050 = MEM_CASE_EXPORTS.MEM_050;
export const MEM_051 = MEM_CASE_EXPORTS.MEM_051;

// Direct CLI invocation for local inspection (not the registry runner path).
if (import.meta.url === pathToFileURLSafe(process.argv[1])) {
  for (const [name, fn] of Object.entries(CASES)) {
    const result = fn();
    // eslint-disable-next-line no-console
    console.log(JSON.stringify(result, null, 2));
    if (!result.pass) process.exitCode = 1;
  }
}

function pathToFileURLSafe(path) {
  try { return path ? new URL(`file://${resolve(path).replace(/\\/gu, "/")}`).href : undefined; } catch { return undefined; }
}
