// MBR-501: observable event envelope contract tests.
//
// Verifies the TypeScript twin in `mcp/observable-events.mjs` against the
// normalized cross-agent event envelope contract: every client produces a
// verifiable envelope, tampering is detected, all six kinds are accepted,
// and the bus delivers one event per emit without a sibling listener being
// blocked by another listener's exception.

import test from "node:test";
import assert from "node:assert/strict";

import {
  OBSERVABLE_EVENT_KINDS,
  OBSERVABLE_EVENT_SCHEMA_VERSION,
  canonicalJsonStringify,
  emitObservableEvent,
  newObservableEvent,
  onObservableEvent,
  recomputeDigest,
  verifyObservableEvent,
  __resetObservableEventBusForTests,
} from "../../mcp/observable-events.mjs";

const INSTALLATION_ID = "11111111-1111-4111-8111-111111111111";

function sampleEnvelope(overrides = {}) {
  return newObservableEvent({
    kind: OBSERVABLE_EVENT_KINDS.context_delivered,
    clientId: "claude",
    installationId: INSTALLATION_ID,
    occurredAtUnixMs: 1_700_000_000_000,
    payload: { reason: "unit-test", token_count: 42 },
    scopeGrant: "sgv1-3f6b1c2a-9d4e-4a1b-8c7d-2e5f6a7b8c9d",
    mechanism: "membrane_context",
    ...overrides,
  });
}

test("newObservableEvent produces a verifiable envelope", () => {
  const envelope = sampleEnvelope();
  assert.equal(envelope.schemaVersion, OBSERVABLE_EVENT_SCHEMA_VERSION);
  assert.equal(envelope.kind, "context_delivered");
  assert.match(envelope.eventId, /^[0-9a-f-]{36}$/);
  assert.ok(verifyObservableEvent(envelope), "fresh envelope must verify");
});

test("verifyObservableEvent returns false on a tampered payload", () => {
  const envelope = sampleEnvelope();
  // Tamper the payload; the recomputed digest must no longer match.
  const tampered = { ...envelope, payload: { reason: "tampered", token_count: 0 } };
  assert.equal(verifyObservableEvent(tampered), false);
});

test("verifyObservableEvent returns false when the digest is wrong", () => {
  const envelope = sampleEnvelope();
  const bad = { ...envelope, digest: "sha256:" + "0".repeat(64) };
  assert.equal(verifyObservableEvent(bad), false);
});

test("all six kinds are accepted", () => {
  const kinds = Object.values(OBSERVABLE_EVENT_KINDS);
  assert.equal(kinds.length, 6);
  for (const kind of kinds) {
    const envelope = newObservableEvent({
      kind,
      clientId: "codex",
      installationId: INSTALLATION_ID,
      occurredAtUnixMs: 1_700_000_000_001,
      payload: { kind },
    });
    assert.equal(envelope.kind, kind);
    assert.ok(verifyObservableEvent(envelope), `kind ${kind} must verify`);
  }
});

test("bus listener is invoked exactly once per emit", () => {
  __resetObservableEventBusForTests();
  const calls = [];
  const unsubscribe = onObservableEvent((envelope) => calls.push(envelope.eventId));
  try {
    const envelope = sampleEnvelope();
    emitObservableEvent(envelope);
    emitObservableEvent(envelope);
    assert.equal(calls.length, 2);
    assert.equal(calls[0], envelope.eventId);
    assert.equal(calls[1], envelope.eventId);
  } finally {
    unsubscribe();
    __resetObservableEventBusForTests();
  }
});

test("bus listener that throws does not block sibling listeners", () => {
  __resetObservableEventBusForTests();
  const seen = [];
  const throwing = onObservableEvent(() => {
    throw new Error("intentional");
  });
  const healthy = onObservableEvent((envelope) => seen.push(envelope.eventId));
  try {
    const envelope = sampleEnvelope();
    emitObservableEvent(envelope);
    assert.deepEqual(seen, [envelope.eventId]);
  } finally {
    throwing();
    healthy();
    __resetObservableEventBusForTests();
  }
});

test("digest is stable across two envelopes with equal ids", () => {
  // Two envelopes with identical inputs (after equalizing the fresh
  // eventId and the occurredAtUnixMs) MUST produce the same digest.
  const baseInputs = {
    kind: OBSERVABLE_EVENT_KINDS.adapter_heartbeat,
    clientId: "cursor",
    installationId: INSTALLATION_ID,
    occurredAtUnixMs: 1_700_000_000_002,
    payload: { status: "active" },
  };
  const a = newObservableEvent(baseInputs);
  const b = newObservableEvent(baseInputs);
  // Equalize the minted fields the test cannot control.
  a.eventId = "22222222-2222-4222-8222-222222222222";
  b.eventId = "22222222-2222-4222-8222-222222222222";
  a.digest = recomputeDigest(a);
  b.digest = recomputeDigest(b);
  assert.equal(a.digest, b.digest);
  assert.ok(verifyObservableEvent(a));
  assert.ok(verifyObservableEvent(b));
});

test("canonicalJsonStringify sorts keys and drops whitespace", () => {
  const canonical = canonicalJsonStringify({
    b: 1,
    a: { y: [true, null], x: "s" },
  });
  assert.equal(canonical, `{"a":{"x":"s","y":[true,null]},"b":1}`);
});

test("emitObservableEvent refuses an envelope with a tampered digest", () => {
  __resetObservableEventBusForTests();
  try {
    const envelope = sampleEnvelope();
    const tampered = { ...envelope, digest: "sha256:" + "f".repeat(64) };
    assert.throws(
      () => emitObservableEvent(tampered),
      /envelope_digest_mismatch/,
    );
  } finally {
    __resetObservableEventBusForTests();
  }
});

test("newObservableEvent rejects an invalid kind", () => {
  assert.throws(
    () =>
      newObservableEvent({
        kind: "private_kind",
        clientId: "claude",
        installationId: INSTALLATION_ID,
        occurredAtUnixMs: 1_700_000_000_003,
        payload: {},
      }),
    /observable_event_kind_invalid/,
  );
});