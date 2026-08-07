// MBR-505 acceptance test — auto-use bounded working context.
//
// Walks the bounded working-context contract at the JS level (the Rust
// twin lives in `engine/crates/membrane-runtime/src/working_context.rs`)
// and asserts every property of the MBR-505 acceptance condition:
//
//   "The runtime selects and renders a working context automatically,
//    but only inside an explicit budget; if the budget is exceeded the
//    runtime emits a typed `context.budget_exceeded` envelope and falls
//    back to metadata-only delivery."
//
// Run with:  node --test tests/session/working-context.test.mjs

import test from "node:test";
import assert from "node:assert/strict";

import {
  selectWorkingContext,
  renderWorkingContext,
  verifyWorkingContextEnvelope,
  recomputeWorkingContextDigest,
  WORKING_CONTEXT_KIND_NORMAL,
  WORKING_CONTEXT_KIND_BUDGET_EXCEEDED,
  WORKING_CONTEXT_BUDGET_SCHEMA_VERSION,
  WORKING_CONTEXT_SELECTION_SCHEMA_VERSION,
  WORKING_CONTEXT_ENVELOPE_SCHEMA_VERSION,
} from "../../mcp/working-context.mjs";

test("empty input throws no_candidates", () => {
  assert.throws(
    () => selectWorkingContext([], [], { maxBlocks: 4, maxBytes: 1024, maxRecentTurns: 4 }),
    /no_candidates/,
  );
});

test("budget of zero throws budget_invalid", () => {
  // maxBlocks = 0
  assert.throws(
    () => selectWorkingContext(["a"], [10], { maxBlocks: 0, maxBytes: 1024, maxRecentTurns: 4 }),
    /budget_invalid/,
  );
  // maxBytes = 0
  assert.throws(
    () => selectWorkingContext(["a"], [10], { maxBlocks: 4, maxBytes: 0, maxRecentTurns: 4 }),
    /budget_invalid/,
  );
  // maxRecentTurns = 0
  assert.throws(
    () => selectWorkingContext(["a"], [10], { maxBlocks: 4, maxBytes: 1024, maxRecentTurns: 0 }),
    /budget_invalid/,
  );
});

test("three candidates totaling 300 bytes with budget maxBytes=1024 fits and is not exceeded", () => {
  const candidates = ["alpha", "beta", "gamma"];
  const candidateBytes = [100, 100, 100];
  const selection = selectWorkingContext(candidates, candidateBytes, {
    maxBlocks: 8,
    maxBytes: 1024,
    maxRecentTurns: 8,
  });
  assert.equal(selection.exceeded, false);
  assert.equal(selection.exceededReason, null);
  assert.deepEqual(selection.selectedBlocks, candidates);
  assert.equal(selection.selectedBytes, 300);
  assert.equal(selection.schemaVersion, WORKING_CONTEXT_SELECTION_SCHEMA_VERSION);
});

test("five candidates totaling 5000 bytes with budget maxBytes=1024 exceeds on max_bytes", () => {
  const candidates = ["a", "b", "c", "d", "e"];
  const candidateBytes = [1000, 1000, 1000, 1000, 1000];
  const selection = selectWorkingContext(candidates, candidateBytes, {
    maxBlocks: 8,
    maxBytes: 1024,
    maxRecentTurns: 8,
  });
  assert.equal(selection.exceeded, true);
  assert.equal(selection.exceededReason, "max_bytes");
  // Only the first block fits before the cumulative sum would exceed 1024.
  assert.deepEqual(selection.selectedBlocks, ["a"]);
  assert.equal(selection.selectedBytes, 1000);
});

test("ten candidates with budget maxBlocks=3 exceeds on max_blocks with three selected", () => {
  const candidates = Array.from({ length: 10 }, (_, i) => `block-${i}`);
  const candidateBytes = new Array(10).fill(1);
  const selection = selectWorkingContext(candidates, candidateBytes, {
    maxBlocks: 3,
    maxBytes: 1024,
    maxRecentTurns: 10,
  });
  assert.equal(selection.exceeded, true);
  assert.equal(selection.exceededReason, "max_blocks");
  assert.equal(selection.selectedBlocks.length, 3);
  assert.deepEqual(selection.selectedBlocks, ["block-0", "block-1", "block-2"]);
});

