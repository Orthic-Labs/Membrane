import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { applyFileDelta } from "../src/graph/delta-store.mjs";
import { stableRead } from "../src/graph/stable-read.mjs";
import { buildGraphGeneration, parseFileFacts, scanSourcesPublic } from "../src/graph/static-provider.mjs";
import { closeStore, getGenerationEnvelope, openStore, saveGeneration, searchGenerationSymbols } from "../src/graph/store-sqlite.mjs";
import { compareRepoPaths, normalizeRepoPath } from "../src/graph/path-order.mjs";
import { canonicalProviderId } from "../src/graph/provider-identity.mjs";

const FIXTURE = join(import.meta.dirname, "..", "evals/fixture-repos/typescript-commerce");

function makeRepo() {
  const repo = mkdtempSync(join(tmpdir(), "blueprint-delta-"));
  cpSync(FIXTURE, repo, { recursive: true });
  buildGraphGeneration(repo, { outDir: ".agent", persist: true });
  return repo;
}

function readDelta(repo, path, eventKind = "modify", renameTo = null) {
  const source = scanSourcesPublic(repo);
  const absPath = join(repo, renameTo ?? path);
  const read = eventKind === "delete" ? null : stableRead(absPath);
  const descriptor = read ? {
    absolutePath: absPath,
    path: renameTo ?? path,
    text: read.bytes.toString("utf8"),
    lines: read.bytes.toString("utf8").split(/\r?\n/),
    contentHash: read.contentDigest.replace(/^xxh128:/, ""),
    size: read.bytes.length,
  } : null;
  const files = (source.files ?? []).filter((file) => file.path !== path);
  if (descriptor) files.push(descriptor);
  return {
    eventKind,
    path,
    renameTo,
    parsed: descriptor ? parseFileFacts(repo, descriptor, { files }) : null,
    contentDigest: read?.contentDigest ?? null,
    fileIdentity: read?.fileIdentity ?? null,
    size: read?.bytes.length ?? 0,
    mtimeMs: read?.statAfter?.mtimeMs ?? null,
    provider: { id: "lexical", version: "repo-local-delta-v1" },
  };
}

function openDb(repo) { return openStore(join(repo, ".agent/graph/graph.db")); }

test("file edit replaces only owned facts and leaves unrelated rows unchanged", () => {
  const repo = makeRepo();
  const db = openDb(repo);
  try {
    const stableRows = (sql) => db.prepare(sql).all().map(({ node_ordinal, ...row }) => row);
    const before = {
      file: stableRows("SELECT * FROM files WHERE path='src/store.ts'"),
      symbols: stableRows("SELECT * FROM symbols WHERE path='src/store.ts' ORDER BY rowid"),
      edges: stableRows("SELECT * FROM edges WHERE source LIKE 'file:src/store.ts%' OR source LIKE 'symbol:src/store.ts%' ORDER BY rowid"),
    };
    writeFileSync(join(repo, "src/service.ts"), `${readFileSync(join(repo, "src/service.ts"), "utf8")}\nexport const changed = true;\n`);
    const result = applyFileDelta(db, readDelta(repo, "src/service.ts"));
    const ordinals = db.prepare("SELECT node_ordinal FROM files UNION ALL SELECT node_ordinal FROM symbols UNION ALL SELECT node_ordinal FROM annotation_nodes ORDER BY node_ordinal").all().map((row) => row.node_ordinal);
    assert.deepEqual(ordinals, ordinals.map((_, index) => index));
    assert.ok(result.orderWrites <= ordinals.length);
    assert.deepEqual({
      file: stableRows("SELECT * FROM files WHERE path='src/store.ts'"),
      symbols: stableRows("SELECT * FROM symbols WHERE path='src/store.ts' ORDER BY rowid"),
      edges: stableRows("SELECT * FROM edges WHERE source LIKE 'file:src/store.ts%' OR source LIKE 'symbol:src/store.ts%' ORDER BY rowid"),
    }, before);
  } finally { closeStore(db); rmSync(repo, { recursive: true, force: true }); }
});

test("structural delta reindexes symbols under its resealed generation", () => {
  const repo = makeRepo();
  const db = openDb(repo);
  try {
    writeFileSync(join(repo, "src/service.ts"), `${readFileSync(join(repo, "src/service.ts"), "utf8")}\nexport const deltaSearchNeedle = true;\n`);
    applyFileDelta(db, readDelta(repo, "src/service.ts"));
    const generationId = getGenerationEnvelope(db)?.manifest?.generationId;
    const rows = searchGenerationSymbols(db, generationId, ["needle"], 4);
    assert.equal(rows.length, 1);
    assert.equal(rows[0].name, "deltaSearchNeedle");
    assert.equal(rows[0].generationId, generationId);
  } finally { closeStore(db); rmSync(repo, { recursive: true, force: true }); }
});

