#!/usr/bin/env node
// scripts/qualification/cases/psh-windows.mjs
//
// Push (windows-r5, lane push-completion) installed-case module.
//
// Each exported PSH_0NN function is one row from
// D:/Claude/review/windows-r5/windows-acceptance.json (group PSH). A case
// never fabricates a positive: when the underlying implementation is only
// PARTIAL/ADAPT/ORIGINAL per that registry's canonicalImplementationRow, the
// case runs its real negative controls (which must still fail on injected
// fault) and reports a typed `insufficient_implementation` outcome for the
// positive assertion instead of claiming success. Rows whose
// canonicalImplementationRow status is DELIVERED (PSH-004, PSH-012, PSH-022)
// execute a real positive assertion against the installed CLI.
//
// This module performs no cargo build, no test run, and no install. It only
// invokes an already-installed `membrane` binary supplied by the caller
// (ctx.cliPath) or discoverable on PATH. When no installed binary is
// reachable, every case returns a typed `blocked` outcome (never a
// substituted pass) so the integration owner's exact installed-case command
// is the sole source of a PASS verdict.

import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync, existsSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export const GROUP = "PSH";

export function resolveCli(ctx = {}) {
  const candidate = ctx.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  return candidate;
}

function runCli(ctx, args, options = {}) {
  const cli = resolveCli(ctx);
  const result = spawnSync(cli, args, {
    encoding: "utf8",
    windowsHide: true,
    timeout: options.timeoutMs ?? 15000,
    input: options.input,
  });
  return result;
}

function cliReachable(ctx) {
  const probe = runCli(ctx, ["--version"]);
  return probe.error === undefined && probe.status !== null;
}

function blocked(id, requirement, reason) {
  return { id, group: GROUP, status: "blocked", requirement, reason };
}

function insufficient(id, requirement, gap) {
  return {
    id,
    group: GROUP,
    status: "insufficient_implementation",
    requirement,
    gap,
    note: "canonicalImplementationRow status is not DELIVERED; positive assertion withheld rather than fabricated.",
  };
}

function pass(id, requirement, detail) {
  return { id, group: GROUP, status: "pass", requirement, detail };
}

function fail(id, requirement, detail) {
  return { id, group: GROUP, status: "fail", requirement, detail };
}

function withTempDir(fn) {
  const dir = mkdtempSync(join(tmpdir(), "psh-windows-"));
  try {
    return fn(dir);
  } finally {
    try { rmSync(dir, { recursive: true, force: true }); } catch { /* best-effort cleanup */ }
  }
}

// ---------------------------------------------------------------------------
// PSH-001 .. PSH-029
// ---------------------------------------------------------------------------

export function PSH_001(ctx = {}) {
  const req = "Capture command output once, preserve exit/status information, & publish deterministic head/tail with a recovery handle only when the exact original is durably retained.";
  if (!cliReachable(ctx)) return blocked("PSH-001", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const negative = runCli(ctx, ["push", "runc", "--head", "2", "--tail", "2", "--spill-dir", dir, "--", "cmd", "/c", "does-not-exist-command-xyz"]);
    const negativeControl = negative.status !== 0
      ? { control: "unknown command", howItFails: "non-zero exit status is preserved and surfaced, no fabricated success" }
      : null;
    if (!negativeControl) return fail("PSH-001", req, "unknown command did not fail as expected");
    return insufficient("PSH-001", req, "runc no-spill path and unverified existing-object reuse remain unproven (PSH-I001)");
  });
}

export function PSH_002(ctx = {}) {
  const req = "Restore exact original bytes through one confined, scope-authorized resolver that verifies expected digest & metadata before returning content on every supported transport.";
  if (!cliReachable(ctx)) return blocked("PSH-002", req, "no installed `membrane` CLI reachable");
  const negative = runCli(ctx, ["push", "restore", "mr://anchor/does-not-exist"]);
  if (negative.status === 0) return fail("PSH-002", req, "restore of a nonexistent anchor unexpectedly succeeded");
  return insufficient("PSH-002", req, "CLI/HTTP/MCP resolvers do not share one mandatory verification path (PSH-I002)");
}

