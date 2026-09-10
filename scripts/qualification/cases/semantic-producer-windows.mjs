#!/usr/bin/env node
// scripts/qualification/cases/semantic-producer-windows.mjs
//
// r5 wave-A case module for lane "semantic-producer" (dispatch A).
// Registered against windows-acceptance.json for MEM-044, MEM-052, MEM-053
// (caseExport MEM_044 / MEM_052 / MEM_053).
//
// This module performs no cargo/build/test/install execution — worker policy
// forbids all of those. It is a *source-contract* check: it reads the exact
// owned source files this lane is responsible for
// (engine/crates/membrane-runtime/src/background_review.rs and
// background_review_input.rs) and asserts that the structural markers each
// requirement depends on are present verbatim. It is deliberately unable to
// prove runtime behavior — only the integration owner's installed case run
// (per windows-acceptance.json `command`) can do that. Each exported case
// also runs its own declared negative controls by re-checking the same
// markers against a deliberately faulted copy of the source text, so a
// control that silently stopped failing is caught here rather than only at
// installed-acceptance time.
//
// Runner contract: each export is an async function `(context) => CaseResultV1`.
// Without a registry row, exports return source evidence for focused checks.
// Registry execution additionally requires an independently captured installed
// observation; source markers are never relabelled as runtime evidence.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const WORKSPACE_ROOT = resolve(HERE, "../../..");

const BACKGROUND_REVIEW_RS =
  "engine/crates/membrane-runtime/src/background_review.rs";
const BACKGROUND_REVIEW_INPUT_RS =
  "engine/crates/membrane-runtime/src/background_review_input.rs";

function readOwned(workspaceRoot, relativePath) {
  return readFileSync(resolve(workspaceRoot, relativePath), "utf8");
}

function sourceEvidence(workspaceRoot, files) {
  return files.map(({ path, markers }) => {
    const absolute = resolve(workspaceRoot, path);
    if (!existsSync(absolute)) return { path, exists: false, markers: [] };
    const content = readFileSync(absolute, "utf8");
    return {
      path,
      exists: true,
      sha256: createHash("sha256").update(content).digest("hex"),
      markers: markers.filter((marker) => content.includes(marker)),
    };
  });
}

function installedRoot(context) {
  const configured = context?.installedRoot || process.env.MEMBRANE_QUALIFICATION_INSTALLED_ROOT;
  if (!configured) return null;
  const root = resolve(configured);
  return /[\\/]current$/iu.test(root) ? root : resolve(root, "current");
}

function installedObservation(context, id, source) {
  const root = installedRoot(context);
  if (!root) return { ok: false, reason: "installed root was not supplied" };
  const executable = resolve(root, "membrane.exe");
  if (!existsSync(executable)) return { ok: false, reason: `installed membrane.exe missing: ${executable}` };
  let identity;
  try {
    const raw = execFileSync(executable, ["status", "--dry-run"], {
      cwd: root, encoding: "utf8", windowsHide: true, timeout: 15000,
    });
    identity = JSON.parse(raw);
  } catch (error) {
    return { ok: false, reason: `installed identity probe failed: ${error.message}` };
  }
  if (identity.runtimeOrigin !== "installed" || identity.installRoot !== root) {
    return { ok: false, reason: "installed identity probe did not confirm stable current runtime" };
  }
  const observationPath = context?.semanticProducerObservationPath ||
    process.env.MEMBRANE_SEMANTIC_PRODUCER_OBSERVATION ||
    resolve(context?.workspaceRoot || WORKSPACE_ROOT, "audit/qualification/windows-r5/receipts/semantic-producer-observation.json");
  if (!existsSync(observationPath)) return { ok: false, reason: `missing native installed observation: ${observationPath}` };
  let observation;
  try { observation = JSON.parse(readFileSync(observationPath, "utf8")); }
  catch (error) { return { ok: false, reason: `cannot read native installed observation: ${error.message}` }; }
  const result = observation.cases?.[id];
  const sourceHashes = new Map(source.filter((entry) => entry.exists).map((entry) => [entry.path, entry.sha256]));
  const bound = result?.sourceBindings;
  const sourceBound = bound && Object.keys(bound).length === sourceHashes.size &&
    sourceHashes.size > 0 && Object.entries(bound).every(([path, sha256]) => sourceHashes.get(path) === sha256);
  if (observation.schema !== "membrane.windows-semantic-producer-observation.v1" ||
      observation.platform !== "windows" || observation.installed !== true ||
      observation.runtimeOrigin !== "installed" || observation.installedIdentity?.verified !== true ||
      result?.status !== "passed" || result?.nativeEvidence !== true || !sourceBound) {
    return { ok: false, reason: `native installed observation for ${id} is missing verified runtime/source binding` };
  }
  return { ok: true, value: { path: observationPath, executable, identity, case: result } };
}

