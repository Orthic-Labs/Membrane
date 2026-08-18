#!/usr/bin/env node
// CX-B1: MCP runtime adapter over the shared application service. The server
// root is bound at process start (--root, --repo-id, CORTEX_REPO_ROOTS); tool
// inputs never accept an unrestricted repoRoot. Exactly six frozen tools, nine
// URI-addressable resources (eight static plus cortex://effects), and six
// prompts. Every success returns redacted structuredContent plus identical
// text JSON; every failure returns isError with the stable typed envelope
// (code/message/details/retryable/remediation), redacted and stack-free.

import { existsSync, realpathSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { CallToolRequestSchema, McpError, ErrorCode } from "@modelcontextprotocol/sdk/types.js";
import { z } from "zod";

import { createCortexApplicationService } from "../src/lib/application/service.mjs";
import { RootRegistry } from "../src/lib/application/root-registry.mjs";
import { CortexError } from "../src/lib/application/errors.mjs";
import { redactForEgress } from "../src/lib/redaction.mjs";
import { closeStore, openStoreReadOnly } from "../src/graph/store-sqlite.mjs";
import { readIndexedMeta } from "../src/graph/traverse-store.mjs";
import { RESOURCE_URIS, resourceForUri } from "../src/mcp/resources.mjs";
import { PROMPTS, promptByName } from "../src/mcp/prompts.mjs";
import { TOOL_EFFECTS } from "../src/mcp/effects.mjs";

const OUT_DIR = ".agent";

function parseRootConfig() {
  const argv = process.argv.slice(2);
  const flags = new Map();
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === "--root") { flags.set("root", argv[index + 1]); index += 1; }
    else if (token === "--repo-id") { flags.set("repoId", argv[index + 1]); index += 1; }
    else if (token.startsWith("--root=")) flags.set("root", token.slice(7));
    else if (token.startsWith("--repo-id=")) flags.set("repoId", token.slice(10));
  }
  const envRoots = String(process.env.CORTEX_REPO_ROOTS ?? "").split(":").filter(Boolean);
  return { root: flags.get("root"), repoId: flags.get("repoId"), envRoots };
}

function buildService({ root, repoId, envRoots = [] } = {}) {
  const entries = [];
  if (root) entries.push({ root: resolve(root), repoId });
  for (const envRoot of envRoots) entries.push({ root: resolve(envRoot) });
  if (!entries.length) {
    throw new Error("cortex-mcp requires --root, --repo-id, or CORTEX_REPO_ROOTS");
  }
  const registry = new RootRegistry(entries);
  return createCortexApplicationService({ outDir: OUT_DIR, rootRegistry: registry, allowEmbeddedRoot: false });
}

// Common optional fields every tool accepts (D07 root confinement: repoRoot is
// deliberately absent; only the server-start root config and repoId select a
// repository).
const COMMON_FIELDS = {
  repoId: z.string().optional(),
  generation: z.string().optional(),
  allowStale: z.boolean().optional(),
};