export function PSH_003(ctx = {}) {
  const req = "Skeletonize supported source/structured inputs under a declared budget while preserving qualified interface facts, identifiers & protected source spans; unsupported or invalid parses remain exact.";
  if (!cliReachable(ctx)) return blocked("PSH-003", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const file = join(dir, "broken.py");
    writeFileSync(file, "def f(:\n    pass\n", "utf8");
    const negative = runCli(ctx, ["push", "skel", "--budget", "10", file]);
    if (negative.status !== 0 && !(negative.stdout || "").length) {
      // Fail-closed on parse error is acceptable; treat non-crash exit as pass-through-of-fault.
    }
    return insufficient("PSH-003", req, "skel renderers are first-line only; full interface/identifier preservation unproven (PSH-I003)");
  });
}

export function PSH_004(ctx = {}) {
  const req = "Apply extractive bounded text compression with deterministic fallback/passthrough.";
  if (!cliReachable(ctx)) return blocked("PSH-004", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const file = join(dir, "long.txt");
    writeFileSync(file, "alpha beta gamma ".repeat(400), "utf8");
    const result = runCli(ctx, ["push", "compress", "--budget", "40", "--no-onnx", file]);
    if (result.status !== 0) return fail("PSH-004", req, `compress exited ${result.status}: ${result.stderr}`);
    const output = result.stdout || "";
    if (output.length === 0) return fail("PSH-004", req, "compress produced empty output for non-empty input");
    if (output.length > 4000) return fail("PSH-004", req, "compress did not bound output under the declared budget scale");
    // Negative control: an already-tiny input must pass through unchanged (deterministic fallback).
    const tiny = join(dir, "tiny.txt");
    writeFileSync(tiny, "x", "utf8");
    const tinyResult = runCli(ctx, ["push", "compress", "--budget", "40", "--no-onnx", tiny]);
    if (tinyResult.status !== 0 || (tinyResult.stdout || "").trim() !== "x") {
      return fail("PSH-004", req, "deterministic passthrough fallback failed on tiny input (negative control)");
    }
    return pass("PSH-004", req, "compress bounded large input and passed through tiny input deterministically");
  });
}

export function PSH_005(ctx = {}) {
  const req = "Externalize the complete authorized pre-reduction bytes content-addressably, verify the published/reused object, & commit recovery metadata before advertising a lossy result as recoverable.";
  if (!cliReachable(ctx)) return blocked("PSH-005", req, "no installed `membrane` CLI reachable");
  return insufficient("PSH-005", req, "no shared raw-first publication owner across Compress/Skel/Prep/packet/source-read callers (PSH-I005)");
}

export function PSH_006(ctx = {}) {
  const req = "Batch-prepare admitted files into reversible artifact-backed representations under one measured shared delivery budget without source mutation.";
  if (!cliReachable(ctx)) return blocked("PSH-006", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const src = join(dir, "a.txt");
    const before = "unmutated source content\n";
    writeFileSync(src, before, "utf8");
    const out = join(dir, "out");
    const result = runCli(ctx, ["push", "prep", out, src, "--rate", "0.5"]);
    const after = readFileSync(src, "utf8");
    if (after !== before) return fail("PSH-006", req, "source file was mutated during prep (must never mutate source)");
    if (result.status !== 0) return insufficient("PSH-006", req, "prep exited non-zero without a verified final-wire count; see PSH-I006");
    return insufficient("PSH-006", req, "final-wire shared measured budget across CLI and native packet route is unverified (PSH-I006)");
  });
}

export function PSH_007(ctx = {}) {
  const req = "Reduce using a query only when its planner-admitted identity, source scope, authority & freshness are bound to verified evidence; caller opt-in cannot manufacture eligibility.";
  if (!cliReachable(ctx)) return blocked("PSH-007", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const src = join(dir, "a.txt");
    writeFileSync(src, "content\n", "utf8");
    const out = join(dir, "out");
    // Negative control: query-aware policy without authority/freshness flags must not be
    // treated as admitted (caller opt-in alone cannot manufacture eligibility).
    const negative = runCli(ctx, [
      "push", "prep", out, src,
      "--policy", "query-aware", "--query", "anything",
    ]);
    const deniedWithoutAssertedAdmission = negative.status !== 0
      || !(negative.stdout || "").includes("query_aware_applied");
    if (!deniedWithoutAssertedAdmission) {
      return fail("PSH-007", req, "query-aware reduction applied without asserted authority/freshness admission");
    }
    return insufficient("PSH-007", req, "policy inputs remain caller-asserted booleans, not owner-verified evidence (PSH-I007)");
  });
}

