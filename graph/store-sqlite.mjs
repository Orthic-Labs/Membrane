// SQLite graph store — the single persisted generation store for Cortex.
// Build-time providers produce the generation-shaped input; this store persists
// its envelope and relational rows, and indexed queries read it without loading
// the whole graph into JavaScript.
//
// Runtime: `node:sqlite` (`DatabaseSync`) — a Node v22.5+/v24+ builtin, no
// dependency. This module writes ONLY inside the sqlite file path it is
// explicitly given (or ':memory:'); it never resolves or writes outside that.

import { DatabaseSync } from "node:sqlite";
import { join } from "node:path";
import { statSync } from "node:fs";
import { computeFullLedger } from "./merkle-ledger.mjs";

// Derived from MIGRATIONS below, never hardcoded: a literal here silently
// desyncs the moment a migration is appended, and migrate() would then stop
// short of applying it. Defined after MIGRATIONS (see end of the migrate block).

// ---------------------------------------------------------------------------
// Open / init / migrate
// ---------------------------------------------------------------------------

// Opens (creating if absent) the sqlite file at `dbPath`, or an in-memory
// database when `dbPath` is ":memory:" or omitted. Always runs migrate()
// before returning, so every caller gets a ready-to-use store.
export function openStore(dbPath = ":memory:") {
  const db = new DatabaseSync(dbPath);
  db.exec("PRAGMA foreign_keys = ON;");
  if (dbPath !== ":memory:") {
    db.exec("PRAGMA journal_mode = WAL;");
    // WAL permits one writer at a time; without a busy_timeout a second writer gets an
    // immediate SQLITE_BUSY instead of queueing. That is exactly how the fleet watcher died
    // in production: an actor's initialize raced a concurrent `cortex build` (or the context
    // hook) on the same repo's graph.db and threw "database is locked" — serial tests never
    // exercise the race. Five seconds did NOT cover it: on 2026-08-03 a repo under a
    // concurrent release build still threw "database is locked" out of markGap() with the
    // 5s timeout in place. A background watcher has no deadline worth failing for, so it
    // waits far longer rather than surfacing a spurious lock error.
    db.exec("PRAGMA busy_timeout = 30000;");
  }
  migrate(db);
  return db;
}

/**
 * Read-only handle for DOWNSTREAM CONSUMERS (membrane's freshness evaluator, and
 * anything else on a latency budget that must never mutate the store).
 *
 * Deliberately does NOT run migrate(): a reader must never upgrade someone
 * else's schema, and on an older store it must fail loudly rather than silently
 * rewrite it. Callers compare `storeSchemaVersion` against the SCHEMA_VERSION
 * they pinned and decide for themselves.
 *
 * Concurrency contract: the store is WAL, so a reader opened here sees the last
 * COMMITTED generation while `cortex build` writes, and never a torn
 * envelope — saveGeneration writes rows and envelope inside transactions.
 */
export function openStoreReadOnly(dbPath) {
  const db = new DatabaseSync(dbPath, { readOnly: true });
  // Readers rarely block under WAL, but they CAN hit SQLITE_BUSY during a checkpoint;
  // waiting briefly is always better than a spurious failure on a latency-budget path.
  db.exec("PRAGMA busy_timeout = 5000;");
  return db;
}

export function closeStore(db) {
  db.close();
}

/**
 * The cheap freshness surface: everything a downstream consumer needs to pin a
 * generation, and nothing that costs a materialisation.
 *
 * Explicitly EXCLUDES docTruth. That single envelope row is ~8.5 MB on this
 * workspace and costs ~205 ms to read, which alone would blow a 900 ms prompt
 * hook; the rest of the envelope reads in ~1 ms. Returns null when no generation
 * has been sealed, which is distinct from a torn or corrupt one (that throws).
 */
export function readManifestEnvelope(db) {
  const get = (key) => {
    const row = db.prepare("SELECT value FROM generation WHERE key = ?").get(key);
    if (!row) return undefined;
    try {
      return JSON.parse(row.value);
    } catch {
      throw new Error(`graph store envelope key "${key}" is not valid JSON`);
    }
  };
  const manifest = get("manifest");
  if (!manifest) return null;
  const versionRow = db.prepare("SELECT value FROM meta WHERE key = 'schema_version'").get();
  return {
    storeSchemaVersion: versionRow ? Number(versionRow.value) : 0,
    schemaVersion: get("schemaVersion") ?? null,
    generationId: manifest.generationId ?? null,
    manifestDigest: manifest.manifestDigest ?? null,
    provider: manifest.provider ?? null,
    lexicalProvider: manifest.lexicalProvider ?? null,
    providerComposition: manifest.providerComposition ?? null,
    complete: manifest.complete === true,
    fileLimit: manifest.fileLimit ?? 0,
    repo: manifest.repo ?? null,
    counts: manifest.counts ?? null,
    // "built at commit X, clean or dirty" — the distinction membrane uses to
    // refuse treating a dirty-overlay build as a committed snapshot.
    sourceObservation: get("sourceObservation") ?? null,
    repoRoot: get("repoRoot") ?? null,
  };
}

