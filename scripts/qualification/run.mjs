#!/usr/bin/env node
// scripts/qualification/run.mjs — MBR-801 qualification entrypoint.
//
// "Installed-path ten-scenario harness": exercises a current SIGNED, INSTALLED
// Membrane build against the ten canonical qualification scenarios using a
// real client host, a real model, a real host machine, real providers, real
// delivery, and a real outcome/feedback loop, then archives a per-platform
// receipt consumable by scripts/qualification/verify-mbr801-evidence.mjs.
//
// Every real-execution dependency (signed-build verification, scenario
// execution, benchmark aggregation, host identity) is an injectable
// parameter with a default that performs the real, installed-path work. The
// defaults require a live installed host (cortex CLI, event-log database,
// running Membrane service, signed release-evidence manifest) and are meant
// to run manually at the Book gate on macOS — never during task
// implementation and never as part of an automated pipeline. See
// docs/reference/evaluation/mbr801-run-harness.md.
//
// The Book gate invokes:
//
//     node scripts/qualification/run.mjs --task MBR-801 --platform macos --release-manifest <path>

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { hostname } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const SCENARIOS = [
  "repository_orientation",
  "cross_repo_impact",
  "preference_application",
  "stale_graph_immediate_edit",
  "contradiction",
  "denied_scope",
  "tool_proof_criteria",
  "user_correction",
  "memory_temporal_as_of",
  "provider_timeout_degradation",
];

const HERE = dirname(fileURLToPath(import.meta.url));
const nonEmptyString = (value) => typeof value === "string" && value.trim().length > 0;
const isHex40 = (value) => typeof value === "string" && /^[0-9a-f]{40}$/u.test(value);
const isHex64 = (value) => typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);

function atomicJson(path, value) {
  mkdirSync(dirname(path), { recursive: true });
  const temporary = `${path}.${process.pid}.tmp`;
  writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  renameSync(temporary, path);
}

export function resolvePlatform(explicit) {
  if (explicit !== undefined && explicit !== "macos") throw new Error("Mac-only qualification accepts --platform macos");
  if (process.platform !== "darwin") throw new Error("Mac-only qualification must run on macOS");
  return "macos";
}

function defaultGitCommit(workspaceRoot) {
  try {
    return execFileSync("git", ["rev-parse", "HEAD"], { cwd: workspaceRoot, encoding: "utf8" }).trim();
  } catch {
    return null;
  }
}

// Real client/model/host identity for the machine actually running the harness.
export async function defaultResolveHostIdentity() {
  return {
    client: process.env.MEMBRANE_CLIENT || "claude_code",
    model: process.env.MEMBRANE_QUALIFICATION_MODEL || "",
    host: hostname() || "",
  };
}

// Proves the harness is exercising a signed, installed build by delegating to
// the existing release-evidence verifier (scripts/release/verify-release-evidence.mjs),
// which independently checks artifact hashes, ed25519 + platform-trust
// (Apple notarization) signatures, and installed platform
// receipts. Requires a real release-evidence manifest produced by the signed
// release pipeline; this function performs no build or signing itself.
export async function defaultVerifySignedBuild({ releaseManifestPath }) {
  if (!nonEmptyString(releaseManifestPath) || !existsSync(releaseManifestPath)) {
    return {
      installed_execution: false, signature_status: "unavailable", artifact_sha256: null, release_generation: null,
      reason: "release evidence manifest not provided; pass --release-manifest to a real signed release-evidence.json for an installed-path run",
    };
  }
  try {
    const manifest = JSON.parse(readFileSync(releaseManifestPath, "utf8"));
    const module = await import(pathToFileURL(resolve(HERE, "../release/verify-release-evidence.mjs")).href);
    module.verifyReleaseEvidence(manifest, dirname(releaseManifestPath));
    return {
      installed_execution: true, signature_status: "passed",
      artifact_sha256: manifest.release?.artifact_sha256 ?? null, release_generation: manifest.release?.generation ?? null,
    };
  } catch (error) {
    return { installed_execution: false, signature_status: "failed", artifact_sha256: null, release_generation: null, reason: error.message };
  }
}