// The SDK validates tool inputs before the handler runs and surfaces a
// rejection as a JSON-RPC protocol error (never a silent strip), so the
// advertised inputSchema is a strict zod object: unknown fields are rejected,
// the client sees a visible error, and the SDK's own message (which quotes
// the offending field) never enters a tool result's text surface. All known
// fields are typed; required fields are enforced per tool below.
const TOOL_DEFINITIONS = Object.freeze([
  Object.freeze({
    name: "cortex_orient",
    description: "Establish current repository and generation context.",
    strict: z.strictObject({ ...COMMON_FIELDS, task: z.string().optional(), query: z.string().optional(), limit: z.number().int().min(1).max(100).optional() }),
    outputKeys: ["schemaVersion", "action", "reasonCode", "generationId", "candidateSet", "freshnessReceipt", "omissions", "claimBoundary"],
    call: (service) => (input) => service.orient(input),
  }),
  Object.freeze({
    name: "cortex_search",
    description: "Search symbols, files, claims, routes, and concepts.",
    strict: z.strictObject({ ...COMMON_FIELDS, query: z.string().min(1), limit: z.number().int().min(1).max(100).optional() }),
    outputKeys: ["schemaVersion", "kind", "generationId", "provider", "query", "results", "omissions", "truncated", "continuationCursor", "freshnessReceipt"],
    call: (service) => (input) => service.search(input),
  }),
  Object.freeze({
    name: "cortex_expand",
    description: "Expand one anchor into a bounded evidence slice.",
    schemaVersion: 2,
    strict: z.strictObject({ ...COMMON_FIELDS, anchor: z.string().min(1), depth: z.number().int().min(1).max(5).optional(), budget: z.number().int().min(128).max(32000).optional(), cursor: z.string().optional() }),
    outputKeys: ["schemaVersion", "provider", "kind", "generationId", "sourceState", "dirtyFileCount", "root", "counts", "nodes", "edges", "truncated", "continuationCursor", "budget"],
    call: (service) => (input) => service.expand(input),
  }),
  Object.freeze({
    name: "cortex_impact",
    description: "Return upstream impact, tests, routes, schemas, and uncertainty.",
    schemaVersion: 2,
    strict: z.strictObject({ ...COMMON_FIELDS, anchor: z.string().min(1), depth: z.number().int().min(1).max(8).optional(), budget: z.number().int().min(128).max(32000).optional(), cursor: z.string().optional() }),
    outputKeys: ["schemaVersion", "provider", "kind", "generationId", "sourceState", "dirtyFileCount", "root", "counts", "target", "impacted", "edges", "truncated", "continuationCursor", "budget"],
    call: (service) => (input) => service.impact(input),
  }),
  Object.freeze({
    name: "cortex_doc_truth",
    description: "Return current, stale, contradicted, and unknown document claims.",
    strict: z.strictObject({ ...COMMON_FIELDS, claimId: z.string().optional(), kind: z.string().optional(), limit: z.number().int().min(1).max(1000).optional() }),
    outputKeys: ["schemaVersion", "generationId", "claims", "freshnessReceipt", "omissions", "truncated"],
    call: (service) => (input) => service.documentTruth(input),
  }),
  Object.freeze({
    name: "cortex_status",
    description: "Return freshness, coverage, service health, and repair actions.",
    strict: z.strictObject({ ...COMMON_FIELDS }),
    outputKeys: ["schemaVersion", "repository", "state", "manifestPath", "manifest", "providerMismatch", "manifestDigestValid", "ledger", "pendingPaths", "scanTruncated", "truncationReasons", "clocks", "capabilities", "claimBoundary"],
    optionalOutputKeys: ["manifest", "providerMismatch", "manifestDigestValid", "ledger", "pendingPaths", "scanTruncated", "truncationReasons", "clocks", "capabilities"],
    call: (service) => async (input) => {
      // service.status never opens a freshness session, so the adapter pins
      // the requested generation itself (CX-B1 common input contract).
      if (input.generation !== undefined) {
        const root = service.resolveCanonicalRoot(input);
        const observed = currentGenerationId(root);
        if (observed !== input.generation) {
          throw new CortexError("generation_mismatch", "Requested generation is not current.", { expected: input.generation, observed });
        }
      }
      return service.status(input);
    },
  }),
]);

