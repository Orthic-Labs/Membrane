import { createHash } from "node:crypto";

export const GENERIC_WALKER_VERSION = "1.1.0";

const child = (node, field) => node?.childForFieldName?.(field) ?? node?.namedChildren?.find((item) => item.fieldName === field) ?? null;
const named = (node, types) => (node?.namedChildren ?? []).filter((item) => types.includes(item.type));
const strip = (value) => String(value ?? "").trim().replace(/^[`'"]|[`'"]$/g, "");
const descendants = (node, type) => node?.descendantsOfType?.(type) ?? [];

function nameOf(node, pattern = {}) {
  const byField = pattern.field ? child(node, pattern.field) : null;
  return byField?.text ?? named(node, pattern.nodeTypes ?? [])[0]?.text ?? null;
}

function context({ table, tree, file = {}, filePath, providerId, grammarHash, precisionTier }) {
  const hash = file.contentHash ?? createHash("sha256").update(file.text ?? "").digest("hex");
  const root = tree?.rootNode ?? null;
  const meta = { provider: providerId, grammarHash, precisionTier };
  const evidence = (node) => ({ path: filePath, startLine: node.startPosition.row + 1, endLine: node.endPosition.row + 1, contentHash: hash });
  return { table, tree, file, filePath, root, meta, evidence, confidence: root?.hasError ? 0.6 : 1, nodes: [], containsEdges: [], definesEdges: [], rawImports: [], rawCalls: [], configuresTargets: [], qualifiedCounts: new Map() };
}

function symbol(ctx, node, name, qualifiedName, labels, kind = "symbol", container = `file:${ctx.filePath}`, relation = "CONTAINS") {
  if (!name) return null;
  const evidence = ctx.evidence(node);
  const value = { id: `symbol:${ctx.filePath}::${qualifiedName}`, kind, labels, name, qualifiedName, path: ctx.filePath, confidence: ctx.confidence, confidenceTier: "EXACT_RESOLUTION", evidence: [evidence], ...ctx.meta };
  ctx.nodes.push(value);
  (relation === "DEFINES" ? ctx.definesEdges : ctx.containsEdges).push({ source: container, target: value.id, evidence: [evidence], ...ctx.meta });
  return value;
}

function addCalls(ctx, sourceId, node, isTest = false) {
  const call = ctx.table.capabilities?.call;
  if (!call || !node) return;
  for (const expression of descendants(node, call.type)) {
    const callee = child(expression, call.field);
    if (!callee) continue;
    const name = callee.type === "identifier" ? callee.text : child(callee, call.member)?.text;
    if (name) ctx.rawCalls.push({ sourceId, calleeName: name, isTest, ...ctx.meta });
  }
}

function addImport(ctx, node, specifier, kind = null) {
  if (!specifier) return;
  ctx.rawImports.push({ specifier: strip(specifier), ...(kind ? { kind } : {}), evidence: ctx.evidence(node), ...ctx.meta });
}

function nestedLiteral(node, depth = 0) {
  if (!node || depth > 4) return null;
  if (["string", "template_string"].includes(node.type)) return strip(node.text) || null;
  for (const item of node.namedChildren ?? []) { const found = nestedLiteral(item, depth + 1); if (found) return found; }
  return null;
}

function jsLike(ctx) {
  const { root, table, filePath } = ctx;
  const caps = table.capabilities;
  const unwrap = (node) => node.type === "export_statement" ? child(node, "declaration") ?? node : node;
  const functionTypes = caps.functionTypes ?? ["function_declaration", "generator_function_declaration"];
  const addFunction = (node, name, qualifiedName, labels, owner = `file:${filePath}`, relation = "CONTAINS", body = null, isTest = false) => {
    const value = symbol(ctx, node, name, qualifiedName, labels, "symbol", owner, relation);
    if (value) addCalls(ctx, value.id, body ?? child(node, "body") ?? node, isTest);
  };
  for (const raw of root.namedChildren ?? []) {
    const node = unwrap(raw);
    if (node.type === "import_statement") { addImport(ctx, node, child(node, "source")?.text); continue; }
    if (functionTypes.includes(node.type)) { const name = child(node, "name")?.text; addFunction(node, name, name, ["Function"]); continue; }
    if (node.type === "class_declaration") {
      const name = child(node, "name")?.text;
      const owner = symbol(ctx, node, name, name, ["Class"], "class");
      for (const member of child(node, "body")?.namedChildren ?? []) {
        if (member.type !== "method_definition") continue;
        const method = child(member, "name")?.text;
        if (method) addFunction(member, method, `${name}.${method}`, ["Method"], owner?.id, "DEFINES");
      }
      continue;
    }
    const labels = caps.typeLabels?.[node.type];
    if (labels) { const name = child(node, "name")?.text; symbol(ctx, node, name, name, labels); continue; }
    if (["lexical_declaration", "variable_declaration"].includes(node.type)) {
      for (const declarator of named(node, ["variable_declarator"])) {
        const name = child(declarator, "name")?.text;
        const value = child(declarator, "value");
        if (!name || !value) continue;
        if (["arrow_function", "function_expression"].includes(value.type)) addFunction(node, name, name, ["Function"], `file:${filePath}`, "CONTAINS", value);
        else {
          const constant = symbol(ctx, node, name, name, ["Constant"]);
          const literal = nestedLiteral(value);
          if (constant && literal) ctx.configuresTargets.push({ name, value: literal, node, ...ctx.meta });
        }
      }
      continue;
    }
    if (node.type === "expression_statement") {
      const call = node.namedChild(0);
      const callee = child(call, caps.call?.field)?.text;
      const label = strip(child(call, "arguments")?.namedChild(0)?.text);
      if (call?.type === caps.call?.type && ["test", "it"].includes(callee) && label) addFunction(call, label, label, ["Test"], `file:${filePath}`, "CONTAINS", call, true);
    }
  }
}

function python(ctx) {
  const { root, filePath } = ctx;
  const addFunction = (node, owner = `file:${filePath}`, prefix = null) => {
    const name = child(node, "name")?.text;
    const qualified = prefix ? `${prefix}.${name}` : name;
    const isTest = !prefix && /^test_/.test(name ?? "");
    const value = symbol(ctx, node, name, qualified, [isTest ? "Test" : prefix ? "Method" : "Function"], "symbol", owner, prefix ? "DEFINES" : "CONTAINS");
    if (value) addCalls(ctx, value.id, child(node, "body") ?? node, isTest);
  };
  const addClass = (node) => {
    const name = child(node, "name")?.text;
    const owner = symbol(ctx, node, name, name, ["Class"], "class");
    for (const member of child(node, "body")?.namedChildren ?? []) {
      const definition = member.type === "decorated_definition" ? member.namedChildren.find((item) => item.type === "function_definition") : member;
      if (definition?.type === "function_definition") addFunction(definition, owner?.id, name);
    }
  };
  for (const raw of root.namedChildren ?? []) {
    const node = raw.type === "decorated_definition" ? raw.namedChildren.find((item) => ["function_definition", "class_definition"].includes(item.type)) : raw;
    if (!node) continue;
    if (node.type === "import_statement") addImport(ctx, node, child(node, "name")?.text, "absolute");
    else if (node.type === "import_from_statement") { const mod = child(node, "module_name"); addImport(ctx, node, mod?.text, mod?.type === "relative_import" ? "relative" : "absolute"); }
    else if (node.type === "function_definition") addFunction(node);
    else if (node.type === "class_definition") addClass(node);
  }
}

function rust(ctx) {
  const { root, filePath } = ctx;
  const typeName = (node) => node.text.match(/impl(?:\s*<[^>]*>)?\s+(?:[\w:]+(?:<[^>]*>)?\s+for\s+)?([\w:]+)/)?.[1]?.split("::").at(-1) ?? null;
  const addFunction = (node, owner = `file:${filePath}`, prefix = null, relation = "CONTAINS") => {
    const name = child(node, "name")?.text;
    const isTest = node.previousNamedSibling?.type === "attribute_item" && /\btest\b/.test(node.previousNamedSibling.text);
    const value = symbol(ctx, node, name, prefix ? `${prefix}.${name}` : name, [isTest ? "Test" : prefix ? "Method" : "Function"], "symbol", owner, relation);
    if (value) addCalls(ctx, value.id, child(node, "body") ?? node, isTest);
  };
  for (const node of root.namedChildren ?? []) {
    if (node.type === "use_declaration") addImport(ctx, node, node.text.replace(/^\s*pub\s+/, ""), "use");
    else if (node.type === "mod_item") { const name = child(node, "name")?.text; if (!node.namedChildren.some((item) => item.type === "declaration_list")) addImport(ctx, node, name, "mod"); }
    else if (node.type === "function_item") addFunction(node);
    else if (["struct_item", "enum_item", "trait_item"].includes(node.type)) { const name = child(node, "name")?.text; symbol(ctx, node, name, name, node.type === "struct_item" ? ["Class", "Struct"] : node.type === "enum_item" ? ["Class", "Enum"] : ["Class", "Trait"], "class"); }
    else if (node.type === "impl_item") {
      const name = typeName(node);
      const owner = ctx.nodes.find((item) => item.qualifiedName === name)?.id ?? `file:${filePath}`;
      for (const member of node.namedChildren.find((item) => item.type === "declaration_list")?.namedChildren ?? []) if (member.type === "function_item") addFunction(member, owner, name, owner === `file:${filePath}` ? "CONTAINS" : "DEFINES");
    }
  }
}

function uniqueQualifiedName(ctx, name, scope, node) {
  const base = [...scope, name].filter(Boolean).join(".");
  const count = (ctx.qualifiedCounts.get(base) ?? 0) + 1;
  ctx.qualifiedCounts.set(base, count);
  return count === 1 ? base : `${base}#${node.startPosition.row + 1}-${count}`;
}

function patterns(ctx) {
  const visit = (node, scope = []) => {
    const scopeNames = [];
    for (const pattern of ctx.table.functions ?? []) if (pattern.nodeTypes.includes(node.type)) {
      const name = nameOf(node, pattern.name);
      const value = symbol(ctx, node, name, uniqueQualifiedName(ctx, name, scope, node), pattern.labels ?? ["Function"]);
      if (value) { addCalls(ctx, value.id, node); scopeNames.push(name); }
    }
    for (const pattern of ctx.table.classes ?? []) if (pattern.nodeTypes.includes(node.type)) {
      const name = nameOf(node, pattern.name);
      if (symbol(ctx, node, name, uniqueQualifiedName(ctx, name, scope, node), pattern.labels ?? ["Type"], "class")) scopeNames.push(name);
    }
    for (const pattern of ctx.table.declarations ?? []) if (pattern.nodeTypes.includes(node.type)) {
      const name = strip(nameOf(node, pattern.name));
      if (!name) continue;
      symbol(ctx, node, name, uniqueQualifiedName(ctx, name, scope, node), pattern.labels ?? ["Declaration"]);
      scopeNames.push(name);
    }
    for (const pattern of ctx.table.imports ?? []) if (pattern.nodeTypes.includes(node.type)) addImport(ctx, node, nameOf(node, pattern.name));
    for (const pattern of ctx.table.comments ?? []) if (pattern.nodeTypes.includes(node.type)) {
      const evidence = ctx.evidence(node);
      ctx.nodes.push({ id: `comment:${ctx.filePath}:${node.id}`, kind: "comment", labels: ["Comment"], name: null, qualifiedName: null, path: ctx.filePath, confidence: ctx.confidence, confidenceTier: "EXACT_RESOLUTION", evidence: [evidence], text: node.text ?? "", ...ctx.meta });
    }
    const childScope = scopeNames.length ? [...scope, scopeNames[0]] : scope;
    for (const item of node.namedChildren ?? []) visit(item, childScope);
  };
  visit(ctx.root);
}

export function walkTable(options) {
  const ctx = context(options);
  if (!ctx.root) return { ...ctx, reports: [{ kind: "parse_failed", path: ctx.filePath, message: "no root node" }] };
  ({ jslike: jsLike, python, rust }[ctx.table.capabilities?.strategy] ?? patterns)(ctx);
  const edges = [
    ...ctx.containsEdges.map((edge) => ({ ...edge, kind: "CONTAINS" })),
    ...ctx.definesEdges.map((edge) => ({ ...edge, kind: "DEFINES" })),
    ...ctx.rawImports.map((item) => ({ kind: "IMPORTS", source: `file:${ctx.filePath}`, target: item.specifier ? `file:${item.specifier}` : null, evidence: [item.evidence], ...ctx.meta })),
    ...ctx.rawCalls.map((item) => ({ kind: "CALLS", source: item.sourceId, target: null, callName: item.calleeName, ...ctx.meta })),
  ];
  return { ...ctx, edges, reports: ctx.root.hasError ? [{ kind: "partial_parse", path: ctx.filePath, message: "parse contains errors" }] : [] };
}
