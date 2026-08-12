// D30: daemon IPC server — the daemon owns writable handles and serializes
// writes per repository. Read requests use generation-pinned read-only
// handles and bounded queues. Newline-delimited JSON over Unix socket
// (macOS/Linux) or named pipe (Windows).

import { connect, createServer } from "node:net";
import { randomUUID } from "node:crypto";
import { existsSync, lstatSync, mkdirSync, readFileSync, renameSync, rmSync, rmdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { createCortexApplicationService } from "../lib/application/service.mjs";
import { RootRegistry } from "../lib/application/root-registry.mjs";
import { createBuildSingleflight } from "./build-singleflight.mjs";
import { PROTOCOL_VERSION, decodeLine, encodeResponse, METHODS, validateDeadlineMs, validateProtocolVersion } from "./protocol.mjs";
import { daemonEndpoint } from "./paths.mjs";

export const MAX_FRAME_BYTES = 16 * 1024;

function endpointError(code, message) {
  return Object.assign(new Error(message), { code });
}

async function removeStaleUnixEndpoint(socketPath) {
  if (!existsSync(socketPath)) return;
  if (!lstatSync(socketPath).isSocket()) throw endpointError("endpoint_invalid", `daemon endpoint is not a socket: ${socketPath}`);
  const state = await new Promise((resolve) => {
    const probe = connect(socketPath);
    const timer = setTimeout(() => { probe.destroy(); resolve("unknown"); }, 250);
    probe.once("connect", () => { clearTimeout(timer); probe.end(); resolve("live"); });
    probe.once("error", (error) => { clearTimeout(timer); resolve(error.code === "ECONNREFUSED" ? "stale" : "unknown"); });
  });
  if (state === "live") throw endpointError("endpoint_in_use", `daemon endpoint is already live: ${socketPath}`);
  if (state !== "stale") throw endpointError("endpoint_unavailable", `daemon endpoint could not be verified stale: ${socketPath}`);
  rmSync(socketPath);
}

function unixLockPath(socketPath) {
  return `${socketPath}.lock`;
}

function ownerIsLive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return null;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error.code === "ESRCH") return false;
    return null;
  }
}

function cleanupNewUnixLock(path, token) {
  try {
    const owner = JSON.parse(readFileSync(join(path, "owner.json"), "utf8"));
    if (owner?.token === token && owner.pid === process.pid) rmSync(path, { recursive: true, force: true });
  } catch {
    try { rmdirSync(path); } catch {}
  }
}

export function acquireUnixLock(socketPath, { writeOwner = writeFileSync } = {}) {
  const path = unixLockPath(socketPath);
  const token = randomUUID();
  mkdirSync(dirname(path), { recursive: true });
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      mkdirSync(path);
      try {
        writeOwner(join(path, "owner.json"), JSON.stringify({ pid: process.pid, token }), { flag: "wx" });
      } catch {
        cleanupNewUnixLock(path, token);
        throw endpointError("endpoint_locked", `daemon lock owner could not be written: ${socketPath}`);
      }
      return { path, token };
    } catch (error) {
      if (error.code !== "EEXIST") throw endpointError("endpoint_locked", `daemon lock could not be acquired: ${socketPath}`);
      let owner;
      try {
        owner = JSON.parse(readFileSync(join(path, "owner.json"), "utf8"));
      } catch {
        throw endpointError("endpoint_locked", `daemon lock ownership is unreadable: ${socketPath}`);
      }
      if (typeof owner?.token !== "string" || ownerIsLive(owner.pid) !== false) {
        throw endpointError("endpoint_locked", `daemon lock is owned by a live or unverifiable process: ${socketPath}`);
      }
      const quarantine = `${path}.stale-${randomUUID()}`;
      try {
        renameSync(path, quarantine);
        rmSync(quarantine, { recursive: true, force: true });
      } catch {
        throw endpointError("endpoint_locked", `daemon stale lock could not be quarantined: ${socketPath}`);
      }
    }
  }
  throw endpointError("endpoint_locked", `daemon lock could not be acquired: ${socketPath}`);
}

function releaseUnixLock(lock) {
  if (!lock) return;
  try {
    const owner = JSON.parse(readFileSync(join(lock.path, "owner.json"), "utf8"));
    if (owner?.token === lock.token && owner.pid === process.pid) rmSync(lock.path, { recursive: true, force: true });
  } catch {
    // An unreadable or replaced lock is never deleted by a non-owner.
  }
}

