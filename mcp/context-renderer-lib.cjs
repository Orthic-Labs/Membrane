'use strict';
// Single source of truth for the rendering core (plan 2.2).
//
// The Membrane-owned renderer and the Forge hook MUST produce byte-identical
// output for the same packet. Both import/require this module rather than
// carrying their own copy. The ESM wrapper `context-renderer.mjs` re-exports
// everything here plus the ESM-only ContextSessionV1 class; the Forge hook
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

/** MBR-010: schema id every host-issued native-delivery receipt must carry. */
const HOST_DELIVERY_RECEIPT_SCHEMA = "orthic.host-delivery-receipt.v1";

/**
 * MBR-010: the verdict for a native-delivery claim. A block may only be
 * reported as `native` when a host-issued receipt matches client + sessionId +
 * sourceHash. Absent receipt → `missing`; present-but-mismatched or malformed
 * → `unknown`. Never report `native` on `missing`/`unknown`.
 */
const NATIVE_DELIVERY_STATUSES = Object.freeze(["native", "unknown", "missing"]);

const SOURCE_HASH_RE = /^sha256:[a-f0-9]{64}$/;

/**
 * Validate and normalize a host-delivery receipt. Returns the normalized
 * receipt, or `null` when the value is malformed (wrong schema id, missing
 * required field, bad sourceHash shape). A malformed receipt is NEVER proof of
 * native delivery.
 */
function validateHostDeliveryReceipt(receipt) {
  if (!receipt || typeof receipt !== "object") return null;
  if (receipt.schema !== HOST_DELIVERY_RECEIPT_SCHEMA) return null;
  const receiptId = String(receipt.receipt_id || "").trim();
  const sessionId = String(receipt.sessionId || "").trim();
  const sourceHash = String(receipt.sourceHash || "").trim();
  const mechanism = String(receipt.mechanism || "").trim();
  const deliveredAt = String(receipt.deliveredAt || "").trim();
  const client = typedClient(receipt.client);
  if (!receiptId || !sessionId || !mechanism || !deliveredAt) return null;
  if (!SOURCE_HASH_RE.test(sourceHash)) return null;
  if (Number.isNaN(Date.parse(deliveredAt))) return null;
  return {
    schema: HOST_DELIVERY_RECEIPT_SCHEMA,
    receipt_id: receiptId,
    client,
    sessionId,
    sourceHash,
    deliveredAt,
    mechanism,
  };
}

/**
 * MBR-010: match a candidate block against the host-issued receipts for a
 * session. Returns one of NATIVE_DELIVERY_STATUSES:
 *   - "native"  : a well-formed receipt matches client + sessionId + sourceHash
 *   - "missing" : no receipts were issued for this session at all
 *   - "unknown" : receipts exist but none validly cover this content
 */
function matchHostDeliveryReceipt(receipts, { client, sessionId, sourceHash } = {}) {
  const list = Array.isArray(receipts) ? receipts : [];
  if (list.length === 0) return "missing";
  const wantClient = typedClient(client);
  const wantSession = String(sessionId || "");
  const wantHash = String(sourceHash || "");
  for (const candidate of list) {
    const receipt = validateHostDeliveryReceipt(candidate);
    if (!receipt) continue;
    if (
      receipt.client === wantClient &&
      receipt.sessionId === wantSession &&
      receipt.sourceHash === wantHash
    ) {
      return "native";
    }
  }
  // Receipts were issued but none validly covers THIS content (or all were
  // malformed). That is "unknown", never "native".
  return "unknown";
}

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
 * This is the ONLY renderer; both the Membrane ESM surface and the Forge
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

  // MBR-011: surface a selected-without-delivery failure on the packet itself.
  packet.deliveryOutcome = evaluateDeliveryOutcome(packet);

  return { body: sections.join("\n\n"), deliveredChars: used };
}

/** MBR-011: the ONE stable alert id for a selected-without-delivery failure. */
const SELECTED_WITHOUT_DELIVERY_ALERT = "selected_without_delivery";

/**
 * MBR-011: a packet that selected content but delivered NOTHING to the agent —
 * by any mode — is a visible failure, not a quiet success.
 *
 * A block counts as delivered when it reached the agent by any proven mode:
 * rendered inline (`deliveredChars > 0`), natively loaded with a matching host
 * receipt (`deliveryMode === "native"`, MBR-010), or already delivered in a
 * prior turn (`dropReason === "already_delivered"`). When at least one block
 * was selected (`selectedTokens > 0`) but zero blocks reached the agent, the
 * outcome is `degraded`, exactly one stable alert id is emitted, and release
 * promotion is blocked.
 *
 * @param {object} packet a finalized packet (blocks carry delivery accounting)
 * @returns {{status:"ok"|"degraded", alert:string|null, selected:number, delivered:number, releasePromotionBlocked:boolean}}
 */
function evaluateDeliveryOutcome(packet) {
  const blocks = Array.isArray(packet?.blocks) ? packet.blocks : [];
  let selected = 0;
  let delivered = 0;
  for (const block of blocks) {
    const selectedTokens = Number(block.selectedTokens ?? block.estimatedTokens ?? 0) || 0;
    if (selectedTokens > 0) selected += 1;
    const reached =
      block.deliveryMode === "native" ||
      Number(block.deliveredChars ?? 0) > 0 ||
      block.dropReason === "already_delivered";
    if (reached) delivered += 1;
  }
  const failed = selected > 0 && delivered === 0;
  return {
    status: failed ? "degraded" : "ok",
    alert: failed ? SELECTED_WITHOUT_DELIVERY_ALERT : null,
    selected,
    delivered,
    releasePromotionBlocked: failed,
  };
}

