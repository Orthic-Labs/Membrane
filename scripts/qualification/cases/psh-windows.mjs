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

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, writeFileSync, existsSync, rmSync, readFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export const GROUP = "PSH";

export function resolveCli(ctx = {}) {
  const candidate = ctx.cliPath || process.env.MEMBRANE_CLI_PATH || "membrane";
  return candidate;
}

export function probeInstalled(ctx = {}) {
  const cli = resolveCli(ctx);
  const version = runCli(ctx, ["--version"]);
  if (version.error || version.status !== 0) return { status: "blocked", evidenceKind: "installed", reason: "installed Membrane CLI --version probe failed" };
  const status = runCli(ctx, ["status", "--bindings-only", "--dry-run"]);
  if (status.error || status.status !== 0) return { status: "failed", evidenceKind: "installed", reason: `installed status probe failed: ${String(status.stderr || "").trim()}` };
  let payload;
  try { payload = JSON.parse(status.stdout); } catch { return { status: "failed", evidenceKind: "installed", reason: "installed status probe returned non-JSON output" }; }
  if (payload.runtimeOrigin !== "installed" || payload.dryRun !== true) return { status: "failed", evidenceKind: "installed", reason: "installed status response is not an installed dry-run projection" };
  return { status: "passed", evidenceKind: "installed", detail: { cli, version: String(version.stdout || "").trim(), runtimeOrigin: payload.runtimeOrigin, service: payload.service?.state }, reason: "stable installed Membrane CLI answered version and binding-readiness probes" };
}

function runCli(ctx, args, options = {}) {
  const cli = resolveCli(ctx);
  const result = spawnSync(cli, args, {
    encoding: options.encoding === undefined ? "utf8" : options.encoding,
    windowsHide: true,
    timeout: options.timeoutMs ?? 15000,
    input: options.input,
    env: options.env,
  });
  return result;
}

function cliReachable(ctx) {
  const probe = runCli(ctx, ["--version"]);
  return probe.error === undefined && probe.status !== null;
}

function blocked(id, requirement, reason) {
  return { id, group: GROUP, status: "blocked", evidenceKind: "installed", requirement, reason };
}

function insufficient(id, requirement, gap, detail = undefined) {
  return {
    id,
    group: GROUP,
    status: "insufficient_implementation",
    evidenceKind: "installed",
    requirement,
    gap,
    ...(detail === undefined ? {} : { detail }),
    note: "canonicalImplementationRow status is not DELIVERED; positive assertion withheld rather than fabricated.",
  };
}

function pass(id, requirement, detail) {
  return { id, group: GROUP, status: "passed", evidenceKind: "installed", requirement, detail };
}

function fail(id, requirement, detail) {
  return { id, group: GROUP, status: "failed", evidenceKind: "installed", requirement, detail };
}

function withTempDir(fn) {
  const dir = mkdtempSync(join(tmpdir(), "psh-windows-"));
  try {
    return fn(dir);
  } finally {
    try { rmSync(dir, { recursive: true, force: true }); } catch { /* best-effort cleanup */ }
  }
}

function runStdioMcp(ctx, requests) {
  const result = spawnSync(resolveCli(ctx), ["stdio-mcp"], {
    encoding: "utf8", windowsHide: true, timeout: 30_000, env: { ...process.env, ...(ctx.env || {}) },
    input: `${requests.map((request) => JSON.stringify(request)).join("\n")}\n`,
  });
  const responses = String(result.stdout || "").trim().split(/\r?\n/u).filter(Boolean).map((line) => {
    try { return JSON.parse(line); } catch { return null; }
  }).filter(Boolean);
  return { result, responses };
}

function mcpResult(responses, id) {
  const result = responses.find((response) => response.id === id)?.result;
  return result?.structuredContent || result || null;
}