// Versioned migration runner. MIGRATIONS[n] takes the schema from version n
// to version n+1. Idempotent: re-running migrate() on an already-current
// store is a no-op (single SELECT to check the recorded version).
const MIGRATIONS = [
  // 0 -> 1: initial schema.
  (db) => {
    db.exec(`
      CREATE TABLE IF NOT EXISTS meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
      );

      CREATE TABLE IF NOT EXISTS files (
        path TEXT PRIMARY KEY,
        content_hash TEXT,
        language TEXT,
        provider TEXT,
        parse_status TEXT,
        error_node_count INTEGER,
        generation_id TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_files_generation ON files(generation_id);

      CREATE TABLE IF NOT EXISTS symbols (
        id TEXT PRIMARY KEY,
        kind TEXT NOT NULL,
        labels TEXT NOT NULL,
        name TEXT NOT NULL,
        qualified_name TEXT NOT NULL,
        path TEXT NOT NULL,
        confidence REAL NOT NULL,
        evidence TEXT NOT NULL,
        generation_id TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_symbols_path ON symbols(path);
      CREATE INDEX IF NOT EXISTS idx_symbols_generation ON symbols(generation_id);

      CREATE TABLE IF NOT EXISTS edges (
        id TEXT PRIMARY KEY,
        kind TEXT NOT NULL,
        source TEXT NOT NULL,
        target TEXT,
        confidence REAL NOT NULL,
        resolved INTEGER NOT NULL DEFAULT 1,
        specifier TEXT,
        evidence TEXT NOT NULL,
        generation_id TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source);
      CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);
      CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind);
      CREATE INDEX IF NOT EXISTS idx_edges_generation ON edges(generation_id);
    `);
  },
  // Migration 2 — optional embedding storage + brute-force similarity search.
  //
  // Deliberately NOT sqlite-vec. `node:sqlite` does expose loadExtension(), so the
  // extension is technically loadable, but it ships platform-specific native
  // binaries — the same portability problem that made web-tree-sitter the choice
  // over native tree-sitter, since this workspace also runs on Windows.
  //
  // Brute-force cosine in JS was measured on this machine at 20,000 vectors x 384
  // dims in 41ms, which is comfortably fast at repo-symbol scale. An index only
  // earns its complexity somewhere past ~500k vectors. Do not "optimise" this into
  // a native dependency without re-measuring and confirming Windows support.
  //
  // Embeddings are OFF by default and nothing in cortex generates them yet.
  // This table exists so the capability is ready the moment it is turned on; every
  // function here must behave correctly on an empty table.
  (db) => {
    db.exec(`
      CREATE TABLE IF NOT EXISTS vectors (
        node_id TEXT PRIMARY KEY,
        dim INTEGER NOT NULL,
        emb BLOB NOT NULL,
        model TEXT NOT NULL,
        generation_id TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_vectors_generation ON vectors(generation_id);
      CREATE INDEX IF NOT EXISTS idx_vectors_model ON vectors(model);
    `);
  },
  // Migration 3 — make the store SUFFICIENT, so it can be the only store.
  //
  // Until now this was a query cache beside an authoritative graph.json, so it
  // only kept the fields queries needed. A file node lost its id/labels/name/
  // confidence/evidence, and the generation envelope (provider, manifest,
  // docTruth, repoRoot) was not persisted at all — the store could answer
  // questions about a generation but could not reproduce one.
  //
  // These columns plus the `generation` envelope table close that gap:
  // saveGeneration/loadGeneration now round-trip byte-equivalent generations,
  // which is what lets graph.json be deleted rather than merely duplicated.
  // The `extra` columns are the durable part of this lesson. B3 added
  // `confidenceTier` to every edge and this schema silently dropped it, because
  // a fixed column list can only persist the fields it was told about. `extra`
  // holds every key a provider emits beyond the indexed ones, so a new field is
  // preserved without a migration and — critically — presence is preserved:
  // `resolved`, `specifier` and `reason` appear on SOME edges, and a nullable
  // column cannot distinguish "absent" from "present and null".
  (db) => {
    db.exec(`
      ALTER TABLE files ADD COLUMN node_id TEXT;
      ALTER TABLE files ADD COLUMN labels TEXT;
      ALTER TABLE files ADD COLUMN name TEXT;
      ALTER TABLE files ADD COLUMN qualified_name TEXT;
      ALTER TABLE files ADD COLUMN confidence REAL;
      ALTER TABLE files ADD COLUMN evidence TEXT;
      ALTER TABLE files ADD COLUMN extra TEXT;

      ALTER TABLE symbols ADD COLUMN extra TEXT;

      ALTER TABLE edges ADD COLUMN confidence_tier TEXT;
      ALTER TABLE edges ADD COLUMN extra TEXT;
      CREATE INDEX IF NOT EXISTS idx_edges_tier ON edges(confidence_tier);

      CREATE TABLE IF NOT EXISTS generation (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
      );
    `);
  },
  // Migration 4 — per-file ownership and Merkle freshness state.
  (db) => {
    db.exec(`
      CREATE TABLE IF NOT EXISTS file_state (
        path TEXT PRIMARY KEY,
        content_digest TEXT NOT NULL,
        size INTEGER NOT NULL,
        mtime_ms REAL,
        file_identity TEXT,
        last_event_seq INTEGER,
        applied_clock INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS fact_owner (
        fact_id TEXT NOT NULL,
        fact_kind TEXT NOT NULL CHECK (fact_kind IN ('node','edge')),
        source_path TEXT NOT NULL,
        source_digest TEXT NOT NULL,
        provider_id TEXT NOT NULL,
        provider_version TEXT NOT NULL,
        freshness_domain TEXT NOT NULL CHECK (freshness_domain IN ('structural','doc','semantic')),
        fact_kind_detail TEXT,
        PRIMARY KEY (fact_id, fact_kind, provider_id)
      );
      CREATE INDEX IF NOT EXISTS idx_fact_owner_path ON fact_owner(source_path);
      CREATE INDEX IF NOT EXISTS idx_fact_owner_domain ON fact_owner(freshness_domain);
      CREATE TABLE IF NOT EXISTS dependency_index (
        source_path TEXT NOT NULL,
        dependent_path TEXT NOT NULL,
        reason TEXT NOT NULL CHECK (reason IN ('import','call','route','schema','config','manifest')),
        PRIMARY KEY (source_path, dependent_path, reason)
      );
      CREATE INDEX IF NOT EXISTS idx_dep_source ON dependency_index(source_path);
      CREATE TABLE IF NOT EXISTS generation_leaf (
        path TEXT PRIMARY KEY,
        kind TEXT NOT NULL CHECK (kind IN ('file','dir')),
        digest TEXT NOT NULL
      );
    `);
  },
  // Migration 5 — durable watcher clocks, cursor/gap state, and event journal.
  (db) => {
    db.exec(`
      CREATE TABLE IF NOT EXISTS watch_state (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS event_journal (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        observed_ms INTEGER NOT NULL,
        event_kind TEXT NOT NULL CHECK (event_kind IN ('create','modify','delete','rename')),
        path TEXT NOT NULL,
        rename_to TEXT,
        source_clock INTEGER NOT NULL,
        applied INTEGER NOT NULL DEFAULT 0 CHECK (applied IN (0,1,2)),
        applied_clock INTEGER,
        UNIQUE(observed_ms, event_kind, path, rename_to, source_clock)
      );
      CREATE INDEX IF NOT EXISTS idx_event_journal_applied ON event_journal(applied, seq);
    `);
  },
  // Migration 6 — query-time freshness barrier receipts.
  (db) => {
    db.exec(`
      CREATE TABLE IF NOT EXISTS generation_receipt (
        receipt_id TEXT PRIMARY KEY,
        created_ms INTEGER NOT NULL,
        repo_root TEXT NOT NULL,
        generation_id TEXT,
        source_clock INTEGER NOT NULL,
        applied_clock INTEGER NOT NULL,
        event_gap INTEGER NOT NULL,
        barrier_result TEXT NOT NULL CHECK (barrier_result IN ('caught_up','gap_blocked','timeout')),
        details_json TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_generation_receipt_created ON generation_receipt(created_ms);
    `);
  },
  // Migration 7 — derived artifact bindings.
  (db) => {
    db.exec(`
      CREATE TABLE IF NOT EXISTS artifact_state (
        artifact TEXT PRIMARY KEY,
        generation_id TEXT NOT NULL,
        fingerprint TEXT NOT NULL,
        updated_ms INTEGER NOT NULL
      );
    `);
  },
  // Migration 8 — direct-parent Merkle lookups for ancestor-only updates.
  (db) => {
    db.exec(`
      ALTER TABLE generation_leaf ADD COLUMN parent_path TEXT;
      ALTER TABLE generation_leaf ADD COLUMN name TEXT;
      CREATE INDEX IF NOT EXISTS idx_generation_leaf_parent ON generation_leaf(parent_path, name);
    `);
    const update = db.prepare("UPDATE generation_leaf SET parent_path=?, name=? WHERE path=?");
    for (const row of db.prepare("SELECT path FROM generation_leaf").all()) {
      const path = String(row.path).replaceAll("\\", "/");
      const split = path.lastIndexOf("/");
      update.run(path === "" ? null : split >= 0 ? path.slice(0, split) : "", path === "" ? "" : split >= 0 ? path.slice(split + 1) : path, row.path);
    }
  },
  // Migration 9 — generation-pinned FTS symbol index.
  //
  // FTS5 is present in supported Node/Python SQLite builds. The core-table
  // fallback keeps old/minimal SQLite builds readable; callers can detect it
  // and use their existing bounded lexical fallback instead of crashing.
  (db) => {
    try {
      db.exec("CREATE VIRTUAL TABLE IF NOT EXISTS symbol_search USING fts5(id UNINDEXED, generation_id UNINDEXED, name, qualified_name, path)");
    } catch {
      db.exec(`
        CREATE TABLE IF NOT EXISTS symbol_search (
          id TEXT NOT NULL,
          generation_id TEXT NOT NULL,
          name TEXT NOT NULL,
          qualified_name TEXT NOT NULL,
          path TEXT NOT NULL,
          PRIMARY KEY (id, generation_id)
        );
        CREATE INDEX IF NOT EXISTS idx_symbol_search_generation ON symbol_search(generation_id, id);
      `);
    }
  },
  // Migration 10 — portable cold-path symbol term index.
  //
  // FTS can fault many external-volume pages on its first query. This compact
  // primary-key B-tree serves Membrane's one longest-token lookup without FTS
  // ranking work; `*` provides a deterministic valid-generation fallback.
  (db) => {
    db.exec(`
      CREATE TABLE IF NOT EXISTS symbol_terms (
        generation_id TEXT NOT NULL,
        token TEXT NOT NULL,
        symbol_id TEXT NOT NULL,
        PRIMARY KEY (generation_id, token, symbol_id)
      ) WITHOUT ROWID;
      CREATE INDEX IF NOT EXISTS idx_symbol_terms_symbol ON symbol_terms(symbol_id);
    `);
  },
  // Migration 11 — repair stores stamped by the brief-lived pre-FTS ordering.
  // Kept idempotent so a store that already received migration 10 is unchanged.
  (db) => {
    db.exec(`
      CREATE TABLE IF NOT EXISTS symbol_terms (
        generation_id TEXT NOT NULL,
        token TEXT NOT NULL,
        symbol_id TEXT NOT NULL,
        PRIMARY KEY (generation_id, token, symbol_id)
      ) WITHOUT ROWID;
      CREATE INDEX IF NOT EXISTS idx_symbol_terms_symbol ON symbol_terms(symbol_id);
    `);
  },
  // Migration 12 — derived_fact_owner lineage: which generation and which
  // repository produced each fact. `fact_owner` already tracks per-fact
  // provider ownership (Migration 4); this adds the two columns the
  // multi-repo resident Watchman needs so a fact can be attributed without
  // re-deriving it from files/symbols or assuming "this DB == one repo"
  // holds for every consumer. repo_root is the canonical (realpath'd) root
  // the owning generation was built from; generation_id is the manifest
  // generationId active when the row was last written (full build or delta).
  (db) => {
    db.exec(`
      ALTER TABLE fact_owner ADD COLUMN generation_id TEXT;
      ALTER TABLE fact_owner ADD COLUMN repo_root TEXT;
      CREATE INDEX IF NOT EXISTS idx_fact_owner_generation ON fact_owner(generation_id);
      CREATE INDEX IF NOT EXISTS idx_fact_owner_repo_root ON fact_owner(repo_root);
    `);
  },
  // Migration 13 — compound indexes for indexed traversal.
  //
  // Phase 7.1 of the context-stack plan moves the neighbor/path frontier out of
  // JavaScript into a recursive CTE that joins `edges` on (generation_id, source/target).
  // Without a compound index, SQLite falls back to scanning every edge in the
  // generation for each iteration of the recursion. The compound indexes below
  // let the planner drive the CTE from a generation-bounded seek on the seed
  // ids, instead of a full scan. Created idempotently so a store rebuilt by an
  // older build picks them up on next open without conflicting with a future
  // schema that already names them.
  (db) => {
    db.exec(`
      CREATE INDEX IF NOT EXISTS idx_edges_gen_source_kind ON edges(generation_id, source, kind);
      CREATE INDEX IF NOT EXISTS idx_edges_gen_target_kind ON edges(generation_id, target, kind);
    `);
  },
  // Migration 14 — relational docTruth (Phase 7.3).
  //
  // Replaces the single ~8.5 MB envelope blob with four generation-pinned tables:
  // documents, claims, claim_code_edges, document_supersession. Every row carries
  // its own generation_id so multi-generation stores stay queryable per-generation.
  // Body prose stays in the source files; these tables hold structure and spans
  // only. A downstream consumer can now ask for `claims` / `claim_code_edges`
  // for one generation in a SQL seek instead of paying the full envelope parse.
  //
  // Idempotent on an older store that already holds a `docTruth` envelope key
  // (no column-add, so a re-run is a no-op); the next saveGeneration writes
  // rows AND stops persisting the blob, so a clean rebuild leaves only rows.
  (db) => {
    db.exec(`
      CREATE TABLE IF NOT EXISTS documents (
        id TEXT PRIMARY KEY,
        path TEXT NOT NULL,
        content_hash TEXT,
        lifecycle_status TEXT,
        lifecycle_superseded_by TEXT,
        lifecycle_superseded_on TEXT,
        generated_at TEXT,
        generation_id TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_documents_generation ON documents(generation_id);
      CREATE INDEX IF NOT EXISTS idx_documents_path ON documents(path);

      CREATE TABLE IF NOT EXISTS claims (
        id TEXT PRIMARY KEY,
        document_id TEXT NOT NULL,
        source TEXT,
        line INTEGER,
        text TEXT NOT NULL,
        status TEXT,
        sha1 TEXT,
        generation_id TEXT NOT NULL,
        FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
      );
      CREATE INDEX IF NOT EXISTS idx_claims_generation ON claims(generation_id);
      CREATE INDEX IF NOT EXISTS idx_claims_document ON claims(document_id);

      CREATE TABLE IF NOT EXISTS claim_code_edges (
        id TEXT PRIMARY KEY,
        claim_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        source TEXT NOT NULL,
        target TEXT NOT NULL,
        confidence REAL NOT NULL,
        confidence_class TEXT,
        reason TEXT,
        evidence_doc_path TEXT,
        evidence_doc_line INTEGER,
        evidence_doc_sha1 TEXT,
        evidence_code_path TEXT,
        evidence_code_exists INTEGER,
        evidence_code_node_id TEXT,
        evidence_code_content_hash TEXT,
        generation_id TEXT NOT NULL,
        FOREIGN KEY (claim_id) REFERENCES claims(id) ON DELETE CASCADE
      );
      CREATE INDEX IF NOT EXISTS idx_claim_code_edges_generation ON claim_code_edges(generation_id);
      CREATE INDEX IF NOT EXISTS idx_claim_code_edges_claim ON claim_code_edges(claim_id);
      CREATE INDEX IF NOT EXISTS idx_claim_code_edges_kind ON claim_code_edges(kind);

      CREATE TABLE IF NOT EXISTS document_supersession (
        id TEXT PRIMARY KEY,
        source_kind TEXT NOT NULL,
        source_doc TEXT,
        source_line INTEGER,
        source_text TEXT,
        source_external INTEGER NOT NULL DEFAULT 0,
        target_doc TEXT,
        target_external INTEGER NOT NULL DEFAULT 0,
        target_match TEXT,
        superseded_on TEXT,
        generation_id TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_document_supersession_generation ON document_supersession(generation_id);
    `);
  },
  // Migration 17 — immutable named, generation-bound review snapshots.
  (db) => {
    db.exec(`
      CREATE TABLE IF NOT EXISTS named_snapshot (
        name TEXT PRIMARY KEY,
        repo_root TEXT NOT NULL,
        generation_id TEXT NOT NULL,
        manifest_digest TEXT NOT NULL,
        identity_json TEXT NOT NULL,
        created_ms INTEGER NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_named_snapshot_generation ON named_snapshot(generation_id);
    `);
  },
];

