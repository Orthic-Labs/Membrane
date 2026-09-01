import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { buildGraphGeneration } from "./static-provider.mjs";
import { loadGeneration } from "./store-sqlite.mjs";

function fail(code) { const error = new Error(code); error.code = code; return error; }
function rootOf(root) { try { return realpathSync(resolve(root)); } catch { return resolve(root); } }

function stableStringify(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`).join(",")}}`;
}

function digest(value) {
  return `sha256:${createHash("sha256").update(stableStringify(value)).digest("hex")}`;
}

function semanticSurface(value, omitted = new Set()) {
  return Object.fromEntries(Object.entries(value ?? {})
    .filter(([key, item]) => !omitted.has(key) && item !== undefined)
    .map(([key, item]) => [key, item]));
}

function evidenceCitations(evidence) {
  const entries = Array.isArray(evidence) ? evidence : evidence ? [evidence] : [];
  return entries.map((item) => {
    if (typeof item === "string") return item;
    return semanticSurface(item, new Set(["text", "content", "snippet", "source"]));
  });
}

function semanticGraphFromGeneration(generation) {
  const nodes = generation?.nodes ?? [];
  const idToKey = new Map();
  const keyedNodes = nodes.map((node) => {
    const key = String(node.id ?? (node.kind === "file"
      ? `file:${node.path}`
      : `${node.kind ?? "node"}:${node.path ?? ""}:${node.qualifiedName ?? node.name ?? "anonymous"}`));
    idToKey.set(node.id, key);
    const surface = semanticSurface(node, new Set(["id", "generationId", "provider"]));
    return {
      key,
      fingerprint: digest(surface),
      evidence: evidenceCitations(node.evidence),
      summary: semanticSurface(node, new Set(["id", "generationId", "provider", "evidence", "content", "text", "snippet"])),
    };
  }).sort((left, right) => left.key.localeCompare(right.key));
  const keyedEdges = (generation?.edges ?? []).map((edge) => {
    const source = idToKey.get(edge.source) ?? `external:${edge.source}`;
    const target = idToKey.get(edge.target) ?? (edge.target ? `external:${edge.target}` : null);
    const key = String(edge.id ?? `${edge.kind}:${source}->${target ?? edge.specifier ?? "unresolved"}`);
    const surface = {
      ...semanticSurface(edge, new Set(["id", "generationId", "provider", "source", "target"])),
      source,
      target,
    };
    return {
      key,
      fingerprint: digest(surface),
      evidence: evidenceCitations(edge.evidence),
      summary: semanticSurface(surface, new Set(["evidence", "content", "text", "snippet"])),
    };
  }).sort((left, right) => left.key.localeCompare(right.key));
  return { schemaVersion: 1, nodes: keyedNodes, edges: keyedEdges };
}

function semanticGraphFromStore(db) {
  return semanticGraphFromGeneration(loadGeneration(db));
}

function semanticDelta(before, after, limit) {
  const compareKind = (beforeItems = [], afterItems = []) => {
    const previous = new Map(beforeItems.map((item) => [item.key, item]));
    const current = new Map(afterItems.map((item) => [item.key, item]));
    const added = [];
    const removed = [];
    const changed = [];
    for (const key of [...new Set([...previous.keys(), ...current.keys()])].sort()) {
      const prior = previous.get(key);
      const next = current.get(key);
      if (!prior) added.push({ key, after: next });
      else if (!next) removed.push({ key, before: prior });
      else if (prior.fingerprint !== next.fingerprint) changed.push({ key, before: prior, after: next });
    }
    const total = added.length + removed.length + changed.length;
    return {
      added: added.slice(0, limit),
      removed: removed.slice(0, limit),
      changed: changed.slice(0, limit),
      receipt: { total, limit, truncated: total > limit },
    };
  };
  return { nodes: compareKind(before?.nodes, after?.nodes), edges: compareKind(before?.edges, after?.edges) };
}

