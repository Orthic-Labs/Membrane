# Competitor adoption synthesis — DSv4-Pro pass

One document consolidating the DSv4-Pro model pass — 55 per-repo ledgers, 10 adoptions each —
into a deduplicated, sequenced program for Cortex.

- **Input**: 55 study files (~550 proposals). No summary README; each file stands alone.
- **Corpus**: complete — all 55 repositories in the competitor index.
- **Companions**: `synthesis-qwen.md` (mechanism pass, 45/55) and `synthesis-m3.md` (architecture
  pass, 55/55). §2 explains what separates this one; §9 lists the conflicts across all three.

Nothing here is implemented. Claims about competitor internals are as reported and have not been
re-verified against the vendored repos.

---

## 1. How to read this

| Bucket | Count | Meaning |
|---|---|---|
| **Convergences** | 11 clusters | ≥3 repos proposing the same move. |
| **Distinct adoptions** | ~75 | Single-source, worth taking. §4, by subsystem. |
| **Seam-only proposals** | ~180 | "Repo X has a module for Y; Cortex's seam is Z." True but not actionable without a design decision. Compressed into §5. |
| **Forward/contingent** | ~60 | Depends on lanes Cortex doesn't have. §7. |
| **Low value or void** | ~20 | Build-system cargo-culting, plus two void entries. §10. |

---

## 2. What kind of pass this is

**This is a seam-mapping pass.** Every one of the ~550 proposals ends with a `*Seam:*` line naming
a concrete Cortex file. That is its defining strength and its defining weakness.

**Strength — it knows the codebase.** The named seams are real and specific: `graph/merkle-ledger.mjs`,
`lib/admission.mjs`, `watchman/repo-actor.mjs`, `graph/scip-provider.mjs`, `lib/evidence-pack.mjs`,
`graph/precision-tiers.mjs`, `lib/orientation-evidence.mjs`, `graph/delta-store.mjs`,
`lib/token-budget.mjs` (with `chunkBySyntax` named), `graph/language-tables/`. Where the M3 pass
proposed inventing `lib/ports/` and `lib/freshen/`, this pass points at files that exist. Every
adoption arrives pre-routed, which makes the corpus directly schedulable in a way the other two
are not.

**Weakness — breadth over depth.** Roughly a third of the items are of the form "repo X has a
`clustering/` module; Cortex's seam is `graph/analytics/index.mjs`". That identifies a landing spot
without saying what to land. Compare its Sourcetrail entry (ten module names, mapped) to the Qwen
pass on the same repo (the subtract-shared partial-clearing algorithm, spelled out). Both are
useful; only one is implementable as written.