/** Current schema version = number of migrations. Derived, so it cannot desync. */
export const SCHEMA_VERSION = MIGRATIONS.length;

export function getSchemaVersion(db) {
  const row = db.prepare("SELECT value FROM meta WHERE key = 'schema_version'").get();
  return row ? Number(row.value) : 0;
}

export function recordArtifactState(db, artifact, generationId, fingerprint, updatedMs = Date.now()) {
  db.prepare(`INSERT INTO artifact_state(artifact, generation_id, fingerprint, updated_ms)
    VALUES (?, ?, ?, ?) ON CONFLICT(artifact) DO UPDATE SET generation_id=excluded.generation_id,
      fingerprint=excluded.fingerprint, updated_ms=excluded.updated_ms`)
    .run(String(artifact), String(generationId), String(fingerprint), Number(updatedMs));
}

export function listArtifactState(db) {
  return db.prepare("SELECT artifact, generation_id AS generationId, fingerprint, updated_ms AS updatedMs FROM artifact_state ORDER BY artifact").all();
}

export function migrate(db) {
  db.exec("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);");
  let version = getSchemaVersion(db);
  if (!Number.isInteger(version) || version < 0 || version > MIGRATIONS.length) {
    throw new Error(`invalid graph store schema version: ${version}`);
  }
  const initialVersion = version;
  while (version < MIGRATIONS.length) {
    db.exec("BEGIN;");
    try {
      MIGRATIONS[version](db);
      version += 1;
      db.prepare("INSERT INTO meta (key, value) VALUES ('schema_version', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value").run(String(version));
      db.exec("COMMIT;");
    } catch (error) {
      db.exec("ROLLBACK;");
      throw error;
    }
  }
  if (initialVersion < 11 && version >= 11) rebuildSymbolTerms(db);
  return version;
}

// ---------------------------------------------------------------------------
// Bulk insert (one transaction).
// ---------------------------------------------------------------------------

// `generation` = { nodes: [...], edges: [...], fileReports?: [...] } — the
// shape produced by treesitter-provider.mjs's buildTreeSitterGraph(), or any
// generation-shaped object with the same node/edge fields (static-provider's
// generation.nodes/generation.edges also fit, since the node/edge shapes are
// deliberately compatible — see treesitter-provider.mjs's header comment).
//
// `mode: "replace"` (default) clears all prior rows before inserting, since
  // this store is a single-current-generation database (never an accumulating
  // history). `mode: "append"` skips the clear, for callers deliberately
// keeping multiple generations side by side (e.g. a test asserting
// generation_id filtering).
function insertGenerationRows(db, generation, options = {}) {
  const mode = options.mode ?? "replace";
  const generationId = String(options.generationId ?? generation.manifest?.generationId ?? generation.generationId ?? "unknown");
  const nodes = generation.nodes ?? [];
  const edges = generation.edges ?? [];
  const fileReports = generation.fileReports ?? [];
  const fileReportByPath = new Map(fileReports.map((report) => [report.path, report]));

  const insertFile = db.prepare(`
    INSERT INTO files (path, content_hash, language, provider, parse_status, error_node_count, generation_id,
                       node_id, labels, name, qualified_name, confidence, evidence, extra)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(path) DO UPDATE SET
      content_hash = excluded.content_hash, language = excluded.language, provider = excluded.provider,
      parse_status = excluded.parse_status, error_node_count = excluded.error_node_count, generation_id = excluded.generation_id,
      node_id = excluded.node_id, labels = excluded.labels, name = excluded.name,
      qualified_name = excluded.qualified_name, confidence = excluded.confidence, evidence = excluded.evidence,
      extra = excluded.extra
  `);
  const insertSymbol = db.prepare(`
    INSERT INTO symbols (id, kind, labels, name, qualified_name, path, confidence, evidence, generation_id, extra)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET
      kind = excluded.kind, labels = excluded.labels, name = excluded.name, qualified_name = excluded.qualified_name,
      path = excluded.path, confidence = excluded.confidence, evidence = excluded.evidence,
      generation_id = excluded.generation_id, extra = excluded.extra
  `);
  const insertEdge = db.prepare(`
    INSERT INTO edges (id, kind, source, target, confidence, resolved, specifier, evidence, generation_id,
                       confidence_tier, extra)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET
      kind = excluded.kind, source = excluded.source, target = excluded.target, confidence = excluded.confidence,
      resolved = excluded.resolved, specifier = excluded.specifier, evidence = excluded.evidence,
      generation_id = excluded.generation_id, confidence_tier = excluded.confidence_tier, extra = excluded.extra
  `);

  // Keys the relational columns already carry. Anything else a provider emits
  // goes to `extra` verbatim, which is what keeps a schema change from silently
  // dropping a new field (see the migration-3 comment).
  const NODE_COLUMN_KEYS = new Set(["id", "kind", "labels", "name", "qualifiedName", "path", "confidence", "evidence"]);
  const EDGE_COLUMN_KEYS = new Set(["id", "kind", "source", "target", "confidence", "confidenceTier", "evidence"]);
  const extraOf = (object, columnKeys) => {
    const extra = {};
    for (const [key, value] of Object.entries(object)) {
      if (!columnKeys.has(key)) extra[key] = value;
    }
    return Object.keys(extra).length ? JSON.stringify(extra) : null;
  };

  if (mode === "replace") {
    db.exec("DELETE FROM files; DELETE FROM symbols; DELETE FROM edges; DELETE FROM vectors; DELETE FROM symbol_search; DELETE FROM symbol_terms; DELETE FROM claim_code_edges; DELETE FROM claims; DELETE FROM documents; DELETE FROM document_supersession;");
  }
  for (const node of nodes) {
    if (node.kind === "file") {
      const report = fileReportByPath.get(node.path);
      insertFile.run(
        node.path,
        node.evidence?.[0]?.contentHash ?? null,
        report?.language ?? null,
        report?.provider ?? null,
        report?.parseStatus ?? null,
        report?.errorNodeCount ?? null,
        generationId,
        node.id,
        JSON.stringify(node.labels ?? []),
        node.name ?? null,
        node.qualifiedName ?? null,
        node.confidence ?? 1,
        JSON.stringify(node.evidence ?? []),
        extraOf(node, NODE_COLUMN_KEYS),
      );
    } else {
      insertSymbol.run(
        node.id,
        node.kind,
        JSON.stringify(node.labels ?? []),
        node.name,
        node.qualifiedName,
        node.path,
        node.confidence ?? 1,
        JSON.stringify(node.evidence ?? []),
        generationId,
        extraOf(node, NODE_COLUMN_KEYS),
      );
      replaceSymbolSearchEntry(db, {
        id: node.id,
        generationId,
        name: node.name,
        qualifiedName: node.qualifiedName,
        path: node.path,
      });
    }
  }
  for (const edge of edges) {
    insertEdge.run(
      edge.id,
      edge.kind,
      edge.source,
      edge.target ?? null,
      edge.confidence ?? 1,
      edge.resolved === false ? 0 : 1,
      edge.specifier ?? null,
      JSON.stringify(edge.evidence ?? []),
      generationId,
      edge.confidenceTier ?? null,
      extraOf(edge, EDGE_COLUMN_KEYS),
    );
  }
  // Phase 7.3 — relational docTruth. Decompose the docTruth envelope into four
  // generation-pinned tables. Each row carries its own generation_id and ID;
  // prose stays in the source files (this store holds structure + spans only).
  // The instructionPolicy:data_only invariant is upheld here: rows describe
  // WHAT documents/claims exist and WHERE they point, never the prose, so
  // graph connectivity cannot promote repo prose to agent instructions.
  const docTruth = generation.docTruth;
  if (docTruth) {
    insertDocTruthRows(db, docTruth, generationId);
  }
  return {
    generationId,
    fileCount: nodes.filter((n) => n.kind === "file").length,
    symbolCount: nodes.filter((n) => n.kind !== "file").length,
    edgeCount: edges.length,
    claimCount: docTruth ? countDocTruthRows(docTruth) : { documents: 0, claims: 0, edges: 0, supersedes: 0 },
  };
}