export function currentGitIdentity(root) {
  const repoRoot = rootOf(root);
  try {
    const head = execFileSync("git", ["rev-parse", "HEAD"], { cwd: repoRoot, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
    const status = execFileSync("git", ["status", "--porcelain=v1", "-z", "--untracked-files=all", "--", ".", ":(exclude).agent", ":(exclude).agent/**"], { cwd: repoRoot, encoding: "buffer", stdio: ["ignore", "pipe", "ignore"] });
    return { head, dirty: status.length > 0 };
  } catch { return null; }
}

function identityFromStore(db, repoRoot) {
  let rows;
  try { rows = db.prepare("SELECT key,value FROM generation").all(); } catch { throw fail("snapshot_missing"); }
  const envelope = Object.fromEntries(rows.map((row) => { try { return [row.key, JSON.parse(row.value)]; } catch { throw fail("snapshot_malformed"); } }));
  const manifest = envelope.manifest;
  if (!manifest || manifest.complete !== true || typeof manifest.generationId !== "string" || !manifest.generationId || typeof manifest.manifestDigest !== "string" || !manifest.manifestDigest) throw fail("snapshot_incomplete");
  if (!envelope.sourceObservation || typeof envelope.sourceObservation.head !== "string" || !envelope.sourceObservation.head || typeof envelope.sourceObservation.dirty !== "boolean") throw fail("snapshot_malformed");
  if (envelope.sourceObservation.dirty === true) throw fail("snapshot_dirty");
  const leaves = db.prepare("SELECT path,digest FROM generation_leaf WHERE kind='file' ORDER BY path").all();
  if (leaves.some((leaf) => typeof leaf.path !== "string" || typeof leaf.digest !== "string" || !leaf.digest)) throw fail("snapshot_malformed");
  return {
    repoRoot: rootOf(repoRoot),
    generationId: String(manifest.generationId),
    manifestDigest: String(manifest.manifestDigest),
    sourceObservation: envelope.sourceObservation,
    leaves,
    semanticGraph: semanticGraphFromStore(db),
  };
}

export function createSnapshot(db, name, repoRoot, { current = currentGitIdentity(repoRoot) } = {}) {
  const snapshotName = String(name ?? "").trim();
  if (!snapshotName || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(snapshotName)) throw fail("snapshot_invalid_name");
  const identity = identityFromStore(db, repoRoot);
  if (!current || current.dirty) throw fail("snapshot_current_dirty");
  if (identity.repoRoot !== rootOf(repoRoot) || identity.sourceObservation.head !== current.head || identity.sourceObservation.dirty !== false) throw fail("snapshot_stale");
  const existing = db.prepare("SELECT identity_json FROM named_snapshot WHERE name=?").get(snapshotName);
  const json = JSON.stringify(identity);
  if (existing) {
    try {
      if (JSON.stringify(JSON.parse(existing.identity_json)) === json) return { ...identity, name: snapshotName, idempotent: true };
    } catch { throw fail("snapshot_malformed"); }
    throw fail("snapshot_conflict");
  }
  db.prepare("INSERT INTO named_snapshot(name,repo_root,generation_id,manifest_digest,identity_json,created_ms) VALUES (?,?,?,?,?,?)").run(snapshotName, identity.repoRoot, identity.generationId, identity.manifestDigest, json, Date.now());
  return { ...identity, name: snapshotName, idempotent: false };
}

export function getSnapshot(db, name) {
  const snapshotName = String(name ?? "").trim();
  if (!snapshotName || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(snapshotName)) throw fail("snapshot_invalid_name");
  const row = db.prepare("SELECT identity_json FROM named_snapshot WHERE name=?").get(snapshotName);
  if (!row) throw fail("snapshot_missing");
  try { return { ...JSON.parse(row.identity_json), name: snapshotName }; } catch { throw fail("snapshot_malformed"); }
}

export function listSnapshots(db) {
  return db.prepare("SELECT name,repo_root AS repoRoot,generation_id AS generationId,manifest_digest AS manifestDigest,created_ms AS createdMs FROM named_snapshot ORDER BY name").all();
}

export function changesSince(db, name, { limit = 100 } = {}) {
  if (limit !== undefined && limit !== null && (typeof limit === "string" && limit.trim() === "" || !Number.isInteger(Number(limit)) || Number(limit) < 1)) throw fail("snapshot_invalid_limit");
  const snapshot = getSnapshot(db, name);
  const current = identityFromStore(db, snapshot.repoRoot);
  const before = new Map(snapshot.leaves.map((leaf) => [leaf.path, leaf.digest]));
  const after = new Map(current.leaves.map((leaf) => [leaf.path, leaf.digest]));
  const changes = [];
  for (const path of new Set([...before.keys(), ...after.keys()])) {
    if (!before.has(path)) changes.push({ path, kind: "added" });
    else if (!after.has(path)) changes.push({ path, kind: "deleted" });
    else if (before.get(path) !== after.get(path)) changes.push({ path, kind: "modified" });
  }
  changes.sort((a, b) => a.path.localeCompare(b.path) || a.kind.localeCompare(b.kind));
  const cap = Math.min(10000, Number(limit));
  return {
    name: snapshot.name,
    base: { generationId: snapshot.generationId, manifestDigest: snapshot.manifestDigest, sourceObservation: snapshot.sourceObservation },
    head: { generationId: current.generationId, manifestDigest: current.manifestDigest, sourceObservation: current.sourceObservation },
    changes: changes.slice(0, cap),
    receipt: { total: changes.length, limit: cap, truncated: changes.length > cap },
  };
}

function gitTreeishChanges(repoRoot, base, head = "HEAD") {
  const from = String(base ?? "").trim();
  const to = String(head ?? "HEAD").trim() || "HEAD";
  if (!from) throw fail("treeish_base_required");
  let output;
  try {
    output = execFileSync("git", ["diff", "--name-status", "-z", "--find-renames", from, to, "--"], {
      cwd: rootOf(repoRoot), encoding: "buffer", stdio: ["ignore", "pipe", "ignore"], timeout: 10_000, maxBuffer: 4 * 1024 * 1024,
    });
  } catch { throw fail("treeish_unavailable"); }
  const fields = output.toString("utf8").split("\0").filter(Boolean);
  const changes = [];
  for (let index = 0; index < fields.length;) {
    const status = fields[index++];
    const before = fields[index++];
    if (!before) break;
    if (status.startsWith("R") || status.startsWith("C")) {
      const after = fields[index++];
      changes.push({ path: String(after).replaceAll("\\", "/"), previousPath: String(before).replaceAll("\\", "/"), kind: status.startsWith("R") ? "renamed" : "copied" });
    } else {
      const kind = ({ A: "added", D: "deleted", M: "modified", T: "type_changed", U: "unmerged" })[status[0]] ?? "modified";
      changes.push({ path: String(before).replaceAll("\\", "/"), kind });
    }
  }
  return changes.sort((left, right) => left.path.localeCompare(right.path) || left.kind.localeCompare(right.kind));
}

function safeTreePath(path) {
  const normalized = String(path ?? "").replaceAll("\\", "/");
  return normalized && !normalized.startsWith("/") && !normalized.split("/").includes("..") ? normalized : null;
}

function treeishSemanticGraph(repoRoot, reference, paths, { maxFiles = 2_000, maxBytes = 64 * 1024 * 1024 } = {}) {
  const selected = new Set(paths.map(safeTreePath).filter(Boolean));
  if (selected.size > maxFiles) throw fail("treeish_semantic_limit");
  if (selected.size === 0) return { schemaVersion: 1, nodes: [], edges: [] };
  let listing;
  try {
    listing = execFileSync("git", ["ls-tree", "-r", "-z", String(reference)], {
      cwd: rootOf(repoRoot), encoding: "buffer", stdio: ["ignore", "pipe", "ignore"], timeout: 10_000, maxBuffer: 32 * 1024 * 1024,
    });
  } catch { throw fail("treeish_unavailable"); }
  const blobs = [];
  for (const record of listing.toString("utf8").split("\0").filter(Boolean)) {
    const tab = record.indexOf("\t");
    if (tab < 0) continue;
    const path = safeTreePath(record.slice(tab + 1));
    if (!path || !selected.has(path)) continue;
    const [mode, type, oid] = record.slice(0, tab).split(" ");
    if (type === "blob" && mode !== "160000" && oid) blobs.push({ path, oid });
  }
  const temporary = mkdtempSync(join(tmpdir(), "blueprint-treeish-"));
  let bytes = 0;
  try {
    for (const blob of blobs.sort((left, right) => left.path.localeCompare(right.path))) {
      let content;
      try {
        content = execFileSync("git", ["cat-file", "blob", blob.oid], {
          cwd: rootOf(repoRoot), encoding: "buffer", stdio: ["ignore", "pipe", "ignore"], timeout: 10_000, maxBuffer: maxBytes + 1,
        });
      } catch { throw fail("treeish_unavailable"); }
      bytes += content.length;
      if (bytes > maxBytes) throw fail("treeish_semantic_limit");
      const target = join(temporary, blob.path);
      mkdirSync(dirname(target), { recursive: true });
      writeFileSync(target, content);
    }
    return semanticGraphFromGeneration(buildGraphGeneration(temporary));
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}

function treeishSemanticPair(repoRoot, base, head, changes) {
  const beforePaths = [];
  const afterPaths = [];
  for (const change of changes) {
    if (change.kind !== "added") beforePaths.push(change.previousPath ?? change.path);
    if (change.kind !== "deleted") afterPaths.push(change.path);
  }
  return {
    before: treeishSemanticGraph(repoRoot, base, beforePaths),
    after: treeishSemanticGraph(repoRoot, head, afterPaths),
  };
}

function citeCurrentNodes(db, generationId, changes) {
  const file = db.prepare("SELECT node_id AS id FROM files WHERE generation_id=? AND path=? ORDER BY node_id");
  const symbols = db.prepare("SELECT id FROM symbols WHERE generation_id=? AND path=? ORDER BY id LIMIT 50");
  return changes.map((change) => ({
    ...change,
    currentEvidence: {
      fileNodeId: file.get(generationId, change.path)?.id ?? null,
      symbolNodeIds: symbols.all(generationId, change.path).map((row) => row.id),
    },
  }));
}

/** Historical comparison is a disposable projection. Current graph truth is
 * never mutated or replaced by snapshot/treeish state. */
export function changesSinceReference(db, repoRoot, { snapshot, generation, treeish, head = "HEAD", limit = 100 } = {}) {
  const current = identityFromStore(db, repoRoot);
  let source;
  let raw;
  let beforeSemantic = null;
  let afterSemantic = current.semanticGraph;
  const omissions = [];
  if (snapshot) {
    source = { kind: "snapshot", value: String(snapshot) };
    const reference = getSnapshot(db, snapshot);
    raw = changesSince(db, snapshot, { limit: 10_000 }).changes;
    beforeSemantic = reference.semanticGraph ?? null;
    if (!beforeSemantic) omissions.push({ reason: "snapshot_semantic_evidence_unavailable", snapshot: String(snapshot) });
  } else if (generation) {
    source = { kind: "generation", value: String(generation) };
    if (String(generation) === current.generationId) {
      raw = [];
      beforeSemantic = current.semanticGraph;
    }
    else {
      const row = db.prepare("SELECT name FROM named_snapshot WHERE generation_id=? ORDER BY name LIMIT 1").get(String(generation));
      if (row) {
        const reference = getSnapshot(db, row.name);
        raw = changesSince(db, row.name, { limit: 10_000 }).changes;
        beforeSemantic = reference.semanticGraph ?? null;
        if (!beforeSemantic) omissions.push({ reason: "generation_semantic_evidence_unavailable", generationId: String(generation) });
      }
      else {
        raw = [];
        omissions.push({ reason: "generation_history_unavailable", generationId: String(generation) });
      }
    }
  } else if (treeish) {
    const base = typeof treeish === "string" ? treeish : treeish.base ?? treeish.from;
    const target = typeof treeish === "string" ? head : treeish.head ?? treeish.to ?? head;
    source = { kind: "treeish", value: String(base), head: String(target) };
    raw = gitTreeishChanges(repoRoot, base, target);
    const pair = treeishSemanticPair(repoRoot, base, target, raw);
    beforeSemantic = pair.before;
    afterSemantic = pair.after;
  } else throw fail("change_reference_required");
  const cap = Math.min(10_000, Math.max(1, Number(limit) || 100));
  const cited = citeCurrentNodes(db, current.generationId, raw);
  return {
    schemaVersion: 2,
    kind: "SemanticChangeProjection",
    authority: "history_reference_only",
    source,
    currentTruth: { generationId: current.generationId, manifestDigest: current.manifestDigest },
    changes: cited.slice(0, cap),
    semanticDelta: beforeSemantic ? semanticDelta(beforeSemantic, afterSemantic, cap) : null,
    receipt: { total: cited.length, limit: cap, truncated: cited.length > cap },
    omissions,
  };
}
