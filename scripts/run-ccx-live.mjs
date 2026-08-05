#!/usr/bin/env node
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";

const membraneRoot = resolve(dirname(new URL(import.meta.url).pathname), "..");
const root = resolve(membraneRoot, "..");
const evidenceRoot = join(root, "tasks", "evidence", "membrane-competitor-parity-completion");
const ingressPath = join(root, "tools", ".cache", "memory", "context-telemetry-ingress.jsonl");
const profileSettings = join(homedir(), "ClaudeProfiles", "claudecodex-profile", "claude-config", "settings.json");
const sourceSync = join(root, "claudecodeX", "mac", "sync-claude-profile.py");
const installedSync = join(homedir(), ".local", "share", "claudecodex", "mac", "sync-claude-profile.py");
const sourceLauncher = join(root, "claudecodeX", "bin", "ccx");
const installedLauncher = join(homedir(), "bin", "ccx");
const contextHook = join(root, "forge", "hooks", "claude-code", "context.js");
const sha256 = (value) => createHash("sha256").update(value).digest("hex");
const fileHash = (path) => sha256(readFileSync(path));
const parseLines = (text) => text.split(/\r?\n/).filter(Boolean).flatMap((line) => {
  try { return [JSON.parse(line)]; } catch { return []; }
});
const observableEvents = (lines) => lines.flatMap((row) => Array.isArray(row.observable_events) ? row.observable_events : []);
const atomicJson = (path, value) => {
  const temp = `${path}.${process.pid}.tmp`;
  writeFileSync(temp, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  renameSync(temp, path);
};

for (const path of [ingressPath, profileSettings, sourceSync, installedSync, sourceLauncher, installedLauncher, contextHook]) {
  if (!existsSync(path)) throw new Error(`required ccx path missing: ${path}`);
}
const healthRun = spawnSync("curl", ["-fsS", "--max-time", "5", "http://127.0.0.1:8801/health"], { encoding: "utf8" });
let health = null;
try { health = JSON.parse(healthRun.stdout || ""); } catch { /* fail below */ }
const profile = JSON.parse(readFileSync(profileSettings, "utf8"));
const hookCommands = Object.values(profile.hooks || {}).flatMap((groups) => groups || []).flatMap((group) => group.hooks || []).map((row) => String(row.command || ""));
const profileCurrent = hookCommands.some((command) => command.includes("context.js"))
  && hookCommands.some((command) => command.includes("tool-receipt.js"));
const before = readFileSync(ingressPath, "utf8").split(/\r?\n/).filter(Boolean).length;
const live = spawnSync(
  installedLauncher,
  [
    "-p", "--model", "sonnet", "--output-format", "stream-json", "--verbose",
    "--dangerously-skip-permissions", "--allowedTools", "Bash", "--max-turns", "3",
    "Use Bash exactly once to run printf ccx-tool-ok. Then reply exactly CCX-LIVE-OK.",
  ],
  { cwd: root, encoding: "utf8", timeout: 120_000, env: { ...process.env } },
);
const stream = parseLines(live.stdout || "");
const terminal = [...stream].reverse().find((row) => row.type === "result");
const toolUse = stream.find((row) => row.type === "assistant" && row.message?.content?.some?.((block) => block.type === "tool_use"));
const toolBlock = toolUse?.message?.content?.find?.((block) => block.type === "tool_use");
const toolResult = stream.find((row) => row.type === "user" && row.message?.content?.some?.((block) => block.type === "tool_result"));
const sessionId = terminal?.session_id || toolUse?.session_id || null;
const newIngress = readFileSync(ingressPath, "utf8").split(/\r?\n/).filter(Boolean).slice(before);
const linkedEvents = observableEvents(parseLines(newIngress.join("\n"))).filter((event) => event.session_id === sessionId);
const packet = linkedEvents.find((event) => event.client_id === "ccx" && event.event_type === "packet_delivered");
const receipt = linkedEvents.find((event) => event.client_id === "ccx" && event.event_type === "tool_receipt" && event.trace_id === toolBlock?.id);

const degradedDir = mkdtempSync(join(tmpdir(), "ccx-degraded-"));
const degradedIngress = join(degradedDir, "events.jsonl");
const degraded = spawnSync(process.execPath, [contextHook], {
  cwd: root,
  input: JSON.stringify({
    hook_event_name: "UserPromptSubmit",
    session_id: "ccx-degraded-session",
    prompt: "degradation probe",
    cwd: root,
  }),
  encoding: "utf8",
  timeout: 10_000,
  env: {
    ...process.env,
    ANTHROPIC_BASE_URL: "http://127.0.0.1:8801",
    MEMBRANE_CONTEXT_CLIENT: join(degradedDir, "missing-client.mjs"),
    CRYPT_TELEMETRY_INGRESS: degradedIngress,
  },
});
const degradedOutput = parseLines(degraded.stdout || "")[0]?.hookSpecificOutput?.additionalContext || "";
const degradedEvents = existsSync(degradedIngress) ? observableEvents(parseLines(readFileSync(degradedIngress, "utf8"))) : [];
const degradedEvent = degradedEvents.find((event) => event.client_id === "ccx" && event.event_type === "packet_delivered");

const result = {
  schema: "orthic.ccx-live.v1",
  generated_at: new Date().toISOString(),
  passed: Boolean(
    healthRun.status === 0 && health?.status === "ok"
    && profileCurrent
    && fileHash(sourceSync) === fileHash(installedSync)
    && fileHash(sourceLauncher) === fileHash(installedLauncher)
    && live.status === 0 && terminal?.is_error === false && terminal?.result === "CCX-LIVE-OK"
    && toolBlock?.name === "Bash" && toolBlock?.input?.command === "printf ccx-tool-ok"
    && toolResult?.message?.content?.some?.((block) => block.type === "tool_result" && block.content === "ccx-tool-ok" && block.is_error === false)
    && packet && packet.completeness?.receipt === true
    && receipt && receipt.completeness?.receipt === true
    && degraded.status === 0 && degradedOutput.includes("Membrane: degraded")
    && degradedEvent && degradedEvent.completeness?.packet === false
  ),
  gateway: {
    url: "http://127.0.0.1:8801",
    status: health?.status || null,
    model: "MiniMax-M3",
    route: "sonnet",
  },
  identity: {
    source_sync_sha256: fileHash(sourceSync),
    installed_sync_sha256: fileHash(installedSync),
    source_launcher_sha256: fileHash(sourceLauncher),
    installed_launcher_sha256: fileHash(installedLauncher),
    profile: basename(profileSettings),
    profile_current: profileCurrent,
  },
  live_session: {
    session_id: sessionId,
    status: live.status,
    terminal_error: terminal?.is_error ?? null,
    response_sha256: terminal?.result ? sha256(terminal.result) : null,
    tool_call_id: toolBlock?.id || null,
    tool_output_sha256: toolResult ? sha256(JSON.stringify(toolResult.message?.content || [])) : null,
    packet_event_id: packet?.event_id || null,
    receipt_event_id: receipt?.event_id || null,
    automatic_injection: Boolean(packet),
    tool_receipt: Boolean(receipt),
  },
  degradation: {
    status: degraded.status,
    visible: degradedOutput.includes("Membrane: degraded"),
    event_id: degradedEvent?.event_id || null,
    packet_complete: degradedEvent?.completeness?.packet ?? null,
  },
};
atomicJson(join(evidenceRoot, "ccx.json"), result);
process.stdout.write(`${JSON.stringify(result)}\n`);
if (!result.passed) process.exitCode = 1;