// Decompose the in-memory `docTruth` envelope into relational rows. The shape
// is the one `static-provider.mjs:buildDocCodeJoins` produces — a flat array of
// `joins` (typed edges joining a claim to a code node), a flat array of
// `supersedes` (typed edges joining docs across lifecycle states), and a
// `sourceDocMap` describing the source `map.json` (which contains docs and
// claims we re-derive from the join evidence so the tables stay queryable per-
// generation without depending on the `map.json` still being on disk).
function insertDocTruthRows(db, docTruth, generationId) {
  const docIdByPath = new Map();
  const docById = new Map();
  const claimIdByKey = new Map();
  const insertDoc = db.prepare(`
    INSERT INTO documents (id, path, content_hash, lifecycle_status, lifecycle_superseded_by, lifecycle_superseded_on, generated_at, generation_id)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET
      path = excluded.path, content_hash = excluded.content_hash,
      lifecycle_status = excluded.lifecycle_status,
      lifecycle_superseded_by = excluded.lifecycle_superseded_by,
      lifecycle_superseded_on = excluded.lifecycle_superseded_on,
      generated_at = excluded.generated_at,
      generation_id = excluded.generation_id
  `);
  const insertClaim = db.prepare(`
    INSERT INTO claims (id, document_id, source, line, text, status, sha1, generation_id)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET
      document_id = excluded.document_id, source = excluded.source, line = excluded.line,
      text = excluded.text, status = excluded.status, sha1 = excluded.sha1,
      generation_id = excluded.generation_id
  `);
  const insertJoin = db.prepare(`
    INSERT INTO claim_code_edges (id, claim_id, kind, source, target, confidence, confidence_class, reason,
                                  evidence_doc_path, evidence_doc_line, evidence_doc_sha1,
                                  evidence_code_path, evidence_code_exists,
                                  evidence_code_node_id, evidence_code_content_hash, generation_id)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET
      claim_id = excluded.claim_id, kind = excluded.kind, source = excluded.source, target = excluded.target,
      confidence = excluded.confidence, confidence_class = excluded.confidence_class, reason = excluded.reason,
      evidence_doc_path = excluded.evidence_doc_path, evidence_doc_line = excluded.evidence_doc_line,
      evidence_doc_sha1 = excluded.evidence_doc_sha1, evidence_code_path = excluded.evidence_code_path,
      evidence_code_exists = excluded.evidence_code_exists, evidence_code_node_id = excluded.evidence_code_node_id,
      evidence_code_content_hash = excluded.evidence_code_content_hash,
      generation_id = excluded.generation_id
  `);
  const insertSuper = db.prepare(`
    INSERT INTO document_supersession (id, source_kind, source_doc, source_line, source_text, source_external,
                                       target_doc, target_external, target_match, superseded_on, generation_id)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET
      source_kind = excluded.source_kind, source_doc = excluded.source_doc,
      source_line = excluded.source_line, source_text = excluded.source_text, source_external = excluded.source_external,
      target_doc = excluded.target_doc, target_external = excluded.target_external,
      target_match = excluded.target_match, superseded_on = excluded.superseded_on,
      generation_id = excluded.generation_id
  `);

  // Iterate joins to discover every (claim, doc, code) tuple that needs a row.
  for (const join of docTruth.joins ?? []) {
    const docRef = join?.evidence?.docRef ?? null;
    const codeRef = join?.evidence?.codeRef ?? null;
    const codeNode = join?.evidence?.codeNode ?? null;
    const docPath = docRef?.path ? String(docRef.path) : null;
    if (!docPath) continue;
    const docId = docIdByPath.get(docPath) ?? `doc:${docPath}`;
    if (!docIdByPath.has(docPath)) {
      docIdByPath.set(docPath, docId);
      // We carry the document row's content hash when it is available on the
      // join; absent content hashes are NULL rather than synthesised, so a
      // missing hash is observable, not hidden.
      insertDoc.run(
        docId,
        docPath,
        docRef?.sha1 ?? null,
        null,
        null,
        null,
        docTruth.sourceDocMap?.generatedAt ?? null,
        generationId,
      );
      docById.set(docId, { id: docId, path: docPath, contentHash: docRef?.sha1 ?? null });
    }
    // The claim id is `claim:<docPath>:<line>:<text-prefix>` when no explicit
    // id is on the join; `static-provider`'s join shape does not carry one, so
    // we derive a deterministic one keyed on the evidence — two joins that
    // describe the same (doc, line) collapse onto the same claim row, which is
    // the right behaviour for the per-code-ref expansion in classifyJoin.
    const docLine = docRef?.line ?? null;
    const claimKey = `${docId}:${docLine ?? ""}:${String(codeRef?.path ?? "")}:${join.kind ?? ""}`;
    const claimId = claimIdByKey.get(claimKey) ?? `claim:${docId}:${docLine ?? "n"}:${(codeRef?.path ?? "n").replaceAll("/", "_")}:${join.kind ?? "n"}`;
    if (!claimIdByKey.has(claimKey)) {
      claimIdByKey.set(claimKey, claimId);
      insertClaim.run(
        claimId,
        docId,
        docPath,
        docLine,
        join.reason ?? "",
        null,
        docRef?.sha1 ?? null,
        generationId,
      );
    }
    insertJoin.run(
      join.id ?? `claimEdge:${claimId}:${join.kind ?? "n"}:${codeNode?.id ?? codeRef?.path ?? "n"}`,
      claimId,
      join.kind ?? "supports",
      join.source ?? "",
      join.target ?? "",
      join.confidence ?? 1,
      join.confidenceClass ?? null,
      join.reason ?? null,
      docPath,
      docLine,
      docRef?.sha1 ?? null,
      codeRef?.path ?? null,
      codeRef?.exists === false ? 0 : 1,
      codeNode?.id ?? null,
      codeNode?.contentHash ?? null,
      generationId,
    );
  }
  // Phase 7.3 — persist every doc in `sourceDocMap.docPaths`, not just the
  // ones connected to a join. The original `sourceDocMap.docs` count covered
  // every doc node from `map.json`; undercounting would silently break the
  // Phase 3.1 slice `claims` field and the `graph-query.test.mjs` assertion.
  for (const docPath of docTruth.sourceDocMap?.docPaths ?? []) {
    if (!docPath) continue;
    ensureDocumentRow(insertDoc, docIdByPath, docById, String(docPath), docTruth.sourceDocMap?.generatedAt ?? null, generationId);
  }
  for (const claimLocator of docTruth.sourceDocMap?.claimPaths ?? []) {
    const docPath = claimLocator?.path ? String(claimLocator.path) : null;
    if (!docPath) continue;
    ensureDocumentRow(insertDoc, docIdByPath, docById, docPath, docTruth.sourceDocMap?.generatedAt ?? null, generationId);
  }
  for (const superRow of docTruth.supersedes ?? []) {
    const sourceKind = superRow?.kind ?? "supersedes";
    const sourceDoc = superRow?.source?.doc ?? null;
    const sourceLine = superRow?.source?.line ?? null;
    const sourceText = superRow?.source?.text ?? null;
    const sourceExternal = superRow?.source?.external === true ? 1 : 0;
    const targetDoc = superRow?.target?.doc ?? null;
    const targetExternal = superRow?.target?.external === true ? 1 : 0;
    const targetMatch = superRow?.target ? String(JSON.stringify(superRow.target)) : null;
    const supersededOn = superRow?.target?.supersededOn ?? superRow?.evidence?.supersededOn ?? null;
    // A superseded document may have no joins (it is a doc-side fact only), so
    // ensure it has a row to keep `listDocuments` aligned with the original
    // `map.json` doc count — supersedes-only generations must still surface
    // their docs in the slice `claims` field and the relational readers.
    if (!sourceExternal && sourceDoc) ensureDocumentRow(insertDoc, docIdByPath, docById, String(sourceDoc), docTruth.sourceDocMap?.generatedAt ?? null, generationId);
    if (!targetExternal && targetDoc) ensureDocumentRow(insertDoc, docIdByPath, docById, String(targetDoc), docTruth.sourceDocMap?.generatedAt ?? null, generationId);
    const id = `supersede:${generationId}:${sourceDoc ?? "ext"}->${targetDoc ?? "ext"}:${sourceLine ?? 0}`;
    insertSuper.run(
      id,
      sourceKind,
      sourceDoc,
      sourceLine,
      sourceText,
      sourceExternal,
      targetDoc,
      targetExternal,
      targetMatch,
      supersededOn,
      generationId,
    );
  }
}

function ensureDocumentRow(insertDoc, docIdByPath, docById, docPath, generatedAt, generationId) {
  if (docIdByPath.has(docPath)) return;
  const docId = `doc:${docPath}`;
  docIdByPath.set(docPath, docId);
  insertDoc.run(
    docId,
    docPath,
    null,
    null,
    null,
    null,
    generatedAt,
    generationId,
  );
  docById.set(docId, { id: docId, path: docPath, contentHash: null });
}

function countDocTruthRows(docTruth) {
  const docPaths = new Set();
  for (const join of docTruth.joins ?? []) {
    const path = join?.evidence?.docRef?.path;
    if (path) docPaths.add(String(path));
  }
  for (const superRow of docTruth.supersedes ?? []) {
    if (superRow?.source?.doc) docPaths.add(String(superRow.source.doc));
    if (superRow?.target?.doc) docPaths.add(String(superRow.target.doc));
  }
  return {
    documents: docPaths.size,
    claims: (docTruth.joins ?? []).length,
    edges: (docTruth.joins ?? []).length,
    supersedes: (docTruth.supersedes ?? []).length,
  };
}

/** Replace one symbol's generation-bound search terms atomically with its row. */
export function replaceSymbolSearchEntry(db, row) {
  const symbolId = String(row?.id ?? "");
  const generationId = String(row?.generationId ?? "");
  if (!symbolId || !generationId) return;
  db.prepare("DELETE FROM symbol_search WHERE id = ?").run(symbolId);
  db.prepare("INSERT INTO symbol_search(id, generation_id, name, qualified_name, path) VALUES (?, ?, ?, ?, ?)")
    .run(symbolId, generationId, searchableSymbolText(row.name), searchableSymbolText(row.qualifiedName), searchableSymbolText(row.path));
  replaceSymbolTermsEntry(db, row);
}

function replaceSymbolTermsEntry(db, row) {
  const symbolId = String(row?.id ?? "");
  const generationId = String(row?.generationId ?? "");
  if (!symbolId || !generationId) return;
  db.prepare("DELETE FROM symbol_terms WHERE symbol_id = ?").run(symbolId);
  const insertTerm = db.prepare("INSERT OR IGNORE INTO symbol_terms(generation_id, token, symbol_id) VALUES (?, ?, ?)");
  for (const token of symbolTermTokens([row.name, row.qualifiedName, row.path])) {
    insertTerm.run(generationId, token, symbolId);
  }
}

function rebuildSymbolTerms(db) {
  db.exec("DELETE FROM symbol_terms;");
  for (const row of db.prepare("SELECT id, generation_id AS generationId, name, qualified_name AS qualifiedName, path FROM symbols").all()) {
    replaceSymbolTermsEntry(db, row);
  }
}

function searchableSymbolText(value) {
  return String(value ?? "")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replaceAll("_", " ")
    .replaceAll("-", " ");
}

export function deleteSymbolSearchEntry(db, symbolId) {
  db.prepare("DELETE FROM symbol_search WHERE id = ?").run(String(symbolId));
  db.prepare("DELETE FROM symbol_terms WHERE symbol_id = ?").run(String(symbolId));
}

function symbolSearchIsFts(db) {
  const row = db.prepare("SELECT sql FROM sqlite_master WHERE name='symbol_search'").get();
  return /VIRTUAL TABLE[\s\S]*fts5/i.test(String(row?.sql ?? ""));
}

export function symbolTermTokens(values) {
  const terms = new Set(["*"]);
  for (const value of values ?? []) {
    for (const term of String(value ?? "")
      .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
      .toLowerCase()
      .replaceAll("-", "_")
      .split(/[^a-z0-9_]+/)) {
      if (!term || term.length > 64) continue;
      terms.add(term);
      for (const part of term.split("_")) if (part) terms.add(part);
    }
  }
  return [...terms].sort();
}