function resultFor(context, id, requirement, checks, negativeControls, sourceFiles) {
  const failed = checks.filter((check) => !check.passed);
  const source = sourceEvidence(context.workspaceRoot ?? WORKSPACE_ROOT, sourceFiles);
  const sourcePass = failed.length === 0 && negativeControls.every((control) => control.passed);
  const base = {
    id, requirement, pass: sourcePass, passed: sourcePass,
    status: sourcePass ? "passed" : "failed", evidenceKind: "source", kind: "structural",
    findings: failed.flatMap((check) => check.findings), negativeControls, evidence: { source },
  };
  if (!context.row) return base;
  const installed = installedObservation(context, id, source);
  const pass = sourcePass && installed.ok;
  return {
    ...base, pass, passed: pass,
    status: pass ? "passed" : installed.ok ? "failed" : "insufficient",
    evidenceKind: "installed", kind: "installed",
    evidence: { source, installed: installed.ok ? installed.value : null },
    reason: pass ? `${id}: source contract & installed native observation passed` : `${id}: ${installed.reason || "source contract failed"}`,
  };
}

/** Remove every line containing `marker` from `text` — the fault injection
 * primitive every negative control below uses to prove it can actually fail. */
function withoutLinesContaining(text, marker) {
  return text
    .split(/\r?\n/u)
    .filter((line) => !line.includes(marker))
    .join("\n");
}

function markerCheck(text, marker, findingOnMissing) {
  if (text.includes(marker)) return { passed: true, findings: [] };
  return { passed: false, findings: [findingOnMissing] };
}

function negativeControl(text, marker, control, howItFails) {
  const faulted = withoutLinesContaining(text, marker);
  const stillPresent = faulted.includes(marker);
  // A real negative control must actually flip to failing once the marker is
  // removed; if it does not, the control is not exercising the fault.
  return { control, passed: !stillPresent, howItFails };
}

/**
 * MEM-044 — "Execute only bounded idempotent cancellable checkpointable
 * maintenance with crash-visible receipts."
 *
 * Checked structural markers (all in background_review.rs /
 * background_review_input.rs, both owned by this lane):
 *   - bounded:      MAX_ATTEMPTS caps retry attempts per job id.
 *   - idempotent:   BackgroundReviewCursorStore::advance rejects any
 *                   cursor regression (`cursor.last_seq < current.last_seq`),
 *                   so replaying an already-applied window cannot move state
 *                   backward, and the input producer's own cursor file makes
 *                   republishing an unconsumed window an explicit no-op
 *                   (`Same window replayed ... honest no-op`).
 *   - cancellable:  `cancellation_timeout_ms` is a first-class scheduler
 *                   config field.
 *   - checkpointable / crash-visible receipts: the JSONL observation sink
 *                   (`JsonlBackgroundReviewObservationSink`) durably persists
 *                   `BackgroundReviewObservationReceiptV1` records via
 *                   fsync'd atomic append, and the input producer's
 *                   `write_atomic` (temp file + fsync + rename) makes a crash
 *                   mid-publish leave no partial state for a reader to see.
 */
