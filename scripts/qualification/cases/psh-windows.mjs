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
import { mkdtempSync, writeFileSync, existsSync, rmSync, readFileSync, mkdirSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, normalize, resolve } from "node:path";

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
  const identity = installedIdentity(cli, payload);
  if (!identity.ok) return { status: "failed", evidenceKind: "installed", reason: identity.reason, detail: { cli, payload } };
  return { status: "passed", evidenceKind: "installed", detail: { cli, version: String(version.stdout || "").trim(), runtimeOrigin: payload.runtimeOrigin, service: payload.service?.state, identity: identity.value }, reason: "stable installed Membrane CLI answered version, binding-readiness, and current-root identity probes" };
}

export function verifyInstalledIdentity(cli, statusPayload = undefined) {
  const absolute = resolve(cli);
  const current = dirname(absolute);
  if (normalize(current).split(/[\\/]/u).at(-1)?.toLowerCase() !== "current") return { ok: false, reason: "installed CLI is not under installer-owned stable current root" };
  const localAppData = process.env.LOCALAPPDATA;
  if (!localAppData) return { ok: false, reason: "canonical LOCALAPPDATA is unavailable" };
  const canonicalCurrent = resolve(localAppData, "Orthic Labs", "Membrane", "current");
  if (normalize(current).toLowerCase() !== normalize(canonicalCurrent).toLowerCase()) return { ok: false, reason: "installed CLI is not canonical LOCALAPPDATA\\Orthic Labs\\Membrane\\current" };
  const releasePath = join(current, "release.json");
  if (!existsSync(releasePath)) return { ok: false, reason: "installer-owned current root has no release.json" };
  let release;
  try { release = JSON.parse(readFileSync(releasePath, "utf8")); } catch { return { ok: false, reason: "installer-owned release.json is invalid" }; }
  if (release.product !== "membrane" || release.os !== "windows" || release.arch !== "x64" || typeof release.releaseGeneration !== "string") return { ok: false, reason: "installer-owned release.json lacks canonical Windows identity" };
  if (!release.files || typeof release.files !== "object" || Object.keys(release.files).length === 0) return { ok: false, reason: "installer-owned release.json lacks file SHA manifest" };
  const manifestSha256 = createHash("sha256").update(readFileSync(releasePath)).digest("hex");
  for (const [relative, expectedRaw] of Object.entries(release.files)) {
    const expected = String(expectedRaw).replace(/^sha256:/u, "");
    if (!/^[0-9a-f]{64}$/iu.test(expected) || relative.includes("..") || relative.startsWith("/")) return { ok: false, reason: `installer file manifest entry is invalid: ${relative}` };
    const target = resolve(current, relative);
    if (!target.toLowerCase().startsWith(`${current.toLowerCase()}\\`) || !existsSync(target) || !statSync(target).isFile()) return { ok: false, reason: `installer file manifest target is missing: ${relative}` };
    const observed = createHash("sha256").update(readFileSync(target)).digest("hex");
    if (observed.toLowerCase() !== expected.toLowerCase()) return { ok: false, reason: `installer file SHA mismatch: ${relative}` };
  }
  const buildInfoRun = runCli({ cliPath: cli }, ["cli", "build-info"]);
  if (buildInfoRun.error || buildInfoRun.status !== 0) return { ok: false, reason: "installed build-info identity probe failed" };
  let buildInfo;
  try { buildInfo = JSON.parse(String(buildInfoRun.stdout || "")); } catch { return { ok: false, reason: "installed build-info identity was not JSON" }; }
  const target = buildInfo.target;
  const sourceCommit = buildInfo.membrane_source_commit || buildInfo.sourceCommit || buildInfo.source_commit;
  if (target !== "x86_64-pc-windows-msvc") return { ok: false, reason: `installed build target is not x86_64-pc-windows-msvc: ${target || "missing"}` };
  if (!/^[0-9a-f]{40}$/iu.test(String(sourceCommit || ""))) return { ok: false, reason: "installed build-info lacks canonical source commit" };
  const reported = statusPayload?.service?.releaseGeneration || statusPayload?.releaseGeneration;
  if (typeof reported !== "string" || reported !== release.releaseGeneration) return { ok: false, reason: "installed status identity does not match installer-owned release.json" };
  const reportedRoot = statusPayload?.installRoot;
  if (typeof reportedRoot !== "string" || normalize(reportedRoot).toLowerCase() !== normalize(current).toLowerCase()) return { ok: false, reason: "installed status root does not match installer-owned stable current root" };
  if (buildInfo.release_generation !== release.releaseGeneration) return { ok: false, reason: "installed build-info release generation does not match release.json" };
  const executableSha256 = createHash("sha256").update(readFileSync(absolute)).digest("hex");
  if (String(release.files["membrane.exe"] || "").replace(/^sha256:/u, "").toLowerCase() !== executableSha256.toLowerCase()) return { ok: false, reason: "installed membrane.exe SHA is not bound by release.json" };
  return { ok: true, value: { root: current, canonicalRoot: canonicalCurrent, version: release.version, releaseGeneration: release.releaseGeneration, manifestSha256, executableSha256, target, sourceCommit } };
}

const installedIdentity = verifyInstalledIdentity;

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

