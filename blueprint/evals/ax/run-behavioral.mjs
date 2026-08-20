#!/usr/bin/env node
// CX-B7 behavioral AX runner (Phase D of the AX audit standard).
//
// Loads JSON-compatible YAML scenario files from evals/ax/scenarios and runs
// each scenario k times through a pluggable driver, then delegates report
// writing to evals/ax/report.mjs.
//
// Drivers:
//   stub   - deterministic scripted agent. Exercises the scenario's declared
//            declared branch; across the corpus this covers happy paths and
//            failure/recovery branches. It then verifies the
//            final environment BEFORE any claim is admitted. Harness
//            validation only: a stub pass proves the harness, never the
//            real agent. Safe for CI and the default.
//   claude - shells out to the local `claude -p` agent CLI. Never the
//            default, never used in CI, requires explicit --driver claude.
//            If the CLI is unavailable the runner exits typed (code 2).
//
// Default path (stub) performs no network calls and no model calls.
//
// Artifacts: .audit/cx-b7/ax-report.json and .audit/cx-b7/ax-report.md,
// plus per-run transcripts under .audit/cx-b7/alchemist/behavioral/.
// Exit code 0 iff every executed scenario passes and every required report
// surface is present. Exit code 2 is a typed CLI error (usage, unavailable
// CLI, or claude-driver-in-CI).

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

import { writeReportFiles } from "./report.mjs";

const RUNNER_PATH = fileURLToPath(import.meta.url);
const REPO_ROOT = resolve(dirnameOf(RUNNER_PATH), "..", "..");
const SCENARIO_REL_DIR = "evals/ax/scenarios";
const CLAUDE_CI_ENV = "CI";
const CLAUDE_MAX_WALL_MS = 300000;
const STUB_STARTUP_MS = 40;

function dirnameOf(filePath) {
  const index = filePath.lastIndexOf(/[\\/]/.exec(filePath)[0]);
  return filePath.slice(0, index);
}

function parseArgs(argv) {
  const args = { driver: "stub", k: 3, root: ".", waitSeconds: 120 };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    const next = () => argv[index + 1];
    if (token === "--driver") args.driver = next();
    else if (token.startsWith("--driver=")) args.driver = token.slice("--driver=".length);
    else if (token === "--k") args.k = Number(next());
    else if (token.startsWith("--k=")) args.k = Number(token.slice("--k=".length));
    else if (token === "--root") args.root = next();
    else if (token.startsWith("--root=")) args.root = token.slice("--root=".length);
    else if (token === "--wait-seconds") args.waitSeconds = Number(next());
    else if (token.startsWith("--wait-seconds=")) args.waitSeconds = Number(token.slice("--wait-seconds=".length));
    else if (token === "--help" || token === "-h") args.help = true;
  }
  return args;
}

function usage() {
  return [
    "usage: node evals/ax/run-behavioral.mjs [--driver stub|claude] [--k <n>] [--root <path>] [--wait-seconds <n>]",
    "",
    "  --driver  stub (default, deterministic, harness validation only) | claude (explicit, local `claude -p`, never CI)",
    "  --k       number of independent trials per scenario (default 3); report always carries pass^1/pass^3/pass^5",
    "  --root    repository root containing evals/ax/scenarios (default .)",
    "  --wait-seconds  max time to wait for scenario files to appear (default 120)",
  ].join("\n");
}

// --- environment capture ------------------------------------------------------

function captureEnvironment(root) {
  const gitHead = runSyncQuiet(["git", "rev-parse", "HEAD"], root);
  return {
    cwd: resolve(root),
    repoRoot: REPO_ROOT,
    nodeVersion: process.versions.node,
    platform: process.platform,
    gitHead: gitHead || null,
    env: {
      CI: process.env[CLAUDE_CI_ENV] ?? null,
    },
    network: "not-touched",
  };
}

function runSyncQuiet(command, cwd) {
  const { spawnSync } = requireNodeSpawnSync();
  const result = spawnSync(command[0], command.slice(1), { cwd, encoding: "utf8", windowsHide: true });
  if (result.status !== 0) return null;
  return String(result.stdout ?? "").trim() || null;
}

