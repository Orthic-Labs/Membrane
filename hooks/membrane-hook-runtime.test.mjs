import assert from "node:assert/strict";
import test from "node:test";
import { dispatchMembraneHookEvent, normalizeHookEvent, typedStatus } from "./membrane-hook-runtime.mjs";

function operations(log) {
  return Object.freeze({
    status: () => { log.push("status"); return typedStatus("available", "crypt_healthy"); },
    recall: () => { log.push("recall"); return typedStatus("available", "memory_recalled"); },
    preCompact: () => { log.push("preCompact"); return typedStatus("available", "checkpoint_saved"); },
    postCompact: () => { log.push("postCompact"); return typedStatus("available", "checkpoint_captured"); },
    ingest: () => { log.push("ingest"); return typedStatus("available", "memory_ingested"); },
  });
}

test("normalizes host spellings through neutral HookHost", () => {
  const event = normalizeHookEvent({ hook_event_name: "UserPromptSubmit", thread_id: "thread-1" });
  assert.equal(event.event, "UserPromptSubmit");
  assert.equal(event.sessionId, "thread-1");
});

test("dispatches Crypt status before memory behavior in declared order", async () => {
  const log = [];
  const result = await dispatchMembraneHookEvent({ event: "UserPromptSubmit" }, operations(log));
  assert.deepEqual(log, ["recall"]);
  assert.equal(result.schemaVersion, 1);
  assert.deepEqual(result.results.map(({ id }) => id), [
    "membrane.crypt-status", "membrane.memory-rearm", "membrane.memory-recall",
    "membrane.memory-pre-compact", "membrane.memory-post-compact", "membrane.memory-bump",
    "membrane.memory-conflict", "membrane.tool-observer", "membrane.memory-ingest", "membrane.memory-nag",
  ]);
  assert.equal(result.results[0].output.reason, "event_not_applicable");
  assert.equal(result.results[2].output.reason, "memory_recalled");
});

test("SessionStart performs health/status only and never invokes memory operations", async () => {
  const log = [];
  const result = await dispatchMembraneHookEvent({ event: "SessionStart" }, operations(log));
  assert.deepEqual(log, ["status"]);
  assert.equal(result.results[0].output.reason, "crypt_healthy");
  assert.equal(result.results[1].output.reason, "event_not_applicable");
});

test("HookHost returns a typed deadline failure", async () => {
  const stalled = { ...operations([]), status: () => new Promise(() => {}) };
  const result = await dispatchMembraneHookEvent({ event: "SessionStart" }, stalled, { timeoutMs: 5 });
  assert.equal(result.status, "error");
  assert.match(result.results[0].error, /timed out/);
});
