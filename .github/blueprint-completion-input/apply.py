from pathlib import Path
import subprocess

ROOT = Path.cwd()


def write(path, content):
    p = ROOT / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content)


def replace_once(path, old, new):
    p = ROOT / path
    text = p.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected one match in {path}, got {count}: {old[:100]!r}")
    p.write_text(text.replace(old, new, 1))


def commit(message, files, tests):
    subprocess.run(["node", "--test", *tests], check=True)
    subprocess.run(["git", "add", *files], check=True)
    subprocess.run(["git", "commit", "-m", message], check=True)

subprocess.run(["git", "config", "user.name", "Blueprint completion automation"], check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], check=True)

# ---------------------------------------------------------------------------
# Commit 1: doc-truth public grounding + categorical confidence for extracted joins
# ---------------------------------------------------------------------------
write("blueprint/src/graph/doc-truth-projection.mjs", r'''// Generation-bound document truth projection.
//
// Claims remain declarations. This module compares those declarations with
// deterministic code evidence already persisted by Blueprint and never turns
// documentation prose into observed code truth.

const DETERMINISTIC_CLASSES = new Set(["EXTRACTED", "DETERMINISTIC_EXTRACTION", "AUTHORITATIVE_SEMANTIC"]);

function publicConfidence(edge) {
  return DETERMINISTIC_CLASSES.has(String(edge?.confidenceClass ?? "")) ? null : edge?.confidence ?? null;
}

function citationFor(edge) {
  const doc = edge?.evidenceDocPath ? {
    kind: "document",
    path: edge.evidenceDocPath,
    line: edge.evidenceDocLine ?? null,
    contentHash: edge.evidenceDocSha1 ?? null,
  } : null;
  const code = edge?.evidenceCodePath ? {
    kind: "code",
    path: edge.evidenceCodePath,
    nodeId: edge.evidenceCodeNodeId ?? null,
    contentHash: edge.evidenceCodeContentHash ?? null,
  } : null;
  return [doc, code].filter(Boolean);
}

function groundingState(edges, freshness) {
  if (freshness && freshness !== "fresh") return "stale";
  const kinds = new Set(edges.map((edge) => edge.kind));
  if (kinds.has("supports") && kinds.has("contradicts")) return "ambiguous";
  if (kinds.has("contradicts")) return "contradicted";
  if (kinds.has("supports")) return "direct";
  if (kinds.has("supersedes")) return "indirect";
  return "unsupported";
}

export const GROUNDING_STATES = Object.freeze(["direct", "indirect", "unsupported", "contradicted", "ambiguous", "stale"]);

export function projectDocumentTruth({ claims = [], supersedes = [], generationId = null, freshness = "unknown" } = {}) {
  const grounded = claims.map((claim) => {
    const edges = [...(claim.edges ?? [])];
    const state = groundingState(edges, freshness);
    const citations = [];
    const seen = new Set();
    for (const edge of edges) {
      for (const citation of citationFor(edge)) {
        const key = JSON.stringify(citation);
        if (!seen.has(key)) { seen.add(key); citations.push(citation); }
      }
    }
    const observed = edges.map((edge) => ({
      kind: edge.kind,
      source: edge.source,
      target: edge.target,
      reason: edge.reason ?? null,
      provenance: edge.confidenceClass ?? null,
      confidence: publicConfidence(edge),
      evidence: citationFor(edge),
    }));
    return {
      claimId: claim.id,
      declared: {
        documentId: claim.documentId ?? null,
        source: claim.source ?? null,
        line: claim.line ?? null,
        status: claim.status ?? "unknown",
        sourceHash: claim.sha1 ?? null,
      },
      grounding: state,
      observed,
      mismatch: ["contradicted", "ambiguous"].includes(state)
        ? { present: true, reason: state === "contradicted" ? "declared_intent_conflicts_with_observed_code" : "conflicting_grounding_evidence" }
        : { present: false, reason: null },
      citations,
      confidence: observed.length && observed.every((row) => row.confidence === null) ? null : observed.map((row) => row.confidence).filter((value) => value !== null),
      invalidation: { generationId, freshness, stale: state === "stale" },
    };
  });
  const counts = Object.fromEntries(GROUNDING_STATES.map((state) => [state, grounded.filter((item) => item.grounding === state).length]));
  return Object.freeze({
    schemaVersion: 1,
    kind: "document-truth-grounding",
    generationId,
    freshness,
    claims: grounded,
    supersedes: [...supersedes],
    counts,
  });
}
''')