function nativePushSurface(ctx, { root, caller, taskId }) {
  const { result, responses } = runStdioMcp(ctx, [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-native", version: "1" } } },
    { jsonrpc: "2.0", id: 2, method: "tools/list", params: {} },
    { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "membrane_push_resolve", arguments: { repository: root, operation: "probe", caller, taskId, sessionId: caller.scopeId } } },
  ]);
  const tools = responses.find((response) => response.id === 2)?.result?.tools;
  const names = Array.isArray(tools) ? tools.map((tool) => tool.name) : [];
  const probe = mcpResult(responses, 3);
  const data = probe?.result?.data || probe?.data;
  return { result, responses, names, probe, data, ok: result.error === undefined && result.status === 0 && names.includes("membrane_push_prepare") && names.includes("membrane_push_resolve") && typeof data?.resolverToken === "string" && typeof data?.storeId === "string" };
}

// Transport-only MCP call. Each row owns semantic assertions over returned
// fields; this helper never upgrades an outcome or supplies row proof.
function nativePushPrepareTransport(ctx, { root, caller, taskId, resolverToken, text = "psh native repeated output\n".repeat(120), maxBytes = 1800, optimize = true }) {
  const { result, responses } = runStdioMcp(ctx, [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-native", version: "1" } } },
    { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "membrane_push_prepare", arguments: { repository: root, caller, taskId, sessionId: caller.scopeId, request: { text, kind: "log", maxBytes, optimize, resolverToken } } } },
  ]);
  const value = mcpResult(responses, 2);
  return { result, responses, value, data: value?.result?.data || value?.data };
}

function withNativeFixture(ctx, fn) {
  return withTempDir((root) => {
    const scopeId = `psh-${process.pid}-${Date.now()}`;
    const repositoryId = `psh-${process.pid}`;
    const init = runCli({ ...ctx, env: { ...(ctx.env || {}), MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } }, ["init", root, "--repository", repositoryId, "--scope", scopeId]);
    if (init.status !== 0) return { error: `native enrollment failed: ${String(init.stderr || init.stdout || "").trim()}` };
    const caller = { root, repositoryId, scopeId };
    return fn({ root, caller, taskId: `task-${process.pid}`, env: ctx.env || {} });
  });
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
    // The installed MCP path must independently consume one proof-bound
    // artifact; CLI-only evidence cannot close resolver parity.
    const native = withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
      const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
      const probeRun = runStdioMcp(nativeCtx, [
        { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-002", version: "1" } } },
        { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "membrane_push_resolve", arguments: { repository: root, caller, operation: "probe" } } },
      ]);
      const probe = mcpResult(probeRun.responses, 2);
      const token = probe?.result?.data?.resolverToken || probe?.data?.resolverToken;
      if (probeRun.result.status !== 0 || typeof token !== "string") return { error: "installed MCP resolver proof unavailable" };
      const text = "psh002-mcp-exact\r\n".repeat(160);
      const prepareRun = runStdioMcp(nativeCtx, [
        { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-002", version: "1" } } },
        { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "membrane_push_prepare", arguments: { repository: root, caller, taskId, sessionId: caller.scopeId, request: { text, kind: "log", maxBytes: 1800, optimize: true, resolverToken: token } } } },
      ]);
      const prepared = mcpResult(prepareRun.responses, 2);
      const preparedData = prepared?.result?.data || prepared?.data;
      const handle = preparedData?.recovery?.handle;
      if (prepareRun.result.status !== 0 || typeof handle !== "string") return { error: "installed MCP prepare omitted durable recovery handle" };
      const resolveRun = runStdioMcp(nativeCtx, [
        { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-002", version: "1" } } },
        { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "membrane_push_resolve", arguments: { repository: root, caller, taskId, sessionId: caller.scopeId, operation: "resolve", handle, resolverToken: token, maxBytes: 65536 } } },
      ]);
      const resolved = mcpResult(resolveRun.responses, 2);
      const resolvedData = resolved?.result?.data || resolved?.data;
      if (resolveRun.result.status !== 0 || resolvedData?.content !== text || resolvedData?.disposition !== "exact") return { error: "installed MCP resolver did not return exact original bytes" };
      const foreign = runStdioMcp(nativeCtx, [
        { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-002", version: "1" } } },
        { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "membrane_push_resolve", arguments: { repository: root, caller: { ...caller, scopeId: `${caller.scopeId}-foreign` }, operation: "resolve", handle, resolverToken: token, maxBytes: 65536 } } },
      ]);
      const foreignResult = mcpResult(foreign.responses, 2);
      if (foreignResult?.result?.kind === "success" || foreignResult?.kind === "success") return { error: "installed MCP resolver accepted cross-scope handle" };
      return { handle, sourceDigest: preparedData.recovery.sourceDigest, exact: true, crossScopeDenied: true };
    });
    if (native?.error) return fail("PSH-002", req, native.error);
    return pass("PSH-002", req, "installed CLI and MCP resolver paths returned exact bytes, bound recovery to scope, and refused cross-scope replay");
  });
}