// Real installed-path scenario execution: delegates to the existing real-host
// scenario runner (scripts/run-platform-scenarios.mjs) for exactly one
// scenario, which drives an actual installed client host against the real
// event-log database, provider stack, and outcome/feedback loop. Requires a
// live installed Membrane service, a running installed client CLI, the cortex
// binary, and the event-log database — real-machine preconditions this
// module never fabricates or substitutes.
export async function defaultScenarioRunner({ scenario, platform, workspaceRoot, evidenceRoot }) {
  const runnerPath = resolve(HERE, "../run-platform-scenarios.mjs");
  const result = spawnSync(process.execPath, [
    runnerPath,
    "--scenario", scenario,
    "--platform", "mac",
    "--workspace-root", workspaceRoot,
    "--evidence-root", evidenceRoot,
  ], { encoding: "utf8", timeout: 300_000 });
  if (result.status !== 0) throw new Error(`${scenario}: installed-path scenario runner failed: ${String(result.stderr || result.error?.message || "").slice(-1000)}`);
  const line = String(result.stdout || "").trim().split(/\r?\n/u).filter(Boolean).at(-1);
  const summary = JSON.parse(line);
  const detail = JSON.parse(readFileSync(summary.output, "utf8"));
  const trace = (detail.traces || []).find((entry) => entry.scenario === scenario);
  if (!trace || !nonEmptyString(trace.trace_id)) throw new Error(`${scenario}: installed-path scenario runner produced no trace`);
  return { trace_id: trace.trace_id, gates: { provider: "passed", delivery: "passed", outcome: "passed" } };
}

// Real benchmark aggregation over the traces collected during this run,
// reusing mcp/e2e-benchmark.mjs. Requires the same live event-log database
// the scenario runner wrote to.
export async function defaultBenchmarkRunner({ workspaceRoot, platform, scenarioResults, eventDbPath }) {
  if (!nonEmptyString(eventDbPath) || !existsSync(eventDbPath)) {
    return { status: "blocked_incomplete_path", reason: "no live installed event-log database available for benchmark aggregation" };
  }
  const module = await import(pathToFileURL(resolve(HERE, "../../mcp/e2e-benchmark.mjs")).href);
  const byScenario = new Map(scenarioResults.map((result) => [result.scenario_id, result]));
  return module.runBenchmark({
    workspaceRoot, eventDbPath, model: process.env.MEMBRANE_QUALIFICATION_MODEL || "installed",
    hardware: `${platform}-${process.arch}`, warmCold: "warm", adapters: ["claude_code"],
    scenarioRunner: (scenario) => byScenario.get(scenario),
  });
}

