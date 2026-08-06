// Contract tests for the Membrane-owned context renderer (plan 2.2) and the
// ContextSessionV1 delivery ledger (plan 2.3).
//
// These assert behavior that used to live in Forge's repo. The point of
// moving it is that ONE renderer is under test — previously each adapter could
// drift from the other and only one was ever covered.

import assert from "node:assert/strict";
import test from "node:test";

import {
  CLIENT_IDENTITIES,
  ContextSessionV1,
  DEFAULT_PACKET_CHAR_BUDGET,
  NATIVE_DELIVERY_STATUSES,
  applyDeliveryLedger,
  finalize,
  loadsWorkspaceRules,
  matchHostDeliveryReceipt,
  render,
  typedClient,
  validateHostDeliveryReceipt,
} from "./context-renderer.mjs";
import { createRequire } from "node:module";

// MBR-011 owns context-renderer-lib.cjs (not context-renderer.mjs in this
// book), so the delivery-outcome symbols are pulled from the CJS lib directly.
const require = createRequire(import.meta.url);
const { evaluateDeliveryOutcome, SELECTED_WITHOUT_DELIVERY_ALERT } = require("./context-renderer-lib.cjs");

function blockOf(overrides = {}) {
  return {
    id: "b1",
    provider: "federated",
    text: "hello",
    resolver: "read a.md",
    priority: 1,
    sourceHash: "sha256:aaa",
    ...overrides,
  };
}

test("typedClient maps every unknown identity to 'other', never a new ad-hoc string", () => {
  // Plan convention 3: the retired ccx/host-adapter strings must not survive
  // anywhere; an unrecognized client is 'other', not itself.
  assert.equal(typedClient("claude_code"), "claude_code");
  assert.equal(typedClient("codex"), "codex");
  assert.equal(typedClient("ccx"), "other");
  assert.equal(typedClient("host-adapter"), "other");
  assert.equal(typedClient(""), "other");
  assert.equal(typedClient(undefined), "other");
  for (const id of CLIENT_IDENTITIES) assert.equal(typedClient(id), id);
});

test("self-loading capability follows the typed identity", () => {
  assert.equal(loadsWorkspaceRules("claude_code"), true);
  assert.equal(loadsWorkspaceRules("codex"), true);
  assert.equal(loadsWorkspaceRules("api_worker"), false);
  // The retired string must NOT be treated as self-loading by accident.
  assert.equal(loadsWorkspaceRules("ccx"), false);
});

test("finalize renders within the effective budget and stamps accounting", () => {
  const packet = { blocks: [blockOf(), blockOf({ id: "b2", text: "world" })] };
  const { body, deliveredChars } = finalize(packet, DEFAULT_PACKET_CHAR_BUDGET);
  assert.match(body, /--- b1 \(federated\) ---/);
  assert.match(body, /--- b2 \(federated\) ---/);
  assert.ok(deliveredChars > 0);
  assert.equal(packet.blocks[0].deliveryClass, "rendered");
  assert.equal(packet.blocks[0].dropReason, "none");
  assert.equal(packet.budget.packetCharBudgetDefault, DEFAULT_PACKET_CHAR_BUDGET);
  assert.ok(packet.providerAccounting.federated);
});

test("finalize drops over-budget blocks to resolver_backed rather than truncating them", () => {
  const packet = { blocks: [blockOf({ text: "x".repeat(500) })] };
  finalize(packet, 50);
  assert.equal(packet.blocks[0].dropReason, "packet_budget_exceeded");
  assert.equal(packet.blocks[0].deliveryClass, "resolver_backed");
  assert.equal(packet.blocks[0].deliveredChars, 0);
});

test("finalize renders highest priority first, stable within a priority", () => {
  const packet = {
    blocks: [
      blockOf({ id: "low", priority: 1, text: "low" }),
      blockOf({ id: "high", priority: 9, text: "high" }),
      blockOf({ id: "low2", priority: 1, text: "low2" }),
    ],
  };
  const { body } = finalize(packet, DEFAULT_PACKET_CHAR_BUDGET);
  assert.ok(body.indexOf("--- high") < body.indexOf("--- low "), body);
  assert.ok(body.indexOf("--- low ") < body.indexOf("--- low2"), body);
});

test("a self-loading host with a matching host receipt never receives rule bytes (MBR-010)", () => {
  // Native delivery is allowed ONLY because the host issued a receipt that
  // matches client + sessionId + sourceHash. The block is not serialized.
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
    blocks: [blockOf({ id: "rules:AGENTS.md", sourceKind: "doc", text: "RULE BODY", sourceHash })],
  };
  applyDeliveryLedger(packet, session);
  assert.equal(packet.blocks[0].text, "", "natively-loaded rule body must not be serialized");
  assert.equal(packet.blocks[0].deliveryMode, "native");
  assert.equal(packet.blocks[0].delivery, "native");
  const entry = session.delivered.find((d) => d.id === "rules:AGENTS.md");
  assert.equal(entry.deliveryMode, "native");
  assert.equal(entry.bytes, 0);
});

