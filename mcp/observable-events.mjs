// MBR-501: the TypeScript twin of `engine/crates/membrane-protocol/src/observable_event.rs`.
// Every Membrane client emits the SAME observable event envelope so the Hub,
// CLI, and federation gateway can consume one normalized schema. The contract
// here MUST stay byte-compatible with the Rust side: every envelope that
// `newObservableEvent` produces must `verifyObservableEvent` cleanly, and the
// canonical JSON this module emits must match the Rust `canonicalize` rules
// (sorted keys, no insignificant whitespace, null preserved).

import { createHash, randomBytes } from "node:crypto";

// Closed set of observable event kinds every Membrane client must emit.
// Keeping the set closed prevents a client from inventing a private kind
// and silently bypassing a downstream consumer.
export const OBSERVABLE_EVENT_KINDS = Object.freeze({
  context_delivered: "context_delivered",
  scope_grant_issued: "scope_grant_issued",
  mechanism_receipt: "mechanism_receipt",
  adapter_heartbeat: "adapter_heartbeat",
  provider_error: "provider_error",
  client_conflation: "client_conflation",
});

// Schema version of the observable event envelope. Bumped on incompatible
// shape changes; downstream consumers refuse unknown versions.
export const OBSERVABLE_EVENT_SCHEMA_VERSION = 1;

// Sorted insertion order so the JSON Schema `enum` and the Rust enum
// `as_str` both list kinds in the same canonical order.
const KIND_STRINGS = Object.freeze([
  "context_delivered",
  "scope_grant_issued",
  "mechanism_receipt",
  "adapter_heartbeat",
  "provider_error",
  "client_conflation",
]);

function assertKind(kind) {
  if (!KIND_STRINGS.includes(kind)) {
    throw new Error(`observable_event_kind_invalid: ${kind}`);
  }
}

function assertNonEmpty(field, value) {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error(`observable_event_${field}_required`);
  }
}

// RFC 8785-flavored canonical JSON: sorted keys, no insignificant whitespace.
// Mirrors `engine/crates/membrane-protocol/src/canonical.rs::canonicalize`.
// We intentionally do NOT depend on `safe-stable-stringify` to keep the
// module dependency-free; a fresh implementation is small enough to audit.
export function canonicalJsonStringify(value) {
  if (value === null) return "null";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      throw new Error("observable_event_payload_nan");
    }
    return JSON.stringify(value);
  }
  if (typeof value === "string") return JSON.stringify(value);
  if (Array.isArray(value)) {
    const inner = value.map((item) => canonicalJsonStringify(item));
    return `[${inner.join(",")}]`;
  }
  if (typeof value === "object") {
    const keys = Object.keys(value).sort();
    const parts = keys.map((key) => `${JSON.stringify(key)}:${canonicalJsonStringify(value[key])}`);
    return `{${parts.join(",")}}`;
  }
  throw new Error(`observable_event_payload_unsupported: ${typeof value}`);
}

function canonicalDigestOf(value) {
  const canonical = canonicalJsonStringify(value);
  return `sha256:${createHash("sha256").update(canonical).digest("hex")}`;
}

// Mint a lowercase UUIDv4 from 16 random bytes. Mirrors the Rust
// `mint_v4_uuid` helper in `observable_event.rs`.
function mintV4Uuid() {
  const bytes = randomBytes(16);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = bytes.toString("hex");
  return (
    `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20, 32)}`
  );
}

/**
 * Build a fresh observable event envelope.
 *
 * @param {object} input
 * @param {string} input.kind - one of OBSERVABLE_EVENT_KINDS
 * @param {string} input.clientId - stable client id from operations/clients.yaml
 * @param {string} input.installationId - UUIDv4 installation id
 * @param {number} input.occurredAtUnixMs - producing client wall clock (ms)
 * @param {object} input.payload - open per-kind payload (must be an object)
 * @param {string=} input.scopeGrant - optional scope-grant id
 * @param {string=} input.mechanism - optional mechanism name
 * @returns {object} the envelope, with `digest` already computed.
 */