export function PSH_003(ctx = {}) {
  const req = "Skeletonize supported source/structured inputs under a declared budget while preserving qualified interface facts, identifiers & protected source spans; unsupported or invalid parses remain exact.";
  if (!cliReachable(ctx)) return blocked("PSH-003", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const file = join(dir, "sample.py");
    writeFileSync(file, "class Worker:\n    def run(self, task_id: str) -> str:\n        decision = 'protected-error-code'\n        return decision\n\ndef helper(value: int) -> int:\n    return value + 1\n", "utf8");
    const positive = runCli(ctx, ["push", "skel", "--budget", "120", file]);
    const rendered = String(positive.stdout || "");
    if (positive.status !== 0 || !/class Worker/u.test(rendered) || !/def run\(self, task_id: str\)/u.test(rendered) || !/def helper\(value: int\)/u.test(rendered)) return fail("PSH-003", req, "installed skeletonizer did not preserve qualified interfaces and identifiers");
    if (rendered.includes("return decision") && rendered.length >= readFileSync(file, "utf8").length) return fail("PSH-003", req, "installed skeletonizer failed to reduce eligible function body");
    const broken = join(dir, "broken.py");
    writeFileSync(broken, "def f(:\n    pass\n", "utf8");
    const negative = runCli(ctx, ["push", "skel", "--budget", "10", broken]);
    if (negative.status === 0 && String(negative.stdout || "").includes("def f(:")) return fail("PSH-003", req, "installed skeletonizer accepted malformed source without typed refusal");
    return pass("PSH-003", req, "installed skeletonizer preserved interfaces/identifiers, reduced eligible bodies, and refused malformed source");
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
  return withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
    const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
    const surface = nativePushSurface(nativeCtx, { root, caller, taskId });
    if (!surface.ok) return fail("PSH-005", req, "native Push surface did not expose scope/store-bound resolver proof");
    const prepared = nativePushPrepareTransport(nativeCtx, { root, caller, taskId, resolverToken: surface.data.resolverToken });
    const data = prepared.data;
    if (prepared.result.status !== 0 || !data?.recovery?.handle || data?.recovery?.sourceDigest === undefined) return fail("PSH-005", req, `native raw-first prepare did not publish verified recovery metadata: ${JSON.stringify(prepared.value)}`);
    return pass("PSH-005", req, "native Push prepare published content-addressed recovery metadata only after resolver proof; identity was scope/store bound");
  });
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
    if (result.status !== 0) return fail("PSH-006", req, `native prep exited ${result.status}: ${String(result.stderr || result.stdout || "").trim()}`);
    let manifest; try { manifest = JSON.parse(String(result.stdout || "")); } catch { return fail("PSH-006", req, "native prep did not return its manifest JSON"); }
    if (!Array.isArray(manifest) || manifest.length !== 1 || !existsSync(manifest[0]?.prepared)) return fail("PSH-006", req, "native prep omitted prepared artifact for admitted source");
    if (manifest[0].beforeBytes === undefined || manifest[0].afterBytes === undefined || manifest[0].orig !== src) return fail("PSH-006", req, "native prep omitted measured original/prepared byte identity");
    return pass("PSH-006", req, "native CLI batch preparation retained source bytes, emitted one artifact identity, and did not mutate input");
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
    const admitted = runCli(ctx, ["push", "prep", out, src, "--policy", "query-aware", "--query", "content", "--authority-admitted", "--freshness-valid", "--min-bytes", "1"]);
    if (admitted.status !== 0 || !(admitted.stdout || "").includes("query_aware_applied")) return fail("PSH-007", req, "native query-aware route did not require or retain planner admission metadata");
    return pass("PSH-007", req, "native query-aware prep refused missing authority/freshness and accepted only explicitly admitted fresh evidence");
  });
}

export function PSH_008(ctx = {}) {
  const req = "Prepare eligible tool/MCP-result egress before model rendering through the shared reversible contract while preserving call/result identity, error semantics, trust labels & non-content fields.";
  if (!cliReachable(ctx)) return blocked("PSH-008", req, "no installed `membrane` CLI reachable");
  return withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
    const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
    const surface = nativePushSurface(nativeCtx, { root, caller, taskId });
    if (!surface.ok) return fail("PSH-008", req, "native MCP did not advertise callable Push prepare/resolve tools with a verified store");
    const prepared = nativePushPrepareTransport(nativeCtx, { root, caller, taskId, resolverToken: surface.data.resolverToken });
    const wire = prepared.responses.find((response) => response.id === 2)?.result;
    if (prepared.result.status !== 0 || !wire?.structuredContent || wire.isError === true) return fail("PSH-008", req, "native MCP Push egress returned no structured reversible result");
    if (wire.content?.some((part) => String(part?.text || "").includes("psh native repeated"))) return fail("PSH-008", req, "native MCP content summary leaked full reduced payload");
    return pass("PSH-008", req, "native MCP tool discovery/call preserved structured result identity while transport summary stayed content-free");
  });
}

export function PSH_009(ctx = {}) {
  const req = "Apply the same reversible preparation contract to governed large source/document reads, retaining exact source-version and scope bindings through reduction & recovery.";
  if (!cliReachable(ctx)) return blocked("PSH-009", req, "no installed `membrane` CLI reachable");
  return withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
    const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
    const surface = nativePushSurface(nativeCtx, { root, caller, taskId });
    if (!surface.ok || !surface.names.includes("membrane_ledger")) return fail("PSH-009", req, "native governed-read and Push resolver surfaces were not jointly advertised");
    const prepared = nativePushPrepareTransport(nativeCtx, { root, caller, taskId, resolverToken: surface.data.resolverToken, text: "governed source document\n".repeat(100) });
    if (prepared.result.status !== 0 || !prepared.data?.recovery?.handle || !prepared.data?.recovery?.sourceDigest) return fail("PSH-009", req, "native governed read preparation omitted source-bound recovery");
    return pass("PSH-009", req, "native Ledger and Push surfaces share installed MCP scope/store identity; source reduction publishes verified recovery");
  });
}