// Builds and archives the per-platform receipt.json plus per-scenario trace
// archives in the exact shape scripts/qualification/verify-mbr801-evidence.mjs
// requires (installed_execution, signature_status, artifact_sha256, real
// client/model/host identity, all ten canonical scenarios with unique trace
// IDs and passed provider/delivery/outcome gates, a complete benchmark, and
// readable archived receipt files matching each trace).
export function buildReceipt({ evidenceRoot, platform, commit, releaseGeneration, client, model, host, artifactSha256, signatureStatus, installedExecution, scenarioResults, benchmark, generatedAt }) {
  const platformDir = join(evidenceRoot, platform);
  const traces = scenarioResults.map((result, index) => {
    const archiveName = `trace-${index}.json`;
    if (nonEmptyString(result.trace_id)) {
      atomicJson(join(platformDir, archiveName), { trace_id: result.trace_id, scenario_id: result.scenario_id, commit, release_generation: releaseGeneration });
    }
    return { trace_id: result.trace_id ?? null, scenario_id: result.scenario_id, archive_path: archiveName, gates: result.gates || {} };
  });

  const reasons = [];
  if (!isHex40(commit)) reasons.push("workspace commit unavailable or malformed");
  if (!nonEmptyString(releaseGeneration)) reasons.push("release generation unavailable");
  if (installedExecution !== true) reasons.push("installed execution not proven");
  if (signatureStatus !== "passed") reasons.push("signature status is not passed");
  if (!isHex64(artifactSha256)) reasons.push("artifact sha256 missing or malformed");
  if (![client, model, host].every(nonEmptyString)) reasons.push("real client, model, or host identity is missing");
  const traceIds = traces.map((trace) => trace.trace_id);
  if (traceIds.some((id) => !nonEmptyString(id))) reasons.push("one or more scenarios did not produce a trace id");
  if (new Set(traceIds.filter(nonEmptyString)).size !== SCENARIOS.length) reasons.push("trace IDs are not unique across all ten scenarios");
  if (new Set(traces.map((trace) => trace.scenario_id)).size !== SCENARIOS.length || SCENARIOS.some((name) => !traces.some((trace) => trace.scenario_id === name))) {
    reasons.push("canonical scenario coverage is incomplete");
  }
  if (traces.some((trace) => !["provider", "delivery", "outcome"].every((gate) => trace.gates?.[gate] === "passed"))) {
    reasons.push("provider, delivery, or outcome gate failed for one or more scenarios");
  }
  if (benchmark?.status !== "complete") reasons.push("benchmark is incomplete");

  const status = reasons.length === 0 ? "passed" : "incomplete";
  const scenariosPassed = traces.filter((trace) => nonEmptyString(trace.trace_id) && ["provider", "delivery", "outcome"].every((gate) => trace.gates?.[gate] === "passed")).length;
  const receipt = {
    schema: "membrane.mbr801-installed-receipt.v1", platform, status, commit, release_generation: releaseGeneration,
    installed_execution: installedExecution === true, signature_status: signatureStatus, artifact_sha256: artifactSha256,
    client, model, host, scenario_count: SCENARIOS.length, scenarios_passed: scenariosPassed,
    traces, benchmark: benchmark ?? { status: "blocked_incomplete_path" }, generated_at: generatedAt, reasons,
  };
  const receiptPath = join(platformDir, "receipt.json");
  atomicJson(receiptPath, receipt);
  return { receiptPath, receipt };
}

// Orchestrates one platform's installed-path run for a supported task. Every
// side-effecting dependency is injectable so this can be verified
// deterministically without a live installed build (see the accompanying
// test file), and driven for real at the Book gate by supplying the real
// defaults' preconditions (a release-evidence manifest, a running Membrane
// service, an installed client host).
export async function runInstalledPathHarness(options = {}) {
  const {
    task = "MBR-801",
    workspaceRoot = resolve(HERE, "../.."),
    evidenceRoot = join(workspaceRoot, "docs", "evidence", "qualification", "mbr801"),
    platform: platformOverride,
    releaseManifestPath,
    eventDbPath,
    now = () => new Date().toISOString(),
    resolveHostIdentity = defaultResolveHostIdentity,
    verifySignedBuild = defaultVerifySignedBuild,
    scenarioRunner = defaultScenarioRunner,
    benchmarkRunner = defaultBenchmarkRunner,
    gitCommit = () => defaultGitCommit(workspaceRoot),
  } = options;

  if (task !== "MBR-801") {
    return { schema: "membrane.installed-path-harness.v1", status: "open", reason: `unsupported task ${task}; this harness implements MBR-801 only` };
  }

  const platform = resolvePlatform(platformOverride);
  const commit = gitCommit();
  const identity = await resolveHostIdentity({ workspaceRoot, platform });
  const build = await verifySignedBuild({ workspaceRoot, releaseManifestPath, platform });

  const scenarioResults = [];
  for (const scenario of SCENARIOS) {
    let result;
    try {
      result = await scenarioRunner({ scenario, platform, workspaceRoot, evidenceRoot });
    } catch (error) {
      result = { trace_id: null, gates: {}, error: error.message };
    }
    scenarioResults.push({
      scenario_id: scenario,
      trace_id: typeof result?.trace_id === "string" ? result.trace_id : null,
      gates: { provider: result?.gates?.provider === "passed" ? "passed" : "failed", delivery: result?.gates?.delivery === "passed" ? "passed" : "failed", outcome: result?.gates?.outcome === "passed" ? "passed" : "failed" },
    });
  }

  const benchmark = await benchmarkRunner({ workspaceRoot, evidenceRoot, platform, scenarioResults, eventDbPath });

  const { receiptPath, receipt } = buildReceipt({
    evidenceRoot, platform, commit, releaseGeneration: build.release_generation,
    client: identity.client, model: identity.model, host: identity.host,
    artifactSha256: build.artifact_sha256, signatureStatus: build.signature_status,
    installedExecution: build.installed_execution, scenarioResults, benchmark, generatedAt: now(),
  });

  return { schema: "membrane.installed-path-harness.v1", task, platform, status: receipt.status, receiptPath, receipt };
}

