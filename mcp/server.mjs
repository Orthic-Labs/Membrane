#!/usr/bin/env node
// Provider-neutral MCP stdio adapter for resident MemRight HTTP service.
// Content stays inside loopback responses; stderr is reserved for diagnostics.
import readline from "node:readline";

const port = Number(process.env.MEMRIGHT_PORT || 38123);
const base = `http://127.0.0.1:${port}`;

async function call(path, method = "GET", body) {
  const response = await fetch(`${base}${path}`, {
    method,
    headers: body ? { "content-type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await response.text();
  let value;
  try { value = JSON.parse(text); } catch { value = text; }
  if (!response.ok) throw new Error(typeof value === "string" ? value : JSON.stringify(value));
  return value;
}

const tools = [
  { name: "memright_recall", description: "Recall relevant durable memory previews.", inputSchema: { type: "object", properties: { query: { type: "string" }, k: { type: "integer" }, scope: { type: "string" } }, required: ["query"] } },
  { name: "memright_put", description: "Store or update one durable memory.", inputSchema: { type: "object", properties: { name: { type: "string" }, content: { type: "string" }, scope: { type: "string" }, tier: { type: "string" } }, required: ["name", "content"] } },
  { name: "memright_get", description: "Fetch one memory in full.", inputSchema: { type: "object", properties: { id: { type: "string" } }, required: ["id"] } },
  { name: "memright_health", description: "Read resident service health.", inputSchema: { type: "object", properties: {} } },
];

async function handle(request) {
  if (request.method === "initialize") return { protocolVersion: "2024-11-05", capabilities: { tools: {} }, serverInfo: { name: "memright", version: "0.1.1" } };
  if (request.method === "notifications/initialized") return null;
  if (request.method === "tools/list") return { tools };
  if (request.method !== "tools/call") throw new Error(`unsupported method: ${request.method}`);
  const { name, arguments: args = {} } = request.params || {};
  const value = name === "memright_recall"
    ? await call("/recall", "POST", { query: args.query, k: args.k ?? 6, scope: args.scope })
    : name === "memright_put"
      ? await call("/put", "POST", args)
      : name === "memright_get"
        ? await call("/get", "POST", args)
        : name === "memright_health" ? await call("/health") : (() => { throw new Error(`unknown tool: ${name}`); })();
  return { content: [{ type: "text", text: typeof value === "string" ? value : JSON.stringify(value) }], structuredContent: value };
}

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
input.on("line", async (line) => {
  if (!line.trim()) return;
  let request;
  try {
    request = JSON.parse(line);
    const result = await handle(request);
    if (request.id !== undefined) process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id: request.id, result })}\n`);
  } catch (error) {
    if (request?.id !== undefined) process.stdout.write(`${JSON.stringify({ jsonrpc: "2.0", id: request.id, error: { code: -32000, message: String(error.message || error) } })}\n`);
  }
});
