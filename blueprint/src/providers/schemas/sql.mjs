// D29: schema/config/IaC fact profiles. Emits domain nodes and edges only
// (resources, services, workflows, operations, messages, types, models,
// tables, columns, migrations; reads/writes/configures/deploys/generates).
// Never emits generic functions/calls from data formats.

export const PROFILE_KINDS = Object.freeze([
  "terraform", "dockerfile", "kubernetes", "github-actions", "openapi",
  "protobuf", "graphql", "prisma", "sql", "migration",
]);

export function profileForPath(path) {
  const lower = String(path ?? "").toLowerCase();
  if (/\.tf$/.test(lower) || /\.tfvars$/.test(lower)) return "terraform";
  if (/dockerfile$/.test(lower) || /\.dockerfile$/.test(lower)) return "dockerfile";
  if (/\.ya?ml$/.test(lower)) {
    if (lower.includes("k8s") || lower.includes("kubernetes") || lower.includes("deployment") || lower.includes("service")) return "kubernetes";
    if (lower.includes("workflow") || lower.includes("action")) return "github-actions";
    return "yaml";
  }
  if (/\.(json|ya?ml)$/.test(lower) && /openapi|swagger/.test(lower)) return "openapi";
  if (/\.proto$/.test(lower)) return "protobuf";
  if (/\.graphql$/.test(lower) || /\.gql$/.test(lower)) return "graphql";
  if (/\.prisma$/.test(lower)) return "prisma";
  if (/\.sql$/.test(lower)) return lower.includes("migration") ? "migration" : "sql";
  return null;
}

// Deterministic domain facts from a SQL schema/migration (CREATE TABLE /
// ALTER TABLE / CREATE INDEX).
export function extractSqlFacts(text) {
  const facts = [];
  const tablePattern = /CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?["`]?([A-Za-z0-9_.-]+)["`]?\s*\(/gi;
  let match;
  while ((match = tablePattern.exec(String(text ?? ""))) !== null) {
    facts.push({ kind: "table", name: match[1], line: lineOf(text, match.index) });
  }
  const indexPattern = /CREATE\s+(?:UNIQUE\s+)?INDEX\s+(?:IF\s+NOT\s+EXISTS\s+)?["`]?([A-Za-z0-9_.-]+)["`]?\s+ON\s+["`]?([A-Za-z0-9_.-]+)["`]?/gi;
  while ((match = indexPattern.exec(String(text ?? ""))) !== null) {
    facts.push({ kind: "index", name: match[1], table: match[2], line: lineOf(text, match.index) });
  }
  const migrationPattern = /^\s*--\s*Migration:\s*(.+)$/gim;
  while ((match = migrationPattern.exec(String(text ?? ""))) !== null) {
    facts.push({ kind: "migration", name: match[1].trim(), line: lineOf(text, match.index) });
  }
  return facts;
}

// Deterministic domain facts from a Dockerfile (FROM / EXPOSE / ENTRYPOINT).
export function extractDockerfileFacts(text) {
  const facts = [];
  const lines = String(text ?? "").split(/\r?\n/);
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i].trim();
    const from = line.match(/^FROM\s+(.+)$/i);
    if (from) facts.push({ kind: "base_image", name: from[1].trim(), line: i + 1 });
    const expose = line.match(/^EXPOSE\s+(\d+)/i);
    if (expose) facts.push({ kind: "port", name: expose[1], line: i + 1 });
    const entrypoint = line.match(/^ENTRYPOINT\s+(.+)$/i);
    if (entrypoint) facts.push({ kind: "entrypoint", name: entrypoint[1].trim().slice(0, 80), line: i + 1 });
  }
  return facts;
}

function lineOf(text, index) {
  return String(text ?? "").slice(0, index).split(/\r?\n/).length;
}
