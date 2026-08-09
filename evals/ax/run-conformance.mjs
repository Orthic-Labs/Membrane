#!/usr/bin/env node
// CX-B7 MCP conformance runner: spawns the real scripts/cortex-mcp.mjs server over
// stdio and speaks MCP JSON-RPC directly — initialize, tools/list, resources/list,
// prompts/list, a read-only capability probe, then per-tool probes (valid args,
// missing required, unknown field). Results must be schema-valid or typed stable-code
// errors; stack traces fail. structuredContent is validated against advertised
// outputSchema (self-contained subset validator); missing outputSchema/
// structuredContent/annotations stay skipped-pending. Any probe or check failure
// (incl. outputSchema validation) forces row, totals, and verdict to fail, exit 1.
// Artifacts: .audit/cx-b7/capabilities.json, .audit/cx-b7/conformance-report.json

import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const RUNNER_PATH = fileURLToPath(import.meta.url);
const REPO_ROOT = resolve(dirname(RUNNER_PATH), "..", "..");
const SERVER_SCRIPT = "scripts/cortex-mcp.mjs";
const PROTOCOL_VERSION = "2025-03-26";
const DEFAULT_TIMEOUT_MS = 60000;
const OMIT = Symbol("omit");
const PROBE_KINDS = ["valid-args", "missing-required", "unknown-field"];

function parseArgs(argv) {
  const args = { root: ".", timeoutMs: DEFAULT_TIMEOUT_MS };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === "--root") args.root = argv[index + 1];
    else if (token.startsWith("--root=")) args.root = token.slice("--root=".length);
    else if (token === "--timeout-ms") args.timeoutMs = Number(argv[index + 1]);
    else if (token.startsWith("--timeout-ms=")) args.timeoutMs = Number(token.slice("--timeout-ms=".length));
  }
  return args;
}

// --- minimal JSON-RPC client over newline-delimited stdio -------------------

function createRpcClient(child, { timeoutMs, onUnsolicited }) {
  let buffer = "";
  let nextId = 0;
  const pending = new Map();
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    buffer += chunk;
    let index;
    while ((index = buffer.indexOf("\n")) !== -1) {
      const line = buffer.slice(0, index).trim();
      buffer = buffer.slice(index + 1);
      if (!line) continue;
      let message;
      try { message = JSON.parse(line); } catch { onUnsolicited?.({ kind: "non-json", line: line.slice(0, 200) }); continue; }
      if (message && message.id !== undefined && pending.has(message.id)) {
        const entry = pending.get(message.id);
        pending.delete(message.id);
        clearTimeout(entry.timer);
        entry.resolve(message);
      } else onUnsolicited?.(message);
    }
  });
  return {
    request(method, params) {
      const id = ++nextId;
      return new Promise((resolvePromise, rejectPromise) => {
        const timer = setTimeout(() => { pending.delete(id); rejectPromise(new Error(`request timed out after ${timeoutMs}ms: ${method}`)); }, timeoutMs);
        pending.set(id, { resolve: resolvePromise, timer });
        child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
      });
    },
    notify(method, params) {
      child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method, ...(params === undefined ? {} : { params }) }) + "\n");
    },
    close() {
      for (const entry of pending.values()) clearTimeout(entry.timer);
      pending.clear();
    },
  };
}

// --- raw stack trace detection ----------------------------------------------

function findStackTraces(text) {
  const hits = [];
  const lines = String(text ?? "").split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const next = lines[index + 1] ?? "";
    if (/^\s*at\s+/.test(line)) {
      hits.push({ line: index + 1, text: line.trim().slice(0, 200) });
    } else if (/^\s*(?:Error|TypeError|ReferenceError|RangeError|SyntaxError|ZodError|MCPError):/.test(line) && /^\s*at\s+/.test(next)) {
      hits.push({ line: index + 1, text: line.trim().slice(0, 200) });
    } else if (/^\s*node:internal\//.test(line)) {
      hits.push({ line: index + 1, text: line.trim().slice(0, 200) });
    } else if (/^\s*(?:file:\/\/)?[A-Za-z]:[\\/][^:]+\.mjs:\d+:\d+\)?$/.test(line.trim())) {
      hits.push({ line: index + 1, text: line.trim().slice(0, 200) });
    }
  }
  return hits;
}