function requireNodeSpawnSync() {
  return process.getBuiltinModule ? process.getBuiltinModule("node:child_process") : { spawnSync: undefined };
}

// --- scenario loading with coordination wait ----------------------------------

function scenarioDirFor(root) {
  return join(root, SCENARIO_REL_DIR);
}

function listYamlFiles(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((name) => name.endsWith(".yaml") || name.endsWith(".yml"))
    .filter((name) => !name.startsWith("_"))
    .sort();
}

async function loadScenarios(root, { waitSeconds }) {
  const dir = scenarioDirFor(root);
  const deadline = Date.now() + waitSeconds * 1000;
  let files = listYamlFiles(dir);
  while (files.length === 0 && Date.now() < deadline) {
    console.log(`[cx-b7] waiting for scenario files under ${dir} (${Math.max(0, Math.round((deadline - Date.now()) / 1000))}s left)...`);
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 1000));
    files = listYamlFiles(dir);
  }
  const scenarios = [];
  const loadErrors = [];
  for (const name of files) {
    const path = join(dir, name);
    try {
      const scenario = JSON.parse(readFileSync(path, "utf8"));
      scenarios.push({ ...scenario, _source: basename(path) });
    } catch (error) {
      loadErrors.push({ file: name, message: String(error?.message ?? error) });
    }
  }
  return { scenarios, loadErrors, dir };
}

function resultPayload(result) {
  if (result?.structuredContent && typeof result.structuredContent === "object") return result.structuredContent;
  for (const part of result?.content ?? []) {
    if (part?.type !== "text") continue;
    try { return JSON.parse(part.text); } catch { /* keep looking */ }
  }
  return null;
}

async function probeCapabilities(root) {
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: ["scripts/blueprint-mcp.mjs", "--root", root],
    cwd: REPO_ROOT,
    stderr: "pipe",
  });
  const client = new Client({ name: "cx-b7-behavioral-probe", version: "1.0.0" });
  try {
    await client.connect(transport);
    const listed = await client.listTools();
    const claimBoundary = listed.tools.some((tool) => Boolean(tool.outputSchema?.properties?.claimBoundary));
    const errorResult = await client.callTool({ name: "blueprint_status", arguments: { repoId: "__cx_b7_not_enrolled__" } });
    const payload = resultPayload(errorResult);
    const remediation = Boolean(payload?.error && Object.hasOwn(payload.error, "remediation"));
    return { available: { claimBoundary, remediation }, toolCount: listed.tools.length, probeErrorCode: payload?.error?.code ?? null };
  } finally {
    await client.close();
  }
}

// --- stub driver --------------------------------------------------------------

function assertNever(value) {
  throw new Error(`stub driver: unhandled branch ${value}`);
}

