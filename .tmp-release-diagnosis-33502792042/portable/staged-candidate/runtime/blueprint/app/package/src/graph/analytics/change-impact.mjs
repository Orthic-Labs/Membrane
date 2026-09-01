import { execFileSync } from "node:child_process";
import { resolveSeeds } from "../seed-resolver.mjs";

function normalizePath(value) {
  return String(value ?? "").trim().replaceAll("\\", "/").replace(/^a\//, "").replace(/^b\//, "");
}

function gitChangedPaths(root, base, head) {
  const from = String(base ?? "").trim();
  const to = String(head ?? "HEAD").trim() || "HEAD";
  if (!from) return { paths: [], omission: { reason: "treeish_base_required" } };
  try {
    const output = execFileSync("git", ["diff", "--name-only", "--diff-filter=ACDMRTUXB", from, to, "--"], {
      cwd: root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: 10_000,
      maxBuffer: 4 * 1024 * 1024,
    });
    return { paths: output.split(/\r?\n/).map(normalizePath).filter(Boolean), omission: null };
  } catch {
    return { paths: [], omission: { reason: "treeish_unavailable", base: from, head: to } };
  }
}

function diffPaths(diff) {
  const paths = [];
  for (const line of String(diff ?? "").split(/\r?\n/)) {
    const match = /^(?:\+\+\+|---)\s+([^\t ]+)/.exec(line);
    if (match && match[1] !== "/dev/null") paths.push(normalizePath(match[1]));
  }
  return paths;
}

function stackLocations(stack) {
  const locations = [];
  const pattern = /(?:^|\s|\()([A-Za-z0-9_./\\@ -]+\.[A-Za-z0-9]+):(\d+)(?::\d+)?\)?/gm;
  let match;
  while ((match = pattern.exec(String(stack ?? ""))) !== null) {
    locations.push({ path: normalizePath(match[1]), line: Number(match[2]) });
  }
  return locations;
}

function symbolAtLine(db, generationId, path, line) {
  if (!Number.isSafeInteger(line) || line < 1) return null;
  const rows = db.prepare("SELECT id,evidence FROM symbols WHERE generation_id=? AND path=? ORDER BY id").all(generationId, path);
  const matches = [];
  for (const row of rows) {
    try {
      const evidence = JSON.parse(row.evidence ?? "[]")[0] ?? {};
      const start = Number(evidence.startLine ?? 0);
      const end = Number(evidence.endLine ?? start);
      if (start > 0 && line >= start && line <= end) matches.push({ id: row.id, span: end - start });
    } catch { /* malformed evidence cannot become a seed */ }
  }
  return matches.sort((left, right) => left.span - right.span || left.id.localeCompare(right))[0]?.id ?? null;
}

/** Resolve all supported impact seed families without silently promoting an
 * ambiguous lexical candidate. The returned envelope is advisory input to
 * traversal; each repository still owns an independent node space. */
export function resolveImpactSeedEnvelope(db, root, generationId, input = {}) {
  const omissions = [];
  const locations = stackLocations(input.stack);
  const inputFiles = Array.isArray(input.files)
    ? input.files
    : input.files
      ? [input.files]
      : [];
  const paths = [
    ...([input.file, ...inputFiles].filter(Boolean).map(normalizePath)),
    ...diffPaths(input.diff),
    ...locations.map((item) => item.path),
  ];
  const treeish = typeof input.treeish === "string"
    ? { base: input.treeish, head: input.head ?? "HEAD" }
    : input.treeish;
  if (treeish) {
    const changed = gitChangedPaths(root, treeish.base ?? treeish.from, treeish.head ?? treeish.to ?? "HEAD");
    paths.push(...changed.paths);
    if (changed.omission) omissions.push(changed.omission);
  }
  const explicitLine = Number(input.line ?? 0);
  if (input.file && explicitLine > 0) locations.push({ path: normalizePath(input.file), line: explicitLine });
  const seedIds = [...new Set(locations.map((item) => symbolAtLine(db, generationId, item.path, item.line)).filter(Boolean))];
  const anchors = [...new Set(paths.filter(Boolean))].sort();
  const task = String(input.test ?? input.anchor ?? input.query ?? "").trim();
  const resolution = resolveSeeds(db, task, {
    generationId,
    seedIds: [...seedIds, ...(input.nodeId ? [input.nodeId] : []), ...(input.anchor ? [input.anchor] : [])],
    anchors: [...anchors, ...(input.anchor ? [input.anchor] : [])],
    maxSeeds: Number(input.maxSeeds ?? 32),
  });
  if (resolution.state === "ambiguous") omissions.push({ reason: "ambiguous_seed", candidates: resolution.candidates });
  if (resolution.state === "unresolved") omissions.push({ reason: resolution.reason });
  return Object.freeze({
    schemaVersion: 1,
    kind: "ImpactSeedEnvelope",
    generationId,
    families: Object.freeze({
      node: Boolean(input.nodeId), file: anchors.length > 0, line: locations.length > 0,
      diff: Boolean(input.diff), stack: Boolean(input.stack), test: Boolean(input.test), treeish: Boolean(treeish),
    }),
    changedPaths: Object.freeze(anchors),
    seeds: Object.freeze(resolution.seeds.map((seed) => ({ id: seed.id, reason: seed.reason, exactness: seed.exactness, evidence: seed.evidence }))),
    resolution,
    omissions: Object.freeze(omissions),
  });
}
