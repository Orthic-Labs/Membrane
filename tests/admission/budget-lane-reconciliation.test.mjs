// tests/admission/budget-lane-reconciliation.test.mjs — MBR-608.
//
// Verifies that the JS renderer keeps ONE cross-provider budget with explicit
// lanes and that every receipt reconciles its selected and delivered tokens
// under that budget. The tests assert the contract FROM THE JS SIDE: the
// renderer classifies each block into exactly one lane, stamps the lane on
// the block, and stamps a single reconciliation on the packet. The Rust
// mirror lives in engine/crates/membrane-core/src/reconcile.rs and the
// typed contract lives in engine/crates/membrane-protocol/src/types.rs.
//
// This file is part of the SAME task commit. Test execution is deferred to
// the Book 1 gate.

import assert from "node:assert/strict";
import test from "node:test";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const {
  BUDGET_LANE_KINDS,
  BUDGET_RECONCILIATION_ALERTS,
  ContextSessionV1,
  DEFAULT_PACKET_CHAR_BUDGET,
  classifyBudgetLane,
  finalize,
  reconcileBudget,
} = require("../../mcp/context-renderer-lib.cjs");

function blockOf(overrides = {}) {
  return {
    id: "b1",
    provider: "federated",
    text: "hello",
    resolver: "read a.md",
    priority: 1,
    sourceHash: "sha256:aaa",
    selectedTokens: 25,
    estimatedTokens: 25,
    ...overrides,
  };
}

test("BUDGET_LANE_KINDS lists exactly the four explicit lanes in contract order", () => {
  assert.deepEqual(
    [...BUDGET_LANE_KINDS],
    ["native", "rendered", "resolver_backed", "metadata_only"],
    "lane order must be the MBR-608 contract order",
  );
});

test("classifyBudgetLane routes every block to exactly one lane", () => {
  assert.equal(classifyBudgetLane({ deliveryClass: "rendered", deliveryMode: "inline" }), "rendered");
  assert.equal(classifyBudgetLane({ deliveryClass: "metadata_only", deliveryMode: "reference" }), "metadata_only");
  assert.equal(classifyBudgetLane({ deliveryClass: "resolver_backed", deliveryMode: "reference" }), "resolver_backed");
  // Native delivery wins regardless of deliveryClass.
  assert.equal(classifyBudgetLane({ deliveryClass: "rendered", deliveryMode: "native" }), "native");
  assert.equal(classifyBudgetLane({ deliveryMode: "native" }), "native");
  // Unknown deliveryClass is unclassified.
  assert.equal(classifyBudgetLane({ deliveryClass: "inline", deliveryMode: "inline" }), null);
  assert.equal(classifyBudgetLane({}), null);
  assert.equal(classifyBudgetLane(null), null);
});

test("finalize stamps the lane on every rendered block", () => {
  const packet = {
    budget: { maxTokens: 1000 },
    blocks: [
      blockOf({ id: "rendered", text: "rendered body" }),
      blockOf({ id: "metadata", text: "", deliveryClass: "metadata_only", deliveryMode: "reference" }),
    ],
  };
  finalize(packet, DEFAULT_PACKET_CHAR_BUDGET);
  const lanes = packet.blocks.map((block) => block.lane);
  assert.ok(lanes.includes("rendered"), "rendered block must land in the rendered lane");
  assert.ok(lanes.includes("metadata_only"), "metadata block must land in the metadata_only lane");
});

test("finalize stamps a reconciliation on the packet", () => {
  const packet = {
    budget: { maxTokens: 1000 },
    blocks: [blockOf({ selectedTokens: 25, renderedTokens: 25, deliveredChars: 100, text: "hello" })],
  };
  finalize(packet, DEFAULT_PACKET_CHAR_BUDGET);
  assert.ok(packet.reconciliation, "reconciliation must be stamped on the packet");
  assert.equal(packet.reconciliation.schemaVersion, 1);
  assert.equal(packet.reconciliation.maxTokens, 1000);
  assert.equal(packet.reconciliation.balanced, true);
  assert.equal(packet.reconciliation.unclassifiedBlocks, 0);
  // Every lane row is present, in the contract order.
  assert.deepEqual(
    packet.reconciliation.lanes.map((row) => row.lane),
    [...BUDGET_LANE_KINDS],
  );
});

