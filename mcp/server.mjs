#!/usr/bin/env node
// Public Membrane MCP adapter. It deliberately exposes no raw memory CRUD surface.

import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { bindingFor } from "./project-registry.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const CLIENT = join(HERE, "client.mjs");
const PROTOCOL_URI = "membrane://protocol/v1";
const TOOLS = [
  { name: "membrane_context", description: "Federated context packet for one task/repository binding.", inputSchema: { type: "object", required: ["task", "repository"], properties: { task: { type: "string" }, repository: { type: "string" }, budget: { type: "integer", minimum: 1 }, intent: { type: "string" }, session: { type: "string" }, anchors: { type: "string" }, scopeGrantId: { type: "string" } } } },
  { name: "membrane_source_read", description: "Hash-bound DocReadV1 section fetch.", inputSchema: { type: "object", required: ["sourceRef", "anchorId", "expectedContentHash"], properties: { sourceRef: { type: "string" }, anchorId: { type: "string" }, expectedContentHash: { type: "string" } } } },
  { name: "membrane_knowledge_propose", description: "Submit a typed KnowledgeEmission proposal for normal admission.", inputSchema: { type: "object", required: ["emission"], properties: { emission: { type: "object" } } } },
  { name: "membrane_checkpoint_save", description: "Save an A0 session checkpoint; never durable knowledge.", inputSchema: { type: "object", required: ["checkpoint"], properties: { checkpoint: { type: "object" } } } },
  { name: "membrane_checkpoint_load", description: "Load an unexpired A0 session checkpoint.", inputSchema: { type: "object", required: ["id"], properties: { id: { type: "string" }, asOfMs: { type: "integer" } } } },
  { name: "membrane_feedback", description: "Record receipt-bound outcome feedback.", inputSchema: { type: "object", required: ["receiptId", "outcome"], properties: { receiptId: { type: "string" }, outcome: { type: "string", enum: ["used", "ignored", "contradicted"] } } } },
];

const protocol = `# Membrane MCP v1\n\nContext calls federation, not raw recall. Knowledge is proposed, never directly put. Checkpoints are A0 session orientation state. Source reads require a hash-bound DocReadV1 reference.`;

function rpcResult(id, result) { return { jsonrpc: "2.0", id, result }; }
function rpcError(id, code, message) { return { jsonrpc: "2.0", id, error: { code, message } }; }
function text(value) { return { content: [{ type: "text", text: typeof value === "string" ? value : JSON.stringify(value) }] }; }

function run(command, args, input) {
  return new Promise((resolve) => {
    const child = spawn(command, args, { windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
    let stdout = "", stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", (error) => resolve({ code: 127, stdout, stderr: error.message }));
    child.on("close", (code) => resolve({ code: code ?? 1, stdout, stderr }));
    child.stdin.end(input);
  });
}

function memrightArgs(args) { return ["--db", process.env.MEMRIGHT_DB || "", ...args].filter((v, i) => !(i === 1 && !v)); }
async function callTool(name, args) {
  if (name === "membrane_context") {
    const binding = await bindingFor(args.repository);
    const request = { task: args.task, repo: binding.repository_id, maxTokens: args.budget, intent: args.intent, session: args.session, anchors: args.anchors, scopeGrantId: args.scopeGrantId };
    const out = await run(process.execPath, [CLIENT, "--input", "-"], JSON.stringify(request));
    return text(out.stdout.trim() || { status: "unavailable", error: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_source_read") {
    const out = await run(process.env.MEMRIGHT_BIN || "memright", memrightArgs(["doc", "read", "--source-ref", args.sourceRef, "--anchor", args.anchorId, "--expected-hash", args.expectedContentHash]), "");
    return text(out.stdout.trim() || { error: "source_read_unavailable", detail: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_checkpoint_save") {
    const out = await run(process.env.MEMRIGHT_BIN || "memright", memrightArgs(["checkpoint", "save"]), JSON.stringify(args.checkpoint));
    return text(out.stdout.trim() || { error: "checkpoint_save_unavailable", detail: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_checkpoint_load") {
    const params = ["checkpoint", "load", args.id];
    if (Number.isInteger(args.asOfMs)) params.push("--as-of-ms", String(args.asOfMs));
    const out = await run(process.env.MEMRIGHT_BIN || "memright", memrightArgs(params), "");
    return text(out.stdout.trim() || { error: "checkpoint_load_unavailable", detail: out.stderr.slice(0, 240) });
  }
  if (name === "membrane_knowledge_propose") return text({ status: "needs_review", emission: args.emission });
  if (name === "membrane_feedback") return text({ status: "needs_review", receiptId: args.receiptId, outcome: args.outcome });
  throw new Error("unknown tool");
}

async function handle(message) {
  const { id, method, params = {} } = message;
  if (method === "initialize") return rpcResult(id, { protocolVersion: "2025-03-26", capabilities: { tools: {}, resources: {} }, serverInfo: { name: "membrane", version: "1.0.0" }, instructions: "Use membrane_context for bounded federated context. Never expect raw memory CRUD." });
  if (method === "tools/list") return rpcResult(id, { tools: TOOLS });
  if (method === "resources/list") return rpcResult(id, { resources: [{ uri: PROTOCOL_URI, name: "Membrane protocol v1", mimeType: "text/markdown" }] });
  if (method === "resources/read" && params.uri === PROTOCOL_URI) return rpcResult(id, { contents: [{ uri: PROTOCOL_URI, mimeType: "text/markdown", text: protocol }] });
  if (method === "tools/call") return rpcResult(id, await callTool(params.name, params.arguments || {}));
  if (id === undefined) return null;
  return rpcError(id, -32601, "method not found");
}

const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of lines) {
  try { const response = await handle(JSON.parse(line)); if (response) process.stdout.write(JSON.stringify(response) + "\n"); }
  catch (error) { process.stdout.write(JSON.stringify(rpcError(null, -32603, String(error.message || error))) + "\n"); }
}