// --- response classification -------------------------------------------------

function tryParseJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function classifyResponse(message) {
  if (message?.error) return { kind: "jsonrpc-error", code: message.error.code === undefined || message.error.code === null ? "unknown" : String(message.error.code), message: String(message.error.message ?? "") };
  const result = message?.result ?? {};
  const texts = (result.content ?? []).filter((part) => part && part.type === "text").map((part) => String(part.text ?? ""));
  const joined = texts.join("\n");
  const stack = findStackTraces(joined);
  if (stack.length) return { kind: "stack-trace", stack };
  if (result.isError === true) {
    const typed = texts.map((text) => ({ text, parsed: tryParseJson(text) })).find(({ parsed }) => parsed && parsed.error && parsed.error.code !== undefined);
    if (typed) return { kind: "typed-error", code: String(typed.parsed.error.code), message: String(typed.parsed.error.message ?? ""), sample: typed.text.slice(0, 400) };
    const match = /^MCP error (-?\d+):/.exec(joined.trim());
    if (match) return { kind: "typed-error", code: match[1], message: joined.trim().slice(0, 400), sample: joined.trim().slice(0, 400) };
    return { kind: "untyped-error", sample: joined.slice(0, 500) };
  }
  return { kind: "success", structuredContent: result.structuredContent ?? null, sample: joined.slice(0, 300) };
}

// --- valid argument construction from the advertised inputSchema -------------

const SAFE_OPTIONALS = new Set(["task", "query", "limit", "depth", "budget"]);

function semanticValue(name, type, schema, anchor) {
  switch (name) {
    case "query":
    case "task": return "conformance";
    case "anchor": return anchor ?? "file:scripts/cortex-mcp.mjs";
    case "limit": return 5;
    case "depth": return 1;
    case "budget": return 2000;
    case "direction": return "both";
    case "claimId":
    case "kind":
    case "cursor":
    case "repoId":
    case "generation":
    case "allowStale":
      return OMIT;
  }
  if (type === "string") return "a";
  if (type === "integer" || type === "number") return Math.min(Math.max(typeof schema.minimum === "number" ? schema.minimum : 1, 1), typeof schema.maximum === "number" ? schema.maximum : 100);
  if (type === "boolean") return true;
  if (type === "object") return {};
  if (type === "array") return [];
  return OMIT;
}

function buildValidArgs(tool, anchor) {
  const properties = tool.inputSchema?.properties ?? {};
  const required = new Set(tool.inputSchema?.required ?? []);
  const args = {};
  for (const [name, schema] of Object.entries(properties)) {
    const type = Array.isArray(schema.type) ? schema.type[0] : schema.type;
    const value = semanticValue(name, type, schema, anchor);
    if (value === OMIT) continue;
    if (!required.has(name) && !SAFE_OPTIONALS.has(name)) continue;
    args[name] = value;
  }
  return args;
}

function omitRequired(args, tool) {
  const required = new Set(tool.inputSchema?.required ?? []);
  const next = { ...args };
  for (const key of required) delete next[key];
  return next;
}

// --- self-contained JSON-Schema (draft-07 subset) validator ------------------

function deepEqual(left, right) {
  if (left === right) return true;
  if (typeof left !== typeof right) return false;
  if (left === null || right === null) return false;
  if (Array.isArray(left) && Array.isArray(right)) return left.length === right.length && left.every((item, index) => deepEqual(item, right[index]));
  if (typeof left === "object" && typeof right === "object") return Object.keys(left).length === Object.keys(right).length && Object.keys(left).every((key) => deepEqual(left[key], right[key]));
  return false;
}

function matchesType(value, type) {
  switch (type) {
    case "string": return typeof value === "string";
    case "number": return typeof value === "number";
    case "integer": return typeof value === "number" && Number.isInteger(value);
    case "boolean": return typeof value === "boolean";
    case "null": return value === null;
    case "array": return Array.isArray(value);
    case "object": return value !== null && typeof value === "object" && !Array.isArray(value);
    default: return true;
  }
}

