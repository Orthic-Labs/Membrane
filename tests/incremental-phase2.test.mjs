import assert from "node:assert/strict";
import test from "node:test";

import {
  DIMENSIONS,
  buildIncrementalPhase2Plan,
  isReconciliationDecisionCurrent,
  reconcileInputFingerprint,
  sealPhase2Artifacts,
} from "../src/lib/incremental-phase2.mjs";

function graph(generationId, hashes) {
  return {
    manifest: { generationId },
    nodes: Object.entries(hashes).map(([path, contentHash]) => ({
      id: `file:${path}`,
      kind: "file",
      path,
      evidence: [{ path, startLine: 1, endLine: 1, contentHash }],
    })),
  };
}

const queue = {
  claims: [
    {
      id: "claim-a",
      source: "docs/a.md",
      line: 1,
      status: "implemented",
      text: "A is implemented.",
      candidateFiles: ["src/a.ts"],
    },
    {
      id: "claim-b",
      source: "docs/b.md",
      line: 1,
      status: "implemented",
      text: "B is implemented.",
      candidateFiles: ["src/b.ts"],
    },
  ],
  anchors: ["src/a.ts", "src/b.ts"],
};

function draftUnderstanding() {
  return {
    architecture: { summary: "Architecture" },
    interfaces: { summary: "Interfaces" },
    health: { summary: "Health" },
    contract: { summary: "Contract" },
    security: { summary: "Security" },
    solid: { summary: "Solid" },
    incremental: {
      dimensions: Object.fromEntries(DIMENSIONS.map((dimension) => [
        dimension,
        dimension === "security"
          ? { inputFiles: ["docs/a.md", "src/a.ts"], inputVerdictIds: ["claim-a"] }
          : { inputFiles: ["docs/b.md", "src/b.ts"], inputVerdictIds: ["claim-b"] },
      ])),
    },
  };
}

const baseHashes = {
  "docs/a.md": "doc-a-v1",
  "docs/b.md": "doc-b-v1",
  "src/a.ts": "code-a-v1",
  "src/b.ts": "code-b-v1",
};

test("cold Phase 2 schedules every verdict and synthesis dimension", () => {
  const plan = buildIncrementalPhase2Plan({
    graph: graph("sha256:g1", baseHashes),
    queue,
    verdictEnvelope: null,
    understanding: null,
  });

  assert.deepEqual(plan.verdicts.reuse, []);
  assert.deepEqual(plan.verdicts.verify.map((item) => item.claimId), ["claim-a", "claim-b"]);
  assert.deepEqual(plan.dimensions.reuse, []);
  assert.deepEqual(plan.dimensions.synthesize.map((item) => item.dimension), DIMENSIONS);
  assert.equal(plan.complete, false);
});

test("unchanged dependencies reuse Phase 2 across whole-repo generations", () => {
  const firstGraph = graph("sha256:g1", baseHashes);
  const sealed = sealPhase2Artifacts({
    graph: firstGraph,
    queue,
    verdictEnvelope: {
      verdicts: [
        { claimId: "claim-a", verdict: "verified", evidence: "src/a.ts:1", note: "A" },
        { claimId: "claim-b", verdict: "verified", evidence: "src/b.ts:1", note: "B" },
      ],
    },
    understanding: draftUnderstanding(),
  });
  const nextGraph = graph("sha256:g2", baseHashes);
  const plan = buildIncrementalPhase2Plan({
    graph: nextGraph,
    queue,
    verdictEnvelope: sealed.verdicts,
    understanding: sealed.understanding,
  });

  assert.deepEqual(plan.verdicts.reuse.map((item) => item.claimId), ["claim-a", "claim-b"]);
  assert.deepEqual(plan.verdicts.verify, []);
  assert.deepEqual(plan.dimensions.reuse, DIMENSIONS);
  assert.deepEqual(plan.dimensions.synthesize, []);
  assert.equal(plan.complete, true);
  assert.equal(
    sealed.verdicts.verdicts[0].reconciliationFingerprint,
    reconcileInputFingerprint(sealed.verdicts.verdicts[0]),
  );
});

test("a new source file invalidates synthesis discovery but reuses unchanged verdicts", () => {
  const firstGraph = graph("sha256:g1", baseHashes);
  const sealed = sealPhase2Artifacts({
    graph: firstGraph,
    queue,
    verdictEnvelope: {
      verdicts: [
        { claimId: "claim-a", verdict: "verified", evidence: "src/a.ts:1", note: "A" },
        { claimId: "claim-b", verdict: "verified", evidence: "src/b.ts:1", note: "B" },
      ],
    },
    understanding: draftUnderstanding(),
  });
  const plan = buildIncrementalPhase2Plan({
    graph: graph("sha256:g2", { ...baseHashes, "src/new-component.ts": "new-file" }),
    queue,
    verdictEnvelope: sealed.verdicts,
    understanding: sealed.understanding,
  });

  assert.deepEqual(plan.verdicts.verify, []);
  assert.deepEqual(plan.dimensions.reuse, []);
  assert.deepEqual(plan.dimensions.synthesize.map((item) => item.dimension), DIMENSIONS);
  assert.ok(plan.dimensions.synthesize.every((item) => item.reason === "repository_shape_changed"));
});