# Make the existing nullable-confidence migration reusable and add schema 20 for doc-truth joins.
write("blueprint/src/graph/confidence-migration.mjs", r'''// Nullable confidence migrations for INV-004.
// Called inside migrateDb's existing transaction/backup boundary. Schema-only
// changes preserve rowids, indexes, triggers and historical values verbatim.

function makeConfidenceNullable(db, table, suffix) {
  const definition = db.prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name=?").get(table);
  const columns = db.prepare(`PRAGMA table_info("${table}")`).all();
  const confidence = columns.find((column) => column.name === "confidence");
  if (!definition?.sql || !confidence) {
    throw Object.assign(new Error(`confidence migration: missing ${table}.confidence`), { code: "confidence_migration_schema_mismatch" });
  }
  if (!confidence.notnull) return;
  const nullable = definition.sql.replace(/(\bconfidence\s+REAL)\s+NOT\s+NULL\b/i, "$1");
  if (nullable === definition.sql) {
    throw Object.assign(new Error(`confidence migration: unrecognized ${table} definition`), { code: "confidence_migration_schema_mismatch" });
  }
  const temporary = `${table}_nullable_confidence_${suffix}`;
  const create = nullable.replace(/^CREATE TABLE\s+(?:IF NOT EXISTS\s+)?(?:"[^\"]+"|\w+)/i, `CREATE TABLE "${temporary}"`);
  const dependents = db.prepare("SELECT sql FROM sqlite_master WHERE tbl_name=? AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY type,name").all(table);
  const names = columns.map((column) => `"${column.name.replaceAll('"', '""')}"`).join(", ");
  db.exec(create);
  db.exec(`INSERT INTO "${temporary}" (rowid, ${names}) SELECT rowid, ${names} FROM "${table}" ORDER BY rowid`);
  db.exec(`DROP TABLE "${table}"`);
  db.exec(`ALTER TABLE "${temporary}" RENAME TO "${table}"`);
  for (const dependent of dependents) db.exec(dependent.sql);
}

// Schema 19: canonical graph node/edge confidence can be NULL.
export function migrateNullableFactConfidence(db) {
  for (const table of ["symbols", "edges"]) makeConfidenceNullable(db, table, "v19");
}

// Schema 20: deterministic doc↔code joins obey the same categorical contract.
export function migrateNullableDocTruthConfidence(db) {
  makeConfidenceNullable(db, "claim_code_edges", "v20");
}
''')
replace_once(
    "blueprint/src/graph/store-sqlite.mjs",
    'import { migrateNullableFactConfidence } from "./confidence-migration.mjs";',
    'import { migrateNullableFactConfidence, migrateNullableDocTruthConfidence } from "./confidence-migration.mjs";',
)
replace_once(
    "blueprint/src/graph/store-sqlite.mjs",
    '''  // Migration 19 — confidence is nullable for categorical facts. The existing
  // migration runner supplies the backup and atomic commit/rollback boundary.
  migrateNullableFactConfidence,
];''',
    '''  // Migration 19 — confidence is nullable for categorical graph facts.
  migrateNullableFactConfidence,
  // Migration 20 — deterministic doc↔code joins use the same nullable contract.
  migrateNullableDocTruthConfidence,
];''',
)
replace_once(
    "blueprint/src/graph/store-sqlite.mjs",
    '      join.confidence ?? 1,',
    '      join.confidence === undefined ? 1 : join.confidence,',
)
replace_once(
    "blueprint/src/graph/store-sqlite.mjs",
    '    confidence: row.confidence ?? 1,\n    confidenceClass: row.confidence_class ?? undefined,',
    '    confidence: row.confidence,\n    confidenceClass: row.confidence_class ?? undefined,',
)
replace_once(
    "blueprint/src/graph/static-provider.mjs",
    '''      confidence: 0.9,
      confidenceClass: "INFERRED",''',
    '''      confidence: 0.9,
      confidenceClass: "HEURISTIC_BRIDGE",''',
)
replace_once(
    "blueprint/src/graph/static-provider.mjs",
    '''      confidence: 0.85,
      confidenceClass: "EXTRACTED",
      reason: `claim status=${status || "claim"} mentions stale/drift/missing/contradict; code node exists`,''',
    '''      confidence: null,
      confidenceClass: "DETERMINISTIC_EXTRACTION",
      reason: `claim status=${status || "claim"} mentions stale/drift/missing/contradict; code node exists`,''',
)
replace_once(
    "blueprint/src/graph/static-provider.mjs",
    '''      confidence: 0.85,
      confidenceClass: "EXTRACTED",
      reason: `claim status=implemented; code node exists`,''',
    '''      confidence: null,
      confidenceClass: "DETERMINISTIC_EXTRACTION",
      reason: `claim status=implemented; code node exists`,''',
)

# Public application surface exposes the grounding projection while preserving legacy claims.
replace_once(
    "blueprint/src/lib/application/service.mjs",
    '''  closeStore,
  listClaimSlice,
  loadGeneration,''',
    '''  closeStore,
  listClaimSlice,
  listDocumentSupersession,
  loadGeneration,''',
)
replace_once(
    "blueprint/src/lib/application/service.mjs",
    'import { buildDisposableArchitectureProjection } from "../../graph/architecture-model.mjs";\n',
    'import { buildDisposableArchitectureProjection } from "../../graph/architecture-model.mjs";\nimport { projectDocumentTruth } from "../../graph/doc-truth-projection.mjs";\n',
)
old_doc = '''    async documentTruth(input = {}, options = {}) {
      return withCurrentDb(input, ({ db, meta, receipt }) => ({
        schemaVersion: 1,
        generationId: meta.manifest.generationId,
        claims: listClaimSlice(db, meta.manifest.generationId, {
          limit: Number(input.limit ?? 200),
          claimId: input.claimId,
          kind: input.kind,
        }),
        freshnessReceipt: receipt,
        omissions: [],
        truncated: false,
      }), options);
    },'''