export function newObservableEvent({ kind, clientId, installationId, occurredAtUnixMs, payload, scopeGrant, mechanism }) {
  assertKind(kind);
  assertNonEmpty("client_id", clientId);
  assertNonEmpty("installation_id", installationId);
  if (!Number.isInteger(occurredAtUnixMs) || occurredAtUnixMs < 0) {
    throw new Error("observable_event_occurred_at_unix_ms_invalid");
  }
  if (payload === null || typeof payload !== "object" || Array.isArray(payload)) {
    throw new Error("observable_event_payload_must_be_object");
  }
  if (scopeGrant !== undefined && scopeGrant !== null) {
    assertNonEmpty("scope_grant", scopeGrant);
  }
  if (mechanism !== undefined && mechanism !== null) {
    assertNonEmpty("mechanism", mechanism);
  }
  const envelope = {
    schemaVersion: OBSERVABLE_EVENT_SCHEMA_VERSION,
    eventId: mintV4Uuid(),
    kind,
    clientId,
    installationId,
    occurredAtUnixMs,
    payload,
    digest: "",
  };
  if (scopeGrant !== undefined && scopeGrant !== null) envelope.scopeGrant = scopeGrant;
  if (mechanism !== undefined && mechanism !== null) envelope.mechanism = mechanism;
  envelope.digest = recomputeDigest(envelope);
  return envelope;
}

// Strip the digest field, canonicalize, and hash — the same self-referential
// pattern the Rust `recompute_digest` uses. Exported so tests can construct
// envelopes with controlled digests.
export function recomputeDigest(envelope) {
  const withoutDigest = { ...envelope };
  delete withoutDigest.digest;
  return canonicalDigestOf(withoutDigest);
}

/**
 * Recompute the envelope's digest and return true iff it matches. Returns
 * false on tampered payload, missing fields, or any structural mismatch.
 *
 * @param {object} envelope
 * @returns {boolean}
 */
export function verifyObservableEvent(envelope) {
  if (!envelope || typeof envelope !== "object") return false;
  if (envelope.schemaVersion !== OBSERVABLE_EVENT_SCHEMA_VERSION) return false;
  if (typeof envelope.eventId !== "string" || envelope.eventId.length === 0) return false;
  if (!KIND_STRINGS.includes(envelope.kind)) return false;
  if (typeof envelope.clientId !== "string" || envelope.clientId.length === 0) return false;
  if (typeof envelope.installationId !== "string" || envelope.installationId.length === 0) return false;
  if (!Number.isInteger(envelope.occurredAtUnixMs) || envelope.occurredAtUnixMs < 0) return false;
  if (typeof envelope.digest !== "string" || envelope.digest.length === 0) return false;
  return recomputeDigest(envelope) === envelope.digest;
}

// Tiny bus: the bridge between the JS event producers (Claude, Codex,
// Cursor, Windsurf, the CLI, the federation gateway) and the durable
// writer on the Rust side. Listeners register via `onObservableEvent`; the
// runtime calls `emitObservableEvent` from the same place it currently
// emits bespoke JS events. Listener exceptions are swallowed so a single
// failing subscriber cannot wedge sibling subscribers.
const listeners = new Set();

export function onObservableEvent(listener) {
  if (typeof listener !== "function") {
    throw new Error("observable_event_listener_must_be_function");
  }
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function emitObservableEvent(envelope) {
  if (!verifyObservableEvent(envelope)) {
    throw new Error("envelope_digest_mismatch");
  }
  for (const listener of listeners) {
    try {
      listener(envelope);
    } catch {
      // intentional: a single failing listener cannot block siblings.
    }
  }
}

// Test-only escape hatch so unit tests can reset state between runs.
export function __resetObservableEventBusForTests() {
  listeners.clear();
}