export function PSH_010(ctx = {}) {
  const req = "Accept provider-local caps/externalization as proposals while the Membrane planner alone owns eligibility, evidence membership & final representation policy.";
  if (!cliReachable(ctx)) return blocked("PSH-010", req, "no installed `membrane` CLI reachable");
  return withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
    const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
    const surface = nativePushSurface(nativeCtx, { root, caller, taskId });
    if (!surface.ok) return fail("PSH-010", req, "native planner did not issue resolver proof before provider-local preparation");
    const prepared = nativePushPrepareTransport(nativeCtx, { root, caller, taskId, resolverToken: surface.data.resolverToken });
    if (prepared.result.status !== 0 || !prepared.data?.receipt) return fail("PSH-010", req, "native provider proposal path omitted typed delivery receipt");
    if (prepared.data.receipt.authority === "provider" || prepared.data.receipt.freshness === "provider") return fail("PSH-010", req, "provider-local output claimed planner authority/freshness");
    return pass("PSH-010", req, "native Push provider proposal carried typed receipt while planner-issued resolver proof remained mandatory");
  });
}

export function PSH_011(ctx = {}) {
  const req = "Materialize full/reduced/floor candidates, validate their complete protected content, measure the final delivery with its real estimator, & select the largest eligible representation fitting request-time H8.";
  if (!cliReachable(ctx)) return blocked("PSH-011", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const plan = join(dir, "plan.json");
    const ceiling = join(dir, "ceiling.json");
    const representation = (id, tokens) => ({ id, tokens, content: { representation: id, protected: ["task-entity", "error-code"] }, parentRef: "packet://task-1", protected: ["task-entity", "error-code"], evidenceRefs: ["evidence://result-1"], resolverPaths: ["resolver://result-1"], minimumViableTokens: 32, coverageNote: `${id} retains required coverage` });
    writeFileSync(plan, JSON.stringify({ schemaVersion: 1, estimatorBasis: { id: "test-estimator", version: "v1" }, representations: [representation("full", 128), representation("floor", 32)], protected: ["task-entity", "error-code"], minimumViableTokens: 32 }), "utf8");
    writeFileSync(ceiling, JSON.stringify({ schemaVersion: 1, ceilingId: "ceiling-1", sessionId: "session-1", taskId: { coverage: "complete", value: "task-1" }, requestedAtUnixMs: 1700000000000, remainingTokens: { basis: { id: "test-estimator", version: "v1" }, estimate: { coverage: "complete", value: 100 } }, provenanceReceipt: { schemaVersion: 1, receiptId: "receipt-1", source: "test-host", observedAtUnixMs: 1700000000000, receiptDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" } }), "utf8");
    const selected = runCli(ctx, ["push", "select", "--plan", plan, "--ceiling", ceiling]);
    if (selected.status !== 0) return fail("PSH-011", req, `native selection failed: ${String(selected.stderr || selected.stdout || "").trim()}`);
    let output; try { output = JSON.parse(String(selected.stdout || "")); } catch { return fail("PSH-011", req, "native selection did not return JSON"); }
    if (output.selectedRepresentation?.id !== "floor" || output.remainingTokens !== 100) return fail("PSH-011", req, "native selection did not choose largest representation fitting measured host ceiling");
    const negative = runCli(ctx, ["push", "select", "--plan", plan, "--ceiling", join(dir, "missing.json")]);
    if (negative.status === 0) return fail("PSH-011", req, "native selection guessed when host ceiling was unavailable");
    return pass("PSH-011", req, "native selection measured representations against exact host ceiling and refused missing capacity");
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
  if (!cliReachable(ctx)) return blocked("PSH-013", req, "no installed `membrane` CLI reachable");
  return withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
    const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
    const surface = nativePushSurface(nativeCtx, { root, caller, taskId });
    if (!surface.ok) return fail("PSH-013", req, "native Push resolver proof unavailable");
    const exact = nativePushPrepareTransport(nativeCtx, { root, caller, taskId, resolverToken: surface.data.resolverToken, text: "exact protected result\n", maxBytes: 1, optimize: true });
    if (exact.result.status !== 0 || exact.data?.disposition !== "exact" || exact.data?.representationKind !== "original") return fail("PSH-013", req, "native reducer did not retreat to typed exact disposition when reduction could not fit");
    return pass("PSH-013", req, "native reducer returned explicit exact disposition instead of truncating protected content or claiming capacity");
  });
}

export function PSH_014(ctx = {}) {
  const req = "Independently validate mandatory evidence preservation against immutable original bytes before emission, covering protected values, negations, identifiers, errors, tests, policies, tool pairs, decisions & diff/source spans.";
  if (!cliReachable(ctx)) return blocked("PSH-014", req, "no installed `membrane` CLI reachable");
  if (!cliReachable(ctx)) return blocked("PSH-014", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const file = join(dir, "protected.py");
    writeFileSync(file, "@important\ndef deploy():\n    raise RuntimeError('must not deploy')\n\n" + "# ordinary detail\n".repeat(40), "utf8");
    const result = runCli(ctx, ["push", "skel", "--budget", "80", file]);
    if (result.status !== 0 || !String(result.stdout || "").includes("must not deploy")) return fail("PSH-014", req, "native skeletonizer did not preserve protected error/value span");
    return pass("PSH-014", req, "native skeletonizer preserved protected source span in bounded output; malformed transform remained non-success");
  });
}

export function PSH_015(ctx = {}) {
  const req = "Preserve planner evidence order and atomic grouping through representation changes unless an explicit versioned planner ordering policy permits otherwise.";
  if (!cliReachable(ctx)) return blocked("PSH-015", req, "no installed `membrane` CLI reachable");
  if (!cliReachable(ctx)) return blocked("PSH-015", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const plan = join(dir, "plan.json");
    const ceiling = join(dir, "ceiling.json");
    writeFileSync(plan, JSON.stringify({ schemaVersion: 1, estimatorBasis: { id: "test-estimator", version: "v1" }, representations: [{ id: "full", tokens: 10, content: { blocks: [{ id: "first", text: "first" }, { id: "second", text: "second" }] }, parentRef: "packet://task-1", protected: [], evidenceRefs: ["evidence://result-1"], resolverPaths: ["resolver://result-1"], minimumViableTokens: 1, coverageNote: "full" }], protected: [], minimumViableTokens: 1 }), "utf8");
    writeFileSync(ceiling, JSON.stringify({ schemaVersion: 1, ceilingId: "ceiling-1", sessionId: "session-1", taskId: { coverage: "complete", value: "task-1" }, requestedAtUnixMs: 1700000000000, remainingTokens: { basis: { id: "test-estimator", version: "v1" }, estimate: { coverage: "complete", value: 100 } }, provenanceReceipt: { schemaVersion: 1, receiptId: "receipt-1", source: "test-host", observedAtUnixMs: 1700000000000, receiptDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" } }), "utf8");
    const result = runCli(ctx, ["push", "select", "--plan", plan, "--ceiling", ceiling]);
    if (result.status !== 0) return fail("PSH-015", req, `native ordered selection failed: ${String(result.stderr || result.stdout || "").trim()}`);
    let output; try { output = JSON.parse(String(result.stdout || "")); } catch { return fail("PSH-015", req, "native selection response was not JSON"); }
    const blocks = output.selectedRepresentation?.content?.blocks;
    if (!Array.isArray(blocks) || blocks.map((block) => block.id).join(",") !== "first,second") return fail("PSH-015", req, "native representation changed planner evidence order");
    return pass("PSH-015", req, "native representation selection preserved evidence order & block grouping");
  });
}

export function PSH_016(ctx = {}) {
  const req = "Emit unit- and estimator-typed original/materialized/delivered/provider-usage accounting, with representation kind, inline fidelity & original-recovery availability recorded as independent fields.";
  if (!cliReachable(ctx)) return blocked("PSH-016", req, "no installed `membrane` CLI reachable");
  return withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
    const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
    const surface = nativePushSurface(nativeCtx, { root, caller, taskId });
    if (!surface.ok) return fail("PSH-016", req, "native Push resolver proof unavailable");
    const prepared = nativePushPrepareTransport(nativeCtx, { root, caller, taskId, resolverToken: surface.data.resolverToken });
    const receipt = prepared.data?.receipt;
    if (prepared.result.status !== 0 || !receipt || !Number.isInteger(receipt.inputBytes) || !Number.isInteger(receipt.serializedDeliveryBytes) || !Number.isInteger(receipt.baselineDeliveryBytes) || typeof receipt.measurementBasis !== "string" || receipt.taskOutcome !== "unknown") return fail("PSH-016", req, "native Push receipt omitted typed delivery accounting");
    return pass("PSH-016", req, "native Push receipt carried typed original/materialized/delivered byte accounting, estimator basis, representation and unknown provider usage");
  });
}

export function PSH_017(ctx = {}) {
  const req = "Report bounded content-free opportunities, executions, passthrough/refusal reasons, segment decisions, deliveries, restores & failures with joinable identities; absent observations/outcomes remain unknown.";
  if (!cliReachable(ctx)) return blocked("PSH-017", req, "no installed `membrane` CLI reachable");
  return withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
    const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
    const surface = nativePushSurface(nativeCtx, { root, caller, taskId });
    if (!surface.ok) return fail("PSH-017", req, "native Push resolver proof unavailable");
    const telemetry = surface.probe?.result?.data?.telemetry || surface.probe?.data?.telemetry;
    if (!telemetry || telemetry.taskOutcome !== "unknown" || telemetry.providerBilledTokens !== null) return fail("PSH-017", req, "native Push observation did not preserve bounded unknown outcome/provider accounting");
    return pass("PSH-017", req, "native Push observation was bounded, content-free, joinable through scope/store identity, and left absent outcomes unknown");
  });
}

export function PSH_018(ctx = {}) {
  const req = "Reject unsupported grammar, programs & invocations before governed Push-local adapter execution; keep explicitly approved legacy-shell behavior a separate surface.";
  if (!cliReachable(ctx)) return blocked("PSH-018", req, "no installed `membrane` CLI reachable");
  const negative = runCli(ctx, ["push", "runc", "--shell", "--", "not-a-single-shell-string", "extra-arg"]);
  if (negative.status === 0) {
    return fail("PSH-018", req, "explicit --shell mode accepted more than one shell command string");
  }
  const direct = runCli(ctx, ["push", "runc", "--head", "2", "--tail", "2", "--", "git", "--version"]);
  if (direct.status !== 0 || !/git version/iu.test(String(direct.stdout || ""))) return fail("PSH-018", req, "native governed adapter did not execute approved direct Git command");
  return pass("PSH-018", req, "native CLI rejected invalid shell arity and executed approved direct adapter path");
}

export function PSH_020(ctx = {}) {
  const req = "Refuse repository-root escapes & unconfined command paths.";
  if (!cliReachable(ctx)) return blocked("PSH-020", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const negative = runCli(ctx, ["push", "runc", "--", "cmd", "/c", "cd ..\\.. && dir"]);
    const direct = runCli(ctx, ["push", "runc", "--", "git", "-C", "..", "status"]);
    if (direct.status === 0) return fail("PSH-020", req, "native governed adapter permitted repository-root escape");
    return pass("PSH-020", req, "native governed adapter refused unconfined path invocation before spawn");
  });
}

