# Competitor adoption synthesis — M3 pass

One document consolidating the M3 model pass — 55 per-repo adoption ledgers, 10 proposals each —
into a single deduplicated, sequenced program for Cortex.

- **Input**: 55 study files plus the pass's own thematic `README.md` (~550 raw proposals).
- **Corpus**: complete. M3 covers all 55 repositories in the competitor index, **including the 10
  the Qwen pass skipped**: `ast-grep`, `gritql`, `codegraph-ai/CodeGraph`, `colbymchenry/codegraph`,
  `rag-rat`, `contextplus`, `codeql`, `stack-graphs`, `axon`, `signum`. Section §8 isolates what
  only this pass can tell us.
- **Companion**: `synthesis-qwen.md` covers the same corpus from a different angle. The two passes
  are complementary rather than redundant — see §2 for what separates them, and §9 for the conflicts
  a cross-model merge will have to resolve.

Nothing here has been implemented. Claims about competitor internals are as reported by the pass
and have not been independently re-verified against the vendored repos.

---

## 1. How to read this

Raw proposals were deduplicated on mechanism. The result:

| Bucket | Count | Meaning |
|---|---|---|
| **Convergences** | 12 clusters | ≥3 independent repos proposing the same structural move. |
| **Distinct adoptions** | ~85 | Single-source proposals worth taking, organised by subsystem in §4. |
| **Cheap lifts** | 22 | Verbatim constant lists, regexes and pattern tables — hours of work, no design risk. Isolated in §5 because they are the fastest value in the corpus. |
| **Speculative / contingent** | ~90 | Depends on lanes Cortex doesn't have (LSP, embeddings, Rust kernel, multi-backend store) or on architecture decisions not yet made. §7. |
| **Low-confidence or wrong** | ~25 | Proposals resting on a misread of Cortex or of the competitor. §9. |

---

## 2. What kind of pass this is

This matters more than usual, because it determines how much weight each item carries.

**The M3 pass is an architecture-and-shape pass.** Where the Qwen pass asks *"what mechanism does
this repo implement, and what does Cortex's equivalent get wrong?"*, M3 asks *"how is this repo
organised, and what would Cortex look like organised that way?"* Consequences:

- **Strength**: it sees the whole surface. Module boundaries, port/adapter discipline, command
  vocabulary, packaging, installer UX, CLI ergonomics, skill bundles, config precedence, lint
  posture — an entire layer the mechanism-focused pass barely touches. Where M3 is specific, it is
  specific about *shape*, and shape is where Cortex's reported pain (one large `service.mjs`, flat
  MCP tool list, scattered freshness logic) actually lives.
- **Weakness**: many items are inferred from directory listings rather than from reading the
  implementation. A proposal like "adopt a `concerns/` module because infigraph has one" carries no
  evidence about what the module *does*. Roughly a third of the corpus is of this form.
- **Consequence for ranking**: I have weighted proposals by whether the study quotes a concrete
  mechanism (constant, algorithm, type, invariant) versus naming a file or folder. Items that only
  name a path are demoted regardless of how confidently the study asserts them.

**Reliability caveats to carry into every reading:**

1. **Two repo identifications look wrong.** M3 reports `semantic/` as a Rails 3.1 CMS
   (`semantic2`, 72 Ruby files) and `semantica` as *an org with no public repositories*. The Qwen
   pass, reading the vendored `repos/` copies, reports `semantic/` as GitHub's archived Haskell
   tree-sitter analysis library and `semantica/` as a ~178K-LOC Python NLP/knowledge-graph
   framework. M3 appears to have resolved GitHub org names instead of reading the vendored
   directory. **Prefer the Qwen identification for these two**; M3's `semantic2` and `semantica`
   entries should be treated as void.
2. **The pass's own README is unreliable.** Its theme tables cite sources that do not exist in the
   corpus — "Bio0-sig", "ForgRadius", "jsr-praiso", "marmel", "McRank" (for MCrank), "infograph"
   (for infigraph). The per-file studies are sounder than the README that summarises them; this
   synthesis is built from the files, not the README.
3. **Several proposals assert Cortex lacks something it may already have** (signing primitives, a
   diff-treeish path, path-confinement, redaction). Each such item is marked *verify-first* below —
   the work may be an audit rather than a build.
4. **Named seams are speculative.** Paths like `lib/ports/`, `lib/pipeline/pass-*.mjs`,
   `lib/freshen/` are the pass's proposals, not existing files. Read them as intent, not as
   references.

---

## 3. The 12 convergences

### C1. Ports and adapters: a named interface per concern
*Codealmanac (service/store/ports triad per domain), CodeGraphContext (`DatabaseManager` +
per-backend wrapper), potpie (`GraphDefinition`/`GraphMutationPolicy`/`GraphBackend` composed by a
builder), Code-Index-MCP (one interface module per concern), codebase-graph (`driver.ts` +
`driver-registry` + `drivers/`), CodeGraph (rocksdb/memory/namespaced backends behind one open()),
colbymchenry/codegraph (`sqlite-adapter.ts` as an adapter, not a hard dep), llama_index
(`graph_stores/` family under one contract), Sourcetrail (`ParserClient` callback sink decouples
parser from store).*

Nine repos, one shape: **services depend on port interfaces; concrete adapters live behind them.**
The two payoffs the studies name are testability (inject a fake store instead of monkey-patching)
and optionality (swap or add a backend without touching core).