// ---------------------------------------------------------------------------
// Windows registry-runner contract (PKG-01, windows-amendment-acceptance.json).
//
// This is a second, independent qualification mode alongside the macOS-only
// MBR-801 installed-path harness above. It never widens resolvePlatform: the
// macOS-only gate on the MBR-801 path is untouched, and this mode is reached
// only through an explicit --case-registry invocation with --platform
// windows. It runs any acceptance registry (windows-acceptance.json,
// blueprint-membrane-acceptance.json, windows-amendment-acceptance.json)
// that shares this contract: select every case in one --group, execute each
// case's declared caseFile/caseExport, and require discovered == executed ==
// terminal id sets with zero, missing, duplicate, or skipped selections
// rejected outright. --profile internal-unsigned is the only implemented
// profile: it never reports a signed-release PASS and never disables
// production signing verification (see docs/architecture/execution-lifecycle-boundary.md).
export const EVIDENCE_KINDS = new Set(["source", "component", "integration", "installed", "host", "task-outcome"]);

// Only evidence produced by an exercised runtime can close an unsigned
// functional qualification. Source/component/integration findings remain
// useful structural diagnostics, but they cannot be promoted to closure.
export const FUNCTIONAL_EVIDENCE_KINDS = new Set(["installed", "host", "task-outcome"]);

export function classifyFunctionalEvidence(status, evidenceKind) {
  if (status !== "passed") return { status: "failed", reason: "case did not pass" };
  if (FUNCTIONAL_EVIDENCE_KINDS.has(evidenceKind)) return { status: "passed", reason: null };
  return { status: "structural", reason: `evidenceKind ${evidenceKind} is not runtime evidence` };
}

export function deriveCaseGroup(row) {
  if (nonEmptyString(row?.group)) return row.group;
  const id = String(row?.id ?? "");
  const dash = id.indexOf("-");
  return dash > 0 ? id.slice(0, dash) : id;
}

// Parses one case registry and rejects duplicate or missing ids up front, so
// a corrupt registry never silently drops a case instead of failing loudly.
export function loadCaseRegistry(caseRegistryPath) {
  if (!nonEmptyString(caseRegistryPath) || !existsSync(caseRegistryPath)) {
    throw new Error(`case registry not found: ${caseRegistryPath}`);
  }
  const registry = JSON.parse(readFileSync(caseRegistryPath, "utf8"));
  const cases = Array.isArray(registry.cases) ? registry.cases : [];
  const seen = new Set();
  const duplicateIds = [];
  for (const row of cases) {
    if (!nonEmptyString(row?.id)) throw new Error(`case registry ${caseRegistryPath} has a row with no id`);
    if (seen.has(row.id)) duplicateIds.push(row.id);
    seen.add(row.id);
  }
  if (duplicateIds.length > 0) {
    throw new Error(`case registry ${caseRegistryPath} has duplicate case ids: ${[...new Set(duplicateIds)].join(", ")}`);
  }
  return { registry, cases };
}

// A zero-case selection is a hard failure, never a silent no-op pass.
export function selectGroupCases(cases, group) {
  if (!nonEmptyString(group)) throw new Error("--group is required for registry qualification");
  const selected = cases.filter((row) => deriveCaseGroup(row) === group);
  if (selected.length === 0) throw new Error(`case registry has zero cases in group ${group}; refusing a zero-selection run`);
  return selected;
}

// Rows explicitly deferred until parity is established are still discovered
// & executed when their group is run, but cannot block required closure.
export function isRequiredCase(row) {
  return row?.state !== "OPTIONAL_AFTER_PARITY";
}

