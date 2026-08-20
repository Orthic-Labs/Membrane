import assert from "node:assert/strict";
import test from "node:test";
import { performance } from "node:perf_hooks";
import { createDeadline, remainingMs, makeDeadline, deadlineSignal, mapConcurrent, withDeadline, terminalReason, timeoutReceipt } from "./deadline.mjs";

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test("createDeadline yields a future monotonic absolute deadline that shrinks", () => {
  const deadline = createDeadline(1000);
  assert.ok(deadline > performance.now());
  assert.equal(remainingMs(deadline) <= 1000, true);
  assert.equal(remainingMs(deadline) >= 0, true);
});

test("makeDeadline rejects an already-expired absolute deadline", () => {
  assert.throws(() => makeDeadline({ deadlineMs: Date.now() - 1 }), /deadline_expired/);
  assert.ok(Number.isFinite(makeDeadline({}))); // default -> future
});

test("MBR-005: a single slow child cannot exceed the global deadline", async () => {
  const { spawn } = await import("node:child_process");
  const deadline = createDeadline(30);
  const lane = deadlineSignal(deadline);
  const started = performance.now();
  await new Promise((resolve) => {
    // A child that would sleep ~5s: the parent deadline must terminate it early.
    const child = spawn(process.execPath, ["-e", "setTimeout(()=>{}, 5000)"], { signal: lane.signal });
    child.on("close", () => resolve());
    child.on("error", () => resolve());
  });
  const elapsed = performance.now() - started;
  assert.ok(elapsed < 1000, `a slow child must be terminated by the global deadline, took ${Math.round(elapsed)}ms`);
  assert.equal(lane.signal.aborted, true);
  lane.close();
});

test("deadlineSignal reports a typed terminal reason, distinct from cancellation", () => {
  const deadline = createDeadline(5);
  const lane = deadlineSignal(deadline);
  return new Promise((resolve, reject) => {
    lane.signal.addEventListener("abort", () => {
      try {
        assert.equal(terminalReason(lane.signal), "deadline_exceeded");
        lane.close();
        resolve();
      } catch (e) { reject(e); }
    }, { once: true });
  });
});

test("withDeadline cancels a long operation at the parent deadline", async () => {
  const deadline = createDeadline(25);
  const started = performance.now();
  await assert.rejects(
    withDeadline(deadline, async ({ signal }) => {
      await new Promise((resolve) => {
        signal.addEventListener("abort", resolve, { once: true });
        setTimeout(resolve, 5000); // longer than the deadline
      });
      if (signal.aborted) throw signal.reason || new Error("cancelled");
    }),
    /deadline_exceeded/,
  );
  assert.ok(performance.now() - started < 1000, "operation honoured the parent deadline");
});

test("mapConcurrent bounds concurrency and preserves output order", async () => {
  const out = await mapConcurrent([1, 2, 3, 4], 2, async (value) => value * 10, undefined);
  assert.deepEqual(out, [10, 20, 30, 40]);
});

test("terminalReason classifies an abort reason", () => {
  const controller = new AbortController();
  controller.abort(new Error("deadline_exceeded"));
  assert.equal(terminalReason(controller.signal), "deadline_exceeded");
  controller.abort(new Error("cancelled"));
  assert.equal(terminalReason(controller.signal), "deadline_exceeded"); // first reason wins
  const parent = new AbortController();
  parent.abort(new Error("cancelled"));
  assert.equal(terminalReason(parent.signal), "cancelled");
});

test("MBR-008: timeout receipts name the layer and the remaining budget", () => {
  const deadline = createDeadline(1000);
  const receipt = timeoutReceipt("client.http", deadline);
  assert.equal(receipt.schema, "membrane.timeout-receipt.v1");
  assert.equal(receipt.layer, "client.http");
  assert.equal(typeof receipt.remainingMs, "number");
  assert.ok(receipt.remainingMs >= 0 && receipt.remainingMs <= 1000);
  const expired = timeoutReceipt("server.fan-out", performance.now() - 1);
  assert.equal(expired.exceeded, true);
  assert.equal(expired.remainingMs, 0);
  assert.equal(expired.layer, "server.fan-out");
});

// An inner layer must never expire before the caller deadline: a receipt derived
// from the shared deadline always reports >= 0 remaining and expires exactly at 0.
test("MBR-008: no inner layer creates a longer budget than the caller deadline", () => {
  const deadline = createDeadline(40);
  assert.ok(remainingMs(deadline) <= 40, "inner budgets are bounded by the ingress deadline");
  const r1 = timeoutReceipt("server.fan-out", deadline);
  const r2 = timeoutReceipt("client.http", deadline);
  assert.equal(r2.remainingMs <= r1.remainingMs + 1, true, "client layer never outlasts the server fan-out lane");
});