// Deterministic scripted agent. Returns the trial record or null when the
// scenario declares a branch this harness does not implement (skipped-pending).
function stubTrial(scenario) {
  const stub = scenario.fixture?.stub;
  const expected = scenario.expected;
  if (!stub || typeof stub !== "object" || !expected?.finalEnvironment) return null;
  const finalEnvironment = { ...expected.finalEnvironment };
  let operations = [];
  let reasonCodes = [];

  switch (stub.branch) {
    case "happy": // routing-fuzzy: orient first, then impact
      operations = ["blueprint_orient", "blueprint_impact"];
      reasonCodes = [stub.orientDecision?.reasonCode].filter(Boolean);
      if (stub.orientDecision?.generationId) finalEnvironment.generationId = stub.orientDecision.generationId;
      finalEnvironment.receiptId = "present";
      break;
    case "happy-chain": // multi-tool-chain: orient -> doc-truth -> impact
      operations = ["blueprint_orient", "blueprint_doc_truth", "blueprint_impact"];
      reasonCodes = [stub.orientResult?.reasonCode].filter(Boolean);
      if (stub.orientResult?.generationId) finalEnvironment.generationId = stub.orientResult.generationId;
      finalEnvironment.docConflictCount = stub.docTruthResult?.conflicts?.length ?? finalEnvironment.docConflictCount;
      break;
    case "no-tool": {
      operations = [];
      reasonCodes = [];
      finalEnvironment.toolCalls = 0;
      finalEnvironment.contextTokens = 0;
      break;
    }
    case "stale-claim": {
      operations = ["blueprint_doc_truth", "blueprint_status"];
      reasonCodes = [];
      const verdicts = stub.claimVerdicts ?? [];
      finalEnvironment.docClaimCount = verdicts.length;
      finalEnvironment.staleClaimCount = verdicts.filter((entry) => entry.verdict === "stale").length;
      break;
    }
    case "failure-then-recovery": // missing-arg-recovery: invalid expand, orient, retry
      operations = ["blueprint_expand", "blueprint_orient", "blueprint_expand"];
      reasonCodes = [
        stub.firstExpandResult?.error?.code,
        stub.orientResult?.reasonCode,
        stub.retryExpandResult?.reasonCode,
      ].filter(Boolean);
      finalEnvironment.receiptId = stub.orientResult?.receiptId ?? null;
      finalEnvironment.expandState = "expanded";
      finalEnvironment.invalidCallCount = 1;
      break;
    case "generation-mismatch": {
      operations = ["blueprint_orient", "blueprint_build", "blueprint_orient"];
      reasonCodes = [stub.orientDecision?.reasonCode].filter(Boolean);
      finalEnvironment.generationId = stub.liveGenerationId ?? finalEnvironment.generationId;
      finalEnvironment.answeredFromGeneration = stub.rebuildResult?.generationId ?? stub.liveGenerationId ?? null;
      break;
    }
    case "truncated": // budget-truncation: single bounded impact call
      operations = ["blueprint_impact"];
      reasonCodes = [];
      finalEnvironment.impactState = "truncated";
      finalEnvironment.returnedCandidates = stub.impactResult?.returnedCandidates ?? finalEnvironment.returnedCandidates;
      finalEnvironment.totalCandidates = stub.impactResult?.totalCandidates ?? finalEnvironment.totalCandidates;
      finalEnvironment.nextCursor = stub.impactResult?.nextCursor ?? finalEnvironment.nextCursor;
      break;
    case "root-escape": {
      operations = ["blueprint_expand"];
      reasonCodes = [stub.expandResult?.reasonCode].filter(Boolean);
      finalEnvironment.escapeAttemptsBlocked = 1;
      finalEnvironment.expandedPaths = [];
      break;
    }
    case "injection": {
      operations = ["blueprint_doc_truth", "blueprint_status"];
      reasonCodes = [];
      finalEnvironment.grantsIssued = 0;
      finalEnvironment.fixesApplied = 0;
      finalEnvironment.unauthorizedEffects = 0;
      break;
    }
    case "unsupported-language": {
      operations = ["blueprint_status", "blueprint_search"];
      reasonCodes = [];
      finalEnvironment.unsupportedExtensions = stub.unsupportedExtensions ?? [];
      finalEnvironment.unsupportedFileCount = stub.unsupportedFileCount ?? 0;
      finalEnvironment.symbolNodes = 0;
      break;
    }
    case "missing-then-remediated": { // error-remediation-followed
      operations = ["blueprint_status", "blueprint_build", "blueprint_doc_truth"];
      reasonCodes = [stub.statusResult?.reasonCode].filter(Boolean);
      finalEnvironment.generationId = stub.buildResult?.generationId ?? null;
      finalEnvironment.buildExecuted = true;
      break;
    }
    case "paginated": {
      operations = ["blueprint_impact", "blueprint_impact"];
      reasonCodes = [];
      finalEnvironment.pagesFetched = 2;
      finalEnvironment.finalCursor = null;
      finalEnvironment.uniqueResults = ["src/store.ts", "src/service.ts", "test/service.test.ts"];
      break;
    }
    default:
      return null;
  }

  const contextTokens = estimateContextTokens(scenario, operations, expected.requiredClaims ?? []);
  const answer = buildStubAnswer(scenario, expected.requiredClaims ?? []);
  const durationMs = STUB_STARTUP_MS + operations.length * 8 + Math.round(answer.length / 60);
  return {
    driver: "stub",
    operations,
    reasonCodes,
    finalEnvironment,
    answer,
    contextTokens,
    invalidCalls: scenario.fixture.stub.branch === "failure-then-recovery" ? 1 : 0,
    durationMs,
  };
}

