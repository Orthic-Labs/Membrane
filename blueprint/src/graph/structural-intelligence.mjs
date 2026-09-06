import { createHash } from "node:crypto";

import { EDGE_CONFIDENCE_TIERS, tierConfidence } from "./confidence-tiers.mjs";

export const STRUCTURAL_INTELLIGENCE_PROVIDER = Object.freeze({
  id: "blueprint-structural-intelligence",
  version: "scope-hierarchy-frontier-v1",
});

function normalizePath(value) {
  return String(value ?? "").replaceAll("\\", "/").replace(/^\.\//, "");
}

function sourceEvidence(file, line) {
  return [{ path: file.path, startLine: line, endLine: line, contentHash: file.contentHash ?? null }];
}

function safe(value) {
  return String(value ?? "").replace(/[^A-Za-z0-9_.-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 120) || "unknown";
}

function factEdge(kind, source, target, evidence, tier = EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION, extra = {}) {
  return {
    id: `edge:${kind}:${source}->${target}:${STRUCTURAL_INTELLIGENCE_PROVIDER.id}`,
    kind,
    source,
    target,
    confidence: tierConfidence(tier),
    confidenceTier: tier,
    resolved: true,
    provider: STRUCTURAL_INTELLIGENCE_PROVIDER.id,
    factProvider: STRUCTURAL_INTELLIGENCE_PROVIDER,
    evidence,
    ...extra,
  };
}

function symbolNodes(generation) {
  return (generation?.nodes ?? []).filter((node) => node?.kind === "symbol" || node?.kind === "class" || node?.labels?.some((label) => ["Class", "Interface", "Trait", "Struct", "Method", "Function", "Test"].includes(label)));
}

function indexes(generation) {
  const symbols = symbolNodes(generation);
  const byPath = new Map();
  const byName = new Map();
  const byQualified = new Map();
  for (const symbol of symbols) {
    const path = normalizePath(symbol.path);
    if (!byPath.has(path)) byPath.set(path, []);
    byPath.get(path).push(symbol);
    if (!byName.has(symbol.name)) byName.set(symbol.name, []);
    byName.get(symbol.name).push(symbol);
    if (symbol.qualifiedName) {
      const key = `${path}\0${symbol.qualifiedName}`;
      if (!byQualified.has(key)) byQualified.set(key, []);
      byQualified.get(key).push(symbol);
    }
  }
  const importedPaths = new Map();
  for (const edge of generation?.edges ?? []) {
    if (edge.kind !== "IMPORTS" || !edge.target || !edge.source?.startsWith("file:") || !edge.target?.startsWith("file:")) continue;
    const from = edge.source.slice(5);
    const to = edge.target.slice(5);
    if (!importedPaths.has(from)) importedPaths.set(from, new Set());
    importedPaths.get(from).add(to);
  }
  return { symbols, byPath, byName, byQualified, importedPaths };
}

function typeLike(node) {
  const labels = new Set(node?.labels ?? []);
  return labels.has("Class") || labels.has("Interface") || labels.has("Trait") || labels.has("Struct") || node?.kind === "class";
}

/**
 * Exact-first lexical/scope resolver used by deterministic fallback providers.
 * Same-file unique name wins, then an exact symbol in a resolved imported file,
 * then a repository-wide exact name only when it is globally unique. Anything
 * else terminates at an inspectable frontier rather than guessing.
 */
export function resolveScopedSymbol(generation, { fromPath, name, typesOnly = false } = {}) {
  const ix = indexes(generation);
  const path = normalizePath(fromPath);
  const accept = (rows) => (rows ?? []).filter((row) => !typesOnly || typeLike(row));
  const local = accept(ix.byPath.get(path)).filter((row) => row.name === name);
  if (local.length === 1) return { state: "resolved", tier: "same_file", symbol: local[0], candidates: [local[0].id] };
  if (local.length > 1) return { state: "ambiguous", tier: "same_file", reason: "same_file_name_ambiguous", candidates: local.map((row) => row.id).sort() };

  const imports = [...(ix.importedPaths.get(path) ?? [])].sort();
  const imported = [];
  for (const targetPath of imports) imported.push(...accept(ix.byPath.get(targetPath)).filter((row) => row.name === name));
  const uniqueImported = [...new Map(imported.map((row) => [row.id, row])).values()];
  if (uniqueImported.length === 1) return { state: "resolved", tier: "import", symbol: uniqueImported[0], candidates: [uniqueImported[0].id] };
  if (uniqueImported.length > 1) return { state: "ambiguous", tier: "import", reason: "imported_name_ambiguous", candidates: uniqueImported.map((row) => row.id).sort() };

  const global = accept(ix.byName.get(name));
  if (global.length === 1) return { state: "resolved", tier: "global_unique", symbol: global[0], candidates: [global[0].id] };
  if (global.length > 1) return { state: "ambiguous", tier: "global", reason: "global_name_ambiguous", candidates: global.map((row) => row.id).sort() };
  return { state: "unresolved", tier: "none", reason: "symbol_not_found", candidates: [] };
}

function sourceSymbol(generation, path, name, typesOnly = false) {
  const rows = symbolNodes(generation).filter((node) => normalizePath(node.path) === normalizePath(path) && node.name === name && (!typesOnly || typeLike(node)));
  return rows.length === 1 ? rows[0] : null;
}

function relationDeclarations(file) {
  const rows = [];
  const lines = String(file.text ?? "").split(/\r?\n/);
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    let match = line.match(/\bclass\s+([A-Za-z_$][\w$]*)\s+extends\s+([A-Za-z_$][\w$]*)(?:\s+implements\s+([^\{]+))?/);
    if (match) {
      rows.push({ sourceName: match[1], targetName: match[2], kind: "INHERITS", line: index + 1 });
      for (const name of String(match[3] ?? "").split(",").map((value) => value.trim().match(/^([A-Za-z_$][\w$]*)/)?.[1]).filter(Boolean)) rows.push({ sourceName: match[1], targetName: name, kind: "IMPLEMENTS", line: index + 1 });
      continue;
    }
    match = line.match(/\bclass\s+([A-Za-z_$][\w$]*)\s+implements\s+([^\{]+)/);
    if (match) {
      for (const name of match[2].split(",").map((value) => value.trim().match(/^([A-Za-z_$][\w$]*)/)?.[1]).filter(Boolean)) rows.push({ sourceName: match[1], targetName: name, kind: "IMPLEMENTS", line: index + 1 });
      continue;
    }
    match = line.match(/\binterface\s+([A-Za-z_$][\w$]*)\s+extends\s+([^\{]+)/);
    if (match) {
      for (const name of match[2].split(",").map((value) => value.trim().match(/^([A-Za-z_$][\w$]*)/)?.[1]).filter(Boolean)) rows.push({ sourceName: match[1], targetName: name, kind: "INHERITS", line: index + 1 });
      continue;
    }
    match = line.match(/^\s*class\s+([A-Za-z_]\w*)\s*\(([^)]*)\)\s*:/);
    if (match) {
      for (const name of match[2].split(",").map((value) => value.trim().match(/^([A-Za-z_]\w*)/)?.[1]).filter(Boolean)) rows.push({ sourceName: match[1], targetName: name, kind: "INHERITS", line: index + 1 });
      continue;
    }
    match = line.match(/^\s*impl(?:<[^>]+>)?\s+([A-Za-z_]\w*)\s+for\s+([A-Za-z_]\w*)/);
    if (match) rows.push({ sourceName: match[2], targetName: match[1], kind: "IMPLEMENTS", line: index + 1 });
  }
  return rows;
}

function frontier({ file, line, relation, sourceId = null, targetName = null, outcome, dispatch = "lexical_scope" }) {
  return {
    id: `frontier:${safe(file.path)}:${line}:${relation}:${safe(targetName)}`,
    source: sourceId,
    sourcePath: file.path,
    line,
    relation,
    targetName,
    dispatch,
    state: outcome.state,
    reason: outcome.reason ?? "resolution_stopped",
    candidates: [...(outcome.candidates ?? [])],
    evidence: sourceEvidence(file, line),
  };
}

function methodName(node) {
  const labels = new Set(node?.labels ?? []);
  if (!(labels.has("Method") || /\./.test(String(node?.qualifiedName ?? "")))) return null;
  return node.name ?? String(node.qualifiedName).split(".").at(-1) ?? null;
}

function declaringTypeName(node) {
  const qualified = String(node?.qualifiedName ?? "");
  const parts = qualified.split(".");
  return parts.length > 1 ? parts.at(-2) : null;
}

function addOverrides(generation, hierarchyEdges, edges, frontiers) {
  const symbols = symbolNodes(generation);
  const typeById = new Map(symbols.filter(typeLike).map((node) => [node.id, node]));
  const methodsByTypeAndName = new Map();
  for (const node of symbols) {
    const name = methodName(node);
    const typeName = declaringTypeName(node);
    if (!name || !typeName) continue;
    const key = `${normalizePath(node.path)}\0${typeName}\0${name}`;
    if (!methodsByTypeAndName.has(key)) methodsByTypeAndName.set(key, []);
    methodsByTypeAndName.get(key).push(node);
    node.declaringType = node.declaringType ?? typeName;
    node.parentSymbol = node.parentSymbol ?? sourceSymbol(generation, node.path, typeName, true)?.id ?? null;
  }
  for (const relation of hierarchyEdges.filter((edge) => edge.kind === "INHERITS")) {
    const childType = typeById.get(relation.source);
    const baseType = typeById.get(relation.target);
    if (!childType || !baseType) continue;
    const childMethods = symbols.filter((node) => normalizePath(node.path) === normalizePath(childType.path) && declaringTypeName(node) === childType.name && methodName(node));
    for (const child of childMethods) {
      const key = `${normalizePath(baseType.path)}\0${baseType.name}\0${methodName(child)}`;
      const bases = methodsByTypeAndName.get(key) ?? [];
      if (bases.length === 1) {
        edges.push(factEdge("OVERRIDES", child.id, bases[0].id, child.evidence ?? relation.evidence, EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION, { declaringType: childType.id, baseType: baseType.id }));
      } else if (bases.length > 1) {
        frontiers.push({
          id: `frontier:override:${safe(child.id)}`,
          source: child.id,
          sourcePath: child.path,
          line: child.evidence?.[0]?.startLine ?? null,
          relation: "OVERRIDES",
          targetName: methodName(child),
          dispatch: "inheritance_member_lookup",
          state: "ambiguous",
          reason: "base_member_ambiguous",
          candidates: bases.map((node) => node.id).sort(),
          evidence: child.evidence ?? [],
        });
      }
    }
  }
}

function mroProjection(generation, hierarchyEdges) {
  const parents = new Map();
  for (const edge of hierarchyEdges.filter((item) => item.kind === "INHERITS")) {
    if (!parents.has(edge.source)) parents.set(edge.source, []);
    parents.get(edge.source).push(edge.target);
  }
  const rows = [];
  for (const node of symbolNodes(generation).filter(typeLike)) {
    const order = [];
    const seen = new Set([node.id]);
    let cycle = false;
    const visit = (id) => {
      for (const parent of (parents.get(id) ?? []).slice().sort()) {
        if (seen.has(parent)) { cycle = true; continue; }
        seen.add(parent);
        order.push(parent);
        visit(parent);
      }
    };
    visit(node.id);
    if (order.length || cycle) rows.push({ typeId: node.id, order, cycle });
  }
  return rows.sort((a, b) => a.typeId.localeCompare(b.typeId));
}

function canonicalEventTopics(generation, edges) {
  const topicNodes = (generation.nodes ?? []).filter((node) => node?.labels?.includes("EventTopic") && typeof node.name === "string");
  const groups = new Map();
  for (const node of topicNodes) {
    if (/\$\{|\{\{|<[^>]+>/.test(node.name)) continue;
    if (!groups.has(node.name)) groups.set(node.name, []);
    groups.get(node.name).push(node);
  }
  const aliases = new Map();
  for (const [name, nodes] of groups) {
    if (!nodes.length) continue;
    const canonicalId = `domain:event-topic:sha256:${createHash("sha256").update(name).digest("hex")}`;
    let canonical = generation.nodes.find((node) => node.id === canonicalId);
    if (!canonical) {
      const evidence = nodes.flatMap((node) => node.evidence ?? []);
      canonical = {
        id: canonicalId,
        kind: "domain",
        labels: ["EventTopic"],
        name,
        qualifiedName: `event:${name}`,
        path: null,
        confidence: null,
        provider: STRUCTURAL_INTELLIGENCE_PROVIDER.id,
        factProvider: STRUCTURAL_INTELLIGENCE_PROVIDER,
        domainIdentity: { kind: "event_topic", address: name },
        evidence,
      };
      generation.nodes.push(canonical);
    }
    for (const node of nodes) aliases.set(node.id, canonicalId);
  }
  if (!aliases.size) return;
  for (const edge of generation.edges ?? []) if (aliases.has(edge.target)) edge.target = aliases.get(edge.target);
  for (const edge of generation.edges ?? []) {
    if (edge.kind !== "CONSUMES" || !edge.target || !edge.source || !edge.target.startsWith("domain:event-topic:")) continue;
    const id = `edge:HANDLES:${edge.target}->${edge.source}:${STRUCTURAL_INTELLIGENCE_PROVIDER.id}`;
    if (!generation.edges.some((item) => item.id === id) && !edges.some((item) => item.id === id)) edges.push(factEdge("HANDLES", edge.target, edge.source, edge.evidence ?? [], EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION, { dispatch: "event_literal" }));
  }
}

function classifyTests(generation, files, edges) {
  const fileByPath = new Map(files.map((file) => [normalizePath(file.path), file]));
  const isTestPath = (path) => /(^|\/)(?:test|tests|__tests__)(\/|$)|(?:^|[._-])(test|spec)\.[^.]+$/i.test(path);
  const tests = [];
  for (const node of symbolNodes(generation)) {
    const path = normalizePath(node.path);
    const labels = new Set(node.labels ?? []);
    const testish = labels.has("Test") || isTestPath(path) || /^test[_A-Z]/.test(String(node.name ?? ""));
    if (!testish) continue;
    if (!labels.has("Test")) node.labels = [...labels, "Test"];
    node.entityKind = "test";
    node.testFramework = node.testFramework ?? (/\.py$/.test(path) ? "python" : /\.(?:[cm]?[jt]sx?)$/.test(path) ? "javascript" : "unknown");
    node.entryPoint = true;
    tests.push(node);
  }
  const outgoing = new Map();
  for (const edge of generation.edges ?? []) {
    if (!["CALLS", "REFERENCES"].includes(edge.kind) || !edge.target) continue;
    if (!outgoing.has(edge.source)) outgoing.set(edge.source, []);
    outgoing.get(edge.source).push(edge);
  }
  const testIds = new Set(tests.map((node) => node.id));
  for (const test of tests) {
    for (const relation of outgoing.get(test.id) ?? []) {
      if (testIds.has(relation.target)) continue;
      const id = `edge:TESTS:${test.id}->${relation.target}:${STRUCTURAL_INTELLIGENCE_PROVIDER.id}`;
      if ((generation.edges ?? []).some((edge) => edge.id === id) || edges.some((edge) => edge.id === id)) continue;
      edges.push(factEdge("TESTS", test.id, relation.target, relation.evidence ?? test.evidence ?? [], relation.confidenceTier ?? EDGE_CONFIDENCE_TIERS.SAME_FILE_LEXICAL, { reachability: "direct_static", viaRelationId: relation.id }));
    }
  }
  return tests.map((node) => ({ id: node.id, path: node.path, framework: node.testFramework })).sort((a, b) => a.id.localeCompare(b.id));
}

function existingFrontiers(generation) {
  const rows = [];
  for (const edge of generation.edges ?? []) {
    if (edge.target !== null || edge.resolved !== false) continue;
    rows.push({
      id: `frontier:${safe(edge.id)}`,
      source: edge.source ?? null,
      sourcePath: edge.evidence?.[0]?.path ?? null,
      line: edge.evidence?.[0]?.startLine ?? null,
      relation: edge.kind,
      targetName: edge.specifier ?? null,
      dispatch: edge.kind === "CALLS" ? "call" : edge.kind === "IMPORTS" ? "module" : "reference",
      state: /ambig/i.test(String(edge.reason ?? "")) ? "ambiguous" : "unresolved",
      reason: edge.reason ?? "unresolved_relation",
      candidates: [...(edge.candidates ?? [])],
      evidence: edge.evidence ?? [],
    });
  }
  return rows;
}

/**
 * Deterministic post-processing provider for BPT-075..080. It adds only facts
 * provable from source syntax plus already-resolved graph identities. Anything
 * that cannot be bound exactly becomes a resolution frontier.
 */
export function augmentStructuralIntelligence(generation, files = []) {
  const edges = [];
  const frontiers = existingFrontiers(generation);
  const hierarchyEdges = [];
  for (const file of files) {
    for (const declaration of relationDeclarations(file)) {
      const source = sourceSymbol(generation, file.path, declaration.sourceName, true);
      if (!source) {
        frontiers.push(frontier({ file, line: declaration.line, relation: declaration.kind, targetName: declaration.targetName, sourceId: null, outcome: { state: "unresolved", reason: "declaring_type_not_found", candidates: [] } }));
        continue;
      }
      const outcome = resolveScopedSymbol(generation, { fromPath: file.path, name: declaration.targetName, typesOnly: true });
      if (outcome.state !== "resolved") {
        frontiers.push(frontier({ file, line: declaration.line, relation: declaration.kind, targetName: declaration.targetName, sourceId: source.id, outcome }));
        continue;
      }
      const tier = outcome.tier === "same_file" ? EDGE_CONFIDENCE_TIERS.SAME_FILE_LEXICAL : EDGE_CONFIDENCE_TIERS.EXACT_RESOLUTION;
      const relation = factEdge(declaration.kind, source.id, outcome.symbol.id, sourceEvidence(file, declaration.line), tier, { resolutionTier: outcome.tier });
      hierarchyEdges.push(relation);
      edges.push(relation);
      source.rawDeclaredType = source.rawDeclaredType ?? declaration.targetName;
    }
  }
  addOverrides(generation, hierarchyEdges, edges, frontiers);
  canonicalEventTopics(generation, edges);
  const tests = classifyTests(generation, files, edges);
  const existingEdgeIds = new Set(generation.edges.map((edge) => edge.id));
  const newEdges = edges.filter((edge) => !existingEdgeIds.has(edge.id));
  generation.edges.push(...newEdges);
  const mro = mroProjection(generation, hierarchyEdges);
  const summary = Object.freeze({
    provider: STRUCTURAL_INTELLIGENCE_PROVIDER.id,
    version: STRUCTURAL_INTELLIGENCE_PROVIDER.version,
    hierarchyEdges: hierarchyEdges.length,
    overrideEdges: newEdges.filter((edge) => edge.kind === "OVERRIDES").length,
    dynamicDispatchEdges: newEdges.filter((edge) => edge.kind === "HANDLES").length,
    tests,
    mro,
    frontiers: frontiers.sort((a, b) => a.id.localeCompare(b.id)),
  });
  return summary;
}