**Not an independent pass.** Several entries cite "Cortex's own M3 ledger" as prior art
(CodeGraphContext #1, codebase-graph #1). It ran with the M3 output in view, so agreement between
this pass and M3 is *not* independent corroboration. Where all three passes agree — and Qwen was
independent — the signal is real.

**Reliability caveats:**

1. **Two repo identifications are inherited and wrong.** `semantic/` is reported as a Rails-era web
   app and `semantica` as a non-existent org, both explicitly citing the M3 note. The Qwen pass,
   reading the vendored `repos/` copies, has them as GitHub's archived Haskell tree-sitter analysis
   library and a ~178K-LOC Python KG framework. **Prefer Qwen; void both entries here.** Notably,
   this pass reports attempting a fresh clone of `semantica` rather than reading `repos/semantica/`
   — the same methodological slip as M3, which is why the error propagated.
2. **Infrastructure filler.** Nix flakes, devcontainers, Bazel hermeticity, goreleaser, Docker
   matrices and changelog tooling recur across ~15 entries with little bearing on Cortex's actual
   problems. Collected and demoted in §10 rather than scattered through the ranking.
3. **A few items assert Cortex lacks something it may have** — output truncation, atomic writes,
   git-absence handling, per-command schemas. Marked *verify-first*.

---

## 3. The 11 convergences

### C1. A canonical span/interval type, used everywhere
*Mentat (`Interval` with `contains`/`intersects`, the `path:start-end[,a-b]` string grammar,
`CodeFeature` whose `str()` **is** the canonical ref, deduplicated by path+interval), oxc
(`oxc_span` as a first-class crate with labels and source-type awareness), ast-grep (`Position`
carrying line, byte-column and byte-offset), treesitter-chunker (`normalize_boundary_path` —
one idempotent POSIX-ification used for **every** identity and serialization), stack-graphs
(`lsp-positions` as a separate crate), claude-context (AST splitter at syntax boundaries).*

Six repos treat "a location in source" as a type with an algebra, not an ad-hoc pair of integers.
The pass's reading of Cortex is that spans appear in `sourceRef`, evidence, truncation receipts and
protected anchors, each constructed independently.

**Adopt**: one `Interval`/`Span` type with a single string grammar, merge/intersect/contains
operations, a whole-file sentinel, and one path-normalisation function that every identity and
serialization path funnels through. The last part matters most — it is what makes graph identities
stable across operating systems.

### C2. Freshness verdicts with reasons, plus read-repair
*Code-Index-MCP (`FreshnessVerdict` = `FRESH | STALE_COMMIT | STALE_AGE | INVALID`, verified by
**commit ancestry** via `git merge-base --is-ancestor` *and* artifact age — two signals, one typed
verdict), sense (`staleSnapshot` per-query sweep that **inline re-indexes stale paths** so an
edit-then-immediately-query is honest before the debounced watcher fires; plus a freshness footer
on every response), repo-graph (refresh-on-connect so a stale graph self-heals), claude-context
(Merkle-based synchronizer), Understand-Anything (post-commit git-event hook as a freshness
trigger, complementing file watching).*

The standout is sense's **read-repair**: freshness as a query-time repair action rather than only a
reported state. Combined with the four-value verdict and a footer on every response, this is the
most complete freshness story across all three passes.

**Adopt**: the two-signal verdict enum with reasons; read-repair on query for a bounded set of
stale paths; a freshness footer on every response; git-event hooks alongside file watching.

### C3. Ports over the store, one interface, in-memory double
*CodeGraphContext (five backends behind one manager, plus embedded-vs-remote variants of the same
backend), codebase-graph (driver + registry + drivers/), CodeGraph (rocksdb / memory / namespaced),
Brain0 (a `Storage` trait with SQLite and Postgres dialects), octocode (`Store` abstraction),
axon (`storage/base.py`), llama_index (per-backend packages under one contract), code-compress
(`ISymbolStore` injected), Codealmanac (ports per concern).*

Nine repos, same as the M3 pass found. **The same caution applies**: the Qwen pass read those
backends and rated every one weaker than Cortex's SQLite. Take the seam and the in-memory double
(Sourcetrail's `TestStorage`, CodeGraph's `in_memory()`); do not ship a second production store.

One item here is genuinely new and cheap: CodeGraphContext's **thread-safe singleton connection
manager**. The pass notes Cortex opens store handles per-process, and that watchman plus CLI
concurrency is exactly where that bites.

### C4. Benchmark harnesses that measure agent outcomes, not graph facts
*sense (scenarios + `.rubric.yaml` files + judge/score/report scripts + **`PINNED_COMMITS.json`** so
benchmark repos are reproducible + **held-out scenarios**), repo-graph (headless A/B: the same agent
run twice per (repo, task), with and without the graph, reading cost/turns/exploration from the
agent's own JSON output; per-repo-shape configs; answer-key grading and a `diagnose.py`), GitNexus
(SWE-bench-style harness with **three prompt arms** — baseline / native / native-augment — driving
the MCP server through a bridge, plus result analysis), claude-context (an MCP-efficiency analysis
that measures and plots what the server actually saves), juspay (token-efficiency benchmark: graph
query vs naive file reading), colbymchenry (agent-eval A/B reused by the add-a-language workflow),
octocode (a benchmark matrix writing committed `RESULTS.json`), codebase-graph (versioned
`cgbench-v1`), Mentat (typed `BenchmarkResult` + plots).*

Nine repos. Every mechanism needed for a serious eval story appears here: pinned fixture commits,
held-out sets, rubric judging, prompt arms, A/B against a no-graph control, answer keys,
committed versioned results, and diagnosis tooling for failures.

**Adopt**, in order: pin fixture-repo commits (one file, removes silent corpus drift); an A/B
harness measuring turns/cost/tokens with and without Cortex; rubric-based scoring with answer keys;
held-out scenarios; committed versioned results.

### C5. Attestation and audit above content hashes
*Brain0 (Ed25519 in-toto statements where **verify needs only the public key**, so a third party
confirms an attestation without being able to mint one; plus **DLP read-secret tracking** —
`task_versions.read_secrets_json` records `{path, kinds}` for every read whose content held
secrets; plus payload purge/tombstones so sensitive content is crypto-shredded while graph structure
survives), roam-code (SLSA VSA + a **run-ledger-root statement anchored at an HMAC final signature**,
so a verifier trusts the whole chain without replaying every event; status provenance marked
`derived` vs `asserted`), Code-Index-MCP (sign/verify with an explicit **scope preflight** so
attestation fails loudly when credentials lack write scope), rag-rat (papertrail append-only audit;
`PRAGMA synchronous=FULL` raised only during authored writes and restored to `NORMAL` after).*

Four repos, four different layers, all above what content-hash receipts provide. Two items are
distinctive across all three passes: **read-secret tracking** (Cortex redacts on egress but does not
record which reads touched secrets) and the **HMAC-anchored ledger root** (verify the chain without
replaying it).

### C6. Declared ↔ actual reconciliation
*Brain0 (agent declares changes; observer records what actually changed; the reconcile crate
gap-fills and emits a first-class `Drift` signal), potpie (`llm_reconciliation.py` +
`reconciliation_issues.py` + `reconciliation_flags.py` — reconciliation with issues and flags
recorded), contextplus (git shadow state for safe freshness probing without touching real git),
axon (diff via worktrees so the user's HEAD is never switched).*

Same finding as the M3 pass, from the same primary source, with potpie's issue/flag recording as
the addition: reconciliation should produce typed issues, not a boolean.

### C7. Per-language contract with a fallback tier
*Sourcetrail (`IndexerBase` — one base class every language package implements, emitting identical
structures), code-compress (`ILanguageParser`, one parser per language behind one contract),
CodeGraph (`codegraph-parser-api` with IR + entities + relationships + traits, one crate per
language), repo-lens (`IAnalyzer` with a `canAnalyze(files, repoPath)` gate plus a registry),
code-index-mcp/johnhuang (strategy factory **with an explicit fallback strategy**), axon
(`parsers/base.py`), opengrok (ctags fallback for unsupported languages), ast-grep (one language
crate behind one interface).*

Eight repos. The pass's reading is that Cortex has ~40 language tables but no common *extractor
interface* — so adding a language touches several places. Two refinements beyond the interface
itself: repo-lens's `canAnalyze` gate (a language declares whether it can handle this repo, rather
than the registry guessing) and johnhuang's explicit fallback strategy (unsupported languages
degrade to a named tier instead of dropping out).

### C8. Numbered, independently-testable ingestion passes
*axon (eleven numbered phases: walk → structure → parse → imports → calls → heritage → types →
community → process → dead-code → coupling, each its own module, with **phase 0 reserved for
diff**), Understand-Anything (scan → structure → import-map → fingerprints → batches → merge, with
**hierarchical batch→subdomain→whole merging** so rebuilds only recompose affected subgraphs),
repo-lens (scanner → parser → analyzer → output as strict layers), ai-code-audit (five parallel
perspective nodes feeding one report node, orchestrated by a state graph with conditional edges),
context8 (pipeline threading a typed context object), cognee (named pipeline modules).*

Six repos. Two mechanisms stand out beyond the general shape: **hierarchical merge** (recompose only
affected subgraphs, not the whole) and **parallel perspectives** (architecture / security / quality
/ business / modernization as independent lenses over one graph, merged by a typed aggregator).

### C9. Two-tier index: shallow for listing, deep for symbols
*code-index-mcp/johnhuang (shallow JSON index manager + deep SQLite index manager, separate
builders), code-compress (shallow JSON for fast listing, deep store for full records),
contextplus (file-skeleton tool — symbols only, for orientation), ast-grep (a dedicated `outline`
crate extracting a file's skeleton independently of full analysis), code-compress again
(`ProjectOutline` + `ModuleApi` as first-class compression outputs), repo-graph (`_truncate` with
a labelled budget on every tool output).*

Six repos separate "cheap structural view" from "full record". The pass's framing: cold reads and
orientation should never pay the cost of the deep store.

**Adopt**: an outline/skeleton tier answering listing and orientation, with the deep store consulted
only on a hit; labelled truncation on every bounded output.

### C10. Rule severity, baselines and named rule modules
*ast-grep (`Hint | Info | Warning | Error | Off`, with **exit codes driven by severity** — the
process fails only when Error-level rules fire), dependency-cruiser (a **known-violations baseline
file** generated by its own command, so new violations fail CI while legacy ones are grandfathered
and shrinkable; named composable rule modules — no-circular, no-orphans, not-to-unresolvable;
severity-tiered presets: recommended / strict / warn-only), react-doctor (per-rule severity controls
and per-path ignore overrides), codeql (`.ql` query libraries per language), joern (a `querydb`
registry of named reusable queries), semgrep/opengrep (rule templates), roam-code (a detector
catalog where each detector ships with its fix).*

Seven repos. Three composable pieces: a canonical severity ladder, severity-gated CI exit codes, and
a generated baseline that makes strict rules adoptable on a repo with history.

### C11. Multi-surface delivery from one core
*react-doctor (the same analysis ships as an ESLint plugin, an oxlint plugin, a CLI and an API),
ast-grep (napi + wasm + pyo3 + LSP from one core), agentic-codebase (CLI + FFI + WASM + npm + MCP),
gritql (JS and Python SDKs, wasm bindings, LSP), scip (official bindings in six languages),
Understand-Anything / GitNexus / signum / claude-context (multi-host plugin manifests: Claude,
Cursor, Copilot, Codex packaged from one product), codebase-memory-mcp (a nine-target package
matrix), Sourcetrail (nine IDE plugins over one indexer).*

Nine repos. The near-term, low-cost slice is **multi-host plugin packaging** — the same MCP server
declared for several agent hosts — plus emitting a host skill pack from current config. The
bindings matrix (wasm/napi/FFI) is a much larger bet and belongs in §7.

---

## 4. Distinct adoptions

Items marked ★ quote a concrete mechanism; unmarked ones name a seam without specifying the fill.

### 4.1 Correctness and safety

| # | Adoption | Source |
|---|---|---|
| ★S1 | **`require-safe-parse` ESLint rule.** GitNexus forbids direct `parser.parse(content)` calls because the tree-sitter binding **SIGSEGVs on Windows for strings > 32,767 chars**, routing everything through a chunked `parseSourceSafe`. A crash class prevented by a lint rule rather than a code review. Cortex parses with `web-tree-sitter` on every platform — worth verifying whether the same limit applies, and adopting the helper plus rule either way. Highest-value single item in this pass. | GitNexus |
| ★S2 | **Token masking in subprocess error text, including URL-encoded forms.** When a git command fails, the error string has the access token masked in *both* raw and `quote()`-encoded forms before it surfaces. Redaction on egress payloads does not cover error text from subprocesses. | deepwiki-open |
| ★S3 | **Read-only query guard** — strip comments first, then block write keywords (`DELETE/DROP/CREATE/SET/MERGE/...`), so any query surface is provably read-only. | axon |
| ★S4 | **Query sanitizer with a four-step fallback ladder** — agents prepend system prompts to search strings; passthrough → last-question-sentence → last-sentence → truncate, each step warned. A real failure mode for any agent-facing search. | codebase-graph |
| ★S5 | **Root confinement extended to subprocess operations**, not just reads — clone/git operations scoped to a fixed root. *(verify-first)* | deepwiki-open |
| ★S6 | **Atomic writes via temp-file + rename** for every artifact a reader can observe, so a crash never leaves a half-written file. *(verify-first)* | roam-code |
| ★S7 | **`PRAGMA synchronous=FULL` only during authored writes**, restored to `NORMAL` after — durability where it matters without the steady-state cost. | rag-rat |
| ★S8 | **Striped mutex** for concurrent index writes, reducing contention versus one global lock. | lsif-go |
| ★S9 | **Index lock** around the whole index so concurrent readers and writers cannot corrupt state — the pass flags watchman + CLI concurrency as the exposure. | octocode |

### 4.2 Resolution and extraction

| # | Adoption | Source |
|---|---|---|
| ★R1 | **`NORM_VERSION` fingerprint auto-invalidation.** Fingerprints carry a normalisation version; when extraction logic changes shape, old rows are auto-excluded by a version filter and the next reindex produces the new version — **no migration**. The pass calls this one of the two highest-value adoptions in the entire corpus, and the M3 pass ranked it #2 independently. | rag-rat |
| ★R2 | **Oracle as a query-time side table.** SCIP-style resolution written to `edge_oracle` rows *alongside* heuristic edges, never overwriting them, joined by **identifier-token containment rather than line equality**. Registry + spec + join + run as separate concerns. | rag-rat |
| ★R3 | **LSP→SCIP conversion.** Cortex reads repository-supplied SCIP; a converter would let it *produce* SCIP from any LSP-capable language, turning the exact-resolution tier from "if the repo ships an index" into "if a language server exists". | infigraph |
| ★R4 | **`scip lint` before trusting an index** — validate any repository-supplied SCIP export against the schema and rules at ingest, rather than assuming well-formedness. | scip |
| ★R5 | **`scip snapshot` golden testing** — render an index as a canonical text snapshot and diff it, so ingestion changes are visible. | scip |
| ★R6 | **`reprolang`** — a tiny purpose-built language with its own grammar and indexer, used to test the whole pipeline end-to-end. A fixture language is far more controllable than a real repo. | scip |
| ★R7 | **Stack-graphs resolution**: bindings as paths, symbol stack + scope stack, partial paths stitched incrementally across file boundaries, explicit cycle handling. The principled upgrade for the exact-resolution tier. | stack-graphs |
| ★R8 | **`tags`-crate tagging** — tree-sitter's own definition/reference tag extraction as the basis for symbol extraction, rather than a bespoke taxonomy. | tree-sitter |
| ★R9 | **Qualified-name normalisation** as an explicit resolution step. | code-index-mcp |
| ★R10 | **Generated stdlib maps** per language, so stdlib references resolve without heuristics. | lsif-go |
| ★R11 | **Compile-database and package-data caching** across files in one index run. | lsif-go |
| ★R12 | **Kernel↔wasm extraction parity harness** — run both extraction paths over the same files and diff per-file results as canonicalized sets (nodes/edges/refs), with a max-deferral policy and distinct exit codes for parity / diffs / setup error. Required the moment two extraction paths exist. | colbymchenry |
| ★R13 | **Defined add-a-language workflow** as scripts: check grammar → dump AST → wipe and index → verify extraction → A/B retrieval bench. | colbymchenry |

### 4.3 Graph capability

| # | Adoption | Source |
|---|---|---|
| ★G1 | **Effect classification with transitive propagation** — a taxonomy (`pure / reads_db / writes_db / network / filesystem / time / random / mutates_global / cache / queue / logging`) where callers **inherit their callees' effects**. Turns impact from "what is connected" into "what does this change *do*". The most distinctive graph capability in this pass. | roam-code |
| ★G2 | **Change-coupling edges from git history** — `COUPLED_WITH` between files that change together. (Third pass to surface this; a genuine convergence across models.) | axon |
| ★G3 | **Community detection** producing community nodes + membership edges as module boundaries. | axon, juspay, infigraph |
| ★G4 | **Dead-code pass** as a named health signal rather than an ad-hoc query. | axon, RepoDoctor |
| ★G5 | **Graph snapshot diffing for review** — capture node/edge counts, qualified names and community assignments, then diff two snapshots to show *what changed in the graph*, not just which files changed. The natural PR-review surface for a graph tool. | juspay |
| ★G6 | **Typed blast-radius result** with a reachable set and severity, rather than a connectivity list. | code-compress, contextplus |
| ★G7 | **Hot-path detection** — frequently-touched code ranked for map assembly. | code-compress |
| ★G8 | **Per-node complexity and metrics at parse time**, carried on the node rather than computed later. | CodeGraph, treesitter-chunker |
| ★G9 | **Hierarchy cache** for inheritance queries, so path/impact traversals do not re-walk type hierarchies. | Sourcetrail |
| ★G10 | **Cycle-safe traversal** with explicit cycle handling so resolution and impact walks terminate. | stack-graphs |
| ★G11 | **Orphan and circular-dependency detection** as named rules. | dependency-cruiser |
| ★G12 | **Full-text side index** (FTS5) alongside the graph for fuzzy symbol search. | Sourcetrail |
| ★G13 | **Suggester** — symbol and query suggestions over the index vocabulary. | opengrok |
| ★G14 | **Governing docs attached to symbol reads** — when reading a symbol, surface the docs that govern it. | repo-graph |
| ★G15 | **Feature-hub graph with orphan detection** — hierarchical map-of-content organisation, where orphaned files are a doctor finding. | contextplus |
| ★G16 | **Canonical label inference** for nodes, so display names are derived rather than ad hoc. | potpie |
| ★G17 | **Formal graph contract + ontology** defining permitted node/edge kinds, with a **mutation policy** governing what may change. | potpie |
| ★G18 | **Graph-quality metrics** as a scored doctor output. | potpie |

### 4.4 Context assembly

| # | Adoption | Source |
|---|---|---|
| ★C1 | **Auto-context under a token ceiling**, reporting how much of the context was auto-selected versus explicitly requested. | Mentat |
| ★C2 | **Diff-aware context** — bias orientation toward files touched since a reference point, as a first-class context builder rather than a ranking nudge. | Mentat |
| ★C3 | **Ranked repo map** — a token-budgeted whole-repo map with symbols ranked (link-graph/PageRank style) so the map leads with the most important and truncates gracefully; scaled by a multiplier when few files are in play. | Aider |
| ★C4 | **Curated always-include list** — the inverse of an ignore list: files that always appear regardless of ranking. | Aider |
| ★C5 | **`TreeContext`-style rendering** — show a matched symbol inside its enclosing structure rather than as a bare span. | Aider |
| ★C6 | **Labelled output truncation** on every tool response: what was truncated, against what budget. | repo-graph |
| ★C7 | **Compaction rather than truncation** — summarise older context while keeping recent detail. | PraisonAI |
| ★C8 | **`diet` command** — reduce a repo to a minimal working context for a stated task. A named product surface over budget machinery. | RepoDoctor |
| ★C9 | **Cost accounting on the context message** — tokens *and* cost carried on every context payload. | Mentat |
| ★C10 | **Chunking strategy profiles** — several named chunk strategies selected by profile rather than one hard-coded strategy. | treesitter-chunker |

### 4.5 Pipeline, freshness and runtime

| # | Adoption | Source |
|---|---|---|
| ★P1 | **Intermediate storage then merge** — indexing writes to an in-memory intermediate store; results are injected/merged into the persistent store only on success, so a failed build can never corrupt a sealed store. | Sourcetrail |
| ★P2 | **Hash-based changed-path detection** producing changed/deleted sets in deterministic order. | treesitter-chunker |
| ★P3 | **Git-diff-driven incremental re-parse** — changed files plus impacted files only, with a worker cap. | juspay |
| ★P4 | **Merkle-based synchronisation** so only changed subtrees are transferred or recomputed. | claude-context |
| ★P5 | **Delta policy + delta resolver** alongside delta storage — a policy deciding *when a delta is valid* is the missing half of an incremental story. | Code-Index-MCP |
| ★P6 | **Retention policy** for accumulated generations (keep N, or by age). | Code-Index-MCP |
| ★P7 | **Integrity gate** blocking consumption of invalid or untrusted artifacts before orientation. | Code-Index-MCP |
| ★P8 | **Cancellation token** polling a durable store and exposing an `AbortSignal`, with a distinct `TaskCancelled` exception so cancelled ≠ failed. | context8 |
| ★P9 | **Run queue with typed transitions, worker locks and streamed events.** | Codealmanac |
| ★P10 | **Checkpoints** so an interrupted long run resumes rather than restarts. | PraisonAI |
| ★P11 | **Bounded parallel executor** as a named package rather than ad-hoc concurrency. | lsif-go |
| ★P12 | **Interprocess indexer commands** — work split into commands executed across processes with results shared back. | Sourcetrail |
| ★P13 | **Arena allocation** for hot parse paths. | stack-graphs, codebase-memory-mcp |
| ★P14 | **Marker-fenced git-hook installer** — a pre-commit hook that pre-warms and caches the graph so teammates and CI inherit it, fenced by markers so uninstall removes exactly what was added. | repo-graph |
| ★P15 | **Post-commit / post-merge git-event hooks** as a freshness trigger complementary to file watching. | Understand-Anything |
| ★P16 | **Schema migrations as a discipline** — a migration module rather than a version integer. | juspay, cognee, CodeGraph |

### 4.6 Output, errors and surfaces

| # | Adoption | Source |
|---|---|---|
| ★O1 | **Errors carrying `reason`, `source` and `suggestions[]`**, rendered as a "Suggested fixes:" block — making doctor output actionable rather than diagnostic. | CodeGraphContext |
| ★O2 | **Typed result envelope with metadata** — one result object carrying the payload plus `{analyzedAt, durationMs, version}`. | repo-lens |
| ★O3 | **Renderer registry** — one result, many renderers (json / terminal / markdown / mermaid), selected at the boundary. | RepoDoctor, repo-lens, opengrep |
| ★O4 | **Per-command typed schemas** so every command's output is machine-validatable. *(verify-first)* | RepoDoctor |
| ★O5 | **Skipped checks reported, not silently omitted.** | react-doctor |
| ★O6 | **Health score** computed from findings. | react-doctor, RepoDoctor |
| ★O7 | **Structured check envelope** — `{check, status, summary, findings}` as the shape every check emits. | signum |
| ★O8 | **Glossary CI check** — canonical terms plus aliases, scanned across documents, emitting the same structured envelope. (All three passes surfaced this from the same repo.) | signum |
| ★O9 | **NDJSON phase events** for long operations, so clients can render real progress. | deepwiki-open |
| ★O10 | **MCP middleware chain** injecting project context per request. | code-index-mcp |
| ★O11 | **`.well-known/mcp.json` discovery metadata.** | code-index-mcp |
| ★O12 | **Graph-dump debug command** for inspecting the live graph. | colbymchenry |
| ★O13 | **Generated OpenAPI spec** for the HTTP surface, produced in CI. | opengrok |
| ★O14 | **Golden-test harness with `bless` mode** — compare extraction against goldens, with a blessing command and cross-platform normalisation so goldens are stable. | CodeGraph |
| ★O15 | **Per-language snapshot test matrix** — the same suite across every supported language with golden snapshots, making language coverage gaps visible. | serena |

---

## 5. The seam-only register

About a third of this corpus follows the pattern "repo X has a module for Y; Cortex's seam is Z."
These identify where work would land without saying what to build. Collected here so the ranking
stays honest, and because the *frequency* of a seam being named is itself a signal about where
this pass thinks Cortex's structure is thinnest.

Most-named seams, with the count of distinct repos pointing at them:

| Seam | Times named | What repos point at |
|---|---|---|
| `graph/analytics/index.mjs` | ~22 | clustering, communities, complexity, coupling, dead code, taint, concerns, semantic overlays, outlines, hot paths, dependency scanning |
| `evals/` | ~20 | harnesses, rubrics, answer keys, snapshots, held-out sets, pinned commits, A/B arms, profiles, token-efficiency, versioned results |
| `graph/store-sqlite.mjs` | ~15 | ports, drivers, migrations, two-tier index, locks, striped mutexes, connection managers, arenas |
| `lib/generated-docs.mjs` | ~10 | structure/content split, managed frontmatter, tours, onboarding guides, AGENTS.md, publishing adapters |
| `graph/ignored-prefixes.mjs` | ~9 | repo-local ignore files, gitignore loaders, generated ignore lists, per-path overrides, matchers |
| `lib/explorer/` | ~9 | viewers, dashboards, HTML reports, mermaid rendering, visualisation |
| `graph/language-registry.mjs` | ~8 | analyzer registries, strategy+fallback, scoping rules, framework bridges, ctags fallback |
| `lib/operations/doctor.mjs` | ~8 | typed reports, health scores, error ledgers, structured envelopes, skipped checks |
| `release/` + `CHANGELOG.md` | ~12 | changelog fragments, goreleaser, changesets, generated notes, cross-compile, k8s, compose |

Read the top two as the pass's real verdict: **analytics and evaluation are where it believes Cortex
has the most unbuilt surface.** Both are consistent with the other two passes.

---

## 6. The ranked program

Top 20 by (leverage × evidence quality) ÷ effort. S ≤ 1 day, M ≤ 1 week, L > 1 week.

| Rank | Item | Ref | Effort | Why |
|---|---|---|---|---|
| 1 | Safe-parse helper + lint rule | S1 | S | Prevents a hard-crash class permanently. Verify the limit applies to `web-tree-sitter`; adopt the guard either way. |
| 2 | Pin fixture-repo commits | C4 | S | One file; removes silent corpus drift under every other eval item. |
| 3 | `NORM_VERSION` fingerprint auto-invalidation | R1 | S | Migration-free schema evolution. Ranked top-3 by two independent passes. |
| 4 | Redact tokens in subprocess error text (incl. URL-encoded) | S2 | S | A real secret-leak path that egress redaction does not cover. |
| 5 | Four-value freshness verdict + read-repair + footer | C2 | M | Makes edit-then-query honest instead of merely reported. Most complete freshness story across all passes. |
| 6 | Canonical Interval/Span type + one path normaliser | C1 | M | Cross-OS identity stability plus a single parseable ref grammar. Unblocks context-assembly work. |
| 7 | Oracle as a query-time side table | R2 | M | Precision without discarding the heuristic tier as evidence. |
| 8 | A/B agent-outcome harness | C4 | M | The metric that demonstrates the graph's value. All three passes agree. |
| 9 | Intermediate storage then merge | P1 | M | A failed build can never corrupt a sealed store. |
| 10 | Errors with reason/source/suggestions | O1 | S | Turns doctor from diagnostic into actionable for a small change. |
| 11 | Effect classification with transitive propagation | G1 | M | Impact becomes "what does this change do", not "what is connected". Most distinctive capability here. |
| 12 | Severity ladder + gated exit codes + generated baseline | C10 | M | Makes rules adoptable on a repo with history, and CI-meaningful. |
| 13 | Read-only query guard + agent query sanitizer | S3, S4 | S | Two small guards on real agent-facing failure modes. |
| 14 | Golden harness with bless mode + per-language snapshot matrix | O14, O15 | M | Extraction regressions become visible; language gaps become countable. |
| 15 | Two-tier index / outline tier | C9 | M | Orientation and listing stop paying deep-store cost. |
| 16 | Per-language contract with `canAnalyze` gate + fallback tier | C7 | M | Adding a language becomes one drop-in rather than several edits. |
| 17 | LSP→SCIP conversion | R3 | M–L | Exact resolution for any language with a language server, not only those shipping SCIP. |
| 18 | `scip lint` + `scip snapshot` at ingest | R4, R5 | S–M | Stop trusting supplied indexes; make ingestion changes visible. |
| 19 | Kernel/path parity harness | R12 | M | Required before any second extraction path exists; cheap to build first. |
| 20 | Cancellation token + typed cancelled outcome | P8 | M | Long runs become interruptible without leaving partial state. |

---

## 7. Forward and contingent

**If embeddings land**: confine them to one module boundary so nothing else assumes them
(octocode); provider registry behind a base class (claude-context); hybrid keyword+vector search
(axon, juspay); embed-pass vs embed-nodes backfill separation (codebase-graph); clustering over
embedding space (contextplus).

**If dataflow lands**: the CPG model — AST + control flow + data dependency merged in one graph
(joern); an interprocedural dataflow engine; taint tracking as a security-impact query; a
control-flow-graph builder with DOT export (oxc).

**If a declarative extraction DSL lands**: patterns plus rules declaring which nodes, edges and
attributes to create; an execution engine; a static checker; built-in functions; scoped variables
(tree-sitter-graph). Per-language scoping rules compiled from declarations (stack-graphs). **All
three passes independently flag this as the same strategic bet.**

**If a declarative query language lands**: queries as `from/where/select` over a typed schema, with
schema fragments contributed per language (codeql); a named query database (joern); the GritQL
pattern DSL (gritql).

**If a native kernel lands**: kernel alongside wasm with the parity harness (R12) as the gate;
separate bundle and kernel builds; napi/wasm/FFI bindings matrix.

**If remote repos land**: clone service with root confinement and guaranteed cleanup
(deepwiki-open, repo-lens); GitHub URL service; temp-dir hygiene utilities.

**If an LLM-assisted pass lands**: prompts centralised in one module rather than inline
(deepwiki-open, RepoDoctor); a harness abstraction so the verifier is swappable (Codealmanac); JSON
retries with a fresh streamer per attempt; a skeleton→enrich two-call pipeline; sandboxed
verification runtimes (potpie).

---

## 8. What this pass uniquely contributes

Against the other two syntheses:

1. **Every item is pre-routed to a real file.** No other pass does this. It converts a backlog of
   ideas into a backlog of tickets.
2. **The safe-parse crash rule (S1).** Appears nowhere else and prevents a whole class of hard
   failure.
3. **The most complete eval corpus (C4).** Pinned commits, held-out sets, rubric judging, prompt
   arms, A/B controls, answer keys, diagnosis tooling, committed versioned results — nine repos'
   worth of harness design in one cluster.
4. **Read-repair freshness (C2).** Freshness as a query-time repair action rather than a reported
   state. Only this pass surfaces it.
5. **Effect classification with transitive propagation (G1).** The one genuinely new *graph
   capability* across all three passes.
6. **Parity and add-a-language harnesses (R12, R13).** Process tooling nobody else proposes.
7. **`reprolang` (R6).** A purpose-built fixture language for testing the whole pipeline — more
   controllable than any real repository.
8. **DLP read-secret tracking and payload tombstones (C5).** Recording *which reads touched
   secrets*, and crypto-shredding payloads while preserving graph structure.

---

## 9. Cross-pass agreement and conflict

**Where all three agree** (and Qwen was independent, so this is real corroboration):

- Git co-change / change-coupling edges.
- Ports over the store, with the in-memory double as the actual prize.
- The declarative extraction DSL as the long-term architectural bet.
- Agent-outcome benchmarking over graph-fact checking.
- Glossary discipline enforced in CI.
- Community detection, dead-code, and clone/duplication as named analytics.
- Baseline-of-known-violations for rules.
- SCIP as under-exploited.

**Where this pass and M3 agree but Qwen dissents** — treat with care, since this pass saw M3's
output:

- **Backend pluralism.** Both propose swappable stores from nine repos; Qwen read those backends
  and rated all of them weaker than SQLite. Resolution unchanged: **take the port and the test
  double, not a second production store.**
- **`semantic` / `semantica` identification.** Both are wrong for the same reason — resolving
  GitHub org names instead of reading `repos/`. Prefer Qwen; void four entries across two files.

**Where this pass is thinner than the others:**

- **Determinism.** Qwen makes it a top-tier finding from eleven repos with a concrete checklist
  (posix-normalise before hashing, seeded RNG, sorted merges, explicit tiebreaks, incremental≡full
  equivalence). This pass mentions it only in passing, via cross-platform golden normalisation.
  Any adoption here that touches ranking or graph content should inherit Qwen's checklist.
- **Epistemic honesty.** Qwen's nine-repo convergence on `exact | lower-bound` impact envelopes with
  machine-readable cause counters has no counterpart here.
- **Per-edge confidence.** Qwen's six-repo convergence on stamping each edge with strategy and
  confidence is absent; this pass touches it only through potpie's contract and codebase-graph's
  consensus score.

**Net**: the three passes are complementary. Qwen says *what to compute and how to know it is
right*; M3 says *how to arrange the system*; this one says *where each change lands and how to
prove it worked*.

---

## 10. Low value, and what to skip

**Build-system and packaging filler** (~30 items across ~15 entries): Nix flakes, devcontainers,
Bazel hermeticity, dual builds, goreleaser, cliff, changesets, Docker matrices, k8s manifests,
docker-compose, shell completions, cross-compile configs, husky hooks, per-package Makefiles, Zig
builds. Individually defensible, collectively a distraction from the ranked program. Two are worth
keeping: **changelog fragments per change** (semgrep — avoids merge conflicts and guarantees every
change is release-noted) and **marker-fenced git hooks** (repo-graph, already ranked at P14).

**Void entries**: `semantic/semantic2` (misidentified; its ten Rails-shape items — schema file,
migrations, seeds, concerns, routing, per-environment config, rake tasks, uploaders, deploy config,
lockfile discipline — carry nothing Cortex needs) and `semantica` (reported non-existent;
contradicted by Qwen). Both should be re-studied from the vendored `repos/` directories.

**Redundant with existing capability**: several proposals restate what the pass elsewhere
acknowledges Cortex has — merkle change detection, generation identity, evidence packs, path
confinement, SARIF output. Marked *verify-first* throughout; these are audits, not builds.