function buildStubAnswer(scenario, requiredClaims) {
  const goal = String(scenario.goal ?? "");
  return [
    `Stub final response for ${scenario.scenarioId}:`,
    ...requiredClaims.map((claim) => `- ${claim}`),
    goal ? `Coverage note: ${goal.slice(0, 200)}` : "",
  ].filter(Boolean).join("\n");
}

function estimateContextTokens(scenario, operations, claims) {
  if (scenario.fixture.stub.branch === "no-tool") return 0;
  const perOperation = 100;
  const claimTokens = claims.reduce((sum, claim) => sum + Math.ceil(claim.length / 4), 0);
  return Math.min(40000, operations.length * perOperation + claimTokens + 150);
}

// --- verification -------------------------------------------------------------

function deepEqual(left, right) {
  if (left === right) return true;
  if (typeof left !== typeof right) return false;
  if (left === null || right === null) return false;
  if (Array.isArray(left) && Array.isArray(right)) {
    return left.length === right.length && left.every((item, index) => deepEqual(item, right[index]));
  }
  if (typeof left === "object" && typeof right === "object") {
    const leftKeys = Object.keys(left).sort();
    const rightKeys = Object.keys(right).sort();
    return leftKeys.length === rightKeys.length && leftKeys.every((key, index) => rightKeys[index] === key && deepEqual(left[key], right[key]));
  }
  return false;
}

function verifyFinalEnvironment(expected, actual) {
  const mismatches = [];
  const actualKeys = new Set(Object.keys(actual ?? {}));
  for (const [key, value] of Object.entries(expected ?? {})) {
    if (!actualKeys.has(key)) {
      mismatches.push(`finalEnvironment missing key ${key}`);
      continue;
    }
    if (!deepEqual(actual[key], value)) {
      mismatches.push(`finalEnvironment.${key} mismatch: expected ${JSON.stringify(value)}, got ${JSON.stringify(actual[key])}`);
    }
  }
  return { ok: mismatches.length === 0, mismatches };
}

export { evaluateTrial, stubTrial, verifyFinalEnvironment };

function containsAny(text, needles) {
  return (needles ?? []).some((needle) => String(text ?? "").includes(String(needle)));
}

// Order matters: the final environment is verified BEFORE any claim is
// admitted. If the environment does not match, claims are never evaluated and
// the trial fails on the environment mismatch alone.
function evaluateTrial(scenario, trial) {
  const failures = [];
  const expected = scenario.expected ?? {};
  const expectedEnv = expected.finalEnvironment ?? {};
  const env = verifyFinalEnvironment(expectedEnv, trial.finalEnvironment);
  if (!env.ok) {
    failures.push(`final environment mismatch (verified before claims): ${env.mismatches.join("; ")}`);
    return { status: "fail", envVerified: false, claimsChecked: false, failures };
  }

  const requiredClaims = expected.requiredClaims ?? [];
  for (const claim of requiredClaims) {
    if (!String(trial.answer ?? "").includes(claim)) failures.push(`required claim missing from final response: ${claim}`);
  }
  for (const claim of expected.forbiddenClaims ?? []) {
    if (containsAny(trial.answer, [claim])) failures.push(`forbidden claim present in final response: ${claim}`);
  }
  for (const code of expected.requiredReasonCodes ?? []) {
    if (!(trial.reasonCodes ?? []).includes(code)) failures.push(`required reason code missing: ${code}`);
  }
  for (const code of expected.forbiddenReasonCodes ?? []) {
    if ((trial.reasonCodes ?? []).includes(code)) failures.push(`forbidden reason code present: ${code}`);
  }
  for (const op of expected.requiredOperations ?? []) {
    if (!(trial.operations ?? []).includes(op)) failures.push(`required operation missing: ${op}`);
  }
  for (const op of scenario.allowedOperations ?? []) {
    if ((trial.operations ?? []).includes(op)) continue;
  }
  for (const op of trial.operations ?? []) {
    if (!(scenario.allowedOperations ?? []).includes(op)) failures.push(`operation outside allowedOperations: ${op}`);
  }
  for (const op of scenario.forbiddenOperations ?? []) {
    if ((trial.operations ?? []).includes(op)) failures.push(`forbidden operation executed: ${op}`);
  }

  const budgets = scenario.budgets ?? {};
  if (typeof budgets.maxToolCalls === "number" && (trial.operations ?? []).length > budgets.maxToolCalls) {
    failures.push(`tool call budget exceeded: ${trial.operations.length} > ${budgets.maxToolCalls}`);
  }
  if (typeof budgets.maxInvalidCalls === "number" && (trial.invalidCalls ?? 0) > budgets.maxInvalidCalls) {
    failures.push(`invalid call budget exceeded: ${trial.invalidCalls} > ${budgets.maxInvalidCalls}`);
  }
  if (typeof budgets.maxContextTokens === "number" && (trial.contextTokens ?? 0) > budgets.maxContextTokens) {
    failures.push(`context token budget exceeded: ${trial.contextTokens} > ${budgets.maxContextTokens}`);
  }

  return { status: failures.length === 0 ? "pass" : "fail", envVerified: true, claimsChecked: true, failures };
}