export function createDaemonServer({ service = null, endpoint = null, registryEntries = [], rootRegistry = null, buildSingleflight = null } = {}) {
  const registry = rootRegistry ?? (service || registryEntries.length === 0 ? null : new RootRegistry(registryEntries));
  const queueRegistry = rootRegistry ?? (registryEntries.length > 0 ? new RootRegistry(registryEntries) : null);
  const appService = service ?? createCortexApplicationService({ rootRegistry: registry, allowEmbeddedRoot: false });
  const builds = buildSingleflight ?? createBuildSingleflight();
  const socketPath = endpoint ?? daemonEndpoint();
  const isWindows = process.platform === "win32";
  const pending = new Map(); // requestId -> work retained until its promise exits
  const activeSockets = new Set();
  const socketPending = new Map();
  const rootQueues = new Map();
  let unixLock = null;
  let closePromise = null;

  async function queueFreshness(root, work) {
    if (!root) return work();
    const previous = rootQueues.get(root) ?? Promise.resolve();
    let release;
    const own = new Promise((resolve) => { release = resolve; });
    const tail = previous.catch(() => {}).then(() => own);
    rootQueues.set(root, tail);
    await previous.catch(() => {});
    try {
      return await work();
    } finally {
      release();
      if (rootQueues.get(root) === tail) rootQueues.delete(root);
    }
  }

  function canonicalRoot(message) {
    if (typeof appService.resolveCanonicalRoot === "function") {
      return appService.resolveCanonicalRoot({ ...(message.input ?? {}), repoId: message.repoId ?? message.input?.repoId });
    }
    if (!queueRegistry) return null;
    return queueRegistry.resolve({ ...(message.input ?? {}), repoId: message.repoId ?? message.input?.repoId });
  }

  function settle(entry, response) {
    if (!entry || entry.settled) return false;
    entry.settled = true;
    clearTimeout(entry.timer);
    if (!entry.socket.destroyed) entry.socket.write(encodeResponse(response));
    return true;
  }

  function complete(entry) {
    if (pending.get(entry.id) === entry) pending.delete(entry.id);
    const entries = socketPending.get(entry.socket);
    entries?.delete(entry);
    if (entries?.size === 0) socketPending.delete(entry.socket);
  }

  function frameTooLarge(socket) {
    socket.end(encodeResponse({ requestId: null, ok: false, generation: null, result: null, error: { code: "frame_too_large", message: "request frame exceeds limit" } }));
  }

  const server = createServer((socket) => {
    activeSockets.add(socket);
    socket.on("close", () => {
      activeSockets.delete(socket);
      for (const entry of socketPending.get(socket) ?? []) {
        entry.controller.abort();
        settle(entry, { requestId: entry.id, ok: false, generation: null, result: null, error: { code: "request_cancelled", message: "request cancelled" } });
      }
    });
    let buffer = Buffer.alloc(0);
    let closingForFrame = false;
    socket.on("data", (chunk) => {
      if (closingForFrame) return;
      buffer = Buffer.concat([buffer, chunk]);
      let newline;
      while ((newline = buffer.indexOf(0x0a)) !== -1) {
        const frame = buffer.subarray(0, newline);
        buffer = buffer.subarray(newline + 1);
        if (frame.length > MAX_FRAME_BYTES) { closingForFrame = true; frameTooLarge(socket); return; }
        const message = decodeLine(frame.toString("utf8"));
        if (!message) continue;
        handleMessage(socket, message).catch(() => {});
      }
      if (buffer.length > MAX_FRAME_BYTES) { closingForFrame = true; frameTooLarge(socket); }
    });
    socket.on("error", () => {});
  });

  async function handleMessage(socket, message) {
    const requestId = String(message.requestId ?? "");
    try {
      validateProtocolVersion(message.protocolVersion);
    } catch (error) {
      socket.write(encodeResponse({ requestId, ok: false, generation: null, result: null, error: { code: error.code, message: error.message } }));
      return;
    }
    if (message.method === "cancel") {
      const target = pending.get(String(message.input?.targetRequestId ?? ""));
      if (target && target.socket === socket) {
        target.controller.abort();
        settle(target, { requestId: target.id, ok: false, generation: null, result: null, error: { code: "request_cancelled", message: "request cancelled" } });
      }
      return;
    }
    let deadlineMs;
    try { deadlineMs = validateDeadlineMs(message.deadlineMs, message.method); } catch (error) {
      socket.write(encodeResponse({ requestId, ok: false, generation: null, result: null, error: { code: error.code, message: error.message } }));
      return;
    }
    if (!METHODS.includes(message.method)) {
      socket.write(encodeResponse({ requestId, ok: false, generation: null, result: null, error: { code: "method_unknown", message: `unknown method ${message.method}` } }));
      return;
    }
    if (pending.has(requestId)) {
      socket.write(encodeResponse({ requestId, ok: false, generation: null, result: null, error: { code: "request_duplicate", message: "requestId is already active" } }));
      return;
    }
    if ((socketPending.get(socket)?.size ?? 0) >= 32 || pending.size >= 256) {
      socket.write(encodeResponse({ requestId, ok: false, generation: null, result: null, error: { code: "capacity_exceeded", message: "daemon request capacity exceeded" } }));
      return;
    }
    const controller = new AbortController();
    const entry = { id: requestId, socket, controller, settled: false, timer: null, work: null };
    entry.timer = setTimeout(() => {
      controller.abort();
      settle(entry, { requestId, ok: false, generation: null, result: null, error: { code: "deadline_exceeded", message: "request deadline exceeded" } });
    }, deadlineMs);
    pending.set(requestId, entry);
    if (!socketPending.has(socket)) socketPending.set(socket, new Set());
    socketPending.get(socket).add(entry);
    try {
      const mergedInput = { ...(message.input ?? {}) };
      if (message.repoId) mergedInput.repoId = message.repoId;
      if (message.generation) mergedInput.generation = message.generation;
      const root = canonicalRoot(message);
      if (root) mergedInput.repoRoot = root;
      if (message.method === "build") {
        entry.work = builds.build({ root, outDir: mergedInput.outDir, options: mergedInput.options ?? mergedInput }, { signal: controller.signal });
      } else {
        const method = appService[message.method];
        entry.work = (async () => {
          const session = typeof appService.openFreshnessSession === "function"
            ? await queueFreshness(root, () => appService.openFreshnessSession(mergedInput, { signal: controller.signal }))
            : null;
          try {
            return await method(mergedInput, { signal: controller.signal, session });
          } finally {
            session?.close?.();
          }
        })();
      }
      const result = await entry.work;
      settle(entry, { requestId, ok: true, generation: result?.generationId ?? null, result, error: null });
    } catch (error) {
      settle(entry, { requestId, ok: false, generation: null, result: null, error: { code: error.code ?? "internal_error", message: String(error.message ?? error) } });
    } finally { complete(entry); }
  }

  return Object.freeze({
    protocolVersion: PROTOCOL_VERSION,
    async listen() {
      if (!isWindows) {
        unixLock = acquireUnixLock(socketPath);
        try {
          await removeStaleUnixEndpoint(socketPath);
          mkdirSync(dirname(socketPath), { recursive: true });
        } catch (error) {
          releaseUnixLock(unixLock);
          unixLock = null;
          throw error;
        }
      }
      try {
        await new Promise((resolve, reject) => {
          server.once("error", reject);
          server.listen(socketPath, resolve);
        });
      } catch (error) {
        releaseUnixLock(unixLock);
        unixLock = null;
        throw error;
      }
      return { endpoint: socketPath };
    },
    close() {
      if (closePromise) return closePromise;
      closePromise = (async () => {
        const work = [...pending.values()];
        for (const entry of work) {
          clearTimeout(entry.timer);
          entry.controller.abort();
          settle(entry, { requestId: entry.id, ok: false, generation: null, result: null, error: { code: "request_cancelled", message: "request cancelled" } });
        }
        for (const socket of activeSockets) socket.destroy();
        activeSockets.clear();
        await Promise.allSettled(work.map((entry) => entry.work));
        await builds.waitForIdle();
        pending.clear();
        socketPending.clear();
        rootQueues.clear();
        if (server.listening) await new Promise((resolve) => server.close(resolve));
        if (!isWindows && unixLock) {
          rmSync(socketPath, { force: true });
          releaseUnixLock(unixLock);
          unixLock = null;
        }
      })();
      return closePromise;
    },
  });
}