test("one changed file invalidates only its verdict and dependent dimensions", () => {
  const firstGraph = graph("sha256:g1", baseHashes);
  const sealed = sealPhase2Artifacts({
    graph: firstGraph,
    queue,
    verdictEnvelope: {
      verdicts: [
        { claimId: "claim-a", verdict: "verified", evidence: "src/a.ts:1", note: "A" },
        { claimId: "claim-b", verdict: "verified", evidence: "src/b.ts:1", note: "B" },
      ],
    },
    understanding: draftUnderstanding(),
  });
  const changedGraph = graph("sha256:g2", { ...baseHashes, "src/a.ts": "code-a-v2" });
  const plan = buildIncrementalPhase2Plan({
    graph: changedGraph,
    queue,
    verdictEnvelope: sealed.verdicts,
    understanding: sealed.understanding,
  });

  assert.deepEqual(plan.verdicts.reuse.map((item) => item.claimId), ["claim-b"]);
  assert.deepEqual(plan.verdicts.verify.map((item) => item.claimId), ["claim-a"]);
  assert.deepEqual(plan.dimensions.reuse, DIMENSIONS.filter((dimension) => dimension !== "security"));
  assert.deepEqual(plan.dimensions.synthesize.map((item) => item.dimension), ["security"]);
});

test("missing evidence fails closed instead of producing a cache hit", () => {
  const firstGraph = graph("sha256:g1", baseHashes);
  const sealed = sealPhase2Artifacts({
    graph: firstGraph,
    queue,
    verdictEnvelope: {
      verdicts: [
        { claimId: "claim-a", verdict: "verified", evidence: "src/a.ts:1", note: "A" },
        { claimId: "claim-b", verdict: "verified", evidence: "src/b.ts:1", note: "B" },
      ],
    },
    understanding: draftUnderstanding(),
  });
  const { ["src/a.ts"]: _removed, ...remaining } = baseHashes;
  const plan = buildIncrementalPhase2Plan({
    graph: graph("sha256:g2", remaining),
    queue,
    verdictEnvelope: sealed.verdicts,
    understanding: sealed.understanding,
  });

  assert.deepEqual(plan.verdicts.verify.map((item) => item.claimId), ["claim-a"]);
  assert.deepEqual(plan.dimensions.synthesize.map((item) => item.dimension), DIMENSIONS);
  assert.ok(plan.verdicts.verify[0].missingInputs.includes("src/a.ts"));
});

test("a changed verdict result invalidates dependent synthesis despite unchanged files", () => {
  const firstGraph = graph("sha256:g1", baseHashes);
  const sealed = sealPhase2Artifacts({
    graph: firstGraph,
    queue,
    verdictEnvelope: {
      verdicts: [
        { claimId: "claim-a", verdict: "verified", evidence: "src/a.ts:1", note: "A" },
        { claimId: "claim-b", verdict: "verified", evidence: "src/b.ts:1", note: "B" },
      ],
    },
    understanding: draftUnderstanding(),
  });
  sealed.verdicts.verdicts.find((item) => item.claimId === "claim-a").verdict = "contradicted";
  const plan = buildIncrementalPhase2Plan({
    graph: graph("sha256:g2", baseHashes),
    queue,
    verdictEnvelope: sealed.verdicts,
    understanding: sealed.understanding,
  });

  assert.deepEqual(plan.verdicts.verify, []);
  assert.deepEqual(plan.dimensions.synthesize.map((item) => item.dimension), ["security"]);
});

test("reconciliation decisions are reusable only for an unchanged verdict result", () => {
  const verified = { claimId: "claim-a", verdict: "contradicted", evidence: "src/a.ts:1", note: "A" };
  const decision = {
    claimId: "claim-a",
    decision: "update-doc",
    inputFingerprint: reconcileInputFingerprint(verified),
  };
  assert.equal(isReconciliationDecisionCurrent(verified, decision), true);
  assert.equal(isReconciliationDecisionCurrent(verified, { ...decision, inputFingerprint: "xxh128:stale" }), false);
  assert.equal(isReconciliationDecisionCurrent({ ...verified, note: "different reality" }, decision), false);
  assert.equal(isReconciliationDecisionCurrent(verified, { ...decision, decision: null }), false);
  assert.match(decision.inputFingerprint, /^xxh128:[a-f0-9]{32}$/, "fresh fingerprints must be minted in the xxh128 form");
});

// Migration guard (locked 2026-07-25): a Phase-4 user decision sealed BEFORE
// the sha256 -> xxh3 algorithm switch must still be honoured after the
// switch. isReconciliationDecisionCurrent gates a STORED USER DECISION, so
// silently invalidating it on an algorithm change would re-open every
// already-settled reconciliation the next time Phase 2 runs.
test("a decision with no recorded human choice is never current, whatever its fingerprint", () => {
  // Replaces the old legacy-sha256 compatibility test. That branch was removed
  // 2026-07-26: across every .agent/reconcile.json on disk there were 11
  // entries and ZERO with a non-null `decision`, so the guard below already
  // rejected all of them and the legacy path was unreachable. This asserts the
  // guard that made it dead code, so the reasoning stays checkable.
  const verified = { claimId: "claim-a", verdict: "contradicted", evidence: "src/a.ts:1", note: "A" };
  const legacyShaped = {
    claimId: "claim-a",
    decision: null,
    inputFingerprint: `sha256:${"0".repeat(64)}`,
  };
  assert.equal(isReconciliationDecisionCurrent(verified, legacyShaped), false);

  // And a real decision is current only under the xxh128 fingerprint.
  const current = {
    claimId: "claim-a",
    decision: "update-doc",
    inputFingerprint: reconcileInputFingerprint(verified),
  };
  assert.match(current.inputFingerprint, /^xxh128:[a-f0-9]{32}$/);
  assert.equal(isReconciliationDecisionCurrent(verified, current), true);
  assert.equal(
    isReconciliationDecisionCurrent({ ...verified, note: "different reality" }, current),
    false,
    "a genuinely changed verdict must still go stale",
  );
});