**Adopt**: define ports for the seams Cortex actually has — graph store, parser, watcher, redaction,
token budget — and move today's implementations behind them. This is the corpus's single most
repeated structural claim. Note the ceiling: several of these repos ship 3–5 backends and the Qwen
pass rated every one of those backends *weaker* than Cortex's SQLite. **Adopt the port, do not adopt
the backend sprawl** — the value is the seam and the in-memory test double (Sourcetrail's
`TestStorage`, CodeGraph's `in_memory()`), not a second production store.

### C2. Per-pass pipeline instead of one orchestrator
*codebase-memory-mcp (`pipeline/pass_*.c` + one orchestrator; the pass's clearest case),
ai-code-audit (five parallel perspective nodes → aggregator), cognee (`cognify` ingest vs `memify`
rewrite as separate pipelines), llama_index (`run_transformations`), Codealmanac (`Workflow` with
steps/states/retries), context8 (typed `PipelineContext` threaded through), treesitter-chunker
(`processors/` strategy fan-out), RepoDoctor (per-command vertical slice owning schema + prompts +
renderer).*

Eight repos. The mechanism worth naming: each pass is `{ id, run(snapshot, graph) → diff }`,
registered in an explicit DAG, individually skippable and testable. The claimed payoff is
optionality — a new edge kind, an LSP upgrade, an embedding lane all become *a pass* rather than an
edit to the orchestrator.

**Adopt**: decompose the incremental/phase-2 path into registered passes with a typed diff return
and per-pass receipts. `--skip-pass <id>` falls out for free and makes bisection trivial.

### C3. Six-verb MCP surface with skills 1:1
*repo-graph (`orient/find/impact/trace/read/refresh` as the **only** surface, one Claude skill per
verb), code-compress (`Server/Scoping/` — named scopes select tool subsets), PraisonAI
(`guardrails/` + `allowed_tools_filter`), GitNexus (`read-only-policy`), octocode (multi-server
composition, one provider module per concern), juspay (tools grouped by domain), code-index-mcp
(tools categorised by domain), roam-code (profile-gated registration), react-doctor (13 shipped
skills), colbymchenry (skills shipped alongside).*

Ten repos converge on the same complaint: **a flat, large tool list degrades agent routing.** Two
distinct fixes appear — collapse to a small canonical verb set (repo-graph), or keep the tools and
gate them by named scope/profile (code-compress, PraisonAI, roam-code). They compose: a default
six-verb scope over a full registry.

**Adopt**: a canonical verb vocabulary as the default scope, named scopes (`minimal/explore/audit/
full`) selecting subsets, a shipped skill per verb, and a test pinning the registered tool set so
the surface can't drift silently.

### C4. Freshness as one module, not a scattered concern
*sense (`freshen/` = debounce + reconcile + lock + process + service + git + watcher in one
package — the pass calls this the cleanest of the 55), GitNexus (`staleness.ts` as its own module),
Understand-Anything (`fingerprint.ts` + `staleness.ts` paired), Brain0 (`Snapshot` as the
mode-agnostic ingest input shared by git and watch paths), Code-Index-MCP (four-value
`FreshnessVerdict` enum), codebase-graph (`STALENESS_THRESHOLD_MS` + `isStale()`),
claude-context (`SnapshotManager` + stale-lock with env override), signum (`staleness-check.sh`).*

Eight repos. Two sub-claims worth separating: (a) **the code belongs in one module** — Cortex's
freshness logic reportedly spans the watcher, orientation evidence and git snapshot paths;
(b) **the verdict should be an enum with a reason**, not a boolean — `fresh | stale_commit |
stale_age | invalid`.

**Adopt**: consolidate the freshness surface into one module with an explicit debounce → batch →
reconcile → lock → apply pipeline, and standardise the four-value verdict with a reason field.
Brain0's `Snapshot` shape is the right input contract: one mode-agnostic ingest path fed by git,
watch or CLI, tagged with which.

### C5. Version-stamped everything, granular invalidation
*rag-rat (`NORM_VERSION` per fingerprint kind — old rows auto-excluded by a version filter, next
reindex produces the new version; self-healing without a migration), roam-code
(`LanguageExtractor.VERSION` with an explicit bump policy: mismatch tells consumers to rebuild),
Aider (parser-version-keyed cache dir; `CACHE_VERSION` bumped when the tag shape changed),
claude-context (multi-version snapshot schema with backward-compatible readers), treesitter-chunker
(per-language version detection so a claim can be scoped "holds on Python ≥3.10"),
Understand-Anything (`FingerprintV1` carrying its granularity).*

Six repos, one complaint: **a single global schema version is too coarse.** A parser tweak
shouldn't invalidate claim verdicts; a claim-schema bump shouldn't force re-parsing.

**Adopt**: separate version stamps per fingerprint granularity — parse, symbol, claim, attestation —
each with its own bump policy, and a version filter on read that excludes stale-shaped rows so the
next run heals them. This is the cheapest high-leverage item in the whole M3 corpus.

### C6. A canonical ignore vocabulary, stated once
*contextplus (`ALWAYS_IGNORE` frozen set), axon (`DEFAULT_IGNORE_PATTERNS` frozenset), ai-code-audit
(`DEFAULT_EXCLUDE_PATTERNS`), context8 (comprehensive default excludes), Aider (pinned start-list in
`watch.py`), Understand-Anything (`ignore-generator` that *suggests* new entries from observed
over-representation), CodeGraphContext (`.cgcignore` with parent traversal bounded to the git
worktree), octocode (`.noindex` sentinel files).*

Eight repos ship essentially the same list. Two refinements are worth more than the list itself:
CodeGraphContext's **scoping rule** (walk parents for ignore files only while inside the git
worktree — otherwise a repo under `/tmp` picks up unrelated rules), and Understand-Anything's
**generator** (report that `build/` is 28% of indexed files and suggest ignoring it).

**Adopt**: one canonical list consumed by parser, watcher and init alike; the worktree-bounded
parent-traversal rule; and a `--suggest-ignore` doctor mode.

### C7. Typed cancellation and abort, store-backed
*context8 (`CancellationToken` with dual poll+abort control, `TaskCancelledException` as a distinct
type), contextplus (`EMBED_TIMEOUT_MS` + a top-level `cancelAllEmbeddings()`), claude-context
(`indexingTasks: Map<path, {controller, promise}>` — clear-index *awaits* cancellation rather than
racing it), react-doctor (`active-scan-abort-registry`), sense (cancellable debounce loop),
code-index-mcp (FIFO limiter with structured queue timeout), Codealmanac (out-of-process workers
with psutil lifecycle control).*

Seven repos. The invariant they share: **a cancellation is not a failure**, it is its own typed
outcome, and shutdown awaits in-flight work rather than dropping it.

**Adopt**: a cancellation token threaded through long operations, an active-run registry keyed by
id, await-before-drop on shutdown, and a distinct cancelled outcome in receipts so agents can tell
"you stopped me" from "it broke".

### C8. Signed, hash-chained provenance above content hashes
*rag-rat (device-signed op-log: `[domain, stream_id, prev_hash, lamport, device_fingerprint,
op_bytes]`, unknown ops stay opaque-but-verified; canonical CBOR for the signed wire form; LWW fold
by Lamport+device for conflict resolution), Brain0 (Ed25519 with a `Signer`/`Verifier` split and
private key redacted from Debug output), roam-code (in-toto Code-Graph Attestation with a merkle
root over per-file symbol fingerprints plus an edge-bundle digest; a shared VSA emitter; cosign
blob signing), Code-Index-MCP (attestation dataclass wrapping `gh attestation sign/verify` with an
up-front scope check), PraisonAI (audit module separate from telemetry), semgrep/codeql (provenance
posture).*

Six repos treat provenance as a signed, ordered, verifiable chain. The pass's blunt framing:
"Cortex's content-hash receipts are a pale version of this."

**Adopt** (verify-first — confirm what signing Cortex already has): a device identity minted on
install (`fingerprint = sha256(pubkey)`), a `Signer`/`Verifier` split with key material never
printed, and — the item with the clearest external payoff — roam-code's **CGA predicate**: an
attestation carrying a merkle root over per-file symbol fingerprints plus an edge-bundle digest,
verifiable in milliseconds by re-deriving both from the live DB **without access to source**.

### C9. Declared vs actual reconciliation
*Brain0 (`DeclaredChange` vs `ActualChange`: gap-fill links every observed change to a task, drift
fires when declared ≠ actual), contextplus (a hidden `mcp-shadow-history` branch recording every
agent-applied change, with restore points), colbymchenry (worktree-index mismatch detection and
warning), axon (diff via git worktrees so the user's HEAD is never touched), rag-rat (`papertrail`
mirroring issue-tracker state so a claim can cite it as evidence).*

Five repos independently build the same missing primitive: **the system checks what an agent said
it did against what it observably did.** Brain0 attaches every change to a task node with intent and
session; drift becomes a typed finding rather than an argument after the fact.

**Adopt**: a reconciliation surface comparing declared paths against observed changes, emitting
`{declaredPaths, actualPaths, missing, unexpected}`; task/session identity on changes; and — for
any surface that applies edits — the shadow-branch restore-point pattern.

### C10. Multi-format, multi-renderer output
*dependency-cruiser (≈10 report formats including `dot-webpage` and an **`anon` mode that hashes
identifiers so a graph can be shared without leaking paths or symbols**), juspay (four rendering
modes with `auto` switching to community view above a node threshold), RepoDoctor (renderer family:
terminal / command / json), repo-lens (JSON verbatim vs markdown, spinners silenced in JSON mode),
stack-graphs (its own graph renderer beyond Mermaid), Understand-Anything (builder-per-task, each
with a named output shape).*

**Adopt**: a renderer family behind one interface (mermaid / dot / json / markdown / csv), the
auto-degrading render mode for large graphs, and the anonymising exporter — that last one is a
genuinely distinctive capability, not a cosmetic.

### C11. Curated pattern tables are worth copying, not deriving
*repo-lens (entry points, test dirs, config files — three curated lists), juspay
(`_FRAMEWORK_DECORATOR_PATTERNS` for handler detection, `SECURITY_KEYWORDS` for criticality
scoring), colbymchenry (framework resolvers per stack: Astro, Laravel, NestJS, cargo-workspace…),
potpie (`KNOWN_BINARY_EXTENSIONS`, ~60 entries), code-index-mcp (`SUPPORTED_EXTENSIONS` as one
authoritative list), infigraph (route-decorator → service-edge mapping), axon (`_is_test_file`
heuristic), Aider (curated special/important-files list).*

Eight repos ship tables that are tedious to derive and cheap to copy. The pass is right that this
is the highest value-per-hour in the corpus — see §5.

### C12. Glossary and vocabulary discipline enforced by CI
*signum (`project.glossary.json` with canonical terms + aliases, enforced by a CI check across code
and docs; plus `doc-parity-check`, `metric-ratchet` for only-up metrics, `anti-entropy-report`),
Understand-Anything (alias tables normalising type drift before validation), opengrok (`LangMap`:
one canonical language id, many aliases), agentic-codebase (a single 16-value edge enum forcing one
vocabulary), CodeGraph (core principles declared at the top of the library entry point).*

Five repos. Signum's case is the sharpest and the cheapest: one JSON file plus one check script,
applied to every doc, claim and receipt. The pass observes that Cortex's own writing uses
"decision" / "verdict" / "recipe" for what is sometimes the same thing.

**Adopt**: a glossary file with canonical terms and aliases, a CI check, and — separately valuable —
signum's **metric ratchet**: assert that named metrics (exact-resolution %, claim fidelity) are
non-decreasing across tagged commits.

---

## 4. Thematic adoption catalogue

Distinct proposals beyond the convergences. Source repo in parentheses. Items marked ★ quote a
concrete mechanism; unmarked items are structural proposals inferred from layout — weight
accordingly.

### 4.1 Resolution and extraction

| # | Adoption | Source |
|---|---|---|
| ★R1 | **SCIP oracle as a side table, never an overwrite.** Read `.scip`, join occurrences to edges by *identifier-token containment, not line equality*, write `edge_oracle` rows alongside the heuristic edge rather than replacing it. Doctor runs the oracle pass; queries merge. Preserves the heuristic tier as evidence of what the cheap path concluded. | rag-rat |
| ★R2 | **Compilation-database awareness.** Read `compile_commands.json`; expose include paths, system/framework header paths and macros to the C/C++ path. The pass claims exact-resolution on C++ moves from ~12% to ~60% — treat the numbers as unverified, the mechanism as sound: tree-sitter cannot know `-DFOO=bar`. | Sourcetrail, codebase-memory-mcp |
| ★R3 | **Symbol stack vs scope stack.** Two stacks, not one: what we are resolving vs where we are looking. A name binding *is a path* through the graph, and the path is worth recording on the edge alongside source and target. | stack-graphs |
| ★R4 | **Partial paths as a first-class result.** When resolution can't complete, return `{from, steps, reached, blockedAt}` rather than an error — the agent learns *where* it is stuck. | stack-graphs |
| ★R5 | **Case-insensitive-identifier declaration per language.** PHP, Apex, HCL and FoxPro fold case; a resolver that case-folds unconditionally produces false bindings to stdlib names. One boolean per language table. | roam-code |
| ★R6 | **Standard extractor interfaces with a version stamp.** `extract_symbols` → `{name, qualified_name, kind, signature, line_start, line_end, docstring, visibility, is_exported, parent_name}`; `extract_references` → `{source_name, target_name, kind, line, import_path}`; base class carries `VERSION` with a documented bump policy. Providers conform or refuse to load. | roam-code |
| R7 | **Two-tier parser per language**: AST tier plus a token/generic tier, so a language whose grammar fails degrades to pattern matching at a lower confidence tier instead of dropping out. | opengrep |
| R8 | **Typed-AST tier where the language has a real one** (Python `ast`, Rust `syn`, Go `go/ast`) as a distinct confidence tier above tree-sitter. | opengrep |
| ★R9 | **Align the symbol taxonomy with tree-sitter's `tags` crate** rather than inventing one. The most-shipped tag vocabulary in existence, and it costs nothing to match. | tree-sitter |
| ★R10 | **Language alias map**: one canonical id per language with an alias list (`python`/`python3`/`py`) and extension list, so routing has a single source of truth. | opengrok |
| ★R11 | **Route edges from framework decorators** — `@GetMapping("/foo")`, `app.get(...)` → `CallsRoute {symbol, method, path}`, making `impact --route /foo` answerable. | infigraph, juspay |
| ★R12 | **Config-link pass**: code symbol → env var / IaC / chart value edges, so "what configuration affects this code path?" is a query. | codebase-memory-mcp, agentic-codebase |
| ★R13 | **Identifier splitting at index time** (camelCase / snake / kebab / dot → token list stored on the symbol) so `userName` matches `getUserNameById`. | code-compress |
| ★R14 | **Signature-only extraction** as a distinct cheap surface: `{name, args, returnType, throws, modifiers}` — a much smaller wire shape for impact queries than a full body. | octocode |

### 4.2 Graph model and vocabulary

| # | Adoption | Source |
|---|---|---|
| ★G1 | **A single closed edge-kind enum.** agentic-codebase's 16: `Calls, Imports, Inherits, Implements, Overrides, Contains, References, Tests, Documents, Configures, CouplesWith, BreaksWith, PatternOf, VersionOf, FfiBinds, UsesType`. Cortex need not adopt all sixteen — the value is being *forced to decide* the vocabulary in one place. `Tests`, `Documents` and `Configures` are the ones agents ask for. | agentic-codebase |
| ★G2 | **A node-kind enum with real granularity** — `Module | Symbol | Type | Function | Parameter | Import | Test` — so `--kind function` filters mean something. | agentic-codebase |
| ★G3 | **Edge identity split from occurrence.** (Same finding the Qwen pass reports from Sourcetrail.) Here it arrives via the `ParserClient` sink: the parser emits typed records, the store dedups. | Sourcetrail |
| ★G4 | **Layers as a persisted overlay**: `{id, name, description, nodeIds[]}` derived once, so architecture answers come back in layers rather than in nodes. | Understand-Anything |
| ★G5 | **Flows as first-class entities** (`flows` + `flow_memberships`), with criticality scored partly from a security-keyword table. | juspay |
| ★G6 | **Namespaced storage**: several graphs sharing one backend, removing per-repo lock contention in federated setups. | CodeGraph |
| ★G7 | **Branch-aware graph state** — a map of graph state keyed by treeish rather than a single current state. | infigraph, octocode |
| ★G8 | **Cycle detection as a named module** over neighbourhood walks, with cycles logged to receipts. | stack-graphs |
| ★G9 | **Fluent query builder** — `.node_type(...).in_file(...).property(...).execute()` — so a new filter is a typed predicate rather than an edit to an imperative query function. | CodeGraph |
| ★G10 | **Payload store off-row.** Large content behind content-addressed blobs with opaque `PayloadRef` pointers; the pass estimates 5–10× DB size reduction on content-heavy repos. | Brain0 |
| ★G11 | **Shallow + deep dual index** — a lightweight content-hash JSON map for fast watch matching, with the full store consulted only on a hit. Makes very large repos watch-friendly. | code-index-mcp |
| ★G12 | **Typed structural diff**: `{added_nodes, removed_nodes, modified_nodes, added_relationships, removed_relationships}`, produced by parsing a **git worktree** so the user's HEAD is never switched or stashed. | axon |

### 4.3 Rules, claims and verification

| # | Adoption | Source |
|---|---|---|
| ★V1 | **Five-tier severity** `hint | info | warning | error | off` on verdicts — semantic, not descriptive, and the shape the structural-search community already shares. | ast-grep |
| ★V2 | **`RuleConfig` as a typed artifact**: `{name, language, pattern, fix?, severity, metadata}`, bucketed per language, with glob scoping that carries an explicit `case_insensitive` flag and a prebuilt glob set. | ast-grep |
| ★V3 | **Pattern vs Predicate separation** — "this matches" and "this is true of it" are different node types in a rule body, not interleaved prose. | gritql |
| ★V4 | **A typed `Constant` model** (`Boolean | String | Integer | Float | Undefined`) with explicit `isTruthy`/`isUndefined`, so evidence values stop relying on JS truthiness. | gritql |
| ★V5 | **Rules declare their effects** (`{reads, writes}`) and are statically analysed before running; a rule that claims read-only and mutates is caught at admission. | gritql |
| ★V6 | **Suppression as a modelled concept** — a comment syntax parsed into typed suppressions, plus explicit ids for *suppress-all* and, importantly, **unused suppressions** surfaced by doctor. | ast-grep |
| ★V7 | **Metavariable bindings** (`$VAR` capture with an environment) so a claim can be "this exact pattern with these bindings existed", not just "this string was present". | ast-grep, gritql |
| ★V8 | **Rule packs** — a declarative bundle manifest (`{id, version, rules[]}`) so rules ship and version as units. | codeql |
| ★V9 | **Baseline violations file** — record accepted violations; subsequent runs report only new ones. (Independently proposed in the Qwen pass from the same repo; two passes agreeing raises confidence.) | dependency-cruiser |
| ★V10 | **Dependency-path traversal** — invert child edges into a parent map and walk upward, emitting paths ordered direct-introducer-first, so "how did this dependency get here?" is answerable. | semgrep |
| ★V11 | **Dependency-aware rules** — a rule can declare `requires: <package>` and be cross-checked against the resolved dependency graph. | semgrep |
| ★V12 | **Dream-mode findings** — a maintenance sweep emitting `coverage_gap | stale_reference | memory_unverifiable`. Answers "this load-bearing symbol has no claim at all", which pure staleness checking never asks. | rag-rat |
| ★V13 | **Anti-entropy report** — count drifted facts (verdicts N generations old with no review) as a named, tracked metric. | signum |
| ★V14 | **Ontology module** — typed types and predicates that verdicts reference, instead of free-form claim vocabulary. | cognee |
| ★V15 | **CODEOWNERS-aware findings** — attach owners to impact results and claims. | gritql, codeql |
| ★V16 | **Multi-perspective analysis** — architectural / security / quality / business / modernisation as parallel lenses over one graph, each a deterministic subroutine, merged by a typed aggregator. | ai-code-audit |

### 4.4 Runtime, process and performance

| # | Adoption | Source |
|---|---|---|
| ★P1 | **Lazy heavy-module loading** — defer the graph/store load until first query; the pass claims MCP startup drops to <100ms. | GitNexus |
| ★P2 | **Out-of-process worker with a typed IPC protocol** (`core` / `ipc` / `protocol` triad), so heavy analysis is isolated, cancellable and independently debuggable. | GitNexus, Codealmanac, CodeGraphContext |
| ★P3 | **Heap re-spawn for memory-heavy operations** — one command that needs a large heap re-execs with a raised limit rather than every command paying for it. | GitNexus |
| ★P4 | **mmap above a size threshold** (256 KB) plus **sample-based binary detection** (8 KB sample, >30% non-text ⇒ binary, skip parse). | potpie |
| ★P5 | **Parallel, pathspec-aware walker** with an explicit concurrency option and progress callback. | potpie, context8 |
| ★P6 | **WAL pressure valve** — after N writes in a long ingest, checkpoint-truncate explicitly rather than letting the WAL grow unbounded. | colbymchenry |
| ★P7 | **Platform-aware watch policy** — detect WSL2, network shares and bind mounts, and fall back to git hooks instead of pretending fs events work. Plus per-platform observer selection with documented pitfalls. | colbymchenry, code-index-mcp |
| ★P8 | **Explicit worker pool contract** (`spawn` / `wait` / `cancel`) with parallelism capped by config. | codebase-memory-mcp |
| ★P9 | **Ordered output from parallel work** — collect, sort by path, flush in batches, so concurrency never changes the output bytes. | lsif-go |
| ★P10 | **Arena allocation for parse-intermediate objects** to cut per-node allocation churn. | stack-graphs |
| ★P11 | **Debounced batch with changed/removed separated** (`{changed[], removed[]}` after ~300ms) as the watcher's output contract, rather than per-file events. | sense |
| ★P12 | **Lock with stale timeout and env override** so two watchers cooperate instead of colliding. | sense, claude-context |
| ★P13 | **Co-running serve mode** — MCP server and watcher in one process sharing a heap, so MCP reads are always fresh. | axon |
| ★P14 | **Scheduling adapter port** (launchd / Task Scheduler / systemd / interval) for periodic re-verification that survives reboots. | Codealmanac |

### 4.5 CLI, install and host integration

| # | Adoption | Source |
|---|---|---|
| ★H1 | **Marker-anchored idempotent installer.** `MARKER_START`/`MARKER_END` delimit the block the installer owns in every host config file (CLAUDE.md, `.mcp.json`, permissions), with a target registry per host. Re-runs update in place instead of stomping or duplicating. Named by the pass as the cheapest real upgrade in the corpus. | repo-graph |
| ★H2 | **Typed run context built at start** — `{ts, command, args, env}` referenced by every receipt from that invocation. | react-doctor |
| ★H3 | **Typed CLI outcome** — `{status: ok|warn|error, findings[], receiptId}` instead of a bare exit code. | react-doctor |
| ★H4 | **Code-frame errors** — render the source frame around an error location rather than a raw stack. | react-doctor |
| ★H5 | **Typed config precedence** resolved in one place: CLI > env > project config file > defaults. | semgrep |
| ★H6 | **A constants module** with the operational defaults exposed via a `--show-constants` flag. | semgrep, claude-context |
| ★H7 | **Typed env access** — a registry of known environment variables with types, replacing scattered raw lookups. | claude-context |
| ★H8 | **Install-method detection** (npm / npx / source / bundle) so upgrade runs the right path, plus the detached-helper dance for Windows in-place upgrade (the pattern rustup, nvm-windows and Volta all use). | colbymchenry |
| ★H9 | **Version check via the releases/latest redirect** rather than the API, avoiding rate limits. | sense |
| ★H10 | **Project-root resolver** — walk up for `package.json` / `Cargo.toml` / `go.mod` / `pyproject.toml` and pick the closest, instead of assuming cwd. | code-compress |
| ★H11 | **Polymorphic `--repo`** accepting a local path or a git URL, with shallow-clone-and-cache plus a guaranteed cleanup callback for the URL case. | repo-graph, repo-lens |
| ★H12 | **Emit a host skill pack from current config**, so setup is one command rather than a documentation exercise. | GitNexus, colbymchenry |
| ★H13 | **Build-cost reported at init** — `{nodes, edges, crossStackEdges, unknownEdges}`, with cross-stack edges counted separately so multi-language repos aren't summarised into one meaningless number. | repo-graph |
| ★H14 | **NDJSON event stream** (`{ts, kind, payload}` per line) as the uniform progress channel, with severity on each event, so `--follow` is just `tail -f`. | deepwiki-open, Mentat |
| ★H15 | **IDE extension stubs** (VS Code, JetBrains) shipped in-repo — three peers ship them alongside the MCP server. | juspay, CodeGraph, serena |

### 4.6 Repository and project hygiene

| # | Adoption | Source |
|---|---|---|
| ★Y1 | **Module manifest** (`{path, name, status, owner, description}` per module) driving owner-based reviewer routing. | signum |
| ★Y2 | **Doc-parity check** — regenerate the generated docs and diff against what's committed; an override is allowed but must be explicit and tracked. | signum |
| ★Y3 | **ADR check** — every architecture decision record must resolve to a working reference. | signum |
| ★Y4 | **Per-module change notes** rather than one changelog, so a reader learns about the module they touched. | codeql |
| ★Y5 | **Strict lint baseline as policy**: warn on `console.log`, `TODO`, `FIXME`, `debugger`; ban `eval`/`new Function`; zero-warnings CI. | Brain0, CodeGraph |
| ★Y6 | **Per-schema files with an index**, replacing one pooled catalog, each with a matched validator. | dependency-cruiser, signum |
| ★Y7 | **Per-language test corpora** — `tests/fixtures/<language>/` for every parser, so coverage gaps are visible as missing directories. | joern, lsif-go |
| ★Y8 | **Fuzz corpus of regressions** — every input that once broke the pipeline becomes a permanent test. | react-doctor, tree-sitter |
| ★Y9 | **In-memory store implementing the production interface**, so tests exercise the same contract rather than a stub. | Sourcetrail, CodeGraph |
| ★Y10 | **Round-trip tests on any interchange format** — emit then parse must produce an equal value. | scip |

---

## 5. The cheap-lift register

Twenty-two items the pass identifies as copyable tables, constants or small guards. Collectively a
few days of work with essentially no design risk, and several close real gaps. Ordered by
value-per-hour.

1. **Ignore vocabulary** — merge the four lists (contextplus, axon, ai-code-audit, context8) into
   one canonical set. *(C6)*
2. **Entry-point patterns** — `index.*`, `main.*`, `app.*`, `server.*`, `cmd/main.go`. *(repo-lens)*
3. **Test-directory patterns** — `^tests?/`, `^__tests__/`, `^spec/`, `^e2e/`. *(repo-lens)*
4. **Config-file patterns** — `.env`, `tsconfig.json`, `Dockerfile`, `Makefile`, `.github/`.
   *(repo-lens)*
5. **Binary extensions** — ~60 entries. *(potpie)*
6. **Binary detection thresholds** — 8 KB sample, 30% non-text ratio. *(potpie)*
7. **Framework decorator patterns** for handler/entry detection. *(juspay)*
8. **Security keywords** for criticality scoring. *(juspay)*
9. **Supported-extensions list** as one authoritative constant. *(code-index-mcp)*
10. **Safe-path regex** as a shared exported helper instead of re-inlined validation. *(axon)*
11. **`assertWithinRoot` guard** replacing ad-hoc relative-path checks. *(contextplus)* — verify-first
12. **`ensureAbsolutePath`** normalisation at every command entry. *(claude-context)*
13. **Max traverse depth** as a single constant shared by every graph walk. *(axon)*
14. **Confidence tag** — three thresholds rendering as ``/`(~)`/`(?)` inline in output. *(axon)*
15. **Conservation verdict bands** — `Excellent ≥0.9 … Wasteful <0.3`. *(agentic-codebase)*
16. **Freshness verdict enum** — four values with a reason. *(Code-Index-MCP)*
17. **Severity enum** — five values on verdicts. *(ast-grep)*
18. **Debounce default** — 300 ms with changed/removed separation. *(sense)*
19. **Concurrency ceiling** — a small FIFO limiter with a structured queue-timeout error.
    *(code-index-mcp)*
20. **mmap threshold** — 256 KB. *(potpie)*
21. **Sync/lock stale timeouts** with env override. *(claude-context)*
22. **Glossary file** — one JSON of canonical terms and aliases, plus a check script. *(signum)*

---

## 6. The ranked program

Top 20 by (leverage × evidence quality) ÷ effort. Effort: S ≤ 1 day, M ≤ 1 week, L > 1 week.

| Rank | Item | Ref | Effort | Why |
|---|---|---|---|---|
| 1 | Cheap-lift register (all 22) | §5 | S–M | Days of work; closes several real gaps; zero design risk. Do this first regardless of everything below. |
| 2 | Granular version stamps per fingerprint kind | C5 | S | Removes the all-or-nothing invalidation coupling. Self-healing on read. Cheapest structural win in the pass. |
| 3 | Marker-anchored idempotent installer | H1 | S–M | Fixes a re-run correctness bug in host config edits, not just ergonomics. |
| 4 | SCIP oracle as a side table, never an overwrite | R1 | M | The highest-value resolution item; the side-table framing keeps the heuristic tier as evidence. |
| 5 | Canonical verb surface + named scopes + skills | C3 | M | Ten-repo convergence; directly addresses agent-routing degradation on a wide tool list. |
| 6 | Freshness consolidated into one module + 4-value verdict | C4 | M | Eight-repo convergence; the scatter is the reported problem, and the enum makes staleness actionable. |
| 7 | Typed cancellation + active-run registry + await-before-drop | C7 | S–M | Ctrl-C during a long run currently leaves partial state; cancelled ≠ failed. |
| 8 | Edge-kind and node-kind enums decided in one place | G1, G2 | M | Forces the vocabulary decision now rather than after twenty ad-hoc kinds exist. |
| 9 | Ports for the seams that exist (store, parser, watcher) + in-memory store | C1 | M | Nine-repo convergence. Take the seam and the test double; skip the backend sprawl. |
| 10 | Per-pass pipeline with typed diffs and per-pass receipts | C2 | M–L | Converts future optionality (LSP, embeddings, new edge kinds) from core edits into pass registrations. |
| 11 | Compilation-database awareness for C/C++ | R2 | M | The single largest resolution gap for a whole language family. |
| 12 | Declared-vs-actual reconciliation + task/session identity | C9 | M | Five-repo convergence; turns provenance disputes into typed findings. |
| 13 | Glossary + CI check + metric ratchet | C12 | S | One file, one script, repo-wide effect; the ratchet keeps quality metrics honest over time. |
| 14 | Multi-renderer family incl. anonymising export | C10 | M | The `anon` exporter is a distinctive capability, not a cosmetic. |
| 15 | Structural diff via git worktree | G12 | M | Two-commit comparison without switching or stashing the user's tree. |
| 16 | Dependency-path traversal | V10 | M | Answers "how did this get here?" — needs an ecosystem/manifest pass first. |
| 17 | Lazy load + worker IPC + heap policy | P1–P3 | M | Startup latency and memory ceilings; each is independently shippable. |
| 18 | Severity + RuleConfig + suppressions (incl. unused) | V1, V2, V6 | M | Gives claim verdicts a vocabulary the wider tooling community already shares. |
| 19 | CGA-style graph attestation (merkle root + edge-bundle digest) | C8 | M–L | Verify-first. Verifiable without source access — the clearest external-facing payoff in the pass. |
| 20 | Partial paths + symbol/scope stacks | R3, R4 | L | The principled resolution model; large, and only worth it after 4 and 11 land. |

---

## 7. Parked and contingent

Shapes decided in advance for lanes that don't exist. Deciding now is most of the value.

**If embeddings land**: multi-provider engine behind one register-at-boot abstraction (contextplus);
explicit dimension negotiation with a typed mismatch receipt rather than a crash (GitNexus);
incremental embed-pass separated from retroactive embed-nodes backfill (codebase-graph); auto
`similar_to` edges from cosine with a typed weight (contextplus); spectral clustering with the
eigengap heuristic (contextplus).

**If LSP lands**: a typed LSP port with request/response types and a lifecycle manager that
restarts on file events and monitors health (serena); an LSP-to-SCIP converter so live LSP data
enters through the *same* path as SCIP files rather than a second one (infigraph); LSP surface and
cross-file passes as pipeline passes (codebase-memory-mcp).

**If a declarative extraction DSL lands**: AST → static checker → execution runner as three separate
modules, with typed parse errors and injected variables and functions (tree-sitter-graph); per-
language `.tsg` rule files shipped alongside grammars (stack-graphs). Note this is the same
strategic bet the Qwen pass flags from the same two repos — two passes, one conclusion.

**If a native kernel lands**: a sibling package with IPC to the JS shell, dual WASM/NAPI
distribution, and a dual-build discipline (colbymchenry, oxc, joern).

**If remote/multi-repo ingestion lands**: shallow clone with guaranteed cleanup keyed by URL
(repo-lens, repo-graph); a multi-repo registry file (juspay); fleet manifests with a per-SCM adapter
(roam-code); namespaced storage so federated repos share one backend (CodeGraph); an LWW fold
ordered by Lamport + device for conflicting verdicts over the same span (rag-rat).

**If agent-applied edits land**: shadow-branch restore points (contextplus); autofix as a typed
edit gated by an explicit scope grant (semgrep); minimal-splice materialisation; dry-run by default.

**If issue-tracker integration lands**: provider-neutral mirroring so a claim verdict can cite
tracker state as evidence (rag-rat).

---

## 8. What only this pass provides

The ten repos the Qwen pass skipped, and what each is worth. This is the part of the M3 corpus with
no substitute.

| Repo | Verdict | Take |
|---|---|---|
| **stack-graphs** | **Highest-value of the ten.** The academic-grade name-resolution substrate (Visser scope-graph formalism). | Symbol stack vs scope stack (R3); binding-as-path; partial paths with `blockedAt` (R4); cycles as a first-class module; arena allocation. The formal model behind what Cortex's cross-file tiers approximate. |
| **ast-grep** | Strong, concrete. The de-facto structural search and rewrite tool. | Severity enum (V1), `RuleConfig` (V2), rule buckets, case-aware glob sets, metavariable bindings (V7), suppressions including *unused* (V6), fixer/replacer. Gives claim verdicts a shared community vocabulary. |
| **gritql** | Strong on typed intermediate shapes. | `Constant` model (V4), Pattern vs Predicate (V3), query context and static definitions, declared rule effects (V5), pattern analysis, CODEOWNERS mapping. Each is small; together they give rules a type system. |
| **rag-rat** | Dense and surgical; among the most valuable single entries. | Signed hash-chained op-log with canonical CBOR (C8), `NORM_VERSION` granular invalidation (C5 — ranked #2 overall), the SCIP oracle side-table (R1 — ranked #4), dream-mode maintenance findings (V12), clone detection, LWW fold, papertrail. |
| **codeql** | Posture and convention, not mechanism. | Rule packs (V8), per-module change notes (Y4), language folders, shared-analysis library layout, per-domain CODEOWNERS. Raises posture without changing behaviour. |
| **CodeGraph** (codegraph-ai) | Deep — a real graph DB, not a wrapper. | Fluent query builder (G9), namespaced storage (G6), a published parser-API contract package, `FileInfo` parser input, split parser/graph error models, strict lint posture, declared core principles. |
| **codegraph** (colbymchenry) | Most production-featured peer for install/runtime concerns. | WAL valve (P6), WSL/platform-aware watch policy (P7), framework resolvers (C11), worktree-index mismatch detection, install-method detection and the Windows detached-upgrade dance (H8). |
| **contextplus** | Small but two genuinely distinctive items. | Shadow-history branch with restore points (C9) — reversible agent edits; `ALWAYS_IGNORE` (C6); `assertWithinRoot`; feature-anchor comments; wiki-style `[[cross-links]]` in doc comments. |
| **axon** | Small, disciplined, several near-free items. | Structural diff via git worktree (G12), read-only query guard by write-keyword regex, `MAX_TRAVERSE_DEPTH` constant, confidence tags, dead-code report, co-running serve mode (P13). |
| **signum** | Not a code-graph tool; one idea, well made. | Glossary discipline enforced by CI (C12), module manifest (Y1), doc-parity check (Y2), ADR check (Y3), anti-entropy report (V13), metric ratchet. Cheapest repo-wide quality lift in the corpus. |

Beyond the ten repos, this pass contributes **an entire layer the mechanism-focused pass does not
address**: ports and adapters, pass pipelines, CLI ergonomics, installer idempotency, packaging,
scoped tool surfaces, glossary discipline, and skill bundles. If the Qwen pass says *what Cortex
should compute*, this one says *how Cortex should be arranged so computing it stays cheap*.

---

## 9. Conflicts to resolve in a cross-model merge

Where this pass and the Qwen pass disagree, or where this pass contradicts itself. A merge must
settle these.

1. **Backend pluralism.** M3 proposes swappable graph stores from six repos (C1) and, in places,
   treats shipping a second backend as the goal. The Qwen pass, having read those backends,
   concluded every one of them is *weaker* than SQLite and put "graph-backend abstraction sprawl" on
   its do-not-absorb list. **Resolution**: take the port and the in-memory test double; do not ship
   a second production backend. The seam buys testability; the second backend buys maintenance.
2. **`semantic` and `semantica` identification.** Direct contradiction (§2, caveat 1). **Prefer
   Qwen**; void M3's two entries.
3. **Determinism.** The Qwen pass makes determinism a top-tier finding backed by eleven repos. M3
   barely mentions it, and several of its proposals cut against it — a `learned/` module,
   spectral clustering, LLM-invoked passes, wall-clock staleness thresholds. **Resolution**: any M3
   proposal touching ranking or graph content inherits the Qwen determinism checklist; `learned/`
   stays opt-in, out of the graph, and never authoritative — which infigraph itself recommends.
4. **Tool-surface direction.** M3 wants both a six-verb minimal surface (repo-graph) *and* a 60+
   typed feature surface (infigraph), in different files. **Resolution**: they compose as
   registry-plus-scopes — a small default scope over a full registry — but the decision must be
   made explicitly rather than by adopting both proposals.
5. **Where freshness code lives.** M3 proposes consolidating into a `freshen/` module (C4) *and*
   separately proposes `staleness.mjs`, `fingerprint.mjs`, `change-tracker.mjs` and
   `freshness.mjs` as distinct modules in four other entries. Pick one shape.
6. **Attestation depth.** Three different schemes are proposed — signed op-log (rag-rat), Ed25519
   attest crate (Brain0), in-toto CGA + VSA + cosign (roam-code) — plus `gh attestation` wrapping
   (Code-Index-MCP). They are not alternatives to each other so much as different layers; a merge
   should pick the layer, not accumulate all four.
7. **Claims that Cortex lacks something.** Every *verify-first* item — signing, path confinement,
   redaction, diff treeish, ignore handling — may be an audit rather than a build. Check before
   scheduling.

---

## 10. Coverage and confidence

**Evidence quality by entry**, judged on whether the study quotes concrete mechanisms:

- **High** (quotes constants, algorithms, invariants, real type shapes): rag-rat, Brain0, roam-code,
  ast-grep, gritql, stack-graphs, sense, potpie, context8, claude-context, colbymchenry/codegraph,
  axon, code-index-mcp, repo-graph, code-compress, GitNexus, agentic-codebase, repo-lens,
  react-doctor, signum, Sourcetrail, CodeGraphContext, dependency-cruiser, semgrep, tree-sitter,
  tree-sitter-graph, scip, Mentat, Aider, deepwiki-open, codebase-memory-mcp, juspay,
  Understand-Anything, CodeGraph.
- **Medium** (mixes mechanism with layout inference): Codealmanac, codebase-graph, octocode,
  infigraph, opengrok, opengrep, serena, oxc, llama_index, cognee, PraisonAI, RepoDoctor,
  treesitter-chunker, ai-code-audit, contextplus, lsif-go, joern, codeql, Consiliency/Code-Index-MCP.
- **Low / void**: `semantic2` (misidentified — void), `semantica` (reported as non-existent;
  contradicted — void), joern and codeql in part (build-system proposals with little bearing on
  Cortex's problems).

**Net**: 53 usable entries out of 55, complete coverage of the competitor index, and ten repos with
no coverage anywhere else. The pass's judgement about its own top item is worth recording verbatim
and is consistent with the ranking above: *"repo-graph is the cheapest upgrade in this set — the
6-verb naming and a marker-anchored installer could ship today without touching graph internals."*