test("MBR-010: absent host receipt produces delivery=missing, never native", () => {
  // A self-loading host with NO receipt must not be assumed to have the rules.
  const sourceHash = `sha256:${"b".repeat(64)}`;
  const session = new ContextSessionV1({ sessionId: "s1", client: "claude_code" });
  const packet = {
    blocks: [blockOf({ id: "rules:AGENTS.md", sourceKind: "doc", text: "RULE BODY", sourceHash })],
  };
  applyDeliveryLedger(packet, session);
  assert.equal(packet.blocks[0].delivery, "missing");
  assert.notEqual(packet.blocks[0].deliveryMode, "native", "no receipt → never native-delivered");
  assert.equal(packet.blocks[0].text, "RULE BODY", "unproven native claim falls back to inline delivery");
  assert.equal(packet.blocks[0].nativeDeliveryGap, "no_host_receipt");
});

test("MBR-010: mismatched host receipt produces delivery=unknown, never native", () => {
  // The host issued a receipt, but for DIFFERENT content — it does not prove
  // this block was natively loaded.
  const session = new ContextSessionV1({
    sessionId: "s1",
    client: "claude_code",
    hostReceipts: [{
      schema: "orthic.host-delivery-receipt.v1",
      receipt_id: "r1",
      client: "claude_code",
      sessionId: "s1",
      sourceHash: `sha256:${"0".repeat(64)}`, // different content
      deliveredAt: "2026-08-07T00:00:00Z",
      mechanism: "workspace_rules",
    }],
  });
  const packet = {
    blocks: [blockOf({ id: "rules:AGENTS.md", sourceKind: "doc", text: "RULE BODY", sourceHash: `sha256:${"c".repeat(64)}` })],
  };
  applyDeliveryLedger(packet, session);
  assert.equal(packet.blocks[0].delivery, "unknown");
  assert.notEqual(packet.blocks[0].deliveryMode, "native", "mismatched receipt → never native-delivered");
  assert.equal(packet.blocks[0].nativeDeliveryGap, "receipt_mismatch");
});

test("MBR-010: a malformed receipt is never proof of native delivery", () => {
  const session = new ContextSessionV1({
    sessionId: "s1",
    client: "codex",
    hostReceipts: [{ schema: "wrong.schema", receipt_id: "x" }],
  });
  const status = session.nativeDeliveryStatus(`sha256:${"d".repeat(64)}`);
  assert.equal(status, "unknown");
  assert.equal(validateHostDeliveryReceipt({ schema: "wrong.schema" }), null);
});

test("MBR-010: matchHostDeliveryReceipt distinguishes native/unknown/missing", () => {
  const hash = `sha256:${"e".repeat(64)}`;
  const good = { schema: "orthic.host-delivery-receipt.v1", receipt_id: "r", client: "codex", sessionId: "s", sourceHash: hash, deliveredAt: "2026-08-07T00:00:00Z", mechanism: "native_load" };
  assert.equal(matchHostDeliveryReceipt([], { client: "codex", sessionId: "s", sourceHash: hash }), "missing");
  assert.equal(matchHostDeliveryReceipt([good], { client: "codex", sessionId: "s", sourceHash: hash }), "native");
  assert.equal(matchHostDeliveryReceipt([good], { client: "codex", sessionId: "s", sourceHash: `sha256:${"f".repeat(64)}` }), "unknown");
  assert.equal(matchHostDeliveryReceipt([good], { client: "claude_code", sessionId: "s", sourceHash: hash }), "unknown");
  assert.deepEqual([...NATIVE_DELIVERY_STATUSES].sort(), ["missing", "native", "unknown"]);
});

test("a headless client receives rule content inline", () => {
  const session = new ContextSessionV1({ sessionId: "s1", client: "api_worker" });
  const packet = {
    blocks: [blockOf({ id: "rules:AGENTS.md", sourceKind: "doc", text: "RULE BODY" })],
  };
  applyDeliveryLedger(packet, session);
  assert.equal(packet.blocks[0].text, "RULE BODY");
  assert.equal(packet.blocks[0].deliveryMode, "inline");
  assert.ok(session.delivered[0].bytes > 0);
});

