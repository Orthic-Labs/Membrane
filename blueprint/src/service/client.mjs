// D30: daemon IPC client — connects to the daemon endpoint and issues
// request/response/cancel with deadlines.

import { connect } from "node:net";
import { randomUUID } from "node:crypto";
import { MAX_DEADLINE_MS, PROTOCOL_VERSION, encodeRequest, encodeResponse, encodeCancel, decodeLine, validateDeadlineMs, validateProtocolVersion } from "./protocol.mjs";
import { daemonEndpoint } from "./paths.mjs";

function typedSocketError(code, message) {
  return Object.assign(new Error(message), { code });
}

export class DaemonClient {
  constructor({ endpoint = null } = {}) {
    this.endpoint = endpoint ?? daemonEndpoint();
    this.socket = null;
    this.buffer = "";
    this.waiters = new Map();
    this.connecting = null;
  }

  terminal(error, socket = this.socket) {
    if (socket && this.socket && socket !== this.socket) return;
    if (socket === this.socket) this.socket = null;
    this.buffer = "";
    const terminal = error?.code === "protocol_version_mismatch"
      ? error
      : typedSocketError("socket_closed", String(error?.message ?? "daemon socket closed"));
    for (const [requestId, waiter] of this.waiters) {
      this.waiters.delete(requestId);
      clearTimeout(waiter.timer);
      waiter.reject(terminal);
    }
  }

  async connect() {
    if (this.socket && !this.socket.destroyed) return this;
    if (this.socket?.destroyed) this.socket = null;
    if (this.connecting) return this.connecting;
    const socket = connect(this.endpoint);
    socket.setEncoding("utf8");
    socket.on("data", (chunk) => {
      this.buffer += chunk;
      let newline;
      while ((newline = this.buffer.indexOf("\n")) !== -1) {
        const line = this.buffer.slice(0, newline);
        this.buffer = this.buffer.slice(newline + 1);
        const message = decodeLine(line);
        if (!message) continue;
        try {
          validateProtocolVersion(message.protocolVersion);
        } catch (error) {
          this.terminal(error, socket);
          socket.destroy();
          return;
        }
        const waiter = this.waiters.get(message.requestId);
        if (waiter) {
          this.waiters.delete(message.requestId);
          clearTimeout(waiter.timer);
          waiter.resolve(message);
        }
      }
    });
    this.connecting = new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        socket.destroy();
        reject(Object.assign(new Error("connect timeout"), { code: "connect_timeout" }));
      }, 3000);
      socket.once("connect", () => { clearTimeout(timer); this.socket = socket; resolve(this); });
      socket.once("error", (error) => { clearTimeout(timer); reject(error); });
    });
    socket.on("error", (error) => this.terminal(error, socket));
    socket.on("end", () => this.terminal(null, socket));
    socket.on("close", () => this.terminal(null, socket));
    try {
      return await this.connecting;
    } finally {
      this.connecting = null;
    }
  }

  async request({ method, input = {}, repoId = null, generation = null, deadlineMs = 2000 }) {
    const validatedDeadlineMs = validateDeadlineMs(deadlineMs, method);
    if (!this.socket || this.socket.destroyed) {
      this.terminal(typedSocketError("socket_closed", "daemon socket is not connected"));
      await this.connect();
    }
    const requestId = randomUUID();
    const envelopeRepoId = repoId ?? input.repoId ?? null;
    const envelopeGeneration = generation ?? input.generation ?? null;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.waiters.delete(requestId);
        reject(Object.assign(new Error("request deadline exceeded"), { code: "deadline_exceeded" }));
      }, validatedDeadlineMs + 500);
      this.waiters.set(requestId, { resolve, reject, timer });
      try {
        this.socket.write(encodeRequest({ requestId, repoId: envelopeRepoId, generation: envelopeGeneration, method, deadlineMs: validatedDeadlineMs, input }));
      } catch (error) {
        this.waiters.delete(requestId);
        clearTimeout(timer);
        this.terminal(typedSocketError("socket_write_failed", String(error.message ?? error)));
        reject(typedSocketError("socket_write_failed", String(error.message ?? error)));
      }
    });
  }

  // Findings lane (design §7.1 items 5–7). Detection scans the repository, so
  // these default to the maximum non-build deadline instead of the 2s default.
  findingsGet(input = {}, options = {}) {
    return this.request({ method: "findings.get", input, deadlineMs: MAX_DEADLINE_MS, ...options });
  }

  findingsSarif(input = {}, options = {}) {
    return this.request({ method: "findings.sarif", input, deadlineMs: MAX_DEADLINE_MS, ...options });
  }

  findingsExplain(input = {}, options = {}) {
    return this.request({ method: "findings.explain", input, deadlineMs: MAX_DEADLINE_MS, ...options });
  }

  findingsEvidencePack(input = {}, options = {}) {
    return this.request({ method: "findings.evidence_pack", input, deadlineMs: MAX_DEADLINE_MS, ...options });
  }

  findingsBaselineCapture(input = {}, options = {}) {
    return this.request({ method: "findings.baseline.capture", input, deadlineMs: MAX_DEADLINE_MS, ...options });
  }

  findingsBaselineList(input = {}, options = {}) {
    return this.request({ method: "findings.baseline.list", input, deadlineMs: MAX_DEADLINE_MS, ...options });
  }

  cancel(targetRequestId) {
    if (!this.socket || this.socket.destroyed) {
      this.terminal(typedSocketError("socket_closed", "daemon socket is not connected"));
      throw typedSocketError("cancel_write_failed", "daemon socket is not connected");
    }
    try {
      this.socket.write(encodeCancel(targetRequestId));
    } catch (error) {
      this.terminal(typedSocketError("cancel_write_failed", String(error.message ?? error)));
      throw typedSocketError("cancel_write_failed", String(error.message ?? error));
    }
  }

  close() {
    return new Promise((resolve) => {
      if (!this.socket) return resolve();
      const socket = this.socket;
      this.socket = null;
      if (socket.destroyed) return resolve();
      socket.once("close", resolve);
      socket.destroy();
    });
  }
}

export { PROTOCOL_VERSION };
