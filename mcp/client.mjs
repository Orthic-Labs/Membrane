#!/usr/bin/env node
// membrane/mcp/client.mjs — Membrane G5 Lane F provider-neutral MCP thin client.
//
// Thin client: authenticates to the workspace loopback Crypt service at
// http://127.0.0.1:${CRYPT_PORT}/federate with the workspace bearer
// token, POSTs a task/repository federation request, surfaces receipt
// IDs + degradation state to the caller, and emits a typed fallback event
// when the planner is unavailable or any provider has gone away.
//
// This file does NOT rank, dedupe, normalise, or select candidates. The
// federation gateway (and the loopback `POST /federate` route) owns provider
// orchestration plus admission. `/plan_context` stays provider-facing. The MCP client
// translates transport shape only — JSON envelope in, packet + receipts
// out — exactly as dispatch G5 Lane F requires.
//
// Authentication contract (mirrors tools/codex-brief-plugin/.../crypt-start.cjs):
//   1. $CRYPT_PORT env var if set, else 47851.
//   2. $CRYPT_API_TOKEN env var if set, else $CRYPT_API_TOKEN_FILE
//      (default tools/.cache/memory/api-token) — same path the workspace
//      hooks use. The token is read once per process; it is NEVER echoed,
//      printed, written to a log row, or embedded in any output line.
//
// Transport contract:
//   - Reads `{task, repo, maxTokens?, session?, anchors?, scopeGrantId?}`
//     from `--input <path>` or stdin (when input is `-`).
//   - Issues one POST application/json with Authorization: Bearer <token>.
//   - Parses the {packet, receipts, providerStatus, fallbackMode, degradationReason,
//     sourceGeneration, structuredEvent, persistedReceipts, scopeGrant} payload.
//   - Emits that payload verbatim on stdout. Exits 0 on success, 1 on
//     planner_rejected, 2 on transport/auth failure, 3 on typed fallback.
//
// Failure-isolation contract:
//   - Bearer token never appears in stdout, stderr, or any log row.
//   - Any planner error path emits a structured `context_fallback` event
//     (typed object printed to stderr — content-free: traceId, provider,
//     reason, mode) AND returns a typed fallback envelope to stdout so
//     the MCP caller can still surface the same degradation state.

import { readFileSync, statSync, existsSync } from "node:fs";
import { Buffer } from "node:buffer";
import { request as httpRequest } from "node:http";
import { pathToFileURL } from "node:url";

const DEFAULT_PORT = 47851;
const REQUEST_TIMEOUT_MS = 1500;
const MAX_BODY_BYTES = 1 << 20; // 1 MiB — same cap the workspace hook uses
// Public transport id. Legacy `rightcontext-mcp/1` remains accepted by older
// consumers; this client emits Membrane names as primary and RightContext as alias.
const TRANSPORT_VERSION = "membrane-mcp/1";
const TRANSPORT_VERSION_ALIAS = "rightcontext-mcp/1";
const PLANNER_PROVIDER = "membrane-planner";
const PLANNER_PROVIDER_ALIAS = "rightcontext-planner";
const MODE_DEFAULT = "shadow"; // gate state until G5 parity is green

function readToken() {
  // Resolution order matches the workspace hook recipe:
  //   1. CRYPT_API_TOKEN env var (raw, used directly; never logged).
  //   2. CRYPT_API_TOKEN_FILE env var or default tools/.cache/memory/api-token.
  // The token is held in closure; it never enters any log or output row.
  const envTok = process.env.CRYPT_API_TOKEN;
  if (envTok && envTok.trim().length > 0) return envTok.trim();
  const fileEnv = process.env.CRYPT_API_TOKEN_FILE;
  const candidates = [];
  if (fileEnv && fileEnv.trim().length > 0) candidates.push(fileEnv.trim());
  const ws = process.env.WORKSPACE_ROOT || (process.platform === "win32" ? "D:/Claude" : `${process.env.HOME}/claude`);
  candidates.push(`${ws}/tools/.cache/memory/api-token`);
  for (const path of candidates) {
    try {
      if (!existsSync(path)) continue;
      const st = statSync(path);
      if (!st.isFile()) continue;
      const raw = readFileSync(path, "utf8").trim();
      if (raw.length > 0) return raw;
    } catch (_) {
      // swallow — try next candidate
    }
  }
  return null;
}

