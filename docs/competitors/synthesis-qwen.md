# Competitor absorption synthesis — Qwen pass

One document consolidating the 45 per-repo absorption studies produced by the Qwen model pass
into a single, deduplicated, sequenced program for Cortex.

- **Input**: 45 study files, one per competitor, each proposing ~5–10 absorptions (≈420 raw items).
- **Corpus**: `docs/competitors/` index lists 55 repositories under `repos/`; **10 are unstudied in
  this pass** — `ast-grep`, `biomejs/gritql`, `codegraph-ai/CodeGraph`, `colbymchenry/codegraph`,
  `cq27-dev/rag-rat`, `forloopcodes/contextplus`, `github/codeql`, `github/stack-graphs`,
  `harshkedia177/axon`, `heurema/signum`. Treat conclusions below as covering 45/55.
- **Scope**: this is the synthesis of **one model's** pass. Three further model passes exist; this
  file is written so a cross-model merge can be layered on top without restructuring.

Everything here is a *proposal*, not a landed change. Nothing in this document has been
implemented; source claims about competitor internals are as reported by the per-repo studies and
have not been independently re-verified against the vendored repos.

---

## 1. How to read this

Raw items were deduplicated on mechanism, not on wording. The result:

| Bucket | Count | Meaning |
|---|---|---|
| **Convergences** | 14 clusters | ≥3 independent repos arrived at the same mechanism. Highest confidence; these are the load-bearing findings. |
| **Distinct absorptions** | ~95 | Single-source mechanisms worth taking. Organised by theme in §3. |
| **Confirmations** | ~60 | Competitor does what Cortex already does. Value is as an audit checklist, not work. |
| **Contingent** | ~45 | Depends on a lane Cortex hasn't built (vector/FTS/LLM/remote). Parked in §7 with the shape pre-decided. |
| **Anti-patterns** | ~30 | Explicitly-observed mistakes. Register in §6; several are worth encoding as regression tests. |

Confidence heuristic used throughout: **a mechanism that three unrelated codebases independently
converged on is a design fact; a mechanism only one repo has is a design opinion.**

---

## 2. The 14 convergences

These are the strongest signals in the corpus. Each names the repos that independently landed on
the same mechanism.

### C1. Per-edge resolution confidence, not per-provider precision
*CodeGraphContext (9-tier `_TIER_CONFIDENCE`), codebase-memory-mcp (`import_map 0.95 → fuzzy 0.30`
bands), sense (centralised policy table), potpie (`deterministic 1.0 … speculative 0.2`),
code-review-graph, cognee, repo-lens (claim-level twin).*

Six repos stamp **each edge** with the strategy that produced it and a numeric confidence, and
expose an explicit `AMBIGUOUS` class. Cortex carries precision at the *provider* tier — coarser than
every serious competitor. This is the single most-agreed-upon gap.

**Absorb**: one documented confidence-policy table; every edge stamped `(strategy, confidence,
label)`; ambiguous edges queryable; strict-vs-permissive traversal becomes a caller choice.

### C2. Epistemic honesty on every bounded response
*GitNexus (`epistemic: exact|lower-bound` + machine-readable cause counters), codebase-memory-mcp
(`truncated` flag), repo-graph (`[... truncated: N of M chars omitted]`), brain0 (`*_total` +
`+N more`), context8 (`hasMore` + `nextPageSuggestion`), praisonai (head/tail marker), sense
(suppress-not-truncate + ring pagination), Code-Index-MCP (`readiness` + `safe_fallback`),
potpie (worst-family coverage as overall confidence).*

Nine repos independently concluded that a truncated or incomplete answer must **say so in a
machine-readable field**. GitNexus goes furthest: it distinguishes "this impact set is exact" from
"this is a floor" and counts *why* (dropped call sites, DI boundaries, externals).

**Absorb**: a standard response envelope — `epistemic`, `partial`/`truncated`, `*_total`,
`readiness`, `provider_used`/`fallback_used`, `nextPageSuggestion`, per-family coverage. One shape,
every surface.

### C3. Co-change coupling from git history as a ranking signal
*roam-code (NPMI), sense (`minCoChanges=3`, 6-month window, 2s timeout), agentic-codebase
(co-change matrix, strength = co/max), codebase-graph (`MODIFIED_IN` temporal edges), octocode,
CodeGraphContext (`EvolutionTimeline`).*

Six repos derive an edge type static analysis cannot produce. Bounded consistently: minimum
co-change count, time window, hard timeout, silent no-op without git.

**Absorb**: bounded co-change edges as a low-confidence relationship kind + a ranking component,
with the caps as explicit config.

### C4. Query-shape classification steering ranking weights
*sense (`classifyQuery` → Identifier|NaturalLanguage|Mixed, ~200 lines, no model), octocode
(`classify_query_weights`, ≤3 words + identifier chars → tilt 0.3/0.7), agentic-codebase
(`QueryIntent::classify`), roam-code (`_is_impl_style_query`), code-review-graph (`infer_intent`),
context8 (mode→facet table).*

All six are **pure lexical** classifiers — deterministic, offline, cheap. Cortex uses fixed weights
regardless of query shape.

**Absorb**: a lexical query-shape classifier that tilts component weights, with the classification
reported as part of the explanation.

### C5. Rank-based fusion (RRF k=60) for heterogeneous lanes
*llama_index (RRF + min-max-3σ + weights), semantica (k=60 "industry fixed point"), GitNexus
(K=60), octocode (weighted RRF + recompute-true-distance-after-fusion), Code-Index-MCP (per-branch
timeouts + partial success), codebase-graph (weighted `rrfFuse`), claude-context (server-side).*

Seven repos, one constant. The refinement worth stealing is octocode's: **fusion order is
rank-based; displayed/thresholded scores are recomputed on a comparable scale.**

**Absorb**: RRF k=60 when merging independently-ranked lanes (weighted sum stays for aligned
components); per-source timeouts producing a `DEGRADED_SOURCE` receipt rather than stalling.

### C6. Rename/move identity resolution instead of add+delete
*brain0 (shingle-Jaccard ≥0.6, deterministic sort, greedy 1:1, `Renamed`/`Moved` preserving
ArtifactId), Code-Index-MCP (`file_moves` matched by content hash), semgrep (baseline rename
remapping), opengrok (no-merge-on-rename), opengrep (multi-fingerprint identity), treesitter-chunker
(`definition_id` from `qualified_route`), GitNexus (strategy-tagged rename preview).*

Cortex treats a rename as delete + add; every edge and claim referencing the old symbol goes stale.
Seven repos solved it, three different ways: structural fingerprints, content-hash matching, and
route-derived stable ids.

**Absorb**: two identities per span — `contentHash` (verification, unchanged) plus a route-derived
`definition_id` (continuity) — and a shingle-Jaccard pairing pass emitting typed
`SYMBOL_RENAMED`/`SYMBOL_MOVED` edges with similarity scores.

### C7. Coverage honesty — a queryable "what I could not index"
*codebase-memory-mcp (`index_coverage` tables + `graph:"missed"` shadow projection),
agentic-codebase (`ParseCoverageStats` skip-reason histogram), repo-graph (`coverage()` →
per-(language, edge_category) blind-spot footer), code-compress (`ParseFailure` records),
roam-code (`steps_status`), Sourcetrail (`indexed`/`complete` flags + errors as located elements),
code-index-mcp/johnhuang (`status:"partial"` with counts), ai-code-audit (visible SKIPPED
placeholders), deepwiki (`SKIPPED_OVERSIZE` receipt).*

Nine repos. The strongest form (codebase-memory-mcp) makes the *gap* queryable through the same API
as the data — "no false clean" made mechanical.

**Absorb**: per-build skip-reason accounting, per-file parse partiality with line ranges, a
queryable coverage surface, and an agent-facing blind-spots footer telling the caller where to
fall back to grep.

### C8. Parse errors stay in the tree; extraction degrades, never fails
*tree-sitter (`has_error`/`error_cost`/`is_missing` O(1) bits), oxc (dummy nodes + `panicked`
flag), semantic (`Parse.Success | Parse.Error` per child), treesitter-chunker (depth-guard →
sliding-window fallback with `FALLBACK_USED` receipt), Sourcetrail (`recordError(fatal, location)`),
RepoDoctor (per-module fault isolation), code-compress (`ParseFailure` non-aborting).*

Three parser-grade codebases (tree-sitter, oxc, semantic) converged on the *identical* contract
from three languages: **never return "no tree" — return a complete-but-degraded tree plus a health
tag.** Downstream branches on one bit instead of handling exceptions.