new_doc = '''    async documentTruth(input = {}, options = {}) {
      return withCurrentDb(input, ({ db, meta, receipt }) => {
        const claims = listClaimSlice(db, meta.manifest.generationId, {
          limit: Number(input.limit ?? 200),
          claimId: input.claimId,
          kind: input.kind,
        });
        const supersedes = listDocumentSupersession(db, meta.manifest.generationId, { limit: Number(input.limit ?? 200) });
        const grounding = projectDocumentTruth({
          claims,
          supersedes,
          generationId: meta.manifest.generationId,
          freshness: receipt.freshness,
        });
        return {
          schemaVersion: 1,
          generationId: meta.manifest.generationId,
          claims,
          grounding: grounding.claims,
          groundingCounts: grounding.counts,
          supersedes: grounding.supersedes,
          freshnessReceipt: receipt,
          omissions: [],
          truncated: false,
        };
      }, options);
    },'''
replace_once("blueprint/src/lib/application/service.mjs", old_doc, new_doc)

write("blueprint/tests/doc-truth-projection.test.mjs", r'''import assert from "node:assert/strict";
import test from "node:test";
import { openStore } from "../src/graph/store-sqlite.mjs";
import { migrateNullableDocTruthConfidence } from "../src/graph/confidence-migration.mjs";
import { projectDocumentTruth } from "../src/graph/doc-truth-projection.mjs";

const edge = (kind, confidenceClass = "DETERMINISTIC_EXTRACTION", confidence = null) => ({
  kind, source: "doc:a", target: "file:src/a.ts", confidenceClass, confidence,
  reason: `${kind} reason`, evidenceDocPath: "docs/a.md", evidenceDocLine: 4,
  evidenceDocSha1: "doc-hash", evidenceCodePath: "src/a.ts", evidenceCodeNodeId: "file:src/a.ts",
  evidenceCodeContentHash: "code-hash",
});
const claim = (id, edges) => ({ id, documentId: "doc:a", source: "docs/a.md", line: 4, status: "implemented", sha1: "doc-hash", edges });

test("doc truth preserves declaration and observed evidence with deterministic null confidence", () => {
  const projection = projectDocumentTruth({ claims: [claim("c1", [edge("supports")])], generationId: "g1", freshness: "fresh" });
  assert.equal(projection.claims[0].grounding, "direct");
  assert.equal(projection.claims[0].declared.status, "implemented");
  assert.equal(projection.claims[0].observed[0].confidence, null);
  assert.equal(projection.claims[0].mismatch.present, false);
  assert.deepEqual(projection.claims[0].citations.map((c) => c.kind), ["document", "code"]);
});

test("doc truth emits contradicted, ambiguous, unsupported and stale rather than guessing", () => {
  assert.equal(projectDocumentTruth({ claims: [claim("c", [edge("contradicts")])], freshness: "fresh" }).claims[0].grounding, "contradicted");
  assert.equal(projectDocumentTruth({ claims: [claim("c", [edge("supports"), edge("contradicts")])], freshness: "fresh" }).claims[0].grounding, "ambiguous");
  assert.equal(projectDocumentTruth({ claims: [claim("c", [])], freshness: "fresh" }).claims[0].grounding, "unsupported");
  assert.equal(projectDocumentTruth({ claims: [claim("c", [edge("supports")])], freshness: "changed_since_generation" }).claims[0].grounding, "stale");
});

test("heuristic doc relation may retain inferential confidence", () => {
  const projected = projectDocumentTruth({ claims: [claim("c", [edge("supersedes", "HEURISTIC_BRIDGE", 0.9)])], freshness: "fresh" });
  assert.equal(projected.claims[0].grounding, "indirect");
  assert.equal(projected.claims[0].observed[0].confidence, 0.9);
});

test("schema 20 makes claim_code_edges confidence nullable without changing other columns", () => {
  const db = openStore(":memory:", { upToVersion: 19 });
  try {
    assert.equal(db.prepare("PRAGMA table_info(claim_code_edges)").all().find((c) => c.name === "confidence").notnull, 1);
    migrateNullableDocTruthConfidence(db);
    const columns = db.prepare("PRAGMA table_info(claim_code_edges)").all();
    assert.equal(columns.find((c) => c.name === "confidence").notnull, 0);
    assert.ok(columns.some((c) => c.name === "evidence_code_content_hash"));
  } finally { db.close(); }
});
''')