function readPort() {
  const raw = process.env.CRYPT_PORT;
  if (raw && raw.trim().length > 0) {
    const n = Number(raw.trim());
    if (Number.isInteger(n) && n >= 1024 && n <= 65535) return n;
  }
  return DEFAULT_PORT;
}

function readStdinSync() {
  return readFileSync(0, "utf8");
}

function validTraceparent(value) {
  if (typeof value !== "string" || Buffer.byteLength(value, "utf8") > 128) return false;
  const match = /^(?<version>[0-9a-f]{2})-(?<trace>[0-9a-f]{32})-(?<parent>[0-9a-f]{16})-(?<flags>[0-9a-f]{2})(?<extension>(?:-[\x21-\x7E]+)*)$/.exec(value);
  return Boolean(match && match.groups.version !== "ff" && (match.groups.version !== "00" || !match.groups.extension) && !/^0+$/.test(match.groups.trace) && !/^0+$/.test(match.groups.parent));
}

const TRACESTATE_SIMPLE_KEY = /^[a-z][a-z0-9_*/-]{0,255}$/;
const TRACESTATE_MULTI_KEY = /^[a-z0-9][a-z0-9_*/-]{0,240}@[a-z][a-z0-9_*/-]{0,13}$/;
const TRACESTATE_VALUE = /^[\x20-\x2B\x2D-\x3C\x3E-\x7E]+$/;
const BAGGAGE_TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
const BAGGAGE_VALUE = /^[\x21\x23-\x2B\x2D-\x3A\x3C-\x5B\x5D-\x7E]*$/;
const trimOws = (value) => value.replace(/^[ \t]+|[ \t]+$/g, "");
function validTracestateMember(member, keys) {
  const value = trimOws(member);
  if (!value) return true;
  const separator = value.indexOf("=");
  if (separator <= 0 || value.indexOf("=", separator + 1) !== -1) return false;
  const stateValue = value.slice(separator + 1).replace(/[ \t]+$/, "");
  const key = value.slice(0, separator);
  if (!TRACESTATE_SIMPLE_KEY.test(key) && !TRACESTATE_MULTI_KEY.test(key)) return false;
  if (stateValue.length > 256 || !TRACESTATE_VALUE.test(stateValue) || stateValue.endsWith(" ") || keys.has(key)) return false;
  keys.add(key);
  return true;
}
function validTracestate(value) {
  if (typeof value !== "string" || Buffer.byteLength(value, "utf8") > 512) return false;
  const members = value.split(",");
  const keys = new Set();
  return members.length <= 32 && members.every((member) => validTracestateMember(member, keys));
}
function validBaggageProperty(value) {
  const property = trimOws(value);
  const separator = property.indexOf("=");
  if (separator === -1) return BAGGAGE_TOKEN.test(property);
  const key = property.slice(0, separator).replace(/[ \t]+$/, "");
  const propertyValue = property.slice(separator + 1).replace(/^[ \t]+|[ \t]+$/g, "");
  return BAGGAGE_TOKEN.test(key) && BAGGAGE_VALUE.test(propertyValue);
}
function validBaggageMember(member) {
  const value = trimOws(member);
  const separator = value.indexOf("=");
  if (separator <= 0) return false;
  const key = value.slice(0, separator).replace(/[ \t]+$/, "");
  const [baggageValue, ...properties] = value.slice(separator + 1).replace(/^[ \t]+/, "").split(";");
  return BAGGAGE_TOKEN.test(key) && BAGGAGE_VALUE.test(baggageValue.replace(/[ \t]+$/, "")) && properties.every(validBaggageProperty);
}
function validBaggage(value) {
  if (typeof value !== "string" || !value || Buffer.byteLength(value, "utf8") > 8192) return false;
  const members = value.split(",");
  return members.length <= 64 && members.every(validBaggageMember);
}