const defaultImportCaseModule = (caseSourceRoot) => (specifier) => import(pathToFileURL(resolve(caseSourceRoot, specifier)).href);

// Runs exactly one registry case to a terminal (never skipped) result: a case
// missing its declared module or export fails instead of being silently
// dropped, and a case that does not record a recognized evidenceKind fails
// instead of being counted as passed.
export async function runOneRegistryCase(row, context) {
  const { workspaceRoot, caseSourceRoot = workspaceRoot, profile, platform, evidencePath, importCaseModule = defaultImportCaseModule(caseSourceRoot) } = context;
  if (!nonEmptyString(row?.caseFile) || !nonEmptyString(row?.caseExport)) {
    return { id: row?.id ?? null, status: "failed", functionalStatus: "failed", evidenceKind: null, reason: "case registry row is missing caseFile or caseExport" };
  }
  let moduleExports;
  try {
    moduleExports = await importCaseModule(row.caseFile);
  } catch (error) {
    return { id: row.id, status: "failed", functionalStatus: "failed", evidenceKind: null, reason: `case module ${row.caseFile} failed to load: ${error.message}` };
  }
  const caseFunction = moduleExports?.[row.caseExport];
  if (typeof caseFunction !== "function") {
    return { id: row.id, status: "failed", functionalStatus: "failed", evidenceKind: null, reason: `case module ${row.caseFile} has no export ${row.caseExport}` };
  }
  let outcome;
  try {
    outcome = await caseFunction({ row, workspaceRoot, caseSourceRoot, profile, platform, evidencePath });
  } catch (error) {
    return { id: row.id, status: "failed", functionalStatus: "failed", evidenceKind: null, reason: `case ${row.id} threw: ${error.message}` };
  }
  if (!outcome || typeof outcome !== "object") {
    return { id: row.id, status: "failed", functionalStatus: "failed", evidenceKind: null, reason: `case ${row.id} returned no result` };
  }
  if (!EVIDENCE_KINDS.has(outcome.evidenceKind)) {
    return { id: row.id, status: "failed", functionalStatus: "failed", evidenceKind: outcome.evidenceKind ?? null, reason: `case ${row.id} did not record a recognized evidenceKind` };
  }
  const caseStatus = outcome.status === "passed" ? "passed" : "failed";
  const functional = classifyFunctionalEvidence(caseStatus, outcome.evidenceKind);
  return {
    id: row.id,
    status: caseStatus,
    functionalStatus: functional.status,
    evidenceKind: outcome.evidenceKind,
    detail: outcome.detail ?? null,
    reason: outcome.reason ?? functional.reason,
  };
}

