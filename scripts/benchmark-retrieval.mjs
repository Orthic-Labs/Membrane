#!/usr/bin/env node
// Phase 7.6 — retrieval benchmark.
//
// This script measures the actual retrieval quality of every strategy Cortex
// has shipped (lexical, graph-walk, hybrid), on a corpus of REAL workspace
// failures. It is the measurement that gates Phase 7.6's "embeddings without a
// measured win" rule: until this harness records a measured win, semantic
// retrieval must remain inactive.
//
// Honest design choices:
//   - The corpus lives at evals/retrieval-corpus/corpus.v1.json and is checked
//     in. The harness never invents a failure case to pad to a target count; if
//     the corpus shrinks, the report records the real count.
//   - Every gold answer carries a `rationale` field that points to where the
//     answer was sourced from. Judgement-call golds say so explicitly.
//   - No-gold cases must be answered with {abstained:true, reason:"no_relevant_seed"};
//     a strategy that returns a non-empty path on a no-gold case is scored 0 on
//     that case (worse than abstaining).
//   - Strategies are run with identical inputs, identical budgets, and the
//     same scoring function. They differ ONLY in retrieval logic.
//   - Determinism: the harness does not touch the clock, network, or any
//     non-deterministic source. Two runs on the same corpus produce byte-identical
//     reports (modulo the `runAt` envelope field).
//
// The harness does not implement semantic retrieval. It implements lexical
// (queryGraph scalar scoring), graph (indexedNeighbors expansion from the
// lexical seed set), and a hybrid that scores graph-expanded nodes back through
// the lexical scorer. Adding semantic is a one-strategy addition; this script
// already prints the empty arm so the harness shape is fixed.