# Existing relational doc-truth test should now prove null deterministic confidence too.
replace_once(
    "blueprint/tests/graph-substrate.test.mjs",
    '  assert.ok(truth.joins.every((join) => join.confidenceClass && join.evidence.codeNode.contentHash === "abc123"));',
    '  assert.ok(truth.joins.every((join) => join.confidenceClass && join.evidence.codeNode.contentHash === "abc123"));\n  assert.ok(truth.joins.filter((join) => ["supports", "contradicts"].includes(join.kind)).every((join) => join.confidence === null));',
)
replace_once(
    "blueprint/tests/application-service-queries.test.mjs",
    '    assert.ok(Array.isArray(result.claims));\n    assert.ok(result.freshnessReceipt);',
    '    assert.ok(Array.isArray(result.claims));\n    assert.ok(Array.isArray(result.grounding));\n    assert.ok(result.groundingCounts && typeof result.groundingCounts === "object");\n    assert.ok(result.freshnessReceipt);',
)

commit(
    "feat(blueprint-doc-truth): expose generation-bound grounding states",
    [
      "blueprint/src/graph/doc-truth-projection.mjs",
      "blueprint/src/graph/confidence-migration.mjs",
      "blueprint/src/graph/store-sqlite.mjs",
      "blueprint/src/graph/static-provider.mjs",
      "blueprint/src/lib/application/service.mjs",
      "blueprint/tests/doc-truth-projection.test.mjs",
      "blueprint/tests/graph-substrate.test.mjs",
      "blueprint/tests/application-service-queries.test.mjs",
    ],
    [
      "blueprint/tests/doc-truth-projection.test.mjs",
      "blueprint/tests/graph-substrate.test.mjs",
      "blueprint/tests/application-service-queries.test.mjs",
      "blueprint/tests/store-migrations.test.mjs",
      "blueprint/tests/store-sqlite.test.mjs",
    ],
)

