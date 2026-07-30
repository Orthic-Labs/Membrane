#!/usr/bin/env node
// Public Membrane MCP adapter. It deliberately exposes no raw memory CRUD surface.

import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { createHash } from "node:crypto";
import { McpServer, fromJsonSchema } from "@modelcontextprotocol/server";
import { serveStdio } from "@modelcontextprotocol/server/stdio";
import { bindingFor } from "./project-registry.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const CLIENT = join(HERE, "client.mjs");
const PROTOCOL_URI = "membrane://protocol/v1";
const MAX_REQUEST_BYTES = 32 * 1024;
const MAX_PROPOSAL_BYTES = 16 * 1024;
const MAX_FEEDBACK_BYTES = 2 * 1024;
const RATE_WINDOW_MS = 60_000;
const RATE_LIMITS = { proposal: 12, checkpoint: 24, feedback: 48 };
const rateWindows = new Map();
const CALLER_SCHEMA = {
  type: "object",
  required: ["root", "repositoryId", "scopeId"],
  properties: {
    root: { type: "string", minLength: 1 },
    repositoryId: { type: "string", minLength: 1 },
    scopeId: { type: "string", minLength: 1 },
    scopeDescriptor: { type: "object" },
  },
  additionalProperties: false,
};
const TOOLS = [
  { name: "membrane_context", description: "Federated context packet for one exact caller binding.", inputSchema: { type: "object", required: ["task", "repository", "caller"], properties: { task: { type: "string", minLength: 1, pattern: "\\S" }, repository: { type: "string" }, caller: CALLER_SCHEMA, budget: { type: "integer", minimum: 1 }, intent: { type: "string" }, session: { type: "string" }, anchors: { type: "string" }, scopeGrantId: { type: "string" } } } },
  { name: "membrane_source_read", description: "Hash-bound DocReadV1 section fetch for one exact caller binding.", inputSchema: { type: "object", required: ["repository", "caller", "sourceRef", "anchorId", "expectedContentHash"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, sourceRef: { type: "string" }, anchorId: { type: "string" }, expectedContentHash: { type: "string" } } } },
  { name: "membrane_knowledge_propose", description: "Submit a bounded typed KnowledgeEmission proposal for quarantine review.", inputSchema: { type: "object", required: ["repository", "caller", "emission"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, emission: { type: "object" } } } },
  { name: "membrane_checkpoint_save", description: "Save an A0 session checkpoint for one exact caller binding; never durable knowledge.", inputSchema: { type: "object", required: ["repository", "caller", "checkpoint"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, checkpoint: { type: "object" } } } },
  { name: "membrane_checkpoint_load", description: "Load an unexpired A0 session checkpoint for one exact caller binding.", inputSchema: { type: "object", required: ["repository", "caller", "id"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, id: { type: "string" }, asOfMs: { type: "integer", minimum: 0 } } } },
  { name: "membrane_feedback", description: "Record bounded receipt-bound outcome feedback for quarantine review.", inputSchema: { type: "object", required: ["repository", "caller", "receiptId", "outcome"], properties: { repository: { type: "string" }, caller: CALLER_SCHEMA, receiptId: { type: "string" }, outcome: { type: "string", enum: ["used", "ignored", "contradicted"] } } } },
];

const TRACE_FIELDS = {
  traceparent: { type: "string", maxLength: 128 },
  tracestate: { type: "string", maxLength: 512 },
  baggage: { type: "string", maxLength: 8192 },
};
const TOOL_OUTPUT_SCHEMA = {
  type: "object",
  required: ["data", "trace"],
  properties: {
    data: {},
    trace: {
      type: "object",
      properties: TRACE_FIELDS,
      additionalProperties: false,
    },
  },
  additionalProperties: false,
};

const protocol = `# Membrane MCP v1\n\nmembrane_context routes through the loopback /federate endpoint, never raw recall. Knowledge is proposed, never directly put. Checkpoints are A0 session orientation state. Source reads require a hash-bound DocReadV1 reference.`;

