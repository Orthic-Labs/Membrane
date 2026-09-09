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

import { readFileSync, existsSync, statSync } from "node:fs";
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
  return {
    case: "BM09",
    evidenceKind: "source",
    pass,
    status: pass ? "passed" : "failed",
    findings,
    note: "Structural/static: proves descriptors exist and are complete. Live available->intact->discoverable->effective proof requires the installed registry-command run by the integration owner.",
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

  const parseJsonl = (path) => readFileSync(path, "utf8").split(/\r?\n/u).filter((line) => line.trim().length > 0).map((line, lineNumber) => {
    try { return JSON.parse(line); } catch { throw new Error(`${path}:${lineNumber + 1}: invalid JSON`); }
  });

  const tasks = parseJsonl(tasksPath);
  const evidence = parseJsonl(evidencePath);
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
  return {
    case: "BM11",
    evidenceKind: "source",
    pass,
    status: pass ? "passed" : "failed",
    findings,
    taskCount: tasks.length,
    note: "Minimum baseline per acceptance row; statistical/category expansion is sequenced later and not required at this checkpoint. This is a fixture-corpus attestation (structural/static), not a measured task-outcome.",
  };
}

export const CASES = { BM09, BM11 };

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