# ---------------------------------------------------------------------------
# Commit 2: first-class entry registry + honest liveness projection
# ---------------------------------------------------------------------------
write("blueprint/src/graph/entry-points.mjs", r'''// Entry-point registry for derived architecture/liveness views.
// Explicit source-backed entry points are separated from structural candidates:
// zero inbound degree is useful orientation evidence but never proves execution.

function labels(node) { return new Set((node?.labels ?? []).map((label) => String(label).toLowerCase())); }
function evidence(node) { return Array.isArray(node?.evidence) ? node.evidence.filter(Boolean) : []; }

export function buildEntryPointRegistry(generation, { includeStructuralCandidates = true } = {}) {
  const nodes = generation?.nodes ?? [];
  const edges = generation?.edges ?? [];
  const incoming = new Set(edges.map((edge) => edge.target).filter(Boolean));
  const outgoing = new Set(edges.map((edge) => edge.source).filter(Boolean));
  const rows = [];
  for (const node of nodes) {
    const tagged = node?.entryPoint === true || labels(node).has("entrypoint") || labels(node).has("entry_point");
    if (tagged) {
      rows.push({ id: node.id, node, authority: "explicit", reason: "source_backed_entrypoint_marker", evidence: evidence(node) });
      continue;
    }
    if (includeStructuralCandidates && node?.kind === "symbol" && outgoing.has(node.id) && !incoming.has(node.id)) {
      rows.push({ id: node.id, node, authority: "structural_candidate", reason: "outgoing_with_zero_observed_inbound", evidence: evidence(node) });
    }
  }
  return rows.sort((a, b) => String(a.id).localeCompare(String(b.id)));
}
''')
write("blueprint/src/graph/liveness.mjs", r'''import { buildEntryPointRegistry } from "./entry-points.mjs";

export const LIVENESS_STATES = Object.freeze(["LIVE", "UNREACHED", "UNKNOWN"]);

function citations(values) {
  const rows = [], seen = new Set();
  for (const value of values) for (const item of value?.evidence ?? []) {
    const row = { path: item.path ?? null, startLine: item.startLine ?? null, endLine: item.endLine ?? null, contentHash: item.contentHash ?? null };
    const key = JSON.stringify(row);
    if (!seen.has(key)) { seen.add(key); rows.push(row); }
  }
  return rows;
}

export function buildLivenessProjection(generation, options = {}) {
  const generationId = generation?.manifest?.generationId ?? null;
  const maxNodes = Math.max(1, Math.min(20000, Number(options.maxNodes ?? 5000) || 5000));
  const maxEdges = Math.max(1, Math.min(100000, Number(options.maxEdges ?? 20000) || 20000));
  const maxHops = Math.max(1, Math.min(64, Number(options.maxHops ?? 24) || 24));
  const sourceState = options.sourceState ?? "clean";
  const allNodes = generation?.nodes ?? [];
  const selected = allNodes.slice(0, maxNodes);
  const boundedOut = allNodes.length > selected.length;
  const complete = generation?.manifest?.complete === true && !generation?.manifest?.truncated && !boundedOut;
  const registry = buildEntryPointRegistry(generation, { includeStructuralCandidates: true });
  const admitted = registry.filter((entry) => entry.authority === "explicit" && entry.evidence.length > 0);
  const trustworthy = complete && sourceState === "clean" && admitted.length > 0;

  if (!trustworthy) {
    const reason = sourceState !== "clean" ? "source_not_current" : !complete ? "generation_incomplete_or_bounded" : "no_explicit_entrypoint_evidence";
    return Object.freeze({
      schemaVersion: 1, kind: "liveness", generationId, sourceState,
      entryPoints: registry.map(({ node, ...entry }) => ({ ...entry, nodeId: node.id })),
      results: selected.map((node) => ({ nodeId: node.id, path: node.path ?? null, state: "UNKNOWN", reason, evidence: citations([node]), reachabilityPath: [] })),
      counts: { LIVE: 0, UNREACHED: 0, UNKNOWN: selected.length },
      omissions: boundedOut ? [{ reason: "node_ceiling", count: allNodes.length - selected.length }] : [],
      truncated: boundedOut,
    });
  }

  const allowed = new Set(selected.map((node) => node.id));
  const edges = (generation?.edges ?? []).filter((edge) => edge.target && edge.resolved !== false && edge.confidenceTier !== "UNRESOLVED" && allowed.has(edge.source) && allowed.has(edge.target)).slice(0, maxEdges);
  const adjacency = new Map();
  for (const edge of edges) {
    if (!adjacency.has(edge.source)) adjacency.set(edge.source, []);
    adjacency.get(edge.source).push(edge);
  }
  for (const rows of adjacency.values()) rows.sort((a, b) => String(a.id).localeCompare(String(b.id)));
  const reached = new Map();
  const queue = admitted.filter((entry) => allowed.has(entry.id)).map((entry) => ({ id: entry.id, hops: 0 }));
  for (const entry of admitted) if (allowed.has(entry.id)) reached.set(entry.id, { parent: null, edge: null, root: entry.id });
  while (queue.length) {
    const current = queue.shift();
    if (current.hops >= maxHops) continue;
    for (const edge of adjacency.get(current.id) ?? []) {
      if (reached.has(edge.target)) continue;
      const root = reached.get(current.id)?.root ?? current.id;
      reached.set(edge.target, { parent: current.id, edge, root });
      queue.push({ id: edge.target, hops: current.hops + 1 });
    }
  }
  const byId = new Map(selected.map((node) => [node.id, node]));
  const livePath = (id) => {
    const ids = [id], edgeRows = [];
    let cursor = id;
    while (reached.get(cursor)?.parent) {
      const state = reached.get(cursor);
      edgeRows.push(state.edge);
      cursor = state.parent;
      ids.push(cursor);
    }
    ids.reverse(); edgeRows.reverse();
    return { ids, evidence: citations([...ids.map((nodeId) => byId.get(nodeId)), ...edgeRows]) };
  };
  const results = selected.map((node) => {
    if (!reached.has(node.id)) return { nodeId: node.id, path: node.path ?? null, state: "UNREACHED", reason: "no_path_from_admitted_entrypoint", evidence: citations([node]), reachabilityPath: [] };
    const path = livePath(node.id);
    return { nodeId: node.id, path: node.path ?? null, state: "LIVE", reason: "evidence_backed_path_from_admitted_entrypoint", evidence: path.evidence, reachabilityPath: path.ids };
  });
  return Object.freeze({
    schemaVersion: 1, kind: "liveness", generationId, sourceState,
    entryPoints: registry.map(({ node, ...entry }) => ({ ...entry, nodeId: node.id })),
    results,
    counts: Object.fromEntries(LIVENESS_STATES.map((state) => [state, results.filter((row) => row.state === state).length])),
    omissions: (generation?.edges?.length ?? 0) > edges.length ? [{ reason: "edge_ceiling_or_out_of_scope", count: (generation?.edges?.length ?? 0) - edges.length }] : [],
    truncated: (generation?.edges?.length ?? 0) > maxEdges,
  });
}
''')

# Flow inventory now consumes the shared registry but retains structural candidates for orientation.
replace_once(
    "blueprint/src/graph/static-provider.mjs",
    'import { semanticAuthorityRankForFact } from "./evidence-authority.mjs";\n',
    'import { semanticAuthorityRankForFact } from "./evidence-authority.mjs";\nimport { buildEntryPointRegistry } from "./entry-points.mjs";\n',
)
replace_once(
    "blueprint/src/graph/static-provider.mjs",
    '''  const outgoing = new Set(generation.edges.map((edge) => edge.source));
  const incoming = new Set(generation.edges.map((edge) => edge.target));
  const entryPoints = generation.nodes
    .filter((node) => node.kind === "symbol" && outgoing.has(node.id) && !incoming.has(node.id))
    .sort((left, right) => compareCanonicalText(left.id, right.id));''',
    '''  const entryPoints = buildEntryPointRegistry(generation, { includeStructuralCandidates: true })
    .map((entry) => entry.node)
    .sort((left, right) => compareCanonicalText(left.id, right.id));''',
)
replace_once(
    "blueprint/src/lib/application/service.mjs",
    'import { projectDocumentTruth } from "../../graph/doc-truth-projection.mjs";\n',
    'import { projectDocumentTruth } from "../../graph/doc-truth-projection.mjs";\nimport { buildLivenessProjection } from "../../graph/liveness.mjs";\n',
)
replace_once(
    "blueprint/src/lib/application/service.mjs",
    '''        if (view === "projection") {
          const generation = loadGeneration(db);''',
    '''        if (view === "liveness") {
          return { ...buildLivenessProjection(loadGeneration(db), {
            sourceState: receipt.freshness === "fresh" ? "clean" : "stale",
            maxNodes: input.maxNodes,
            maxEdges: input.maxEdges,
            maxHops: input.maxHops,
          }), freshnessReceipt: receipt };
        }
        if (view === "projection") {
          const generation = loadGeneration(db);''',
)
replace_once(
    "blueprint/src/lib/application/service.mjs",
    '        if (view !== "summary") fail("architecture_view_invalid", "Architecture view must be summary, flows, projection, or changes.");',
    '        if (view !== "summary") fail("architecture_view_invalid", "Architecture view must be summary, flows, liveness, projection, or changes.");',
)

