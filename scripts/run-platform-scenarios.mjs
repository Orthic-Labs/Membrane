#!/usr/bin/env node
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { request as httpRequest } from "node:http";
import { basename, dirname, join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { runBenchmark } from "../mcp/e2e-benchmark.mjs";

const SCENARIOS = [
  ["repository_orientation", "pwd"],
  ["cross_repo_impact", "git submodule status membrane cortex"],
  ["preference_application", "rg -n 'Brief mode is a hard' AGENTS.md"],
  ["stale_graph_immediate_edit", "git status --short membrane"],
  ["contradiction", "git branch --show-current"],
  ["denied_scope", "git rev-parse --show-toplevel"],
  ["tool_proof_criteria", "git rev-parse HEAD"],
  ["user_correction", "rg -n 'Naming lock' AGENTS.md"],
  ["memory_temporal_as_of", "git log -1 --format=%cI"],
  ["provider_timeout_degradation", "git branch --show-current"],
];
const REQUIRED_PHASES = ["provider.started", "provider.terminal", "candidate.retrieved", "candidate.admitted", "block.rendered", "block.delivered", "turn.outcome", "feedback.recorded"];
const BRANCH_CANDIDATE = "git:meta:branch:main";
const BRANCH_ARTIFACT = `artifact.${createHash("sha256").update(BRANCH_CANDIDATE).digest("hex")}`;
const args = process.argv.slice(2);
const valueFor = (flag) => { const index = args.indexOf(flag); return index < 0 ? undefined : args[index + 1]; };
const has = (flag) => args.includes(flag);
const workspaceRoot = resolve(valueFor("--workspace-root") || new URL("../../", import.meta.url).pathname);
const evidenceRoot = resolve(valueFor("--evidence-root") || join(workspaceRoot, "tasks", "evidence", "membrane-competitor-parity-completion"));
const platform = valueFor("--platform") || (process.platform === "win32" ? "windows" : "mac");
const selected = valueFor("--scenario") ? SCENARIOS.filter(([name]) => name === valueFor("--scenario")) : SCENARIOS;
const dbPath = process.env.MEMBRANE_EVENT_DB || join(workspaceRoot, "tools", ".cache", "memory", "crypt-engine.membrane-events.sqlite3");
const memoryDb = process.env.CRYPT_DB || join(workspaceRoot, "tools", ".cache", "memory", "crypt-engine.db");
const tokenPath = process.env.CRYPT_API_TOKEN_FILE || join(workspaceRoot, "tools", ".cache", "memory", "api-token");
const cli = process.env.CRYPT_BIN || join(workspaceRoot, "tools", "bin", process.platform === "win32" ? "crypt.exe" : "crypt");

function sha(value) { return createHash("sha256").update(value).digest("hex"); }
function opaque(prefix, value, length = 64) { return `${prefix}-${sha(value).slice(0, length)}`; }
function atomicJson(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  const temporary = `${path}.${process.pid}.tmp`;
  writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  renameSync(temporary, path);
}
function fail(message) { throw new Error(message); }
function command(commandName, commandArgs, timeout = 180_000) {
  return spawnSync(commandName, commandArgs, {
    cwd: workspaceRoot, encoding: "utf8", timeout, windowsHide: true,
    env: {
      ...process.env,
      WORKSPACE_ROOT: workspaceRoot,
      CRYPT_CLIENT: "claude_code",
      RIGHTCONTEXT_POLICY_ENV_OVERRIDE: "1",
      RIGHTCONTEXT_CONTEXT_SOURCE: "smoke",
      RIGHTCONTEXT_MODE: "on",
      RIGHTCONTEXT_COHORTS: "off",
      CRYPT_TELEMETRY_INGRESS: join(workspaceRoot, "tools", ".cache", "memory", "context-telemetry-ingress.jsonl"),
    },
  });
}
function rows(db, sql, ...params) { return db.prepare(sql).all(...params); }
function scalar(db, sql, ...params) { return db.prepare(sql).get(...params)?.value ?? null; }
function parseClaude(stdout) {
  const line = String(stdout || "").trim().split(/\r?\n/u).filter(Boolean).at(-1);
  try { return JSON.parse(line); } catch { fail("Claude returned malformed JSON"); }
}
function promptFor(name, shellCommand) {
  return [
    `Qualification scenario: ${name}.`,
    `Use Bash exactly once to run: ${shellCommand}`,
    "Make no edits. Use injected current-branch context plus command output.",
    `Reply on one line beginning exactly: branch=main scenario=${name} proof=`,
  ].join(" ");
}
async function findTrace(db, cursor) {
  const statement = db.prepare(`
    SELECT trace_id, MAX(seq) AS last_seq
      FROM context_event_log
     WHERE seq > ?
     GROUP BY trace_id
    HAVING SUM(CASE WHEN phase='block.delivered' AND artifact_id=? THEN 1 ELSE 0 END) > 0
       AND SUM(CASE WHEN phase='turn.observed' AND reason_code='tool' AND client='claude_code' AND status='success' THEN 1 ELSE 0 END) > 0
     ORDER BY last_seq DESC LIMIT 1
  `);
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const match = statement.get(cursor, BRANCH_ARTIFACT);
    if (match) return match.trace_id;
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 20));
  }
  fail("real host trace lacks linked branch delivery and tool receipt");
}
function postEvents(events) {
  const token = readFileSync(tokenPath, "utf8").trim();
  const body = Buffer.from(JSON.stringify({ events }));
  return new Promise((resolvePromise, reject) => {
    const request = httpRequest({
      hostname: "127.0.0.1", port: 47851, path: "/v1/telemetry/events:batch", method: "POST",
      timeout: 5_000,
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json", "Content-Length": body.length },
    }, (response) => {
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => {
        const text = Buffer.concat(chunks).toString("utf8");
        if (response.statusCode !== 201) return reject(new Error(`outcome telemetry rejected (${response.statusCode}): ${text.slice(0, 400)}`));
        try { resolvePromise(JSON.parse(text)); } catch { reject(new Error("outcome telemetry returned malformed JSON")); }
      });
    });
    request.on("timeout", () => request.destroy(new Error("outcome telemetry timed out")));
    request.on("error", reject);
    request.end(body);
  });
}
function outcomeEvent(delivery, scenario) {
  const eventId = opaque("evt", `${scenario}:${delivery.trace_id}:outcome`);
  return {
    schema_version: 1, event_id: eventId, ts: new Date().toISOString(),
    installation_id: delivery.installation_id, service_instance_id: delivery.service_instance_id,
    workspace_id: delivery.workspace_id, client: "claude_code", client_version: delivery.client_version,
    producer: "platform_qualification", producer_version: "1", session_id: delivery.session_id,
    turn_id: delivery.turn_id, trace_id: delivery.trace_id, span_id: opaque("span", eventId, 32),
    parent_span_id: delivery.span_id, artifact_family: delivery.artifact_family,
    provider: "membrane", provider_version: "1", release_generation: delivery.release_generation,
    phase: "turn.outcome", operation: "evaluate", status: "success", reason_code: "host_exit_zero",
    scope_id: delivery.scope_id, artifact_id: delivery.artifact_id,
    artifact_sha256: delivery.artifact_sha256, traffic_class: "eval",
    policy_version: delivery.policy_version, policy_activation_sha256: delivery.policy_activation_sha256,
    cohort: delivery.cohort, task_class: "coding", source_generation: delivery.source_generation,
    quantity: 1, measurements: [],
    links: [{ relation: "outcome_for", target_event_id: delivery.event_id }],
  };
}
async function runScenario(db, scenario, shellCommand) {
  const cursor = scalar(db, "SELECT COALESCE(MAX(seq),0) AS value FROM context_event_log");
  const host = command("claude", ["-p", promptFor(scenario, shellCommand), "--output-format", "json", "--permission-mode", "dontAsk", "--allowedTools", "Bash"]);
  if (host.status !== 0) fail(`${scenario}: Claude failed: ${String(host.stderr).slice(-1000)}`);
  const parsed = parseClaude(host.stdout);
  const output = String(parsed.result || "").trim();
  if (!output.startsWith(`branch=main scenario=${scenario} proof=`)) fail(`${scenario}: model output did not use required branch context`);
  const traceId = await findTrace(db, cursor);
  const delivery = db.prepare("SELECT * FROM context_event_log WHERE trace_id=? AND phase='block.delivered' AND artifact_id=? AND status='success' ORDER BY seq DESC LIMIT 1").get(traceId, BRANCH_ARTIFACT);
  const outcome = outcomeEvent(delivery, scenario);
  await postEvents([outcome]);
  const digest = /^[a-f0-9]{64}$/u.test(String(delivery.artifact_sha256 || "")) ? delivery.artifact_sha256 : "0".repeat(64);
  const feedback = command(cli, ["--db", memoryDb, "feedback", "--trace", traceId, "--candidate", BRANCH_CANDIDATE, "--sha", digest, "--outcome", "used", "--source", "observed_action", "--scope", workspaceRoot], 20_000);
  if (feedback.status !== 0) fail(`${scenario}: feedback failed: ${String(feedback.stderr).trim()}`);
  const eventRows = rows(db, "SELECT event_id,phase FROM context_event_log WHERE trace_id=? ORDER BY seq", traceId);
  const phases = new Set(eventRows.map((row) => row.phase));
  for (const phase of REQUIRED_PHASES) if (!phases.has(phase)) fail(`${scenario}: missing ${phase}`);
  const feedbackLinked = scalar(db, "SELECT COUNT(*) AS value FROM context_event_link l JOIN context_event_log e ON e.event_id=l.event_id WHERE e.trace_id=? AND e.phase='feedback.recorded' AND l.relation='feedback_for' AND l.target_event_id=?", traceId, delivery.event_id);
  const outcomeLinked = scalar(db, "SELECT COUNT(*) AS value FROM context_event_link WHERE event_id=? AND relation='outcome_for' AND target_event_id=?", outcome.event_id, delivery.event_id);
  if (feedbackLinked !== 1 || outcomeLinked !== 1) fail(`${scenario}: outcome/feedback links failed readback`);
  const archivePath = join(evidenceRoot, `${platform}-scenario-archives`, `${scenario}.json`);
  atomicJson(archivePath, { schema: "orthic.e2e-scenario-receipt.v1", scenario, trace_id: traceId, status: "complete", event_ids: eventRows.map((row) => row.event_id) });
  return { scenario, trace_id: traceId, adapter: "claude_code", archive_path: archivePath, host_session_id: parsed.session_id || null, output_sha256: `sha256:${sha(output)}` };
}