/**
 * `ContextSessionV1` — the per-session delivery ledger (plan 2.3).
 *
 * Records what each session has already received so an unchanged turn can
 * deliver zero static bytes, and so a self-loading host is never sent a
 * duplicate of rules it loaded itself.
 */
class ContextSessionV1 {
  constructor({ sessionId, client, hostReceipts } = {}) {
    this.schema = "orthic.context-session.v1";
    this.sessionId = String(sessionId || "");
    this.client = typedClient(client);
    this.capabilities = {
      loadsWorkspaceRules: loadsWorkspaceRules(this.client),
      supportsResolvers: true,
    };
    this.delivered = [];
    this.selectedRepoGenerations = {};
    // MBR-010: the host-issued native-delivery receipts for this session. Only
    // these prove a block reached the agent natively.
    this.hostReceipts = Array.isArray(hostReceipts) ? hostReceipts.slice() : [];
  }

  /** True when this exact content already reached this session. */
  hasDelivered(id, sourceHash) {
    return this.delivered.some(
      (entry) => entry.id === id && entry.sourceHash === sourceHash,
    );
  }

  /** Record a host-issued native-delivery receipt for this session. */
  recordHostReceipt(receipt) {
    const normalized = validateHostDeliveryReceipt(receipt);
    if (!normalized) throw new Error("invalid host delivery receipt");
    this.hostReceipts.push(normalized);
    return normalized;
  }

  /**
   * MBR-010: the receipt-verified native-delivery verdict for a piece of
   * content in this session: "native" | "unknown" | "missing".
   */
  nativeDeliveryStatus(sourceHash) {
    return matchHostDeliveryReceipt(this.hostReceipts, {
      client: this.client,
      sessionId: this.sessionId,
      sourceHash,
    });
  }

  /**
   * Record a delivery. `native` means the host loaded it itself — zero bytes
   * ride the prompt, but the fact is still recorded so we can prove it.
   */
  record(id, deliveryMode, sourceHash, bytes = 0) {
    if (!DELIVERY_MODES.includes(deliveryMode)) {
      throw new Error(
        `deliveryMode must be one of ${DELIVERY_MODES.join("|")}, got ${deliveryMode}`,
      );
    }
    const entry = { id: String(id), deliveryMode, sourceHash: String(sourceHash || ""), bytes };
    this.delivered.push(entry);
    return entry;
  }

  noteGeneration(repoId, generationId) {
    if (repoId && generationId) this.selectedRepoGenerations[String(repoId)] = String(generationId);
  }

  toJSON() {
    return {
      schema: this.schema,
      sessionId: this.sessionId,
      client: this.client,
      capabilities: this.capabilities,
      delivered: this.delivered,
      selectedRepoGenerations: this.selectedRepoGenerations,
      hostReceipts: this.hostReceipts,
    };
  }
}

/**
 * Classify how each block should be delivered, and mark rule blocks a
 * self-loading host already has as `native` so they are never serialized.
 *
 * Returns the ledger so a caller can persist it across turns.
 */
function applyDeliveryLedger(packet, session) {
  const blocks = Array.isArray(packet?.blocks) ? packet.blocks : [];
  for (const block of blocks) {
    const id = String(block.id || "block");
    const sourceHash = String(block.sourceHash || "");
    const isRule = String(block.sourceKind || "") === "doc" && id.startsWith("rules:");

    if (isRule && session.capabilities.loadsWorkspaceRules) {
      // MBR-010: native delivery must be PROVEN by a host-issued receipt that
      // matches client + sessionId + sourceHash. Absent receipt → "missing";
      // mismatched/malformed → "unknown"; neither may be reported native. Only
      // a matched receipt lets the block skip serialization as native-loaded.
      const status = session.nativeDeliveryStatus(sourceHash);
      block.delivery = status;
      if (status === "native") {
        block.text = "";
        block.deliveryMode = "native";
        session.record(id, "native", sourceHash, 0);
        continue;
      }
      // Unproven native claim: deliver the content inline so the host actually
      // receives it, and record the receipt gap honestly instead of assuming
      // the host already had it.
      block.nativeDeliveryGap = status === "missing" ? "no_host_receipt" : "receipt_mismatch";
      const ruleBytes = typeof block.text === "string" ? Buffer.byteLength(block.text, "utf8") : 0;
      block.deliveryMode = ruleBytes > 0 ? "inline" : "reference";
      session.record(id, block.deliveryMode, sourceHash, ruleBytes);
      continue;
    }
    if (session.hasDelivered(id, sourceHash)) {
      block.text = "";
      block.deliveryMode = "reference";
      block.dropReason = "already_delivered";
      continue;
    }
    const bytes = typeof block.text === "string" ? Buffer.byteLength(block.text, "utf8") : 0;
    const mode = bytes > 0 ? "inline" : "reference";
    block.deliveryMode = mode;
    session.record(id, mode, sourceHash, bytes);
  }
  return session;
}

module.exports = {
  CLIENT_IDENTITIES,
  ContextSessionV1,
  DEFAULT_PACKET_CHAR_BUDGET,
  DELIVERY_MODES,
  HOST_DELIVERY_RECEIPT_SCHEMA,
  MAX_CONTEXT_CHARS,
  MAX_PACKET_BYTES,
  NATIVE_DELIVERY_STATUSES,
  SELECTED_WITHOUT_DELIVERY_ALERT,
  SELF_LOADING_RULE_CLIENTS,
  applyDeliveryLedger,
  digest,
  evaluateDeliveryOutcome,
  finalize,
  loadsWorkspaceRules,
  matchHostDeliveryReceipt,
  typedClient,
  validateHostDeliveryReceipt,
};