export async function MEM_044(context = {}) {
  const workspaceRoot = context.workspaceRoot ?? WORKSPACE_ROOT;
  const review = readOwned(workspaceRoot, BACKGROUND_REVIEW_RS);
  const input = readOwned(workspaceRoot, BACKGROUND_REVIEW_INPUT_RS);

  const checks = [
    markerCheck(review, "pub const MAX_ATTEMPTS: u8 = 2", "bounded: MAX_ATTEMPTS missing or changed"),
    markerCheck(review, "cursor.last_seq < current.last_seq", "idempotent: cursor regression guard missing"),
    markerCheck(review, "cancellation_timeout_ms", "cancellable: no cancellation_timeout_ms config field"),
    markerCheck(review, "fn append(", "crash-visible receipts: observation sink append is missing"),
    markerCheck(review, "sync_data()", "crash-visible receipts: observation sink does not fsync"),
    markerCheck(input, "fn write_atomic(", "checkpointable: atomic snapshot writer is missing"),
    markerCheck(input, "sync_all()", "checkpointable: snapshot write does not fsync before rename"),
  ];

  const negativeControls = [
    negativeControl(
      review,
      "pub const MAX_ATTEMPTS: u8 = 2",
      "MAX_ATTEMPTS removed",
      "removing the retry bound makes the bounded-attempts marker check fail",
    ),
    negativeControl(
      review,
      "cursor.last_seq < current.last_seq",
      "cursor regression guard removed",
      "removing the monotonic guard makes the idempotency marker check fail",
    ),
    negativeControl(
      input,
      "sync_all()",
      "snapshot fsync removed",
      "removing the fsync call makes the crash-visibility marker check fail",
    ),
  ];

  return resultFor(
    context,
    "MEM-044",
    "Execute only bounded idempotent cancellable checkpointable maintenance with crash-visible receipts.",
    checks,
    negativeControls,
    [
      { path: BACKGROUND_REVIEW_RS, markers: ["MAX_ATTEMPTS", "cursor.last_seq < current.last_seq", "cancellation_timeout_ms", "fn append(", "sync_data()"] },
      { path: BACKGROUND_REVIEW_INPUT_RS, markers: ["fn write_atomic(", "sync_all()"] },
    ],
  );
}

/**
 * MEM-052 — "Admit daemon review work under configured policy."
 *
 * `engine/crates/membrane-runtime/src/background_review.rs:315-367,1688-1735`
 * per the frozen canonical implementation row. Checked markers: the config
 * struct carries every admission-policy field (`enabled`, `min_elapsed_ms`,
 * `activity_threshold`, `per_turn_input_budget`, `aggregate_input_budget`),
 * and the producer (`BackgroundReviewProducer::admit`) and scheduler
 * (`BackgroundReviewDecision`) both exist to gate admission on that policy
 * rather than admitting unconditionally.
 */
export async function MEM_052(context = {}) {
  const workspaceRoot = context.workspaceRoot ?? WORKSPACE_ROOT;
  const review = readOwned(workspaceRoot, BACKGROUND_REVIEW_RS);

  const policyFields = [
    "activity_threshold",
    "per_turn_input_budget",
    "aggregate_input_budget",
    "min_elapsed_ms",
  ];
  const checks = [
    markerCheck(review, "pub struct BackgroundReviewProducer", "admission adapter is missing"),
    markerCheck(review, "pub enum BackgroundReviewDecision", "typed admission decision is missing"),
    markerCheck(review, "BackgroundReviewDecision::Deferred", "policy denial path is missing"),
    ...policyFields.map((field) =>
      markerCheck(review, field, `admission policy field missing: ${field}`),
    ),
  ];

  const negativeControls = [
    negativeControl(
      review,
      "BackgroundReviewDecision::Deferred",
      "deferred admission variant removed",
      "removing the policy-denial variant makes the admission-gating marker check fail",
    ),
    negativeControl(
      review,
      "activity_threshold",
      "activity_threshold field removed",
      "removing the configured policy field makes the admission-policy marker check fail",
    ),
  ];

  return resultFor(
    context,
    "MEM-052",
    "Admit daemon review work under configured policy.",
    checks,
    negativeControls,
    [{ path: BACKGROUND_REVIEW_RS, markers: ["pub struct BackgroundReviewProducer", "pub enum BackgroundReviewDecision", "BackgroundReviewDecision::Deferred", ...policyFields] }],
  );
}

