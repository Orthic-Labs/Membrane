import { existsSync, readFileSync, realpathSync } from "node:fs";
import { join, resolve } from "node:path";
import { diffLedgerAgainstTree } from "../src/graph/merkle-ledger.mjs";
import { normalizeIgnoredPrefixes } from "../src/graph/ignored-prefixes.mjs";
import { reconcileRenameAliases } from "../src/graph/reanchor.mjs";
import { scanSourceMetadataPublic, scanSourcesForPublication, sourceHashPublic } from "../src/graph/static-provider.mjs";
import { stableRead } from "../src/graph/stable-read.mjs";
import { assertSafeMutableStorePath, closeStore, getGenerationEnvelope, loadGeneration, openStore } from "../src/graph/store-sqlite.mjs";
import { gitSourceObservation } from "../src/graph/git-source-observation.mjs";
import { computeManifestDigest } from "../src/graph/generation-identity.mjs";
import { acquireStoreLease } from "../src/graph/store-lease.mjs";
import { eventsSince, writeSnapshot } from "./adapter.mjs";
import { appendWatchEvents, drainJournal } from "./repo-actor.mjs";
import { completePendingDocDomain } from "../src/lib/phase2-completion.mjs";

function cancelled() { return Object.assign(new Error("request cancelled"), { code: "request_cancelled" }); }
function throwIfAborted(signal) { if (signal?.aborted) throw cancelled(); }
function sleep(ms, signal) { return new Promise((resolvePromise, reject) => {
  if (signal?.aborted) return reject(cancelled());
  const timer = setTimeout(resolvePromise, ms);
  signal?.addEventListener("abort", () => { clearTimeout(timer); reject(cancelled()); }, { once: true });
}); }

// Same source of truth the build-time walk uses (.agent/config.json →
// ignoredPrefixes, via graph/static-provider.mjs configuredIgnoredPrefixes);
// duplicated read here because that helper is module-private and watch-side
// code must not import the whole provider for one config lookup.
function readConfiguredIgnoredPrefixes(root, outDir = ".agent") {
  try {
    const config = JSON.parse(readFileSync(join(resolve(root), outDir, "config.json"), "utf8"));
    return normalizeIgnoredPrefixes(config?.ignoredPrefixes);
  } catch {
    return [];
  }
}

function snapshotPath(root, outDir) { return join(resolve(root), outDir, "graph", "watch.snapshot"); }
function canonicalRoot(value) { const root = resolve(value); try { return realpathSync(root); } catch { return root; } }

function isDocumentPath(path) {
  const normalized = String(path ?? "").replaceAll("\\", "/");
  return normalized.endsWith(".md") || ["AGENTS.md", "CLAUDE.md", "README.md"].includes(normalized);
}

// A saved native-watch snapshot can replay an old create/update after a full
// build has already published identical document content. Keep real edits
// observable, but do not reopen doc freshness for unchanged startup replay.
function isUnchangedDocumentEvent(db, root, event) {
  if (!isDocumentPath(event?.path) || !["create", "modify"].includes(event?.eventKind)) return false;
  const path = String(event.path).replaceAll("\\", "/");
  const leaf = db.prepare("SELECT digest FROM generation_leaf WHERE path=? AND kind='file'").get(path);
  if (!leaf) return false;
  try { return stableRead(join(root, path)).contentDigest === leaf.digest; }
  catch { return false; }
}

function metadataEvents(db, root, scanMetadata = scanSourceMetadataPublic, walkOptions = {}) {
  const recorded = new Map(db.prepare("SELECT path,size,mtime_ms,file_identity FROM file_state").all().map((row) => [row.path, row]));
  const current = new Map(scanMetadata(root, walkOptions).files.map((file) => [file.path, file]));
  const events = [];
  const observedMs = Date.now();
  for (const [path, file] of current) {
    const prior = recorded.get(path);
    if (!prior) events.push({ eventKind: "create", path, observedMs });
    else if (Number(prior.size) !== file.size || Number(prior.mtime_ms) !== file.mtimeMs || (prior.file_identity ?? null) !== file.identity) {
      events.push({ eventKind: "modify", path, observedMs });
    }
  }
  for (const path of recorded.keys()) if (!current.has(path)) events.push({ eventKind: "delete", path, observedMs });
  return events;
}

