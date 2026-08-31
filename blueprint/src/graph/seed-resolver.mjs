import { hydrateNodesByIds } from "./store-sqlite.mjs";

const STOPWORDS = new Set(["a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "is", "it", "of", "on", "or", "that", "the", "this", "to", "with"]);

function terms(task) {
  return [...new Set(String(task ?? "").replace(/([a-z0-9])([A-Z])/g, "$1 $2").toLowerCase()
    .split(/[^a-z0-9_./:-]+/).filter((term) => term.length > 1 && !STOPWORDS.has(term)))];
}

function nodeEvidence(node) {
  return Array.isArray(node?.evidence) ? node.evidence.filter(Boolean).map((item) => ({
    path: item.path ?? node.path ?? null,
    startLine: item.startLine ?? null,
    endLine: item.endLine ?? null,
    contentHash: item.contentHash ?? null,
  })) : [];
}

function rowsToCandidates(db, rows, exactness, reason, maxSeeds) {
  const ids = [...new Set(rows.map((row) => String(row.id)))].slice(0, maxSeeds + 1);
  const nodes = new Map(hydrateNodesByIds(db, ids).map((node) => [node.id, node]));
  return ids.map((id) => {
    const node = nodes.get(id);
    return node ? { id, exactness, reason, node, evidence: nodeEvidence(node) } : null;
  }).filter(Boolean);
}

function envelope(state, candidates, attempts, reason = null) {
  return {
    schemaVersion: 1,
    kind: "SeedResolution",
    state,
    seeds: state === "resolved" ? candidates : [],
    candidates: candidates.map(({ node, ...candidate }) => candidate),
    candidateCount: candidates.length,
    ambiguous: state === "ambiguous",
    reason,
    attempts,
  };
}

function decide(candidates, attempts, { allowMany = false } = {}) {
  if (!candidates.length) return null;
  if (!allowMany && candidates.length > 1) return envelope("ambiguous", candidates, attempts, "multiple_candidates");
  return envelope("resolved", candidates, attempts);
}

export function resolveSeeds(db, task, { generationId, seedIds = [], anchors = [], maxSeeds = 8, allowAmbiguousTaskSeeds = false } = {}) {
  if (!generationId) throw Object.assign(new Error("seed resolution requires generation"), { code: "generation_required" });
  const attempts = [];
  const explicitRows = seedIds.map((id) => ({ id: String(id) }));
  const explicit = rowsToCandidates(db, explicitRows, 0, "node_id", maxSeeds);
  attempts.push({ lane: "node_id", requested: explicitRows.length, matched: explicit.length });
  const explicitDecision = decide(explicit, attempts, { allowMany: true });
  if (explicitDecision) return explicitDecision;

  const normalizedAnchors = [...new Set(anchors.map((anchor) => String(anchor).trim()).filter(Boolean))];
  const exactAddresses = [...new Set([...normalizedAnchors, ...terms(task).filter((term) => term.includes("/") || term.includes("."))])];
  const byPath = db.prepare("SELECT node_id AS id FROM files WHERE generation_id = ? AND path = ? ORDER BY node_id LIMIT ?");
  const addressRows = [];
  for (const raw of exactAddresses) {
    const address = raw.startsWith("file:") ? raw.slice(5) : raw;
    addressRows.push(...byPath.all(generationId, address.replaceAll("\\", "/"), maxSeeds + 1));
  }
  const addressed = rowsToCandidates(db, addressRows, 1, "source_address", maxSeeds);
  attempts.push({ lane: "source_address", requested: exactAddresses.length, matched: addressed.length });
  const addressDecision = decide(addressed, attempts);
  if (addressDecision) return addressDecision;

  const queryTerms = terms(task);
  const qualifiedRows = [];
  const byQualified = db.prepare("SELECT id FROM symbols WHERE generation_id = ? AND qualified_name = ? ORDER BY confidence DESC,id LIMIT ?");
  for (const term of queryTerms) qualifiedRows.push(...byQualified.all(generationId, term, maxSeeds + 1));
  const qualified = rowsToCandidates(db, qualifiedRows, 2, "qualified_symbol", maxSeeds);
  attempts.push({ lane: "qualified_symbol", requested: queryTerms.length, matched: qualified.length });
  const qualifiedDecision = decide(qualified, attempts, { allowMany: allowAmbiguousTaskSeeds && normalizedAnchors.length === 0 });
  if (qualifiedDecision) return qualifiedDecision;

  const exactRows = [];
  const byName = db.prepare("SELECT id FROM symbols WHERE generation_id = ? AND name = ? ORDER BY confidence DESC,path,id LIMIT ?");
  for (const term of queryTerms) exactRows.push(...byName.all(generationId, term, maxSeeds + 1));
  const exact = rowsToCandidates(db, exactRows, 3, "exact_term", maxSeeds);
  attempts.push({ lane: "exact_term", requested: queryTerms.length, matched: exact.length });
  const exactDecision = decide(exact, attempts, { allowMany: allowAmbiguousTaskSeeds && normalizedAnchors.length === 0 });
  if (exactDecision) return exactDecision;

  const indexedRows = [];
  const byTerm = db.prepare("SELECT symbol_id AS id FROM symbol_terms WHERE generation_id = ? AND token = ? ORDER BY symbol_id LIMIT ?");
  for (const term of queryTerms) {
    indexedRows.push(...byTerm.all(generationId, term, maxSeeds + 1));
    if (indexedRows.length > maxSeeds) break;
  }
  const indexed = rowsToCandidates(db, indexedRows, 4, "bounded_lexical", maxSeeds);
  attempts.push({ lane: "bounded_lexical", requested: queryTerms.length, matched: indexed.length });
  const indexedDecision = decide(indexed, attempts, { allowMany: allowAmbiguousTaskSeeds && normalizedAnchors.length === 0 });
  if (indexedDecision) return indexedDecision;
  return envelope("unresolved", [], attempts, "no_relevant_seed");
}