write("blueprint/tests/liveness.test.mjs", r'''import assert from "node:assert/strict";
import test from "node:test";
import { buildEntryPointRegistry } from "../src/graph/entry-points.mjs";
import { buildLivenessProjection, LIVENESS_STATES } from "../src/graph/liveness.mjs";

const ev = (path) => [{ path, startLine: 1, endLine: 1, contentHash: `${path}-hash` }];
const generation = {
  manifest: { generationId: "g-live", complete: true },
  nodes: [
    { id: "entry", kind: "symbol", path: "src/main.ts", labels: ["Function", "EntryPoint"], evidence: ev("src/main.ts") },
    { id: "live", kind: "symbol", path: "src/live.ts", labels: ["Function"], evidence: ev("src/live.ts") },
    { id: "orphan", kind: "symbol", path: "src/orphan.ts", labels: ["Function"], evidence: ev("src/orphan.ts") },
    { id: "candidate", kind: "symbol", path: "src/candidate.ts", labels: ["Function"], evidence: ev("src/candidate.ts") },
    { id: "leaf", kind: "symbol", path: "src/leaf.ts", labels: ["Function"], evidence: ev("src/leaf.ts") },
  ],
  edges: [
    { id: "e1", source: "entry", target: "live", resolved: true, confidenceTier: "EXACT_RESOLUTION", evidence: ev("src/main.ts") },
    { id: "e2", source: "candidate", target: "leaf", resolved: true, confidenceTier: "EXACT_RESOLUTION", evidence: ev("src/candidate.ts") },
  ],
};

test("entry registry separates explicit evidence from zero-inbound structural candidates", () => {
  const rows = buildEntryPointRegistry(generation);
  assert.equal(rows.find((row) => row.id === "entry").authority, "explicit");
  assert.equal(rows.find((row) => row.id === "candidate").authority, "structural_candidate");
});

test("liveness emits only LIVE UNREACHED UNKNOWN and never calls zero-inbound dead", () => {
  const result = buildLivenessProjection(generation);
  assert.deepEqual(LIVENESS_STATES, ["LIVE", "UNREACHED", "UNKNOWN"]);
  assert.equal(result.results.find((row) => row.nodeId === "entry").state, "LIVE");
  assert.equal(result.results.find((row) => row.nodeId === "live").state, "LIVE");
  assert.equal(result.results.find((row) => row.nodeId === "candidate").state, "UNREACHED");
  assert.equal(result.results.find((row) => row.nodeId === "orphan").state, "UNREACHED");
  assert.ok(result.results.every((row) => !["DEAD", "UNUSED"].includes(row.state)));
  assert.deepEqual(result.results.find((row) => row.nodeId === "live").reachabilityPath, ["entry", "live"]);
});

test("liveness fails to UNKNOWN when source or entrypoint basis is not trustworthy", () => {
  const stale = buildLivenessProjection(generation, { sourceState: "stale" });
  assert.ok(stale.results.every((row) => row.state === "UNKNOWN"));
  const noExplicit = { ...generation, nodes: generation.nodes.map((node) => ({ ...node, labels: (node.labels ?? []).filter((label) => label !== "EntryPoint") })) };
  assert.ok(buildLivenessProjection(noExplicit).results.every((row) => row.state === "UNKNOWN"));
  const incomplete = { ...generation, manifest: { ...generation.manifest, complete: false } };
  assert.ok(buildLivenessProjection(incomplete).results.every((row) => row.state === "UNKNOWN"));
});
''')

commit(
    "feat(blueprint-liveness): add evidence-bound entry registry and liveness states",
    [
      "blueprint/src/graph/entry-points.mjs",
      "blueprint/src/graph/liveness.mjs",
      "blueprint/src/graph/static-provider.mjs",
      "blueprint/src/lib/application/service.mjs",
      "blueprint/tests/liveness.test.mjs",
    ],
    [
      "blueprint/tests/liveness.test.mjs",
      "blueprint/tests/flow-inventory.test.mjs",
      "blueprint/tests/application-service-queries.test.mjs",
      "blueprint/tests/query-runtime-cluster.test.mjs",
    ],
)

