// MBR-305: shared MCP lifecycle semantics for work that can outlive one turn.

export const DEFAULT_CANCELLATION_GRACE_MS = 250;
export const MAX_LIFECYCLE_LOG_EVENTS = 8;
export const LIFECYCLE_VERSION = 1;
export const LIFECYCLE_SCHEMA = "membrane.lifecycle.v1";
export const LIFECYCLE_STATES = Object.freeze(["hello", "starting", "ready", "degraded", "draining", "stopped", "incompatible", "failed"]);
export const LIFECYCLE_COMMANDS = Object.freeze(["drain", "stop", "update_handoff", "ownership_loss"]);

const TERMINAL_STATES = new Set(["stopped", "incompatible", "failed"]);
const DRAINING_STATES = new Set(["draining", ...TERMINAL_STATES]);
const REGISTER_STATES = new Set(["starting", "ready", "degraded", "incompatible", "failed"]);

function nonEmpty(value, name) {
  if (typeof value !== "string" || value.length === 0 || value.length > 256) throw new TypeError(`${name} must be a non-empty bounded string`);
  return value;
}

function lifecycleIdentity(identity = {}) {
  const required = ["installationId", "productId", "instanceId", "artifactDigest", "dataRoot"];
  const out = Object.fromEntries(required.map((key) => [key, nonEmpty(identity[key], key)]));
  out.lifecycleVersion = identity.lifecycleVersion ?? LIFECYCLE_VERSION;
  if (out.lifecycleVersion !== LIFECYCLE_VERSION) throw new Error("lifecycle_version_incompatible");
  return Object.freeze(out);
}

/**
 * Child-side membrane.lifecycle.v1 channel. The Hub owns lease issuance; this object only
 * authenticates inherited transport, binds hello identity/fence, rejects stale requests,
 * and performs bounded drain/stop on parent or ownership loss.
 */
export function createLifecycleChannel({ channel, identity, fence, drain = async () => {}, stop = async () => {}, drainTimeoutMs = 1000 } = {}) {
  if (!channel || channel.inherited !== true || channel.authenticated !== true) throw new Error("lifecycle_channel_not_authenticated");
  if (!Number.isSafeInteger(fence) || fence < 1) throw new TypeError("lifecycle fence must be a positive integer");
  const boundIdentity = lifecycleIdentity(identity);
  let state = "hello";
  let activeFence = fence;
  let parentLost = false;
  let stopping = false;
  const listeners = new Set();
  const snapshot = () => ({ lifecycleVersion: LIFECYCLE_VERSION, state, fence: activeFence, ...boundIdentity });
  const publish = async (next, extra = {}) => {
    if (state === next && !extra.force) return snapshot();
    if (TERMINAL_STATES.has(state)) return snapshot();
    state = next;
    const register = extra.kind === "register" || REGISTER_STATES.has(next);
    const message = register
      ? { kind: "register", state: next, fence: activeFence, ...(extra.endpoint ? { endpoint: extra.endpoint } : {}), ...(extra.capability ? { capability: extra.capability } : {}) }
      : { ...snapshot(), ...extra };
    // Parent-loss closes inherited transport before child can publish its terminal state;
    // keep local drain semantics authoritative when that send fails.
    if (register) {
      try { await channel.send?.(message); } catch { /* transport already lost */ }
    }
    for (const listener of listeners) await listener(message);
    return message;
  };
  const boundedDrain = async (reason) => {
    await publish("draining", { reason });
    let completed = false;
    await Promise.race([
      Promise.resolve().then(drain).then(() => { completed = true; }),
      new Promise((resolve) => setTimeout(resolve, drainTimeoutMs)),
    ]);
    return completed;
  };
  const boundedDrainStop = async (reason) => {
    if (stopping || state === "stopped") return true;
    stopping = true;
    const completed = await boundedDrain(reason);
    if (!completed) {
      await publish("failed", { reason: "drain_timeout" });
      return false;
    }
    await Promise.resolve().then(stop);
    await publish("stopped", { reason });
    return true;
  };
  const acknowledge = async (command) => {
    const frame = { kind: "ack", command, fence: activeFence };
    try { await channel.send?.(frame); } catch { /* transport already lost */ }
    return frame;
  };
  const onParentLoss = () => { parentLost = true; return boundedDrainStop("parent_loss"); };
  channel.on?.("close", onParentLoss);
  channel.on?.("error", onParentLoss);
  return Object.freeze({
    identity: boundIdentity,
    get state() { return state; },
    get fence() { return activeFence; },
    get parentLost() { return parentLost; },
    snapshot,
    onState(listener) { listeners.add(listener); return () => listeners.delete(listener); },
    async hello() { return publish("starting", { kind: "register" }); },
    async ready(endpoint) {
      if (!endpoint || typeof endpoint !== "object") throw new Error("lifecycle_endpoint_registration_invalid");
      const host = nonEmpty(endpoint.host, "endpoint.host");
      if (!new Set(["127.0.0.1", "::1", "localhost"]).has(host)) throw new Error("lifecycle_endpoint_not_loopback");
      if (!Number.isSafeInteger(endpoint.port) || endpoint.port < 1 || endpoint.port > 65535) throw new Error("lifecycle_endpoint_port_invalid");
      const capability = nonEmpty(endpoint.capability, "endpoint.capability");
      if (DRAINING_STATES.has(state)) throw new Error("lifecycle_not_accepting");
      return publish("ready", { kind: "register", endpoint: { host, port: endpoint.port }, capability });
    },
    async degraded(reason) { return publish("degraded", { reason: nonEmpty(reason, "reason") }); },
    async command(command, commandFence = activeFence) {
      if (!LIFECYCLE_COMMANDS.includes(command)) throw new Error("lifecycle_command_invalid");
      if (!Number.isSafeInteger(commandFence) || commandFence !== activeFence) throw new Error("lifecycle_stale_fence");
      if (command === "drain") {
        if (!await boundedDrain("hub_drain")) return { kind: "failed", command, fence: activeFence, reason: "drain_timeout" };
        return acknowledge(command);
      }
      if (command === "stop") {
        if (!await boundedDrainStop("hub_stop")) return { kind: "failed", command, fence: activeFence, reason: "drain_timeout" };
        return acknowledge(command);
      }
      if (command === "ownership_loss") {
        if (!await boundedDrainStop("ownership_loss")) return { kind: "failed", command, fence: activeFence, reason: "drain_timeout" };
        return acknowledge(command);
      }
      if (!await boundedDrain("update_handoff")) return { kind: "failed", command, fence: activeFence, reason: "drain_timeout" };
      return acknowledge(command);
    },
    assertFence(requestFence) {
      if (!Number.isSafeInteger(requestFence) || requestFence !== activeFence) throw new Error("lifecycle_stale_fence");
      if (DRAINING_STATES.has(state)) throw new Error("lifecycle_not_accepting");
      return true;
    },
    parentLoss: onParentLoss,
    stop: () => boundedDrainStop("child_stop"),
  });
}

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
    schema: "membrane.mcp.lifecycle-log.v1",
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
