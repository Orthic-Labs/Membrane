import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";

const SCENARIOS = ["repository_orientation", "cross_repo_impact", "preference_application", "stale_graph_immediate_edit", "contradiction", "denied_scope", "tool_proof_criteria", "user_correction", "memory_temporal_as_of", "provider_timeout_degradation"];
const REQUIRED_PHASES = ["provider.started", "provider.terminal", "candidate.retrieved", "candidate.admitted", "block.rendered", "block.delivered", "turn.outcome", "feedback.recorded"];
const sha256 = (value) => `sha256:${createHash("sha256").update(value).digest("hex")}`;
const commit = (cwd) => { try { return execFileSync("git", ["rev-parse", "HEAD"], { cwd, encoding: "utf8" }).trim(); } catch { return "unknown"; } };

export function benchmarkManifest({ workspaceRoot, eventDbPath, model = "unspecified", hardware = process.platform, warmCold = "unspecified", adapters = [] } = {}) {
  return { schema: "membrane.e2e-benchmark-manifest.v1", execution_mode: "installed", workspace_commit: commit(workspaceRoot), event_db_sha256: existsSync(eventDbPath || "") ? sha256(readFileSync(eventDbPath)) : null, model, hardware, warm_cold: warmCold, adapters, scenarios: SCENARIOS, generated_at: new Date().toISOString() };
}

function archivedReceipt(path, scenario, traceId, eventIds) {
  if (!path || !existsSync(path)) return { valid: false, sha256: null };
  try {
    const bytes = readFileSync(path);
    const receipt = JSON.parse(bytes);
    const recorded = new Set(receipt.event_ids || []);
    const valid = receipt.schema === "membrane.e2e-scenario-receipt.v1"
      && receipt.scenario === scenario
      && receipt.trace_id === traceId
      && receipt.status === "complete"
      && eventIds.every((eventId) => recorded.has(eventId));
    return { valid, sha256: sha256(bytes) };
  } catch {
    return { valid: false, sha256: null };
  }
}

function traceProof(db, source, scenario) {
  const traceId = typeof source?.trace_id === "string" ? source.trace_id : "";
  const adapter = typeof source?.adapter === "string" ? source.adapter : "";
  if (!traceId || !adapter) return { scenario, trace_id: traceId || null, adapter: adapter || null, stages: [], event_ids: [], archive_path: source?.archive_path || null, archive_sha256: null, status: "blocked_missing_identifiers" };
  const rows = db.prepare("SELECT event_id, client, phase, status, reason_code FROM context_event_log WHERE trace_id = ? ORDER BY seq").all(traceId);
  const phases = new Set(rows.map((row) => row.phase));
  const toolObserved = rows.some((row) => row.phase === "turn.observed" && row.reason_code === "tool" && row.client === adapter && row.status === "success");
  const lifecycleComplete = REQUIRED_PHASES.every((phase) => phases.has(phase));
  const archive = archivedReceipt(source.archive_path, scenario, traceId, rows.map((row) => row.event_id));
  const status = rows.length > 0 && toolObserved && lifecycleComplete && archive.valid ? "complete" : "blocked_incomplete_path";
  return { scenario, trace_id: traceId, adapter, stages: [...phases], event_ids: rows.map((row) => row.event_id), archive_path: source.archive_path || null, archive_sha256: archive.sha256, archive_valid: archive.valid, status };
}

export function runBenchmark({ workspaceRoot, eventDbPath, scenarioRunner, model, hardware, warmCold, adapters = [] } = {}) {
  const manifest = benchmarkManifest({ workspaceRoot, eventDbPath, model, hardware, warmCold, adapters });
  if (!eventDbPath || !existsSync(eventDbPath) || typeof scenarioRunner !== "function") {
    return { schema: "membrane.e2e-benchmark-result.v1", manifest, records: [], status: "blocked_incomplete_path", exit_gate: { exact_manifest: false, failures_archived: false, model_tool_behavior_exercised: false, isolated_ann_claim: false } };
  }
  const db = new DatabaseSync(eventDbPath, { readOnly: true });
  try {
    const records = SCENARIOS.map((scenario) => traceProof(db, scenarioRunner(scenario), scenario));
    const uniqueTraces = new Set(records.map((record) => record.trace_id)).size === SCENARIOS.length;
    const exactManifest = manifest.workspace_commit !== "unknown" && manifest.event_db_sha256 !== null && model !== undefined && model !== "unspecified" && hardware !== undefined && warmCold !== undefined && warmCold !== "unspecified" && adapters.length > 0 && records.every((record) => adapters.includes(record.adapter));
    const failuresArchived = records.every((record) => record.archive_valid === true);
    const modelToolBehaviorExercised = records.every((record) => record.status === "complete");
    const complete = uniqueTraces && exactManifest && failuresArchived && modelToolBehaviorExercised;
    return { schema: "membrane.e2e-benchmark-result.v1", manifest, records, status: complete ? "complete" : "blocked_incomplete_path", exit_gate: { exact_manifest: exactManifest, failures_archived: failuresArchived, model_tool_behavior_exercised: modelToolBehaviorExercised, unique_traces: uniqueTraces, isolated_ann_claim: false } };
  } finally {
    db.close();
  }
}
