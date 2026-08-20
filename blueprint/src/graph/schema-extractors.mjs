import { EDGE_CONFIDENCE_TIERS, tierConfidence } from "./confidence-tiers.mjs";

export const SCHEMA_EXTENSIONS = new Set(["graphql", "gql", "sql"]);

export function extractSchemaSymbols(file, addNode) {
  const extension = file.path.split(".").at(-1)?.toLowerCase();
  if (extension === "graphql" || extension === "gql") extractGraphqlSymbols(file, addNode);
  else if (extension === "sql") extractSqlSymbols(file, addNode);
}

function extractGraphqlSymbols(file, addNode) {
  for (let index = 0; index < file.lines.length; index += 1) {
    const match = file.lines[index].match(/^\s*(?:extend\s+)?(type|input|enum|interface|scalar|union)\s+([A-Za-z_]\w*)\b/);
    if (!match) continue;
    const [, definitionKind, name] = match;
    const endLine = ["scalar", "union"].includes(definitionKind) ? index + 1 : findBraceEnd(file.lines, index);
    addNode(symbolNode(file, name, index + 1, endLine, [`GraphQL${titleCase(definitionKind)}`]));
  }
}

function extractSqlSymbols(file, addNode) {
  const pattern = /^\s*CREATE\s+(?:OR\s+REPLACE\s+)?(TABLE|VIEW|FUNCTION|PROCEDURE|TRIGGER)\s+(?:IF\s+NOT\s+EXISTS\s+)?([^\s(]+)/i;
  for (let index = 0; index < file.lines.length; index += 1) {
    const match = file.lines[index].match(pattern);
    if (!match) continue;
    const name = normalizeSqlName(match[2]);
    addNode(symbolNode(file, name, index + 1, findSqlEnd(file.lines, index), [`Sql${titleCase(match[1].toLowerCase())}`]));
  }
}

export function addSchemaReferenceEdges(files, nodes, edges, indexes = {}) {
  const schemaNodes = indexes.schemaNodes ?? nodes.filter((node) => node.labels?.some((label) => label.startsWith("GraphQL") || label.startsWith("Sql")));
  const filesByPath = indexes.sourceFilesByPath ?? new Map(files.map((file) => [file.path, file]));
  for (const source of schemaNodes) {
    const file = filesByPath.get(source.path);
    if (!file) continue;
    const evidence = source.evidence[0];
    const body = file.lines.slice(evidence.startLine - 1, evidence.endLine).join("\n");
    for (const target of schemaNodes) {
      if (source.id === target.id) continue;
      const sameFamily = source.labels[0].startsWith("GraphQL") === target.labels[0].startsWith("GraphQL");
      if (!sameFamily) continue;
      const referenced = source.labels[0].startsWith("GraphQL")
        ? new RegExp(`\\b${escapeRegExp(target.name)}\\b`).test(body.replace(new RegExp(`^\\s*(?:extend\\s+)?\\w+\\s+${escapeRegExp(source.name)}\\b`), ""))
        : new RegExp(`\\bREFERENCES\\s+(?:[A-Za-z_]\\w*\\.)?["\\[]?${escapeRegExp(target.name)}["\\]]?\\b`, "i").test(body);
      if (referenced) edges.push(referenceEdge(source, target));
    }
  }
}

function symbolNode(file, name, startLine, endLine, labels) {
  return {
    id: `symbol:${file.path}::${name}`,
    kind: "symbol",
    labels,
    name,
    qualifiedName: name,
    path: file.path,
    evidence: [{ path: file.path, startLine, endLine, contentHash: file.contentHash }],
  };
}

function referenceEdge(source, target) {
  // Text-pattern name match against another schema symbol — a heuristic that
  // crosses file boundaries without a resolved binding (cortex B3).
  const tier = EDGE_CONFIDENCE_TIERS.CROSS_FILE_HEURISTIC;
  return {
    id: `edge:REFERENCES:${source.id}->${target.id}`,
    kind: "REFERENCES",
    source: source.id,
    target: target.id,
    confidence: tierConfidence(tier),
    confidenceTier: tier,
    evidence: source.evidence,
  };
}

function findBraceEnd(lines, startIndex) {
  let depth = 0;
  let seen = false;
  for (let index = startIndex; index < lines.length; index += 1) {
    for (const character of lines[index]) {
      if (character === "{") { depth += 1; seen = true; }
      else if (character === "}") { depth -= 1; if (seen && depth <= 0) return index + 1; }
    }
  }
  return startIndex + 1;
}

function findSqlEnd(lines, startIndex) {
  for (let index = startIndex; index < lines.length; index += 1) if (lines[index].includes(";")) return index + 1;
  return startIndex + 1;
}

function normalizeSqlName(value) {
  return value.split(".").at(-1).replace(/^["`\[]|["`\]]$/g, "");
}

function titleCase(value) {
  return value[0].toUpperCase() + value.slice(1);
}

function escapeRegExp(value) {
  return String(value).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