/**
 * Generation-pinned B-tree symbol lookup. The longest exact token wins; `*`
 * remains a deterministic fallback whenever that generation has symbols.
 */
export function searchGenerationSymbols(db, generationId, tokens, limit = 20) {
  const generation = String(generationId ?? "");
  if (!generation) return [];
  const normalized = symbolTermTokens(tokens).filter((token) => token !== "*");
  const cap = Math.max(1, Math.min(256, Number(limit) || 20));
  const columns = "s.id, s.name, s.qualified_name AS qualifiedName, s.path, s.confidence, s.evidence, s.generation_id AS generationId";
  const join = "FROM symbol_terms st JOIN symbols s ON s.id=st.symbol_id AND s.generation_id=st.generation_id";
  const select = (token) => db.prepare(`SELECT ${columns} ${join} WHERE st.generation_id=? AND st.token=?
    ORDER BY s.confidence DESC, s.path, s.id LIMIT ?`).all(generation, token, cap);
  for (const token of normalized.sort((left, right) => right.length - left.length || left.localeCompare(right))) {
    const matched = select(token);
    if (matched.length) return matched;
  }
  return select("*");
}

export function bulkInsertGeneration(db, generation, options = {}) {
  db.exec("BEGIN;");
  try {
    const summary = insertGenerationRows(db, generation, options);
    db.exec("COMMIT;");
    return summary;
  } catch (error) {
    db.exec("ROLLBACK;");
    throw error;
  }
}

// ---------------------------------------------------------------------------
// Whole-generation round trip — the store as the ONLY store.
// ---------------------------------------------------------------------------

// Envelope fields: everything in a generation that is not a node or an edge.
// Kept as JSON blobs rather than columns because nothing queries into them —
// they are read whole or not at all, and giving them columns would invent a
// schema that no query needs (and that would then have to be migrated).
// `sourceObservation` records the commit the graph was built at and whether the
// tree was clean — a downstream consumer must be able to tell a committed
// snapshot from a dirty-overlay build without opening git.
const ENVELOPE_KEYS = ["schemaVersion", "provider", "manifest", "repoRoot", "augmentation", "sourceObservation"];

/**
 * Persist a complete generation: nodes, edges, and envelope in ONE transaction.
 *
 * A crash mid-write leaves the prior complete generation intact — never a torn
 * mixture of fresh rows with a stale envelope (or stale envelope keys surviving
 * from an older schema).
 */
export function saveGeneration(db, generation, options = {}) {
  const envelopeKeysPresent = ENVELOPE_KEYS.filter((key) => generation[key] !== undefined);
  const put = db.prepare(
    "INSERT INTO generation (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
  );
  db.exec("BEGIN;");
  try {
    const summary = insertGenerationRows(db, generation, options);
    if (options.populateState) populateGenerationState(db, generation);
    for (const key of envelopeKeysPresent) {
      put.run(key, JSON.stringify(generation[key]));
    }
    if (envelopeKeysPresent.length > 0) {
      const placeholders = envelopeKeysPresent.map(() => "?").join(", ");
      db.prepare(`DELETE FROM generation WHERE key NOT IN (${placeholders})`).run(...envelopeKeysPresent);
    } else {
      db.exec("DELETE FROM generation;");
    }
    db.exec("COMMIT;");
    return summary;
  } catch (error) {
    db.exec("ROLLBACK;");
    throw error;
  }
}

function normalizeContentDigest(value) {
  const text = String(value ?? "");
  return text.startsWith("xxh128:") ? text : `xxh128:${text}`;
}

function providerIdentity(generation) {
  const provider = generation.provider ?? generation.manifest?.provider ?? {};
  const id = typeof provider === "string" ? provider : provider.id ?? "lexical";
  const version = typeof provider === "string" ? "unknown" : provider.version ?? "unknown";
  return {
    id: id.includes("treesitter") ? "treesitter" : id.includes("scip") ? "scip" : "lexical",
    version: String(version),
  };
}

function providerForFact(fact, generationProvider) {
  const declared = fact?.factProvider;
  if (!declared) return generationProvider;
  const id = String(declared.id ?? generationProvider.id);
  return {
    id: id.includes("treesitter") ? "treesitter" : id.includes("scip") ? "scip" : "lexical",
    version: String(declared.version ?? generationProvider.version),
  };
}

function fileStateForNode(generation, node) {
  const digest = normalizeContentDigest(node.evidence?.[0]?.contentHash ?? "unknown");
  let size = 0;
  let mtimeMs = null;
  let identity = null;
  try {
    const stat = statSync(join(generation.repoRoot ?? "", node.path));
    size = stat.size;
    mtimeMs = stat.mtimeMs;
    identity = stat.dev !== undefined && stat.ino !== undefined ? `${stat.dev}:${stat.ino}` : null;
  } catch {
    /* a persisted generation may outlive its source tree */
  }
  return { digest, size, mtimeMs, identity };
}

function populateGenerationState(db, generation) {
  const provider = providerIdentity(generation);
  const generationId = generation.manifest?.generationId ?? null;
  const repoRoot = generation.repoRoot ?? null;
  const files = (generation.nodes ?? []).filter((node) => node.kind === "file");
  const fileById = new Map(files.map((node) => [node.id, node.path]));
  db.exec("DELETE FROM file_state; DELETE FROM fact_owner; DELETE FROM dependency_index; DELETE FROM generation_leaf;");
  const insertFileState = db.prepare("INSERT INTO file_state(path, content_digest, size, mtime_ms, file_identity, last_event_seq, applied_clock) VALUES (?, ?, ?, ?, ?, NULL, 0)");
  for (const file of files) {
    const state = fileStateForNode(generation, file);
    insertFileState.run(file.path, state.digest, state.size, state.mtimeMs, state.identity);
  }
  const insertOwner = db.prepare("INSERT OR REPLACE INTO fact_owner(fact_id, fact_kind, source_path, source_digest, provider_id, provider_version, freshness_domain, fact_kind_detail, generation_id, repo_root) VALUES (?, ?, ?, ?, ?, ?, 'structural', ?, ?, ?)");
  for (const node of generation.nodes ?? []) {
    const path = node.path;
    if (!path) continue;
    const digest = normalizeContentDigest(files.find((file) => file.path === path)?.evidence?.[0]?.contentHash ?? "unknown");
    const owner = providerForFact(node, provider);
    insertOwner.run(node.id, "node", path, digest, owner.id, owner.version, node.kind, generationId, repoRoot);
  }
  for (const edge of generation.edges ?? []) {
    const sourcePath = fileById.get(edge.source) ?? (generation.nodes ?? []).find((node) => node.id === edge.source)?.path;
    if (!sourcePath) continue;
    const digest = normalizeContentDigest(files.find((file) => file.path === sourcePath)?.evidence?.[0]?.contentHash ?? "unknown");
    const owner = providerForFact(edge, provider);
    insertOwner.run(edge.id, "edge", sourcePath, digest, owner.id, owner.version, edge.kind, generationId, repoRoot);
    if (edge.kind === "IMPORTS" && edge.target) {
      const targetPath = fileById.get(edge.target);
      if (targetPath) db.prepare("INSERT OR IGNORE INTO dependency_index(source_path, dependent_path, reason) VALUES (?, ?, 'import')").run(targetPath, sourcePath);
    }
  }
  computeFullLedger(db, files.map((file) => ({ path: file.path, contentDigest: file.evidence?.[0]?.contentHash })));
}

export function deleteFactsByOwner(db, path, providerId = null) {
  const clauses = ["source_path = ?"];
  const params = [String(path)];
  if (providerId) { clauses.push("provider_id = ?"); params.push(String(providerId)); }
  const owners = db.prepare(`SELECT fact_id, fact_kind FROM fact_owner WHERE ${clauses.join(" AND ")}`).all(...params);
  db.prepare(`DELETE FROM fact_owner WHERE ${clauses.join(" AND ")}`).run(...params);
  for (const owner of owners) {
    const retained = db.prepare("SELECT 1 FROM fact_owner WHERE fact_id=? AND fact_kind=? LIMIT 1").get(owner.fact_id, owner.fact_kind);
    if (retained) continue;
    if (owner.fact_kind === "edge") db.prepare("DELETE FROM edges WHERE id = ?").run(owner.fact_id);
    else if (owner.fact_kind === "node") {
      db.prepare("DELETE FROM symbols WHERE id = ?").run(owner.fact_id);
      deleteSymbolSearchEntry(db, owner.fact_id);
    }
  }
  return owners;
}

/**
 * The derived_fact_owner ledger view: for a given path (or the whole store),
 * which generation and which repository last produced each fact this store
 * holds. Backed by `fact_owner`'s generation_id/repo_root columns (Migration
 * 12) — this is a read helper, not a second table, so lineage can never drift
 * from the ownership rows themselves.
 */
export function listDerivedFactOwners(db, { path = null } = {}) {
  const rows = path
    ? db.prepare("SELECT fact_id, fact_kind, source_path, provider_id, provider_version, freshness_domain, generation_id, repo_root FROM fact_owner WHERE source_path = ? ORDER BY fact_kind, fact_id").all(String(path))
    : db.prepare("SELECT fact_id, fact_kind, source_path, provider_id, provider_version, freshness_domain, generation_id, repo_root FROM fact_owner ORDER BY source_path, fact_kind, fact_id").all();
  return rows.map((row) => ({
    factId: row.fact_id,
    factKind: row.fact_kind,
    sourcePath: row.source_path,
    providerId: row.provider_id,
    providerVersion: row.provider_version,
    freshnessDomain: row.freshness_domain,
    generationId: row.generation_id,
    repoRoot: row.repo_root,
  }));
}

export function loadFileState(db, path) {
  return db.prepare("SELECT * FROM file_state WHERE path = ?").get(String(path)) ?? null;
}

export function listFileMetadata(db) {
  return db.prepare(`SELECT f.path, f.language, f.provider, f.parse_status AS parseStatus,
    f.error_node_count AS errorNodeCount, s.content_digest AS contentDigest, s.size,
    s.mtime_ms AS mtimeMs, s.file_identity AS fileIdentity
    FROM files f LEFT JOIN file_state s ON s.path=f.path ORDER BY f.path`).all();
}

export function listSymbolMetadata(db, providerId = null) {
  const rows = providerId
    ? db.prepare(`SELECT s.id, s.labels, s.name, s.qualified_name AS qualifiedName, s.path
      FROM symbols s JOIN fact_owner o ON o.fact_id=s.id AND o.fact_kind='node'
      WHERE o.provider_id=? ORDER BY s.path,s.id`).all(providerId)
    : db.prepare("SELECT id, labels, name, qualified_name AS qualifiedName, path FROM symbols ORDER BY path, id").all();
  return rows
    .map((row) => ({ ...row, labels: JSON.parse(row.labels || "[]") }));
}

