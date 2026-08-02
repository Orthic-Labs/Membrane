import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";
import { runBenchmark } from "./e2e-benchmark.mjs";

const scenarios = ["repository_orientation", "cross_repo_impact", "preference_application", "stale_graph_immediate_edit", "contradiction", "denied_scope", "tool_proof_criteria", "user_correction", "memory_temporal_as_of", "provider_timeout_degradation"];
const phases = ["turn.observed", "provider.started", "provider.terminal", "candidate.retrieved", "candidate.admitted", "block.rendered", "block.delivered", "turn.outcome", "feedback.recorded"];

function fixture() {
  const dir = mkdtempSync(join(tmpdir(), "membrane-e2e-"));
  const dbPath = join(dir, "events.sqlite3");
  const db = new DatabaseSync(dbPath);
  db.exec("CREATE TABLE context_event_log (seq INTEGER PRIMARY KEY AUTOINCREMENT, event_id TEXT NOT NULL UNIQUE, trace_id TEXT NOT NULL, client TEXT NOT NULL, phase TEXT NOT NULL, status TEXT NOT NULL, reason_code TEXT)");
  const insert = db.prepare("INSERT INTO context_event_log(event_id,trace_id,client,phase,status,reason_code) VALUES(?,?,?,?,?,?)");
  const sources = new Map();
  for (const [index, scenario] of scenarios.entries()) {
    const traceId = `trace-${index}`;
    const eventIds = phases.map((phase, phaseIndex) => `${traceId}-${phaseIndex}`);
    phases.forEach((phase, phaseIndex) => insert.run(eventIds[phaseIndex], traceId, "codex", phase, "success", phase === "turn.observed" ? "tool" : null));
    const archivePath = join(dir, `${scenario}.json`);
    writeFileSync(archivePath, JSON.stringify({ schema: "orthic.e2e-scenario-receipt.v1", scenario, trace_id: traceId, status: "complete", event_ids: eventIds }));
    sources.set(scenario, { trace_id: traceId, adapter: "codex", archive_path: archivePath });
  }
  db.close();
  return { dbPath, sources };
}

test("E1 fails closed without installed event-ledger evidence", () => {
  const result = runBenchmark({ workspaceRoot: new URL("../../", import.meta.url).pathname, scenarioRunner: () => ({ model_tool_exercised: true }) });
  assert.equal(result.status, "blocked_incomplete_path");
  assert.equal(result.exit_gate.model_tool_behavior_exercised, false);
});

test("E1 rejects caller booleans when persisted tool observation is absent", () => {
  const { dbPath, sources } = fixture();
  const db = new DatabaseSync(dbPath);
  db.prepare("DELETE FROM context_event_log WHERE trace_id = ? AND phase = 'turn.observed'").run("trace-0");
  db.close();
  const result = runBenchmark({ workspaceRoot: new URL("../../", import.meta.url).pathname, eventDbPath: dbPath, model: "test-model", hardware: "test-host", warmCold: "warm", adapters: ["codex"], scenarioRunner: (scenario) => ({ ...sources.get(scenario), model_tool_exercised: true }) });
  assert.equal(result.status, "blocked_incomplete_path");
  assert.equal(result.records[0].status, "blocked_incomplete_path");
});

test("E1 accepts ten unique traces with full ledger and archive proof", () => {
  const { dbPath, sources } = fixture();
  const result = runBenchmark({ workspaceRoot: new URL("../../", import.meta.url).pathname, eventDbPath: dbPath, model: "test-model", hardware: "test-host", warmCold: "warm", adapters: ["codex"], scenarioRunner: (scenario) => sources.get(scenario) });
  assert.equal(result.status, "complete");
  assert.equal(result.records.length, 10);
  assert.equal(result.exit_gate.unique_traces, true);
  assert.ok(result.records.every((record) => record.stages.includes("feedback.recorded")));
});
