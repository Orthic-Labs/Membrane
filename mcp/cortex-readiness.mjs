// Observer-only Cortex seam. Membrane connects to an already-running Cortex
// daemon; it never imports sibling source, reads graph.db, or spawns Cortex.
import { randomUUID } from "node:crypto";
import { connect } from "node:net";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

export const READINESS_STATES = Object.freeze(["current", "degraded", "stale", "unwatched"]);

const MAX_RESPONSE_BYTES = 16 * 1024;

function endpoint(env = process.env) {
  return env.MEMBRANE_CORTEX_ENDPOINT?.trim() || join(homedir(), ".cortex", "cortex.sock");
}

function unavailable(reason) {
  return Object.freeze({
    freshness: "unwatched", reason, alive: false, generationId: null, manifestDigest: null,
    source: "cortex-ipc", fallback: "observer_unavailable",
  });
}

function stateFrom(status) {
  if (status.state === "fresh") return "current";
  if (status.state === "stale") return "stale";
  if (["indeterminate", "incomplete", "corrupt"].includes(status.state)) return "degraded";
  return "unwatched";
}

function validStatus(status) {
  return Boolean(status && typeof status === "object" && !Array.isArray(status)
    && status.schemaVersion === 1 && typeof status.state === "string"
    && status.repository && typeof status.repository === "object");
}

/** Make one bounded request to an already-running Cortex IPC endpoint. */
export async function requestCortex(method, input, { deadlineMs = 150, endpointPath = endpoint(), signal } = {}) {
  if (!Number.isInteger(deadlineMs) || deadlineMs < 10 || deadlineMs > 30_000) throw new TypeError("Cortex observer deadline must be 10..30000ms");
  if (typeof method !== "string" || !method) throw new TypeError("Cortex observer method is required");
  if (signal?.aborted) throw new Error("cortex_observer_cancelled");
  const requestId = randomUUID();
  const request = `${JSON.stringify({
    protocolVersion: 1, requestId, repoId: null, generation: null, method,
    deadlineMs, input,
  })}\n`;
  return new Promise((resolveResult, rejectResult) => {
    const socket = connect(endpointPath);
    let buffered = "";
    let settled = false;
    const finish = (value, error = null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      socket.destroy();
      signal?.removeEventListener("abort", abort);
      if (error) rejectResult(error); else resolveResult(value);
    };
    const abort = () => finish(null, new Error("cortex_observer_cancelled"));
    const timer = setTimeout(() => finish(null, new Error("cortex_observer_deadline")), deadlineMs);
    signal?.addEventListener("abort", abort, { once: true });
    socket.setEncoding("utf8");
    socket.once("connect", () => socket.write(request));
    socket.on("data", (chunk) => {
      buffered += chunk;
      if (Buffer.byteLength(buffered, "utf8") > MAX_RESPONSE_BYTES) return finish(null, new Error("cortex_observer_response_too_large"));
      const newline = buffered.indexOf("\n");
      if (newline < 0) return;
      let response;
      try { response = JSON.parse(buffered.slice(0, newline)); } catch { return finish(null, new Error("cortex_observer_invalid_json")); }
      if (response?.protocolVersion !== 1 || response.requestId !== requestId || response.ok !== true) {
        return finish(null, new Error("cortex_observer_schema_invalid"));
      }
      finish(response.result);
    });
    socket.once("error", () => finish(null, new Error("cortex_observer_unavailable")));
    socket.once("close", () => { if (!settled) finish(null, new Error("cortex_observer_unavailable")); });
  });
}

/** Read the published Cortex IPC status seam with a bounded deadline. */
export async function observeCortexStatus(absoluteRoot, options = {}) {
  const status = await requestCortex("status", { repoRoot: resolve(absoluteRoot) }, options);
  if (!validStatus(status)) throw new Error("cortex_observer_schema_invalid");
  return status;
}

/** Convert typed Cortex status into Membrane's four-state readiness contract. */
export async function readCortexReadiness(absoluteRoot, { observer = observeCortexStatus, deadlineMs = 150 } = {}) {
  let status;
  try { status = await observer(absoluteRoot, { deadlineMs }); } catch { return unavailable("cortex_status_unavailable"); }
  if (!validStatus(status)) return unavailable("cortex_status_schema_invalid");
  const freshness = stateFrom(status);
  const manifest = status.manifest && typeof status.manifest === "object" ? status.manifest : {};
  return Object.freeze({
    freshness,
    reason: freshness === "current" ? null : `cortex_${status.state}`,
    alive: freshness !== "unwatched",
    generationId: typeof manifest.generationId === "string" ? manifest.generationId : null,
    manifestDigest: typeof manifest.manifestDigest === "string" ? manifest.manifestDigest : null,
    source: "cortex-ipc",
    fallback: null,
  });
}