// Phase 7.3 — relational docTruth readers.
//
// Slim, ordered reads that return only structure + spans. None of these
// materialise document or claim prose — that stays in the source files. They
// exist so the slice `claims` field (Phase 3.1) can be filled by SQL without
// loading the full generation envelope.
//
// `generationId` is REQUIRED: every row carries it, and an unfiltered scan
// would mix rows across generations. `null` is the safe empty answer.

function claimTextFor(row) {
  // The text column carries `join.reason` (a short classifier string, not full
  // prose) because the build-time classifier does not preserve the full claim
  // text. That is intentional — full prose stays in the source file and the
  // slice consumer reads it through the `source` pointer. Returning it here
  // would be the only way for graph connectivity to surface prose, which the
  // instructionPolicy:data_only invariant forbids.
  return row.text;
}

export function listDocuments(db, generationId, options = {}) {
  if (!generationId) return [];
  const limit = Math.max(1, Math.min(10000, Number(options.limit ?? 10000) || 10000));
  const rows = db.prepare(`SELECT id, path, content_hash AS contentHash, lifecycle_status AS lifecycleStatus,
    lifecycle_superseded_by AS lifecycleSupersededBy, lifecycle_superseded_on AS lifecycleSupersededOn,
    generated_at AS generatedAt
    FROM documents WHERE generation_id = ? ORDER BY rowid LIMIT ?`).all(String(generationId), limit);
  return rows;
}

export function listClaims(db, generationId, options = {}) {
  if (!generationId) return [];
  const limit = Math.max(1, Math.min(10000, Number(options.limit ?? 10000) || 10000));
  const rows = db.prepare(`SELECT id, document_id AS documentId, source, line, text, status, sha1
    FROM claims WHERE generation_id = ? ORDER BY rowid LIMIT ?`).all(String(generationId), limit);
  return rows.map((row) => ({ ...row, text: claimTextFor(row) }));
}

export function listClaimCodeEdges(db, generationId, options = {}) {
  if (!generationId) return [];
  const limit = Math.max(1, Math.min(10000, Number(options.limit ?? 10000) || 10000));
  const kind = options.kind ? String(options.kind) : null;
  const claimId = options.claimId ? String(options.claimId) : null;
  const clauses = ["generation_id = ?"];
  const params = [String(generationId)];
  if (kind) { clauses.push("kind = ?"); params.push(kind); }
  if (claimId) { clauses.push("claim_id = ?"); params.push(claimId); }
  const where = clauses.join(" AND ");
  const rows = db.prepare(`SELECT id, claim_id AS claimId, kind, source, target, confidence,
    confidence_class AS confidenceClass, reason,
    evidence_doc_path AS evidenceDocPath, evidence_doc_line AS evidenceDocLine, evidence_doc_sha1 AS evidenceDocSha1,
    evidence_code_path AS evidenceCodePath, evidence_code_exists AS evidenceCodeExists,
    evidence_code_node_id AS evidenceCodeNodeId, evidence_code_content_hash AS evidenceCodeContentHash
    FROM claim_code_edges WHERE ${where} ORDER BY rowid LIMIT ?`).all(...params, limit);
  return rows;
}

export function listDocumentSupersession(db, generationId, options = {}) {
  if (!generationId) return [];
  const limit = Math.max(1, Math.min(10000, Number(options.limit ?? 10000) || 10000));
  const rows = db.prepare(`SELECT id, source_kind AS sourceKind, source_doc AS sourceDoc, source_line AS sourceLine,
    source_text AS sourceText, source_external AS sourceExternal,
    target_doc AS targetDoc, target_external AS targetExternal,
    target_match AS targetMatch, superseded_on AS supersededOn
    FROM document_supersession WHERE generation_id = ? ORDER BY rowid LIMIT ?`).all(String(generationId), limit);
  return rows.map((row) => ({
    ...row,
    sourceExternal: row.sourceExternal === 1,
    targetExternal: row.targetExternal === 1,
  }));
}

/**
 * Slim, ordered, per-generation docTruth slice used by the Phase 3.1
 * RepositorySubgraphSliceV1 `claims` field. Each entry combines a claim and
 * its typed join rows (supports / contradicts / supersedes) so a downstream
 * consumer does not need to join three tables itself. Empty arrays on every
 * field when the generation has no doc coverage — never null — so callers
 * can iterate without a presence check.
 */
export function listClaimSlice(db, generationId, options = {}) {
  if (!generationId) return [];
  const claims = listClaims(db, generationId, options);
  if (claims.length === 0) return [];
  const edges = listClaimCodeEdges(db, generationId, options);
  const byClaim = new Map();
  for (const edge of edges) {
    if (!byClaim.has(edge.claimId)) byClaim.set(edge.claimId, []);
    byClaim.get(edge.claimId).push(edge);
  }
  return claims.map((claim) => ({
    id: claim.id,
    documentId: claim.documentId,
    source: claim.source,
    line: claim.line,
    status: claim.status,
    text: claim.text,
    sha1: claim.sha1,
    edges: byClaim.get(claim.id) ?? [],
  }));
}

export function collectDependents(db, path, options = {}) {
  const maxHops = Math.max(0, Number(options.maxHops ?? 2));
  const maxFiles = Math.max(0, Number(options.maxFiles ?? 500));
  const root = String(path);
  const visited = new Set([root]);
  const paths = [];
  const remaining = [];
  let frontier = [root];
  for (let hop = 0; hop < maxHops && frontier.length > 0; hop += 1) {
    const next = [];
    for (const current of frontier) {
      const rows = db.prepare("SELECT dependent_path FROM dependency_index WHERE source_path = ? ORDER BY dependent_path").all(current);
      for (const row of rows) {
        const dependent = String(row.dependent_path);
        if (visited.has(dependent)) continue;
        visited.add(dependent);
        next.push(dependent);
      }
    }
    next.sort();
    for (const dependent of next) {
      if (paths.length < maxFiles) paths.push(dependent);
      else remaining.push(dependent);
    }
    frontier = next;
  }
  return { paths, remaining, truncated: remaining.length > 0 };
}

export function dependentsOf(db, path, maxHops = 2, maxFiles = 500) {
  return collectDependents(db, path, { maxHops, maxFiles }).paths;
}

export function insertGenerationReceipt(db, receipt) {
  db.prepare(`INSERT INTO generation_receipt(receipt_id, created_ms, repo_root, generation_id, source_clock, applied_clock, event_gap, barrier_result, details_json)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`).run(
    receipt.receiptId,
    receipt.createdMs,
    receipt.repoRoot,
    receipt.generationId ?? null,
    receipt.sourceClock,
    receipt.appliedClock,
    receipt.eventGap ? 1 : 0,
    receipt.barrierResult,
    JSON.stringify(receipt.details ?? {}),
  );
  return receipt;
}

export function loadGenerationReceipt(db, receiptId) {
  const row = db.prepare("SELECT * FROM generation_receipt WHERE receipt_id=?").get(receiptId);
  if (!row) return null;
  return {
    receiptId: row.receipt_id,
    createdMs: row.created_ms,
    repoRoot: row.repo_root,
    generationId: row.generation_id,
    sourceClock: row.source_clock,
    appliedClock: row.applied_clock,
    eventGap: Boolean(row.event_gap),
    barrierResult: row.barrier_result,
    details: JSON.parse(row.details_json),
  };
}

/**
 * Reconstruct the generation object that saveGeneration persisted.
 *
 * Returns null when the store holds no generation, so a caller can tell "no
 * graph yet, build one" from "graph exists and is empty" — those demand
 * different responses and must never collapse into the same value.
 *
 * Node order is files-then-symbols, which is the order the providers emit and
 * therefore what the JSON artifacts held; several consumers scan for the first
 * matching node, so a stable order is part of the contract, not cosmetic.
 *
 * The contract is SEMANTIC equality, not byte equality: object KEY order can
 * differ, because optional edge fields (`resolved`, `specifier`, `reason`) come
 * back from `extra` and land after `evidence` rather than before it. Verified
 * safe rather than assumed — `generationId` is the only value that hashes
 * JSON.stringify output, and it is computed at build time over the pre-
 * augmentation node set, then carried in the manifest. It is already not
 * recomputable from a persisted generation (re-hashing today's graph.json does
 * not reproduce its own stored id either), so nothing can regress on key order.
 * If a future caller ever needs to re-derive an id from a loaded generation, it
 * must sort keys first — do not assume this returns byte-identical JSON.
 */
export function loadGeneration(db) {
  const envelopeRows = db.prepare("SELECT key, value FROM generation").all();
  if (envelopeRows.length === 0) return null;

  const generation = {};
  for (const row of envelopeRows) {
    try {
      generation[row.key] = JSON.parse(row.value);
    } catch {
      // A corrupt envelope value is a corrupt store, not a recoverable state.
      throw new Error(`graph store envelope key "${row.key}" is not valid JSON`);
    }
  }

  const fileRows = db.prepare(
    "SELECT node_id, path, labels, name, qualified_name, confidence, evidence, extra FROM files ORDER BY rowid",
  ).all();
  const symbolRows = db.prepare(
    "SELECT id, kind, labels, name, qualified_name, path, confidence, evidence, extra FROM symbols ORDER BY rowid",
  ).all();
  const edgeRows = db.prepare(
    "SELECT id, kind, source, target, confidence, evidence, confidence_tier, extra FROM edges ORDER BY rowid",
  ).all();

  generation.nodes = [
    ...fileRows.map(deserializeFileNodeRow),
    ...symbolRows.map(deserializeSymbolNodeRow),
  ];

  // `resolved`, `specifier` and `reason` come back from `extra`, which records
  // exactly the keys the provider emitted — so an edge that never had
  // `specifier` does not gain a null one, and one that had `specifier: null`
  // keeps it. That distinction is why they are not nullable columns.
  generation.edges = edgeRows.map(deserializeEdgeNodeRow);

  // Phase 7.3 — rehydrate `docTruth` from the relational tables. Cheap reads
  // (readManifestEnvelope, indexedMeta) deliberately skip this and so stay
  // sub-millisecond; only callers that asked for the full generation pay it.
  // Empty when the tables hold no rows for the current generation — that is
  // the correct shape for stores with no doc coverage.
  generation.docTruth = loadDocTruth(db, generation.manifest?.generationId ?? null, providerId(generation.provider));

  return generation;
}