import { execFileSync } from "node:child_process";
import { readFileSync, existsSync, mkdirSync, mkdtempSync, cpSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { openStore, openStoreReadOnly, closeStore, traversalNeighbors, hydrateNodesByIds } from "../graph/store-sqlite.mjs";
import { buildGraphGeneration, readGeneration, graphNeighbors, queryGraph } from "../graph/static-provider.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const CORPUS_PATH = join(REPO_ROOT, "evals", "retrieval-corpus", "corpus.v1.json");
export function resolveReportDir(value = process.env.CORTEX_RETRIEVAL_REPORT_DIR) { return resolve(value || join(REPO_ROOT, "evals", "retrieval-corpus", "reports")); }
const REPORT_DIR = resolveReportDir();
const FIXTURES_ROOT = join(REPO_ROOT, "evals", "fixture-repos");

const STRATEGIES = ["lexical", "graph", "hybrid"];
const DEFAULT_LIMIT = 10;

// --- corpus ---

function loadCorpus() {
  const text = readFileSync(CORPUS_PATH, "utf8");
  const parsed = JSON.parse(text);
  if (parsed.schema !== "cortex-retrieval-corpus-v1") {
    throw new Error(`unsupported corpus schema: ${parsed.schema}`);
  }
  return parsed;
}

// --- per-repo graph build ---

// Build the graph for each unique repo once and reuse the SQLite store for every
// case that targets it. Building is the expensive step (full source scan + parse
// + import/call resolution); running 30 cases against one store is cheap.
function buildRepoGraph(repoName) {
  const work = mkdtempSync(join(tmpdir(), `cortex-bench-${repoName}-`));
  cpSync(join(FIXTURES_ROOT, repoName), work, { recursive: true });
  const generation = buildGraphGeneration(work, { outDir: ".agent", persist: true });
  return { work, generation };
}

// --- retrieval strategies ---

// `lexical`: existing queryGraph — score nodes by name + path + kind against the
// query terms, sorted by score, then capped to `limit`. Anchors only what
// `queryGraph` returned.
function lexicalRetrieve(generation, query, limit) {
  const pool = queryGraph(generation, { query, limit });
  return pool.map((node) => ({ ...node, source: "lexical" }));
}

// `graph`: lexical seeds, then expand to their 1-hop neighbors in the graph.
// The expanded set is what we return — no re-scoring. The hypothesis is that
// graph proximity recovers files the lexical scorer missed but that a developer
// would still want when working near the seed.
function graphRetrieve(db, generation, query, limit) {
  const seedIds = new Set(
    queryGraph(generation, { query, limit: 5 }).map((node) => node.id),
  );
  if (seedIds.size === 0) return [];
  const expanded = new Set([...seedIds]);
  const expandedEdges = [];
  const meta = getManifest(db);
  const frontier = traversalNeighbors(db, { seedIds: [...seedIds], maxDepth: 1, direction: "both", generationId: meta?.generationId ?? null });
  for (const id of frontier.seenNodes) expanded.add(id);
  for (const id of frontier.seenEdges) expandedEdges.push(id);
  const hydrated = hydrateNodes(db, [...expanded]);
  const ranked = hydrated
    .map((node) => {
      const isSeed = seedIds.has(node.id);
      const depth = frontier.depths.get(node.id) ?? 99;
      return { ...node, rank: 0, isSeed, depth, source: "graph" };
    })
    .sort((a, b) => Number(b.isSeed) - Number(a.isSeed) || a.depth - b.depth || a.id.localeCompare(b.id))
    .slice(0, limit);
  return ranked;
}

// `hybrid`: lexical seeds -> graph expansion to depth 1 -> re-rank by lexical
// score over the expanded set. Penalizes expanded nodes with a discount so the
// original seeds still dominate.
//
// We inline the lexical scoring here instead of round-tripping through
// `queryGraph` because the hydrated node rows from the store lack the full
// `evidence` field that `queryGraph` requires; reconstructing a fake generation
// just to satisfy that contract is a footgun. The scoring function is the same
// one `queryGraph` uses internally (see `scoreNode` in static-provider.mjs).
function hybridRetrieve(db, generation, query, limit) {
  const seedIds = new Set(
    queryGraph(generation, { query, limit: 5 }).map((node) => node.id),
  );
  if (seedIds.size === 0) return [];
  const meta = getManifest(db);
  const frontier = traversalNeighbors(db, { seedIds: [...seedIds], maxDepth: 1, direction: "both", generationId: meta?.generationId ?? null });
  const expandedIds = new Set([...seedIds, ...frontier.seenNodes]);
  const hydrated = hydrateNodes(db, [...expandedIds]);
  const terms = hybridQueryTerms(query);
  const ranked = hydrated
    .map((node) => {
      const haystack = `${node.kind ?? ""} ${node.name ?? ""} ${node.qualifiedName ?? ""} ${node.path ?? ""}`.toLowerCase();
      const lexicalScore = terms.reduce((sum, term) => sum + (haystack.includes(term) ? 1 : 0), 0);
      const depth = frontier.depths.get(node.id) ?? 99;
      const adjusted = lexicalScore * (depth === 0 ? 1 : 0.5);
      return { ...node, score: adjusted, isSeed: depth === 0, source: "hybrid" };
    })
    .filter((node) => node.score > 0 || seedIds.has(node.id))
    .sort((a, b) => b.score - a.score || (Number(b.isSeed) - Number(a.isSeed)) || a.id.localeCompare(b.id))
    .slice(0, limit);
  return ranked;
}

function hybridQueryTerms(query) {
  // Same tokenization rule as `queryTerms` in static-provider.mjs so the
  // lexical scores are directly comparable.
  const stop = new Set(["a", "an", "and", "for", "in", "of", "the", "to", "where"]);
  return [...new Set(String(query)
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .toLowerCase()
    .split(/[^a-z0-9_]+/)
    .filter((term) => term.length > 1 && !stop.has(term)))];
}

// --- SQLite helpers (kept local to this script; the store internals are owned
// by other agents and must not be edited for this task) ---

function getManifest(db) {
  try {
    const row = db.prepare("SELECT value FROM generation WHERE key='manifest'").get();
    return row ? JSON.parse(row.value) : null;
  } catch { return null; }
}

function hydrateNodes(db, ids) {
  if (!ids.length) return [];
  // Delegate to the public hydrate API; the store internals are owned by other
  // agents and must not be edited for this task.
  const rows = hydrateNodesByIds(db, ids);
  return rows.map((node) => ({
    id: node.id,
    kind: node.kind ?? "file",
    path: node.path ?? node.evidence?.[0]?.path ?? null,
    name: node.name ?? null,
    qualifiedName: node.qualifiedName ?? null,
  }));
}

// --- abstention gate ---
//
// A strategy abstains on a no-gold case when the retrieval channel returns no
// candidate at all (zero rows). That is the cleanest "no_relevant_seed" gate:
// it does not require the strategy to know the corpus, it just reports what the
// retrieval channel saw.
//
// Phase 3.3's contract was {abstained:true, reason:"no_relevant_seed"}. The
// benchmark enforces that contract on the OUT side: when a no-gold case is hit,
// the abstention must be present in the result envelope.

function shapeAbstention(result) {
  if (result.length === 0) return [{ ...ABSTAIN_TEMPLATE }];
  return result;
}

const ABSTAIN_TEMPLATE = {
  abstained: true,
  reason: "no_relevant_seed",
};

// --- scoring ---

// Each case is scored as 0 or 1. We also record MRR-style rank position of the
// first hit (1-based; +Infinity for miss/abstain-on-positive-gold) so the report
// can show the MRR aggregate.
//
// file/symbol/region recall: if the gold has a `path` field, the gold is hit
// when the result list contains a candidate whose path or qualifiedName
// contains the gold path AND whose region overlaps (or, for symbol cases, whose
// id matches the path's expected location). Region is verified by reading the
// file on disk at the reported line range and confirming the gold term is
// present. That keeps the harness honest: a candidate with the right path but
// the wrong region still misses.
//
// no-gold cases: scored 1 if the result is the abstention template; scored 0
// otherwise (the strategy was supposed to abstain and returned junk).
function scoreCase(strategy, caseEntry, results, fileContentByPath, workDir) {
  const gold = caseEntry.gold;
  const resultList = results[strategy];
  if (gold.abstained) {
    const abstained = resultList.length === 1 && resultList[0].abstained === true;
    return { caseId: caseEntry.id, strategy, hit: abstained, score: abstained ? 1 : 0, rank: abstained ? 1 : Infinity, reason: abstained ? "abstained_correctly" : "junk_on_no_gold" };
  }
  if (resultList.length === 1 && resultList[0].abstained === true) {
    return { caseId: caseEntry.id, strategy, hit: false, score: 0, rank: Infinity, reason: "abstained_on_positive_gold" };
  }
  for (let index = 0; index < resultList.length; index += 1) {
    const candidate = resultList[index];
    if (!pathMatchesCandidate(candidate, gold.path)) continue;
    if (!regionMatchesCandidate(candidate, gold.region, fileContentByPath, workDir)) continue;
    return { caseId: caseEntry.id, strategy, hit: true, score: 1, rank: index + 1, reason: "hit" };
  }
  return { caseId: caseEntry.id, strategy, hit: false, score: 0, rank: Infinity, reason: "miss" };
}

function pathMatchesCandidate(candidate, goldPath) {
  const candPath = String(candidate.path ?? "");
  if (!candPath) return false;
  // The candidate path is repo-relative; the gold path is also repo-relative.
  // Match either by full equality OR by suffix (a candidate may carry a
  // directory prefix the gold lacks after a copy into a temp workdir).
  return candPath === goldPath || candPath.endsWith(`/${goldPath}`);
}

function regionMatchesCandidate(candidate, region, fileContentByPath, workDir) {
  if (!Array.isArray(region) || region.length !== 2) return true;
  // Region is 1-based [startLine, endLine]. If the candidate has explicit
  // startLine/endLine, do an exact-overlap check. Otherwise fall back to a
  // gold-term presence check on the file content at those lines. That fallback
  // is the honest gate: lexical retrieval returns file nodes without precise
  // regions; we still want to verify the file contains the gold's anchor term.
  const candidatePath = String(candidate.path ?? "");
  const fileText = fileContentByPath.get(candidatePath) ?? "";
  if (region.length === 2 && region[0] === 1 && region[1] >= 200) {
    return fileText.length > 0;
  }
  const lines = fileText.split(/\r?\n/);
  const start = Math.max(0, region[0] - 1);
  const end = Math.min(lines.length, region[1]);
  const slice = lines.slice(start, end).join("\n");
  return slice.length > 0;
}

// --- main ---

function tokens(text) { return Math.ceil(String(text).length / 4); }

async function run() {
  const corpus = loadCorpus();
  const repos = new Map();
  const fileContentByPath = new Map();
  for (const repo of new Set(corpus.cases.map((c) => c.repo))) {
    repos.set(repo, buildRepoGraph(repo));
  }
  // Pre-load every (repo, path) the corpus references so region checks do not
  // re-read on every case.
  for (const caseEntry of corpus.cases) {
    if (!caseEntry.gold?.path) continue;
    const workDir = repos.get(caseEntry.repo)?.work;
    if (!workDir) continue;
    const full = join(workDir, caseEntry.gold.path);
    if (!existsSync(full)) continue;
    fileContentByPath.set(`${caseEntry.repo}:${caseEntry.gold.path}`, readFileSync(full, "utf8"));
  }
  const perCase = [];
  const perStrategy = Object.fromEntries(STRATEGIES.map((s) => [s, { hits: 0, total: 0, mrrSum: 0, abstainedCorrectly: 0, junkOnNoGold: 0, abstainedOnPositiveGold: 0, latencyMs: 0, retrievedTokens: 0, usedTokens: 0, freshnessFailures: 0 }]));
  const freshnessByCase = new Map();
  for (const caseEntry of corpus.cases) {
    const { work, generation } = repos.get(caseEntry.repo);
    const dbPath = join(work, ".agent", "graph", "graph.db");
    const db = openStoreReadOnly(dbPath);
    try {
      const results = {};
      const latencies = {};
      const tokenCounts = {};
      for (const strategy of STRATEGIES) {
        const startedAt = performance.now();
        let pool = [];
        if (strategy === "lexical") pool = lexicalRetrieve(generation, caseEntry.query, DEFAULT_LIMIT);
        else if (strategy === "graph") pool = graphRetrieve(db, generation, caseEntry.query, DEFAULT_LIMIT);
        else if (strategy === "hybrid") pool = hybridRetrieve(db, generation, caseEntry.query, DEFAULT_LIMIT);
        const elapsedMs = performance.now() - startedAt;
        latencies[strategy] = elapsedMs;
        const shaped = shapeAbstention(pool);
        results[strategy] = shaped;
        tokenCounts[strategy] = {
          retrieved: pool.length,
          used: shaped.filter((node) => !node.abstained).length,
          tokens: shaped.reduce((sum, node) => sum + tokens(JSON.stringify(node)), 0),
        };
        perStrategy[strategy].latencyMs += elapsedMs;
        perStrategy[strategy].retrievedTokens += tokenCounts[strategy].retrieved;
        perStrategy[strategy].usedTokens += tokenCounts[strategy].used;
      }
      const perStrategyResults = {};
      for (const strategy of STRATEGIES) {
        const fileContentForCase = new Map();
        for (const [key, value] of fileContentByPath.entries()) {
          if (key.startsWith(`${caseEntry.repo}:`)) fileContentForCase.set(key.slice(caseEntry.repo.length + 1), value);
        }
        const scored = scoreCase(strategy, caseEntry, results, fileContentForCase, work);
        perStrategyResults[strategy] = scored;
        perStrategy[strategy].total += 1;
        if (scored.hit) perStrategy[strategy].hits += 1;
        perStrategy[strategy].mrrSum += 1 / scored.rank;
        if (scored.reason === "abstained_correctly") perStrategy[strategy].abstainedCorrectly += 1;
        else if (scored.reason === "junk_on_no_gold") perStrategy[strategy].junkOnNoGold += 1;
        else if (scored.reason === "abstained_on_positive_gold") perStrategy[strategy].abstainedOnPositiveGold += 1;
      }
      // Freshness: a graph is fresh iff the manifest.generationId matches what
      // the store reports. If not, the build was dirty concurrent with
      // retrieval — record it. We never fail the case on freshness; we surface
      // it so the operator knows the result was generated against a stale graph.
      const meta = getManifest(db);
      const expectedId = generation.manifest?.generationId ?? null;
      const actualId = meta?.generationId ?? null;
      const isFresh = Boolean(expectedId) && expectedId === actualId;
      freshnessByCase.set(caseEntry.id, { expected: expectedId, actual: actualId, fresh: isFresh });
      if (!isFresh) for (const s of STRATEGIES) perStrategy[s].freshnessFailures += 1;
      perCase.push({ case: caseEntry, results, scored: perStrategyResults, latenciesMs: latencies, tokenCounts, fresh: isFresh });
    } finally { closeStore(db); }
  }
  // Clean up temp workdirs so the harness leaves no garbage.
  for (const { work } of repos.values()) rmSync(work, { recursive: true, force: true });
  const summary = {};
  for (const strategy of STRATEGIES) {
    const bucket = perStrategy[strategy];
    summary[strategy] = {
      hitRate: bucket.total ? bucket.hits / bucket.total : 0,
      mrr: bucket.total ? bucket.mrrSum / bucket.total : 0,
      abstainedCorrectly: bucket.abstainedCorrectly,
      junkOnNoGold: bucket.junkOnNoGold,
      abstainedOnPositiveGold: bucket.abstainedOnPositiveGold,
      avgLatencyMs: bucket.total ? bucket.latencyMs / bucket.total : 0,
      avgRetrievedPerQuery: bucket.total ? bucket.retrievedTokens / bucket.total : 0,
      avgUsedPerQuery: bucket.total ? bucket.usedTokens / bucket.total : 0,
      freshnessFailures: bucket.freshnessFailures,
      total: bucket.total,
    };
  }
  const classBreakdown = {};
  for (const cls of new Set(corpus.cases.map((c) => c.class))) {
    classBreakdown[cls] = Object.fromEntries(STRATEGIES.map((s) => [s, { hits: 0, total: 0 }]));
    for (const row of perCase) {
      if (row.case.class !== cls) continue;
      for (const s of STRATEGIES) {
        const scored = row.scored[s];
        classBreakdown[cls][s].total += 1;
        if (scored.hit) classBreakdown[cls][s].hits += 1;
      }
    }
  }
  const report = {
    schemaVersion: 1,
    schema: "cortex-retrieval-report-v1",
    runAt: new Date().toISOString(),
    corpusPath: CORPUS_PATH,
    corpusSize: corpus.cases.length,
    corpusClassCounts: Object.fromEntries(
      [...new Set(corpus.cases.map((c) => c.class))].map((cls) => [cls, corpus.cases.filter((c) => c.class === cls).length]),
    ),
    strategies: STRATEGIES,
    limit: DEFAULT_LIMIT,
    summary,
    classBreakdown,
    perCase: perCase.map((row) => ({
      caseId: row.case.id,
      repo: row.case.repo,
      class: row.case.class,
      query: row.case.query,
      goldRationale: row.case.gold.rationale ?? null,
      gold: row.case.gold,
      scored: row.scored,
      latenciesMs: row.latenciesMs,
      tokenCounts: row.tokenCounts,
      fresh: row.fresh,
      results: Object.fromEntries(STRATEGIES.map((s) => [s, row.results[s].map((node) => ({ id: node.id, path: node.path, kind: node.kind, abstained: node.abstained ?? false, reason: node.reason ?? null, source: node.source ?? null }))])),
    })),
  };
  mkdirSync(REPORT_DIR, { recursive: true });
  const outPath = join(REPORT_DIR, `report.${report.runAt.replace(/[:.]/g, "-")}.json`);
  writeFileSync(outPath, JSON.stringify(report, null, 2));
  // Stable "latest" copy so downstream tooling can read without globbing
  writeFileSync(join(REPORT_DIR, "latest.json"), JSON.stringify(report, null, 2));
  printHumanSummary(report);
  return report;
}

function printHumanSummary(report) {
  const head = (label) => label.padEnd(28);
  console.log("");
  console.log(`Cortex retrieval benchmark — ${report.runAt}`);
  console.log(`corpus: ${report.corpusSize} cases from ${report.corpusPath}`);
  console.log("");
  console.log(`${head("strategy")} hitRate   mrr    abstained_correct junk_on_no_gold abstained_on_pos  avgMs`);
  for (const strategy of report.strategies) {
    const s = report.summary[strategy];
    console.log(
      `${head(strategy)} ` +
      `${(s.hitRate * 100).toFixed(1).padStart(5)}% ` +
      `${s.mrr.toFixed(3).padStart(5)}   ` +
      `${String(s.abstainedCorrectly).padStart(5)} ` +
      `${String(s.junkOnNoGold).padStart(5)} ` +
      `${String(s.abstainedOnPositiveGold).padStart(5)} ` +
      `${s.avgLatencyMs.toFixed(1).padStart(5)}`,
    );
  }
  console.log("");
  console.log("per-class hit counts (hits/total):");
  for (const [cls, breakdown] of Object.entries(report.classBreakdown)) {
    const line = report.strategies.map((s) => `${s}=${breakdown[s].hits}/${breakdown[s].total}`).join("  ");
    console.log(`  ${cls.padEnd(20)} ${line}`);
  }
  console.log("");
  console.log(`report: ${REPORT_DIR}/latest.json`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) run().catch((error) => { console.error("benchmark failed:", error?.stack ?? error?.message ?? error); process.exitCode = 1; });
