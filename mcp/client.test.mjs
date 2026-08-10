import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { fileURLToPath } from "node:url";

const client = fileURLToPath(new URL("./client.mjs", import.meta.url));
const sourceGeneration = `sha256:${"a".repeat(64)}`;
const largeText = "x".repeat(128 * 1024);
const response = JSON.stringify({
  providerStatus: "ready",
  fallbackMode: "none",
  degradationReason: "none",
  sourceGeneration,
  packet: { blocks: [{ text: largeText }] },
  receipts: [],
});

const requests = [];
const server = createServer((request, reply) => {
  const chunks = [];
  request.on("data", (chunk) => chunks.push(chunk));
  request.on("end", () => {
    requests.push({ headers: request.headers, body: JSON.parse(Buffer.concat(chunks).toString("utf8")) });
    reply.writeHead(200, {
      "content-type": "application/json",
      "content-length": Buffer.byteLength(response),
    });
    reply.end(response);
  });
});

await new Promise((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});

try {
  const { port } = server.address();
  async function runClient(input) {
    const child = spawn(process.execPath, [client, "--input", "-"], {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
      env: {
        ...process.env,
        CRYPT_PORT: String(port),
        CRYPT_API_TOKEN: "test-token",
        WORKSPACE_ROOT: process.cwd(),
      },
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.stdin.end(JSON.stringify(input));
    const code = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("close", resolve);
    });
    return { code, stdout, stderr, parsed: JSON.parse(stdout) };
  }

  const taskEnvelope = { schema: "orthic.task-envelope.v1", task_id: "task-1" };
  const turnEnvelope = { schema: "orthic.turn-envelope.v1", task_id: "task-1", turn_id: "turn-1" };
  const absent = await runClient({
    task: "large response", repo: process.cwd(), session: "session-fallback",
    taskEnvelope, turnEnvelope,
  });
  assert.equal(absent.code, 0, absent.stderr);
  assert.equal(absent.parsed.traceId, "session-fallback");
  assert.equal(requests[0].headers.traceparent, undefined);
  assert.equal(requests[0].headers.tracestate, undefined);
  assert.equal(requests[0].headers.baggage, undefined);
  assert.equal(requests[0].headers["x-membrane-trace"], "session-fallback");
  assert.equal(requests[0].headers["x-rightcontext-trace"], "session-fallback");
  assert.equal(requests[0].headers["x-membrane-version"], "membrane-mcp/1");
  assert.equal(requests[0].headers["x-rightcontext-version"], "rightcontext-mcp/1");
  assert.deepEqual(requests[0].body.taskEnvelope, taskEnvelope);
  assert.deepEqual(requests[0].body.turnEnvelope, turnEnvelope);

  const validTrace = {
    traceparent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
    tracestate: "\t1@system=foo \t, \t, vendor=bar\t",
    baggage: "tenant=foo=bar;meta=one, other=%80value;flag",
  };
  const traced = await runClient({ task: "trace response", repo: process.cwd(), session: "ignored-for-trace", ...validTrace });
  assert.equal(traced.code, 0, traced.stderr);
  assert.equal(traced.parsed.traceId, "4bf92f3577b34da6a3ce929d0e0e4736");
  assert.equal(requests[1].body.traceparent, validTrace.traceparent);
  assert.equal(requests[1].body.baggage, validTrace.baggage);
  assert.equal(requests[1].body.tracestate?.trim(), "1@system=foo \t, \t, vendor=bar");
  assert.equal(requests[1].headers.traceparent, validTrace.traceparent);
  assert.equal(requests[1].headers.baggage, validTrace.baggage);
  assert.equal(requests[1].headers.tracestate?.trim(), "1@system=foo \t, \t, vendor=bar");
  assert.equal(requests[1].headers["x-membrane-trace"], "4bf92f3577b34da6a3ce929d0e0e4736");
  assert.equal(requests[1].headers["x-rightcontext-trace"], "4bf92f3577b34da6a3ce929d0e0e4736");

  const invalid = await runClient({ task: "invalid trace", repo: process.cwd(), session: "invalid-fallback", traceparent: "not-a-trace", tracestate: "x".repeat(513), baggage: "bad\nvalue" });
  assert.equal(invalid.code, 0, invalid.stderr);
  assert.equal(invalid.parsed.traceId, "invalid-fallback");
  assert.equal(requests[2].headers.traceparent, undefined);
  assert.equal(requests[2].headers.tracestate, undefined);
  assert.equal(requests[2].headers.baggage, undefined);
  assert.equal(requests[2].body.traceparent, undefined);
  assert.equal(requests[2].body.tracestate, undefined);
  assert.equal(requests[2].body.baggage, undefined);

  const malformedTraceState = await runClient({
    task: "malformed tracestate", repo: process.cwd(), session: "trace-state-fallback",
    traceparent: validTrace.traceparent, tracestate: "vendor=foo=bar", baggage: "tenant=foo=bar",
  });
  assert.equal(malformedTraceState.code, 0, malformedTraceState.stderr);
  assert.equal(requests[3].headers.traceparent, validTrace.traceparent);
  assert.equal(requests[3].headers.tracestate, undefined);
  assert.equal(requests[3].headers.baggage, "tenant=foo=bar");

  const malformedLists = await runClient({
    task: "malformed trace lists", repo: process.cwd(), session: "trace-list-fallback",
    traceparent: validTrace.traceparent, tracestate: "vendor=ok,=partial", baggage: "tenant=value\r\nInjected: no",
  });
  assert.equal(malformedLists.code, 0, malformedLists.stderr);
  assert.equal(requests[4].headers.traceparent, validTrace.traceparent);
  assert.equal(requests[4].headers.tracestate, undefined);
  assert.equal(requests[4].headers.baggage, undefined);

  const noTraceparent = await runClient({
    task: "tracestate without parent", repo: process.cwd(), session: "no-parent-fallback",
    tracestate: "vendor=foo", baggage: "tenant=foo=bar",
  });
  assert.equal(noTraceparent.code, 0, noTraceparent.stderr);
  assert.equal(requests[5].headers.traceparent, undefined);
  assert.equal(requests[5].headers.tracestate, undefined);
  assert.equal(requests[5].headers.baggage, "tenant=foo=bar");

  const uppercaseParent = await runClient({
    task: "uppercase traceparent", repo: process.cwd(), session: "uppercase-fallback",
    traceparent: validTrace.traceparent.toUpperCase(), tracestate: "vendor=foo", baggage: "tenant=foo=bar",
  });
  assert.equal(uppercaseParent.code, 0, uppercaseParent.stderr);
  assert.equal(requests[6].headers.traceparent, undefined);
  assert.equal(requests[6].headers.tracestate, undefined);
  assert.equal(requests[6].headers.baggage, "tenant=foo=bar");

  const duplicateState = await runClient({
    task: "duplicate tracestate", repo: process.cwd(), session: "duplicate-fallback",
    traceparent: validTrace.traceparent, tracestate: "vendor=foo,vendor=bar", baggage: "tenant=foo=bar",
  });
  assert.equal(duplicateState.code, 0, duplicateState.stderr);
  assert.equal(requests[7].headers.traceparent, validTrace.traceparent);
  assert.equal(requests[7].headers.tracestate, undefined);
  assert.equal(requests[7].headers.baggage, "tenant=foo=bar");

  const rawObsText = await runClient({
    task: "raw baggage obs text", repo: process.cwd(), session: "obs-text-fallback",
    traceparent: validTrace.traceparent, tracestate: "vendor=foo", baggage: "tenant=\x80value",
  });
  assert.equal(rawObsText.code, 0, rawObsText.stderr);
  assert.equal(requests[8].headers.traceparent, validTrace.traceparent);
  assert.equal(requests[8].headers.tracestate, "vendor=foo");
  assert.equal(requests[8].headers.baggage, undefined);

  const value256 = "x".repeat(256);
  const value257 = "x".repeat(257);
  const tracestateBoundary = await runClient({
    task: "tracestate value boundary", repo: process.cwd(), session: "boundary-fallback",
    traceparent: validTrace.traceparent, tracestate: `vendor=${value256}`, baggage: "tenant=foo=bar",
  });
  assert.equal(tracestateBoundary.code, 0, tracestateBoundary.stderr);
  assert.equal(requests[9].headers.tracestate, `vendor=${value256}`);

  const tracestateOverflow = await runClient({
    task: "tracestate value overflow", repo: process.cwd(), session: "overflow-fallback",
    traceparent: validTrace.traceparent, tracestate: `vendor=${value257}`, baggage: "tenant=foo=bar",
  });
  assert.equal(tracestateOverflow.code, 0, tracestateOverflow.stderr);
  assert.equal(requests[10].headers.traceparent, validTrace.traceparent);
  assert.equal(requests[10].headers.tracestate, undefined);

  const futureTraceparent = "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-future";
  const futureTrace = await runClient({
    task: "future traceparent", repo: process.cwd(), session: "future-fallback",
    traceparent: futureTraceparent, tracestate: "vendor=future", baggage: "tenant=foo=bar",
  });
  assert.equal(futureTrace.code, 0, futureTrace.stderr);
  assert.equal(futureTrace.parsed.traceId, "4bf92f3577b34da6a3ce929d0e0e4736");
  assert.equal(requests[11].body.traceparent, futureTraceparent);
  assert.equal(requests[11].body.tracestate, "vendor=future");
  assert.equal(requests[11].headers.traceparent, futureTraceparent);
  assert.equal(requests[11].headers.tracestate, "vendor=future");
  assert.equal(requests[11].headers["x-membrane-trace"], "4bf92f3577b34da6a3ce929d0e0e4736");
  assert.equal(requests[11].headers["x-rightcontext-trace"], "4bf92f3577b34da6a3ce929d0e0e4736");

  const generated = await runClient({ task: "generated trace fallback", repo: process.cwd() });
  assert.equal(generated.code, 0, generated.stderr);
  assert.match(generated.parsed.traceId, /^mcp-[0-9a-f]+$/);
  assert.equal(requests[12].headers.traceparent, undefined);
  assert.equal(requests[12].headers["x-membrane-trace"], generated.parsed.traceId);
  assert.equal(requests[12].headers["x-rightcontext-trace"], generated.parsed.traceId);

  // CU-10 / MBR-007 envelope continuity: clientEnvelope + overlay survive the client round-trip
  const clientEnv = { schema: "orthic.client-envelope.v1", clientId: "test-client", adapterVersion: "9.9.9" };
  const overlayId = { schema: "orthic.overlay-identity.v1", sessionId: "session-fallback", worktreePath: process.cwd() };
  const envelopeFull = await runClient({
    task: "envelope continuity", repo: process.cwd(), session: "session-fallback",
    taskEnvelope, turnEnvelope, clientEnvelope: clientEnv, overlay: overlayId,
  });
  assert.equal(envelopeFull.code, 0, envelopeFull.stderr);
  // The client POSTs all four envelopes to the planner: verify they arrived byte-for-byte
  const lastIdx = requests.length - 1;
  assert.deepEqual(requests[lastIdx].body.taskEnvelope, taskEnvelope);
  assert.deepEqual(requests[lastIdx].body.turnEnvelope, turnEnvelope);
  assert.deepEqual(requests[lastIdx].body.clientEnvelope, clientEnv);
  assert.deepEqual(requests[lastIdx].body.overlay, overlayId);

  assert.equal(absent.parsed.packet.blocks[0].text.length, largeText.length);
} finally {
  await new Promise((resolve) => server.close(resolve));
}
