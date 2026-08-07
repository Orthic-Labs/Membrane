import assert from "node:assert/strict";
import test from "node:test";
import { performance } from "node:perf_hooks";
import { boundedLifecycleId, createLifecycle, withCancellationGrace } from "./lifecycle.mjs";

test("MBR-305: long context work emits ordered progress and structured logs", async () => {
  const logs = [];
  const updates = [];
  const lifecycle = createLifecycle({
    operation: "context",
    requestId: "request-1",
    progressToken: "progress-1",
    log: async (event) => logs.push(event),
    progress: async (event) => updates.push(event),
  });
  await lifecycle.begin();
  await lifecycle.checkpoint("provider_dispatch", 40);
  await lifecycle.complete();
  assert.deepEqual(updates.map((update) => update.progress), [0, 40, 100]);
  assert.ok(logs.every((event) => event.schema === "orthic.mcp.lifecycle-log.v1"));
  assert.deepEqual(logs.map((event) => event.event), ["started", "progress", "progress", "progress", "completed"]);
});

test("MBR-305: cancellation reaches a provider and settles within bounded grace", async () => {
  const controller = new AbortController();
  let providerSawCancellation = false;
  let operationStarted;
  const startedOperation = new Promise((resolve) => { operationStarted = resolve; });
  const started = performance.now();
  const pending = withCancellationGrace(async () => new Promise((resolve) => {
    operationStarted();
    controller.signal.addEventListener("abort", () => {
      providerSawCancellation = true;
      resolve();
    }, { once: true });
  }), { signal: controller.signal, graceMs: 40 });
  await startedOperation;
  controller.abort(new Error("cancelled"));
  await assert.rejects(pending, /cancelled/);
  assert.equal(providerSawCancellation, true);
  assert.ok(performance.now() - started < 250);
});

test("MBR-305: lifecycle IDs are redacted and progress is suppressed without a valid token", async () => {
  const logs = [];
  const progress = [];
  const lifecycle = createLifecycle({ operation: "context\nsecret", requestId: "x".repeat(129), progressToken: {}, log: async (event) => logs.push(event), progress: async (event) => progress.push(event) });
  await lifecycle.begin();
  assert.equal(logs[0].operation, "unknown");
  assert.equal(logs[0].requestId, "untracked");
  assert.deepEqual(progress, []);
  assert.equal(boundedLifecycleId("progress-1"), "progress-1");
});

test("MBR-305: an uncooperative provider is cut off after cancellation grace", async () => {
  const controller = new AbortController();
  const started = performance.now();
  const pending = withCancellationGrace(() => new Promise(() => {}), { signal: controller.signal, graceMs: 30 });
  controller.abort(new Error("cancelled"));
  await assert.rejects(pending, /cancelled/);
  assert.ok(performance.now() - started < 250);
});

test("MBR-305: terminal cancellation suppresses late provider progress and logs", async () => {
  const controller = new AbortController();
  const progress = [];
  const logs = [];
  const lifecycle = createLifecycle({ signal: controller.signal, progressToken: "progress-1", progress: async (event) => progress.push(event), log: async (event) => logs.push(event) });
  await lifecycle.begin();
  controller.abort(new Error("cancelled"));
  await lifecycle.cancelled();
  await lifecycle.checkpoint("late_provider", 90);
  await lifecycle.complete();
  assert.deepEqual(progress.map((event) => event.progress), [0]);
  assert.deepEqual(logs.map((event) => event.event), ["started", "progress", "cancelled"]);
});