// outputSchema advertises the live top-level shape of each operation response.
// All observed fields are required; claimBoundary is adapter-enriched.
function outputSchema(keys, schemaVersion = 1, optionalKeys = []) {
  const arrays = new Set(["results", "omissions", "nodes", "edges", "impacted", "claims", "pendingPaths", "truncationReasons"]);
  const booleans = new Set(["truncated", "providerMismatch", "manifestDigestValid", "scanTruncated", "ledger"]);
  const objects = new Set(["candidateSet", "freshnessReceipt", "counts", "target", "repository", "manifest", "clocks", "capabilities"]);
  return z.object(Object.fromEntries(keys.map((key) => {
    let schema = key === "schemaVersion" ? z.literal(schemaVersion)
      : arrays.has(key) ? z.array(z.unknown())
        : booleans.has(key) ? z.boolean()
          : objects.has(key) ? z.object({}).loose().nullable()
            : key === "dirtyFileCount" || key === "budget" ? z.number().int().nonnegative()
              : key === "continuationCursor" ? z.string().nullable()
                : z.unknown();
    if (key === "claimBoundary") schema = z.object({
      status: z.enum(["clear", "restricted"]), cleanClaimAllowed: z.boolean(),
      safeClaims: z.array(z.string()), prohibitedClaims: z.array(z.string()), gaps: z.array(z.string()),
    }).strict();
    if (optionalKeys.includes(key)) schema = schema.optional();
    return [key, schema];
  }))).loose();
}

function claimBoundaryFor(value) {
  if (value.claimBoundary) return value.claimBoundary;
  const receipt = value.freshnessReceipt;
  const state = value.state ?? (receipt?.barrierResult === "caught_up" ? "fresh" : receipt ? "stale" : "missing");
  const gaps = [...(value.omissions ?? []).map((item) => String(item?.reason ?? "")).filter(Boolean), ...(value.truncationReasons ?? []).map(String)];
  if (receipt?.eventGap) gaps.push("event_gap");
  if (receipt?.domainsPending?.length) gaps.push(...receipt.domainsPending.map((domain) => `pending:${domain}`));
  if (value.pendingPaths?.length) gaps.push("pending_paths");
  if (value.providerMismatch) gaps.push("provider_mismatch");
  if (value.manifestDigestValid === false) gaps.push("manifest_digest_invalid");
  if (value.scanTruncated) gaps.push("scan_truncated");
  const clean = state === "fresh" && !gaps.length;
  const generationId = value.generationId ?? value.manifest?.generationId ?? receipt?.generationId ?? null;
  return {
    status: clean ? "clear" : "restricted", cleanClaimAllowed: clean,
    safeClaims: clean ? ["Results reflect the current sealed generation."] : generationId ? [`Results reflect generation ${generationId}, which predates current worktree changes.`] : ["No sealed graph generation is available to ground claims."],
    prohibitedClaims: clean ? [] : ["Graph-derived facts are current."], gaps: [...new Set(gaps)],
  };
}

function enrichResult(tool, value) {
  return tool.name === "cortex_orient" || tool.name === "cortex_status" ? { ...value, claimBoundary: claimBoundaryFor(value) } : value;
}

function successResult(value) {
  const safe = redactForEgress(value);
  return {
    content: [{ type: "text", text: JSON.stringify(safe, null, 2) }],
    structuredContent: safe,
  };
}

function errorEnvelope(error) {
  const code = error?.code ?? "internal_error";
  const message = String(error?.message ?? error ?? "Unknown error");
  const rawRemediation = error?.remediation;
  const remediation = rawRemediation ? {
    summary: String(rawRemediation.summary ?? ""),
    nextOperation: rawRemediation.nextOperation == null ? null : String(rawRemediation.nextOperation),
    arguments: rawRemediation.arguments && typeof rawRemediation.arguments === "object" && !Array.isArray(rawRemediation.arguments) ? rawRemediation.arguments : {},
  } : null;
  return redactForEgress({
    schemaVersion: 1,
    error: {
      code,
      message,
      details: redactForEgress(error?.details ?? {}),
      retryable: Boolean(error?.retryable ?? false),
      remediation,
    },
  });
}

function failureResult(error) {
  return {
    isError: true,
    content: [{ type: "text", text: JSON.stringify(errorEnvelope(error), null, 2) }],
  };
}

function promptMessages(prompt) {
  const refs = prompt.toolRefs
    .map((name) => TOOL_DEFINITIONS.find((tool) => tool.name === name))
    .filter(Boolean);
  const lines = refs.map((tool) => `- ${tool.name}: ${tool.description}`);
  return {
    messages: [{
      role: "user",
      content: { type: "text", text: `Work through "${prompt.name}" using these Cortex tools:\n${lines.join("\n")}` },
    }],
  };
}

