// mcp/deadline.mjs — MBR-005 / SN-NODE-04 + R04: one absolute monotonic
// deadline and cancellation threaded through every layer. Inner layers must
// never create a fresh full timeout; they borrow the ingress deadline.

import { performance } from "node:perf_hooks";

// Monotonic absolute deadline base (performance.now), safe against clock jumps.
export function createDeadline(timeoutMs) {
  return performance.now() + Math.max(1, Number.isFinite(timeoutMs) ? timeoutMs : 2500);
}
export function remainingMs(deadline) {
  return Math.max(0, Math.floor(deadline - performance.now()));
}

// Wall/replaceable-clock deadline for callers that pass an explicit absolute ms.
export function makeDeadline({ deadlineMs, timeoutMs = 2500, now = Date.now() } = {}) {
  const absolute = Number.isFinite(deadlineMs) ? Number(deadlineMs) : now + timeoutMs;
  if (absolute <= now) throw new Error("deadline_expired");
  return absolute;
}

export function deadlineSignal(deadline, parentSignal) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(new Error("deadline_exceeded")), remainingMs(deadline));
  const relay = () => controller.abort(parentSignal?.reason || new Error("cancelled"));
  parentSignal?.addEventListener("abort", relay, { once: true });
  return {
    signal: controller.signal,
    close() { clearTimeout(timer); parentSignal?.removeEventListener("abort", relay); },
  };
}

export async function mapConcurrent(items, limit, fn, signal) {
  const out = new Array(items.length);
  let cursor = 0;
  async function worker() {
    while (true) {
      if (signal?.aborted) throw signal.reason;
      const index = cursor++;
      if (index >= items.length) return;
      out[index] = await fn(items[index], index, signal);
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return out;
}

export async function withDeadline(deadlineMs, operation, parentSignal) {
  const controller = new AbortController();
  const onAbort = () => controller.abort(parentSignal?.reason || new Error("cancelled"));
  parentSignal?.addEventListener("abort", onAbort, { once: true });
  const delay = Math.max(0, Math.floor(deadlineMs - Date.now()));
  const timer = setTimeout(() => controller.abort(new Error("deadline_exceeded")), delay);
  try {
    return await operation({ signal: controller.signal, deadlineMs });
  } finally {
    clearTimeout(timer);
    parentSignal?.removeEventListener("abort", onAbort);
  }
}

// Terminal reason codes: do not collapse these into federation_error.
export function terminalReason(signal) {
  if (!signal?.aborted) return null;
  return signal.reason?.message === "deadline_exceeded" ? "deadline_exceeded" : "cancelled";
}
