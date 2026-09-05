import assert from "node:assert/strict";
import test from "node:test";

import { FACT_PROVENANCE, withFactProvenance } from "../src/graph/provenance.mjs";
import { defineProvider } from "../src/providers/index.mjs";
import {
  collectSemanticEvidence,
  collectSemanticEvidenceSync,
  createSemanticProviderRegistry,
  crossCheckWithLiveVerifier,
} from "../src/providers/semantic-orchestrator.mjs";

function semanticProvider(overrides = {}) {
  return defineProvider({
    id: "semantic.test",
    version: "1.0.0",
    kind: "compiler",
    protocolRange: ">=1 <2",
    capabilities: ["definitions"],
    permissions: { filesystem: "repo-read", network: "none", process: "none" },
    probe() { return { state: "available" }; },
    collect() { return { nodes: [], edges: [], reports: [] }; },
    ...overrides,
  });
}

function canonical(target = "symbol:target") {
  return withFactProvenance({
    id: "edge:reference",
    kind: "REFERENCES",
    relation: "REFERENCES",
    source: "symbol:source",
    target,
    sourceStateId: "g1",
    sourceRelation: "current",
    confidenceTier: "EXACT_RESOLUTION",
    resolved: true,
    evidence: [{ path: "src/a.ts", startLine: 1, endLine: 1 }],
  }, FACT_PROVENANCE.AUTHORITATIVE_SEMANTIC, null);
}

test("semantic registry accepts compiler providers and rejects unrelated provider kinds", () => {
  const registry = createSemanticProviderRegistry({ providers: [semanticProvider()] });
  assert.equal(registry.list().length, 1);
  assert.throws(() => createSemanticProviderRegistry({ providers: [defineProvider({
    id: "not.semantic", version: "1.0.0", kind: "framework", protocolRange: ">=1 <2",
    capabilities: ["routes"], permissions: { filesystem: "repo-read", network: "none", process: "none" },
    probe() { return { state: "available" }; }, collect() { return { nodes: [], edges: [], reports: [] }; },
  })] }), { code: "semantic_provider_kind_invalid" });
});

test("sync semantic orchestration records indexed, unsupported, and failed terminal dispositions", () => {
  const indexed = semanticProvider();
  const unsupported = semanticProvider({ id: "semantic.missing", probe() { return { state: "unavailable", code: "index_absent", reason: "none" }; } });
  const failed = semanticProvider({ id: "semantic.failed", collect() { throw new Error("boom"); } });
  const result = collectSemanticEvidenceSync({}, { providers: [indexed, unsupported, failed] });
  assert.deepEqual(result.results.map((entry) => entry.disposition.disposition), ["indexed", "unsupported", "failed"]);
  assert.equal(result.results[2].output.edges.length, 0);
});

test("semantic orchestration rejects undeclared relationship output instead of ingesting it", () => {
  const bad = semanticProvider({ collect() {
    return { nodes: [], edges: [{ id: "e", kind: "NOT_A_RELATION", source: "a", target: "b" }], reports: [] };
  } });
  const result = collectSemanticEvidenceSync({}, { providers: [bad] });
  assert.equal(result.results[0].disposition.disposition, "failed");
  assert.match(result.results[0].disposition.code, /relationship|provider/i);
});

test("async semantic orchestration types timeout without promoting partial output", async () => {
  const hanging = semanticProvider({ async collect() { return new Promise(() => {}); } });
  const result = await collectSemanticEvidence({}, { providers: [hanging], timeoutMs: 10 });
  assert.equal(result.results[0].disposition.disposition, "timed_out");
  assert.deepEqual(result.results[0].output.nodes, []);
});

test("live verifier agrees without becoming canonical", async () => {
  const result = await crossCheckWithLiveVerifier({
    canonical: canonical(), sourceStateId: "g1",
    verifier: async () => ({ provider: "lsp-typescript", targetId: "symbol:target", sourceStateId: "g1", evidence: [{ path: "src/a.ts", startLine: 1, endLine: 1 }] }),
  });
  assert.equal(result.state, "agreement");
  assert.equal(result.evaluation.admitted.target, "symbol:target");
  assert.equal(result.verification.provenance, FACT_PROVENANCE.LIVE_VERIFICATION);
});

test("live verifier disagreement returns resolution_conflict and preserves canonical target", async () => {
  const result = await crossCheckWithLiveVerifier({
    canonical: canonical(), sourceStateId: "g1",
    verifier: async () => ({ provider: "lsp-typescript", targetId: "symbol:other", sourceStateId: "g1" }),
  });
  assert.equal(result.state, "resolution_conflict");
  assert.equal(result.canonical.target, "symbol:target");
  assert.equal(result.verification.target, "symbol:other");
});

test("missing or slow live verifier degrades typed and never invents a target", async () => {
  const absent = await crossCheckWithLiveVerifier({ canonical: canonical(), verifier: null });
  assert.equal(absent.state, "unavailable");
  assert.equal(absent.reason, "live_verifier_unavailable");
  const slow = await crossCheckWithLiveVerifier({ canonical: canonical(), timeoutMs: 5, verifier: async () => new Promise(() => {}) });
  assert.equal(slow.state, "unavailable");
  assert.equal(slow.reason, "live_verification_timeout");
});
