function safeLimit(value, fallback = 50) {
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 ? Math.min(parsed, 200) : fallback;
}

function labels(node) { return new Set(node?.labels ?? []); }
function kindMatches(node, kind) {
  if (!kind) return true;
  const wanted = String(kind).toLowerCase();
  if (String(node.kind ?? "").toLowerCase() === wanted) return true;
  return [...labels(node)].some((label) => String(label).toLowerCase() === wanted);
}

function nameMatches(node, name, exact) {
  if (!name) return true;
  const values = [node.name, node.qualifiedName].filter(Boolean).map(String);
  if (exact) return values.some((value) => value === name);
  const wanted = String(name).toLowerCase();
  return values.some((value) => value.toLowerCase().includes(wanted));
}

/**
 * Bounded structural search over canonical AST/compiler symbol facts. This is
 * deliberately not regex-over-source: callers query structural node/edge
 * properties already produced by Tree-sitter/compiler providers.
 */
export function searchAstStructure(generation, pattern = {}, options = {}) {
  const limit = safeLimit(options.limit ?? pattern.limit);
  const pathPrefix = pattern.pathPrefix ? String(pattern.pathPrefix).replaceAll("\\", "/") : null;
  const relation = pattern.relation ? String(pattern.relation) : null;
  const nodes = [];
  for (const node of generation?.nodes ?? []) {
    if (!kindMatches(node, pattern.kind)) continue;
    if (!nameMatches(node, pattern.name, pattern.exactName === true)) continue;
    if (pathPrefix && !String(node.path ?? "").replaceAll("\\", "/").startsWith(pathPrefix)) continue;
    if (pattern.declaringType && node.declaringType !== pattern.declaringType && !String(node.qualifiedName ?? "").startsWith(`${pattern.declaringType}.`)) continue;
    if (pattern.label && !labels(node).has(pattern.label)) continue;
    nodes.push(node);
  }
  nodes.sort((a, b) => String(a.path ?? "").localeCompare(String(b.path ?? "")) || String(a.qualifiedName ?? a.name ?? a.id).localeCompare(String(b.qualifiedName ?? b.name ?? b.id)) || a.id.localeCompare(b.id));
  const selected = nodes.slice(0, limit);
  const selectedIds = new Set(selected.map((node) => node.id));
  const edges = (generation?.edges ?? [])
    .filter((edge) => (!relation || edge.kind === relation) && (selectedIds.has(edge.source) || selectedIds.has(edge.target)))
    .sort((a, b) => a.id.localeCompare(b.id))
    .slice(0, limit * 4);
  return {
    schemaVersion: 1,
    kind: "ast-structural-search",
    generationId: generation?.manifest?.generationId ?? null,
    pattern: { ...pattern },
    nodes: selected,
    edges,
    truncated: nodes.length > selected.length,
    omissions: nodes.length > selected.length ? [{ reason: "structural_search_limit", limit }] : [],
  };
}
