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
  applyDeliveryLedger,
  finalize,
  loadsWorkspaceRules,
  render,
  typedClient,
} from "./context-renderer.mjs";

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

test("a self-loading host never receives rule bytes but the delivery is recorded", () => {
  // Plan 2.3: native-loading hosts get rules marked delivered-by-host and
  // never serialized — the duplicate-truncated-rules bug.
  const session = new ContextSessionV1({ sessionId: "s1", client: "claude_code" });
  const packet = {
    blocks: [blockOf({ id: "rules:AGENTS.md", sourceKind: "doc", text: "RULE BODY" })],
  };
  applyDeliveryLedger(packet, session);
  assert.equal(packet.blocks[0].text, "", "rule body must not be serialized to a self-loading host");
  assert.equal(packet.blocks[0].deliveryMode, "native");
  const entry = session.delivered.find((d) => d.id === "rules:AGENTS.md");
  assert.equal(entry.deliveryMode, "native");
  assert.equal(entry.bytes, 0);
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
