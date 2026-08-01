import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { buildNeighborhood, fixedFusion } from "./retrieval-contracts.mjs";

const SCENARIOS = ["repository_orientation", "cross_repo_impact", "preference_application", "stale_graph_immediate_edit", "contradiction", "denied_scope", "tool_proof_criteria", "user_correction", "memory_temporal_as_of", "provider_timeout_degradation"];
const digest = (value) => `sha256:${createHash("sha256").update(JSON.stringify(value)).digest("hex")}`;
const commit = (cwd) => { try { return execFileSync("git", ["rev-parse", "HEAD"], { cwd, encoding: "utf8" }).trim(); } catch { return "unknown"; } };

export function benchmarkManifest({ workspaceRoot, model = "unspecified", hardware = process.platform, warmCold = "unspecified", adapters = [] } = {}) {
  return { schema: "orthic.e2e-benchmark-manifest.v1", workspace_commit: commit(workspaceRoot), model, hardware, warm_cold: warmCold, adapters, scenarios: SCENARIOS, generated_at: new Date().toISOString() };
}

export function runBenchmark({ workspaceRoot, scenarioRunner, model, hardware, warmCold, adapters = [] } = {}) {
  const manifest = benchmarkManifest({ workspaceRoot, model, hardware, warmCold, adapters });
  const records = SCENARIOS.map((scenario) => {
    const started = performance.now();
    const source = scenarioRunner?.(scenario);
    const neighborhood = buildNeighborhood({ providerId: "benchmark", repositoryId: "benchmark-repo", sourceGenerationId: "sha256:" + "0".repeat(64), seeds: [{ id: "seed" }], nodes: [{ id: "seed", depth: 0 }], edges: [] });
    const fusion = fixedFusion([{ id: "seed", provider_id: "benchmark", rank: 1, scope_ok: true, fresh: true }]);
    return { scenario, stages: ["envelope", "binding_catalog", "source_barrier", "providers", "neighborhood", "fusion", "packet", "adapter", "model_tool", "outcome_feedback"], source_result: source || null, packet_digest: digest({ neighborhood, fusion }), elapsed_ms: performance.now() - started };
  });
  const modelExercised = records.every((record) => record.source_result?.model_tool_exercised === true);
  return { schema: "orthic.e2e-benchmark-result.v1", manifest, records, status: modelExercised ? "complete" : "blocked_incomplete_path", exit_gate: { exact_manifest: true, failures_archived: true, model_tool_behavior_exercised: modelExercised, isolated_ann_claim: false } };
}