test("an unchanged second turn delivers zero static bytes", () => {
  // Plan 2.4: continuation delivers nothing unchanged.
  const session = new ContextSessionV1({ sessionId: "s1", client: "api_worker" });
  const first = { blocks: [blockOf({ id: "doc:spine", text: "SPINE" })] };
  applyDeliveryLedger(first, session);
  assert.equal(first.blocks[0].text, "SPINE");

  const second = { blocks: [blockOf({ id: "doc:spine", text: "SPINE" })] };
  applyDeliveryLedger(second, session);
  assert.equal(second.blocks[0].text, "", "unchanged content must not ride the prompt twice");
  assert.equal(second.blocks[0].dropReason, "already_delivered");
});

test("changed content invalidates and re-delivers", () => {
  const session = new ContextSessionV1({ sessionId: "s1", client: "api_worker" });
  applyDeliveryLedger({ blocks: [blockOf({ id: "doc:spine", sourceHash: "sha256:v1" })] }, session);
  const changed = { blocks: [blockOf({ id: "doc:spine", sourceHash: "sha256:v2", text: "NEW" })] };
  applyDeliveryLedger(changed, session);
  assert.equal(changed.blocks[0].text, "NEW", "a changed sourceHash must re-deliver");
  assert.equal(changed.blocks[0].deliveryMode, "inline");
});

test("record rejects an untyped delivery mode", () => {
  const session = new ContextSessionV1({ sessionId: "s1", client: "codex" });
  assert.throws(() => session.record("x", "smuggled", "sha256:a"), /deliveryMode must be one of/);
});

test("render emits the header, the data-only wrapper, and metadata without duplicating text", () => {
  const session = new ContextSessionV1({ sessionId: "s1", client: "api_worker" });
  const out = render(
    {
      state: "context_enforced",
      eventStore: { status: "persisted" },
      payload: {
        providerStatus: "fresh",
        degradationReason: "none",
        receipts: [],
        packet: { blocks: [blockOf({ text: "PAYLOAD TEXT" })] },
      },
    },
    { session },
  );
  assert.match(out, /^Membrane: context_enforced/);
  assert.match(out, /instructionPolicy="data_only"/);
  assert.match(out, /PAYLOAD TEXT/);
  // The body carries the text; the metadata block must not repeat it.
  const dataBlock = out.slice(out.indexOf("<membrane-context-data>"));
  assert.ok(!dataBlock.includes("PAYLOAD TEXT"), "metadata block must not duplicate rendered text");
  assert.match(dataBlock, /"contextSession"/);
});

test("render reports a degraded state honestly and ships no body", () => {
  const out = render({ state: "degraded", reason: "cortex_unavailable", payload: {} });
  assert.match(out, /^Membrane: degraded/);
  assert.match(out, /omissions: cortex_unavailable/);
  assert.ok(!out.includes("<membrane-context "), "a degraded result must not render a context body");
});

test("MBR-011: selected>0 with delivered=0 is a degraded visible failure that blocks promotion", () => {
  // Two selected blocks, but the packet budget drops both → nothing reaches the
  // agent. That is a failure, not a quiet success.
  const packet = { blocks: [blockOf({ text: "x".repeat(500), selectedTokens: 100 }), blockOf({ id: "b2", text: "y".repeat(500), selectedTokens: 50 })] };
  finalize(packet, 50); // budget too small to render either
  const outcome = packet.deliveryOutcome;
  assert.equal(outcome.status, "degraded");
  assert.equal(outcome.alert, SELECTED_WITHOUT_DELIVERY_ALERT);
  assert.equal(outcome.releasePromotionBlocked, true);
  assert.ok(outcome.selected > 0);
  assert.equal(outcome.delivered, 0);
});

test("MBR-011: the alert id is stable and exactly one is emitted", () => {
  assert.equal(SELECTED_WITHOUT_DELIVERY_ALERT, "selected_without_delivery");
  const outcome = evaluateDeliveryOutcome({ blocks: [blockOf({ selectedTokens: 10, deliveredChars: 0 })] });
  assert.equal(outcome.alert, "selected_without_delivery");
});

test("MBR-011: any delivered block clears the failure", () => {
  const outcome = evaluateDeliveryOutcome({ blocks: [blockOf({ selectedTokens: 10, deliveredChars: 5 })] });
  assert.equal(outcome.status, "ok");
  assert.equal(outcome.alert, null);
  assert.equal(outcome.releasePromotionBlocked, false);
});

test("MBR-011: native or already-delivered blocks count as delivered (no false failure)", () => {
  // A natively-loaded rule (MBR-010) delivered zero rendered chars but DID
  // reach the agent — it must not trigger a selected-without-delivery failure.
  const outcome = evaluateDeliveryOutcome({ blocks: [blockOf({ selectedTokens: 10, deliveryMode: "native", deliveredChars: 0 })] });
  assert.equal(outcome.status, "ok");
  const referenced = evaluateDeliveryOutcome({ blocks: [blockOf({ selectedTokens: 10, deliveredChars: 0, dropReason: "already_delivered" })] });
  assert.equal(referenced.status, "ok");
});
