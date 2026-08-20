import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { closeStore, openStore, saveGeneration, searchGenerationSymbols, symbolTermTokens } from "../src/graph/store-sqlite.mjs";

const CORPUS_PATH = join(import.meta.dirname, "..", "evals", "equivalence", "retrieval-corpus-v1.json");
const corpus = JSON.parse(readFileSync(CORPUS_PATH, "utf8"));

/**
 * Strict baseline comparator: callers must preserve returned order, identity,
 * generation binding, and typed omission. It deliberately does not sort.
 */
export function compareRetrievalOutput(actual, expected, label = "retrieval output") {
  assert.deepEqual(actual, expected, label);
}

function symbolNode(symbol) {
  return {
    ...symbol,
    kind: "symbol",
    labels: ["Function"],
    evidence: [{ path: symbol.path, startLine: 1, endLine: 1, contentHash: "retrieval-corpus" }],
  };
}

function seedCorpus(db) {
  const [current, ...residue] = corpus.generations;
  saveGeneration(db, {
    manifest: { generationId: current.id },
    provider: { id: "blueprint-static", version: "retrieval-corpus-v1" },
    nodes: current.symbols.map(symbolNode),
    edges: [],
  });
  const insertSymbol = db.prepare("INSERT INTO symbols(id, kind, labels, name, qualified_name, path, confidence, evidence, generation_id, extra, node_ordinal) VALUES (?, 'symbol', '[]', ?, ?, ?, ?, '[]', ?, NULL, ?)");
  const insertTerm = db.prepare("INSERT INTO symbol_terms(generation_id, token, symbol_id) VALUES (?, ?, ?)");
  let ordinal = current.symbols.length;
  for (const generation of residue) {
    for (const symbol of generation.symbols) {
      insertSymbol.run(symbol.id, symbol.name, symbol.qualifiedName, symbol.path, symbol.confidence, generation.id, ordinal++);
      for (const token of symbolTermTokens([symbol.name, symbol.qualifiedName, symbol.path])) insertTerm.run(generation.id, token, symbol.id);
    }
  }
}

function executeCase(db, entry) {
  const knownGeneration = corpus.generations.some((generation) => generation.id === entry.generationId);
  const rows = searchGenerationSymbols(db, entry.generationId, entry.tokens, entry.limit);
  return {
    ids: rows.map((row) => row.id),
    generationId: entry.generationId,
    omission: rows.length === 0 && !knownGeneration ? "generation_not_found" : null,
  };
}

function score(outputs) {
  let hits = 0, reciprocalRankSum = 0, omissions = 0;
  for (const { entry, output } of outputs) {
    if (output.omission) omissions += 1;
    const rank = output.ids.findIndex((id) => entry.relevantIds.includes(id));
    if (rank !== -1) { hits += 1; reciprocalRankSum += 1 / (rank + 1); }
  }
  return {
    queries: outputs.length,
    hits,
    reciprocalRankSum,
    hitRate: hits / outputs.length,
    mrr: reciprocalRankSum / outputs.length,
    omissions,
  };
}

test("frozen portable retrieval corpus preserves ordered identities, omissions, and generation binding", () => {
  assert.equal(corpus.schema, "blueprint-retrieval-equivalence-corpus-v1");
  const db = openStore(":memory:");
  try {
    seedCorpus(db);
    const outputs = corpus.cases.map((entry) => ({ entry, output: executeCase(db, entry) }));
    for (const { entry, output } of outputs) compareRetrievalOutput(output, entry.expected, entry.id);
    assert.deepEqual(score(outputs), corpus.expectedSummary, "fixed hit-rate and MRR baseline");
  } finally {
    closeStore(db);
  }
});