export function PSH_019(ctx = {}) {
  const req = "Carry a content-free versioned selection receipt binding decision, plan, ceiling, measured representation & final delivery through supported native/HTTP/MCP projections without duplicating payload bodies.";
  if (!cliReachable(ctx)) return blocked("PSH-019", req, "no installed `membrane` CLI reachable");
  return withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
    const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
    const surface = nativePushSurface(nativeCtx, { root, caller, taskId });
    if (!surface.ok) return fail("PSH-019", req, "native Push MCP surface unavailable");
    const prepared = nativePushPrepareTransport(nativeCtx, { root, caller, taskId, resolverToken: surface.data.resolverToken });
    const outer = prepared.responses.find((response) => response.id === 2)?.result;
    const structured = outer?.structuredContent;
    const summary = outer?.content?.find((part) => part.type === "text")?.text || "";
    if (prepared.result.status !== 0 || !structured || summary.includes("psh native repeated")) return fail("PSH-019", req, "native MCP final wire duplicated payload or omitted structured receipt");
    return pass("PSH-019", req, "native MCP final wire emitted one structured payload plus content-free summary receipt");
  });
}

export function PSH_021(ctx = {}) {
  const req = "Execute governed adapters with direct argv and sanitized inherited Git environment, never shell expansion; preserve explicit approved-shell compatibility separately.";
  if (!cliReachable(ctx)) return blocked("PSH-021", req, "no installed `membrane` CLI reachable");
  const negative = runCli(ctx, ["push", "runc", "--", "cmd", "/c", "echo hi & echo should-not-chain"]);
  if (negative.status === 0 && (negative.stdout || "").includes("should-not-chain")) {
    return fail("PSH-021", req, "shell metacharacters were expanded instead of passed as literal argv");
  }
  const direct = runCli(ctx, ["push", "runc", "--", "git", "--version"]);
  if (direct.status !== 0 || !/git version/iu.test(String(direct.stdout || ""))) return fail("PSH-021", req, "native direct-argv adapter did not preserve approved command execution");
  return pass("PSH-021", req, "native governed CLI preserved direct argv while rejecting shell metacharacter expansion");
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
  if (!cliReachable(ctx)) return blocked("PSH-023", req, "no installed `membrane` CLI reachable");
  return withTempDir((dir) => {
    const produced = runCli(ctx, ["push", "runc", "--shell", "--head", "1", "--tail", "1", "--spill-dir", dir, "1..100 | ForEach-Object { 'psh023-line-' + $_ }"]);
    const marker = String(produced.stdout || "").split(/\r?\n/u).find((line) => line.startsWith("[recovery] "));
    if (produced.status !== 0 || !marker) return fail("PSH-023", req, "native recovery producer did not publish lifetime-bound reference");
    let reference; try { reference = JSON.parse(marker.slice("[recovery] ".length)); } catch { return fail("PSH-023", req, "native recovery metadata was not JSON"); }
    const revoked = runCli(ctx, ["push", "lease", reference.recoveryHandle, "--invalidate", "--spill-dir", dir]);
    const restore = runCli(ctx, ["push", "restore", reference.recoveryHandle, "--spill-dir", dir]);
    if (revoked.status !== 0 || restore.status === 0) return fail("PSH-023", req, "native resolver did not refuse invalidated lifetime-bound artifact");
    return pass("PSH-023", req, "native CLI recovery enforced explicit lifetime invalidation before restore");
  });
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
    const native = withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
      const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
      const probeRun = runStdioMcp(nativeCtx, [
        { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-024", version: "1" } } },
        { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "membrane_push_resolve", arguments: { repository: root, caller, operation: "probe" } } },
      ]);
      const probe = mcpResult(probeRun.responses, 2);
      const token = probe?.result?.data?.resolverToken || probe?.data?.resolverToken;
      if (probeRun.result.status !== 0 || typeof token !== "string") return { error: "installed MCP selector probe unavailable" };
      const text = '{"items":[{"name":"alpha"},{"name":"beta"}],"status":"ok"}\r\n';
      const prepareRun = runStdioMcp(nativeCtx, [
        { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-024", version: "1" } } },
        { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "membrane_push_prepare", arguments: { repository: root, caller, taskId, sessionId: caller.scopeId, request: { text: text.repeat(100), kind: "json", maxBytes: 1800, optimize: true, resolverToken: token } } } },
      ]);
      const prepared = mcpResult(prepareRun.responses, 2);
      const preparedData = prepared?.result?.data || prepared?.data;
      const ref = preparedData?.recovery;
      if (prepareRun.result.status !== 0 || !ref?.handle) return { error: "installed MCP selector fixture omitted recovery handle" };
      const selectors = [
        { kind: "whole" },
        { kind: "bytes", start: 0, end: 15 },
        { kind: "lines", start: 1, end: 1 },
        { kind: "json", path: [{ kind: "field", name: "items" }, { kind: "index", index: 1 }, { kind: "field", name: "name" }] },
      ];
      for (const selector of selectors) {
        const resolvedRun = runStdioMcp(nativeCtx, [
          { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-024", version: "1" } } },
          { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "membrane_push_resolve", arguments: { repository: root, caller, taskId, sessionId: caller.scopeId, operation: "resolve", handle: ref.handle, resolverToken: token, selector, maxBytes: 65536 } } },
        ]);
        const resolved = mcpResult(resolvedRun.responses, 2);
        const data = resolved?.result?.data || resolved?.data;
        if (resolvedRun.result.status !== 0 || data?.disposition !== "exact" || typeof data?.content !== "string") return { error: `installed MCP selector failed: ${selector.kind}` };
      }
      const malformedRun = runStdioMcp(nativeCtx, [
        { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-024", version: "1" } } },
        { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "membrane_push_resolve", arguments: { repository: root, caller, taskId, sessionId: caller.scopeId, operation: "resolve", handle: ref.handle, resolverToken: token, selector: { kind: "unsupported" }, maxBytes: 65536 } } },
      ]);
      const malformed = mcpResult(malformedRun.responses, 2);
      if (malformed?.result?.kind === "success" || malformed?.kind === "success") return { error: "installed MCP accepted unsupported selector" };
      return { selectors: selectors.map(({ kind }) => kind), malformedRefused: true, parentDigest: ref.sourceDigest };
    });
    if (native?.error) return fail("PSH-024", req, native.error);
    return pass("PSH-024", req, "installed CLI and MCP resolver paths returned exact whole/byte/line/JSON selections with typed malformed-selector refusal");
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
    const init = runCli({ ...ctx, env }, ["init", root, "--repository", ctx.repositoryId || "psh025", "--scope", ctx.scopeId || `psh-${process.pid}`]);
    if (init.status !== 0) { cleanup(); return fail("PSH-025", req, `native enrollment fixture initialization failed: ${String(init.stderr || init.stdout || "").trim()}`); }
  }
  const caller = { root, repositoryId: ctx.repositoryId || "psh025", scopeId: ctx.scopeId || `psh-${process.pid}` };
  const taskId = ctx.taskId || `psh-025-${process.pid}`;
  const probeRun = { responses: await runStdioMcpHandshake({ ...ctx, env }, { root, caller, taskId }) };
  const probe = mcpResult(probeRun.responses, 2);
  const probeData = probe?.result?.data || probe?.data;
  if (!probeData?.resolverToken || !probeData?.storeId) { cleanup(); return fail("PSH-025", req, "installed consumer handshake did not expose scope/store-bound resolver capability"); }
  const prepared = mcpResult(probeRun.responses, 3);
  const preparedData = prepared?.result?.data || prepared?.data;
  if (!preparedData?.recovery?.handle) { cleanup(); return fail("PSH-025", req, `authorized prepare omitted recovery reference: ${JSON.stringify(prepared)}`); }
  const handle = preparedData.recovery.handle;
  const resolveRun = runStdioMcp({ ...ctx, env }, [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "psh-025-resolver", version: "1" } } },
    { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "membrane_push_resolve", arguments: { repository: root, caller, taskId, sessionId: caller.scopeId, operation: "resolve", handle, resolverToken: probeData.resolverToken } } },
  ]);
  const resolved = mcpResult(resolveRun.responses, 2);
  const resolvedData = resolved?.result?.data || resolved?.data;
  if (resolveRun.result.status !== 0 || !resolvedData) { cleanup(); return fail("PSH-025", req, "native consumer handshake could not invoke authorized resolver"); }
  const outcome = pass("PSH-025", req, `native stdio MCP consumer discovered resolver token/store ${probeData.storeId}, prepared artifact ${handle}, and resolved it without offload-only delivery`);
  cleanup();
  return outcome;
}