function text(value) {
  if (typeof value !== "string") return value;
  try { return JSON.parse(value); }
  catch (_) { return value; }
}
function byteLength(value) { return Buffer.byteLength(JSON.stringify(value), "utf8"); }
function bounded(value, limit, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label} must be an object`);
  if (byteLength(value) > limit) throw new Error(`${label} exceeds ${limit} bytes`);
}
function receiptId(prefix, value) { return `${prefix}-${createHash("sha256").update(JSON.stringify(value)).digest("hex").slice(0, 24)}`; }
function callerLevel(binding) { return binding.grant_policy?.level || "read-only"; }
function callerDescriptor(caller) { return caller.scopeDescriptor || { kind: "filesystem", path: caller.scopeId }; }
function stableDescriptor(value) {
  if (Array.isArray(value)) return value.map(stableDescriptor);
  if (value && typeof value === "object") return Object.fromEntries(Object.keys(value).sort().map((key) => [key, stableDescriptor(value[key])]));
  return value;
}
function sameDescriptor(left, right) { return JSON.stringify(stableDescriptor(left)) === JSON.stringify(stableDescriptor(right)); }
function permits(binding, action) {
  const level = callerLevel(binding);
  if (["context", "source_read", "checkpoint_load"].includes(action)) return ["read-only", "write-proposed", "write-trusted", "admin"].includes(level);
  return ["write-proposed", "write-trusted", "admin"].includes(level);
}
function takeRate(binding, action) {
  const limit = RATE_LIMITS[action];
  if (!limit) return;
  const key = `${binding.root}:${binding.scope_id}:${action}`;
  const now = Date.now();
  const hits = (rateWindows.get(key) || []).filter((at) => now - at < RATE_WINDOW_MS);
  if (hits.length >= limit) throw new Error(`${action}_rate_limited`);
  hits.push(now);
  rateWindows.set(key, hits);
}
async function authorize(args, action) {
  if (!args || typeof args !== "object" || byteLength(args) > MAX_REQUEST_BYTES) throw new Error("request exceeds bounded 32768-byte limit");
  if (typeof args.repository !== "string" || !args.repository.trim()) throw new Error("repository is required");
  const binding = await bindingFor(args.repository);
  const caller = args.caller;
  if (!caller || typeof caller !== "object" || Array.isArray(caller)) throw new Error("caller binding is required");
  if (typeof caller.root !== "string" || !caller.root.trim()) throw new Error("caller root is required");
  const callerBinding = await bindingFor(caller.root);
  if (binding.root !== callerBinding.root || binding.repository_id !== callerBinding.repository_id || !sameDescriptor(binding.scope_descriptor, callerBinding.scope_descriptor)) throw new Error("cross_root_binding_denied");
  if (caller.repositoryId !== binding.repository_id || caller.scopeId !== binding.scope_id || !sameDescriptor(callerDescriptor(caller), binding.scope_descriptor)) throw new Error("caller_scope_binding_denied");
  if (!permits(binding, action)) throw new Error("caller_not_authorized");
  return binding;
}

function run(command, args, input, env = process.env) {
  return new Promise((resolve) => {
    const child = spawn(command, args, { windowsHide: true, stdio: ["pipe", "pipe", "pipe"], env });
    let stdout = "", stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", (error) => resolve({ code: 127, stdout, stderr: error.message }));
    child.on("close", (code) => resolve({ code: code ?? 1, stdout, stderr }));
    child.stdin.end(input);
  });
}

function memrightArgs(args) { return ["--db", process.env.MEMRIGHT_DB || "", ...args].filter((v, i) => !(i === 1 && !v)); }
async function callTool(name, args, trace = {}) {
  if (name === "membrane_context") {
    const binding = await authorize(args, "context");
    const request = { task: args.task, repo: binding.root, maxTokens: args.budget, intent: args.intent, session: args.session, anchors: args.anchors, scopeGrantId: args.scopeGrantId, scopeDescriptor: binding.scope_descriptor, ...trace };
    const out = await run(process.execPath, [CLIENT, "--input", "-"], JSON.stringify(request), { ...process.env, WORKSPACE_ROOT: binding.root });
    return text(out.stdout.trim() || { status: "unavailable", error: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_source_read") {
    await authorize(args, "source_read");
    const out = await run(process.env.MEMRIGHT_BIN || "memright", memrightArgs(["doc", "read", "--source-ref", args.sourceRef, "--anchor", args.anchorId, "--expected-hash", args.expectedContentHash]), "");
    return text(out.stdout.trim() || { error: "source_read_unavailable", detail: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_checkpoint_save") {
    const binding = await authorize(args, "checkpoint");
    bounded(args.checkpoint, MAX_PROPOSAL_BYTES, "checkpoint");
    takeRate(binding, "checkpoint");
    const out = await run(process.env.MEMRIGHT_BIN || "memright", memrightArgs(["checkpoint", "save"]), JSON.stringify(args.checkpoint));
    return text(out.stdout.trim() || { error: "checkpoint_save_unavailable", detail: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_checkpoint_load") {
    await authorize(args, "checkpoint_load");
    const params = ["checkpoint", "load", args.id];
    if (Number.isInteger(args.asOfMs)) params.push("--as-of-ms", String(args.asOfMs));
    const out = await run(process.env.MEMRIGHT_BIN || "memright", memrightArgs(params), "");
    return text(out.stdout.trim() || { error: "checkpoint_load_unavailable", detail: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_knowledge_propose") {
    const binding = await authorize(args, "proposal");
    bounded(args.emission, MAX_PROPOSAL_BYTES, "emission");
    takeRate(binding, "proposal");
    const proposalId = receiptId("proposal", { scope: binding.scope_id, emission: args.emission });
    return text({ status: "quarantined", proposalId, provenance: { repositoryId: binding.repository_id, scopeId: binding.scope_id, callerLevel: callerLevel(binding) } });
  }
  if (name === "membrane_feedback") {
    const binding = await authorize(args, "feedback");
    if (typeof args.receiptId !== "string" || !args.receiptId.trim() || !["used", "ignored", "contradicted"].includes(args.outcome)) throw new Error("invalid_feedback");
    if (byteLength({ receiptId: args.receiptId, outcome: args.outcome }) > MAX_FEEDBACK_BYTES) throw new Error("feedback exceeds 2048 bytes");
    takeRate(binding, "feedback");
    const feedbackId = receiptId("feedback", { scope: binding.scope_id, receiptId: args.receiptId, outcome: args.outcome });
    return text({ status: "accepted", feedbackId, receiptId: args.receiptId, outcome: args.outcome, provenance: { repositoryId: binding.repository_id, scopeId: binding.scope_id, callerLevel: callerLevel(binding) } });
  }
  throw new Error("unknown tool");
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
function boundedTrace(args = {}) {
  const trace = {};
  const hasTraceparent = validTraceparent(args.traceparent);
  if (hasTraceparent) trace.traceparent = args.traceparent;
  if (hasTraceparent && validTracestate(args.tracestate)) trace.tracestate = args.tracestate;
  if (validBaggage(args.baggage)) trace.baggage = args.baggage;
  return trace;
}
function structuredResult(data, trace = {}) {
  const structuredContent = { data, trace: boundedTrace(trace) };
  return {
    content: [{ type: "text", text: typeof data === "string" ? data : JSON.stringify(data) }],
    structuredContent,
    isError: false,
  };
}
export function buildServer() {
  const server = new McpServer(
    { name: "membrane", version: "1.0.0" },
    { instructions: "Use membrane_context for federated context through /federate. Never expect raw memory CRUD." },
  );
  for (const tool of TOOLS) {
    server.registerTool(tool.name, {
      description: tool.description,
      inputSchema: fromJsonSchema(tool.inputSchema),
      outputSchema: fromJsonSchema(TOOL_OUTPUT_SCHEMA),
    }, async (args, ctx) => {
      const trace = boundedTrace(ctx.mcpReq._meta);
      return structuredResult(await callTool(tool.name, args, trace), trace);
    });
  }
  server.registerResource("Membrane protocol v1", PROTOCOL_URI, { mimeType: "text/markdown" }, async (uri) => {
    return { contents: [{ uri: uri.href, mimeType: "text/markdown", text: protocol }] };
  });
  return server;
}

serveStdio(() => buildServer(), { onerror: (error) => process.stderr.write(`membrane-mcp: ${error.message}\n`) });
