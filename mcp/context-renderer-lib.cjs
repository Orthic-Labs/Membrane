'use strict';
// Single source of truth for the rendering core (plan 2.2).
//
// The Membrane-owned renderer and the Sentinel hook MUST produce byte-identical
// output for the same packet. Both import/require this module rather than
// carrying their own copy. The ESM wrapper `context-renderer.mjs` re-exports
// everything here plus the ESM-only ContextSessionV1 class; the Sentinel hook
// (CJS) requires this directly.
const { createHash } = require('node:crypto');

/** Door cap for rendered content. */
const MAX_CONTEXT_CHARS = 30 * 1000;

/** Contract default packet char budget (tools/lib/context_contracts.py rejects any other default). */
const DEFAULT_PACKET_CHAR_BUDGET = 30000;

/** Max bytes of the serialized metadata block. */
const MAX_PACKET_BYTES = 64 * 1024;

/**
 * The one typed client identity (plan convention 3).
 * Adapters emit exactly these; anything else degrades to "other".
 */
const CLIENT_IDENTITIES = Object.freeze([
  "claude_code",
  "codex",
  "mcp",
  "api_worker",
  "other",
]);

/**
 * Clients whose host loads workspace rule files itself at session start.
 * Kept in sync with engine/federation/providers/rules.py:SELF_LOADING_RULE_CLIENTS.
 */
const SELF_LOADING_RULE_CLIENTS = Object.freeze(["claude_code", "codex"]);

/** How a block's content reached the agent. */
const DELIVERY_MODES = Object.freeze(["native", "inline", "reference"]);

function digest(value) {
  return `sha256:${createHash("sha256")
    .update(typeof value === "string" ? value : JSON.stringify(value))
    .digest("hex")}`;
}

/** Normalize any client string to the typed enum. */
function typedClient(value) {
  const candidate = String(value || "").trim();
  return CLIENT_IDENTITIES.includes(candidate) ? candidate : "other";
}

function loadsWorkspaceRules(client) {
  return SELF_LOADING_RULE_CLIENTS.includes(typedClient(client));
}

/**
 * Select blocks into the char budget and stamp per-block delivery accounting.
 *
 * This is the ONLY renderer; both the Membrane ESM surface and the Sentinel
 * CJS hook delegate here so they can never drift.
 */
function finalize(packet, doorChars = MAX_CONTEXT_CHARS) {
  const blocks = Array.isArray(packet.blocks) ? packet.blocks : [];
  const budget =
    packet.budget && typeof packet.budget === "object" ? packet.budget : (packet.budget = {});
  const configured = Number.isInteger(budget.configuredPacketCharBudget)
    ? budget.configuredPacketCharBudget
    : Number.isInteger(budget.packetCharBudgetDefault)
      ? budget.packetCharBudgetDefault
      : DEFAULT_PACKET_CHAR_BUDGET;
  const effective = Math.max(0, Math.min(configured, doorChars));
  budget.packetCharBudgetDefault = DEFAULT_PACKET_CHAR_BUDGET;
  budget.configuredPacketCharBudget = configured;
  budget.effectivePacketCharBudget = effective;

  // Highest priority first, stable within a priority so the same packet always
  // renders the same way (the cache prefix depends on it).
  const order = blocks
    .map((block, index) => ({ block, index }))
    .sort(
      (left, right) =>
        Number(right.block.priority || 0) - Number(left.block.priority || 0) ||
        left.index - right.index,
    );

  const sections = [];
  let used = 0;
  for (const { block } of order) {
    const text = typeof block.text === "string" ? block.text.trim() : "";
    const resolver = typeof block.resolver === "string" ? block.resolver.trim() : "";
    let deliveryClass = "metadata_only";
    let dropReason = resolver ? "not_selected" : "missing_resolver";
    let deliveredChars = 0;

    if (text) {
      const fragment = `--- ${block.id || "block"} (${block.provider || "federated"}) ---\n${text}`;
      const candidate = (sections.length ? "\n\n" : "") + fragment;
      if (used + candidate.length <= effective) {
        sections.push(fragment);
        used += candidate.length;
        deliveredChars = candidate.length;
        deliveryClass = "rendered";
        dropReason = "none";
      } else {
        dropReason = "packet_budget_exceeded";
        if (resolver) deliveryClass = "resolver_backed";
      }
    } else if (resolver) {
      deliveryClass = "resolver_backed";
      dropReason = "none";
    }

    const selectedTokens = Number(block.selectedTokens ?? block.estimatedTokens ?? 0) || 0;
    Object.assign(block, {
      deliveryStage: "finalized",
      deliveryClass,
      selectedTokens,
      allottedTokens: Number(block.allottedTokens ?? selectedTokens) || 0,
      renderedTokens: deliveredChars ? Math.ceil(deliveredChars / 4) : 0,
      deliveredChars,
      dropReason,
    });
  }

  const accounting = {};
  for (const block of blocks) {
    const provider = String(block.provider || "federated");
    const row =
      accounting[provider] ||
      (accounting[provider] = {
        deliveryStage: "finalized",
        selectedTokens: 0,
        renderedTokens: 0,
        deliveredChars: 0,
        reasons: [],
      });
    row.selectedTokens += Number(block.selectedTokens || 0);
    row.renderedTokens += Number(block.renderedTokens || 0);
    row.deliveredChars += Number(block.deliveredChars || 0);
    row.reasons.push(String(block.dropReason || "none"));
  }
  for (const row of Object.values(accounting)) {
    const unique = [...new Set(row.reasons)];
    delete row.reasons;
    row.dropReason = unique.length === 1 ? unique[0] : "multiple";
  }
  if (Object.keys(accounting).length) packet.providerAccounting = accounting;
  else delete packet.providerAccounting;

  return { body: sections.join("\n\n"), deliveredChars: used };
}

module.exports = {
  CLIENT_IDENTITIES,
  DEFAULT_PACKET_CHAR_BUDGET,
  DELIVERY_MODES,
  MAX_CONTEXT_CHARS,
  MAX_PACKET_BYTES,
  SELF_LOADING_RULE_CLIENTS,
  digest,
  finalize,
  loadsWorkspaceRules,
  typedClient,
};