test("semantic-domain ownership survives structural refresh", () => {
  const repo = makeRepo();
  const db = openDb(repo);
  try {
    db.prepare("INSERT INTO fact_owner(fact_id, fact_kind, source_path, source_digest, provider_id, provider_version, freshness_domain, fact_kind_detail) VALUES (?, 'node', ?, ?, 'phase2', 'v1', 'semantic', 'verdict')").run("semantic:service", "src/service.ts", "xxh128:semantic");
    const before = db.prepare("SELECT * FROM fact_owner WHERE fact_id='semantic:service'").get();
    writeFileSync(join(repo, "src/service.ts"), `${readFileSync(join(repo, "src/service.ts"), "utf8")}\nexport const structuralRefresh = true;\n`);
    applyFileDelta(db, readDelta(repo, "src/service.ts"));
    assert.deepEqual(db.prepare("SELECT * FROM fact_owner WHERE fact_id='semantic:service'").get(), before);
  } finally { closeStore(db); rmSync(repo, { recursive: true, force: true }); }
});

test("delete removes owned facts and marks inbound Synapses unresolved", () => {
  const repo = makeRepo();
  const db = openDb(repo);
  try {
    const inbound = db.prepare("SELECT id FROM edges WHERE source='file:src/routes.ts' AND target='file:src/service.ts'").get();
    const symbolIds = db.prepare("SELECT id FROM symbols WHERE path='src/service.ts'").all().map((row) => row.id);
    assert.ok(inbound);
    applyFileDelta(db, readDelta(repo, "src/service.ts", "delete"));
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM files WHERE path='src/service.ts'").get().n, 0);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM fact_owner WHERE source_path='src/service.ts'").get().n, 0);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM symbol_terms WHERE symbol_id IN (SELECT value FROM json_each(?))").get(JSON.stringify(symbolIds)).n, 0);
    assert.equal(db.prepare("SELECT resolved FROM edges WHERE id=?").get(inbound.id).resolved, 0);
  } finally { closeStore(db); rmSync(repo, { recursive: true, force: true }); }
});

test("same-bytes delta is a no-op", () => {
  const repo = makeRepo();
  const db = openDb(repo);
  try {
    const before = db.prepare("SELECT value FROM generation WHERE key='manifest'").get().value;
    const result = applyFileDelta(db, readDelta(repo, "src/service.ts"));
    assert.equal(result.noop, true);
    assert.equal(db.prepare("SELECT value FROM generation WHERE key='manifest'").get().value, before);
  } finally { closeStore(db); rmSync(repo, { recursive: true, force: true }); }
});

test("rename removes old path and leaves inbound edge unresolved until repair", () => {
  const repo = makeRepo();
  const db = openDb(repo);
  try {
    const oldPath = join(repo, "src/service.ts");
    const newPath = join(repo, "src/service-renamed.ts");
    const contents = readFileSync(oldPath, "utf8");
    rmSync(oldPath);
    writeFileSync(newPath, contents);
    const result = applyFileDelta(db, readDelta(repo, "src/service.ts", "rename", "src/service-renamed.ts"));
    assert.equal(result.applied, true);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM files WHERE path='src/service.ts'").get().n, 0);
    assert.equal(db.prepare("SELECT COUNT(*) AS n FROM files WHERE path='src/service-renamed.ts'").get().n, 1);
    const edge = db.prepare("SELECT resolved FROM edges WHERE source='file:src/routes.ts' AND target='file:src/service.ts'").get();
    assert.equal(edge.resolved, 0);
  } finally { closeStore(db); rmSync(repo, { recursive: true, force: true }); }
});

test("injected delta failure rolls back all rows", () => {
  const repo = makeRepo();
  const db = openDb(repo);
  try {
    const before = db.prepare("SELECT path, content_hash, node_ordinal FROM files ORDER BY path").all();
    writeFileSync(join(repo, "src/service.ts"), `${readFileSync(join(repo, "src/service.ts"), "utf8")}\nexport const rollbackProbe = true;\n`);
    assert.throws(() => applyFileDelta(db, readDelta(repo, "src/service.ts"), { beforeCommit: () => { throw new Error("post-reindex"); } }), /post-reindex/);
    assert.deepEqual(db.prepare("SELECT path, content_hash, node_ordinal FROM files ORDER BY path").all(), before);
  } finally { closeStore(db); rmSync(repo, { recursive: true, force: true }); }
});