**Absorb**: parse-health bits on every file's extraction; a guarded extraction ladder (structural →
positional-window fallback at LEXICAL tier); one extractor's failure is a finding, never a build
failure.

### C9. Determinism as an enumerated discipline, not a hope
*joern (`LinkedHashMap` merged in sorted-file order, associative `mergeWith`), react-doctor (fixed
concat order after concurrent fibers + stable sort, asserted), dependency-cruiser (posix-normalise
before hashing, env-pinned tests, hyperfine perf gates), GitNexus (seeded `mulberry32`,
`LEIDEN_SEED`), codebase-memory-mcp (sequential canonical admission after parallel scoring),
agentic-codebase (`repo_identity_key_is_deterministic`), aider, semantic (commutative monoid
merge), CodeGraphContext (sorted inputs + memoised path resolution), lsif-go (striped-lock dedup),
repo-lens.*

Eleven repos. The checklist that emerges: posix-normalise before hashing; every sort declares a
total order; parallel work produces immutable per-part deltas merged once in sorted order; RNG is
seeded; env vars that affect output are pinned in tests; tie-breaks are explicit, never
map-iteration order.

**Absorb**: the checklist as a contract clause, plus two tests Cortex lacks — a **commutativity
test** (two random file orders → byte-identical generation) and an **incremental ≡ full-rebuild
equivalence test** (GitNexus's; the strongest possible incrementality proof).

### C10. Agent-efficiency is the metric that sells the tool
*claude-context (−39.4% tokens, −36.3% tool calls at equal F1 on SWE-bench subset), repo-graph
(A/B harness: same task, no-MCP arm vs MCP arm, pinned model, fresh clone, median+IQR),
code-review-graph (`token_benchmark.py` naive-corpus vs graph-query tokens), mentat (set
precision/recall at fixed budget), Code-Index-MCP (frozen eval with sha256 corpus checksums +
nDCG/MRR/recall@k), codebase-graph (`cgbench` adapters benchmarking 8 external systems),
llama_index (metrics over the *full* pipeline including post-processors), roam-code (self-bench +
deterministic weight sweeps).*

Cortex's evals measure graph correctness (pass/fail). Eight repos measure something Cortex does
not: *graded* ranking quality, and *agent cost to answer*.

**Absorb, in order**: (a) checksum the frozen eval corpus, fail on tamper; (b) graded metrics
(nDCG/MRR/recall@k, node- and span-level precision/recall with `allowedAlternates` as non-penalty);
(c) metrics computed **after** every post-pass; (d) an A/B agent harness measuring turns/tokens/cost
to completion with and without Cortex mounted.

### C11. Assemble-to-budget as a single named entry point
*code-compress (`assemble_context`: search → 10%-capped overview → active-file tier → per-symbol
size guard → greedy first-fit → usage footer), mentat (residual-budget computation, admitted refs
written back so selection is inspectable), aider (binary search over ranked-prefix length with a
sampling token estimator, O(log n) renders), treesitter-chunker (`pack_hint` ordering score),
octocode (fixed 100 tokens/node, 50/edge estimates), repo-graph (`dense_text` orientation
primitive), praisonai (declarative `FITS|COMPACT|TRUNCATE` route computed before assembly).*

Seven repos ship "task query → packed context within N tokens" as *one* call. Cortex has budgets
and neighbourhoods but no assembler.

**Absorb**: a packing entry point over hybrid ranking — declarative route decision, lane-priority
admission, running cost against the ceiling, a typed omission receipt for every candidate that lost
the budget race, and a usage footer. Order candidates by a `pack_hint` derived from components
already computed.

### C12. Cheap-to-expensive retrieval ladders with skeletons before bodies
*code-compress (`project_outline` → `topic_outline` → `get_module_api` → `get_symbol` →
`expand_symbol`; oversized symbols emit signature + children + `ExpandWith` pointers), aider
(scope-skeleton `TreeContext` output at ~1/50th the tokens), context8 (summary tier + implementation
tier), serena (progressive-shortening ladder, each step logged), infigraph (`skeleton` rendering:
fan-in, statement count, nesting depth), repo-graph (`dense_text` legend + histogram).*

**Absorb**: a skeleton evidence mode and an outline tier admitted before full bodies; oversized
spans emit signature + child list + expansion pointers (Cortex can attach content hashes to the
summary — none of them can).

### C13. Read-set / probe-set invalidation beats input-hash invalidation
*react-doctor (probe sets `[kind, path, answer]`, replay requires all probes to re-answer
identically, superset-soundness enforced by a test, `UNBOUNDED_*` escape hatch), llama_index
(stage memo key = `sha256(content + stage config)`), opengrok (settings snapshot stored **in the
index** so a config toggle invalidates only affected docs), dependency-cruiser (`optionsAreCompatible`
semantic predicates — a change that *weakens* detection can reuse cache; one that could add findings
cannot), serena (version-tuple caches + never-cache-empty), Code-Index-MCP (parse cache keyed on
content **and parser version**), GitNexus (schema + package version + bump changelog),
treesitter-chunker (grammar/distribution versions in every cache key), RepoDoctor (re-validate
cached JSON against current schema on read).*

Nine repos. Cortex invalidates on input content hash. The corpus says: cache keys must include
**the analysis's whole read set**, or at minimum **producer + config versions**, and anything whose
read set can't be proven must be classified always-recompute — with a test that the classification
is exhaustive.

**Absorb**: version-stamped cache keys everywhere (parser, grammar, extractor, config), never-cache-
empty, schema-revalidate-on-read, fail-open decode, and the superset-soundness discipline.

### C14. Soft invalidation with supersession, not deletion
*potpie (soft-invalidate + supersession link + per-family TTL), roam-code (`supersedes_id` chain in
a findings registry with idempotent upsert), codebase-graph (bitemporal `valid_at`/`invalid_at` +
decay/prune lifecycle), semantica (bi-temporal `valid_at` vs `recorded_at` + hash-chained
provenance), cognee (rollback ledger flushed **before** the graph write completes), agentic-codebase
(`TruthMaintainer` re-checks registered claims after rebuild).*

**Absorb**: verdicts gain validity windows and supersession edges; stale facts degrade rather than
vanish; claims are re-checked after a rebuild rather than silently dropped; undo records persist
before the data they protect.

---

## 3. Thematic absorption catalogue

Distinct (non-convergent) mechanisms worth taking, grouped by the Cortex subsystem they land in.
Source repo in parentheses.

### 3.1 Extraction and providers

| # | Mechanism | Source |
|---|---|---|
| E1 | **Honor SCIP `position_encoding` per document.** UTF-16 indexes yield silently shifted spans on non-ASCII source — a latent correctness bug in `scip-provider.mjs`. | scip |
| E2 | **Mine the rest of SCIP.** Provider reads only `roles:["reference"]`. The format already carries: `SymbolInformation.kind` (87-value enum, authoritative over descriptor suffixes), `relationships` four flags → IMPLEMENTS/TYPE_DEF edges, the full `SymbolRole` bitset (Definition/Import/Write/Read/Generated/Test), `enclosing_range` (body spans, not just name ranges), and per-occurrence indexer diagnostics. Free precision-tier edges in files Cortex already reads. | scip |
| E3 | **SCIP as an upgrade layer**, not a parallel provider: SCIP edges supersede lexical edges for the same `(source,target)` with a recorded tier upgrade; heuristic edge retained as fallback. | CodeGraphContext |
| E4 | **`ts_subtree_get_changed_ranges`** — narrow re-extraction to touched spans inside a changed file; unchanged symbols keep their evidence. Plus CoW `ts_tree_edit` + old-tree reuse for cheaper reparses. The highest-leverage tree-sitter API Cortex isn't calling. | tree-sitter |
| E5 | **Field-id access** (`ts_node_child_by_field_id`) instead of positional child scanning; supertypes and quantified captures in patterns — grammar upgrades stop breaking extraction. | tree-sitter |
| E6 | **Merge all of a language's patterns into ONE query**, dispatch matches by `pattern_index`. One cursor pass; match limits apply once. Adoptable inside the current architecture. | tree-sitter-graph |
| E7 | **Confluence contract**: setting an attribute twice is legal only if values are equal; contradictions are loud errors, not last-write-wins. At most one edge per node pair; sorted adjacency. | tree-sitter-graph |
| E8 | **Lazy attribute thunks** (`Unforced→Forcing→Forced`) with cycle detection and force-all-at-end, so no silently-unevaluated state ships. | tree-sitter-graph |
| E9 | **Static spec checker** — validate language tables before the build starts; fail with table name and pattern, not mid-file. | tree-sitter-graph |
| E10 | **Declarative node-type adjustment rules** (merge/reinterpret/promote/synthesise) in the language-table layer: Dart signature+body merge, Elixir `call`-as-definition, C++ out-of-line `Class::method`, R assignment expansion. Grammar quirks live next to the tables, not inside walkers. | treesitter-chunker |
| E11 | **Mixed-language region decomposition** — JSX/Vue SFC/Markdown/notebook regions chunked with their own language, cross-language references recorded. | treesitter-chunker |
| E12 | **String/comment masking before reference resolution** + require import-backing for cross-file lexical edges. Cheap, kills a whole class of false-positive impact claims. | roam-code |
| E13 | **Poison-on-collision name linking**: build maps of *unique* short names and unique dotted suffixes; on collision set the entry to `None` so ambiguous names never link. Resolution order exact id → unique short name → unique suffix. | code-index-mcp (johnhuang) |
| E14 | **Pre-scan imports map** (`symbol → [files]`) in a fast first pass, feeding cheap name resolution without a type engine; plus import-scoped disambiguation as a low→high confidence upgrade pass. | CodeGraphContext, code-review-graph |
| E15 | **Collect-then-resolve**: gather identifier occurrences flat during the walk, resolve against scope chains in one post-pass. No per-scope hashmap churn; easier to make incremental. | oxc |
| E16 | **Sparse-set superset checks over method-signature ids** as a language-agnostic structural IMPLEMENTS detector. | lsif-go |
| E17 | **Deterministic file ownership**: when multiple extractors claim a file, first-in-stable-order wins — stated, tested, surfaced in diagnostics. | lsif-go |
| E18 | **Longest-prefix dependency binding** when attributing external symbols (handles nested modules, vendoring, monorepo subprojects); coalesce all references to one external symbol through a single shared node. | lsif-go |
| E19 | **Magic-byte language tier**: filename-exact → extension → prefix → 8-byte magic (longest-match sorted map) → matcher lambdas → default. | opengrok |
| E20 | **Grammar capability pins**: record capability probes (which node types exist) per grammar in the catalog; a grammar upgrade that loses a capability fails the provider build with a typed finding. | treesitter-chunker |

### 3.2 Identity and storage

| # | Mechanism | Source |
|---|---|---|
| S1 | **Split edge identity from occurrence locations.** Edge identity = `(kind, sourceId, targetId)` deduped on write; evidence spans become occurrences referencing that edge. One edge, many evidence sites. | Sourcetrail |
| S2 | **Subtract-shared partial clearing** — collect element ids with locations in the cleared files, iteratively subtract elements that also occur elsewhere, delete the remainder. Lets a single-file refresh run inside a generation without destroying shared symbols. | Sourcetrail |
| S3 | **Checkout-root-independent ids**: separate the read path from the identity path; assert with a test that indexes one fixture from two mount paths and requires identical node ids. | treesitter-chunker |
| S4 | **Locale-independent digests**: length-framed UTF-8, UTF-16 code-unit sort order. Add a cross-locale test (the `tr_TR` collation trap). | Understand-Anything |
| S5 | **Kind-namespaced identity keys** (`kind:value` hashed) so structurally-equal entities of different kinds never collide; warn at admission on identity-field shadowing. | cognee |
| S6 | **Environment-qualified edge identity** — the same relation under a different branch/config/version is a distinct edge with its own verdict history. | potpie |
| S7 | **Overload-index addressing** (`Class/method[1]`) in the symbol vocabulary — disambiguates same-scope duplicates without name mangling. | serena |
| S8 | **Generation-checked pagination cursors** — embed the generation token in every cursor; stale cursors after a reindex fail closed instead of returning wrong pages. | codebase-memory-mcp |
| S9 | **Scheme-stamped write gating**: records carry the producer scheme (provider + extractor version + language-table hash); writes fail fast on mismatch rather than silently mixing differently-shaped evidence. | Code-Index-MCP |
| S10 | **Batched writes**: parameter-chunked multi-row inserts (450 params against SQLite's 999 limit) with an in-memory name→id dedup map held for the build transaction. | Sourcetrail, code-review-graph |
| S11 | **Recursive-CTE BFS with a temp seed table** — traversal in SQL rather than materialised in JS; `GROUP BY node` keeps min depth; canonical `(hop, id)` ordering; explicit `truncated` flag. | code-review-graph, codebase-memory-mcp |
| S12 | **Covering reverse-edge index** `(target_id, kind, source_id)` pinned by an `EXPLAIN QUERY PLAN` test. | sense |
| S13 | **Correlation as SQL over an in-memory materialised view** — recursive CTEs give transitive queries for free instead of a bespoke join engine. | semgrep |
| S14 | **Staging + atomic swap publish**; temp-file + atomic rename for every reader-observable artifact; prior generation preserved on any pre-rename abort. | codebase-memory-mcp, opengrok |
| S15 | **Undo before data**: rollback/provenance records flush before the write they protect completes, and provenance rides in the same transaction as the fact. | cognee |
| S16 | **Store-conformance suite** any backend must pass, encoding the upsert/idempotency contract (File→Path, Symbol→(FileID,Qualified), Edge→(Source,Target,Kind,File)). | sense, potpie |

### 3.3 Incrementality and freshness

| # | Mechanism | Source |
|---|---|---|
| I1 | **Header-aware invalidation**: `filesToClear = changed ∪ getReferencing(changed)`, minus files whose reference is provably unchanged. Editing a header re-indexes what includes it. | Sourcetrail |
| I2 | **Closure-repair route**: persist per-file resolution surfaces (`surface_sha` of definitions + a referenced-identifier bloom filter); before applying a delta, *probe* whether the change set is resolution-closed, else escalate. The refined version of I1. Route enum `NOOP / FORCED_FULL / LEGACY_PARTIAL / CLOSURE_REPAIR`. | codebase-memory-mcp |
| I3 | **Parse scope ≠ write scope**: re-parse everything cross-file resolution needs, write back only the effective write set, expanded 1 hop across writable boundaries (fixes barrel re-export staleness). Escalation gate: write set >50% of repo **and** ≥50 files → wipe + bulk. `incrementalInProgress` dirty flag forces a full rebuild after a crash. | GitNexus |
| I4 | **Cosmetic vs structural change classification**: content hash + structural signature (params/returns/exported/methods/imports/exports) → `NONE | COSMETIC | STRUCTURAL`. A comment-only edit stops costing a full re-extraction. | Understand-Anything |
| I5 | **Update-scope decision matrix**: `SKIP | PARTIAL | ARCHITECTURE | FULL` chosen by change magnitude, gating expensive passes (community detection, claim re-verification) with the chosen scope recorded in receipts. | Understand-Anything |
| I6 | **Git change-feed incremental mode** — harvest changed paths since the last-indexed commit instead of walking, with a content-hash no-op guard. Strictly better than walking on large repos. | opengrok |
| I7 | **Sorted-merge deletion detection** — one O(n+m) merge over a sorted `(path, mtime)` key against stored terms; advancing past a term means the file was deleted. Cheaper than hashing for deletion discovery. | opengrok |
| I8 | **HEAD-divergence freshness contract**: `fresh/dirty/stale/unknown` × `behind/ahead/diverged` via `merge-base --is-ancestor` + `rev-list --left-right --count`, with `unknown` deliberately distinct from `fresh`, timeouts, and a freshness footer on every query response. | Understand-Anything, sense |
| I9 | **Bounded session-start drift reconcile** — compare on-disk mtime vs `indexed_at` (a git pull bumps mtimes, so no persisted HEAD needed), capped at N files, modifications only (deletions left to the watcher to avoid discarding data on a transient stat miss). | sense |
| I10 | **Overlay registry on the generation** — record which derived passes have been applied to the current merkle generation so they can be skipped or selectively re-run without a full rebuild. | joern |
| I11 | **Branch-delta overlay generations**: per-branch store holding only delta files, anchored to a `base_db_commit` manifest, path-override merge at query time, coherence check failing closed when the base drifted, prune on merge/delete. | octocode |
| I12 | **Hash-set differential application** — insert-missing-hash-set + delete-stale-hash-set per file at span granularity. | octocode |
| I13 | **Watcher hardening** (four independent repos): generation-counter debounce so stale timers no-op; debounce on **last** event, not first; drain in-flight callbacks on stop; `remove_files_by_prefix` for directory deletes where per-file events never fire; exclude Cortex's own output paths; `mkdir`-based cross-process lock; documented backend-selection table. | code-index-mcp (johnhuang), infigraph, claude-context, repo-graph |
| I14 | **Neighbour-scoped re-linking** — on change, re-link the changed file plus its graph neighbours only, and keep a set-diff `synchronize` reconciliation pass against disk. | CodeGraphContext |
| I15 | **Structural now, enrichment deferred** — cheap structural updates apply synchronously (<500ms searchable); expensive passes run on a bounded deferred queue with status receipts. | codebase-graph |
| I16 | **Stat-first with hash repair** (ninja pattern): identity `[mtimeMs, size, contentHash]`, verify by stat first, repair same-size mismatches by hashing; **write** hash-then-stat so a racing edit can't create a repairable stale entry. | react-doctor |
| I17 | **Snapshot↔store reconciliation with write guards** — refuse degenerate 0/0 writes, heal drift at startup, classify orphans as transient vs permanent before deleting anything. | claude-context |
| I18 | **Index quarantine** — on ledger/store consistency failure, stop serving the generation with a typed finding rather than serving possibly-wrong evidence. | Code-Index-MCP |

### 3.4 Query surfaces and products

| # | Mechanism | Source |
|---|---|---|
| Q1 | **Diff-range → span scoping.** `git diff --unified=0 <target>` hunk ranges overlaid on evidence-span line ranges answers *which evidence a PR touched*. Combined with a `ChangeTarget` primitive (treeish classification incl. merge-base resolution for PR branches), `diff_impact` goes from "blast radius if X changes" to "blast radius of THIS diff". | code-review-graph, mentat, codebase-memory-mcp |
| Q2 | **Failure-signal resolution** — sniff stack traces, `path::test` ids and unified diffs, and map them to nodes/spans. The productised form of Q1; stack-trace ingestion becomes deterministic. | repo-graph |
| Q3 | **`locate path:line`** — narrowest-span symbol containing a line. Tiny surface, enormous agent UX leverage when staring at a stack trace. | brain0 |
| Q4 | **Liveness / entry-point reachability** as a first-class attribute on every impact row, with a `live_only` filter. Prevents overstated impact claims. | repo-graph |
| Q5 | **Edge provenance in impact output** — each row carries the edge that justified inclusion (`via`), hop depth, and score; structural edges (contains/imports/defines) excluded from fan-out; multi-seed impact keeps the best score per node. | repo-graph |
| Q6 | **Confidence-decay reverse BFS** as a directional impact query complementing PageRank: cumulative confidence product as the primary depth control (`MinConfidence=0.5`), per-kind decay (inherits 0.7, composes 0.5, includes 0.3, floor 0.2), `MaxHops`, `MaxFrontierWidth`, tier classification (breaks/references/tests). | sense |
| Q7 | **Transitive test coverage** (`TESTED_BY` + BFS over CALLS) and a **minimal test set** derived by propagating change probability to Test-kind nodes — "what should I run for this change". | code-review-graph, agentic-codebase |
| Q8 | **Test-target ranking**: derive `TESTED_BY` edges from "Test-kind symbol calls non-Test symbol"; rank untested symbols by `complexity*5 + public*10 + callers*3`. Fully deterministic, built on existing facts. | infigraph |
| Q9 | **Dead code, two ways**: zero-inbound-CALLS with a typed exclusion set (dunder/test/entry-point/decorated), and universe subtraction (defined − referenced) as a cheap parallel-friendly signal. | CodeGraphContext, semantic |
| Q10 | **Trail query** `(origin, target, nodeKinds, edgeKinds, maxDepth, directed)` — bounded paths between two symbols with type masks; the impact-path counterpart to blast radius. | Sourcetrail |
| Q11 | **Cycle detection + transitive dependents** as named first-class queries over precomputed indices, plus the **instability metric** `Ce/(Ca+Ce)` — one line of SQL over existing degree counts. | dependency-cruiser |
| Q12 | **Community detection** (Leiden with seeded RNG, or Louvain with sorted input + delta-Q verification) as an analytics provider emitting cluster nodes + `MEMBER_OF` edges. | codebase-memory-mcp, infigraph |
| Q13 | **Entry→sink process flows** as first-class entities: entry-point detection (no-incoming-CALLS ∪ framework-decorator patterns ∪ name conventions), forward BFS, persisted flows, and — separately valuable — **delete-only-affected + re-seed** as the template for caching *any* derived analytic across generations. | GitNexus, code-review-graph |
| Q14 | **Semantic diff between refs** — symbol-level added/removed/modified classification restricted through the ledger's changed-file set. | infigraph, code-compress |
| Q15 | **Signature-level change reports** — snapshot per-file symbol summaries (`name|kind`, signature) and emit `+added / ~signature-changed / -removed`; with I4 this answers "what API actually changed". | code-compress |
| Q16 | **Hot-path line windows** — given identifiers, return interval-merged match windows *inside* the containing symbol instead of whole bodies. | code-compress |
| Q17 | **File-mention reverse facet** — "which docs/claims mention path P", with glob-metachar escaping and ancestor-directory prefix matching. Powers doc-contradiction discovery and rename impact. | codealmanac |
| Q18 | **Oracle tools** returning deterministic yes/no (`symbol_exists`, `route_exists`, `is_reachable_from_entry`) that agents can use as cheap assertions. | roam-code |
| Q19 | **Auto-merging promotion**: when >0.5 of a container's children are retrieved, replace them with the container; iterate to fixpoint. Plus >50% line-overlap dedup and best-span-per-file partition collapse. | llama_index, claude-context, codealmanac |
| Q20 | **Manifest provider** — `go.mod`/`Cargo.toml`/`requirements.txt`/`package.json` parsers emitting `DEPENDS_ON` edges with runtime/dev distinction, plus npm `scripts` as a ready-made "how do I run/test this" orientation signal. | repo-lens |
| Q21 | **Special-files tier** — a curated ~170-entry manifest (`pyproject.toml`, `Dockerfile`, CI configs) admitted ahead of ranking as a visible `specialFile` component. Fixes the classic "the answer was in the Dockerfile" miss. | aider |

### 3.5 Ranking

| # | Mechanism | Source |
|---|---|---|
| R1 | **Adaptive-damping PageRank**: damping interpolated 0.92 (DAG) → 0.82 (cyclic) by SCC-node ratio; hard iteration cap returning best-so-far so it can never hang; degree-ranking fallback preserving the score-sum contract. Low-risk, direct upgrade over fixed 0.85/20. | roam-code |
| R2 | **Seed inference → personalised PageRank without embeddings**: tokenise the task text (camel/snake splits, abbreviations), accumulate FTS/lexical scores per token, feed the seed distribution as the personalisation vector. Aider's variant adds filename-stem matching and recirculates dangling mass into the query-conditioned set. | roam-code, aider |
| R3 | **Ident-shape edge weights**: ×10 for query-mentioned identifiers, ×10 for informative names (snake/camel, len ≥ 8), ×0.1 for `_private`, ×0.1 for ubiquitous names (defined in >5 places), sqrt-damped reference counts. A call edge through `init` should not weigh like one through `processPaymentIntent`. | aider |
| R4 | **Graph-enrichment components**: test-reference count, dependency depth from entry points, churn recency, caller/importer counts — all derivable from the existing graph in one batched query. | codebase-graph |
| R5 | **Edge importance prior**: implements/extends/architectural 1.0 > imports/calls/uses 0.7 > sibling/parent/child module 0.3, feeding neighbourhood expansion. | octocode |
| R6 | **Test/infrastructure demotion** with the dual pre-norm/post-norm pattern, a `demote()` sign guard against negative scores inverting penalties, and a `queryTargetsTests()` opt-out. | sense |
| R7 | **Dependency-origin demotion** — repo code ordered above vendored dependencies as an explicit component, not an accident. | CodeGraphContext |
| R8 | **Probabilistic-OR fusion** `1 − ∏(1 − factor·weight)` as an alternative mode: absent signals never dilute present ones. Saturating normalisation `x/(x+half)` for unbounded components. Keep the weighted sum, golden both. | brain0 |
| R9 | **Arithmetic mean, never geometric** — a single zero factor must degrade, not veto; per-factor breakdown attached to every result. (Their code history records abandoning geometric mean for exactly this.) | potpie |
| R10 | **Sentinel-rescue guard** — a component returning "missing/uncomputable" excludes the candidate or forces a floor, independent of how strong other components are. | cognee |
| R11 | **Index-derived clock** — any recency component computes age against the generation's own max observed timestamp, never `Date.now()`. Deterministic by construction, goldens stay byte-stable. Half-life decay `0.5^(age/half_life)` as the shape. | brain0, potpie |
| R12 | **Lane-consensus confidence** — when two lanes independently retrieve a candidate, surface the agreement (Jaccard overlap/vectorOnly/graphOnly) as a confidence component; disagreement flags uncertainty. | codebase-graph |
| R13 | **One raw-score pass, many blend weights** — factor component computation from weight application so weight experiments and per-query weight variants are free. | infigraph |
| R14 | **Clone-canonical demotion** — AST node-kind bag Jaccard (or MinHash/LSH b=32×r=2) clone clusters, rename-invariant and cheap; tag canonical vs member spans so duplicated logic stops inflating impact. Parallel scoring with **sequential canonical admission** keeps it deterministic. | roam-code, codebase-memory-mcp |
| R15 | **Learned ranker overlay** trained from the eval corpus with the explainable components as features — explainability survives because the features *are* the components. Requires C10 metrics first. | roam-code |

### 3.6 Evidence, claims and verification

| # | Mechanism | Source |
|---|---|---|
| V1 | **Citation re-location**: distrust stated line numbers by design. Search the quoted snippet verbatim in the actual file bytes, **overwrite** the stated span with the true one; failure → typed `UNVERIFIED_QUOTE`. Converts hallucinated line numbers into verified ones mechanically. | deepwiki-open |
| V2 | **Resolution-time stale-evidence guard** — hash the emitted span bytes against the stored `contentHash` before serving; mismatch emits `STALE_EVIDENCE` (dropped or downgraded), never silently stale code. Also: verify a claimed span's text actually occurs at its claimed lines before admitting it. | mentat, opengrok |
| V3 | **Evidence-coverage admission gate** — a claim's cited `path:start-end` must match an evidence span whose `contentHash` is present in the referenced generation; violations become `UNSUPPORTED_SPAN`. The evidence-before-claims invariant made executable. | mentat |
| V4 | **Cascaded fuzzy anchor ladder** — exact → case-insensitive → stripped → blank-lines-removed, each stage attempted only on the previous miss; a hit emits the evidence at a degraded confidence tier with an `anchor_relocated` note. Never silently. | mentat |
| V5 | **Grounding cascade with repair suggestions** — extract identifiers from a claim, ground exact → qualified-substring → case-insensitive, classify `Verified | Partial{supported, unsupported, suggestions} | Ungrounded`, and offer Levenshtein-nearest suggestions so ungrounded claims get *fixed* rather than just rejected. Citation strength recorded on the verdict. | agentic-codebase |
| V6 | **Line-marked rendering contract** — emit evidence grouped by file with inline `[lines A-B]` markers, and validate that model-facing consumers cite only inside marked ranges. Turns V3 into a rendering property. | deepwiki-open |
| V7 | **Findings registry** with deterministic ids `hash(kind + subject + content)`, UNIQUE for idempotent upsert, `supersedes_id` chains and a suppressions column. One cross-detector evidence layer for parse errors, stale evidence, drift and doctor issues. | roam-code |
| V8 | **Evidence-keyed delta matching** — base/head matching as a multiset over `sha256(rule \0 message \0 whitespace-normalised evidence)`, matched same-file-stable → same-file-occurrence → cross-file last, so a copy can't consume a reformatted local occurrence. Stronger than line numbers. | react-doctor |
| V9 | **Syntactic id over dedented, annotation-stripped lines** plus an occurrence index for genuine duplicates (kept, not merged). If an id ever spans processes or languages, pin the canonical serialisation byte-exactly on both sides. | semgrep |
| V10 | **Baseline mode** — index the prior generation in a temp worktree, subtract findings by signature, remap across renames so a rename doesn't resurface every finding as new. Plus a **baseline file of accepted violations** so strict rules can be adopted without fixing history first. | semgrep, dependency-cruiser |
| V11 | **Range-set formula algebra over spans** — `Inside | Anywhere | And | Or | Not` evaluated as interval set algebra, giving declarative "this claim holds inside that scope" constraints that compose with existing spans. | opengrep |
| V12 | **Prefilter compilation** — compile a rule/claim's positive conjuncts to a regex/token DNF checked against merkle-hashed file content *before* any AST or graph work. A near-free incremental skip layer. | opengrep |
| V13 | **Claim linting at admission** — reject structurally unreachable or self-contradictory claims before they enter, the way rules are linted before a scan. | semgrep |
| V14 | **Heading-path section identity** for documents (`page#A › B › C` + line span) so doc claims survive edits elsewhere in the document. | codealmanac |
| V15 | **Source-type taxonomy** for claims (file/web/commit/pr/issue/conversation/wiki/manual), each with target-shape validation and degrade-to-body-only rather than fail. | codealmanac |
| V16 | **Alias-normalise then validate** — an alias table (`func→function`, `extends→inherits`) repairs well-intentioned type drift at the claim boundary before validation, with a typed receipt recording what was rewritten. | Understand-Anything |
| V17 | **Filesystem-verified dead references** — every doc/claim file ref resolved against the working tree; missing targets become typed `dead_ref` findings with counts. | codealmanac |
| V18 | **Hash-chained verdict history** where the chain covers only immutable fields — mutable annotations stay outside the hash so post-hoc merging can't break integrity. | semantica |

### 3.7 Tool surface and output shaping

| # | Mechanism | Source |
|---|---|---|
| T1 | **Compact aggregate facade** — collapse a large tool list into ~10 aggregate tools taking an `operation` parameter, expanded internally, with granular tools still available. Large tool lists degrade agent routing and burn schema tokens. Pairs with **profile-gated registration** (core/expanded) and progressive disclosure. | agentic-codebase, roam-code |
| T2 | **Steering tool descriptions** — embed token-cost estimates, "prefer X over Y" routing guidance, structured error codes with `retryable`, "Next:" follow-ups, and explicit trap warnings (decorator/receiver gotchas). Descriptions are agent-facing UX, not documentation. | code-compress, octocode |
| T3 | **Session hints block** — `next_steps` from a static workflow graph with already-called tools suppressed, `related`, `warnings`, ≤3 each, embedded in every response. Reduces round-trips at near-zero cost. | code-review-graph |
| T4 | **Output compression layer** — per-tool compressors with a panic-safe raw fallback, a bypass list, and cross-call dedup of content already shown this session. Complements token *accounting* by shrinking the payload itself. | infigraph |
| T5 | **Wire-encoding token counting** — trim least-relevant-first against the tokens of the *actual* wire encoding, suppress rather than truncate partial lists, ring pagination with a retained-index fingerprint. | sense |
| T6 | **Dual-resolution envelopes** — `detail_level="minimal"` returns risk/counts/top-N with a pre-rendered summary; full mode returns everything. | code-review-graph |
| T7 | **Symbol-fetch ambiguity protocol** — ambiguous name → `{status:error, candidates:[…]}`; unknown → available list; oversized body → capped with drill-down guidance by line number. Reject ambiguous query shapes with generated suggestions instead of best-effort misinterpretation. | code-index-mcp (johnhuang), code-compress |
| T8 | **Output-boundary sanitisation** — strip control characters and truncate every code-derived string entering a response; validate git refs by regex; confine repo roots. Cortex treats repo content as untrusted at ingest; the *output* boundary needs the same gate. | code-review-graph |
| T9 | **Path sanitisation on export** — inside root → relative, outside root → basename only; a test asserting no absolute path ever leaks into an artifact. | Understand-Anything |
| T10 | **Per-segment budget utilisation warnings** (`OVER BUDGET`, `92% of budget`) in the response envelope, and **savings attribution by mechanism** (truncation vs summary vs drop) in telemetry. | praisonai |
| T11 | **Never-orphan rule** — if a span is admitted, its binding claim context travels with it or is explicitly elided with a marker. | praisonai |
| T12 | **Cost-visible search UX** — per-candidate token cost and a running cumulative total next to every candidate, with `whyIncluded` components inline, so the caller can see exactly which candidate broke the ceiling. | mentat |
| T13 | **Enclosing-symbol breadcrumbs** (`[impl Store › fn flush]`) standard on every match. | octocode |
| T14 | **Consolidated interval reference grammar** — `path:12-34,50-60` as the universal reference format with parse/format/contains/intersects/merge algebra and a whole-file sentinel that supersedes intervals. | mentat |
| T15 | **Host-aware citation anchors** (GitHub `#L1-L5`, GitLab `#L1-5`, Bitbucket `#lines-1:5`) and source lists rebuilt from authoritative data, never trusted from the producer. | deepwiki-open |
| T16 | **Tool-surface drift lock** — a test asserting the live MCP registry equals the exact expected set. | repo-graph |
| T17 | **MCP↔CLI parity suites** and per-command empty-corpus sweeps. | roam-code |
| T18 | **JSON-mode discipline** — progress output never interleaves with machine-parseable stdout. | repo-lens |

### 3.8 Operational hardening

| # | Mechanism | Source |
|---|---|---|
| O1 | **Lock-vs-corruption discrimination before destructive recovery** — classify store-open failures (lock contention / transient read / genuine corruption), graduated backoff, and a hard rule: **never wipe before the retry budget is exhausted**. Their regression tests exist because transient mid-checkpoint read failures once caused data-loss wipes. | infigraph |
| O2 | **Crash-isolated provider execution** — fork/exec'd worker contains SIGSEGV and hangs and returns RSS to the OS; a crash synthesises a typed `PROVIDER_CRASH` finding with the affected file set and marks those files incomplete, so a targeted retry can fix exactly the broken subset. | codebase-memory-mcp, Sourcetrail |
| O3 | **Restart-and-retry-once wrapper** around provider calls, serialised so the restart cannot race an in-flight request; plus **retryable-race error classification** (LSP `-32801 ContentModified` is a race, not a failure) to cut incremental-build flakiness. | serena |
| O4 | **Degrading cache ladder** as a named contract — cache error → recreate once → in-memory fallback → typed `CACHE_DEGRADED` finding. Correctness never depends on the cache. | aider |
| O5 | **Dynamic, size-scaled timeouts** — per-file timeout scaled by size; parallel-build timeout `0.5s/file` clamped to `[30s, 600s]`; on expiry cancel pending work and report `status:"partial"` with counts. Never silent loss. | opengrep, code-index-mcp (johnhuang) |
| O6 | **Wall-clock budgets on batches** with partial-result contracts (a prefix plus a continuation signal), and **two-phase per-file budgets** so a pathological parse can't starve the extractor. | serena, semantic |
| O7 | **`capToDeadline`** — each phase's budget capped at `min(phase budget, remaining global deadline)`; budget *splitting* for overlapped phases so two CPU pools sum to cores rather than doubling. | react-doctor |
| O8 | **Deterministic LPT batch planning** (largest-first → least-loaded) with documented tie-breaks and a spawn-args char budget. Their measured finding: **don't key batching on worker count** — contended subprocess cold starts regress it. | react-doctor |
| O9 | **Pipelined build** — parse batch N while upserting batch N−1 with a bounded pending-upsert promise and per-file fallback on batch failure. | codebase-graph |
| O10 | **Store-backed cancellation** — a cancel flag in the store (not process memory) polled at batch boundaries, bridged to an AbortSignal, so a stale build can be killed from another process and survives restarts. Wire tree-sitter's cancellation flag into it. | context8, tree-sitter |
| O11 | **Abort registry with await-before-drop** — signal all in-flight operations, await settlement, and surface aborts as a typed error distinct from failures. | claude-context |
| O12 | **Stale-RUNNING recovery** — on startup, reclaim work markers older than a threshold; keep task dependencies in the store, not in process state. | context8 |
| O13 | **FIFO admission limiter** for concurrent MCP operations with a structured `queue_timeout` error instead of a crash or a silent drop. | code-index-mcp (johnhuang) |
| O14 | **Batch count-alignment assertion** — any batch RPC must return exactly as many responses as requests, or the batch hard-fails. Silent misalignment corrupts joins invisibly. | claude-context |
| O15 | **Heartbeats on long streams** (proxies kill idle connections) and TTL eviction of terminal task records so registries don't grow unbounded. | deepwiki-open |
| O16 | **Batched memory release** — process independent units in batches with an explicit cache/memory release between them; default GC/heap behaviour is not a memory budget. | lsif-go |
| O17 | **Schema-version gate with auto-recreate**, plus a build/read pragma split (WAL always; `synchronous=NORMAL` during build, `FULL` for reads; `temp_store=MEMORY`; final `PRAGMA optimize`). | code-index-mcp (johnhuang) |
| O18 | **HTTP surface checklist** — loopback bind, one-time token, path confinement, size cap, known-file allowlist. Audit `lib/http-server.mjs` against it. | Understand-Anything |
| O19 | **Test-mode network tripwire** — a guard that *hard-fails* any network/embedding path invoked under a test sentinel, with explicit opt-in for network-disabled qualification gates. Turns the local-only doctrine into a test failure. | mentat |
| O20 | **Redaction as a pipeline stage** preceding every store write (not a post-hoc filter), byte-offset cursors for append-only sources, and content-hash-keyed derived caches. Ingest doctrine, ready before ingest exists. | brain0 |

### 3.9 Discovery and ingestion

| # | Mechanism | Source |
|---|---|---|
| D1 | **VCS-first discovery** — `git ls-files -c -o --exclude-standard` as the authoritative file list when a git root exists, with nested-repo recursion, a symlink `visited` guard, and `--directory` for untracked enumeration so monster untracked trees stay fast. A filesystem walk plus `.gitignore` semantics drifts on negations, nested repos and sparse checkouts. | mentat |
| D2 | **Single filter entry point** consumed by every surface (build, display, doctor), asserted rather than assumed. | deepwiki-open |
| D3 | **`.noindex` sentinel files** pruning a subtree, memoised per directory. | octocode |
| D4 | **NUL-byte binary probe** (first 8KB) plus a documented **lightweight mode** for oversize files (parse the first N lines) instead of failing or blowing the budget, recorded as a typed finding. | code-index-mcp (johnhuang) |
| D5 | **Monorepo-shallow manifest discovery** (depth ≤ 2) with per-group source attribution, avoiding vendored trees. | repo-lens |
| D6 | **Bounded-depth ancestor cycle guard** on hierarchy-edge insertion — a write-time typed validation error instead of a read-time traversal hang. | codealmanac |
| D7 | **Block-then-merge dedup** — partition by a cheap key (normalised name + kind), union-find within blocks only, avoiding O(n²) cross-provider symbol comparison. | semantica |

### 3.10 Testing and evaluation

| # | Mechanism | Source |
|---|---|---|
| X1 | **Incremental ≡ full-rebuild equivalence test** on the same corpus. The strongest possible incrementality proof; requires seeded RNG everywhere (C9). | GitNexus |
| X2 | **Commutativity test** — two random file orders produce byte-identical generations. | semantic |
| X3 | **Round-trip id stability** — build → export → rebuild-from-export yields identical evidence keys. | oxc |
| X4 | **Frozen eval with corpus checksums** — sha256 over the eval corpus, `HoldoutViolationError` on tamper, and provenance verification tying results back to the collection actually indexed. | Code-Index-MCP |
| X5 | **Graded ranking metrics** — nDCG@k, MRR, recall@k, percentiles; set precision/recall at node and span level with `allowedAlternates` counted as non-penalty true positives; measured **after** every post-pass. | Code-Index-MCP, mentat, llama_index |
| X6 | **A/B agent-efficiency harness** — same task, no-MCP arm vs Cortex-MCP arm, pinned model, fresh clone, isolated settings, optional grep/glob disabling to isolate graph value; median + IQR; checkpoint/resume; rate-limit backoff. | repo-graph, claude-context |
| X7 | **Strict grader rules** — line-number-free answer keys, binding requires *both* a directory-qualified path and a distinctive token, and citing a dead location as live costs precision unless a dead-marker word appears on the same line. Gate: recall ≥0.80 ∧ precision ≥0.90. | repo-graph |
| X8 | **Strategy-matrix harness** — N strategies × M fixtures → pass/wrong/fail table, runnable offline, diffable in CI. Build it *before* the fuzzy-anchor work (V4). | aider |
| X9 | **Whole-graph golden snapshots**, deterministically ordered, per fixture repo — any extraction regression anywhere moves the golden. | tree-sitter-graph |
| X10 | **Inline annotation fixtures** (`ruleid:`/`ok:` style) as cheap spot checks living next to the code, complementing the JSONL corpus. Plus **named failing fixtures** documenting known limitations, so a known-bad behaviour changing is also caught. | opengrep, semantic |
| X11 | **Verdict-preserving fuzz** — semantics-identical rewrites (parens, cast wrappers, concise→block arrows, no-op prologues) must preserve emitted graph facts. Seeded RNG. | react-doctor |
| X12 | **Empty-corpus sweeps** per command, envelope schema-drift tests, and pinned upstream commit SHAs in fixture metadata so "did the corpus move?" is a header check. | roam-code, oxc |
| X13 | **Adapter interface** so the qualification corpus can be run against competitor systems under one harness. | codebase-graph |
| X14 | **`EXPLAIN QUERY PLAN` pinning tests** for hot traversals, and hyperfine-style perf gates on hot commands. | sense, dependency-cruiser |
| X15 | **Capturing-writer E2E seam** — assert on emitted bytes in memory without touching disk. | lsif-go |

---

## 4. The ranked program

Top 25 by (leverage × confidence) ÷ effort, drawing from §2 and §3. Effort: S ≤ 1 day, M ≤ 1 week,
L > 1 week.

| Rank | Item | Ref | Effort | Why |
|---|---|---|---|---|
| 1 | Honor SCIP `position_encoding` | E1 | S | Latent silent-corruption bug on non-ASCII source. Fix before anything else. |
| 2 | Mine SCIP roles + relationships + kinds + enclosing_range | E2 | M | Precision-tier IMPLEMENTS/TYPE_DEF/IMPORT/read-write edges already sitting in files Cortex reads. Largest capability gain per line. |
| 3 | Per-edge resolution strategy + confidence + AMBIGUOUS label | C1 | M | Six-repo convergence; unlocks strict-vs-permissive traversal and honest impact. |
| 4 | Epistemic + partial/truncated + readiness response envelope | C2 | M | Nine-repo convergence; one shape across every surface; prerequisite for honest impact claims. |
| 5 | Coverage honesty: skip accounting + queryable missed-graph + blind-spots footer | C7 | M | Nine-repo convergence; "no false clean" made mechanical. |
| 6 | Adaptive-damping PageRank with guaranteed termination | R1 | S | Direct, low-risk quality upgrade to the existing neighbourhood core. |
| 7 | String/comment masking + import-backing for lexical edges | E12 | S | Kills a whole false-positive class for a day's work. |
| 8 | Graded eval metrics + frozen-corpus checksums | X4, X5 | M | Every later ranking change becomes measurable instead of anecdotal. Gate for R15. |
| 9 | Resolution-time stale-evidence guard | V2 | S | Small, pure, protects every query; makes dirty-tree queries honest without a rebuild. |
| 10 | Citation re-location validator | V1 | M | Turns externally-produced spans from trusted into verified. |
| 11 | Diff-range → span scoping + `ChangeTarget` primitive | Q1 | M | Upgrades existing `diff_impact` evals into a real product surface. |
| 12 | Assemble-to-budget entry point with typed omission receipts | C11 | M | Seven-repo convergence; the missing "one call, packed context" surface. |
| 13 | Route-derived `definition_id` alongside `contentHash` | C6 | M | Identity continuity across body edits; prerequisite for rename resolution. |
| 14 | Rename/move resolution pass emitting typed edges | C6 | M | Depends on 13. Biggest single freshness win in the corpus. |
| 15 | Parse-health bits + guarded extraction fallback ladder | C8 | S–M | Three parser-grade repos converge on the contract; removes the exception path. |
| 16 | Determinism checklist + commutativity + incremental≡full tests | C9, X1, X2 | M | Eleven-repo convergence; the incremental≡full test is the strongest correctness proof available. |
| 17 | Query-shape classifier steering weights | C4 | S | ~200 lines, deterministic, no model, immediate ranking quality. |
| 18 | `locate path:line` narrowest-span query | Q3 | S | Tiny surface, outsized agent UX value. |
| 19 | Version-stamped cache keys + never-cache-empty + schema revalidate | C13 | S–M | Nine-repo convergence; prevents the classic poisoned/stale-cache failures. |
| 20 | Liveness (entry-reachability) + edge provenance on impact rows | Q4, Q5 | M | Stops overstated impact; explains every row. |
| 21 | Header-aware invalidation (`changed ∪ referencing(changed)`) | I1 | M | Highest-value invalidation policy; I2 is the refined follow-on. |
| 22 | Co-change coupling edges + ranking component | C3 | M | Six-repo convergence; an edge kind static analysis cannot produce. |
| 23 | Cosmetic-vs-structural change classification + update-scope matrix | I4, I5 | M | Stops comment edits costing full re-extraction; gates expensive passes by magnitude. |
| 24 | Findings registry with deterministic ids + supersession | V7, C14 | M | Unifies parse errors, drift, stale evidence and doctor issues into one auditable layer. |
| 25 | A/B agent-efficiency harness | X6 | M–L | The metric that actually sells the tool; do it after 8. |

---

## 5. Sequencing

**Wave 0 — correctness fixes and free wins (days).**
E1 (position_encoding), E12 (masking), R1 (adaptive damping), V2 (stale-evidence guard),
Q3 (`locate`), R11 (index-derived clock), S3/S4 (path- and locale-independent ids),
O4 (degrading cache contract), T16 (tool-surface lock), C9 checklist audit.
*Rationale: each is self-contained, none depends on the others, several are latent bugs.*

**Wave 1 — the honesty layer (1–2 weeks).**
C1 (per-edge confidence) → C2 (response envelope) → C7 (coverage) → C8 (parse health) →
Q5 (edge provenance) → Q4 (liveness).
*Rationale: these compose into one coherent story — every answer states how it was derived, what
it missed, and whether it is exact or a floor. This is the differentiator competitors are groping
toward; GitNexus alone has the full form.*

**Wave 2 — measurement (1–2 weeks, parallelisable with Wave 1).**
X4 (frozen checksums) → X5 (graded metrics) → X2/X1 (commutativity, incremental≡full) →
X9 (whole-graph goldens) → X7 (strict grader).
*Rationale: Wave 3 is a ranking and packing programme. Do not start it without numbers.*

**Wave 3 — retrieval and packing (2–4 weeks).**
C11 (assembler) with C12 (skeleton/outline tiers), T14 (interval refs), Q19 (auto-merge/dedup),
C4 (query shape), R2 (seed inference), R3/R4/R5 (component expansion), C5 (RRF where lanes are
independent), T12 (cost-visible output), T5 (wire-encoding trim).
*Rationale: this is where agent-efficiency numbers move, and Wave 2 makes the movement visible.*

**Wave 4 — identity and incrementality (3–6 weeks).**
C6 (definition_id → rename resolution) → I1 → I2 (closure repair) → I3 (parse≠write scope) →
I4/I5 (change classification and scope matrix) → E4 (changed-ranges) → I13 (watcher hardening) →
V7/C14 (findings registry, supersession).
*Rationale: the most valuable and most dangerous cluster. Gate every step on the Wave 2
incremental≡full equivalence test.*

**Wave 5 — products on top of the graph.**
Q1/Q2 (diff and failure-signal targets), Q7/Q8 (test selection and ranking), Q9 (dead code),
Q13 (flows), Q14/Q15 (semantic and signature diff), Q11 (cycles, instability), Q12 (communities),
X6 (A/B harness), T1/T2/T3 (tool surface economics).

**Wave 6 — architectural bet, decide separately.**
E6→E10 and tree-sitter-graph's stanza model: the migration from imperative
`generic-ast-walker.mjs` + per-language tables toward declarative pattern+action extraction with
data-driven scope inheritance. Items E6 (merged query), E7 (confluence) and E9 (static checker) are
adoptable *inside* the current architecture and should be taken regardless; the full DSL migration
is a strategic decision, not a backlog item.

---

## 6. Do-not-absorb register

Recorded so the same evaluation isn't redone, and so several can become regression-test motivations.

**Storage and identity**
- FalkorDB/Kùzu/Neo4j/LanceDB/Milvus/LadybugDB backends and Cypher passthrough — SQLite + ledger is
  the stronger local-first bet (codebase-graph, infigraph, octocode, claude-context, GitNexus).
- Fixed-record `.acb` binary format as a primary store (agentic-codebase) — fine as an *export*
  idea only.
- Identity including line numbers (codebase-graph `Label:path:name:line`) or path+lines in content
  hashes (octocode) — shifts on edit, breaks move stability.
- UUID chunk ids (context8), autoincrement ids unstable across rebuilds (Sourcetrail),
  serialized-name-string symbol identity (Sourcetrail), `(name, path, line)` tuples
  (CodeGraphContext) — all collide or drift.
- JSON-file storage (Understand-Anything, RepoDoctor).

**Freshness**
- mtime-only or mtime-first staleness as the *primary* signal (Sourcetrail, roam-code, repo-graph)
  — acceptable only as a fast path with hash confirmation (I16).
- FS-event-driven incrementality without content hashing (CodeGraphContext).
- "Incremental" paths that rebuild fully under the hood (agentic-codebase).

**Retrieval**
- Keyword search presented as vector search (agentic-codebase); random-embedding stubs (context8).
- BFS/graph-cut context selectors (Code-Index-MCP) and in-memory TF-IDF rebuilt per query
  (roam-code).
- Char-truncation token accounting and `chars/4` estimators as the *primary* budget mechanism
  (ai-code-audit, CodeGraphContext, Code-Index-MCP) — Cortex's two-budget system is strictly
  stronger. Keep `chars/4` as the documented floor never to regress to.
- Substring dependency detection as graph truth (repo-lens); static relevance constants replacing
  explainable components (CodeGraphContext).

**Process**
- LLM-delegated "analysis" with no static core (RepoDoctor, ai-code-audit); LLM-driven extraction
  and curation stages (cognee, semantica) — outside the deterministic scope.
- Whole-repo dump into a single QA prompt (ai-code-audit) — blows context on any real repo.
- Chunks identified only by list index, so no incremental reuse is possible (ai-code-audit).
- Sequential loops labelled "parallel" (ai-code-audit); dead configuration shipped in public option
  surfaces (repo-lens `AnalyzeOptions.depth`); swallow-IO-without-recording error posture
  (repo-lens).
- 100K+ LOC monolith and command-sprawl structures (Code-Index-MCP, roam-code, PraisonAI).

**Two worth encoding as tests rather than notes**
- *No-op touch produces zero re-index work* (motivated by Sourcetrail's `filecontent` fallback).
- *Every ordered emission declares an explicit tiebreak; none relies on map-iteration order*
  (motivated by agentic-codebase and octocode both hitting hash-order nondeterminism).

---

## 7. Parked contingencies

Shapes decided now, cheap to implement when the lane exists. Deciding these in advance is most of
the value — several repos paid for the lesson through a migration incident.

**If a vector/semantic lane lands**
- Vectors in the same SQLite file (`sqlite-vec vec0`), never an external service (semantica).
- Embedding text = graph-context-enriched snippet, not raw code; model-ID watermark in the ledger
  forces re-embedding on model or context change (sense).
- Key vectors on evidence-text hashes so rebuilds never re-embed unchanged text; snapshot embedding
  hashes before a force-reindex so vectors survive the wipe (codebase-graph).
- Brute force below ~200K vectors, HNSW above; `.meta` sidecar validating count/dim/mtime
  (infigraph); size-by-cardinality with growth-triggered rebuild (octocode).
- **On load, quarantine mismatched dimensions rather than mixing them** — keep the majority
  dimension, drop the rest, typed finding + rebuild path. Cheaper to decide now than after a
  migration incident (deepwiki-open).
- Random Indexing (TF-IDF-weighted, int8-quantised, in-DB) is the model-free deterministic option
  if a semantic lane is ever wanted without a model dependency (codebase-memory-mcp).

**If an FTS lane lands**
- Store-resident, written in the same transaction as graph writes so it cannot drift from the
  ledger (GitNexus).
- camelCase splitting at index time, porter tokenizer, structural-label boosting (Function/Method
  +10, Route +8, Class +5), NL-query sanitisation, zero-result `*term*` fallback, injection-safe
  MATCH building (code-compress, codebase-memory-mcp).
- Field-weighted BM25 (title 5.0 / heading 3.0 / body 1.0) with the weights frozen as goldens
  (codealmanac).
- Lazy index creation with explicit handle invalidation — cached handles hold stale snapshots
  (octocode).

**If any LLM-assisted surface lands**
- Salvage ladder: strict parse → truncation salvage from open tag → entity cleanup → regex fallback
  → typed `PARSE_DEGRADED` receipt. Never crash a pipeline on a truncated response (deepwiki-open).
- Balanced-brace JSON extraction with a proper in-string/escape state machine, minimal repair
  (trailing commas), bounded retry (3×) with a fresh stream each attempt (deepwiki-open); or the
  fence-regex → largest-blob → raw ladder plus a one-shot format-repair retry (RepoDoctor).
- Constrained enum structured output with an explicit safe default on parse failure
  (ai-code-audit).
- Verified skeleton + optional enrichment: the graph-and-evidence skeleton always completes;
  enrichment failure emits `ENRICHMENT_DEGRADED` and never invalidates the skeleton
  (deepwiki-open).
- Prompt assets versioned by directory with the expected schema embedded in the template
  (RepoDoctor).

**If statement-level analysis lands**
- Opt-in tier with explicit budgets (`INTERPROC_DEPTH_BUDGET=3`, node budgets), dual interchangeable
  solvers (SSA and GEN/KILL worklist) held equivalent by fuzz testing, anchored consumers separate
  from impact traversal (GitNexus).
- Expensive data-dependence facts materialised as build-time edges with bailout thresholds, not
  computed at query time (joern).
- Demand-driven backward search with a fingerprint-keyed path cache, bounded call depth, and
  held-task completion for cycles (joern).
- Sources/sinks/sanitisers as composable formula predicates, never hardcoded lists (opengrep);
  per-symbol flow semantics as data (joern).

**If cross-repo or remote ingestion lands**
- Typed contract edges with match-cascade provenance (exact → BM25 → embedding), cross-boundary
  fan-out capped with `riskEpistemic: 'lower-bound'`. **Never merge node spaces** — this is the
  sanctioned form of the existing independent-scoping doctrine (GitNexus).
- Blobless clone + `git ls-tree -z` content-addressed listing (potpie); shallow clone into a temp
  dir with guaranteed `finally` cleanup and optional metadata enrichment that never fails the run
  (repo-lens).
- Committable/importable generation snapshot artifacts with integrity checks and a three-tier load
  order (fresh snapshot → incremental → full rebuild) (repo-graph).

**If session/transcript evidence enters scope**
- cwd-matched discovery + `active_since` watermark + shape-tolerant multi-format normalisation
  (codealmanac); byte-offset cursors, content-hash-keyed turn caches, and redaction before
  persistence/summarisation/embedding (brain0).

**Distribution idea, not engine code**
- A pre-tool-use hook intercepting exploration-shaped Agent/Bash/Grep/Glob calls and serving index
  answers instead of grep, plus a session-start freshness probe published in the plugin manifest
  (sense, Understand-Anything).

---

## 8. Corpus coverage

Depth verdicts as reported by each study, to calibrate how much weight to give each source.

**Deep, engineering-dense (take seriously):** codebase-memory-mcp, GitNexus, joern, sourcetrail,
opengrok, opengrep, semgrep, oxc, tree-sitter, tree-sitter-graph, scip, dependency-cruiser,
react-doctor, sense, treesitter-chunker, roam-code, octocode, infigraph, brain0, aider,
codebase-graph, serena, lsif-go, semantic, Code-Index-MCP.

**Mixed — strong in parts, weak in others:** claude-context (discipline yes, chunker no), cognee
(identity yes, pipeline no), llama_index (retrieval yes, everything else irrelevant),
CodeGraphContext (resolution yes, identity no), code-compress (packing yes, dep graph dead),
code-review-graph (review layer yes, storage no), potpie, codealmanac, mentat, deepwiki-open,
code-index-mcp/johnhuang (operational layer only), Understand-Anything, context8, repo-graph
(closed engine; API + eval methodology only).

**Shallow — narrow or negative value:** ai-code-audit (explicitly the weakest: no parsing, indexing,
storage, retrieval or determinism), RepoDoctor (LLM-delegated, no static analysis), repo-lens
(no AST/graph/storage), PraisonAI (context-budget subsystem only), semantica (out of domain),
agentic-codebase (MCP economics only; storage claims overstated).

**Unstudied in this pass (10):** ast-grep, gritql, codegraph-ai/CodeGraph, colbymchenry/codegraph,
rag-rat, contextplus, codeql, stack-graphs, axon, signum. Two of these — `codeql` and
`stack-graphs` — are likely the highest-value remaining reads in the corpus: stack-graphs is the
incremental name-resolution design Cortex's cross-file tiers are approximating, and codeql is the
reference for queries-as-versioned-artifacts.

---

## 9. What the corpus says Cortex is already winning

Recorded because it constrains scope: these came back as confirmations from multiple studies, and
work here is audit, not construction.

- **Storage and identity.** Content-hashed evidence spans + merkle ledger + transactional
  generations were rated stronger than every competitor's equivalent, including the C, Rust and
  Java engines. Repeated verdict: "their storage is weaker; take the policy, not the store."
- **Token accounting.** The two-budget (slice-time / resolve-time) system was rated superior to
  every competitor's `chars/3.5`, `chars/4` or char-truncation scheme. What Cortex lacks is not
  accounting but *shaping* (C11, T4, T5).
- **Explainable ranking components.** Cortex's named-component output is the shape competitors are
  converging toward. The gap is signal **diversity** (R4, R14, C3), not architecture.
- **Precision tiers and determinism doctrine.** Locked invariants already encode what several repos
  learned the hard way. The gap is *granularity* (per-edge, C1) and *enumeration* (the C9
  checklist, X1/X2 tests).
- **Local-first, no external services.** Repeatedly validated against Milvus/Neo4j/LanceDB-dependent
  competitors.

The through-line of all 45 studies: **Cortex's engine is ahead; its honesty layer, its packing
layer, and its measurement layer are behind.** Waves 1–3 exist to close exactly that.