function coalesceRenameEvents(events) {
  const renames = events.filter((event) => event.eventKind === "rename" && event.renameTo);
  return events.filter((event) => event.eventKind === "rename" || !renames.some((rename) => (
    (event.eventKind === "delete" && event.path === rename.path)
    || (event.eventKind === "create" && event.path === rename.renameTo)
  )));
}

// BPT-013: entity/claim-level identity carried by a detected file-level
// rename/move. A generation node has no `text`/`fingerprint` field of its
// own, so it is mapped onto the abstract "fact" shape `reconcileRenameAliases`
// (blueprint/src/graph/reanchor.mjs) expects: `portableId` when the node
// already carries one (BPT-013's stable identity, path-independent); the
// evidence content hash as an exact-byte `fingerprint`, but only for a `file`
// node — a symbol's evidence hash is its *containing file's* digest (the
// lexical/tree-sitter providers do not hash individual spans), so every
// symbol in one file shares it and treating it as that symbol's own
// fingerprint would make ordinary multi-symbol files collide at that tier
// instead of correctly separating by entity; and the qualified name as the
// last, still-exact, normalized-text tier for everything else. None of this
// is fuzzy: ambiguous or unmatched entities come back typed
// `ambiguous`/`stale` from reanchorEvidence and are reported unresolved
// rather than guessed.
export function renameFact(node) {
  return {
    id: node.id,
    path: node.path,
    portableId: node.portableId ?? null,
    fingerprint: node.kind === "file" ? (node.evidence?.[0]?.contentHash ?? null) : null,
    text: node.qualifiedName ?? node.name ?? null,
  };
}

// The real production seam for BPT-013 entity-level rename/move
// reconciliation: every rename `coalesceRenameEvents` recognises drives
// `reconcileRenameAliases` over the generation snapshot from immediately
// before the journal drain (the entities as they stood at the old path) and
// immediately after it (the entities the drain produced at the new path).
// `reconcileRenameAliases` itself only ever emits an alias for an unambiguous
// exact match; everything else is returned typed and unresolved.
export function finishEntityRenames(renameEvents, before, after) {
  return renameEvents.map((event) => reconcileRenameAliases({
    oldPath: event.path,
    newPath: event.renameTo,
    before: (before?.nodes ?? []).map(renameFact),
    after: (after?.nodes ?? []).map(renameFact),
  }));
}

export function evaluateConvergenceOracle(db, sourceFiles, { traversalTruncated = false, truncationReasons = [], eventGapOverride } = {}) {
  const ledgerRows = Number(db.prepare("SELECT COUNT(*) AS n FROM generation_leaf WHERE kind='file'").get()?.n ?? 0);
  const delta = ledgerRows > 0
    ? diffLedgerAgainstTree(db, null, sourceFiles)
    : { changed: [], added: [], removed: [] };
  const pendingEvents = Number(db.prepare("SELECT COUNT(*) AS n FROM event_journal WHERE applied=0").get()?.n ?? 0);
  const domainsPending = String(db.prepare("SELECT value FROM watch_state WHERE key='domains_pending'").get()?.value ?? "")
    .split(",").map((value) => value.trim()).filter(Boolean).sort();
  const eventGap = eventGapOverride ?? (db.prepare("SELECT value FROM watch_state WHERE key='event_gap'").get()?.value === "1");
  const mismatches = Object.freeze({ changed: delta.changed, added: delta.added, removed: delta.removed });
  const converged = !traversalTruncated && !eventGap && pendingEvents === 0 && domainsPending.length === 0
    && mismatches.changed.length === 0 && mismatches.added.length === 0 && mismatches.removed.length === 0;
  return Object.freeze({
    schemaVersion: 1,
    kind: "IncrementalConvergenceOracle",
    converged,
    exact: !traversalTruncated && ledgerRows > 0,
    mismatches,
    pendingEvents,
    domainsPending,
    eventGap,
    omissions: traversalTruncated
      ? [...truncationReasons].map((reason) => ({ reason: "source_scan_truncated", detail: reason }))
      : ledgerRows > 0 ? [] : [{ reason: "source_ledger_unavailable" }],
  });
}