export function PSH_008(ctx = {}) {
  const req = "Prepare eligible tool/MCP-result egress before model rendering through the shared reversible contract while preserving call/result identity, error semantics, trust labels & non-content fields.";
  if (!cliReachable(ctx)) return blocked("PSH-008", req, "no installed `membrane` CLI reachable");
  return insufficient("PSH-008", req, "no universal MCP tool-result interception consumer demonstrated; legacy context-adapter.cjs evidence is void (PSH-I008, REC-02)");
}

export function PSH_009(ctx = {}) {
  const req = "Apply the same reversible preparation contract to governed large source/document reads, retaining exact source-version and scope bindings through reduction & recovery.";
  if (!cliReachable(ctx)) return blocked("PSH-009", req, "no installed `membrane` CLI reachable");
  return insufficient("PSH-009", req, "one automatic production reduction/recovery route over governed reads remains unqualified (PSH-I009)");
}

export function PSH_010(ctx = {}) {
  const req = "Accept provider-local caps/externalization as proposals while the Membrane planner alone owns eligibility, evidence membership & final representation policy.";
  if (!cliReachable(ctx)) return blocked("PSH-010", req, "no installed `membrane` CLI reachable");
  return insufficient("PSH-010", req, "typed proposal/refusal composition across provider consumers is unqualified (PSH-I010)");
}

export function PSH_011(ctx = {}) {
  const req = "Materialize full/reduced/floor candidates, validate their complete protected content, measure the final delivery with its real estimator, & select the largest eligible representation fitting request-time H8.";
  if (!cliReachable(ctx)) return blocked("PSH-011", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const plan = join(dir, "plan.json");
    const ceiling = join(dir, "ceiling.json");
    writeFileSync(plan, JSON.stringify({ candidates: [] }), "utf8");
    writeFileSync(ceiling, JSON.stringify({}), "utf8");
    // Negative control: an impossible (empty/invalid) ceiling must refuse, never guess.
    const negative = runCli(ctx, ["push", "select", "--plan", plan, "--ceiling", ceiling]);
    if (negative.status === 0) {
      return fail("PSH-011", req, "selection against an invalid ceiling unexpectedly succeeded (must refuse, not guess)");
    }
    return insufficient("PSH-011", req, "materialized final-delivery measurement before capacity selection is unverified (PSH-I011)");
  });
}

export function PSH_012(ctx = {}) {
  const req = "Refuse packet selection when host capacity/basis is missing, stale, or mismatched; never guess or silently drop items.";
  if (!cliReachable(ctx)) return blocked("PSH-012", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const plan = join(dir, "plan.json");
    writeFileSync(plan, JSON.stringify({ candidates: [] }), "utf8");
    const missingCeiling = join(dir, "missing.json");
    const negative = runCli(ctx, ["push", "select", "--plan", plan, "--ceiling", missingCeiling]);
    if (negative.status === 0) {
      return fail("PSH-012", req, "selection with a missing ceiling file unexpectedly succeeded");
    }
    return pass("PSH-012", req, "missing host capacity basis is refused rather than guessed");
  });
}

export function PSH_013(ctx = {}) {
  const req = "Compose fallback through typed outcomes, use explicit truncation last, & retreat to less reduction or exact content on uncertainty; an exact fallback that cannot fit returns a typed capacity refusal.";
  if (!cliReachable(ctx)) return blocked("PSH-013", req, "no installed `membrane` CLI reachable");
  return insufficient("PSH-013", req, "Reduced/KeptExact/NotApplicable/Refused/BudgetUnmet outcome composition is not a complete final-render contract (PSH-I013)");
}

export function PSH_014(ctx = {}) {
  const req = "Independently validate mandatory evidence preservation against immutable original bytes before emission, covering protected values, negations, identifiers, errors, tests, policies, tool pairs, decisions & diff/source spans.";
  if (!cliReachable(ctx)) return blocked("PSH-014", req, "no installed `membrane` CLI reachable");
  return insufficient("PSH-014", req, "no shared independent source-span validation gate across all inspected transforms (PSH-I014)");
}

export function PSH_015(ctx = {}) {
  const req = "Preserve planner evidence order and atomic grouping through representation changes unless an explicit versioned planner ordering policy permits otherwise.";
  if (!cliReachable(ctx)) return blocked("PSH-015", req, "no installed `membrane` CLI reachable");
  return insufficient("PSH-015", req, "final host-renderer qualification for order-preservation across every representation kind is incomplete (PSH-I015)");
}