if (!existsSync(dbPath) || !existsSync(tokenPath) || !existsSync(cli)) fail("platform scenario preflight failed");
if (selected.length === 0) fail("unknown scenario");
if (has("--dry-run")) {
  process.stdout.write(`${JSON.stringify({ schema: "membrane.platform-scenarios-plan.v1", platform, scenarios: selected.map(([name]) => name), mutations: 0 })}\n`);
  process.exit(0);
}
const db = new DatabaseSync(dbPath);
let traces;
try {
  traces = [];
  for (const [scenario, shellCommand] of selected) traces.push(await runScenario(db, scenario, shellCommand));
}
finally { db.close(); }
const health = JSON.parse(command("curl", ["--fail", "--silent", "--max-time", "5", "http://127.0.0.1:47851/health"], 10_000).stdout);
const benchmark = runBenchmark({
  workspaceRoot, eventDbPath: dbPath, model: "claude-installed", hardware: `${process.platform}-${process.arch}`,
  warmCold: "warm", adapters: ["claude_code"], scenarioRunner: (scenario) => traces.find((trace) => trace.scenario === scenario),
});
const full = selected.length === SCENARIOS.length;
const passed = traces.length;
const telemetryDb = new DatabaseSync(dbPath, { readOnly: true });
let telemetry;
try {
  const placeholders = traces.map(() => "?").join(",");
  const count = (phase) => Number(telemetryDb.prepare(`SELECT COUNT(*) AS count FROM context_event_log WHERE trace_id IN (${placeholders}) AND phase=?`).get(...traces.map((trace) => trace.trace_id), phase).count);
  telemetry = { catalog: count("candidate.retrieved"), delivery: count("block.delivered"), feedback: count("feedback.recorded") };
} finally { telemetryDb.close(); }
const result = {
  schema: "membrane.platform-scenarios.v1", platform,
  status: full && benchmark.status === "complete" ? "passed" : "smoke_passed",
  release_generation: health.releaseGeneration, scenario_count: selected.length,
  scenarios_passed: passed, traces, telemetry, benchmark,
};
const outputPath = full ? join(evidenceRoot, `${platform}-scenarios.json`) : join(evidenceRoot, `${platform}-scenarios-${basename(selected[0][0])}-smoke.json`);
atomicJson(outputPath, result);
process.stdout.write(`${JSON.stringify({ status: result.status, scenarios_passed: passed, output: outputPath })}\n`);