function validateAgainstSchema(value, schema) {
  const errors = [];
  const walk = (current, currentSchema, path) => {
    if (currentSchema === true) return;
    if (currentSchema === false) { errors.push(`${path}: schema forbids value`); return; }
    if (currentSchema.const !== undefined && !deepEqual(current, currentSchema.const)) errors.push(`${path}: expected const ${JSON.stringify(currentSchema.const)}`);
    if (currentSchema.enum !== undefined && !currentSchema.enum.some((entry) => deepEqual(current, entry))) errors.push(`${path}: value not in enum`);
    if (currentSchema.type !== undefined) {
      const types = Array.isArray(currentSchema.type) ? currentSchema.type : [currentSchema.type];
      if (!types.some((type) => matchesType(current, type))) { errors.push(`${path}: expected type ${types.join("|")}`); return; }
    }
    if (currentSchema.allOf) currentSchema.allOf.forEach((sub) => walk(current, sub, path));
    if (currentSchema.anyOf && !currentSchema.anyOf.some((sub) => validateAgainstSchema(current, sub).valid)) errors.push(`${path}: matches no anyOf branch`);
    if (currentSchema.oneOf) {
      const matched = currentSchema.oneOf.filter((sub) => validateAgainstSchema(current, sub).valid).length;
      if (matched !== 1) errors.push(`${path}: oneOf matched ${matched} branches`);
    }
    if (typeof current === "string") {
      if (currentSchema.minLength !== undefined && current.length < currentSchema.minLength) errors.push(`${path}: shorter than minLength ${currentSchema.minLength}`);
      if (currentSchema.maxLength !== undefined && current.length > currentSchema.maxLength) errors.push(`${path}: longer than maxLength ${currentSchema.maxLength}`);
      if (currentSchema.pattern !== undefined && !new RegExp(currentSchema.pattern).test(current)) errors.push(`${path}: pattern mismatch`);
    }
    if (typeof current === "number") {
      if (currentSchema.minimum !== undefined && current < currentSchema.minimum) errors.push(`${path}: below minimum ${currentSchema.minimum}`);
      if (currentSchema.maximum !== undefined && current > currentSchema.maximum) errors.push(`${path}: above maximum ${currentSchema.maximum}`);
      if (currentSchema.exclusiveMinimum !== undefined && current <= currentSchema.exclusiveMinimum) errors.push(`${path}: not above exclusiveMinimum`);
      if (currentSchema.exclusiveMaximum !== undefined && current >= currentSchema.exclusiveMaximum) errors.push(`${path}: not below exclusiveMaximum`);
    }
    if (Array.isArray(current)) {
      if (currentSchema.minItems !== undefined && current.length < currentSchema.minItems) errors.push(`${path}: fewer than minItems ${currentSchema.minItems}`);
      if (currentSchema.maxItems !== undefined && current.length > currentSchema.maxItems) errors.push(`${path}: more than maxItems ${currentSchema.maxItems}`);
      if (currentSchema.items) current.forEach((item, index) => walk(item, currentSchema.items, `${path}[${index}]`));
    }
    if (current !== null && typeof current === "object" && !Array.isArray(current)) {
      if (currentSchema.properties) Object.entries(currentSchema.properties).forEach(([key, sub]) => { if (current[key] !== undefined) walk(current[key], sub, `${path}.${key}`); });
      if (currentSchema.required) currentSchema.required.forEach((key) => { if (current[key] === undefined) errors.push(`${path}: missing required property ${key}`); });
      if (currentSchema.additionalProperties === false) Object.keys(current).forEach((key) => { if (!currentSchema.properties?.[key]) errors.push(`${path}: unexpected property ${key}`); });
    }
  };
  walk(value, schema, "$");
  return { valid: errors.length === 0, errors };
}

// --- probe and row aggregation -------------------------------------------------

