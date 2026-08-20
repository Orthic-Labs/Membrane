// Observer-only Blueprint seam. Membrane connects to an already-running Blueprint
// daemon through the published IPC socket; it never imports sibling source,
// opens Blueprint's private store, or spawns Blueprint.
import { randomUUID } from "node:crypto";
import { connect } from "node:net";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

export const READINESS_STATES = Object.freeze(["current", "degraded", "stale", "unwatched"]);

const MAX_RESPONSE_BYTES = 16 * 1024;

function endpoint(env = process.env) {
  return env.MEMBRANE_BLUEPRINT_ENDPOINT?.trim() || join(homedir(), ".blueprint", "blueprint.sock");
}

function unavailable(reason) {
  return Object.freeze({
    freshness: "unwatched", reason, alive: false, generationId: null, manifestDigest: null,
    source: "blueprint-ipc", fallback: "observer_unavailable",
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

/** Make one bounded request to an already-running Blueprint IPC endpoint. */
export async function requestBlueprint(method, input, { deadlineMs = 150, endpointPath = endpoint(), signal } = {}) {
  if (!Number.isInteger(deadlineMs) || deadlineMs < 10 || deadlineMs > 30_000) throw new TypeError("Blueprint observer deadline must be 10..30000ms");
  if (typeof method !== "string" || !method) throw new TypeError("Blueprint observer method is required");
  if (signal?.aborted) throw new Error("blueprint_observer_cancelled");
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
    const abort = () => finish(null, new Error("blueprint_observer_cancelled"));
    const timer = setTimeout(() => finish(null, new Error("blueprint_observer_deadline")), deadlineMs);
    signal?.addEventListener("abort", abort, { once: true });
    socket.setEncoding("utf8");
    socket.once("connect", () => socket.write(request));
    socket.on("data", (chunk) => {
      buffered += chunk;
      if (Buffer.byteLength(buffered, "utf8") > MAX_RESPONSE_BYTES) return finish(null, new Error("blueprint_observer_response_too_large"));
      const newline = buffered.indexOf("\n");
      if (newline < 0) return;
      let response;
      try { response = JSON.parse(buffered.slice(0, newline)); } catch { return finish(null, new Error("blueprint_observer_invalid_json")); }
      if (response?.protocolVersion !== 1 || response.requestId !== requestId || response.ok !== true) {
        return finish(null, new Error("blueprint_observer_schema_invalid"));
      }
      finish(response.result);
    });
    socket.once("error", () => finish(null, new Error("blueprint_observer_unavailable")));
    socket.once("close", () => { if (!settled) finish(null, new Error("blueprint_observer_unavailable")); });
  });
}

/** Read the published Blueprint IPC status seam with a bounded deadline. */
export async function observeBlueprintStatus(absoluteRoot, options = {}) {
  const status = await requestBlueprint("status", { repoRoot: resolve(absoluteRoot) }, options);
  if (!validStatus(status)) throw new Error("blueprint_observer_schema_invalid");
  return status;
}

/** Convert typed Blueprint status into Membrane's four-state readiness contract. */
export async function readBlueprintReadiness(absoluteRoot, { observer = observeBlueprintStatus, deadlineMs = 150 } = {}) {
  let status;
  try { status = await observer(absoluteRoot, { deadlineMs }); } catch { return unavailable("blueprint_status_unavailable"); }
  if (!validStatus(status)) return unavailable("blueprint_status_schema_invalid");
  const freshness = stateFrom(status);
  const manifest = status.manifest && typeof status.manifest === "object" ? status.manifest : {};
  return Object.freeze({
    freshness,
    reason: freshness === "current" ? null : `blueprint_${status.state}`,
    alive: freshness !== "unwatched",
    generationId: typeof manifest.generationId === "string" ? manifest.generationId : null,
    manifestDigest: typeof manifest.manifestDigest === "string" ? manifest.manifestDigest : null,
    source: "blueprint-ipc",
    fallback: null,
  });
}