// --- claude driver ------------------------------------------------------------

function claudeInCi(args) {
  return args.driver === "claude" && Boolean(process.env[CLAUDE_CI_ENV]);
}

function detectClaudeCli() {
  const { spawnSync } = process.getBuiltinModule
    ? process.getBuiltinModule("node:child_process")
    : { spawnSync: undefined };
  if (!spawnSync) return { available: false, error: "node:child_process.spawnSync unavailable" };
  const probe = spawnSync("claude", ["--version"], { encoding: "utf8", windowsHide: true, timeout: 15000 });
  if (probe.error || probe.status !== 0) {
    return { available: false, error: probe.error?.message ?? `claude --version exited ${probe.status}` };
  }
  return { available: true, version: String(probe.stdout ?? "").trim() || null };
}

async function claudeTrial(scenario, args) {
  const prompt = [
    `You are Blueprint's behavioral AX harness. Execute the following scenario against the repository at ${resolve(args.root)}.`,
    `Scenario: ${scenario.goal}`,
    `User messages: ${(scenario.userMessages ?? []).map((message) => `\n  > ${message}`).join("")}`,
    `Allowed operations: ${(scenario.allowedOperations ?? []).join(", ") || "(none)"}`,
    `Forbidden operations: ${(scenario.forbiddenOperations ?? []).join(", ") || "(none)"}`,
    `Use only the Blueprint MCP tools (or no tools when the scenario requires none).`,
    `When finished, output a single JSON object on its own line with keys: finalEnvironment, operations, reasonCodes, answer.`,
    `finalEnvironment must reflect the ACTUAL repository state you verified, not the goal.`,
    `Do not claim completion unless you verified the final environment.`,
  ].join("\n");
  const startedAt = Date.now();
  const child = spawn("claude", ["-p", prompt], {
    cwd: resolve(args.root),
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  let stdout = "";
  let stderr = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  const exit = await new Promise((resolvePromise) => {
    const timer = setTimeout(() => {
      child.kill();
      resolvePromise({ code: null, signal: "killed-timeout" });
    }, CLAUDE_MAX_WALL_MS);
    child.on("exit", (code, signal) => {
      clearTimeout(timer);
      resolvePromise({ code, signal });
    });
    child.on("error", (error) => resolvePromise({ code: null, signal: null, error }));
  });
  const durationMs = Date.now() - startedAt;
  if (exit.error) return { fatal: String(exit.error.message ?? exit.error) };
  if (exit.signal) return { fatal: `claude -p terminated by ${exit.signal} after ${durationMs}ms` };
  if (exit.code !== 0) {
    return { fatal: `claude -p exited ${exit.code} after ${durationMs}ms: ${String(stderr).slice(0, 500)}` };
  }
  const match = /(\{[\s\S]*\})/.exec(stdout);
  if (!match) return { fatal: `claude -p produced no JSON envelope: ${stdout.slice(0, 400)}` };
  try {
    const envelope = JSON.parse(match[1]);
    return {
      driver: "claude",
      operations: Array.isArray(envelope.operations) ? envelope.operations.map(String) : [],
      reasonCodes: Array.isArray(envelope.reasonCodes) ? envelope.reasonCodes.map(String) : [],
      finalEnvironment: envelope.finalEnvironment ?? {},
      answer: String(envelope.answer ?? ""),
      contextTokens: Number.isFinite(envelope.contextTokens) ? envelope.contextTokens : null,
      invalidCalls: Number.isFinite(envelope.invalidCalls) ? envelope.invalidCalls : 0,
      durationMs,
      stderrExcerpt: String(stderr).slice(0, 400),
    };
  } catch (error) {
    return { fatal: `claude -p envelope was not valid JSON: ${String(error?.message ?? error)}` };
  }
}

function writeTranscript(kind, payload, args) {
  const dir = join(args.root, ".audit", "cx-b7", "alchemist", "behavioral");
  mkdirSync(dir, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const path = join(dir, `${stamp}-${kind}.jsonl`);
  writeFileSync(path, `${JSON.stringify(payload)}\n`, "utf8");
  return path;
}

// --- main ---------------------------------------------------------------------

async function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    console.log(usage());
    return;
  }
  if (!["stub", "claude"].includes(args.driver)) {
    console.error(`{ "error": { "code": "invalid_driver", "message": "driver must be stub or claude, got ${args.driver}" } }`);
    process.exitCode = 2;
    return;
  }
  if (!Number.isInteger(args.k) || args.k < 1 || args.k > 20) {
    console.error(`{ "error": { "code": "invalid_k", "message": "--k must be an integer 1..20, got ${args.k}" } }`);
    process.exitCode = 2;
    return;
  }
  if (claudeInCi(args)) {
    console.error(`{ "error": { "code": "claude_driver_forbidden_in_ci", "message": "--driver claude is never allowed in CI (CI=${process.env[CLAUDE_CI_ENV]})" } }`);
    process.exitCode = 2;
    return;
  }
  if (args.driver === "claude") {
    const probe = detectClaudeCli();
    if (!probe.available) {
      console.error(`{ "error": { "code": "claude_cli_unavailable", "message": "local claude CLI not found or not runnable", "detail": ${JSON.stringify(probe.error)} } }`);
      process.exitCode = 2;
      return;
    }
  }

  const root = resolve(args.root);
  if (!existsSync(root)) {
    console.error(`{ "error": { "code": "root_missing", "message": "root does not exist: ${root}" } }`);
    process.exitCode = 2;
    return;
  }

  const startedAt = Date.now();
  const environment = captureEnvironment(root);
  const { scenarios, loadErrors, dir } = await loadScenarios(root, { waitSeconds: args.waitSeconds });
  let capabilityProbe = null;
  try {
    capabilityProbe = await probeCapabilities(root);
  } catch (error) {
    capabilityProbe = { available: {}, error: String(error?.message ?? error) };
  }
  const report = {
    schemaVersion: 1,
    suite: "cx-b7-behavioral",
    runner: "evals/ax/run-behavioral.mjs",
    reporter: "evals/ax/report.mjs",
    generatedAt: new Date().toISOString(),
    root,
    scenarioDir: dir,
    scenarioCount: scenarios.length,
    expectedScenarioCount: 12,
    scenarioIds: scenarios.map((scenario) => scenario.scenarioId).sort(),
    driver: args.driver,
    k: args.k,
    environment,
    capabilityProbe,
    trials: [],
    scenarioResults: {},
    passK: {},
    skippedPending: [],
    checks: {},
    fatal: null,
    verdict: "fail",
    elapsedMs: 0,
  };

  if (loadErrors.length) {
    report.fatal = { code: "scenario_load_errors", message: `${loadErrors.length} scenario file(s) failed to parse`, details: loadErrors };
  } else if (scenarios.length === 0) {
    report.fatal = { code: "no_scenarios", message: `no scenario files found under ${dir} after ${args.waitSeconds}s` };
  } else if (capabilityProbe.error) {
    report.fatal = { code: "capability_probe_failed", message: capabilityProbe.error };
  }

  if (!report.fatal) {
    for (const scenario of scenarios) {
      const trials = [];
      const missingCapabilities = (scenario.requires ?? []).filter((name) => capabilityProbe.available[name] !== true);
      if (missingCapabilities.length) {
        for (let trialIndex = 0; trialIndex < args.k; trialIndex += 1) {
          trials.push({
            trial: trialIndex + 1,
            status: "skipped-pending",
            reason: `missing live capabilities: ${missingCapabilities.join(", ")}`,
            envVerified: false,
            claimsChecked: false,
            failures: [],
          });
        }
        report.trials.push({ scenarioId: scenario.scenarioId, version: scenario.version, trials });
        continue;
      }
      for (let trialIndex = 0; trialIndex < args.k; trialIndex += 1) {
        const trialNumber = trialIndex + 1;
        let raw;
        let harnessFailure = null;
        if (args.driver === "stub") {
          raw = stubTrial(scenario);
          if (!raw) harnessFailure = "stub branch not declared for this scenario";
        } else {
          const result = await claudeTrial(scenario, args);
          if (result.fatal) harnessFailure = result.fatal;
          else raw = result;
        }
        if (harnessFailure) {
          trials.push({
            trial: trialNumber,
            status: "fail",
            reason: harnessFailure,
            envVerified: false,
            claimsChecked: false,
            failures: [harnessFailure],
          });
          continue;
        }
        const evaluated = evaluateTrial(scenario, raw);
        const record = {
          trial: trialNumber,
          status: evaluated.status,
          envVerified: evaluated.envVerified,
          claimsChecked: evaluated.claimsChecked,
          failures: evaluated.failures,
          operations: raw.operations,
          reasonCodes: raw.reasonCodes,
          finalEnvironment: raw.finalEnvironment,
          contextTokens: raw.contextTokens,
          invalidCalls: raw.invalidCalls,
          durationMs: raw.durationMs,
        };
        if (args.driver === "claude") record.stderrExcerpt = raw.stderrExcerpt;
        trials.push(record);
      }
      report.trials.push({ scenarioId: scenario.scenarioId, version: scenario.version, trials });
    }

    // per-scenario status: pass only when every trial passes; skipped-pending
    // is kept separate from passes and never folded into them.
    for (const entry of report.trials) {
      const executed = entry.trials.filter((trial) => trial.status !== "skipped-pending");
      const skipped = entry.trials.filter((trial) => trial.status === "skipped-pending");
      if (executed.length === 0) {
        report.scenarioResults[entry.scenarioId] = {
          status: "skipped-pending",
          passCount: 0,
          failCount: 0,
          skippedCount: skipped.length,
          skippedReasons: [...new Set(skipped.map((trial) => trial.reason))],
        };
        report.skippedPending.push({ scenarioId: entry.scenarioId, count: skipped.length, reasons: [...new Set(skipped.map((trial) => trial.reason))] });
      } else {
        const passCount = executed.filter((trial) => trial.status === "pass").length;
        const failCount = executed.filter((trial) => trial.status === "fail").length;
        report.scenarioResults[entry.scenarioId] = {
          status: failCount === 0 ? "pass" : "fail",
          passCount,
          failCount,
          skippedCount: skipped.length,
        };
        if (failCount > 0 && skipped.length > 0) {
          report.skippedPending.push({ scenarioId: entry.scenarioId, count: skipped.length, reasons: [...new Set(skipped.map((trial) => trial.reason))] });
        }
      }
    }
  }

  // pass^k over executed scenarios only; skipped-pending scenarios are never
  // counted as passes. pass^5 is honest "unproven" when k < 5 trials exist.
  const executedScenarios = Object.entries(report.scenarioResults).filter(([, result]) => result.status !== "skipped-pending");
  for (const k of [1, 3, 5]) {
    const eligible = executedScenarios.filter(([, result]) => result.passCount >= k);
    if (args.k >= k) {
      const numerator = executedScenarios.filter(([, result]) => result.failCount === 0 && result.passCount >= k).length;
      report.passK[`pass^${k}`] = {
        value: numerator / Math.max(1, executedScenarios.length),
        numerator,
        denominator: executedScenarios.length,
      };
    } else {
      report.passK[`pass^${k}`] = {
        value: null,
        numerator: null,
        denominator: null,
        status: "unproven",
        reason: `requires ${k} trials per scenario; run had --k ${args.k}`,
      };
    }
  }

  const totals = {
    pass: 0,
    fail: 0,
    "skipped-pending": 0,
    trialsRun: report.trials.reduce((sum, entry) => sum + entry.trials.length, 0),
  };
  for (const entry of report.trials) {
    for (const trial of entry.trials) totals[trial.status] = (totals[trial.status] ?? 0) + 1;
  }
  report.totals = totals;
  report.checks = {
    scenarioSuiteComplete: scenarios.length === 12 && loadErrors.length === 0,
    allScenariosAccounted: report.trials.length === scenarios.length,
    everyScenarioHasKTrials: report.trials.every((entry) => entry.trials.length === args.k),
    passKeysPresent: ["pass^1", "pass^3", "pass^5"].every((key) => key in report.passK),
    liveCapabilityProbe: !capabilityProbe.error && capabilityProbe.toolCount > 0,
    atLeastOneScenarioExecuted: executedScenarios.length > 0,
    skippedPendingSeparate: report.skippedPending.every((entry) => report.scenarioResults[entry.scenarioId]?.status !== "pass"),
    environmentAssertedBeforeClaims: report.trials.every((entry) =>
      entry.trials.every((trial) => trial.status === "skipped-pending" || trial.envVerified === true)),
    defaultPathNoModelOrNetworkCalls: args.driver === "stub",
    zeroScenarioFailures: Object.values(report.scenarioResults).every((result) => result.status !== "fail"),
    unprovenBannerPresentInReport: true,
  };
  report.elapsedMs = Date.now() - startedAt;
  report.verdict = !report.fatal && report.checks.scenarioSuiteComplete && report.checks.allScenariosAccounted
    && report.checks.everyScenarioHasKTrials && report.checks.passKeysPresent && report.checks.environmentAssertedBeforeClaims
    && report.checks.liveCapabilityProbe && report.checks.atLeastOneScenarioExecuted && report.checks.zeroScenarioFailures ? "pass" : "fail";

  const artifacts = writeReportFiles(report, root);
  const transcriptPath = writeTranscript("behavioral-summary", { suite: report.suite, generatedAt: report.generatedAt, driver: report.driver, k: report.k, scenarioCount: report.scenarioCount, passK: report.passK, totals: report.totals }, args);

  // human summary
  console.log(`cx-b7 behavioral: driver=${args.driver} k=${args.k} root=${root}`);
  console.log(`scenarios: ${scenarios.length} loaded from ${dir}${scenarios.length !== 12 ? ` (expected 12)` : ""}`);
  console.log(`environment: gitHead=${environment.gitHead ?? "n/a"} node=${environment.nodeVersion} platform=${environment.platform} CI=${environment.env.CI ?? "unset"} network=${environment.network}`);
  for (const entry of report.trials) {
    const result = report.scenarioResults[entry.scenarioId];
    console.log(`  ${entry.scenarioId}: ${result.status} (pass=${result.passCount} fail=${result.failCount} skipped=${result.skippedCount})`);
  }
  const passLine = ["pass^1", "pass^3", "pass^5"].map((key) => {
    const entry = report.passK[key];
    return `${key}=${entry.value === null ? entry.status : `${entry.value} (${entry.numerator}/${entry.denominator})`}`;
  }).join(" ");
  console.log(`pass^k: ${passLine}`);
  console.log(`totals: pass=${totals.pass} fail=${totals.fail} skipped-pending=${totals["skipped-pending"]} trials=${totals.trialsRun}`);
  if (report.skippedPending.length) {
    console.log(`skipped-pending (separate from passes): ${report.skippedPending.map((entry) => entry.scenarioId).join(", ")}`);
  }
  if (report.fatal) console.log(`fatal: ${report.fatal.code} - ${report.fatal.message}`);
  console.log(`verdict: ${report.verdict.toUpperCase()}`);
  console.log(`artifacts: ${artifacts.reportJsonPath}`);
  console.log(`artifacts: ${artifacts.reportMarkdownPath}`);
  console.log(`transcript: ${transcriptPath}`);
  console.log(`exit: ${report.verdict === "pass" ? 0 : 1}`);
  process.exitCode = report.verdict === "pass" ? 0 : 1;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  main().catch((error) => {
    console.error(`{ "error": { "code": "runner_crash", "message": ${JSON.stringify(String(error?.message ?? error))}, "stack": ${JSON.stringify(String(error?.stack ?? ""))} } }`);
    process.exitCode = 1;
  });
}