test("reconcileBudget sums per-lane totals to the global total (MBR-608)", () => {
  const packet = {
    budget: { maxTokens: 1000 },
    blocks: [
      blockOf({ id: "r1", selectedTokens: 30, renderedTokens: 30, deliveredChars: 120, text: "abc" }),
      blockOf({ id: "r2", selectedTokens: 20, renderedTokens: 20, deliveredChars: 80, text: "def" }),
      blockOf({ id: "m1", deliveryClass: "metadata_only", deliveryMode: "reference", selectedTokens: 0, renderedTokens: 0, deliveredChars: 0, text: "" }),
    ],
  };
  finalize(packet, DEFAULT_PACKET_CHAR_BUDGET);
  const reconciliation = packet.reconciliation;
  assert.equal(reconciliation.totalSelectedTokens, 50);
  assert.equal(reconciliation.totalDeliveredTokens, 50);
  assert.equal(reconciliation.totalDeliveredChars, 200);
  // Every per-lane row equals the sum of the per-block lane totals.
  for (const row of reconciliation.lanes) {
    const sum = packet.blocks
      .filter((block) => block.lane === row.lane)
      .reduce((acc, block) => ({
        selectedTokens: acc.selectedTokens + (block.selectedTokens ?? 0),
        deliveredTokens: acc.deliveredTokens + (block.renderedTokens ?? 0),
        deliveredChars: acc.deliveredChars + (block.deliveredChars ?? 0),
        blocks: acc.blocks + 1,
      }), { selectedTokens: 0, deliveredTokens: 0, deliveredChars: 0, blocks: 0 });
    if (row.lane !== "rendered") {
      // Non-rendered lanes never contribute delivered tokens.
      sum.deliveredTokens = 0;
    }
    assert.equal(row.selectedTokens, sum.selectedTokens, `${row.lane} selectedTokens`);
    assert.equal(row.deliveredTokens, sum.deliveredTokens, `${row.lane} deliveredTokens`);
    assert.equal(row.deliveredChars, sum.deliveredChars, `${row.lane} deliveredChars`);
    assert.equal(row.blocks, sum.blocks, `${row.lane} blocks`);
  }
});

test("reconcileBudget flags a global-ceiling breach as the alert", () => {
  const packet = {
    budget: { maxTokens: 100 },
    blocks: [
      blockOf({
        id: "too-big",
        selectedTokens: 200,
        renderedTokens: 200,
        deliveredChars: 800,
        text: "x".repeat(800),
      }),
    ],
  };
  finalize(packet, DEFAULT_PACKET_CHAR_BUDGET);
  assert.equal(packet.reconciliation.balanced, false);
  assert.equal(packet.reconciliation.alert, BUDGET_RECONCILIATION_ALERTS.globalCeilingExceeded);
});

test("reconcileBudget flags a selected-without-delivered drift as the alert", () => {
  const packet = {
    budget: { maxTokens: 1000 },
    blocks: [
      blockOf({
        id: "dropped",
        selectedTokens: 50,
        renderedTokens: 0,
        deliveredChars: 0,
        text: "x".repeat(500),
      }),
    ],
  };
  // Force a tiny budget so the block is dropped (rendered tokens stay 0).
  finalize(packet, 10);
  assert.equal(packet.reconciliation.balanced, false);
  assert.equal(packet.reconciliation.alert, BUDGET_RECONCILIATION_ALERTS.selectedWithoutDelivered);
});

test("reconcileBudget flags an unclassified block when the drift is a missing lane", () => {
  const packet = {
    budget: { maxTokens: 1000 },
    blocks: [
      blockOf({ id: "no-lane", selectedTokens: 0, renderedTokens: 0, deliveredChars: 0 }),
    ],
  };
  // Simulate a contract drift: the renderer forgot to stamp the lane.
  packet.blocks[0].lane = null;
  packet.blocks[0].deliveryClass = "inline"; // unknown to the lane classifier
  const reconciliation = reconcileBudget(packet);
  assert.equal(reconciliation.balanced, false);
  assert.equal(reconciliation.unclassifiedBlocks, 1);
  assert.equal(reconciliation.alert, BUDGET_RECONCILIATION_ALERTS.unclassified);
});

test("a natively-loaded rule is recorded in the native lane at zero tokens", () => {
  const sourceHash = `sha256:${"a".repeat(64)}`;
  const session = new ContextSessionV1({
    sessionId: "s1",
    client: "claude_code",
    hostReceipts: [{
      schema: "orthic.host-delivery-receipt.v1",
      receipt_id: "r1",
      client: "claude_code",
      sessionId: "s1",
      sourceHash,
      deliveredAt: "2026-08-07T00:00:00Z",
      mechanism: "workspace_rules",
    }],
  });
  const packet = {
    budget: { maxTokens: 1000 },
    blocks: [
      blockOf({
        id: "rules:AGENTS.md",
        sourceKind: "doc",
        text: "RULE BODY",
        sourceHash,
        selectedTokens: 0,
        renderedTokens: 0,
        deliveredChars: 0,
      }),
    ],
  };
  // applyDeliveryLedger stamps deliveryMode = "native" for receipt-matched rules.
  const { applyDeliveryLedger } = require("../../mcp/context-renderer-lib.cjs");
  applyDeliveryLedger(packet, session);
  finalize(packet, DEFAULT_PACKET_CHAR_BUDGET);
  assert.equal(packet.blocks[0].lane, "native");
  const nativeRow = packet.reconciliation.lanes.find((row) => row.lane === "native");
  assert.equal(nativeRow.blocks, 1);
  assert.equal(nativeRow.selectedTokens, 0);
  assert.equal(nativeRow.deliveredTokens, 0);
  assert.equal(packet.reconciliation.balanced, true);
});

test("every receipt carries a reconciliation, even when the packet is empty", () => {
  const packet = { budget: { maxTokens: 0 }, blocks: [] };
  const reconciliation = reconcileBudget(packet);
  assert.equal(reconciliation.totalSelectedTokens, 0);
  assert.equal(reconciliation.totalDeliveredTokens, 0);
  assert.equal(reconciliation.balanced, true);
  assert.equal(reconciliation.unclassifiedBlocks, 0);
  assert.equal(reconciliation.lanes.length, BUDGET_LANE_KINDS.length);
});