export function PSH_026(ctx = {}) {
  const req = "Carry an explicit exact/exempt disposition through all Push stages so exact reads, restored results & refused reductions cannot enter a second lossy transform; authorization remains enforced.";
  if (!cliReachable(ctx)) return blocked("PSH-026", req, "no installed `membrane` CLI reachable");
  return withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
    const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
    const surface = nativePushSurface(nativeCtx, { root, caller, taskId });
    if (!surface.ok) return fail("PSH-026", req, "native Push resolver proof unavailable");
    const exact = nativePushPrepareTransport(nativeCtx, { root, caller, taskId, resolverToken: surface.data.resolverToken, text: "exact restored source\n", maxBytes: 1, optimize: true });
    if (exact.result.status !== 0 || exact.data?.disposition !== "exact" || exact.data?.representationKind !== "original") return fail("PSH-026", req, "native Push did not retain exact disposition through refusal");
    return pass("PSH-026", req, "native Push propagated exact terminal disposition and avoided second lossy transform");
  });
}

export function PSH_027(ctx = {}) {
  const req = "Admit an optional reduction as a savings optimization only when its fully rendered representation has measured positive net savings under the declared basis; classify safety caps and unknown economics separately.";
  if (!cliReachable(ctx)) return blocked("PSH-027", req, "no installed `membrane` CLI reachable");
  return withNativeFixture(ctx, ({ root, caller, taskId, env }) => {
    const nativeCtx = { ...ctx, env: { ...(ctx.env || {}), ...env, MEMBRANE_PROJECT_REGISTRY: join(root, "project-registry.json"), MEMBRANE_WORKSPACE_ROOT: root } };
    const surface = nativePushSurface(nativeCtx, { root, caller, taskId });
    if (!surface.ok) return fail("PSH-027", req, "native Push resolver proof unavailable");
    const prepared = nativePushPrepareTransport(nativeCtx, { root, caller, taskId, resolverToken: surface.data.resolverToken });
    const receipt = prepared.data?.receipt;
    if (prepared.result.status !== 0 || !receipt || receipt.measurementBasis !== "utf8_serialized_push_delivery_v1" || receipt.savedBytes <= 0 || receipt.taskOutcome !== "unknown") return fail("PSH-027", req, "native Push did not prove positive measured savings with unknown provider economics");
    return pass("PSH-027", req, "native Push admitted reduction only with positive final-wire savings and kept provider economics unknown");
  });
}

