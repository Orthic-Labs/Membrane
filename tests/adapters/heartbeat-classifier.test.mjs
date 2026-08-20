// MBR-207: Hub and CLI never conflate installed with active or delivering.
// The classifier is a pure function; the snapshot builder is deterministic.
// Both are exercised here so the JS surface agrees with the Rust supervisor
// before any heartbeat is wired in for real.

import assert from "node:assert/strict";
import test from "node:test";
import { STATUSES, adapterSnapshot, assertNoConflation, classifyStatus } from "../../mcp/adapters.mjs";

const NOW = 1_755_124_800_000; // 2025-08-07T00:00:00Z
const STALENESS_MS = 60_000;

test("classifyStatus defaults to 'installed' on empty input", () => {
  assert.equal(classifyStatus({}, { now: NOW, stalenessMs: STALENESS_MS }), STATUSES.installed);
});

test("classifyStatus honours a self-declared active heartbeat with no receipts", () => {
  const status = classifyStatus(
    { status: STATUSES.active, lastSeen: NOW, receipts: [] },
    { now: NOW, stalenessMs: STALENESS_MS },
  );
  assert.equal(status, STATUSES.active);
});

test("classifyStatus promotes to 'delivering' when any receipt.count > 0", () => {
  const status = classifyStatus(
    {
      status: STATUSES.active,
      lastSeen: NOW,
      receipts: [{ count: 3 }],
    },
    { now: NOW, stalenessMs: STALENESS_MS },
  );
  assert.equal(status, STATUSES.delivering);
});

test("classifyStatus flags a stale heartbeat even when the adapter self-declares active", () => {
  const status = classifyStatus(
    {
      status: STATUSES.active,
      lastSeen: NOW - 120_000,
      receipts: [],
    },
    { now: NOW, stalenessMs: STALENESS_MS },
  );
  assert.equal(status, STATUSES.stale);
});

test("classifyStatus coerces a self-declared installed heartbeat with no receipts to active", () => {
  const status = classifyStatus(
    { status: STATUSES.installed, lastSeen: NOW, receipts: [] },
    { now: NOW, stalenessMs: STALENESS_MS },
  );
  assert.equal(status, STATUSES.active);
});

test("adapterSnapshot produces a deterministic installed envelope for an unknown client", () => {
  const snapshot = adapterSnapshot("cursor", { now: NOW, stalenessMs: STALENESS_MS });
  assert.equal(snapshot.schema, "membrane.adapter-heartbeat.v1");
  assert.equal(snapshot.schemaVersion, 1);
  assert.equal(snapshot.clientId, "cursor");
  assert.equal(snapshot.status, STATUSES.installed);
  assert.equal(snapshot.lastSeen, NOW);
  assert.deepEqual(snapshot.mechanismReceipts, []);
});

test("assertNoConflation throws when installed and delivering coexist on one client", () => {
  const conflated = {
    schema: "membrane.adapter-heartbeat.v1",
    schemaVersion: 1,
    clientId: "broken",
    adapterVersion: "0.1.0",
    status: STATUSES.installed,
    lastSeen: NOW,
    mechanismReceipts: [{ count: 1 }],
  };
  assert.throws(
    () => assertNoConflation(conflated),
    (err) => err.code === "heartbeat-conflation" && err.clientId === "broken",
  );
});

test("assertNoConflation accepts a clean delivering snapshot", () => {
  const clean = adapterSnapshot(
    "windsurf",
    {
      now: NOW,
      stalenessMs: STALENESS_MS,
      status: STATUSES.delivering,
      lastSeen: NOW,
      receipts: [{ count: 2 }],
    },
  );
  assert.doesNotThrow(() => assertNoConflation(clean));
});
