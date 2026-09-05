import { createHash } from "node:crypto";

import { EDGE_CONFIDENCE_TIERS, tierConfidence } from "./confidence-tiers.mjs";
import { resolveScopedSymbol } from "./structural-intelligence.mjs";

export const FRAMEWORK_INTELLIGENCE_PROVIDER = Object.freeze({ id: "blueprint-framework-intelligence", version: "bindings-contracts-ui-v1" });

function normalizePath(value) { return String(value ?? "").replaceAll("\\", "/").replace(/^\.\//, ""); }
function safe(value) { return String(value ?? "").replace(/[^A-Za-z0-9_.-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 120) || "unknown"; }
function sha(value) { return createHash("sha256").update(String(value)).digest("hex"); }
function evidence(file, line) { return [{ path: file.path, startLine: line, endLine: line, contentHash: file.contentHash ?? null }]; }

function domainId(kind, address) { return `domain:${kind}:sha256:${sha(`${kind}\0${address}`)}`; }

function ensureDomain(generation, kind, address, file, line, attributes = {}) {
  const id = domainId(kind, address);
  let node = generation.nodes.find((item) => item.id === id);
  if (!node) {
    node = {
      id,
      kind: "domain",
      labels: [kind],
      name: address,
      qualifiedName: `${kind}:${address}`,
      path: null,
      confidence: null,
      provider: FRAMEWORK_INTELLIGENCE_PROVIDER.id,
      factProvider: FRAMEWORK_INTELLIGENCE_PROVIDER,
      domainIdentity: { kind, address },
      evidence: evidence(file, line),
      ...attributes,
    };
    generation.nodes.push(node);
  } else {
    node.evidence = [...(node.evidence ?? []), ...evidence(file, line)];
    if (attributes.contractRole) {
      const roles = new Set([...(node.contractRoles ?? []), attributes.contractRole]);
      node.contractRoles = [...roles].sort();
    }
  }
  return node;
}

function addEdge(generation, kind, source, target, file, line, attributes = {}) {
  const id = `edge:${kind}:${source}->${target}:${FRAMEWORK_INTELLIGENCE_PROVIDER.id}:${line}`;
  if (generation.edges.some((edge) => edge.id === id)) return null;
  const edge = {
    id,
    kind,
    source,
    target,
    confidence: attributes.confidence ?? tierConfidence(attributes.confidenceTier ?? EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION),
    confidenceTier: attributes.confidenceTier ?? EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION,
    resolved: true,
    provider: FRAMEWORK_INTELLIGENCE_PROVIDER.id,
    factProvider: FRAMEWORK_INTELLIGENCE_PROVIDER,
    evidence: evidence(file, line),
    ...attributes,
  };
  generation.edges.push(edge);
  return edge;
}

function frontier(file, line, relation, targetName, result, reason = null) {
  return {
    id: `frontier:${FRAMEWORK_INTELLIGENCE_PROVIDER.id}:${safe(file.path)}:${line}:${relation}:${safe(targetName)}`,
    sourcePath: file.path,
    line,
    relation,
    targetName,
    state: result?.state ?? "unresolved",
    reason: reason ?? result?.reason ?? "binding_unresolved",
    candidates: [...(result?.candidates ?? [])],
    evidence: evidence(file, line),
  };
}

function exactHandler(generation, file, name) {
  return resolveScopedSymbol(generation, { fromPath: file.path, name, typesOnly: false });
}

function parseConfig(generation, file, summary) {
  const patterns = [
    /(?:process\.env|import\.meta\.env)\.([A-Z][A-Z0-9_]*)/g,
    /(?:os\.getenv|os\.environ\.get)\(\s*["']([A-Z][A-Z0-9_]*)["']/g,
    /env::var\(\s*["']([A-Z][A-Z0-9_]*)["']/g,
  ];
  const lines = String(file.text ?? "").split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    for (const pattern of patterns) {
      for (const match of lines[i].matchAll(pattern)) {
        const key = match[1];
        const node = ensureDomain(generation, "ConfigKey", key, file, i + 1, { bindingKind: "configuration" });
        addEdge(generation, "READS", `file:${file.path}`, node.id, file, i + 1, { bindingKind: "configuration" });
        summary.config += 1;
      }
    }
  }
}

function parseOrm(generation, file, summary) {
  const lines = String(file.text ?? "").split(/\r?\n/);
  const readMethods = new Set(["find", "findFirst", "findMany", "findUnique", "count", "aggregate"]);
  const writeMethods = new Set(["create", "createMany", "update", "updateMany", "delete", "deleteMany", "upsert"]);
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    for (const match of line.matchAll(/\bprisma\.([A-Za-z_]\w*)\.([A-Za-z_]\w*)\s*\(/g)) {
      const [, model, method] = match;
      if (!readMethods.has(method) && !writeMethods.has(method)) continue;
      const node = ensureDomain(generation, "DatabaseModel", model, file, i + 1, { bindingKind: "orm", orm: "prisma" });
      addEdge(generation, readMethods.has(method) ? "READS" : "WRITES", `file:${file.path}`, node.id, file, i + 1, { bindingKind: "orm", operation: method });
      summary.orm += 1;
    }
    for (const match of line.matchAll(/\bFROM\s+([A-Za-z_][\w.]*)/gi)) {
      const node = ensureDomain(generation, "DatabaseTable", match[1], file, i + 1, { bindingKind: "sql_literal" });
      addEdge(generation, "READS", `file:${file.path}`, node.id, file, i + 1, { bindingKind: "sql_literal" });
      summary.orm += 1;
    }
    for (const match of line.matchAll(/\b(?:INSERT\s+INTO|UPDATE|DELETE\s+FROM)\s+([A-Za-z_][\w.]*)/gi)) {
      const node = ensureDomain(generation, "DatabaseTable", match[1], file, i + 1, { bindingKind: "sql_literal" });
      addEdge(generation, "WRITES", `file:${file.path}`, node.id, file, i + 1, { bindingKind: "sql_literal" });
      summary.orm += 1;
    }
  }
}

function parseDi(generation, file, summary, frontiers) {
  const lines = String(file.text ?? "").split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    for (const match of line.matchAll(/@Inject\(\s*["']([^"']+)["']\s*\)/g)) {
      const node = ensureDomain(generation, "DependencyToken", match[1], file, i + 1, { bindingKind: "dependency_injection" });
      addEdge(generation, "USES", `file:${file.path}`, node.id, file, i + 1, { bindingKind: "dependency_injection", framework: "decorator" });
      summary.di += 1;
    }
    for (const match of line.matchAll(/\bDepends\(\s*([A-Za-z_]\w*)\s*\)/g)) {
      const targetName = match[1];
      const result = exactHandler(generation, file, targetName);
      if (result.state === "resolved") {
        addEdge(generation, "USES", `file:${file.path}`, result.symbol.id, file, i + 1, { bindingKind: "dependency_injection", framework: "fastapi" });
        summary.di += 1;
      } else frontiers.push(frontier(file, i + 1, "USES", targetName, result, "dependency_binding_unresolved"));
    }
  }
}

function parseRpc(generation, file, summary, frontiers) {
  const lines = String(file.text ?? "").split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    for (const match of line.matchAll(/\b(?:server|mcp|router)\.(?:tool|registerTool|register_tool)\(\s*["']([^"']+)["']\s*,\s*([A-Za-z_$][\w$]*)/g)) {
      const [, toolName, handlerName] = match;
      const contract = ensureDomain(generation, "ToolContract", toolName, file, i + 1, { contractKind: "tool", contractRole: "provider", contractRoles: ["provider"] });
      const handler = exactHandler(generation, file, handlerName);
      if (handler.state === "resolved") addEdge(generation, "HANDLES", contract.id, handler.symbol.id, file, i + 1, { bindingKind: "rpc_tool", contractId: contract.id });
      else frontiers.push(frontier(file, i + 1, "HANDLES", handlerName, handler, "tool_handler_unresolved"));
      summary.rpc += 1;
    }
    for (const match of line.matchAll(/\b(?:callTool|call_tool|invokeTool)\(\s*["']([^"']+)["']/g)) {
      const contract = ensureDomain(generation, "ToolContract", match[1], file, i + 1, { contractKind: "tool", contractRole: "consumer", contractRoles: ["consumer"] });
      addEdge(generation, "USES", `file:${file.path}`, contract.id, file, i + 1, { bindingKind: "rpc_tool_consumer" });
      summary.rpc += 1;
    }
  }
}

function parseUi(generation, file, summary, frontiers) {
  const text = String(file.text ?? "");
  const lineOf = (offset) => text.slice(0, offset).split(/\r?\n/).length;
  for (const match of text.matchAll(/<Route\b[^>]*\bpath=["']([^"']+)["'][^>]*\belement=\{<([A-Za-z_$][\w$]*)/g)) {
    const [, routePath, screenName] = match;
    const line = lineOf(match.index ?? 0);
    const route = ensureDomain(generation, "UiRoute", routePath, file, line, { contractKind: "ui_route", contractRole: "provider", contractRoles: ["provider"], entryPoint: true });
    const screen = exactHandler(generation, file, screenName);
    if (screen.state === "resolved") {
      screen.symbol.labels = [...new Set([...(screen.symbol.labels ?? []), "Screen"])];
      screen.symbol.entryPoint = true;
      addEdge(generation, "ROUTES_TO", route.id, screen.symbol.id, file, line, { bindingKind: "ui_navigation" });
    } else frontiers.push(frontier(file, line, "ROUTES_TO", screenName, screen, "ui_screen_unresolved"));
    summary.ui += 1;
  }
  for (const match of text.matchAll(/\bnavigate\(\s*["']([^"']+)["']/g)) {
    const routePath = match[1];
    const line = lineOf(match.index ?? 0);
    const route = ensureDomain(generation, "UiRoute", routePath, file, line, { contractKind: "ui_route", contractRole: "consumer", contractRoles: ["consumer"] });
    addEdge(generation, "USES", `file:${file.path}`, route.id, file, line, { bindingKind: "ui_navigation" });
    summary.ui += 1;
  }
}

export function augmentFrameworkIntelligence(generation, files = []) {
  const summary = { provider: FRAMEWORK_INTELLIGENCE_PROVIDER.id, di: 0, orm: 0, config: 0, rpc: 0, ui: 0, frontiers: [] };
  for (const file of files) {
    const normalized = { ...file, path: normalizePath(file.path) };
    parseConfig(generation, normalized, summary);
    parseOrm(generation, normalized, summary);
    parseDi(generation, normalized, summary, summary.frontiers);
    parseRpc(generation, normalized, summary, summary.frontiers);
    parseUi(generation, normalized, summary, summary.frontiers);
  }
  summary.frontiers.sort((a, b) => a.id.localeCompare(b.id));
  return Object.freeze(summary);
}
