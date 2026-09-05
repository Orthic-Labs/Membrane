import { existsSync, mkdirSync, renameSync, writeFileSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { computeManifestDigest, resealGenerationIdentityDelta } from "./generation-identity.mjs";
import { updateLeafChain } from "./merkle-ledger.mjs";
import {
  countRows,
  deleteFactsByOwner,
  getGenerationEnvelope,
  loadFileState,
  recordArtifactState,
  registerProviderRanks,
  reindexNodeOrdinals,
  replaceSymbolSearchEntry,
} from "./store-sqlite.mjs";
import { incrementTelemetry } from "../lib/telemetry.mjs";
import { canonicalProviderId, STATIC_PROVIDER } from "./provider-identity.mjs";
import { normalizeRepoPath } from "./path-order.mjs";
import { confidenceOrLegacyDefault } from "./provenance.mjs";
import { assertCompleteFileBatches } from "./publication-policy.mjs";

export const STRUCTURAL_PROVIDER = Object.freeze({ id: "lexical", version: STATIC_PROVIDER.version });
export const DOC_PROVIDER = Object.freeze({ id: "doctruth", version: "repo-local-doc-v1", freshnessDomain: "doc" });
export const MAX_HOPS = 2;
export const MAX_DEPENDENT_FILES = 500;

function normalizePath(value) {
  return normalizeRepoPath(value);
}

function normalizeDigest(value) {
  const text = String(value ?? "");
  return text.startsWith("xxh128:") ? text : `xxh128:${text}`;
}

function hasTable(db, name) {
  return Boolean(db.prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?").get(name));
}

function ensureWatchState(db) {
  db.exec("CREATE TABLE IF NOT EXISTS watch_state (key TEXT PRIMARY KEY, value TEXT NOT NULL)");
}

function clock(db, key, fallback = 0) {
  const row = db.prepare("SELECT value FROM watch_state WHERE key = ?").get(key);
  return row ? Number(row.value) : fallback;
}

function setClock(db, key, value) {
  db.prepare("INSERT INTO watch_state(key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(key, String(value));
}

export function readPendingDomains(db) {
  ensureWatchState(db);
  return String(db.prepare("SELECT value FROM watch_state WHERE key='domains_pending'").get()?.value ?? "")
    .split(",").map((item) => item.trim()).filter(Boolean).sort();
}

export function markDomainPending(db, domain) {
  const current = readPendingDomains(db);
  if (!current.includes(domain)) current.push(domain);
  setClock(db, "domains_pending", current.sort().join(","));
}

export function clearDomainPending(db, domain) {
  const next = readPendingDomains(db).filter((item) => item !== domain);
  if (next.length) setClock(db, "domains_pending", next.join(","));
  else db.prepare("DELETE FROM watch_state WHERE key='domains_pending'").run();
}

function isDocPath(path) {
  const normalized = normalizePath(path);
  return normalized.endsWith(".md") || ["AGENTS.md", "CLAUDE.md", "README.md"].includes(normalized);
}

function replaceBySource(entries, source, replacements) {
  const output = [];
  let inserted = false;
  for (const entry of Array.isArray(entries) ? entries : []) {
    if (entry?.source === source) {
      if (!inserted) {
        output.push(...replacements);
        inserted = true;
      }
      continue;
    }
    output.push(entry);
  }
  if (!inserted) output.push(...replacements);
  return output;
}

function readJsonIfPresent(path, fallback) {
  if (!existsSync(path)) return fallback;
  try { return JSON.parse(readFileSync(path, "utf8")); } catch { return fallback; }
}

function writeJsonAtomic(path, value, writer = null) {
  if (writer) return writer(path, value);
  mkdirSync(dirname(path), { recursive: true });
  const temp = `${path}.tmp-${process.pid}`;
  writeFileSync(temp, `${JSON.stringify(value, null, 2)}\n`);
  renameSync(temp, path);
}

function restoreTextAtomic(path, text) {
  const temp = `${path}.rollback-${process.pid}`;
  writeFileSync(temp, text);
  renameSync(temp, path);
}

function replaceDocumentArtifacts(db, delta, generationId, options = {}) {
  const root = options.repoRoot;
  const outDir = options.outDir ?? ".agent";
  if (!root) return;
  const deleting = delta.eventKind === "delete";
  const document = deleting ? { claims: [], codeRefs: [], lifecycle: null, contentHash: "tombstone" } : (delta.document ?? { claims: [], codeRefs: [], lifecycle: null });
  const artifactRoot = join(root, outDir);
  const oldSource = normalizePath(delta.path);
  const source = delta.eventKind === "rename" ? normalizePath(delta.renameTo ?? delta.path) : oldSource;
  const claimsPath = join(artifactRoot, "claims.json");
  const stalePath = join(artifactRoot, "stale.json");
  const queuePath = join(artifactRoot, "queue.json");
  const originals = new Map([claimsPath, stalePath, queuePath]
    .filter((path) => existsSync(path))
    .map((path) => [path, readFileSync(path, "utf8")]));
  const fingerprint = normalizeDigest(delta.contentDigest ?? document.contentHash ?? "tombstone");
  const claims = document.claims ?? [];
  const staleClaims = claims.filter((claim) => claim.status === "stale");
  const stale = readJsonIfPresent(stalePath, { missingReferences: [], staleClaims: [], invalidSupersessionMarkers: [] });
  const nextStale = {
    ...stale,
    missingReferences: [
      ...(stale.missingReferences ?? []).filter((item) => ![oldSource, source].includes(item.source)),
      ...(document.codeRefs ?? []).filter((ref) => ref.exists === false).map((ref) => ({ source, path: ref.path })),
    ],
    staleClaims: replaceBySource(replaceBySource(stale.staleClaims, oldSource, []), source, staleClaims),
    invalidSupersessionMarkers: [
      ...(stale.invalidSupersessionMarkers ?? []).filter((item) => ![oldSource, source].includes(item.source)),
      ...(document.lifecycle?.invalidMarker ? [{ source, ...document.lifecycle.invalidMarker }] : []),
    ],
  };
  try {
    if (existsSync(claimsPath)) {
      writeJsonAtomic(claimsPath, replaceBySource(replaceBySource(readJsonIfPresent(claimsPath, []), oldSource, []), source, claims), options.writeJsonAtomic);
      recordArtifactState(db, "claims.json", generationId, fingerprint);
    }
    if (existsSync(stalePath)) {
      writeJsonAtomic(stalePath, nextStale, options.writeJsonAtomic);
      recordArtifactState(db, "stale.json", generationId, fingerprint);
    }
    if (existsSync(queuePath)) {
      const candidateFiles = (document.codeRefs ?? []).filter((ref) => ref.exists !== false).map((ref) => ref.path);
      const queueClaims = claims.map((claim) => ({
        id: claim.id,
        source: claim.source,
        line: claim.line,
        status: claim.status,
        text: claim.text,
        candidateFiles,
      }));
      writeJsonAtomic(queuePath, {
        ...readJsonIfPresent(queuePath, {}),
        claims: replaceBySource(replaceBySource(readJsonIfPresent(queuePath, {}).claims, oldSource, []), source, queueClaims),
      }, options.writeJsonAtomic);
      recordArtifactState(db, "queue.json", generationId, fingerprint);
    }
  } catch (error) {
    for (const [path, text] of originals) restoreTextAtomic(path, text);
    throw error;
  }
}

function fileNode(node, generationId, digest, provider, fileReport = null) {
  const extra = Object.fromEntries(Object.entries(node).filter(([key]) => !["id", "kind", "labels", "name", "qualifiedName", "path", "confidence", "evidence"].includes(key)));
  return {
    path: node.path,
    contentHash: String(digest).replace(/^xxh128:/, ""),
    language: fileReport?.language ?? null,
    provider: fileReport && Object.hasOwn(fileReport, "provider") ? fileReport.provider : provider.id,
    parseStatus: fileReport?.parseStatus ?? null,
    errorNodeCount: fileReport?.errorNodeCount ?? null,
    generationId,
    nodeId: node.id,
    labels: JSON.stringify(node.labels ?? ["File"]),
    name: node.name ?? node.path.split("/").at(-1),
    qualifiedName: node.qualifiedName ?? node.path,
    confidence: confidenceOrLegacyDefault(node.confidence),
    evidence: JSON.stringify(node.evidence ?? []),
    extra: Object.keys(extra).length ? JSON.stringify(extra) : null,
  };
}

function insertParsedFacts(db, parsed, generationId, sourceDigest, provider, fileReport = null, repoRoot = null) {
  provider = { ...provider, id: canonicalProviderId(provider.id), version: String(provider.version ?? "unknown") };
  const insertFile = db.prepare(`INSERT INTO files(path, content_hash, language, provider, parse_status, error_node_count, generation_id, node_id, labels, name, qualified_name, confidence, evidence, extra, node_ordinal)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(path) DO UPDATE SET content_hash=excluded.content_hash, language=excluded.language, provider=excluded.provider,
      parse_status=excluded.parse_status, error_node_count=excluded.error_node_count, generation_id=excluded.generation_id,
      node_id=excluded.node_id, labels=excluded.labels, name=excluded.name, qualified_name=excluded.qualified_name,
      confidence=excluded.confidence, evidence=excluded.evidence, extra=excluded.extra, node_ordinal=excluded.node_ordinal`);
  const hasSymbolOrdinal = db.prepare("PRAGMA table_info(symbols)").all().some((column) => column.name === "node_ordinal");
  const insertSymbol = db.prepare(`INSERT INTO symbols(id, kind, labels, name, qualified_name, path, confidence, evidence, generation_id, extra${hasSymbolOrdinal ? ", node_ordinal" : ""})
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?${hasSymbolOrdinal ? ", ?" : ""})
    ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, labels=excluded.labels, name=excluded.name, qualified_name=excluded.qualified_name,
       path=excluded.path, confidence=excluded.confidence, evidence=excluded.evidence, generation_id=excluded.generation_id, extra=excluded.extra${hasSymbolOrdinal ? ", node_ordinal=excluded.node_ordinal" : ""}`);
  let insertAnnotation = null;
  const insertEdge = db.prepare(`INSERT INTO edges(id, kind, source, target, confidence, resolved, specifier, evidence, generation_id, confidence_tier, extra)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, source=excluded.source, target=excluded.target, confidence=excluded.confidence,
      resolved=excluded.resolved, specifier=excluded.specifier, evidence=excluded.evidence, generation_id=excluded.generation_id,
      confidence_tier=excluded.confidence_tier, extra=excluded.extra`);
  const insertOwner = db.prepare("INSERT OR REPLACE INTO fact_owner(fact_id, fact_kind, source_path, source_digest, provider_id, provider_version, freshness_domain, fact_kind_detail, generation_id, repo_root) VALUES (?, ?, ?, ?, ?, ?, 'structural', ?, ?, ?)");
  const nodes = parsed?.nodes ?? [];
  for (const [nodeOrdinal, node] of nodes.entries()) {
    const stored = node.path === undefined ? node : { ...node, path: normalizePath(node.path) };
    if (db.prepare("SELECT 1 FROM node_provider WHERE node_id=?").get(stored.id)) continue;
    const physical = db.prepare(`SELECT 1 FROM files WHERE node_id=? UNION ALL SELECT 1 FROM symbols WHERE id=? UNION ALL SELECT 1 FROM annotation_nodes WHERE id=? LIMIT 1`).get(stored.id, stored.id, stored.id);
    if (physical) throw Object.assign(new Error(`duplicate_node_id:${stored.id}`), { code: "duplicate_node_id" });
    let inserted = true;
    if (stored.kind === "file") {
      const row = fileNode(stored, generationId, sourceDigest, provider, fileReport);
      if (db.prepare("SELECT 1 FROM files WHERE path=?").get(row.path)) inserted = false;
      else insertFile.run(row.path, row.contentHash, row.language, row.provider, row.parseStatus, row.errorNodeCount, row.generationId, row.nodeId, row.labels, row.name, row.qualifiedName, row.confidence, row.evidence, row.extra, nodeOrdinal);
    } else if (stored.kind === "comment") {
      insertAnnotation ??= db.prepare("INSERT INTO annotation_nodes(id, node_ordinal, payload, generation_id) VALUES (?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET node_ordinal=excluded.node_ordinal, payload=excluded.payload, generation_id=excluded.generation_id");
      insertAnnotation.run(stored.id, nodeOrdinal, JSON.stringify(stored), generationId);
    } else {
      const extra = Object.fromEntries(Object.entries(stored).filter(([key]) => !["id", "kind", "labels", "name", "qualifiedName", "path", "confidence", "evidence"].includes(key)));
      const values = [stored.id, stored.kind, JSON.stringify(stored.labels ?? []), stored.name ?? "", stored.qualifiedName ?? stored.name ?? "", stored.path, confidenceOrLegacyDefault(stored.confidence), JSON.stringify(stored.evidence ?? []), generationId, Object.keys(extra).length ? JSON.stringify(extra) : null];
      if (hasSymbolOrdinal) values.push(nodeOrdinal);
      insertSymbol.run(...values);
    }
    if (!inserted) continue;
    registerProviderRanks(db, [provider.id]);
    db.prepare("INSERT INTO node_provider(node_id,provider_id,source_path,provider_version) VALUES (?,?,?,?)").run(stored.id, provider.id, stored.path ?? "", provider.version);
    insertOwner.run(stored.id, "node", stored.path, sourceDigest, provider.id, provider.version, stored.kind, generationId, repoRoot);
    if (stored.kind !== "file" && stored.kind !== "comment") replaceSymbolSearchEntry(db, { id: stored.id, generationId, name: stored.name, qualifiedName: stored.qualifiedName, path: stored.path });
  }
  for (const edge of parsed?.edges ?? []) {
    const indexed = new Set(["id", "kind", "source", "target", "confidence", "confidenceTier", "evidence"]);
    const extra = Object.fromEntries(Object.entries(edge).filter(([key]) => !indexed.has(key)));
    const exists = db.prepare("SELECT 1 FROM edges WHERE id=?").get(edge.id);
    if (!exists) insertEdge.run(edge.id, edge.kind, edge.source, edge.target ?? null, confidenceOrLegacyDefault(edge.confidence), edge.resolved === false ? 0 : 1, edge.specifier ?? null, JSON.stringify(edge.evidence ?? []), generationId, edge.confidenceTier ?? null, Object.keys(extra).length ? JSON.stringify(extra) : null);
    const sourceNode = nodes.find((node) => node.id === edge.source);
    if (sourceNode && !exists) insertOwner.run(edge.id, "edge", sourceNode.path, sourceDigest, provider.id, provider.version, edge.kind, generationId, repoRoot);
  }
}

function refreshDependencies(db, path, dependencies) {
  db.prepare("DELETE FROM dependency_index WHERE source_path = ? OR dependent_path = ?").run(path, path);
  const insert = db.prepare("INSERT OR REPLACE INTO dependency_index(source_path, dependent_path, reason) VALUES (?, ?, ?)");
  for (const dependency of dependencies ?? []) {
    insert.run(normalizePath(dependency.sourcePath), normalizePath(dependency.dependentPath), dependency.reason);
  }
}

function updateFileState(db, path, digest, sourceClock, fileIdentity = null, size = 0, mtimeMs = null, eventSeq = null) {
  db.prepare(`INSERT INTO file_state(path, content_digest, size, mtime_ms, file_identity, last_event_seq, applied_clock)
    VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(path) DO UPDATE SET content_digest=excluded.content_digest, size=excluded.size,
      mtime_ms=excluded.mtime_ms, file_identity=excluded.file_identity, last_event_seq=excluded.last_event_seq, applied_clock=excluded.applied_clock`)
    .run(path, normalizeDigest(digest), size, mtimeMs, fileIdentity, eventSeq, sourceClock);
}

function refreshManifestCounts(manifest, db) {
  const counts = countRows(db);
  return { ...manifest, counts: { ...manifest.counts, nodes: counts.files + counts.symbols + counts.annotations, edges: counts.edges } };
}

function requireOuterTransaction(db) {
  if (db.isTransaction) return;
  const error = new Error("delta_transaction_required");
  error.code = "delta_transaction_required";
  throw error;
}

export function applyFileDelta(db, delta, options = {}) {
  const path = normalizePath(delta.path);
  const eventKind = delta.eventKind ?? "modify";
  const rawProvider = delta.provider ?? (isDocPath(path) ? DOC_PROVIDER : STRUCTURAL_PROVIDER);
  const provider = { ...rawProvider, id: canonicalProviderId(rawProvider.id) };
  const isDocumentDelta = delta.domain === "doc" || provider.id === DOC_PROVIDER.id || Boolean(delta.document) || isDocPath(path);
  const contentDigestValue = delta.contentDigest == null ? null : normalizeDigest(delta.contentDigest);
  const newPath = eventKind === "rename" ? normalizePath(delta.renameTo ?? delta.parsed?.nodes?.find((node) => node.kind === "file")?.path ?? path) : path;
  const factBatches = (Array.isArray(delta.factBatches) && delta.factBatches.length
    ? delta.factBatches
    : (delta.parsed ? [{ provider, parsed: delta.parsed, fileReport: delta.fileReport ?? null }] : []))
    .map((batch) => ({ ...batch, provider: { ...batch.provider, id: canonicalProviderId(batch.provider?.id ?? provider.id) } }));
  const ownsTransaction = options.inTransaction !== true;
  if (ownsTransaction) db.exec("BEGIN IMMEDIATE");
  else requireOuterTransaction(db);
  try {
    const prior = loadFileState(db, path);
    ensureWatchState(db);
    const previousApplied = clock(db, "applied_clock", 0);
    const appliedClock = Number(delta.sourceClock ?? previousApplied + 1);
    let orderWrites = 0;
    if (contentDigestValue && prior?.content_digest === contentDigestValue && !["delete", "rename", "repair"].includes(eventKind)) {
      const acknowledgedClock = Math.max(previousApplied, appliedClock);
      setClock(db, "applied_clock", acknowledgedClock);
      db.prepare(`UPDATE file_state SET applied_clock=?, last_event_seq=COALESCE(?, last_event_seq),
        size=COALESCE(?, size), mtime_ms=COALESCE(?, mtime_ms), file_identity=COALESCE(?, file_identity) WHERE path=?`)
        .run(acknowledgedClock, delta.journalSeq ?? null, delta.size ?? null, delta.mtimeMs ?? null, delta.fileIdentity ?? null, path);
      if (delta.journalSeq && hasTable(db, "event_journal")) {
        db.prepare("UPDATE event_journal SET applied = 1, applied_clock = ? WHERE seq = ?").run(acknowledgedClock, delta.journalSeq);
      }
      options.beforeCommit?.(db);
      if (ownsTransaction) db.exec("COMMIT");
      return {
        noop: true,
        applied: true,
        path,
        appliedClock: acknowledgedClock,
        orderWrites: 0,
        rootDigest: db.prepare("SELECT digest FROM generation_leaf WHERE path = '' AND kind = 'dir'").get()?.digest ?? null,
      };
    }
    if (eventKind !== "delete") assertCompleteFileBatches(factBatches, newPath);
    const structuralProviders = [...new Set(factBatches.map((batch) => batch.provider?.id).filter(Boolean))];
    const oldOwners = (isDocumentDelta || eventKind === "delete" || eventKind === "rename" || structuralProviders.length === 0)
      ? db.prepare("SELECT fact_id, fact_kind FROM fact_owner WHERE source_path = ?").all(path)
      : db.prepare(`SELECT fact_id, fact_kind FROM fact_owner WHERE source_path = ? AND provider_id IN (${structuralProviders.map(() => "?").join(",")})`).all(path, ...structuralProviders);
    const oldNodeIds = oldOwners.filter((owner) => owner.fact_kind === "node").map((owner) => owner.fact_id);
    if (eventKind === "delete" || eventKind === "rename") {
      for (const nodeId of oldNodeIds) db.prepare("UPDATE edges SET resolved = 0 WHERE target = ?").run(nodeId);
      deleteFactsByOwner(db, path, null);
      db.prepare("DELETE FROM files WHERE path = ?").run(path);
      db.prepare("DELETE FROM file_state WHERE path = ?").run(path);
      updateLeafChain(db, path, null);
    }
    if (isDocumentDelta && eventKind !== "delete") {
      const docEnvelope = getGenerationEnvelope(db);
      const rootDigest = updateLeafChain(db, newPath, contentDigestValue);
      const envelope = resealGenerationIdentityDelta(docEnvelope, rootDigest, appliedClock);
      const generationId = envelope?.manifest?.generationId;
      const repoRoot = options.repoRoot ?? envelope?.repoRoot ?? null;
      for (const providerId of structuralProviders) deleteFactsByOwner(db, path, providerId);
      const composedFileReport = factBatches.find((batch) => batch.fileReport)?.fileReport ?? null;
      for (const batch of factBatches) {
        insertParsedFacts(db, batch.parsed, generationId, contentDigestValue, batch.provider, batch.fileReport ?? composedFileReport ?? { provider: null }, repoRoot);
      }
      db.prepare("DELETE FROM fact_owner WHERE source_path = ? AND provider_id = ?").run(newPath, provider.id);
      for (const claim of delta.document?.claims ?? []) {
        db.prepare(`INSERT OR REPLACE INTO fact_owner(fact_id, fact_kind, source_path, source_digest, provider_id, provider_version, freshness_domain, fact_kind_detail, generation_id, repo_root)
          VALUES (?, 'node', ?, ?, ?, ?, 'doc', 'claim', ?, ?)`)
          .run(claim.id, newPath, contentDigestValue, provider.id, provider.version, generationId ?? null, repoRoot);
      }
      updateFileState(db, newPath, contentDigestValue, appliedClock, delta.fileIdentity ?? null, delta.size ?? 0, delta.mtimeMs ?? null, delta.journalSeq ?? null);
      orderWrites = reindexNodeOrdinals(db).orderWrites;
      Object.assign(envelope.manifest, refreshManifestCounts(envelope.manifest, db));
      envelope.manifest.manifestDigest = computeManifestDigest(envelope.manifest, envelope.sourceObservation);
      db.prepare("UPDATE generation SET value=? WHERE key='manifest'").run(JSON.stringify(envelope.manifest));
    } else if (eventKind !== "delete" && eventKind !== "rename") {
      for (const providerId of structuralProviders) deleteFactsByOwner(db, path, providerId);
    }
    db.prepare("DELETE FROM dependency_index WHERE source_path = ? OR dependent_path = ?").run(path, path);
    if (!isDocumentDelta && eventKind !== "delete" && factBatches.length) {
      const rootBefore = getGenerationEnvelope(db);
      const rootDigest = updateLeafChain(db, newPath, contentDigestValue);
      const envelope = resealGenerationIdentityDelta(rootBefore, rootDigest, appliedClock);
      const repoRoot = options.repoRoot ?? envelope.repoRoot ?? null;
      const dependencies = [];
      const composedFileReport = factBatches.find((batch) => batch.fileReport)?.fileReport ?? null;
      for (const batch of factBatches) {
          insertParsedFacts(db, batch.parsed, envelope.manifest.generationId, contentDigestValue, batch.provider, batch.fileReport ?? composedFileReport, repoRoot);
          dependencies.push(...(batch.parsed?.dependencies ?? []));
        }
        orderWrites = reindexNodeOrdinals(db).orderWrites;
      const uniqueDependencies = [...new Map(dependencies.map((item) => [`${item.sourcePath}:${item.dependentPath}:${item.reason}`, item])).values()];
      refreshDependencies(db, newPath, uniqueDependencies);
      updateFileState(db, newPath, contentDigestValue, appliedClock, delta.fileIdentity ?? null, delta.size ?? factBatches[0]?.parsed?.size ?? 0, delta.mtimeMs ?? null, delta.journalSeq ?? null);
      Object.assign(envelope.manifest, refreshManifestCounts(envelope.manifest, db));
      envelope.manifest.manifestDigest = computeManifestDigest(envelope.manifest, envelope.sourceObservation);
      db.prepare("UPDATE generation SET value = ? WHERE key = 'manifest'").run(JSON.stringify(envelope.manifest));
    } else if (!isDocumentDelta) {
      orderWrites = reindexNodeOrdinals(db).orderWrites;
      const envelope = getGenerationEnvelope(db);
      const rootDigest = updateLeafChain(db, path, null);
      const resealed = resealGenerationIdentityDelta(envelope, rootDigest, appliedClock);
      Object.assign(resealed.manifest, refreshManifestCounts(resealed.manifest, db));
      resealed.manifest.manifestDigest = computeManifestDigest(resealed.manifest, resealed.sourceObservation);
      db.prepare("UPDATE generation SET value = ? WHERE key = 'manifest'").run(JSON.stringify(resealed.manifest));
    }
    if (isDocumentDelta && eventKind === "delete") {
      orderWrites = reindexNodeOrdinals(db).orderWrites;
      const envelope = getGenerationEnvelope(db);
      const rootDigest = db.prepare("SELECT digest FROM generation_leaf WHERE path='' AND kind='dir'").get()?.digest ?? null;
      const resealed = resealGenerationIdentityDelta(envelope, rootDigest, appliedClock);
      Object.assign(resealed.manifest, refreshManifestCounts(resealed.manifest, db));
      resealed.manifest.manifestDigest = computeManifestDigest(resealed.manifest, resealed.sourceObservation);
      db.prepare("UPDATE generation SET value=? WHERE key='manifest'").run(JSON.stringify(resealed.manifest));
    }
    if (isDocumentDelta) {
      const generationId = getGenerationEnvelope(db)?.manifest?.generationId;
      replaceDocumentArtifacts(db, delta, generationId, options);
      markDomainPending(db, "doc");
    }
    setClock(db, "applied_clock", appliedClock);
    if (delta.journalSeq && hasTable(db, "event_journal")) {
      db.prepare("UPDATE event_journal SET applied = 1, applied_clock = ? WHERE seq = ?").run(appliedClock, delta.journalSeq);
    }
    if (eventKind !== "repair") incrementTelemetry(db, "deltas_applied");
    options.beforeCommit?.(db);
    if (ownsTransaction) db.exec("COMMIT");
    return { applied: true, path, appliedClock, orderWrites, rootDigest: db.prepare("SELECT digest FROM generation_leaf WHERE path = '' AND kind = 'dir'").get()?.digest ?? null };
  } catch (error) {
    if (ownsTransaction) db.exec("ROLLBACK");
    throw error;
  }
}