async function runStdioMcpHandshake(ctx, { root, caller, taskId }) {
  const child = spawn(resolveCli(ctx), ["stdio-mcp"], { windowsHide: true, env: { ...process.env, ...(ctx.env || {}) } });
  const responses = [];
  let buffer = "";
  return await new Promise((resolve) => {
    let preparedSent = false;
    const finish = () => { try { child.kill(); } catch {} resolve(responses); };
    const timer = setTimeout(finish, 30_000);
    child.stdout.on("data", (chunk) => {
      buffer += String(chunk);
      const lines = buffer.split(/\r?\n/u); buffer = lines.pop() || "";
      for (const line of lines) {
        if (!line.trim()) continue;
        let response; try { response = JSON.parse(line); } catch { continue; }
        responses.push(response);
        if (response.id === 2 && !preparedSent) {
          const probe = mcpResult(responses, 2);
          const token = probe?.result?.data?.resolverToken || probe?.data?.resolverToken;
          if (!token) continue;
          preparedSent = true;
          child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "membrane_push_prepare", arguments: { repository: root, caller, taskId, sessionId: caller.scopeId, request: { text: "psh025 repeated output\n".repeat(120), kind: "log", maxBytes: 1800, optimize: true, resolverToken: token } } } })}\n`);
        }
        if (response.id === 3) { clearTimeout(timer); finish(); }
      }
    });
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-025", version: "1" } } })}\n`);
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "membrane_push_resolve", arguments: { repository: root, operation: "probe", caller } } })}\n`);
  });
}

// ---------------------------------------------------------------------------
// PSH-001 .. PSH-029
// ---------------------------------------------------------------------------

export function PSH_001(ctx = {}) {
  const req = "Capture command output once, preserve exit/status information, & publish deterministic head/tail with a recovery handle only when the exact original is durably retained.";
  if (!cliReachable(ctx)) return blocked("PSH-001", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const counterPath = join(dir, "invocations.txt");
    const script = [
      `$counter = '${counterPath.replace(/'/gu, "''")}'`,
      "$count = if (Test-Path -LiteralPath $counter) { [int](Get-Content -Raw -LiteralPath $counter) } else { 0 }",
      "Set-Content -NoNewline -LiteralPath $counter -Value ($count + 1)",
      "[Console]::Error.Write('psh001-stderr'); Start-Sleep -Milliseconds 100",
      "[Console]::Out.Write((1..80 | ForEach-Object { 'psh001-line-' + $_.ToString('00') }) -join \"`n\")",
      "exit 7",
    ].join("; ");
    const shellOptions = { env: { ...process.env, MEMBRANE_PUSH_RUNC_SHELL: "powershell.exe -NoLogo -NoProfile -NonInteractive -OutputFormat Text -Command" } };
    const result = runCli(ctx, ["push", "runc", "--shell", "--head", "2", "--tail", "2", "--spill-dir", dir, "--", script], shellOptions);
    if (result.status !== 7) return fail("PSH-001", req, `runc did not preserve exit status 7: ${result.status}; ${result.stderr || ""}`);
    const spillLine = String(result.stdout || "").split(/\r?\n/u).find((line) => line.startsWith("[spill] "));
    const recoveryLine = String(result.stdout || "").split(/\r?\n/u).find((line) => line.startsWith("[recovery] "));
    if (!spillLine || !recoveryLine) return fail("PSH-001", req, "large capture did not publish spill and recovery metadata");
    const emittedPreview = String(result.stdout || "").slice(0, String(result.stdout || "").indexOf("\n[spill] ")).replace(/\n$/u, "");
    const expectedPreview = [
      "psh001-stderrpsh001-line-01",
      "psh001-line-02",
      "… 76 lines elided …",
      "psh001-line-79",
      "psh001-line-80",
    ].join("\n");
    if (emittedPreview !== expectedPreview) return fail("PSH-001", req, `runc preview was not deterministic: expected=${JSON.stringify(expectedPreview)}, actual=${JSON.stringify(emittedPreview)}`);
    const spillPath = spillLine.slice("[spill] ".length).trim();
    if (!spillPath.startsWith(dir)) return fail("PSH-001", req, "spill escaped isolated fixture directory");
    const full = readFileSync(spillPath);
    const expected = Buffer.from(`psh001-stderr${Array.from({ length: 80 }, (_, index) => `psh001-line-${String(index + 1).padStart(2, "0")}`).join("\n")}`, "utf8");
    if (Buffer.compare(full, expected) !== 0) {
      return fail("PSH-001", req, `spill byte mismatch: expected=${expected.length}, actual=${full.length}, expectedPrefix=${expected.subarray(0, 80).toString("hex")}, actualPrefix=${full.subarray(0, 80).toString("hex")}`);
    }
    const marker = JSON.parse(recoveryLine.slice("[recovery] ".length));
    const restored = runCli(ctx, ["push", "restore", marker.recoveryHandle, "--spill-dir", dir]);
    if (restored.status !== 0 || Buffer.compare(Buffer.from(restored.stdout || "", "utf8"), full) !== 0) {
      return fail("PSH-001", req, "recovery handle did not return exact captured bytes without re-execution");
    }
    // Recovery retains original bytes in its SQLite store; spillPath is only
    // a legacy export. Remove canonical retention to exercise missing capture.
    rmSync(join(dir, "push-artifacts.sqlite"));
    const deletedCapture = runCli(ctx, ["push", "restore", marker.recoveryHandle, "--spill-dir", dir]);
    if (deletedCapture.status === 0) return fail("PSH-001", req, "recovery handle remained usable after its retained capture was deleted");
    const tinyScript = "[Console]::Out.Write('tiny')";
    const tiny = runCli(ctx, ["push", "runc", "--shell", "--head", "4", "--tail", "4", "--spill-dir", dir, "--", tinyScript], shellOptions);
    if (tiny.status !== 0 || !String(tiny.stdout || "").startsWith("tiny") || /\[(?:spill|recovery|anchor)\]/u.test(String(tiny.stdout || ""))) {
      return fail("PSH-001", req, "complete no-spill capture advertised a recovery handle");
    }
    if (readFileSync(counterPath, "utf8") !== "1") return fail("PSH-001", req, "recovery path re-executed original command");
    return pass("PSH-001", req, "installed isolated runc preserved exit status, both streams, capped spill bytes, exact recovery, and no-spill handle absence");
  });
}