function providerId(provider) {
  if (provider == null) return null;
  return typeof provider === "string" ? provider : provider.id ?? null;
}

// Reassemble the docTruth envelope from the four relational tables for one
// generation. Round-trip identity (joins/supersedes byte-equal to what was
// persisted) is best-effort: a join's `id` is the row primary key, and any
// optional fields the producer emitted that we did not promote to columns
// stay absent — see the contract note on `loadGeneration` for the same
// semantic-equality vs byte-equality caveat.
function loadDocTruth(db, generationId, provider = null) {
  if (!generationId) return emptyDocTruth(provider);
  const docRows = db.prepare(`SELECT id, path, content_hash, lifecycle_status, lifecycle_superseded_by, lifecycle_superseded_on, generated_at
    FROM documents WHERE generation_id = ? ORDER BY rowid`).all(generationId);
  const claimRows = db.prepare(`SELECT id, document_id, source, line, text, status, sha1
    FROM claims WHERE generation_id = ? ORDER BY rowid`).all(generationId);
  const edgeRows = db.prepare(`SELECT id, claim_id, kind, source, target, confidence, confidence_class, reason,
    evidence_doc_path, evidence_doc_line, evidence_doc_sha1, evidence_code_path, evidence_code_exists,
    evidence_code_node_id, evidence_code_content_hash
    FROM claim_code_edges WHERE generation_id = ? ORDER BY rowid`).all(generationId);
  const superRows = db.prepare(`SELECT id, source_kind, source_doc, source_line, source_text, source_external,
    target_doc, target_external, target_match, superseded_on
    FROM document_supersession WHERE generation_id = ? ORDER BY rowid`).all(generationId);

  if (docRows.length === 0 && claimRows.length === 0 && edgeRows.length === 0 && superRows.length === 0) {
    return emptyDocTruth(provider);
  }

  const joins = edgeRows.map((row) => ({
    id: row.id,
    kind: row.kind,
    source: row.source,
    target: row.target,
    confidence: row.confidence ?? 1,
    confidenceClass: row.confidence_class ?? undefined,
    reason: row.reason ?? undefined,
    evidence: {
      docRef: row.evidence_doc_path ? { path: row.evidence_doc_path, line: row.evidence_doc_line ?? null, sha1: row.evidence_doc_sha1 ?? null } : null,
      codeRef: row.evidence_code_path ? { path: row.evidence_code_path, exists: row.evidence_code_exists === 0 ? false : true } : null,
      codeNode: row.evidence_code_node_id ? { id: row.evidence_code_node_id, path: row.evidence_code_path ?? null, contentHash: row.evidence_code_content_hash ?? null } : null,
    },
  }));
  const supersedes = superRows.map((row) => {
    const source = row.source_external === 1 ? { external: true } : { doc: row.source_doc };
    if (row.source_line != null && !row.source_external) source.line = row.source_line;
    if (row.source_text && !row.source_external) source.text = row.source_text;
    const target = row.target_external === 1 ? { external: true } : { doc: row.target_doc };
    if (row.superseded_on && !row.target_external) target.supersededOn = row.superseded_on;
    return {
      kind: row.source_kind ?? "supersedes",
      source,
      target,
      evidence: { sourceDoc: row.source_doc ?? null, targetMatch: row.target_match ?? null },
    };
  });
  return {
    schemaVersion: 1,
    provider,
    joins,
    supersedes,
    truncated: false,
    sourceDocMap: {
      docs: docRows.length,
      claims: claimRows.length,
      generatedAt: docRows[0]?.generated_at ?? null,
    },
  };
}

function emptyDocTruth(provider = null) {
  return { schemaVersion: 1, provider, joins: [], supersedes: [], truncated: false, sourceDocMap: { docs: 0, claims: 0, generatedAt: null } };
}

function parseJson(text, fallback) {
  if (text === null || text === undefined) return fallback;
  try { return JSON.parse(text); } catch { return fallback; }
}

function deserializeFileNodeRow(row) {
  return {
    id: row.node_id ?? `file:${row.path}`,
    kind: "file",
    labels: parseJson(row.labels, ["File"]),
    name: row.name,
    qualifiedName: row.qualified_name,
    path: row.path,
    confidence: row.confidence ?? 1,
    evidence: parseJson(row.evidence, []),
    ...parseJson(row.extra, {}),
  };
}

function deserializeSymbolNodeRow(row) {
  return {
    id: row.id,
    kind: row.kind,
    labels: parseJson(row.labels, []),
    name: row.name,
    qualifiedName: row.qualified_name,
    path: row.path,
    confidence: row.confidence ?? 1,
    evidence: parseJson(row.evidence, []),
    ...parseJson(row.extra, {}),
  };
}

function deserializeEdgeNodeRow(row) {
  return {
    id: row.id,
    kind: row.kind,
    source: row.source,
    target: row.target,
    confidence: row.confidence ?? 1,
    ...(row.confidence_tier === null || row.confidence_tier === undefined ? {} : { confidenceTier: row.confidence_tier }),
    evidence: parseJson(row.evidence, []),
    ...parseJson(row.extra, {}),
  };
}

/** Read one envelope value without materialising nodes or edges. */
export function getGenerationEnvelope(db, key = null) {
  const rows = key === null
    ? db.prepare("SELECT key, value FROM generation ORDER BY rowid").all()
    : db.prepare("SELECT key, value FROM generation WHERE key = ?").all(key);
  const envelope = {};
  for (const row of rows) {
    try {
      envelope[row.key] = JSON.parse(row.value);
    } catch {
      throw new Error(`graph store envelope key "${row.key}" is not valid JSON`);
    }
  }
  return key === null ? envelope : envelope[key] ?? null;
}

// ---------------------------------------------------------------------------
// Indexed traversal — SQL frontier expansion.
//
// Phase 7.1 moves the neighbor/path frontier out of JavaScript into a recursive
// CTE that joins `edges` on (generation_id, source/target). The CTE keeps the
// frontier bounded by `maxDepth` and reports the SHORTEST depth each reachable
// node is reached from the seed set. Direction semantics match `indexedNeighbors`:
//   - "out"   follows edges where edge.source is in the frontier -> edge.target is new.
//   - "in"    follows edges where edge.target is in the frontier -> edge.source is new.
//   - "both"  follows either direction in the same pass and reports each node at
//             its shortest depth from the seed, regardless of which side matched.
//
// Caller passes `kinds` to limit which edge kinds participate (null = all). The
// generation_id is REQUIRED: an unfiltered scan over every edge of every
// generation in the store is the very workload this function exists to avoid.
// ---------------------------------------------------------------------------

export function traversalNeighbors(db, options = {}) {
  const seedIds = [...new Set((options.seedIds ?? []).map(String))];
  const maxDepth = Number.isFinite(options.maxDepth) ? Math.max(0, options.maxDepth) : 1;
  const direction = options.direction ?? "both";
  const generationId = String(options.generationId ?? "");
  if (!seedIds.length || !generationId) return { seenNodes: [], seenEdges: [], depths: new Map(), edgeRows: [] };
  const includeKind = Array.isArray(options.kinds) && options.kinds.length > 0;
  const kindParam = includeKind ? JSON.stringify(options.kinds) : null;
  const kindClause = includeKind ? "AND e.kind IN (SELECT value FROM json_each(?))" : "";
  const inClause = direction === "out" ? "e.source = b.node_id"
                  : direction === "in" ? "e.target = b.node_id"
                  : "(e.source = b.node_id OR e.target = b.node_id)";
  const nextId = direction === "out" ? "e.target"
               : direction === "in" ? "e.source"
               : "(CASE WHEN e.source = b.node_id THEN e.target ELSE e.source END)";
  const seedJson = JSON.stringify(seedIds);
  const rows = db.prepare(`
    WITH RECURSIVE frontier(node_id, depth) AS (
      SELECT je.value, 0 FROM json_each(?) je
      UNION
      SELECT ${nextId}, b.depth + 1
      FROM edges e
      JOIN frontier b ON ${inClause}
      WHERE b.depth < ?
        AND e.generation_id = ?
        ${kindClause}
    )
    SELECT node_id, MIN(depth) AS depth FROM frontier GROUP BY node_id ORDER BY depth, node_id
  `).all(seedJson, maxDepth, generationId, ...(includeKind ? [kindParam] : []));
  const seenNodes = rows.map((r) => r.node_id);
  const depths = new Map(rows.map((r) => [r.node_id, r.depth]));
  if (!seenNodes.length) return { seenNodes, seenEdges: [], depths, edgeRows: [] };
  const seenJson = JSON.stringify(seenNodes);
  const edgeRows = db.prepare(`
    SELECT e.id, e.kind, e.source, e.target, e.confidence, e.confidence_tier
    FROM edges e
    WHERE e.generation_id = ?
      AND e.source IN (SELECT value FROM json_each(?))
      AND (e.target IS NULL OR e.target IN (SELECT value FROM json_each(?)))
      ${kindClause}
  `).all(generationId, seenJson, seenJson, ...(includeKind ? [kindParam] : []));
  const seenEdges = new Set(edgeRows.map((row) => row.id));
  return { seenNodes, seenEdges: [...seenEdges], depths, edgeRows };
}

/** Read indexed file hashes for freshness checks, without loading graph nodes. */
export function loadFileContentHashes(db, generationId = null) {
  const rows = generationId === null
    ? db.prepare("SELECT path, content_hash FROM files ORDER BY rowid").all()
    : db.prepare("SELECT path, content_hash FROM files WHERE generation_id = ? ORDER BY rowid").all(generationId);
  return new Map(rows.filter((row) => row.content_hash).map((row) => [row.path, row.content_hash]));
}

/** Slim, ordered edge rows for traversal. No evidence or provider extras are parsed. */
export function listEdgeCore(db, options = {}) {
  const clauses = [];
  const params = [];
  if (options.generationId) { clauses.push("generation_id = ?"); params.push(options.generationId); }
  if (options.source) { clauses.push("source = ?"); params.push(options.source); }
  if (options.target) { clauses.push("target = ?"); params.push(options.target); }
  const where = clauses.length ? `WHERE ${clauses.join(" AND ")}` : "";
  return db.prepare(`
    SELECT id, kind, source, target, confidence, confidence_tier
    FROM edges ${where} ORDER BY rowid
  `).all(...params).map((row) => ({
    id: row.id,
    kind: row.kind,
    source: row.source,
    target: row.target,
    confidence: row.confidence ?? 1,
    ...(row.confidence_tier === null || row.confidence_tier === undefined ? {} : { confidenceTier: row.confidence_tier }),
  }));
}

