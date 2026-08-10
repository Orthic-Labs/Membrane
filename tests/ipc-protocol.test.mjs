// D30: daemon IPC protocol — envelopes, methods, cancellation, deadlines.

import assert from "node:assert/strict";
import { mkdtempSync, cpSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { connect, createServer } from "node:net";
import test from "node:test";

import { encodeRequest, encodeResponse, encodeCancel, decodeLine, PROTOCOL_VERSION, METHODS, validateDeadlineMs, validateProtocolVersion } from "../service/protocol.mjs";
import { createDaemonServer } from "../service/server.mjs";
import { DaemonClient } from "../service/client.mjs";
import { daemonEndpoint, temporaryDaemonEndpoint } from "../service/paths.mjs";
import { buildGraphGeneration } from "../graph/static-provider.mjs";
import { createCortexApplicationService } from "../lib/application/service.mjs";
import { RootRegistry } from "../lib/application/root-registry.mjs";

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function rawSocket(endpoint) {
  const socket = connect(endpoint);
  socket.setEncoding("utf8");
  socket.on("error", () => {});
  await new Promise((resolve) => socket.once("connect", resolve));
  const messages = [];
  let buffer = "";
  socket.on("data", (chunk) => {
    buffer += chunk;
    let newline;
    while ((newline = buffer.indexOf("\n")) !== -1) messages.push(decodeLine(buffer.slice(0, newline))), buffer = buffer.slice(newline + 1);
  });
  return { socket, messages };
}

async function until(predicate) {
  for (let attempts = 0; attempts < 50; attempts += 1) {
    if (predicate()) return;
    await pause(10);
  }
  assert.fail("timed out waiting for daemon response");
}

test("protocol envelopes round-trip", () => {
  const request = decodeLine(encodeRequest({ requestId: "r1", repoId: "repo-1", method: "search", input: { query: "x" } }));
  assert.equal(request.protocolVersion, PROTOCOL_VERSION);
  assert.equal(request.method, "search");
  assert.equal(request.repoId, "repo-1");
  assert.equal(request.deadlineMs, 2000);
  const response = decodeLine(encodeResponse({ requestId: "r1", ok: true, generation: "g1", result: {}, error: null }));
  assert.equal(response.ok, true);
  assert.equal(response.generation, "g1");
  const cancel = decodeLine(encodeCancel("r1"));
  assert.equal(cancel.protocolVersion, PROTOCOL_VERSION);
  assert.equal(cancel.method, "cancel");
  assert.equal(cancel.input.targetRequestId, "r1");
  const pipe = daemonEndpoint({ platform: "win32", identity: "local-user" });
  assert.match(pipe, /^\\\\\.\\pipe\\orthic-cortex-[a-f0-9]{16}$/);
  assert.equal(pipe, daemonEndpoint({ platform: "win32", identity: "local-user" }));
});

test("METHODS covers the six-tool surface plus read verbs", () => {
  for (const method of ["status", "search", "resolve", "orient", "expand", "impact", "architecture", "documentTruth"]) {
    assert.ok(METHODS.includes(method));
  }
});

test("client close destroys its socket without waiting for peer EOF", async () => {
  let peer;
  const server = createServer({ allowHalfOpen: true }, (socket) => { peer = socket; socket.on("error", () => {}); });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  const client = new DaemonClient({ endpoint: address.port });
  try {
    await client.connect();
    const socket = client.socket;
    await client.close();
    assert.equal(socket.destroyed, true);
  } finally {
    peer?.destroy();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("daemon serves a search over the shared service", async () => {
  const repo = mkdtempSync(join(tmpdir(), "cortex-daemon-"));
  cpSync(join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce"), repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  const endpoint = temporaryDaemonEndpoint("cortex");
  const registry = new RootRegistry([{ root: repo, repoId: "repo-1" }]);
  const service = createCortexApplicationService({ rootRegistry: registry, allowEmbeddedRoot: false });
  const daemon = createDaemonServer({ service, endpoint });
  try {
    await daemon.listen();
    const client = new DaemonClient({ endpoint });
    const response = await client.request({ method: "search", input: { query: "placeOrder", limit: 5 }, repoId: "repo-1" });
    assert.equal(response.ok, true);
    assert.ok(Array.isArray(response.result.results));
    assert.ok(response.result.results.length > 0);
    await client.close();
  } finally {
    await daemon.close().catch(() => {});
    rmSync(repo, { recursive: true, force: true });
  }
});

test("unknown method returns method_unknown", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-u");
  const daemon = createDaemonServer({ endpoint });
  try {
    await daemon.listen();
    const client = new DaemonClient({ endpoint });
    const response = await client.request({ method: "nope", input: {} });
    assert.equal(response.ok, false);
    assert.equal(response.error.code, "method_unknown");
    await client.close();
  } finally {
    await daemon.close().catch(() => {});
  }
});

test("cancellation settles its exact pending request once & aborts work", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-c");
  let release;
  let signal;
  let calls = 0;
  let socket;
  let messages;
  const daemon = createDaemonServer({ endpoint, service: { status(_input, options) {
    calls += 1;
    signal = options?.signal;
    return new Promise((resolve) => { release = resolve; });
  } } });
  try {
    await daemon.listen();
    ({ socket, messages } = await rawSocket(endpoint));
    socket.write(encodeRequest({ requestId: "pending-1", method: "status", input: {}, deadlineMs: 30000 }));
    await until(() => signal);
    socket.write(encodeRequest({ requestId: "pending-1", method: "status", input: {}, deadlineMs: 30000 }));
    await until(() => messages.some((message) => message?.error?.code === "request_duplicate"));
    assert.equal(calls, 1);
    socket.write(encodeCancel("pending-1"));
    await until(() => messages.some((message) => message?.error?.code === "request_cancelled"));
    assert.equal(signal.aborted, true);
    release({ generationId: "late" });
    await pause(25);
    assert.equal(messages.filter((message) => message?.requestId === "pending-1").length, 2);
    socket.end();
  } finally {
    socket?.destroy();
    await daemon.close().catch(() => {});
  }
});

test("protocol version validation rejects absent and mismatched envelopes", () => {
  assert.equal(validateProtocolVersion(PROTOCOL_VERSION), PROTOCOL_VERSION);
  assert.throws(() => validateProtocolVersion(), { code: "protocol_version_mismatch" });
  assert.throws(() => validateProtocolVersion(PROTOCOL_VERSION + 1), { code: "protocol_version_mismatch" });
});

test("daemon rejects missing and mismatched protocol versions before invoking its service", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-version-request");
  let calls = 0;
  const daemon = createDaemonServer({ endpoint, service: { status: async () => { calls += 1; return { generationId: "g" }; } } });
  let socket;
  try {
    await daemon.listen();
    let messages;
    ({ socket, messages } = await rawSocket(endpoint));
    socket.write(`${JSON.stringify({ requestId: "missing", method: "status", deadlineMs: 2000, input: {} })}\n`);
    socket.write(`${JSON.stringify({ protocolVersion: PROTOCOL_VERSION + 1, requestId: "wrong", method: "status", deadlineMs: 2000, input: {} })}\n`);
    await until(() => messages.length === 2);
    assert.deepEqual(messages.map((message) => message.error.code), ["protocol_version_mismatch", "protocol_version_mismatch"]);
    assert.equal(calls, 0);
  } finally {
    socket?.destroy();
    await daemon.close().catch(() => {});
  }
});

test("daemon close waits for controlled work to settle before releasing ownership", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-close-waits");
  let release;
  const daemon = createDaemonServer({ endpoint, service: { status: () => new Promise((resolve) => { release = resolve; }) } });
  let socket;
  try {
    await daemon.listen();
    ({ socket } = await rawSocket(endpoint));
    socket.write(encodeRequest({ requestId: "controlled", method: "status", deadlineMs: 30000 }));
    await until(() => release);
    let closed = false;
    const closing = daemon.close().then(() => { closed = true; });
    await pause(25);
    assert.equal(closed, false);
    release({ generationId: "g" });
    await closing;
    assert.equal(closed, true);
  } finally {
    release?.({ generationId: "g" });
    socket?.destroy();
    await daemon.close().catch(() => {});
  }
});

test("client rejects a mismatched response version and reconnects for its next request", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-version-response");
  let connections = 0;
  const server = createServer((socket) => {
    socket.setEncoding("utf8");
    socket.once("data", (line) => {
      const request = decodeLine(line);
      connections += 1;
      socket.end(`${JSON.stringify({
        protocolVersion: connections === 1 ? PROTOCOL_VERSION + 1 : PROTOCOL_VERSION,
        requestId: request.requestId,
        ok: true,
        generation: "g",
        result: { accepted: true },
        error: null,
      })}\n`);
    });
  });
  try {
    await new Promise((resolve, reject) => { server.once("error", reject); server.listen(endpoint, resolve); });
    const client = new DaemonClient({ endpoint });
    await assert.rejects(client.request({ method: "status" }), { code: "protocol_version_mismatch" });
    const response = await client.request({ method: "status" });
    assert.equal(response.ok, true);
    assert.equal(connections, 2);
    await client.close();
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

test("client rejects terminal sockets and reconnects after pending work is lost", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-terminal-retry");
  let connections = 0;
  const server = createServer((socket) => {
    socket.setEncoding("utf8");
    socket.once("data", (line) => {
      const request = decodeLine(line);
      connections += 1;
      if (connections === 1) socket.destroy();
      else socket.end(encodeResponse({ requestId: request.requestId, ok: true, generation: "g", result: {}, error: null }));
    });
  });
  try {
    await new Promise((resolve, reject) => { server.once("error", reject); server.listen(endpoint, resolve); });
    const client = new DaemonClient({ endpoint });
    await assert.rejects(client.request({ method: "status", deadlineMs: 30000 }), { code: "socket_closed" });
    const response = await client.request({ method: "status" });
    assert.equal(response.ok, true);
    assert.equal(connections, 2);
    await client.close();
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

test("client returns typed write and cancel failures", async () => {
  const client = new DaemonClient({ endpoint: "unused" });
  client.socket = { destroyed: false, write() { throw new Error("write failed"); } };
  await assert.rejects(client.request({ method: "status" }), { code: "socket_write_failed" });
  assert.throws(() => client.cancel("request-1"), { code: "cancel_write_failed" });
});

test("client sends cancels with the shared protocol version", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-client-cancel");
  let received;
  const server = createServer((socket) => {
    socket.setEncoding("utf8");
    socket.once("data", (line) => { received = decodeLine(line); });
  });
  try {
    await new Promise((resolve, reject) => { server.once("error", reject); server.listen(endpoint, resolve); });
    const client = new DaemonClient({ endpoint });
    await client.connect();
    client.cancel("active-request");
    await until(() => received);
    assert.equal(received.protocolVersion, PROTOCOL_VERSION);
    assert.equal(received.method, "cancel");
    assert.equal(received.input.targetRequestId, "active-request");
    await client.close();
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

test("client discards a destroyed cached socket before reconnecting", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-destroyed-cache");
  const server = createServer((socket) => {
    socket.setEncoding("utf8");
    socket.once("data", (line) => {
      const request = decodeLine(line);
      socket.end(encodeResponse({ requestId: request.requestId, ok: true, generation: "g", result: {}, error: null }));
    });
  });
  try {
    await new Promise((resolve, reject) => { server.once("error", reject); server.listen(endpoint, resolve); });
    const client = new DaemonClient({ endpoint });
    client.socket = { destroyed: true };
    const response = await client.request({ method: "status" });
    assert.equal(response.ok, true);
    await client.close();
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

test("deadline validation accepts only an integer in its shared range", () => {
  for (const value of ["10", null, NaN, Infinity, 10.5, 9, 30001]) {
    assert.throws(() => validateDeadlineMs(value), { code: "deadline_invalid" });
  }
  assert.equal(validateDeadlineMs(), 2000);
  assert.equal(validateDeadlineMs(10), 10);
  assert.equal(validateDeadlineMs(30000), 30000);
});

test("deadline settles a request once despite late completion", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-deadline");
  let release;
  let socket;
  const daemon = createDaemonServer({ endpoint, service: { status: () => new Promise((resolve) => { release = resolve; }) } });
  try {
    await daemon.listen();
    let messages;
    ({ socket, messages } = await rawSocket(endpoint));
    socket.write(encodeRequest({ requestId: "deadline-1", method: "status", input: {}, deadlineMs: 10 }));
    await until(() => messages.some((message) => message?.requestId === "deadline-1"));
    assert.equal(messages.find((message) => message?.requestId === "deadline-1").error.code, "deadline_exceeded");
    release({ generationId: "late" });
    await pause(25);
    assert.equal(messages.filter((message) => message?.requestId === "deadline-1").length, 1);
    socket.end();
  } finally {
    socket?.destroy();
    await daemon.close().catch(() => {});
  }
});

test("daemon rejects every malformed deadline with deadline_invalid", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-deadline-invalid");
  const daemon = createDaemonServer({ endpoint });
  let socket;
  try {
    await daemon.listen();
    const messages = [];
    ({ socket } = await rawSocket(endpoint));
    socket.on("data", (chunk) => {
      for (const line of String(chunk).trim().split("\n")) messages.push(decodeLine(line));
    });
    for (const [index, deadlineMs] of ["10", null, 10.5, 9, 30001].entries()) {
      socket.write(`${JSON.stringify({ protocolVersion: PROTOCOL_VERSION, requestId: `bad-${index}`, method: "status", deadlineMs, input: {} })}\n`);
      await until(() => messages.some((message) => message?.requestId === `bad-${index}`));
      assert.equal(messages.find((message) => message?.requestId === `bad-${index}`).error.code, "deadline_invalid");
    }
  } finally {
    socket?.destroy();
    await daemon.close().catch(() => {});
  }
});

test("total daemon capacity rejects a 257th active request", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-total-capacity");
  const releases = [];
  const daemon = createDaemonServer({ endpoint, service: { status: () => new Promise((resolve) => releases.push(resolve)) } });
  const clients = [];
  try {
    await daemon.listen();
    for (let socketIndex = 0; socketIndex < 8; socketIndex += 1) {
      const client = await rawSocket(endpoint);
      clients.push(client);
      for (let requestIndex = 0; requestIndex < 32; requestIndex += 1) {
        client.socket.write(encodeRequest({ requestId: `total-${socketIndex}-${requestIndex}`, method: "status", deadlineMs: 30000 }));
      }
    }
    await until(() => releases.length === 256);
    const overflow = await rawSocket(endpoint);
    clients.push(overflow);
    overflow.socket.write(encodeRequest({ requestId: "total-overflow", method: "status", deadlineMs: 30000 }));
    await until(() => overflow.messages.some((message) => message?.requestId === "total-overflow"));
    assert.equal(overflow.messages.find((message) => message?.requestId === "total-overflow").error.code, "capacity_exceeded");
  } finally {
    for (const release of releases) release({ generationId: "done" });
    for (const client of clients) client.socket.destroy();
    await daemon.close().catch(() => {});
  }
});

test("daemon parses multiple valid frames larger than 8KiB from one chunk", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-frames");
  const daemon = createDaemonServer({ endpoint, service: { status: async () => ({ generationId: "g" }) } });
  let socket;
  try {
    await daemon.listen();
    const messages = [];
    ({ socket } = await rawSocket(endpoint));
    socket.on("data", (chunk) => {
      for (const line of String(chunk).trim().split("\n")) messages.push(decodeLine(line));
    });
    const padding = "x".repeat(9 * 1024);
    socket.write(encodeRequest({ requestId: "large-a", method: "status", deadlineMs: 2000, input: { padding } })
      + encodeRequest({ requestId: "large-b", method: "status", deadlineMs: 2000, input: { padding } }));
    await until(() => messages.filter((message) => message?.ok).length === 2);
  } finally {
    socket?.destroy();
    await daemon.close().catch(() => {});
  }
});

test("oversized terminated frames receive a typed error & close", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-frame-line");
  const daemon = createDaemonServer({ endpoint });
  let socket;
  try {
    await daemon.listen();
    let messages;
    ({ socket, messages } = await rawSocket(endpoint));
    socket.write(`${"x".repeat(16 * 1024 + 1)}\n`);
    await until(() => messages.some((message) => message?.error?.code === "frame_too_large"));
  } finally {
    socket?.destroy();
    await daemon.close().catch(() => {});
  }
});

test("cancelled work retains per-socket capacity until its promise exits", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-capacity");
  const releases = [];
  const daemon = createDaemonServer({ endpoint, service: { status: () => new Promise((resolve) => releases.push(resolve)) } });
  let socket;
  try {
    await daemon.listen();
    let messages;
    ({ socket, messages } = await rawSocket(endpoint));
    for (let index = 0; index < 32; index += 1) socket.write(encodeRequest({ requestId: `full-${index}`, method: "status", deadlineMs: 30000 }));
    await until(() => releases.length === 32);
    socket.write(encodeRequest({ requestId: "over", method: "status", deadlineMs: 30000 }));
    await until(() => messages.some((message) => message?.requestId === "over"));
    assert.equal(messages.find((message) => message?.requestId === "over").error.code, "capacity_exceeded");
    socket.write(encodeCancel("full-0"));
    await until(() => messages.some((message) => message?.requestId === "full-0" && message.error?.code === "request_cancelled"));
    socket.write(encodeRequest({ requestId: "still-over", method: "status", deadlineMs: 30000 }));
    await until(() => messages.some((message) => message?.requestId === "still-over"));
    assert.equal(messages.find((message) => message?.requestId === "still-over").error.code, "capacity_exceeded");
    releases.shift()({ generationId: "done" });
    socket.write(encodeRequest({ requestId: "freed", method: "status", deadlineMs: 30000 }));
    await until(() => releases.length === 32);
  } finally {
    for (const release of releases) release({ generationId: "done" });
    socket?.destroy();
    await daemon.close().catch(() => {});
  }
});

test("socket close aborts its active work", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-close-abort");
  let signal;
  let release;
  const daemon = createDaemonServer({ endpoint, service: { status: (_input, options) => {
    signal = options.signal;
    return new Promise((resolve) => { release = resolve; });
  } } });
  let socket;
  try {
    await daemon.listen();
    ({ socket } = await rawSocket(endpoint));
    socket.write(encodeRequest({ requestId: "closed", method: "status", deadlineMs: 30000 }));
    await until(() => signal);
    socket.destroy();
    await until(() => signal.aborted);
    release({ generationId: "closed" });
  } finally {
    release?.({ generationId: "closed" });
    socket?.destroy();
    await daemon.close().catch(() => {});
  }
});

test("oversized unterminated frames receive a typed error & close", async () => {
  const endpoint = temporaryDaemonEndpoint("cortex-frame");
  const daemon = createDaemonServer({ endpoint });
  let socket;
  try {
    await daemon.listen();
    let messages;
    ({ socket, messages } = await rawSocket(endpoint));
    let closed = false;
    socket.on("close", () => { closed = true; });
    socket.write("x".repeat(16 * 1024 + 1));
    await until(() => closed);
    assert.equal(messages[0].error.code, "frame_too_large");
  } finally {
    socket?.destroy();
    await daemon.close().catch(() => {});
  }
});