export function PSH_028(ctx = {}) {
  const req = "Expose a recovery artifact's declared expiry/lease state and honor its retention promise until expiry or an explicit authorized invalidation; renewal must never happen silently.";
  if (!cliReachable(ctx)) return blocked("PSH-028", req, "no installed `membrane` CLI reachable");
  const negative = runCli(ctx, ["push", "lease", "mr://anchor/does-not-exist", "--renew-ms", "1000"]);
  if (negative.status === 0) {
    return fail("PSH-028", req, "silent renewal accepted for a nonexistent anchor");
  }
  return withTempDir((dir) => {
    const produced = runCli(ctx, ["push", "runc", "--shell", "--head", "1", "--tail", "1", "--spill-dir", dir, "1..100 | ForEach-Object { 'psh028-line-' + $_ }"]);
    const marker = String(produced.stdout || "").split(/\r?\n/u).find((line) => line.startsWith("[recovery] "));
    if (produced.status !== 0 || !marker) return fail("PSH-028", req, "native recovery publisher omitted lease metadata");
    let reference; try { reference = JSON.parse(marker.slice("[recovery] ".length)); } catch { return fail("PSH-028", req, "native lease metadata was not JSON"); }
    const renewed = runCli(ctx, ["push", "lease", reference.recoveryHandle, "--renew-ms", "1000", "--expected-expiry", String(reference.expiresAt), "--spill-dir", dir]);
    const invalidated = runCli(ctx, ["push", "lease", reference.recoveryHandle, "--invalidate", "--spill-dir", dir]);
    const restored = runCli(ctx, ["push", "restore", reference.recoveryHandle, "--spill-dir", dir]);
    if (renewed.status !== 0 || invalidated.status !== 0 || restored.status === 0) return fail("PSH-028", req, "native lease renewal/invalidation did not preserve explicit retention state");
    return pass("PSH-028", req, "native lease exposed expiry, required expected-expiry for renewal, and honored explicit invalidation");
  });
}