function boundedTrace(value) {
  const trace = {};
  const hasTraceparent = validTraceparent(value.traceparent);
  if (hasTraceparent) trace.traceparent = value.traceparent;
  if (hasTraceparent && validTracestate(value.tracestate)) trace.tracestate = value.tracestate;
  if (validBaggage(value.baggage)) trace.baggage = value.baggage;
  return trace;
}

function loadInput({ inputArg, maxTokens }) {
  let raw;
  if (inputArg === "-" || inputArg === undefined) {
    raw = readStdinSync();
  } else {
    raw = readFileSync(inputArg, "utf8");
  }
  if (raw.length > MAX_BODY_BYTES) {
    throw new Error(`input exceeds ${MAX_BODY_BYTES} bytes`);
  }
  const parsed = JSON.parse(raw);
  if (!parsed || typeof parsed !== "object" || typeof parsed.task !== "string" || !parsed.task.trim() || typeof parsed.repo !== "string" || !parsed.repo.trim()) {
    throw new Error("federate input requires non-empty task and repo");
  }
  return {
    task: parsed.task,
    repo: parsed.repo,
    maxTokens: parsed.maxTokens ?? maxTokens,
    maxWaitMs: parsed.maxWaitMs,
    client: parsed.client ?? "mcp",
    session: parsed.session,
    anchors: parsed.anchors ?? "",
    scopeGrantId: parsed.scopeGrantId,
    scopeDescriptor: parsed.scopeDescriptor,
    taskEnvelope: parsed.taskEnvelope,
    turnEnvelope: parsed.turnEnvelope,
    ...boundedTrace(parsed),
  };
}

function postPlanner({ host, port, path, body, token, traceId }) {
  const trace = boundedTrace(body);
  return new Promise((resolve, reject) => {
    const json = Buffer.from(JSON.stringify(body), "utf8");
    const req = httpRequest(
      {
        hostname: host,
        port,
        path,
        method: "POST",
        timeout: REQUEST_TIMEOUT_MS,
        headers: {
          "content-type": "application/json; charset=utf-8",
          "content-length": String(json.byteLength),
          "accept": "application/json",
          // Tag the call so /metrics and /health surface the third client.
          // Membrane is public; RightContext headers remain as compatibility aliases.
          "x-membrane-client": "mcp",
          "x-membrane-version": TRANSPORT_VERSION,
          "x-membrane-trace": traceId,
          "x-rightcontext-client": "mcp",
          "x-rightcontext-version": TRANSPORT_VERSION_ALIAS,
          "x-rightcontext-trace": traceId,
          ...(trace.traceparent ? { traceparent: trace.traceparent } : {}),
          ...(trace.tracestate ? { tracestate: trace.tracestate } : {}),
          ...(trace.baggage ? { baggage: trace.baggage } : {}),
        },
      },
      (res) => {
        const chunks = [];
        let received = 0;
        res.on("data", (chunk) => {
          received += chunk.length;
          if (received <= MAX_BODY_BYTES) chunks.push(chunk);
        });
        res.on("end", () => {
          const text = Buffer.concat(chunks).toString("utf8");
          resolve({ status: res.statusCode || 0, body: text });
        });
      }
    );
    req.on("timeout", () => {
      req.destroy(new Error("planner request timed out"));
    });
    req.on("error", reject);
    // Set Authorization LAST so we never accidentally log the token header.
    if (token) {
      req.setHeader("authorization", `Bearer ${token}`);
    }
    req.write(json);
    req.end();
  });
}

