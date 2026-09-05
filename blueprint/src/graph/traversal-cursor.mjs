// Continuations are views of one repository generation and one exact query.
// They never authorize access; the application still enforces root/freshness.
const VERSION = 1;
const MAX_CURSOR_BYTES = 8192;

function invalid() {
  return Object.assign(new Error("Traversal cursor is malformed or does not match the served generation and query."), { code: "cursor_invalid" });
}

export function traversalBinding(meta, kind, parameters, freshness = {}) {
  const generationId = meta?.manifest?.generationId;
  if (typeof generationId !== "string" || !generationId.length) {
    throw Object.assign(new Error("A sealed generation is required for traversal."), { code: "graph_missing" });
  }
  // Bind to the store, never to an unverified generation supplied by a caller.
  if (freshness.generationId != null && freshness.generationId !== generationId) throw invalid();
  return Object.freeze({ version: VERSION, kind, generationId,
    repository: meta.repoRoot ?? meta.manifest.repo?.repoId ?? null, ...parameters });
}

export function decodeTraversalCursor(cursor, binding, counts) {
  if (cursor === null || cursor === undefined) return { node: 0, edge: 0 };
  try {
    if (typeof cursor !== "string" || !cursor.length || cursor.length > MAX_CURSOR_BYTES || !/^[A-Za-z0-9_-]+$/.test(cursor)) throw invalid();
    const value = JSON.parse(Buffer.from(cursor, "base64url").toString("utf8"));
    if (!value || Array.isArray(value) || typeof value !== "object") throw invalid();
    const keys = [...Object.keys(binding), "node", "edge"];
    if (Object.keys(value).length !== keys.length || !keys.every((key) => Object.hasOwn(value, key))) throw invalid();
    if (!Object.entries(binding).every(([key, expected]) => value[key] === expected)) throw invalid();
    for (const key of ["node", "edge"]) {
      if (!Number.isSafeInteger(value[key]) || value[key] < 0 || value[key] > counts[key]) throw invalid();
    }
    return { node: value.node, edge: value.edge };
  } catch { throw invalid(); }
}

export function encodeTraversalCursor(binding, node, edge) {
  return Buffer.from(JSON.stringify({ ...binding, node, edge })).toString("base64url");
}