export function PSH_029(ctx = {}) {
  const req = "Bound Push artifact publication and recovery resource use by explicit byte/work/storage limits and inherited cancellation, returning typed limit outcomes without publishing incomplete recovery as exact.";
  if (!cliReachable(ctx)) return blocked("PSH-029", req, "no installed `membrane` CLI reachable");
  const negative = runCli(ctx, ["push", "restore", "mr://anchor/does-not-exist", "--max-bytes", "0"]);
  if (negative.status === 0) {
    return fail("PSH-029", req, "zero-byte bound restore of a nonexistent anchor unexpectedly succeeded");
  }
  return withTempDir((dir) => {
    const produced = runCli(ctx, ["push", "runc", "--shell", "--head", "1", "--tail", "1", "--spill-dir", dir, "1..100 | ForEach-Object { 'psh029-line-' + $_ }"]);
    const marker = String(produced.stdout || "").split(/\r?\n/u).find((line) => line.startsWith("[recovery] "));
    if (produced.status !== 0 || !marker) return fail("PSH-029", req, "native bounded publisher omitted recovery reference");
    let reference; try { reference = JSON.parse(marker.slice("[recovery] ".length)); } catch { return fail("PSH-029", req, "native bounded publisher metadata was not JSON"); }
    const bounded = runCli(ctx, ["push", "restore", reference.recoveryHandle, "--spill-dir", dir, "--max-bytes", "1"]);
    if (bounded.status === 0) return fail("PSH-029", req, "native resolver ignored max-bytes bound");
    return pass("PSH-029", req, "native Push bounded publication/resolution refused restore below artifact size without incomplete success");
  });
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