test("five candidates with budget maxRecentTurns=2 exceeds on max_recent_turns", () => {
  const candidates = ["t0", "t1", "t2", "t3", "t4"];
  const candidateBytes = new Array(5).fill(1);
  const selection = selectWorkingContext(candidates, candidateBytes, {
    maxBlocks: 8,
    maxBytes: 1024,
    maxRecentTurns: 2,
  });
  assert.equal(selection.exceeded, true);
  assert.equal(selection.exceededReason, "max_recent_turns");
  assert.equal(selection.selectedBlocks.length, 2);
  assert.deepEqual(selection.selectedBlocks, ["t0", "t1"]);
});

test("renderWorkingContext picks context.working_context when not exceeded and context.budget_exceeded when exceeded", () => {
  // Within budget → context.working_context
  const within = selectWorkingContext(["a"], [10], {
    maxBlocks: 4,
    maxBytes: 1024,
    maxRecentTurns: 4,
  });
  const envWithin = renderWorkingContext(within, 1_700_000_000_000);
  assert.equal(envWithin.kind, WORKING_CONTEXT_KIND_NORMAL);
  assert.equal(envWithin.kind, "context.working_context");
  assert.equal(envWithin.schemaVersion, WORKING_CONTEXT_ENVELOPE_SCHEMA_VERSION);

  // Over budget → context.budget_exceeded
  const over = selectWorkingContext(
    ["a", "b", "c", "d", "e", "f", "g", "h"],
    new Array(8).fill(1),
    { maxBlocks: 2, maxBytes: 1024, maxRecentTurns: 8 },
  );
  assert.equal(over.exceeded, true);
  const envOver = renderWorkingContext(over, 1_700_000_000_001);
  assert.equal(envOver.kind, WORKING_CONTEXT_KIND_BUDGET_EXCEEDED);
  assert.equal(envOver.kind, "context.budget_exceeded");
});

test("verifyWorkingContextEnvelope passes on a fresh envelope and rejects a tampered selection", () => {
  const selection = selectWorkingContext(["a", "b"], [10, 20], {
    maxBlocks: 8,
    maxBytes: 1024,
    maxRecentTurns: 8,
  });
  const envelope = renderWorkingContext(selection, 1_700_000_000_002);
  assert.equal(verifyWorkingContextEnvelope(envelope), true);

  // Tamper: append a rogue block to the selection.
  const tampered = {
    ...envelope,
    selection: {
      ...envelope.selection,
      selectedBlocks: [...envelope.selection.selectedBlocks, "rogue"],
      selectedBytes: envelope.selection.selectedBytes + 1,
    },
  };
  assert.equal(verifyWorkingContextEnvelope(tampered), false);
  // Re-stamp the digest and verify passes again, confirming the
  // verifier keys on digest equality (not just structural fields).
  const restamped = {
    ...tampered,
    digest: recomputeWorkingContextDigest(tampered),
  };
  assert.equal(verifyWorkingContextEnvelope(restamped), true);
});

test("digest is stable across two envelopes with equalized envelopeId and emittedAtUnixMs", () => {
  const selection = selectWorkingContext(["a", "b"], [10, 20], {
    maxBlocks: 8,
    maxBytes: 1024,
    maxRecentTurns: 8,
  });
  let a = renderWorkingContext(selection, 1_700_000_000_003);
  let b = renderWorkingContext(selection, 1_700_000_000_003);
  const fixedId = "11111111-1111-4111-8111-111111111111";
  a = { ...a, envelopeId: fixedId };
  b = { ...b, envelopeId: fixedId };
  a = { ...a, digest: recomputeWorkingContextDigest(a) };
  b = { ...b, digest: recomputeWorkingContextDigest(b) };
  assert.equal(a.digest, b.digest);
  assert.equal(verifyWorkingContextEnvelope(a), true);
  assert.equal(verifyWorkingContextEnvelope(b), true);
});

test("the budget echoed inside the selection carries the caller's caps and schema_version", () => {
  const selection = selectWorkingContext(["a"], [10], {
    maxBlocks: 4,
    maxBytes: 1024,
    maxRecentTurns: 4,
  });
  assert.equal(selection.budget.schemaVersion, WORKING_CONTEXT_BUDGET_SCHEMA_VERSION);
  assert.equal(selection.budget.maxBlocks, 4);
  assert.equal(selection.budget.maxBytes, 1024);
  assert.equal(selection.budget.maxRecentTurns, 4);
});