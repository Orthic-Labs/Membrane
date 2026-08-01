import assert from "node:assert/strict";
import test from "node:test";
import { buildNeighborhood, conformanceCheck, fixedFusion, providerReadiness } from "./retrieval-contracts.mjs";

test("R1 neighborhood is deterministic, seed-connected, bounded, and omission-explicit", () => {
  const args = {
    providerId: "cortex", repositoryId: "repo-a", sourceGenerationId: "sha256:" + "a".repeat(64),
    seeds: [{ id: "anchor", kind: "user_anchor" }], nodes: [{ id: "anchor" }, { id: "child" }, { id: "orphan" }],
    edges: [{ from: "anchor", to: "child", type: "imports" }, { from: "orphan", to: "orphan", type: "hub" }],
    bounds: { depth: 2, nodes: 8, edges: 8, elapsed_ms: 10, estimated_tokens: 100 },
  };
  const first = buildNeighborhood(args);
  assert.deepEqual(first, buildNeighborhood(args));
  assert.deepEqual(first.nodes.map((node) => node.id), ["anchor", "child"]);
  assert.equal(first.omissions[0].reason, "no_seed_path");
});

test("R2 fixed fusion names policy, quotas providers, and rejects raw probability semantics", () => {
  const result = fixedFusion([
    { id: "a", provider_id: "blueprint", rank: 1, score: 0.99, redundancy_key: "same" },
    { id: "b", provider_id: "membrane", rank: 1, score: 0.01, redundancy_key: "same" },
    { id: "c", provider_id: "membrane", rank: 2, score: 0.5 },
  ], { providerQuotas: { blueprint: 1, membrane: 2 } });
  assert.equal(result.policy, "membrane-fusion-fixed-v1");
  assert.equal(result.diagnostics.raw_scores_are_not_probabilities, true);
  assert.equal(result.candidates.length, 2);
});

test("R3 readiness and conformance expose identity and lane-local failures", () => {
  const ready = providerReadiness({ providerId: "cortex", modelId: "tree-sitter-v1", configDigest: "sha256:" + "c".repeat(64), capabilities: { live_resolution: true } });
  assert.equal(conformanceCheck(ready, { live_resolution: true }).pass, true);
  assert.deepEqual(conformanceCheck(ready, { scope_leak: true, live_resolution: true }).errors, ["scope_leak"]);
});