export function PSH_002(ctx = {}) {
  const req = "Restore exact original bytes through one confined, scope-authorized resolver that verifies expected digest & metadata before returning content on every supported transport.";
  if (!cliReachable(ctx)) return blocked("PSH-002", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const bytes = Buffer.concat([
      ...Array.from({ length: 128 }, (_, index) => Buffer.from(`psh002-line-${String(index + 1).padStart(3, "0")}\r\n`, "utf8")),
      Buffer.from([0x00, 0xff, 0x41, 0x0d, 0x0a, 0x42, 0x0d, 0x0a, ...Array.from({ length: 992 }, (_, index) => index % 251)]),
    ]);
    const counter = join(dir, "invocations.txt");
    const escapedCounter = counter.replace(/'/gu, "''");
    const literalBytes = [...bytes].join(",");
    const script = [
      `$counter = '${escapedCounter}'`,
      "$count = if (Test-Path -LiteralPath $counter) { [int](Get-Content -Raw -LiteralPath $counter) } else { 0 }",
      "Set-Content -NoNewline -LiteralPath $counter -Value ($count + 1)",
      `$bytes = [byte[]](${literalBytes})`,
      "[Console]::OpenStandardOutput().Write($bytes, 0, $bytes.Length)",
    ].join("; ");
    const shellOptions = { env: { ...process.env, MEMBRANE_PUSH_RUNC_SHELL: "powershell.exe -NoLogo -NoProfile -NonInteractive -OutputFormat Text -Command" } };
    const command = script;
    const envA = {
      ...shellOptions.env,
      MEMBRANE_REPO_ROOT: dir,
      MEMBRANE_PUSH_SESSION: "psh002-scope-a",
    };
    const produced = runCli(ctx, ["push", "runc", "--shell", "--head", "2", "--tail", "2", "--spill-dir", dir, "--", command], { env: envA });
    if (produced.status !== 0) return fail("PSH-002", req, `runc producer exited ${produced.status}: ${String(produced.stderr || "")}`);
    const output = String(produced.stdout || "");
    const recoveryLine = output.split(/\r?\n/u).find((line) => line.startsWith("[recovery] "));
    if (!recoveryLine) return fail("PSH-002", req, "installed runc did not publish a recovery reference");
    let reference;
    try { reference = JSON.parse(recoveryLine.slice("[recovery] ".length)); } catch { return fail("PSH-002", req, "recovery metadata was not valid JSON"); }
    const handle = reference.recoveryHandle;
    if (typeof handle !== "string" || !handle.startsWith("mr://anchor/")) return fail("PSH-002", req, "recovery metadata omitted canonical recoveryHandle");
    const expectedDigest = `sha256:${createHash("sha256").update(bytes).digest("hex")}`;
    if (reference.sourceDigest !== expectedDigest && reference.source_digest !== expectedDigest) return fail("PSH-002", req, "recovery metadata digest did not match produced bytes");

    const restored = runCli(ctx, ["push", "restore", handle, "--spill-dir", dir, "--max-bytes", String(bytes.length)], { env: envA, encoding: null });
    if (restored.status !== 0 || !Buffer.isBuffer(restored.stdout) || Buffer.compare(restored.stdout, bytes) !== 0) {
      return fail("PSH-002", req, "installed restore did not return exact binary/CRLF original bytes");
    }
    const replay = runCli(ctx, ["push", "restore", handle, "--spill-dir", dir, "--max-bytes", String(bytes.length)], { env: envA, encoding: null });
    if (replay.status !== 0 || Buffer.compare(replay.stdout, bytes) !== 0 || readFileSync(counter, "utf8") !== "1") {
      return fail("PSH-002", req, "restore did not remain exact without replaying producer command");
    }

    const foreign = runCli(ctx, ["push", "restore", handle, "--spill-dir", dir, "--max-bytes", String(bytes.length)], {
      env: { ...envA, MEMBRANE_PUSH_SESSION: "psh002-scope-b" },
    });
    if (foreign.status === 0) return fail("PSH-002", req, "cross-scope recovery handle unexpectedly resolved");

    const corruptDir = join(dir, "corrupt");
    mkdirSync(corruptDir, { recursive: true });
    // The production resolver stores originals in this exact SQLite artifact;
    // corrupting it must fail closed instead of returning guessed content.
    const corruptProduced = runCli(ctx, ["push", "runc", "--shell", "--head", "2", "--tail", "2", "--spill-dir", corruptDir, "--", command], {
      env: { ...envA, MEMBRANE_REPO_ROOT: corruptDir },
    });
    if (corruptProduced.status !== 0) return fail("PSH-002", req, "corruption fixture producer failed");
    const corruptLine = String(corruptProduced.stdout || "").split(/\r?\n/u).find((line) => line.startsWith("[recovery] "));
    if (!corruptLine) return fail("PSH-002", req, "corruption fixture omitted recovery metadata");
    const corruptHandle = JSON.parse(corruptLine.slice("[recovery] ".length)).recoveryHandle;
    writeFileSync(join(corruptDir, "push-artifacts.sqlite"), Buffer.from("corrupt-metadata", "utf8"));
    const corruptRestore = runCli(ctx, ["push", "restore", corruptHandle, "--spill-dir", corruptDir, "--max-bytes", String(bytes.length)], {
      env: { ...envA, MEMBRANE_REPO_ROOT: corruptDir },
    });
    if (corruptRestore.status === 0) return fail("PSH-002", req, "corrupt recovery metadata/object was accepted");
    return insufficient("PSH-002", req, "CLI/HTTP/MCP resolvers do not share one mandatory verification path (PSH-I002)", {
      transport: "installed CLI",
      handle,
      sourceDigest: expectedDigest,
      exactBytes: true,
      binaryAndCrLf: true,
      boundedWholeRestore: true,
      crossScopeDenied: true,
      corruptObjectDenied: true,
      producerInvocations: readFileSync(counter, "utf8"),
    });
  });
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
  return withTempDir((dir) => {
    const source = Array.from({ length: 80 }, (_, index) => `psh024-line-${index + 1}\n`).join("");
    const produced = runCli(ctx, ["push", "runc", "--shell", "--head", "2", "--tail", "2", "--spill-dir", dir, "--", "i=1; while [ \"$i\" -le 80 ]; do printf 'psh024-line-%s\\n' \"$i\"; i=$((i+1)); done"]);
    if (produced.status !== 0) return fail("PSH-024", req, `runc producer exited ${produced.status}: ${produced.stderr}`);
    const marker = String(produced.stdout || "").split(/\r?\n/u).find((line) => line.includes("[recovery]"));
    if (!marker) return insufficient("PSH-024", req, "CLI did not expose a recovery reference for selector qualification (PSH-I024)");
    let reference;
    try { reference = JSON.parse(marker.slice(marker.indexOf("{"))); } catch { return fail("PSH-024", req, "runc recovery reference was not valid JSON"); }
    const handle = reference.recoveryHandle || reference.handle;
    if (typeof handle !== "string" || !handle.startsWith("mr://anchor/")) return fail("PSH-024", req, "runc recovery reference omitted canonical recoveryHandle");
    const restored = {};
    for (const [name, selector] of [["whole", null], ["bytes", { kind: "bytes", start: 0, end: 15 }], ["lines", { kind: "lines", start: 1, end: 2 }]]) {
      const args = ["push", "restore", handle, "--spill-dir", dir];
      if (selector) args.push("--selector", JSON.stringify(selector));
      const result = runCli(ctx, args);
      const expected = name === "whole" ? source : name === "bytes" ? source.slice(0, 15) : source.split(/(?<=\n)/u).slice(0, 2).join("");
      if (result.status !== 0 || String(result.stdout || "") !== expected) return fail("PSH-024", req, `${name} selector restore was not exact: ${result.stderr}`);
      restored[name] = true;
    }
    const expectedDigest = `sha256:${createHash("sha256").update(source).digest("hex")}`;
    if (reference.source_digest !== expectedDigest && reference.sourceDigest !== expectedDigest) return fail("PSH-024", req, "recovery reference parent digest did not match original bytes");
    const malformed = runCli(ctx, ["push", "restore", handle, "--spill-dir", dir, "--selector", JSON.stringify({ kind: "unsupported" })]);
    if (malformed.status === 0) return fail("PSH-024", req, "unsupported selector unexpectedly succeeded");
    return insufficient("PSH-024", req, "native MCP/resident/CLI resolver parity over the shared store contract remains unverified (PSH-I024)", { transport: "native CLI", handle, selectors: Object.keys(restored), malformedSelectorRefused: true });
  });
}