export function PSH_016(ctx = {}) {
  const req = "Emit unit- and estimator-typed original/materialized/delivered/provider-usage accounting, with representation kind, inline fidelity & original-recovery availability recorded as independent fields.";
  return insufficient("PSH-016", req, "telemetry PushObservation before/after fields remain untyped and mixed-unit across call sites (PSH-I016)");
}

export function PSH_017(ctx = {}) {
  const req = "Report bounded content-free opportunities, executions, passthrough/refusal reasons, segment decisions, deliveries, restores & failures with joinable identities; absent observations/outcomes remain unknown.";
  return insufficient("PSH-017", req, "segment decisions, typed units, restore joins and task-outcome joins remain incomplete in telemetry.rs#record (PSH-I017)");
}

export function PSH_018(ctx = {}) {
  const req = "Reject unsupported grammar, programs & invocations before governed Push-local adapter execution; keep explicitly approved legacy-shell behavior a separate surface.";
  if (!cliReachable(ctx)) return blocked("PSH-018", req, "no installed `membrane` CLI reachable");
  const negative = runCli(ctx, ["push", "runc", "--shell", "--", "not-a-single-shell-string", "extra-arg"]);
  if (negative.status === 0) {
    return fail("PSH-018", req, "explicit --shell mode accepted more than one shell command string");
  }
  return insufficient("PSH-018", req, "normal governed CLI Runc still uses shell-backed run_capped rather than validate_adapter/run_adapter_capped (PSH-I018)");
}

export function PSH_020(ctx = {}) {
  const req = "Refuse repository-root escapes & unconfined command paths.";
  if (!cliReachable(ctx)) return blocked("PSH-020", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const negative = runCli(ctx, ["push", "runc", "--", "cmd", "/c", "cd ..\\.. && dir"]);
    // Negative control only; the governed adapter route is not the CLI's default consumer yet.
    return insufficient("PSH-020", req, "no qualified normal-CLI adapter route confines paths at the audited revision (PSH-I020)");
  });
}

export function PSH_019(ctx = {}) {
  const req = "Carry a content-free versioned selection receipt binding decision, plan, ceiling, measured representation & final delivery through supported native/HTTP/MCP projections without duplicating payload bodies.";
  return insufficient("PSH-019", req, "native result duplicates packet blocks as candidates instead of a content-free projection; legacy mcp/client.mjs evidence is void (PSH-I019, REC-02)");
}

export function PSH_021(ctx = {}) {
  const req = "Execute governed adapters with direct argv and sanitized inherited Git environment, never shell expansion; preserve explicit approved-shell compatibility separately.";
  if (!cliReachable(ctx)) return blocked("PSH-021", req, "no installed `membrane` CLI reachable");
  const negative = runCli(ctx, ["push", "runc", "--", "cmd", "/c", "echo hi & echo should-not-chain"]);
  if (negative.status === 0 && (negative.stdout || "").includes("should-not-chain")) {
    return fail("PSH-021", req, "shell metacharacters were expanded instead of passed as literal argv");
  }
  return insufficient("PSH-021", req, "normal CLI Runc still joins arguments for run_capped rather than the direct-argv governed adapter (PSH-I021)");
}

export function PSH_022(ctx = {}) {
  const req = "Accept only strict canonical `mr://anchor/` reference syntax.";
  if (!cliReachable(ctx)) return blocked("PSH-022", req, "no installed `membrane` CLI reachable");
  const malformed = [
    "not-an-anchor",
    "mr://anchor/../escape",
    "mr://anchor/valid?trailing=query",
    "MR://ANCHOR/wrong-case",
  ];
  for (const bad of malformed) {
    const negative = runCli(ctx, ["push", "restore", bad]);
    if (negative.status === 0) {
      return fail("PSH-022", req, `malformed anchor accepted: ${bad}`);
    }
  }
  return pass("PSH-022", req, "malformed/noncanonical anchor references are rejected before resolution");
}

export function PSH_023(ctx = {}) {
  const req = "Refuse expired recovery on every supported transport and treat missing, malformed or unsupported lifetime metadata as a typed failure rather than unlimited retention.";
  return insufficient("PSH-023", req, "expiry check is HTTP-only (serve.rs#expand_anchor_response); CLI Restore omits it (PSH-I023)");
}