/** Publish from the write owner only after exact indexed bytes match disk. */
export function publishCurrentSourceObservation(db, root, { ignore = [], outDir = ".agent", scan = scanSourcesForPublication } = {}) {
  const before = gitSourceObservation(root);
  if (!before) return { published: false, reason: "vcs_unavailable" };
  const state = Object.fromEntries(db.prepare("SELECT key,value FROM watch_state").all().map(({ key, value }) => [key, value]));
  if (state.event_gap === "1" || state.repair_progress
    || Number(state.applied_clock ?? 0) !== Number(state.source_clock ?? 0)
    || db.prepare("SELECT 1 FROM event_journal WHERE applied=0 LIMIT 1").get()) {
    return { published: false, reason: "pending_work" };
  }
  const envelope = getGenerationEnvelope(db);
  if (!envelope?.manifest?.generationId) return { published: false, reason: "graph_missing" };
  const rootDigest = db.prepare("SELECT digest FROM generation_leaf WHERE path='' AND kind='dir'").get()?.digest;
  const source = scan(root, outDir, { ignoredPrefixes: normalizeIgnoredPrefixes([...readConfiguredIgnoredPrefixes(root, outDir), ...ignore]) });
  const convergence = evaluateConvergenceOracle(db, source.files ?? [], source);
  if (!convergence.converged) return { published: false, reason: convergence.domainsPending.length ? "pending_domains" : "source_mismatch", convergence };
  const exact = diffLedgerAgainstTree(db, null, source.files ?? []);
  if (source.fileLimitReached || source.truncationReasons?.length
    || exact.changed.length || exact.added.length || exact.removed.length) {
    return { published: false, reason: "source_mismatch" };
  }
  // Porcelain status does not change when an already-dirty file changes again.
  // Re-read exact bytes: metadata/status equality alone cannot attest a tree.
  const verifiedSource = scan(root, outDir, { ignoredPrefixes: normalizeIgnoredPrefixes([...readConfiguredIgnoredPrefixes(root, outDir), ...ignore]) });
  if (verifiedSource.traversalTruncated || verifiedSource.fileLimitReached || verifiedSource.truncationReasons?.length
    || verifiedSource.files.length !== source.files.length
    || source.files.some((file, index) => file.path !== verifiedSource.files[index].path
      || file.contentHash !== verifiedSource.files[index].contentHash)) {
    return { published: false, reason: "source_mismatch" };
  }
  const after = gitSourceObservation(root);
  if (!after || before.head !== after.head || before.statusDigest !== after.statusDigest) {
    return { published: false, reason: "source_mismatch" };
  }
  const manifest = { ...envelope.manifest, repo: { ...envelope.manifest.repo,
    sourceHash: sourceHashPublic(source.files ?? []), fileCount: source.files.length, baseCommit: after.head }, manifestDigest: undefined };
  manifest.manifestDigest = computeManifestDigest(manifest, after);
  db.exec("BEGIN IMMEDIATE");
  try {
    const current = Object.fromEntries(db.prepare("SELECT key,value FROM watch_state").all().map(({ key, value }) => [key, value]));
    if (getGenerationEnvelope(db)?.manifest?.generationId !== envelope.manifest.generationId
      || db.prepare("SELECT digest FROM generation_leaf WHERE path='' AND kind='dir'").get()?.digest !== rootDigest
      || current.source_clock !== state.source_clock || current.applied_clock !== state.applied_clock
      || current.event_gap !== state.event_gap || current.domains_pending !== state.domains_pending
      || current.repair_progress || db.prepare("SELECT 1 FROM event_journal WHERE applied=0 LIMIT 1").get()) {
      db.exec("ROLLBACK");
      return { published: false, reason: "pending_work" };
    }
    const put = db.prepare("INSERT INTO generation(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value");
    put.run("sourceObservation", JSON.stringify(after));
    put.run("manifest", JSON.stringify(manifest));
    db.exec("COMMIT");
  } catch (error) { db.exec("ROLLBACK"); throw error; }
  return { published: true };
}

