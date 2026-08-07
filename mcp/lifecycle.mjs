// MBR-305: shared MCP lifecycle semantics for work that can outlive one turn.

export const DEFAULT_CANCELLATION_GRACE_MS = 250;
export const MAX_LIFECYCLE_LOG_EVENTS = 8;

export function boundedLifecycleId(value, fallback = "untracked") {
  if (typeof value === "number" && Number.isSafeInteger(value) && value >= 0) return value;
  if (typeof value !== "string" || value.length === 0 || value.length > 128) return fallback;
  return /^[A-Za-z0-9._:-]+$/.test(value) ? value : fallback;
}

function cancellationError(signal) {
  return signal?.reason instanceof Error ? signal.reason : new Error("cancelled");
}

export function createLifecycle({ operation, requestId, signal, progressToken, log = async () => {}, progress = async () => {} }) {
  let lastProgress = -1;
  let logCount = 0;
  let terminal = false;
  const safeOperation = boundedLifecycleId(operation, "unknown");
  const safeRequestId = boundedLifecycleId(requestId);
  const safeProgressToken = boundedLifecycleId(progressToken, null);
  const event = (eventType, fields = {}) => ({
    schema: "orthic.mcp.lifecycle-log.v1",
    event: eventType,
    operation: safeOperation,
    requestId: safeRequestId,
    ...fields,
  });
  const emitLog = async (eventType, fields) => {
    if (logCount >= MAX_LIFECYCLE_LOG_EVENTS) return;
    logCount += 1;
    await log(event(eventType, fields));
  };
  return {
    signal,
    async begin() {
      if (terminal || signal?.aborted) return;
      await emitLog("started");
      await this.checkpoint("accepted", 0);
    },
    async checkpoint(phase, completed, total = 100) {
      if (terminal || signal?.aborted) return;
      const bounded = Math.max(lastProgress, Math.min(total, Math.max(0, completed)));
      lastProgress = bounded;
      if (safeProgressToken !== null) await progress({ progressToken: safeProgressToken, progress: bounded, total });
      await emitLog("progress", { phase, progress: bounded, total });
    },
    async complete() {
      if (terminal || signal?.aborted) return;
      await this.checkpoint("complete", 100);
      await emitLog("completed");
      terminal = true;
    },
    async cancelled() {
      if (terminal) return;
      terminal = true;
      await emitLog("cancelled", { reason: signal?.reason?.message === "deadline_exceeded" ? "deadline_exceeded" : "cancelled" });
    },
  };
}

// Providers receive the request AbortSignal directly. If one ignores it, the MCP
// request still settles within the grace limit while its owner reaps that worker.
export async function withCancellationGrace(operation, { signal, graceMs = DEFAULT_CANCELLATION_GRACE_MS } = {}) {
  const pending = Promise.resolve().then(operation);
  if (!signal) return pending;
  return new Promise((resolve, reject) => {
    let settled = false;
    let timer;
    const finish = (complete, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal.removeEventListener("abort", onAbort);
      complete(value);
    };
    const onAbort = () => {
      if (timer === undefined) timer = setTimeout(() => finish(reject, cancellationError(signal)), graceMs);
    };
    signal.addEventListener("abort", onAbort, { once: true });
    if (signal.aborted) onAbort();
    pending.then(
      (value) => finish(signal.aborted ? reject : resolve, signal.aborted ? cancellationError(signal) : value),
      (error) => finish(reject, error),
    );
  });
}