function evaluateProbe(kind, classification, tool) {
  if (kind === "valid-args") {
    if (classification.kind === "success") return { status: "pass", detail: "schema-valid result" };
    if (classification.kind === "typed-error" || classification.kind === "jsonrpc-error") return { status: "pass", detail: `typed stable-code error ${classification.code}` };
    if (classification.kind === "stack-trace") return { status: "fail", detail: "raw stack trace in tool result" };
    return { status: "fail", detail: "untyped error", sample: classification.sample };
  }
  if (kind === "missing-required") {
    const required = (tool.inputSchema?.required ?? []).filter((key) => tool.inputSchema?.properties?.[key] !== undefined);
    if (required.length === 0) return { status: "skipped-pending", detail: "tool declares no required input properties" };
    if (classification.kind === "typed-error" || classification.kind === "jsonrpc-error") return { status: "pass", detail: `rejected with typed stable-code error ${classification.code}` };
    if (classification.kind === "success") return { status: "fail", detail: "server accepted call missing required arguments" };
    if (classification.kind === "stack-trace") return { status: "fail", detail: "raw stack trace in tool result" };
    return { status: "fail", detail: "untyped error", sample: classification.sample };
  }
  if (kind === "unknown-field") {
    if (classification.kind === "success") return { status: "pass", detail: "unknown field tolerated: success result (no typed rejection)" };
    if (classification.kind === "typed-error" || classification.kind === "jsonrpc-error") return { status: "pass", detail: `rejected with typed stable-code error ${classification.code}` };
    if (classification.kind === "stack-trace") return { status: "fail", detail: "raw stack trace in tool result" };
    return { status: "fail", detail: "untyped error", sample: classification.sample };
  }
  return { status: "fail", detail: `unknown probe kind ${kind}` };
}

function rowAggregate(row) {
  const items = [
    ...row.probes.map((probe) => ({ kind: `probe ${probe.kind}`, status: probe.status, reason: probe.detail ?? probe.sample ?? null })),
    ...Object.entries(row.checks).map(([name, check]) => ({
      kind: `check ${name}`,
      status: check.status,
      reason: check.reason ?? (check.errors?.length ? check.errors.join("; ") : null),
    })),
  ];
  const failed = items.filter((item) => item.status === "fail");
  if (failed.length) return { status: "fail", reasons: failed.map((item) => `${item.kind}: ${item.reason ?? "failed"}`) };
  const skipped = items.filter((item) => item.status === "skipped-pending");
  if (skipped.length) return { status: "skipped-pending", reasons: skipped.map((item) => `${item.kind}: ${item.reason ?? "skipped"}`) };
  return { status: "pass", reasons: [] };
}

// --- artifact writing ---------------------------------------------------------