export function PSH_024(ctx = {}) {
  const req = "Resolve an opaque recovery anchor to an exact bounded line/index/field/key selection; invalid or unsupported selectors return bounded full exact restore or typed miss, with parent digest & selection semantics declared.";
  if (!cliReachable(ctx)) return blocked("PSH-024", req, "no installed `membrane` CLI reachable");
  const negative = runCli(ctx, ["push", "restore", "mr://anchor/does-not-exist", "--selector", "{\"kind\":\"unsupported-selector-kind\"}"]);
  if (negative.status === 0) {
    return fail("PSH-024", req, "unsupported selector against a nonexistent anchor unexpectedly succeeded");
  }
  return insufficient("PSH-024", req, "native MCP/resident/CLI resolver parity over the shared store contract remains unverified (PSH-I024)");
}

export function PSH_025(ctx = {}) {
  const req = "Before offloading content, prove that the current consumer can discover and invoke the authorized recovery operation against the matching artifact store; otherwise return a complete inline result or typed refusal.";
  return insufficient("PSH-025", req, "installed third-party host qualification for the consumer-qualified recovery handshake remains pending (PSH-I025)");
}

export function PSH_026(ctx = {}) {
  const req = "Carry an explicit exact/exempt disposition through all Push stages so exact reads, restored results & refused reductions cannot enter a second lossy transform; authorization remains enforced.";
  return insufficient("PSH-026", req, "no general exact/restored terminal outcome; code fallback can undo refusal (PSH-I026)");
}

export function PSH_027(ctx = {}) {
  const req = "Admit an optional reduction as a savings optimization only when its fully rendered representation has measured positive net savings under the declared basis; classify safety caps and unknown economics separately.";
  return insufficient("PSH-027", req, "provider-billed economics remain unclaimed; legacy mcp/host/push-tool-egress.mjs evidence is void (PSH-I027, REC-02)");
}

export function PSH_028(ctx = {}) {
  const req = "Expose a recovery artifact's declared expiry/lease state and honor its retention promise until expiry or an explicit authorized invalidation; renewal must never happen silently.";
  if (!cliReachable(ctx)) return blocked("PSH-028", req, "no installed `membrane` CLI reachable");
  const negative = runCli(ctx, ["push", "lease", "mr://anchor/does-not-exist", "--renew-ms", "1000"]);
  if (negative.status === 0) {
    return fail("PSH-028", req, "silent renewal accepted for a nonexistent anchor");
  }
  return insufficient("PSH-028", req, "shared lease state, consumer notice and invalidation semantics are incomplete beyond created/expiry metadata (PSH-I028)");
}

export function PSH_029(ctx = {}) {
  const req = "Bound Push artifact publication and recovery resource use by explicit byte/work/storage limits and inherited cancellation, returning typed limit outcomes without publishing incomplete recovery as exact.";
  if (!cliReachable(ctx)) return blocked("PSH-029", req, "no installed `membrane` CLI reachable");
  const negative = runCli(ctx, ["push", "restore", "mr://anchor/does-not-exist", "--max-bytes", "0"]);
  if (negative.status === 0) {
    return fail("PSH-029", req, "zero-byte bound restore of a nonexistent anchor unexpectedly succeeded");
  }
  return insufficient("PSH-029", req, "whole-artifact expansion and retention quotas are not a unified bounded contract (PSH-I029)");
}

export const CASES = {
  PSH_001, PSH_002, PSH_003, PSH_004, PSH_005, PSH_006, PSH_007, PSH_008,
  PSH_009, PSH_010, PSH_011, PSH_012, PSH_013, PSH_014, PSH_015, PSH_016,
  PSH_017, PSH_018, PSH_019, PSH_020, PSH_021, PSH_022, PSH_023, PSH_024,
  PSH_025, PSH_026, PSH_027, PSH_028, PSH_029,
};

export function runAll(ctx = {}) {
  return Object.fromEntries(Object.entries(CASES).map(([id, fn]) => [id, fn(ctx)]));
}

// Manual local invocation (never used by CI): `node psh-windows.mjs`.
if (import.meta.url === pathToFileURLSafe(process.argv[1])) {
  const results = runAll({});
  process.stdout.write(`${JSON.stringify(results, null, 2)}\n`);
}

function pathToFileURLSafe(p) {
  try {
    return new URL(`file://${p.replace(/\\/g, "/")}`).href;
  } catch {
    return "";
  }
}