/** Hydrate only requested nodes, preserving files-then-symbols and rowid order. */
export function hydrateNodesByIds(db, nodeIds) {
  const ids = [...new Set((nodeIds ?? []).map(String))];
  if (!ids.length) return [];
  const json = JSON.stringify(ids);
  const files = db.prepare("SELECT * FROM files WHERE node_id IN (SELECT value FROM json_each(?)) ORDER BY rowid").all(json);
  const symbols = db.prepare("SELECT * FROM symbols WHERE id IN (SELECT value FROM json_each(?)) ORDER BY rowid").all(json);
  return [...files.map(deserializeFileNodeRow), ...symbols.map(deserializeSymbolNodeRow)];
}

/** Hydrate only requested edges, preserving edge rowid order. */
export function hydrateEdgesByIds(db, edgeIds) {
  const ids = [...new Set((edgeIds ?? []).map(String))];
  if (!ids.length) return [];
  return db.prepare("SELECT * FROM edges WHERE id IN (SELECT value FROM json_each(?)) ORDER BY rowid")
    .all(JSON.stringify(ids)).map(deserializeEdgeNodeRow);
}

/** True when the store holds a persisted generation envelope. */
export function hasGeneration(db) {
  const row = db.prepare("SELECT COUNT(*) AS n FROM generation").get();
  return row.n > 0;
}

// ---------------------------------------------------------------------------
// Read helpers.
// ---------------------------------------------------------------------------

export function countRows(db) {
  return {
    files: db.prepare("SELECT COUNT(*) AS n FROM files").get().n,
    symbols: db.prepare("SELECT COUNT(*) AS n FROM symbols").get().n,
    edges: db.prepare("SELECT COUNT(*) AS n FROM edges").get().n,
  };
}

export function getFile(db, path) {
  return db.prepare("SELECT * FROM files WHERE path = ?").get(path) ?? null;
}

export function getSymbol(db, id) {
  const row = db.prepare("SELECT * FROM symbols WHERE id = ?").get(id);
  return row ? deserializeSymbolRow(row) : null;
}

export function listSymbolsByPath(db, path) {
  return db.prepare("SELECT * FROM symbols WHERE path = ? ORDER BY id").all(path).map(deserializeSymbolRow);
}

export function listEdges(db, options = {}) {
  const clauses = [];
  const params = [];
  if (options.kind) { clauses.push("kind = ?"); params.push(options.kind); }
  if (options.source) { clauses.push("source = ?"); params.push(options.source); }
  if (options.target) { clauses.push("target = ?"); params.push(options.target); }
  const where = clauses.length ? `WHERE ${clauses.join(" AND ")}` : "";
  return db.prepare(`SELECT * FROM edges ${where} ORDER BY id`).all(...params).map(deserializeEdgeRow);
}

function deserializeSymbolRow(row) {
  return {
    id: row.id,
    kind: row.kind,
    labels: JSON.parse(row.labels),
    name: row.name,
    qualifiedName: row.qualified_name,
    path: row.path,
    confidence: row.confidence,
    evidence: JSON.parse(row.evidence),
    generationId: row.generation_id,
  };
}

function deserializeEdgeRow(row) {
  return {
    id: row.id,
    kind: row.kind,
    source: row.source,
    target: row.target,
    confidence: row.confidence,
    resolved: Boolean(row.resolved),
    specifier: row.specifier,
    evidence: JSON.parse(row.evidence),
    generationId: row.generation_id,
  };
}

// ---------------------------------------------------------------------------
// Blast radius — recursive CTE over `edges`, bounded by maxDepth.
// ---------------------------------------------------------------------------

// Given a set of changed node ids (typically "file:<path>" ids, but any node
// id works), returns every node TRANSITIVELY UPSTREAM of them — i.e. every
// node that (directly or indirectly) depends on a changed node, via any edge
// kind, walked in the "who points at this" direction (edge.target is in the
// frontier -> edge.source is newly reached). This is the same direction as
// static-provider.mjs's graphImpact({direction:"in"}): "what breaks if I
// change this". Each reached node is reported at its SHORTEST depth from the
// seed set (MIN across all paths that reach it), bounded by `maxDepth`
// (required — an unbounded blast radius on a real repo is a footgun).
export function blastRadius(db, options = {}) {
  const changedNodeIds = options.changedNodeIds ?? options.changedPaths?.map((p) => `file:${p}`) ?? [];
  const maxDepth = Number.isFinite(options.maxDepth) ? Math.max(0, options.maxDepth) : 5;
  if (changedNodeIds.length === 0) return [];

  const seedJson = JSON.stringify(changedNodeIds);
  const rows = db.prepare(`
    WITH RECURSIVE blast(node_id, depth) AS (
      SELECT je.value, 0 FROM json_each(?) je
      UNION
      SELECT e.source, b.depth + 1
      FROM edges e
      JOIN blast b ON e.target = b.node_id
      WHERE b.depth < ?
    )
    SELECT node_id, MIN(depth) AS depth FROM blast GROUP BY node_id ORDER BY depth, node_id
  `).all(seedJson, maxDepth);

  const symbolIds = rows.filter((r) => String(r.node_id).startsWith("symbol:")).map((r) => r.node_id);
  const fileIds = rows.filter((r) => String(r.node_id).startsWith("file:"));
  const symbolById = new Map();
  if (symbolIds.length > 0) {
    const symbolRows = db.prepare("SELECT * FROM symbols WHERE id IN (SELECT value FROM json_each(?))").all(JSON.stringify(symbolIds));
    for (const row of symbolRows) symbolById.set(row.id, deserializeSymbolRow(row));
  }
  const fileByPath = new Map();
  if (fileIds.length > 0) {
    const paths = fileIds.map((r) => String(r.node_id).slice("file:".length));
    const fileRows = db.prepare("SELECT * FROM files WHERE path IN (SELECT value FROM json_each(?))").all(JSON.stringify(paths));
    for (const row of fileRows) fileByPath.set(row.path, row);
  }

  return rows.map((row) => {
    const isSeed = changedNodeIds.includes(row.node_id);
    if (String(row.node_id).startsWith("symbol:")) {
      const symbol = symbolById.get(row.node_id);
      return { nodeId: row.node_id, depth: row.depth, isSeed, kind: symbol?.kind ?? "symbol", name: symbol?.qualifiedName ?? row.node_id, path: symbol?.path ?? null };
    }
    const path = String(row.node_id).slice("file:".length);
    const file = fileByPath.get(path);
    return { nodeId: row.node_id, depth: row.depth, isSeed, kind: "file", name: path, path: file?.path ?? path };
  });
}

// ---------------------------------------------------------------------------
// Optional embedding layer.
//
// Nothing in cortex generates embeddings today, and they stay off by default:
// lexical search plus graph traversal handles most retrieval. These functions only
// persist and search vectors that some other component supplies, and every one of
// them behaves correctly on an empty table.
// ---------------------------------------------------------------------------

/** Float32Array (or plain number array) -> BLOB bytes for storage. */
function vectorToBlob(vector) {
  const f32 = vector instanceof Float32Array ? vector : Float32Array.from(vector);
  return Buffer.from(f32.buffer, f32.byteOffset, f32.byteLength);
}

/**
 * BLOB bytes -> Float32Array without copying.
 * node:sqlite returns a view into a larger pooled buffer, so byteOffset matters —
 * ignoring it silently reads the wrong floats.
 */
function blobToVector(blob, dim) {
  return new Float32Array(blob.buffer, blob.byteOffset, dim);
}

export function upsertVectors(db, generationId, entries, options = {}) {
  const defaultModel = String(options.model ?? "unknown");
  const stmt = db.prepare(
    `INSERT INTO vectors (node_id, dim, emb, model, generation_id) VALUES (?, ?, ?, ?, ?)
     ON CONFLICT(node_id) DO UPDATE SET
       dim = excluded.dim, emb = excluded.emb,
       model = excluded.model, generation_id = excluded.generation_id`
  );
  let written = 0;
  db.exec("BEGIN;");
  try {
    for (const entry of entries ?? []) {
      const vector = entry?.vector ?? entry?.embedding;
      if (!entry?.nodeId || !vector) continue;
      const f32 = vector instanceof Float32Array ? vector : Float32Array.from(vector);
      if (f32.length === 0) continue;
      stmt.run(String(entry.nodeId), f32.length, vectorToBlob(f32), String(entry.model ?? defaultModel), String(generationId));
      written += 1;
    }
    db.exec("COMMIT;");
  } catch (err) {
    db.exec("ROLLBACK;");
    throw err;
  }
  return { written };
}

export function countVectors(db) {
  return db.prepare("SELECT COUNT(*) AS n FROM vectors").get().n;
}

/**
 * Brute-force cosine similarity over stored vectors.
 *
 * Returns an empty result set on an empty table rather than throwing — embeddings
 * are optional, so an absent vector set is a normal state, not an error.
 *
 * A stored row whose `dim` disagrees with the query is SKIPPED and counted in
 * `dimMismatches`, never compared against a truncated or over-read buffer, which
 * would produce a plausible-looking wrong answer.
 */
export function searchSimilar(db, queryVector, options = {}) {
  const limit = Number.isInteger(options.limit) && options.limit > 0 ? options.limit : 10;
  const minScore = typeof options.minScore === "number" ? options.minScore : -Infinity;
  const model = options.model ? String(options.model) : null;

  const q = queryVector instanceof Float32Array ? queryVector : Float32Array.from(queryVector ?? []);
  if (q.length === 0) return { results: [], scanned: 0, dimMismatches: 0 };

  let qNorm = 0;
  for (let i = 0; i < q.length; i += 1) qNorm += q[i] * q[i];
  qNorm = Math.sqrt(qNorm);
  if (qNorm === 0) return { results: [], scanned: 0, dimMismatches: 0 };

  const rows = model
    ? db.prepare("SELECT node_id, dim, emb, model FROM vectors WHERE model = ?").all(model)
    : db.prepare("SELECT node_id, dim, emb, model FROM vectors").all();

  const results = [];
  let dimMismatches = 0;
  for (const row of rows) {
    if (row.dim !== q.length) { dimMismatches += 1; continue; }
    const emb = blobToVector(row.emb, row.dim);
    let dot = 0;
    let norm = 0;
    for (let i = 0; i < row.dim; i += 1) {
      dot += q[i] * emb[i];
      norm += emb[i] * emb[i];
    }
    if (norm === 0) continue;
    const score = dot / (qNorm * Math.sqrt(norm));
    if (score >= minScore) results.push({ nodeId: row.node_id, score, model: row.model });
  }

  results.sort((a, b) => b.score - a.score);
  return { results: results.slice(0, limit), scanned: rows.length, dimMismatches };
}