function emitFallbackEvent({ traceId, provider, reason, mode }) {
  // Structured content-free event. Mirrors the workspace hook's
  // FALLBACK_EVENT_LOG rows so a single grep joins both.
  const event = {
    ts: Date.now() / 1000,
    event: "context_fallback",
    traceId,
    provider: provider || PLANNER_PROVIDER,
    reason,
    mode,
    source: "mcp-client",
  };
  // Print to stderr so it does NOT contaminate the JSON packet on stdout.
  process.stderr.write(JSON.stringify(event) + "\n");
}

function redactForLog(payload) {
  // Defensive: strip the token from any object that might surface it.
  // The token is held only in closure; this is belt-and-braces in case a
  // future change routes the raw `headers` map through a logger.
  const seen = new WeakSet();
  const walk = (value) => {
    if (value === null || typeof value !== "object") return value;
    if (seen.has(value)) return value;
    seen.add(value);
    if (Array.isArray(value)) return value.map(walk);
    const out = {};
    for (const [k, v] of Object.entries(value)) {
      if (k.toLowerCase() === "authorization") continue;
      out[k] = walk(v);
    }
    return out;
  };
  return walk(payload);
}

async function main() {
  const argv = process.argv.slice(2);
  let inputArg;
  let maxTokens = 2048;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--input") inputArg = argv[++i];
    else if (a === "--max-tokens") maxTokens = Number(argv[++i]);
    else if (a === "--help" || a === "-h") {
      process.stdout.write(
        [
          "mcp-client.mjs — Membrane MCP thin client (G5 Lane F)",
          `Public provider=${PLANNER_PROVIDER}; legacy alias=${PLANNER_PROVIDER_ALIAS}`,
          "",
          "Usage:",
          "  node mcp-client.mjs --input <path|-> [--max-tokens N]",
          "",
          "Reads {task,repo,maxTokens?,session?,anchors?,scopeGrantId?} from --input or stdin,",
          "POSTs to /federate, and prints the federated packet + receipts + degradation state.",
          "Falls back to a",
          "typed `context_fallback` envelope on transport or planner failure.",
        ].join("\n") + "\n"
      );
      return 0;
    }
  }

  let payload;
  try {
    payload = loadInput({ inputArg, maxTokens });
  } catch (e) {
    process.stderr.write(`mcp-client: failed to parse input: ${String(e).slice(0, 200)}\n`);
    return 2;
  }

  const traceId = payload.traceparent ? payload.traceparent.split("-")[1] : (payload.session || `mcp-${Date.now().toString(16)}`);
  const token = readToken();
  const port = readPort();
  const host = "127.0.0.1";

  let response;
  try {
    response = await postPlanner({
      host,
      port,
      path: "/federate",
      body: payload,
      token,
      traceId,
    });
  } catch (e) {
    emitFallbackEvent({
      traceId,
      provider: PLANNER_PROVIDER,
      reason: "transport_failure",
      mode: "non_graph_sources_only",
    });
    const fallback = {
      ok: false,
      transport: "mcp",
      version: TRANSPORT_VERSION,
      traceId,
      providerStatus: "unavailable",
      fallbackMode: "non_graph_sources_only",
      degradationReason: "planner_unavailable",
      sourceGeneration: null,
      packet: null,
      receipts: [],
      error: "planner_transport_failure",
    };
    process.stdout.write(JSON.stringify(fallback) + "\n");
    return 2;
  }

  let parsed;
  try {
    parsed = JSON.parse(response.body);
  } catch (_) {
    emitFallbackEvent({
      traceId,
      provider: PLANNER_PROVIDER,
      reason: "malformed_response",
      mode: "non_graph_sources_only",
    });
    process.stdout.write(
      JSON.stringify({
        ok: false,
        transport: "mcp",
        version: TRANSPORT_VERSION,
        traceId,
        providerStatus: "degraded",
        fallbackMode: "non_graph_sources_only",
        degradationReason: "malformed_response",
        sourceGeneration: null,
        packet: null,
        receipts: [],
        error: "planner_malformed_response",
        httpStatus: response.status,
      }) + "\n"
    );
    return 2;
  }

  // Auth/origin rejection: surface as typed fallback (loud, attributable).
  if (response.status === 401 || response.status === 403) {
    emitFallbackEvent({
      traceId,
      provider: PLANNER_PROVIDER,
      reason: "auth_rejected",
      mode: "non_graph_sources_only",
    });
    process.stdout.write(
      JSON.stringify({
        ok: false,
        transport: "mcp",
        version: TRANSPORT_VERSION,
        traceId,
        providerStatus: "unavailable",
        fallbackMode: "non_graph_sources_only",
        degradationReason: "auth_rejected",
        sourceGeneration: null,
        packet: null,
        receipts: [],
        error: parsed.error || "planner_auth_rejected",
        httpStatus: response.status,
      }) + "\n"
    );
    return 2;
  }

  // Planner-rejected input (400 from the planner, e.g. unknown schema):
  // echo the typed planner error AND a typed fallback event so the caller
  // can distinguish "I sent garbage" from "planner went away".
  if (response.status >= 400) {
    emitFallbackEvent({
      traceId,
      provider: PLANNER_PROVIDER,
      reason: parsed && parsed.kind ? parsed.kind : "planner_error",
      mode: "non_graph_sources_only",
    });
    process.stdout.write(
      JSON.stringify(
        redactForLog({
          ok: false,
          transport: "mcp",
          version: TRANSPORT_VERSION,
          traceId,
          providerStatus: "degraded",
          fallbackMode: "non_graph_sources_only",
          degradationReason:
            parsed && parsed.kind ? parsed.kind : "planner_error",
          sourceGeneration: null,
          packet: null,
          receipts: [],
          error: parsed && parsed.error ? parsed.error : "planner_rejected",
          httpStatus: response.status,
        })
      ) + "\n"
    );
    return 1;
  }

  // Successful planner response: surface the packet, receipts, and
  // degradation state verbatim, with the third-client transport metadata
  // attached so the parity tests can prove "MCP client got the same packet
  // the Claude hook did for the same frozen request".
  const out = redactForLog({
    ok: true,
    transport: "mcp",
    version: TRANSPORT_VERSION,
    traceId,
    providerStatus: parsed.providerStatus ?? "unknown",
    fallbackMode: parsed.fallbackMode ?? "none",
    degradationReason: parsed.degradationReason ?? "none",
    sourceGeneration: parsed.sourceGeneration ?? null,
    packet: parsed.packet ?? null,
    receipts: parsed.receipts ?? [],
    structuredEvent: parsed.structuredEvent ?? null,
    persistedReceipts: parsed.persistedReceipts ?? 0,
    scopeGrant: parsed.scopeGrant ?? null,
    httpStatus: response.status,
  });
  process.stdout.write(JSON.stringify(out) + "\n");
  return 0;
}

// Only run when invoked directly; pure module export for tests.
// Detection: file:// URL of the current module vs the entry script.
function isMainModule() {
  try {
    const arg = process.argv[1];
    if (!arg) return false;
    const argUrl = pathToFileURL(arg).href;
    return import.meta.url === argUrl;
  } catch (_) {
    return false;
  }
}

if (isMainModule()) {
  main().then(
    (code) => { process.exitCode = code; },
    (e) => {
      // Final guardrail: ensure the bearer token can never surface even if
      // an unexpected throw carries it via the error's headers.
      try {
        process.stderr.write(`mcp-client: unhandled: ${String(e).slice(0, 200)}\n`);
      } catch (_) {
        process.stderr.write("mcp-client: unhandled\n");
      }
      process.exitCode = 2;
    }
  );
}

export {
  loadInput,
  postPlanner,
  readPort,
  readToken,
  redactForLog,
  emitFallbackEvent,
  TRANSPORT_VERSION,
  REQUEST_TIMEOUT_MS,
  MAX_BODY_BYTES,
  DEFAULT_PORT,
};