# ---------------------------------------------------------------------------
# Commit 3: evidence-backed test recommendations on impact
# ---------------------------------------------------------------------------
write("blueprint/src/graph/test-recommendation.mjs", r'''import { hydrateNodesByIds } from "./store-sqlite.mjs";

function parseEvidence(value) {
  if (Array.isArray(value)) return value;
  try { return JSON.parse(value ?? "[]"); } catch { return []; }
}

function publicEvidence(node, edges) {
  const rows = [], seen = new Set();
  for (const item of [...(node?.evidence ?? []), ...edges.flatMap((edge) => edge.evidence ?? [])]) {
    if (!item) continue;
    const row = { path: item.path ?? null, startLine: item.startLine ?? null, endLine: item.endLine ?? null, contentHash: item.contentHash ?? null };
    const key = JSON.stringify(row);
    if (!seen.has(key)) { seen.add(key); rows.push(row); }
  }
  return rows;
}

export function recommendTestsForImpact(db, { generationId, impactedIds = [], maxRecommendations = 50 } = {}) {
  const targets = [...new Set((impactedIds ?? []).map(String).filter(Boolean))].slice(0, 500);
  const cap = Math.max(1, Math.min(200, Number(maxRecommendations) || 50));
  if (!generationId || !targets.length) return Object.freeze({
    schemaVersion: 1, kind: "test-recommendations", generationId: generationId ?? null,
    recommendations: [], uncoveredImpact: targets, coverage: { impacted: targets.length, covered: 0, ratio: targets.length ? 0 : null },
    omissions: targets.length ? [{ reason: "generation_missing" }] : [{ reason: "no_impacted_symbols" }], minimality: "not_proven", truncated: false,
  });
  const placeholders = targets.map(() => "?").join(",");
  const rows = db.prepare(`SELECT id, source, target, evidence, confidence_tier AS confidenceTier
    FROM edges WHERE generation_id=? AND kind='TESTS' AND target IN (${placeholders}) AND target IS NOT NULL
    ORDER BY source,target,id LIMIT 5000`).all(String(generationId), ...targets)
    .map((row) => ({ ...row, evidence: parseEvidence(row.evidence) }));
  const grouped = new Map();
  for (const edge of rows) {
    if (!grouped.has(edge.source)) grouped.set(edge.source, []);
    grouped.get(edge.source).push(edge);
  }
  const testNodes = new Map(hydrateNodesByIds(db, [...grouped.keys()]).map((node) => [node.id, node]));
  const all = [...grouped.entries()].map(([testId, edges]) => {
    const node = testNodes.get(testId);
    const coveredTargets = [...new Set(edges.map((edge) => edge.target))].sort();
    return {
      testId,
      path: node?.path ?? null,
      name: node?.qualifiedName ?? node?.name ?? testId,
      reason: "first_class_TESTS_edge_covers_impacted_symbol",
      coveredTargets,
      coverageCount: coveredTargets.length,
      evidence: publicEvidence(node, edges),
    };
  }).sort((a, b) => b.coverageCount - a.coverageCount || String(a.path ?? "").localeCompare(String(b.path ?? "")) || a.testId.localeCompare(b.testId));
  const recommendations = all.slice(0, cap);
  const covered = new Set(all.flatMap((row) => row.coveredTargets));
  const uncoveredImpact = targets.filter((id) => !covered.has(id));
  const omissions = [];
  if (!rows.length) omissions.push({ reason: "no_static_test_reachability_evidence" });
  if (all.length > recommendations.length) omissions.push({ reason: "recommendation_ceiling", count: all.length - recommendations.length });
  if (targets.length >= 500 && impactedIds.length > 500) omissions.push({ reason: "impact_target_ceiling", count: impactedIds.length - 500 });
  return Object.freeze({
    schemaVersion: 1, kind: "test-recommendations", generationId,
    recommendations, uncoveredImpact,
    coverage: { impacted: targets.length, covered: covered.size, ratio: targets.length ? covered.size / targets.length : null },
    omissions, minimality: "not_proven", truncated: all.length > recommendations.length || impactedIds.length > 500,
  });
}
''')
replace_once(
    "blueprint/src/lib/application/service.mjs",
    'import { buildLivenessProjection } from "../../graph/liveness.mjs";\n',
    'import { buildLivenessProjection } from "../../graph/liveness.mjs";\nimport { recommendTestsForImpact } from "../../graph/test-recommendation.mjs";\n',
)
old_return = '''          return {
            ...primary,
            seedEnvelope,
            risk,
            slices,
            omissions: [...(primary.omissions ?? []), ...seedEnvelope.omissions],
          };'''
new_return = '''          const testRecommendations = recommendTestsForImpact(db, {
            generationId: meta.manifest.generationId,
            impactedIds: [...seedEnvelope.seeds.map((seed) => seed.id), ...impacted.map((node) => node.id)].filter(Boolean),
            maxRecommendations: input.maxTestRecommendations,
          });
          return {
            ...primary,
            seedEnvelope,
            risk,
            slices,
            testRecommendations,
            omissions: [...(primary.omissions ?? []), ...seedEnvelope.omissions, ...testRecommendations.omissions],
          };'''