function writeArtifacts(report) {
  const outDir = join(REPO_ROOT, ".audit", "cx-b7");
  mkdirSync(outDir, { recursive: true });
  const capabilities = {
    schemaVersion: 1,
    runner: report.runner,
    generatedAt: report.generatedAt,
    root: report.root,
    server: report.server,
    initialize: report.session.initialize,
    toolsAdvertisedCount: report.session.tools.length,
    tools: report.session.tools.map((tool) => ({
      name: tool.name,
      description: tool.description ?? null,
      inputSchema: tool.inputSchema ?? null,
      outputSchema: tool.outputSchema ?? null,
      outputSchemaAdvertised: Boolean(tool.outputSchema),
      annotations: tool.annotations ?? null,
      annotationsAdvertised: Boolean(tool.annotations),
      requiredInputs: tool.inputSchema?.required ?? [],
      readOnlyHint: tool.annotations?.readOnlyHint ?? null,
      destructiveHint: tool.annotations?.destructiveHint ?? null,
      idempotentHint: tool.annotations?.idempotentHint ?? null,
    })),
    toolsWithOutputSchemaCount: report.session.tools.filter((tool) => Boolean(tool.outputSchema)).length,
    toolsWithAnnotationsCount: report.session.tools.filter((tool) => Boolean(tool.annotations)).length,
    toolsWithStructuredContentCount: report.probes.filter((row) => row.probes.some((probe) => probe.structuredContent !== null && probe.structuredContent !== undefined)).length,
    resources: report.session.resources,
    prompts: report.session.prompts,
    capabilityProbe: report.session.capabilityProbe,
  };
  const reportPath = join(outDir, "conformance-report.json");
  const capabilitiesPath = join(outDir, "capabilities.json");
  writeFileSync(reportPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  writeFileSync(capabilitiesPath, `${JSON.stringify(capabilities, null, 2)}\n`, "utf8");
  return { reportPath, capabilitiesPath };
}

// --- main ---------------------------------------------------------------------

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const root = resolve(args.root);
  const startedAt = Date.now();
  const report = {
    schemaVersion: 1,
    suite: "cx-b7-conformance",
    runner: "evals/ax/run-conformance.mjs",
    generatedAt: new Date().toISOString(),
    root,
    repoRoot: REPO_ROOT,
    server: {
      command: "node",
      args: [SERVER_SCRIPT, "--root", root],
      cwd: REPO_ROOT,
      pid: null,
      exitCode: null,
      exitSignal: null,
      stderrExcerpt: "",
    },
    session: {
      initialize: null,
      tools: [],
      resources: null,
      prompts: null,
      capabilityProbe: null,
      unsolicited: [],
    },
    probes: [],
    rawStackTraces: [],
    checks: {},
    totals: { pass: 0, fail: 0, "skipped-pending": 0 },
    fatal: null,
    verdict: "fail",
    elapsedMs: 0,
  };

  let child = null;
  let rpc = null;
  try {
    child = spawn(process.execPath, [SERVER_SCRIPT, "--root", root], {
      cwd: REPO_ROOT,
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    report.server.pid = child.pid ?? null;
    let stderrText = "";
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk) => { stderrText += chunk; });
    const serverExit = new Promise((resolvePromise) => { child.on("exit", (code, signal) => resolvePromise({ code, signal })); });
    child.on("error", (error) => { report.fatal = { message: String(error?.message ?? error), stack: String(error?.stack ?? "") }; });

    rpc = createRpcClient(child, {
      timeoutMs: args.timeoutMs,
      onUnsolicited: (message) => report.session.unsolicited.push(message),
    });

    // initialize
    const initializeResponse = await rpc.request("initialize", {
      protocolVersion: PROTOCOL_VERSION,
      capabilities: {},
      clientInfo: { name: "cortex-conformance-runner", version: "1.0.0" },
    });
    if (initializeResponse.error) {
      report.session.initialize = { status: "fail", error: { code: initializeResponse.error.code, message: initializeResponse.error.message } };
    } else {
      report.session.initialize = { status: "pass", protocolVersion: initializeResponse.result?.protocolVersion ?? null, serverInfo: initializeResponse.result?.serverInfo ?? null, capabilities: initializeResponse.result?.capabilities ?? null };
    }
    rpc.notify("notifications/initialized");

    // tools/list
    const toolsResponse = await rpc.request("tools/list", {});
    if (toolsResponse.error) {
      report.fatal = { message: `tools/list failed: ${toolsResponse.error.code} ${toolsResponse.error.message}`, stack: "" };
    } else {
      report.session.tools = toolsResponse.result?.tools ?? [];
    }

    // resources/list and prompts/list (recorded even when not advertised)
    const resourcesResponse = await rpc.request("resources/list", {});
    report.session.resources = resourcesResponse.error ? { advertised: false, list: { status: "error", code: resourcesResponse.error.code, message: resourcesResponse.error.message } } : { advertised: true, list: { status: "ok", count: (resourcesResponse.result?.resources ?? []).length } };
    const promptsResponse = await rpc.request("prompts/list", {});
    report.session.prompts = promptsResponse.error ? { advertised: false, list: { status: "error", code: promptsResponse.error.code, message: promptsResponse.error.message } } : { advertised: true, list: { status: "ok", count: (promptsResponse.result?.prompts ?? []).length } };

    // capability probe via a read-only tool
    const readOnlyTool = report.session.tools.find((tool) => tool.name === "cortex_status")
      ?? report.session.tools.find((tool) => !(tool.inputSchema?.required ?? []).length)
      ?? report.session.tools[0];
    if (readOnlyTool) {
      const classification = classifyResponse(await rpc.request("tools/call", { name: readOnlyTool.name, arguments: {} }));
      const stack = classification.kind === "stack-trace" ? classification.stack : [];
      if (stack.length) report.rawStackTraces.push({ tool: readOnlyTool.name, probe: "capability-probe", hits: stack });
      report.session.capabilityProbe = { tool: readOnlyTool.name, status: classification.kind === "success" || classification.kind === "typed-error" || classification.kind === "jsonrpc-error" ? "pass" : "fail", resultKind: classification.kind, code: classification.code ?? null, detail: classification.kind === "success" ? "read-only tool call succeeded" : (classification.message ?? classification.sample ?? "") };
    }

    // resolve an anchor for expand/impact valid-args probes
    let anchor = null;
    if (report.session.tools.some((tool) => tool.name === "cortex_search")) {
      const classification = classifyResponse(await rpc.request("tools/call", { name: "cortex_search", arguments: { query: "cortex-mcp", limit: 5 } }));
      if (classification.kind === "success") {
        const results = tryParseJson(classification.sample)?.results ?? [];
        if (results.length) anchor = String(results[0].id ?? "");
      }
    }

    // per-tool probes: valid args, missing required args, unknown field
    for (const tool of report.session.tools) {
      const row = {
        tool: tool.name,
        description: tool.description ?? null,
        inputSchema: tool.inputSchema ?? null,
        outputSchema: tool.outputSchema ?? null,
        annotations: tool.annotations ?? null,
        probes: [],
        checks: {},
      };
      const validArgs = buildValidArgs(tool, anchor);
      const probes = [
        { kind: "valid-args", args: validArgs },
        { kind: "missing-required", args: omitRequired(validArgs, tool) },
        { kind: "unknown-field", args: { ...validArgs, __cxb7_unknown_field: "probe" } },
      ];
      for (const probe of probes) {
        const started = Date.now();
        let response;
        try {
          response = await rpc.request("tools/call", { name: tool.name, arguments: probe.args });
        } catch (error) {
          row.probes.push({ kind: probe.kind, args: probe.args, status: "fail", resultKind: "client-error", detail: String(error?.message ?? error), durationMs: Date.now() - started });
          continue;
        }
        const classification = classifyResponse(response);
        const outcome = evaluateProbe(probe.kind, classification, tool);
        const stack = classification.kind === "stack-trace" ? classification.stack : [];
        if (stack.length) report.rawStackTraces.push({ tool: tool.name, probe: probe.kind, hits: stack });
        row.probes.push({ kind: probe.kind, args: probe.args, status: outcome.status, resultKind: classification.kind, code: classification.code ?? null, structuredContent: classification.kind === "success" ? (classification.structuredContent ?? null) : null, stackTrace: stack, detail: outcome.detail, sample: outcome.sample ?? classification.sample ?? null, durationMs: Date.now() - started });
      }

      // honest checks for outputSchema / structuredContent / annotations
      if (!tool.outputSchema) {
        row.checks.structuredContentValidation = { status: "skipped-pending", reason: "tool advertises no outputSchema" };
      } else {
        const withStructured = row.probes.find((probe) => probe.structuredContent !== null && probe.structuredContent !== undefined);
        if (!withStructured) {
          row.checks.structuredContentValidation = { status: "skipped-pending", reason: "no tool call returned structuredContent" };
        } else {
          const validation = validateAgainstSchema(withStructured.structuredContent, tool.outputSchema);
          row.checks.structuredContentValidation = { status: validation.valid ? "pass" : "fail", errors: validation.errors, reason: validation.valid ? "structuredContent matches advertised outputSchema" : "structuredContent does not match advertised outputSchema" };
        }
      }
      row.checks.outputSchema = tool.outputSchema ? { status: "pass", schema: tool.outputSchema, reason: "outputSchema advertised and structurally present" } : { status: "skipped-pending", reason: "tool advertises no outputSchema" };
      row.checks.annotations = tool.annotations ? { status: "pass", annotations: tool.annotations, reason: "annotations advertised and structurally present" } : { status: "skipped-pending", reason: "tool advertises no annotations" };
      row.checks.rawStackTrace = row.probes.some((probe) => (probe.stackTrace ?? []).length) ? { status: "fail", reason: "raw stack trace in tool result" } : { status: "pass", reason: "no raw stack traces" };
      const aggregate = rowAggregate(row);
      row.status = aggregate.status;
      row.reasons = aggregate.reasons;
      report.probes.push(row);
    }

    // close the server cleanly
    rpc.close();
    child.stdin.end();
    const exit = await Promise.race([serverExit, new Promise((resolvePromise) => setTimeout(() => { child.kill(); resolvePromise({ code: null, signal: "killed-after-grace" }); }, 5000))]);
    report.server.exitCode = exit.code;
    report.server.exitSignal = exit.signal;
    report.server.stderrExcerpt = stderrText.slice(0, 4000);
    const stderrStack = findStackTraces(stderrText);
    if (stderrStack.length) report.rawStackTraces.push({ source: "stderr", hits: stderrStack });
  } catch (error) {
    report.fatal = { message: String(error?.message ?? error), stack: String(error?.stack ?? "") };
    try { rpc?.close(); } catch { /* ignore */ }
    try { child?.kill(); } catch { /* ignore */ }
  }

  // totals and run-level checks
  for (const row of report.probes) for (const probe of row.probes) report.totals[probe.status] = (report.totals[probe.status] ?? 0) + 1;
  const advertisedTools = report.session.tools.map((tool) => tool.name);
  const rowTools = report.probes.map((row) => row.tool);
  const checks = {
    toolsListCountMatchesRows: advertisedTools.length === rowTools.length && advertisedTools.every((name) => rowTools.includes(name)),
    everyToolRecordsAllProbes: report.probes.every((row) => PROBE_KINDS.every((kind) => row.probes.some((probe) => probe.kind === kind && probe.status !== undefined))),
    zeroRawStackTraces: report.rawStackTraces.length === 0,
    noRowFailed: report.probes.every((row) => row.status !== "fail"),
    initialize: report.session.initialize?.status === "pass",
    toolsList: !report.fatal,
    capabilityProbe: report.session.capabilityProbe?.status === "pass",
    resourcesList: report.session.resources?.list?.status === "error" ? "not-advertised" : report.session.resources?.list?.status,
    promptsList: report.session.prompts?.list?.status === "error" ? "not-advertised" : report.session.prompts?.list?.status,
  };
  report.checks = checks;
  report.elapsedMs = Date.now() - startedAt;
  report.verdict = !report.fatal && report.totals.fail === 0 && checks.toolsListCountMatchesRows && checks.everyToolRecordsAllProbes && checks.zeroRawStackTraces && checks.noRowFailed && checks.initialize ? "pass" : "fail";

  const artifacts = writeArtifacts(report);

  // human summary
  const exitCode = report.verdict === "pass" ? 0 : 1;
  console.log(`cx-b7 conformance: root=${root} server=node ${SERVER_SCRIPT}`);
  console.log(`initialize: ${report.session.initialize?.status ?? "n/a"} (protocol ${report.session.initialize?.protocolVersion ?? "-"})`);
  console.log(`tools/list: ${advertisedTools.length} tools`);
  console.log(`resources/list: ${report.session.resources?.list?.status ?? "n/a"}${report.session.resources?.list?.code !== undefined ? ` (${report.session.resources.list.code})` : ""}`);
  console.log(`prompts/list: ${report.session.prompts?.list?.status ?? "n/a"}${report.session.prompts?.list?.code !== undefined ? ` (${report.session.prompts.list.code})` : ""}`);
  console.log(`capability probe: ${report.session.capabilityProbe?.tool ?? "-"} ${report.session.capabilityProbe?.status ?? "n/a"} (${report.session.capabilityProbe?.resultKind ?? "-"})`);
  for (const row of report.probes) console.log(`  ${row.tool}: ${PROBE_KINDS.map((kind) => `${kind}=${row.probes.find((entry) => entry.kind === kind)?.status ?? "missing"}`).join(" ")} status=${row.status}`);
  console.log(`totals: pass=${report.totals.pass} fail=${report.totals.fail} skipped-pending=${report.totals["skipped-pending"]}`);
  console.log(`checks: toolsListCountMatchesRows=${checks.toolsListCountMatchesRows} everyToolRecordsAllProbes=${checks.everyToolRecordsAllProbes} zeroRawStackTraces=${checks.zeroRawStackTraces} noRowFailed=${checks.noRowFailed}`);
  if (report.fatal) console.log(`fatal: ${report.fatal.message}`);
  console.log(`verdict: ${report.verdict.toUpperCase()}`);
  console.log(`artifacts: ${artifacts.reportPath}`);
  console.log(`artifacts: ${artifacts.capabilitiesPath}`);
  console.log(`exit: ${exitCode}`);
  process.exitCode = exitCode;
}

main().catch((error) => {
  console.error(`conformance runner crashed: ${error?.stack ?? error}`);
  process.exitCode = 1;
});