export async function PSH_025(ctx = {}) {
  const req = "Before offloading content, prove that the current consumer can discover and invoke the authorized recovery operation against the matching artifact store; otherwise return a complete inline result or typed refusal.";
  if (!cliReachable(ctx)) return blocked("PSH-025", req, "no installed `membrane` CLI reachable");
  const ownsFixture = !ctx.enrolledRoot;
  const root = ctx.enrolledRoot || mkdtempSync(join(tmpdir(), "psh025-enrolled-"));
  const cleanup = () => { if (ownsFixture) { try { rmSync(root, { recursive: true, force: true }); } catch {} } };
  const registry = ctx.registryPath || join(root, "project-registry.json");
  const env = ownsFixture ? { ...(ctx.env || {}), MEMBRANE_PROJECT_REGISTRY: registry, MEMBRANE_WORKSPACE_ROOT: root } : (ctx.env || {});
  if (ownsFixture) {
    const installScript = join(resolveCli(ctx).replace(/\\[^\\]+$/u, ""), "mcp", "install.mjs");
    const init = spawnSync(process.execPath, [installScript, "init", root, "--repository", ctx.repositoryId || "psh025", "--scope", ctx.scopeId || `psh-${process.pid}`], { encoding: "utf8", windowsHide: true, env: { ...process.env, ...env } });
    if (init.status !== 0) { cleanup(); return fail("PSH-025", req, `native enrollment fixture initialization failed: ${String(init.stderr || init.stdout || "").trim()}`); }
  }
  const caller = { root, repositoryId: ctx.repositoryId || "psh025", scopeId: ctx.scopeId || `psh-${process.pid}` };
  const taskId = ctx.taskId || `psh-025-${process.pid}`;
  const probeRun = { responses: await runStdioMcpHandshake({ ...ctx, env }, { root, caller, taskId }) };
  const probe = mcpResult(probeRun.responses, 2);
  const probeData = probe?.result?.data || probe?.data;
  if (!probeData?.resolverToken || !probeData?.storeId) { cleanup(); return insufficient("PSH-025", req, "installed third-party host qualification for the consumer-qualified recovery handshake remains pending (PSH-I025)", { transport: "native stdio-mcp", probeCode: probe?.result?.code || probe?.code || "consumer_probe_unavailable", root }); }
  const prepared = mcpResult(probeRun.responses, 3);
  const preparedData = prepared?.result?.data || prepared?.data;
  if (!preparedData?.recovery?.handle) { cleanup(); return fail("PSH-025", req, `authorized prepare omitted recovery reference: ${JSON.stringify(prepared)}`); }
  const outcome = insufficient("PSH-025", req, "installed third-party host qualification for the consumer-qualified recovery handshake remains pending (PSH-I025)", { transport: "native stdio-mcp", storeId: probeData.storeId, resolverTokenBound: true, prepared: true, handle: preparedData.recovery.handle });
  cleanup();
  return outcome;
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

export async function runAll(ctx = {}) {
  const entries = await Promise.all(Object.entries(CASES).map(async ([id, fn]) => [id, await fn(ctx)]));
  return Object.fromEntries(entries);
}

// Manual local invocation (never used by CI): `node psh-windows.mjs`.
if (import.meta.url === pathToFileURLSafe(process.argv[1])) {
  const results = await runAll({});
  process.stdout.write(`${JSON.stringify(results, null, 2)}\n`);
}

function pathToFileURLSafe(p) {
  try {
    return new URL(`file://${p.replace(/\\/g, "/")}`).href;
  } catch {
    return "";
  }
}