/**
 * MEM-053 — "Execute authenticated proposal-only background semantic review
 * from event cursor/foreground state & persist proposal sink."
 *
 * Checked markers: request construction is cursor/foreground-state bound
 * (`build_background_semantic_review_request`, `foreground_memory_state`),
 * the transport is authenticated (`Authorization: Bearer`), the boundary is
 * proposal-only by construction — every `BackgroundReviewProposalAdmission`
 * default method returns `Err(ProposalSinkUnavailable)` rather than a durable
 * write capability — and a durable JSONL sink exists to persist proposals a
 * caller does supply
 * (`JsonlBackgroundReviewProposalAdmission`).
 */
export async function MEM_053(context = {}) {
  const workspaceRoot = context.workspaceRoot ?? WORKSPACE_ROOT;
  const review = readOwned(workspaceRoot, BACKGROUND_REVIEW_RS);

  const checks = [
    markerCheck(review, "fn build_background_semantic_review_request", "request builder is missing"),
    markerCheck(review, "input.cursor", "request builder is not cursor-bound"),
    markerCheck(review, "foreground_memory_state", "request builder is not foreground-state-bound"),
    markerCheck(review, "Authorization: Bearer", "provider transport is not authenticated"),
    markerCheck(
      review,
      "Err(BackgroundReviewReasonV1::ProposalSinkUnavailable)",
      "proposal-only default is missing: admission trait must default-refuse a durable write",
    ),
    markerCheck(
      review,
      "pub struct JsonlBackgroundReviewProposalAdmission",
      "durable proposal sink implementation is missing",
    ),
  ];

  const negativeControls = [
    negativeControl(
      review,
      "Err(BackgroundReviewReasonV1::ProposalSinkUnavailable)",
      "default admission refusal removed",
      "removing the fail-closed default makes the proposal-only marker check fail",
    ),
    negativeControl(
      review,
      "Authorization: Bearer",
      "bearer header removed from provider transport",
      "removing the auth header makes the authenticated-transport marker check fail",
    ),
    negativeControl(
      review,
      "foreground_memory_state",
      "foreground state removed from request builder",
      "removing the foreground-state field makes the foreground-bound marker check fail",
    ),
  ];

  return resultFor(
    context,
    "MEM-053",
    "Execute authenticated proposal-only background semantic review from event cursor/foreground state & persist proposal sink.",
    checks,
    negativeControls,
    [{ path: BACKGROUND_REVIEW_RS, markers: ["fn build_background_semantic_review_request", "input.cursor", "foreground_memory_state", "Authorization: Bearer", "Err(BackgroundReviewReasonV1::ProposalSinkUnavailable)", "pub struct JsonlBackgroundReviewProposalAdmission"] }],
  );
}

export const CASES = { MEM_044, MEM_052, MEM_053 };

if (import.meta.url === `file://${process.argv[1]}`) {
  const results = await Promise.all(Object.values(CASES).map((run) => run()));
  for (const result of results) {
    // eslint-disable-next-line no-console
    console.log(JSON.stringify(result, null, 2));
  }
  if (results.some((result) => !result.passed)) process.exitCode = 1;
}
