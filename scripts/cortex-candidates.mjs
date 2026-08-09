// Lean candidates entrypoint for latency-budget callers (Membrane federation).
//
// `cortex.mjs graph candidates` pays ~2s of module graph before any work: the
// full CLI imports tree-sitter providers, doc pipelines, watchman, phase2 — all
// irrelevant to answering "which files matter for this task". Membrane's
// production fan-out gives every provider well under that just to spawn, so the
// cortex lane could never deliver on a large workspace. This entry imports only
// the store and the candidate assembly, and answers in the low hundreds of
// milliseconds at 150k-symbol scale (measured 2026-08-09: 662 ms wall,
// indexedQueryGeneration 659 ms of it).
//
// Output contract (same as `graph candidates`): a ContextCandidateSet v1
// object on stdout — `{ schemaVersion, candidates: [...] }`. Read-only: this
// path never repairs, reconciles, or writes; freshness verdicts are the
// resident service's job.
import { closeStore, openStoreReadOnly, readManifestEnvelope } from "../graph/store-sqlite.mjs";
import { indexedQueryGeneration } from "../graph/traverse-store.mjs";
import { createContextCandidateSet } from "../graph/static-provider.mjs";
import { join, resolve } from "node:path";

function parseArgs(argv) {
  const args = { _: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith("--")) { args._.push(arg); continue; }
    const [key, inline] = arg.slice(2).split("=", 2);
    args[key] = inline !== undefined ? inline : argv[++i];
  }
  return args;
}

const args = parseArgs(process.argv.slice(2));
const root = resolve(args.root ?? process.cwd());
const outDir = args.out ?? ".agent";
const task = String(args.task ?? args.query ?? "").trim();
const limit = Math.max(1, Math.min(256, Number(args.limit ?? 40) || 40));

const started = process.hrtime.bigint();
const db = openStoreReadOnly(join(root, outDir, "graph", "graph.db"));
try {
  const envelope = readManifestEnvelope(db);
  const generationId = envelope?.generationId ?? null;
  if (!generationId) {
    console.error(JSON.stringify({ error: "graph_missing", detail: "store holds no generation" }));
    process.exit(2);
  }
  const expected = String(args["expected-generation"] ?? "").trim();
  if (expected && expected !== generationId) {
    console.error(JSON.stringify({ error: "generation_mismatch", expected, actual: generationId }));
    process.exit(3);
  }
  const generation = indexedQueryGeneration(db, task, { limit });
  const candidateSet = createContextCandidateSet(generation, {
    task,
    query: task,
    maxCandidates: limit,
    anchors: [],
    repoRoot: root,
    receiptId: null,
  });
  const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6;
  console.log(JSON.stringify({
    ...candidateSet,
    // The caller (Membrane's cortex provider) needs the generation id for its
    // cache key and return contract. Reporting it here removes the separate
    // `graph status` probe, which paid the full CLI module graph per packet.
    generationId,
    _membrane: { stageElapsedMs: { repo_code_scan: elapsedMs } },
  }));
} finally {
  closeStore(db);
}