replace_once("blueprint/src/lib/application/service.mjs", old_return, new_return)
replace_once(
    "blueprint/tests/application-service-queries.test.mjs",
    '    assert.ok(Array.isArray(serviceResult.edges ?? serviceResult.affected ?? []));',
    '    assert.ok(Array.isArray(serviceResult.edges ?? serviceResult.affected ?? []));\n    assert.equal(serviceResult.testRecommendations?.minimality, "not_proven");\n    assert.ok(Array.isArray(serviceResult.testRecommendations?.uncoveredImpact));',
)

write("blueprint/tests/test-recommendation.test.mjs", r'''import assert from "node:assert/strict";
import test from "node:test";
import { openStore, bulkInsertGeneration } from "../src/graph/store-sqlite.mjs";
import { recommendTestsForImpact } from "../src/graph/test-recommendation.mjs";

const ev = (path) => [{ path, startLine: 1, endLine: 2, contentHash: `${path}-hash` }];

test("test recommendations use first-class TESTS evidence and report uncovered impact", () => {
  const db = openStore(":memory:");
  try {
    bulkInsertGeneration(db, {
      nodes: [
        { id: "test:a", kind: "symbol", labels: ["Test"], name: "testA", qualifiedName: "testA", path: "tests/a.test.ts", confidence: null, evidence: ev("tests/a.test.ts") },
        { id: "prod:a", kind: "symbol", labels: ["Function"], name: "a", qualifiedName: "a", path: "src/a.ts", confidence: null, evidence: ev("src/a.ts") },
        { id: "prod:b", kind: "symbol", labels: ["Function"], name: "b", qualifiedName: "b", path: "src/b.ts", confidence: null, evidence: ev("src/b.ts") },
      ],
      edges: [{ id: "tests:a", kind: "TESTS", source: "test:a", target: "prod:a", confidence: null, confidenceTier: "EXACT_RESOLUTION", evidence: ev("tests/a.test.ts") }],
      manifest: { generationId: "g-tests" },
      provider: { id: "blueprint-static" },
    });
    const result = recommendTestsForImpact(db, { generationId: "g-tests", impactedIds: ["prod:a", "prod:b"] });
    assert.equal(result.recommendations.length, 1);
    assert.equal(result.recommendations[0].testId, "test:a");
    assert.deepEqual(result.recommendations[0].coveredTargets, ["prod:a"]);
    assert.deepEqual(result.uncoveredImpact, ["prod:b"]);
    assert.deepEqual(result.coverage, { impacted: 2, covered: 1, ratio: 0.5 });
    assert.equal(result.minimality, "not_proven");
    assert.ok(result.recommendations[0].evidence.length > 0);
  } finally { db.close(); }
});

test("absence of TESTS evidence is an omission, never a claim that no tests exist", () => {
  const db = openStore(":memory:");
  try {
    bulkInsertGeneration(db, {
      nodes: [{ id: "prod:a", kind: "symbol", labels: ["Function"], name: "a", qualifiedName: "a", path: "src/a.ts", confidence: null, evidence: ev("src/a.ts") }],
      edges: [], manifest: { generationId: "g-empty" }, provider: { id: "blueprint-static" },
    });
    const result = recommendTestsForImpact(db, { generationId: "g-empty", impactedIds: ["prod:a"] });
    assert.deepEqual(result.recommendations, []);
    assert.deepEqual(result.uncoveredImpact, ["prod:a"]);
    assert.ok(result.omissions.some((row) => row.reason === "no_static_test_reachability_evidence"));
    assert.equal(result.minimality, "not_proven");
  } finally { db.close(); }
});
''')

commit(
    "feat(blueprint-impact): recommend tests from static reachability evidence",
    [
      "blueprint/src/graph/test-recommendation.mjs",
      "blueprint/src/lib/application/service.mjs",
      "blueprint/tests/test-recommendation.test.mjs",
      "blueprint/tests/application-service-queries.test.mjs",
    ],
    [
      "blueprint/tests/test-recommendation.test.mjs",
      "blueprint/tests/application-service-queries.test.mjs",
      "blueprint/tests/query-runtime-cluster.test.mjs",
      "blueprint/tests/treesitter-provider.test.mjs",
    ],
)

# Consume the transport and restore branch CI to read-only before pushing.
workflow = ROOT / ".github/workflows/blueprint-completion.yml"
w = workflow.read_text()
w = w.replace('    permissions:\n      contents: write\n', '')
w = w.replace('      - name: Apply reviewed baseline closure\n        run: python3 .github/blueprint-completion-input/apply.py\n', '')
workflow.write_text(w)
subprocess.run(["git", "rm", "-r", ".github/blueprint-completion-input"], check=True)
subprocess.run(["git", "add", str(workflow.relative_to(ROOT))], check=True)
subprocess.run(["git", "commit", "-m", "ci(blueprint): remove truth-liveness-test transport"], check=True)
subprocess.run(["git", "push", "origin", "HEAD:blueprint-completion"], check=True)