export async function reconcile(dbOrRoot, rootOrOptions = null, options = {}) {
  const root = canonicalRoot(typeof dbOrRoot === "string" ? dbOrRoot : rootOrOptions);
  const ownedDbPath = typeof dbOrRoot === "string" ? join(root, options.outDir ?? ".agent", "graph", "graph.db") : null;
  if (ownedDbPath) assertSafeMutableStorePath(ownedDbPath);
  const lease = ownedDbPath ? acquireStoreLease(ownedDbPath, { ownerKind: "one_shot" }) : null;
  let db;
  try {
    db = ownedDbPath ? openStore(ownedDbPath, { mutablePathPolicy: "refuse" }) : dbOrRoot;
  } catch (error) {
    lease?.release();
    throw error;
  }
  const outDir = options.outDir ?? ".agent";
  const close = typeof dbOrRoot === "string";
  try {
    const { signal } = options;
    throwIfAborted(signal);
    const adapter = options.adapter ?? { eventsSince, writeSnapshot };
    // Reuses the existing repo-relative prefix-exclusion mechanism that
    // build-time scanning already honors (graph/ignored-prefixes.mjs), so an
    // enrolled sibling repo is invisible to the JS-level scan/ledger-diff
    // fallback the same way it is to the native watch subscription — without
    // it, a full no-snapshot reconcile of a parent repo would re-walk and
    // re-adopt every file under a nested enrolled child as its own.
    //
    // The repo's own configured prefixes (.agent/config.json ignoredPrefixes)
    // must ALWAYS apply, not only when a caller passes them. The freshness
    // barrier invokes reconcile with no ignore list, so its scans diffed the
    // full workspace against a generation built WITH the exclusions: every
    // excluded file registered as "added", tens of thousands of phantom events
    // filled the journal (34,798 observed on 2026-08-09, refilling within
    // minutes of being cleared), and each barrier call then replayed them —
    // permanent churn that stalled every graph query behind minutes of drain.
    const configured = readConfiguredIgnoredPrefixes(root, outDir);
    // Native snapshots & their event filter must use the same exclusions as
    // the authoritative scan. Otherwise a bounded reader reintroduces files
    // that the scan deliberately omitted, then removes them on every query.
    const ignore = [...new Set([...(options.ignore ?? []), ...configured])];
    const ignoredPrefixes = normalizeIgnoredPrefixes([
      ...(options.ignore ?? []).map((rel) => `${rel.replace(/\/$/, "")}/`),
      ...configured,
    ]);
    const snapshot = options.snapshotPath ?? snapshotPath(root, outDir);
    const hadSnapshot = existsSync(snapshot);
    const repairingGap = db.prepare("SELECT value FROM watch_state WHERE key='event_gap'").get()?.value === "1";
    const pending = [];
    let diff = { changed: [], added: [], removed: [] };
    if (hadSnapshot) {
      const fastEvents = await adapter.eventsSince(root, snapshot, ignore);
      throwIfAborted(signal);
      pending.push(...coalesceRenameEvents([
        ...fastEvents,
        ...metadataEvents(db, root, options.scanSourceMetadata ?? scanSourceMetadataPublic, { ignoredPrefixes, trackedOnly: false }),
      ]).filter((event) => !isUnchangedDocumentEvent(db, root, event)));
      diff = {
        changed: [...new Set(pending.filter((event) => event.eventKind === "modify").map((event) => event.path))].sort(),
        added: [...new Set(pending.filter((event) => event.eventKind === "create").map((event) => event.path))].sort(),
        removed: [...new Set(pending.filter((event) => ["delete", "rename"].includes(event.eventKind)).map((event) => event.path))].sort(),
      };
    } else {
      const source = scanSourcesForPublication(root, outDir, { ignoredPrefixes });
      const ledgerRows = db.prepare("SELECT COUNT(*) AS n FROM generation_leaf WHERE kind='file'").get().n;
      diff = ledgerRows > 0
        ? diffLedgerAgainstTree(db, null, source.files ?? [])
        : { changed: [], added: [], removed: [] };
    }
    if (!hadSnapshot) {
      for (const path of diff.changed) pending.push({ eventKind: "modify", path, observedMs: Date.now() });
      for (const path of diff.added) pending.push({ eventKind: "create", path, observedMs: Date.now() });
      for (const path of diff.removed) pending.push({ eventKind: "delete", path, observedMs: Date.now() });
    }
    const unique = new Map();
    for (const event of pending) unique.set(`${event.path}:${event.renameTo ?? ""}`, event);
    const renameEvents = [...unique.values()].filter((event) => event.eventKind === "rename" && event.renameTo);
    // Captured before the journal drains below: the last-committed generation
    // still reflects entities at `oldPath`. See finishEntityRenames/renameFact.
    const beforeGeneration = renameEvents.length ? loadGeneration(db) : null;
    if (unique.size) appendWatchEvents(db, [...unique.values()]);
    const applied = await drainJournal(db, root, { force: true, maxDependentFiles: options.maxDependentFiles, signal });
    const renameReconciliation = renameEvents.length
      ? finishEntityRenames(renameEvents, beforeGeneration, loadGeneration(db))
      : [];
    if (renameReconciliation.length) {
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('rename_reconciliation',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
        .run(JSON.stringify(renameReconciliation));
    }
    if ((hadSnapshot && unique.size > 0) || (!hadSnapshot && repairingGap)) {
      // A real (non-glob) ignore list makes @parcel/watcher's writeSnapshot
      // walk skip every ignored directory instead of the whole tree, so it
      // now returns fast enough to expose a real race: a file applied only
      // moments ago (same tick, via drainJournal just above) can still be
      // snapshotted with its PRE-edit state, and the next reconcile then
      // re-reports it as changed even though nothing touched it since.
      // Empirically confirmed 2026-08-03 — this settle is the fix, not a
      // cosmetic wait; removing it reintroduces a genuinely flaky (not just
      // occasional) false "changed" report on the very next reconcile.
      await sleep(50, signal);
      await adapter.writeSnapshot(root, snapshot, ignore);
    }
    throwIfAborted(signal);
    let authorityScan = scanSourcesForPublication(root, outDir, { ignoredPrefixes });
    let convergence = evaluateConvergenceOracle(db, authorityScan.files ?? [], { ...authorityScan, eventGapOverride: false });
    // Native snapshots and metadata can miss a content mismatch, including
    // legacy normalized README identities. Repair the authoritative diff
    // once through normal journal processing, then rescan to detect races.
    const repairs = [
      ...convergence.mismatches.changed.map((path) => ({ eventKind: "modify", path })),
      ...convergence.mismatches.added.map((path) => ({ eventKind: "create", path })),
      ...convergence.mismatches.removed.map((path) => ({ eventKind: "delete", path })),
    ];
    if (repairs.length > 0) {
      appendWatchEvents(
        db,
        repairs.map((event) => ({
          ...event,
          observedMs: Date.now(),
        })),
      );
      await drainJournal(db, root, {
        force: true,
        maxDependentFiles: options.maxDependentFiles,
        signal,
      });
      throwIfAborted(signal);
      convergence = evaluateConvergenceOracle(db, authorityScan.files ?? [], { ...authorityScan, eventGapOverride: false });
    }
    if (options.completeDocuments) {
      completePendingDocDomain(db, root, { outDir });
      authorityScan = scanSourcesForPublication(root, outDir, { ignoredPrefixes });
      convergence = evaluateConvergenceOracle(db, authorityScan.files ?? [], { ...authorityScan, eventGapOverride: false });
    }
    db.exec("BEGIN;");
    try {
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('event_gap',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(convergence.converged ? "0" : "1");
      if (convergence.converged) db.prepare("DELETE FROM watch_state WHERE key='event_gap_reason'").run();
      else db.prepare("INSERT INTO watch_state(key,value) VALUES ('event_gap_reason','convergence_mismatch') ON CONFLICT(key) DO UPDATE SET value=excluded.value").run();
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('convergence_oracle',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(JSON.stringify(convergence));
      db.prepare("INSERT INTO watch_state(key,value) VALUES ('last_reconcile_ms',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").run(String(Date.now()));
      db.exec("COMMIT;");
    } catch (error) {
      db.exec("ROLLBACK;");
      throw error;
    }
    return { ok: convergence.converged, changed: diff.changed, added: diff.added, removed: diff.removed, queued: unique.size, applied, eventGap: convergence.converged ? 0 : 1, convergence, renameReconciliation };
  } finally {
    if (close) closeStore(db);
    lease?.release();
  }
}