test("path byte order & inTransaction misuse stay typed", () => {
  assert.deepEqual(["ä.ts", "a.ts", "A.ts"].sort(compareRepoPaths), ["A.ts", "a.ts", "ä.ts"]);
  assert.equal(normalizeRepoPath("./dir\\e\u0301.ts"), "dir/é.ts");
  assert.notEqual(compareRepoPaths("é.ts", "e\u0301.ts"), 0, "raw UTF-8 tie-break makes comparator total");
  assert.equal(canonicalProviderId("blueprint-static"), "lexical");
  assert.equal(canonicalProviderId("blueprint-treesitter"), "treesitter");
  assert.equal(canonicalProviderId("my-treesitter-adapter"), "my-treesitter-adapter");
  const repo = makeRepo(), db = openDb(repo);
  try { assert.throws(() => applyFileDelta(db, readDelta(repo, "src/service.ts"), { inTransaction: true }), (error) => error.code === "delta_transaction_required"); }
  finally { closeStore(db); rmSync(repo, { recursive: true, force: true }); }
});

test("document modify replaces scoped structural facts and refreshes counts atomically", () => {
  const db = openStore(":memory:");
  const file = { id: "file:docs/a.md", kind: "file", labels: ["File"], name: "a.md", qualifiedName: "docs/a.md", path: "docs/a.md", confidence: 1, evidence: [{ path: "docs/a.md", contentHash: "before" }] };
  const stale = { id: "symbol:docs/a.md::stale", kind: "symbol", labels: ["Heading"], name: "stale", qualifiedName: "stale", path: "docs/a.md", confidence: 1, evidence: [] };
  const fresh = { ...stale, id: "symbol:docs/a.md::fresh", name: "fresh", qualifiedName: "fresh" };
  const freshTwo = { ...fresh, id: "symbol:docs/a.md::freshTwo", name: "freshTwo", qualifiedName: "freshTwo" };
  try {
    saveGeneration(db, { manifest: { generationId: "doc-gen", counts: { nodes: 2, edges: 0 } }, provider: { id: "lexical", version: "1" }, nodes: [file, stale], edges: [] }, { populateState: true });
    db.prepare(`INSERT INTO fact_owner(fact_id,fact_kind,source_path,source_digest,provider_id,provider_version,freshness_domain,fact_kind_detail,generation_id,repo_root)
      VALUES ('semantic:keep','node','docs/a.md','xxh128:semantic','phase2','1','semantic','verdict','doc-gen',NULL)`).run();
    applyFileDelta(db, { eventKind: "modify", domain: "doc", path: "docs/a.md", contentDigest: "after", sourceClock: 2, provider: { id: "doctruth", version: "1" },
      factBatches: [{ provider: { id: "lexical", version: "1" }, parsed: { nodes: [{ ...file, evidence: [{ path: "docs/a.md", contentHash: "after" }] }, fresh, freshTwo], edges: [] } }],
      document: { claims: [{ id: "claim:fresh", source: "docs/a.md", line: 1, status: "current", text: "fresh" }], codeRefs: [], lifecycle: null } });
    assert.equal(db.prepare("SELECT 1 FROM symbols WHERE id=?").get(stale.id), undefined);
    assert.equal(db.prepare("SELECT 1 FROM symbol_terms WHERE symbol_id=?").get(stale.id), undefined);
    assert.ok(db.prepare("SELECT 1 FROM node_provider WHERE node_id=?").get(fresh.id));
    assert.ok(db.prepare("SELECT 1 FROM fact_owner WHERE fact_id='semantic:keep'").get());
    assert.ok(db.prepare("SELECT 1 FROM fact_owner WHERE fact_id='claim:fresh' AND provider_id='doctruth'").get());
    const counts = getGenerationEnvelope(db).manifest.counts;
    assert.equal(counts.nodes, db.prepare("SELECT (SELECT COUNT(*) FROM files)+(SELECT COUNT(*) FROM symbols)+(SELECT COUNT(*) FROM annotation_nodes) n").get().n);
    db.prepare("INSERT INTO dependency_index(source_path,dependent_path,reason) VALUES ('docs/a.md','src/user.ts','config')").run();
    applyFileDelta(db, { eventKind: "delete", domain: "doc", path: "docs/a.md", sourceClock: 3, provider: { id: "doctruth", version: "1" } });
    for (const table of ["files", "symbols", "node_provider", "fact_owner", "file_state"]) assert.equal(db.prepare(`SELECT COUNT(*) n FROM ${table} WHERE ${["node_provider", "fact_owner"].includes(table) ? "source_path" : "path"}='docs/a.md'`).get().n, 0, table);
    assert.equal(db.prepare("SELECT COUNT(*) n FROM dependency_index WHERE source_path='docs/a.md' OR dependent_path='docs/a.md'").get().n, 0);
    assert.equal(getGenerationEnvelope(db).manifest.counts.nodes, 0);
  } finally { closeStore(db); }
});
