import { hydrateNodesByIds } from "./store-sqlite.mjs";

const STOPWORDS = new Set(["a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "is", "it", "of", "on", "or", "that", "the", "this", "to", "with"]);

function terms(task) {
  return [...new Set(String(task ?? "").replace(/([a-z0-9])([A-Z])/g, "$1 $2").toLowerCase()
    .split(/[^a-z0-9_./:-]+/).filter((term) => term.length > 1 && !STOPWORDS.has(term)))]
    .sort((left, right) => right.length - left.length || left.localeCompare(right));
}

function rowsToSeeds(db, rows, exactness, reason, maxSeeds) {
  const ids = [...new Set(rows.map((row) => String(row.id)))].slice(0, maxSeeds);
  const nodes = new Map(hydrateNodesByIds(db, ids).map((node) => [node.id, node]));
  return ids.map((id) => ({ id, node: nodes.get(id), exactness, reason })).filter((seed) => seed.node);
}

export function resolveSeeds(db, task, { generationId, seedIds = [], anchors = [], maxSeeds = 8 } = {}) {
  if (!generationId) throw Object.assign(new Error("seed resolution requires generation"), { code: "generation_required" });
  const explicit = rowsToSeeds(db, seedIds.map((id) => ({ id })), 0, "node_id", maxSeeds);
  if (explicit.length) return { state: "resolved", seeds: explicit, ambiguous: false };

  const addressRows = [];
  const exactAddresses = [...new Set([...anchors, ...terms(task).filter((term) => term.includes("/") || term.includes("."))])];
  const byPath = db.prepare("SELECT node_id AS id FROM files WHERE generation_id = ? AND path = ? ORDER BY node_id LIMIT ?");
  for (const address of exactAddresses) addressRows.push(...byPath.all(generationId, String(address).replaceAll("\\", "/"), maxSeeds));
  const addressed = rowsToSeeds(db, addressRows, 1, "source_address", maxSeeds);
  if (addressed.length) return { state: "resolved", seeds: addressed, ambiguous: addressed.length > 1 };

  const symbolRows = [];
  const bySymbol = db.prepare("SELECT id FROM symbols WHERE generation_id = ? AND (qualified_name = ? OR name = ?) ORDER BY id LIMIT ?");
  for (const term of terms(task)) symbolRows.push(...bySymbol.all(generationId, term, term, maxSeeds));
  const symbols = rowsToSeeds(db, symbolRows, 2, "exact_symbol", maxSeeds);
  if (symbols.length) return { state: "resolved", seeds: symbols, ambiguous: symbols.length > 1 };

  const indexedRows = [];
  const byTerm = db.prepare("SELECT symbol_id AS id FROM symbol_terms WHERE generation_id = ? AND token = ? ORDER BY symbol_id LIMIT ?");
  for (const term of terms(task)) {
    indexedRows.push(...byTerm.all(generationId, term, maxSeeds));
    if (indexedRows.length >= maxSeeds) break;
  }
  const indexed = rowsToSeeds(db, indexedRows, 3, "indexed_term", maxSeeds);
  if (indexed.length) return { state: "resolved", seeds: indexed, ambiguous: indexed.length > 1 };
  return { state: "unresolved", seeds: [], ambiguous: false, reason: "no_relevant_seed" };
}