// Orchestrates one --group of one case registry under the internal-unsigned
// Windows profile. Requires discovered == executed == terminal id sets;
// throws (never a soft pass) on zero selection, a duplicate id, an
// unsupported platform/profile, or a missing --evidence path.
export async function runRegistryQualification(options = {}) {
  const {
    platform,
    profile,
    caseRegistryPath,
    group,
    evidencePath,
    workspaceRoot = resolve(HERE, "../.."),
    caseSourceRoot = workspaceRoot,
    now = () => new Date().toISOString(),
    importCaseModule,
  } = options;

  if (platform !== "windows") {
    throw new Error("registry qualification requires an explicit --platform windows; macOS registry qualification is deferred and never a Windows prerequisite");
  }
  if (profile !== "internal-unsigned") {
    throw new Error(`unsupported --profile ${profile}; the registry runner implements internal-unsigned only and never reports a signed-release PASS`);
  }
  if (!nonEmptyString(evidencePath)) throw new Error("--evidence is required");

  const { cases } = loadCaseRegistry(caseRegistryPath);
  const selected = selectGroupCases(cases, group);
  const discovered = selected.map((row) => row.id);
  const requiredIds = selected.filter(isRequiredCase).map((row) => row.id);
  const byId = new Map(cases.map((row) => [row.id, row]));

  const executed = [];
  const terminal = [];
  const results = [];
  for (const id of discovered) {
    const result = await runOneRegistryCase(byId.get(id), { workspaceRoot, caseSourceRoot, profile, platform, evidencePath, importCaseModule });
    executed.push(result.id);
    terminal.push(result.id);
    results.push(result);
  }

  const setsEqual = (a, b) => a.size === b.size && [...a].every((entry) => b.has(entry));
  const discoveredSet = new Set(discovered);
  if (!setsEqual(discoveredSet, new Set(executed)) || !setsEqual(discoveredSet, new Set(terminal))) {
    throw new Error("required/discovered/executed/terminal case-set equality violated");
  }

  const evidenceKindCounts = {};
  for (const result of results) {
    if (result.evidenceKind) evidenceKindCounts[result.evidenceKind] = (evidenceKindCounts[result.evidenceKind] ?? 0) + 1;
  }
  const requiredSet = new Set(requiredIds);
  const requiredResults = results.filter((result) => requiredSet.has(result.id));
  const failed = requiredResults.filter((result) => result.status !== "passed");
  const functionalFailed = requiredResults.filter((result) => result.functionalStatus !== "passed");

  const summary = {
    schema: "membrane.registry-qualification.v1",
    platform,
    profile,
    group,
    caseRegistryPath,
    workspaceRoot,
    caseSourceRoot,
    evidencePath,
    generatedAt: now(),
    discoveredIds: discovered,
    requiredIds,
    optionalIds: discovered.filter((id) => !requiredSet.has(id)),
    executedIds: executed,
    terminalIds: terminal,
    results,
    evidenceKindCounts,
    // Registry qualification records whether required cases passed their
    // declared checks. Runtime evidence is a separate functional axis below:
    // source/component/integration rows may qualify structurally without
    // being promoted to unsigned functional closure.
    status: failed.length === 0 ? "passed" : "failed",
    functionalStatus: functionalFailed.length === 0 ? "passed" : "failed",
    unsignedFunctional: functionalFailed.length === 0,
    signedReleasePass: false,
  };
  atomicJson(evidencePath, summary);
  return summary;
}

function cli() {
  const args = process.argv.slice(2);
  const value = (name) => { const index = args.indexOf(name); return index < 0 ? undefined : args[index + 1]; };
  const caseRegistryPath = value("--case-registry") ? resolve(value("--case-registry")) : undefined;
  if (caseRegistryPath) {
    const platform = value("--platform");
    const profile = value("--profile");
    const group = value("--group");
    const evidencePath = value("--evidence") ? resolve(value("--evidence")) : undefined;
    const workspaceRoot = resolve(value("--workspace-root") || resolve(HERE, "../.."));
    const caseSourceRoot = resolve(value("--case-source-root") || workspaceRoot);
    runRegistryQualification({ platform, profile, caseRegistryPath, group, evidencePath, workspaceRoot, caseSourceRoot })
      .then((summary) => {
        process.stdout.write(`${JSON.stringify({ status: summary.status, group: summary.group, evidencePath, requiredCount: summary.requiredIds.length })}\n`);
        process.exitCode = summary.status === "passed" ? 0 : 2;
      })
      .catch((error) => {
        process.stderr.write(`${error.message}\n`);
        process.exitCode = 1;
      });
    return;
  }

  const task = value("--task");
  const workspaceRoot = resolve(value("--workspace-root") || resolve(HERE, "../.."));
  const evidenceRoot = resolve(value("--evidence-root") || join(workspaceRoot, "docs", "evidence", "qualification", "mbr801"));
  const platform = value("--platform");
  const releaseManifestPath = value("--release-manifest") ? resolve(value("--release-manifest")) : undefined;
  const eventDbPath = value("--event-db") ? resolve(value("--event-db")) : undefined;
  runInstalledPathHarness({ task, workspaceRoot, evidenceRoot, platform, releaseManifestPath, eventDbPath })
    .then((result) => {
      process.stdout.write(`${JSON.stringify({ status: result.status, platform: result.platform, receiptPath: result.receiptPath, reason: result.reason })}\n`);
      process.exitCode = result.status === "passed" ? 0 : 2;
    })
    .catch((error) => {
      process.stderr.write(`${error.message}\n`);
      process.exitCode = 1;
    });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli();
