import assert from "node:assert/strict";
import test from "node:test";
import { buildNeighborhood, conformanceCheck, fixedFusion, providerReadiness } from "./retrieval-contracts.mjs";

test("R1 neighborhood is deterministic, seed-connected, bounded, and omission-explicit", () => {
  const args = {
    providerId: "blueprint", repositoryId: "repo-a", sourceGenerationId: "sha256:" + "a".repeat(64),
    seeds: [{ id: "anchor", kind: "user_anchor" }], nodes: [{ id: "anchor" }, { id: "child" }, { id: "orphan" }],
    edges: [{ from: "anchor", to: "child", type: "imports" }, { from: "orphan", to: "orphan", type: "hub" }],
    bounds: { depth: 2, nodes: 8, edges: 8, elapsed_ms: 10, estimated_tokens: 100 },
  };
  const first = buildNeighborhood(args);
  assert.deepEqual(first, buildNeighborhood(args));
  assert.deepEqual(first.nodes.map((node) => node.id), ["anchor", "child"]);
  assert.equal(first.omissions[0].reason, "no_seed_path");
});

test("R1 retains anchors when canonical ordering would otherwise exceed node bound", () => {
  const result = buildNeighborhood({
    providerId: "blueprint", repositoryId: "repo-a", sourceGenerationId: "gen-a",
    seeds: [{ id: "z-anchor" }],
    nodes: [{ id: "a-orphan" }, { id: "z-anchor" }, { id: "z-child" }],
    edges: [{ from: "z-anchor", to: "z-child", type: "imports" }],
    bounds: { nodes: 2 },
  });
  assert.deepEqual(result.nodes.map((node) => node.id), ["z-anchor", "z-child"]);
  assert.equal(result.omissions.some((item) => item.node_id === "z-anchor"), false);
});

test("R1 enforces traversal depth from graph distance, not provider depth fields", () => {
  const result = buildNeighborhood({
    providerId: "blueprint", repositoryId: "repo-a", sourceGenerationId: "gen-a",
    seeds: [{ id: "root" }],
    nodes: [
      { id: "root", depth: 0 }, { id: "one", depth: 0 },
      { id: "two", depth: 0 }, { id: "three", depth: 0 },
    ],
    edges: [
      { from: "root", to: "one", type: "imports" },
      { from: "one", to: "two", type: "imports" },
      { from: "two", to: "three", type: "imports" },
    ],
    bounds: { depth: 2, nodes: 8 },
  });
  assert.deepEqual(result.nodes.map((node) => node.id), ["root", "one", "two"]);
  assert.ok(result.omissions.some((item) => item.kind === "depth" && item.reason === "depth_limit" && item.count === 1));
});

test("R1 receipts aggregate every depth, node, and edge omission deterministically", () => {
  const args = {
    providerId: "blueprint", repositoryId: "repo-a", sourceGenerationId: "gen-a",
    seeds: [{ id: "root" }],
    nodes: [{ id: "root" }, { id: "a" }, { id: "b" }, { id: "deep" }, { id: "orphan" }],
    edges: [
      { from: "root", to: "a", type: "imports" },
      { from: "root", to: "b", type: "imports" },
      { from: "a", to: "deep", type: "imports" },
      { from: "orphan", to: "orphan", type: "imports" },
    ],
    bounds: { depth: 1, nodes: 3, edges: 1 },
  };
  const result = buildNeighborhood(args);
  const first = JSON.stringify(result.omissions);
  assert.equal(first, JSON.stringify(buildNeighborhood({ ...args, edges: [...args.edges].reverse() }).omissions));
  assert.deepEqual(result.nodes.map((node) => node.id), ["root", "a"]);
  assert.ok(result.omissions.some((item) => item.node_id === "b" && item.reason === "no_seed_path"));
  for (const kind of ["depth", "node", "edge"]) {
    const receipt = result.omissions.find((item) => item.kind === kind);
    assert.ok(receipt, `${kind} omission receipt missing`);
    assert.equal(typeof receipt.count, "number");
    assert.match(receipt.omittedIdsSha256, /^sha256:[a-f0-9]{64}$/);
    assert.equal(typeof receipt.recovery, "string");
  }
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
  const ready = providerReadiness({ providerId: "blueprint", modelId: "tree-sitter-v1", configDigest: "sha256:" + "c".repeat(64), capabilities: { live_resolution: true } });
  assert.equal(conformanceCheck(ready, { live_resolution: true }).pass, true);
  assert.deepEqual(conformanceCheck(ready, { scope_leak: true, live_resolution: true }).errors, ["scope_leak"]);
});