function currentGenerationId(root) {
  const dbPath = join(root, OUT_DIR, "graph", "graph.db");
  if (!existsSync(dbPath)) return null;
  const db = openStoreReadOnly(dbPath);
  try {
    return readIndexedMeta(db)?.manifest?.generationId ?? null;
  } finally {
    closeStore(db);
  }
}

export function createCortexMcpServer(options = {}) {
  const service = options.service ?? buildService(options);
  const server = new McpServer({ name: "cortex", version: "0.2.0" });

  for (const tool of TOOL_DEFINITIONS) {
    server.registerTool(tool.name, {
      description: tool.description,
      inputSchema: tool.strict,
      outputSchema: outputSchema(tool.outputKeys, tool.schemaVersion, tool.optionalOutputKeys),
      annotations: { readOnlyHint: true, idempotentHint: true, openWorldHint: false },
      _meta: { effects: TOOL_EFFECTS[tool.name] },
    }, async (input) => {
      try {
        return successResult(enrichResult(tool, await tool.call(service)(input ?? {})));
      } catch (error) {
        return failureResult(error);
      }
    });
  }

  for (const uri of RESOURCE_URIS) {
    server.resource(uri, uri, { mimeType: "application/json", description: `Cortex resource: ${uri}` }, async () => ({
      contents: [{ uri, mimeType: "application/json", text: JSON.stringify(redactForEgress(resourceForUri(uri, { service })), null, 2) }],
    }));
  }
  server.resource("cortex://effects", "cortex://effects", { mimeType: "application/json", description: "Declared effect profile per MCP tool" }, async () => ({
    contents: [{ uri: "cortex://effects", mimeType: "application/json", text: JSON.stringify(TOOL_EFFECTS, null, 2) }],
  }));

  for (const prompt of PROMPTS) {
    if (!promptByName(prompt.name)) throw new Error(`cortex prompt registry is missing ${prompt.name}`);
    server.prompt(prompt.name, prompt.description, () => promptMessages(prompt));
  }

  // CX-B1 strict-input enforcement. The SDK's stock tools/call handler turns
  // input-validation failures into an `isError` RESULT whose message quotes
  // the offending field; the S5 contract demands a visible rejection that
  // never echoes argument names. Replace the tools/call dispatcher with a
  // strict one: unknown fields raise a JSON-RPC error (client sees the
  // rejection), known-but-invalid arguments and service failures keep the
  // typed, redacted, stack-free envelope.
  server.server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const name = request.params?.name;
    const tool = TOOL_DEFINITIONS.find((candidate) => candidate.name === name);
    if (!tool) {
      throw new McpError(ErrorCode.InvalidParams, `Tool ${name} not found`);
    }
    const parsed = tool.strict.safeParse(request.params?.arguments ?? {});
    if (!parsed.success) {
      throw new McpError(ErrorCode.InvalidParams, `Input validation error: Invalid arguments for tool ${name}: schema rejected`);
    }
    try {
      return successResult(enrichResult(tool, await tool.call(service)(parsed.data)));
    } catch (error) {
      return failureResult(error);
    }
  });

  return server;
}

// Programmatic entrypoint used by `cortex mcp serve` (scripts/cli/commands.mjs)
// and by embedders: bind the root at start, then serve over stdio.
export async function startCortexMcpServer({ root, repoId } = {}) {
  const server = createCortexMcpServer({ root, repoId });
  const transport = new StdioServerTransport();
  await server.connect(transport);
  return server;
}

if (process.argv[1] && realpathSync(resolve(process.argv[1])) === realpathSync(fileURLToPath(import.meta.url))) {
  const { root, repoId, envRoots } = parseRootConfig();
  const server = createCortexMcpServer({ root, repoId, envRoots });
  await server.connect(new StdioServerTransport());
}
