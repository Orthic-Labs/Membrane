# Competitor absorption synthesis — DSv4-Flash pass

One document consolidating the DSv4-Flash pass — 55 per-repo studies, 10 absorptions each — into
a deduplicated, sequenced program for Cortex.

- **Input**: 55 study files (~550 proposals, ~590 KB — the largest of the four passes).
- **Corpus**: complete, all 55 repositories.
- **Companions**: `synthesis-qwen.md` (mechanism, 45/55), `synthesis-m3.md` (architecture, 55/55),
  `synthesis-dsv4pro.md` (seam-mapping, 55/55). §2 places this one; §9 resolves the four-way
  disagreements.

Nothing here is implemented. Claims about competitor internals are as reported and have not been
re-verified against the vendored repos.

---

## 1. How to read this

| Bucket | Count | Meaning |
|---|---|---|
| **Convergences** | 13 clusters | ≥3 repos, same mechanism. §3. |
| **Distinct absorptions** | ~150 | Single-source, mechanism-level, seam-mapped. §4–5. |
| **Contingent** | ~55 | Depends on a lane Cortex doesn't have. §7. |
| **Confirmations** | ~35 | Cortex already does this; audit, not build. Marked *verify-first*. |

---

## 2. What kind of pass this is

**This is the strongest of the four, and the ranking below is weighted accordingly.**

It combines what the other three do separately: it names the competitor's mechanism at
algorithm level (like the Qwen pass), routes every item to a real Cortex file (like the DSv4-Pro
pass), and it does both in the same sentence. Three qualities set it apart:

1. **It quotes the competitor's own reasoning.** Where a repo documents *why* a design is the way
   it is, this pass carries that across — GitNexus's ambiguity-decline rationale ("first-wins is
   only sound where duplicate exports are illegal; last-wins is wrong for `try/except ImportError`
   pairs"), roam-code's measured bug ("a one-line edit passed under `mypkg/...` but blocked under
   `src/...` because the default pack only knew four layout/language pairs"), codebase-memory-mcp's
   explicit doctrine ("the absence of a flag is NOT a completeness guarantee"), gritql's fixed RNG
   seed comment, ast-grep's regression tests named after the env-leak class they prevent. That
   material is the difference between copying a shape and understanding a constraint.
2. **It reports negative space precisely.** Numbers, thresholds and cut-offs are carried verbatim:
   Jaccard 0.85 for moved-chunk detection, 0.72 embedding-similarity threshold, `QUIET_PERIOD =
   5.0` / `MAX_DIRTY_AGE = 60.0`, RRF k=60 with a `3× limit, max 300` candidate pool, 0.6 anchor
   threshold with HIGH at 0.85, complexity score `cyclomatic + 0.5·cognitive + 0.3·nesting +
   0.2·|deps|`.
3. **It gets the two disputed repos right.** `semantic` is correctly identified as GitHub's Haskell
   analysis library (dead-code by abstract evaluation, Kleene fixed-point convergence, effect
   carriers, per-language frontends) and `semantica` as the Python provenance/knowledge framework
   (checksum-chained provenance, version storage, policy engine). **Two of four passes got this
   right — Qwen and this one; M3 and DSv4-Pro both resolved GitHub org names instead of reading
   `repos/`.** That is now settled 2–2 on count but 2–0 on evidence.

**Caveats:**

- **Volume over triage.** 550 items at near-uniform depth, with only per-file sequencing tables
  and no cross-repo prioritisation. The convergence and ranking work below is the part the pass
  did not do for itself.
- **Some Cortex-gap claims need checking.** It asserts absences (no PageRank in ranking, no FTS
  lane, no diff-target query path, no per-file completeness bit). Each is marked *verify-first*.
- **Contingent items are not always flagged as such.** Several proposals assume an LLM or
  embedding lane that Cortex deliberately does not have; those are moved to §7.

---

## 3. The 13 convergences

### C1. Rename-tolerant identity: structural fingerprints and position-independent ids
*brain0 (`Fingerprint::of_node` hashes **only named node kinds with depth annotations**, never
identifier text or literals, so renaming a variable leaves the fingerprint unchanged; emits both an
exact structural hash and 3-gram shingles over the kind stream for Jaccard similarity),
treesitter-chunker (`select_node_identity` prefers a name-based `definition_id`, falling back to
position-sensitive `node_id` only to disambiguate overload collisions — "distinct definitions never
share a Boundary id without making every node id position-sensitive"), codebase-memory-mcp (MinHash
body fingerprints + LSH `SIMILAR_TO` pass), Understand-Anything (deterministic `kind:path:name`
node ids with edge-key dedupe), Sourcetrail (serialized `NameHierarchy` as the dedupe key),
serena (`NamePath` matcher), johnhuang (`relative_path::symbol_name` ids), rag-rat (normalised
token multisets).*

Eight repos. Cortex's `contentHash` answers "is this byte-identical"; none of these replace it —
they add a **second identity** answering "is this the same symbol". The pairing is the point.

**Absorb**: a structural fingerprint (kind-stream + depth, plus shingles) and a name-based
`definitionId` stored alongside `contentHash`, with the overload-collision fallback rule.

### C2. Freshness as a typed state machine where "unknown" ≠ "fresh"
*Understand-Anything (closed union `fresh | dirty | stale{behind|ahead|diverged} | unknown{reason}`
computed from two `merge-base --is-ancestor` checks and `rev-list --left-right --count`, with the
doctrine stated in the docstring: "Unknown is intentionally distinct from fresh: if Git metadata
cannot be read, callers should warn softly rather than imply the graph is current"),
Code-Index-MCP (`FreshnessVerdict` FRESH / STALE_COMMIT / STALE_AGE / INVALID from commit ancestry
**and** age; plus a readiness classifier with ~10 states each carrying a remediation string),
sense (read-repair — a per-query stale sweep that inline re-indexes so edit-then-query is honest
before the debounce fires), roam-code (unreadable files stay in the denominator), signum (staleness
as hash-of-declared-upstreams with warn/block policy).*

**Absorb**: the closed union with reason codes; `unknown` never renders as clean; read-repair on
query; per-state remediation strings; and roam's arithmetic rule — a coverage claim carries the
set it could not read.

### C3. Change classification: not all edits deserve re-extraction
*Understand-Anything (content hash + structural signature → `NONE | COSMETIC | STRUCTURAL` with a
**named details list** — "params changed: Y", "significant size change: F (10 → 40 lines)" — and a
conservative fallback: if either side lacks structural analysis, any content change is STRUCTURAL),
codebase-memory-mcp (mtime+size first, re-hash only candidates that pass the cheap check),
react-doctor (stat-first with hash-repair), Code-Index-MCP (`--find-renames`, cost estimation,
flip to full at ≥50% changed ratio or core-dir hits), treesitter-chunker (identity-keyed diff with
moved-detection at SequenceMatcher > 0.85 and per-change confidence: 1.0 add/delete, 0.9 modified,
0.95 moved), axon (four-tier watch: file-local immediate, global after 5s quiet, embeddings on
dirty, coupling on HEAD change — with `MAX_DIRTY_AGE = 60.0` as a **starvation guard** so
continuous writes can't defer the global pass forever).*

Six repos. Axon's tier split with the starvation guard is called out as the cleanest incremental
discipline in the corpus.

**Absorb**: cosmetic-vs-structural classification with named details; cheap-check-then-confirm
change detection; the tiered watch with a max-dirty-age guard; identity-keyed diff with moved
detection and per-change confidence.

### C4. Store hygiene: batching, dedupe maps, atomic publish, typed failure
*Sourcetrail (`InsertBatchStatement` precompiles INSERT statements at batch sizes N, N/2 … 1 to fit
SQLite's 999-variable cap and greedily fills the largest that fits — a documented ~2× speedup —
plus index-time temp dedupe maps keyed serializedName→id, `(source,target,type)`→edge,
sorted-span-tuple→location), Code-Index-MCP (staged rebuild in a temp DB → validate durable rows →
**quarantine** the old DB and its `-wal`/`-shm` sidecars timestamped for forensics → `os.replace`
atomic swap → record provenance after → verify post-swap; plus classified storage failures
flipping the store read-only with typed diagnostics on disk-full/IO error), colbymchenry
(edge identity as a UNIQUE index `(source, target, kind, IFNULL(line,-1), IFNULL(col,-1))` — dedupe
in the schema, not application logic; plus WAL checkpoint valve because default auto-checkpointing
rewrites ~95% of hot pages during bulk index), CodeGraphContext (per-kind MERGE keys, NULL
normalised to `-1` sentinels, node rows deduped but **relationship rows never** — two CALLS edges
from one caller at different sites are distinct; writer lock + backoff because embedded DBs aren't
thread-safe), codebase-memory-mcp (generated-column indexes `json_extract(properties,'$.url_path')`
and a `(project, target_id, type)` reverse-edge index), Sourcetrail again (storage version stamp
with migration-or-rebuild at open).*

Six repos. This cluster is the highest performance-and-safety return per line in the whole corpus.

**Absorb**: batched inserts with halving statement sizes + in-memory dedupe maps; edge identity as
a UNIQUE index; staged-build → quarantine → atomic swap → verify; classified storage failures with
a read-only degrade; generated-column indexes; a storage version stamp with idempotent migrations.

### C5. Resolution as a documented ladder with per-edge confidence and reasons
*CodeGraphContext (9 tiers with explicit scores — 1.00 self/super, 0.95 same-file, 0.90 unique
short name, 0.85 qualified import, 0.70 FQN substring, 0.25 alphabetical-first, 0.08 same-file
fallback — mapped to `EXTRACTED | INFERRED | AMBIGUOUS` labels, and **every skip records a
diagnostics entry** with reason, caller, file and line instead of disappearing; plus overload
disambiguation by arity filter then argument-type scoring: exact 4, mutable-equivalent 3,
collection-supertype 1, incompatible −1), axon (same-file exact 1.0 → import-resolved 1.0 →
receiver-method 0.8 → global fuzzy 0.5 with a ~90-entry builtin **blocklist applied before**
resolution so short common names never produce fuzzy edges), codebase-memory-mcp (per-call
`strategy` + `confidence` + `reason`), GitNexus (strict single-return type resolution deliberately
narrower than search: shadowing fails fast, ambiguity returns null, wildcard origins excluded as
too loose), sense (scope-aware preference with language lanes), infigraph (learned store: when
SCIP disagrees with the heuristic, record the correction with confidence +0.1 per repeat, stored
**outside the graph DB** so rebuilds don't lose it).*

Six repos, one shape, and two refinements nobody else surfaces: the **pre-resolution blocklist**
and the **learned-correction store that survives rebuilds**.

**Absorb**: numeric confidence per tier with a derived label; a `resolution_diagnostics` stream
recording every failure with context; the builtin blocklist; strict-vs-search separation; the
learned-correction replay store.

### C6. Termination and boundedness as engineering, not hope
*GitNexus (Tarjan SCC in reverse-topological order, per-SCC **bounded fixpoint capped at the SCC's
edge count**, early stop on no-progress, and edges still unlinked at the cap marked
`linkStatus: 'unresolved'` rather than guessed; re-export closures precomputed with a `|SCC|+1`
cap), stack-graphs (`SimilarPathDetector` keys frontier states by `(start, end, stack-signature)`
and keeps the best path per key rather than the first, with the honest note that full cycle
detection is the Halting Problem), gritql (`Limit` with an atomic invocation counter shared
across threads via compare-exchange, plus a fixed RNG seed for reproducibility), ast-grep
(kind-set bitset pruning before any matcher runs), axon (per-commit cap of 50 files in coupling
analysis, min 3 co-changes), opengrep (parallel target scheduling by decreasing size),
codealmanac (depth-32 ancestor cap on cycle rejection).*

**Absorb**: SCC + bounded-fixpoint + mark-unresolved as the standing shape for any cross-file
convergence pass; frontier-state dedupe by signature rather than visited-set; shared budgets via
atomic counters; fixed seeds wherever ordering could vary.

### C7. Grounded claims with typed lifecycle: cite → verify → supersede
*agentic-codebase (a full pipeline: `extract_code_references` from claim prose → citation cascade
with declining strength (exact `Direct` → qualified-containment `Strong` → case-insensitive
`Partial` → Levenshtein "did you mean") → `TruthMaintainer` re-checks claims and classifies
`Valid | Stale | Invalidated | Deleted` with the precise rule *citations still resolvable but claim
ungrounded → Stale, not Deleted* → hallucination detection with a typed taxonomy `NonExistent /
WrongBehavior / WrongSignature / WrongLocation / Outdated / InventedFeature`, each carrying
`reality` + `evidence` + severity, aggregated into a score with a `safe_to_use` gate),
roam-code (claim lifecycle with `stableId`, `supersede(new, oldIds)` recording the chain, `refute`,
`markStaleIfExpired`), codealmanac (typed `sources` per claim: FILE / WEB / COMMIT / PR / ISSUE /
CONVERSATION / WIKI / MANUAL, each with its own target-field grammar), potpie (`TruthClass` stamped
on every durable claim), semantica (checksum-chained provenance with documented field exclusions),
mentat (interval-scoped checksums re-read before applying).*

Six repos. agentic-codebase's chain is Cortex's own evidence doctrine written as executable code.

**Absorb**: the citation-strength cascade; the four-state truth lattice with the Stale-vs-Deleted
rule; typed claim sources with per-type target grammars; supersede/refute as stored operations.

### C8. Verdicts as closed enums with precedence and allowlist-on-pass
*roam-code (`pass | pass_with_warnings | needs_review | blocked` with most-severe-wins precedence;
reasons are **objects `{code, ...context}`, never prose-only**; only a check recorded as `pass`
satisfies a requirement — documented as failing closed because a denylist once let `skipped` /
`null` / `"Unverified"` through; a `change_set_unanalyzable` hard gate so "0 of 0 required checks
ran" cannot print the same sentence as a real pass; unmapped statuses **raise** so a new failure
mode can't go quietly green; plus a verification contract publishing `_meta.unmatched_changed_files`
— the denominator), signum (`AUTO_OK / HUMAN_REVIEW / AUTO_BLOCK` derived from risk × coverage by
explicit implication rules the checker enforces; every verifier emits
`{check, status, summary, findings}` with **exit 0 = check completed including block, exit 1 =
infra error**), semgrep (typed exit-code taxonomy), ast-grep (five-level severity driving exit
codes), Code-Index-MCP (readiness states with remediation).*

**Absorb**: the closed verdict enum with precedence; allowlist-on-pass; reason objects with codes;
the unanalyzable-input hard gate; unmapped-status-raise; the published denominator; and the
status-vs-infra-error exit-code split.

### C9. Budget assembly with lane priority, write-back and named drop-steps
*mentat (residual computed as `max_tokens − prompt − meta − already-included − buffer`, then
retrieval fills exactly that, and the admitted refs are **written back into the visible selection**
so auto-context is inspectable), aider (binary search over ranked-prefix length until within 15% of
budget — O(log n) renders; plus a sampled token estimator taking every `n//100`-th line and
extrapolating by byte ratio), code-compress (one-shot assembly with lane priorities),
roam-code (budget reduction via **named drop-steps**, each recording its own reason code, so what
lost the budget race is attributed to a step rather than a blanket omission; plus per-dimension
completeness classification), praisonai (compaction anti-thrashing gate — skip when savings are too
low **and say so**; tool-call/result pair-boundary snapping so an atomic unit is never split;
dynamic budget with explicit reservations so context can't starve the response), contextplus
(bounded excerpts everywhere with documented caps).*

**Absorb**: residual-budget fill with lane priority and write-back; binary-search prefix fitting
with a sampled estimator; named drop-steps carrying reason codes; pair-boundary snapping; the
anti-thrashing gate.

### C10. Git-derived signals static analysis cannot produce
*axon (`git log --name-only --since=6 months` → co-change matrix, skip commits touching >50 files
as merge/reformat noise, keep pairs with ≥3 co-changes, degrade to empty when not a repo),
agentic-codebase (version archaeology: commit-message classification into
BugFix/Feature/Refactor/Performance, per-unit churn and author count, then **`infer_phase`** →
`Active` (<30 days or >10 changes) / `Maturing` / `Stable` (>0.8 stability, >180 days) / `Decaying`
(>0.6 bugfix ratio, >90 days), with `explain_why` composing the evidence into prose),
brain0 (risk factors incl. churn, plus reviewer detection from `Reviewed-by:`/`Acked-by:` commit
trailers — an empty list is the basis for an **"AI-assisted but unreviewed"** signal),
roam-code (recency buckets deliberately coarse to dodge mtime-touch sensitivity),
Code-Index-MCP (rename tracking via `--find-renames`).*

**Absorb**: bounded co-change edges with the two guardrails; evolution phase and churn as node
attributes; reviewer-trailer parsing as a governance signal.

### C11. Every pipeline is a named, ordered, skippable set of passes
*axon (eleven numbered phases, each a module with one entry point consuming the previous phase's
typed dataclass, with a test file per phase so "regressions land on the exact pass that broke"),
codebase-memory-mcp (~16 named passes each emitting its own edge type, incremental mode running a
subset, every pass idempotent and mergeable), Understand-Anything (fingerprints → batches → merge,
hierarchical), semantic (per-language frontends lowering to one core), ai-code-audit (typed state
as the workflow contract with map-reduce fan-out and an explicit combine step), context8 (pipeline
context with counters and error accumulation), Sourcetrail (task scheduler with
parallel/sequence/selector groups and a shared blackboard).*

**Absorb**: passes as `{id, run(input) → typedDiff}` in an explicit DAG; a `producer` pass id
tagged on every edge so an incremental refresh can skip passes whose inputs are unchanged; one test
file per pass.

### C12. Untrusted input handling: parse ladders, bounded parsing, injection guards
*deepwiki-open (structured-output recovery ladder: strip fences → extract block → **on truncation,
salvage from the opening tag and synthesise the close** → strip control chars → escape bare `&` →
strict parse → per-element regex fallback → typed failure, raising only when no block exists),
roam-code (`loads_bounded`: **rejects duplicate keys** — a classic LLM corruption and a real attack
shape — plus a nesting-depth cap), Understand-Anything (fence strip → extract → **per-item
validation with drop-and-continue**, never trusting shape wholesale, never throwing on one bad
item), axon (`escape_cypher` stripping comment and statement-injection sequences), brain0
(secret detectors running **before persistence and embedding**, replacing with
`[REDACTED:aws_access_key]` kind tags, with `scan_kinds()` returning only kinds for a DLP audit
trail), agentic-codebase (shareability classifier that excludes **content hashes** because they
fingerprint code), treesitter-chunker (symlink-escape hardening: refuse symlinked dirs and anything
resolving outside root, documented as anti-planted-symlink).*

**Absorb**: the recovery ladder as a shared helper; bounded JSON with duplicate-key rejection;
per-item validate-and-drop; detector-based redaction at ingest with kind-only auditing; the
symlink-resolve-and-verify rule.

### C13. Fusion, demotion and ranking as small documented tables
*axon (RRF `Σ weight/(k+rank)`, k=60, candidate pool `3× limit` capped at 300, FTS falling back to
fuzzy when empty, first-occurrence-per-node wins), octocode (RRF with the rationale in the code),
codebase-graph (test-file demotion in **two passes** — pre-pool and post-rerank — plus wide-pool
retrieval with a documented floor `max(limit×4, 60)`), Code-Index-MCP (post-FTS rank adjustment
table: `__init__.py` +0.45, test files +0.7 for symbol-precise queries, filename-token matches
−0.22 each capped at 3, path-token −0.08 capped at 4, gated on a query classifier),
aider (identifier salience multipliers: informative names ≥8 chars ×10, `_private` ×0.1,
idents with >5 definers ×0.1, query-mentioned ×10, sqrt-damped reference counts, and self-edges
weight 0.1 so PageRank converges), code-compress (kind-boosted bm25), roam-code
(`importance × recency × log2(1+refs)` with coarse recency buckets and a memoised stat cache).*

**Absorb**: RRF k=60 with a capped candidate pool for lane fusion; the two-pass demotion pattern;
a documented post-rank adjustment table; identifier-salience multipliers; log-damped count bias.

---

## 4. Distinct absorptions — the high-value set

Beyond the convergences. Every item names a mechanism, not a module.

### 4.1 Graph capability

| # | Absorption | Source |
|---|---|---|
| G1 | **Personalized PageRank over def/ref edges.** Files as nodes, referencer→definer edges weighted `mul · sqrt(num_refs)`, personalization vector from query-matched identifiers and pinned anchors, rank redistributed across out-edges to score `(file, ident)` pairs. Named as the single biggest ranking upgrade available from any repo in the corpus. *(verify-first: confirm Cortex has no graph-global rank today.)* | aider |
| G2 | **Library-identity-first edge classification.** Classify call edges by matching library identifiers inside the **resolved qualified name** (`r.get("/api")` → QN `…requests.api.get` → `HTTP_CALLS`), never by callee name — `get`/`post`/`send` are meaningless in isolation. Two-level cascade (library → edge kind, method suffix → verb) with route-registration checked **before** HTTP clients so `gin.GET` isn't an outbound call. Memoised per resolved QN. | codebase-memory-mcp |
| G3 | **Discriminating string-literal extraction.** A route-literal predicate that must start with `/`, must not contain `://`, must not be a filesystem path (checked against `/etc /usr /var /home …` roots, hidden segments `.ssh .aws .kube .env`, hard extensions `.cfg .pem .service`), must not be a path-builder call — with `/api`, `/v1/`, `/graphql`, `/health` marker segments overriding the extension check. Makes "impact of changing `/api/users`" and "who reads config key X" queryable without a model. | codebase-memory-mcp |
| G4 | **Channels and infra bindings as graph citizens.** YAML/HCL subscription configs → `(source, target_url, broker)` bindings; `emit()`/`on()` calls → channel participation matched by channel name across files. Async architecture becomes indexable — a dimension no other repo in the corpus has. | codebase-memory-mcp |
| G5 | **Occurrence relation as its own table.** `(elementId, locationId)` many-to-many: nodes and edges are unique rows, every appearance is an occurrence. "All locations of X" and "all elements at Y" become index hits on a tiny join table instead of span duplication. | Sourcetrail |
| G6 | **Typed locations.** `TOKEN | SCOPE | QUALIFIER | SIGNATURE | COMMENT | ERROR` with start/end pairing, `contains()`, and a `locationsForLines(file, lo, hi)` query — so declaration-vs-usage evidence is distinguishable and nesting queries are mechanical. | Sourcetrail |
| G7 | **NameHierarchy.** A name is a vector of typed elements plus a delimiter, not a string — with `getRange`, signature-aware accessors, and serialization used as the node dedupe key. Unlocks overload disambiguation and clean renames. | Sourcetrail |
| G8 | **Edge-type bitmasks with family classification.** `LAYOUT_VERTICAL = INHERITANCE|OVERRIDE|TEMPLATE_SPECIALIZATION` separates hierarchy from usage families, so impact queries can weight them differently and consumers filter by mask. | Sourcetrail |
| G9 | **Element components table.** `(elementId, type, data)` — per-node attributes (access modifier, entry-point, generated) without a schema migration per attribute. | Sourcetrail |
| G10 | **Layer detection from ordered path-segment patterns.** `routes/controller/handler/api` → API, `service/usecase` → Service, `model/entity/repository` → Data, … with plural forms, first-match-wins, unmatched → Core. Deterministic, zero-token, queryable today; any model refinement is an override on a working baseline. | Understand-Anything |
| G11 | **Kind→node-type map for non-code artifacts.** SQL tables/views → `table`, Terraform resources → `resource`, compose services → `service`, GraphQL routes → `endpoint`, unknown → `concept`, each with child nodes under its file and rich summaries. Config files stop being opaque blobs. | Understand-Anything |
| G12 | **Topological tour without an LLM.** Kahn's algorithm from in-degree-0 entry points, grouped by layer, concepts last — with a head-pointer queue rather than `shift()` (the perf fix is documented in the code) and unreached nodes appended so cycles can't drop files. | Understand-Anything |
| G13 | **Dead code with an exemption catalogue and a correction pass.** Zero-inbound-edge flagging, then exempt entry points, exports, constructors, tests, dunders, `__init__.py` public API, framework decorators, enum bases — then a correction pass that **un-flags** a method if a base class has it alive. Downstream analysis revising upstream verdicts is the reusable shape. | axon |
| G14 | **Open-world dead-code arbiter with per-language fail-closed mention harvesting** — if a language's mention harvester isn't sound, the arbiter declines rather than reporting. | sense |
| G15 | **Community detection over a weighted heterogeneous projection** — CALLS 1.0, heritage edges 0.5, Leiden, labels generated from shared parent directory (else top-2 joined). | axon |
| G16 | **Complexity as stored node metadata.** Cyclomatic (weighted node-kind table), cognitive (nesting-weighted), max nesting, call/branch/loop counts, dependency set; score `cyclomatic + 0.5·cognitive + 0.3·nesting + 0.2·|deps|` with per-kind thresholds (function 10.0, class 50.0). The cheap proxy for "hard to change safely". | treesitter-chunker |
| G17 | **Nearest-tests discovery.** Pattern set across 8 languages, scoring name-match +2.0 / content-match +3.0 / test-dir +1.0, sorted by score then path length, capped at 50 — with the symlink hardening from C12. | treesitter-chunker |
| G18 | **Bi-temporal edges + relevance lifecycle.** `valid_at`/`invalid_at`/`forget_after` with point-in-time queries; touch bumps relevance, decay reduces it for idle entities, prune removes below threshold. | codebase-graph |

### 4.2 Evidence, claims and verification

| # | Absorption | Source |
|---|---|---|
| E1 | **Per-file parse-completeness flags with error ranges** — and the doctrine stated in the source: *"the absence of a flag is NOT a completeness guarantee. Callers should treat a flagged file as 'prefer grep here', never treat an unflagged file as provably complete."* Sourcetrail's counterpart sets `complete = (errors_for_file == 0)` per file. | codebase-memory-mcp, Sourcetrail |
| E2 | **Stale-reference extraction with prose-vs-code modes.** Four ref kinds (markdown inline, reference-style, HTML `href/src`, backtick paths); **in source files only backtick paths are extracted** because `[text](url)` collides with regex character classes and floods findings. Plus decoration stripping (`<>`, trailing punctuation, fragments, percent-decode) and an anchor cache for cross-file anchors. | roam-code |
| E3 | **Rename hints with a provider chain and a triple-guard auto-fix.** git-history renames (HIGH) → symbol-graph similarity → basename match (MEDIUM/LOW by uniqueness); anchor hints via `max(char-ratio, token-Jaccard)` ≥ 0.6. `rewriteIsSafe` refuses when the original already resolves, when the rewrite doubles a directory chain, or when **the new URL doesn't resolve either** — never replace one broken link with another. | roam-code |
| E4 | **Branch-diff filtering for findings.** Keep a finding only when the source file changed on this branch or the target was deleted/renamed on it, with base resolution falling back `origin/main → main → master → HEAD~1`. What makes a gate practical on a repo with historical changelog noise. | roam-code |
| E5 | **Section projection with stable heading paths.** Markdown parsed into sections carrying `heading_path` ("Installation › Windows") and an ordinal; **the section, not the file, is the evidence granularity**, so a claim's anchor survives edits elsewhere in the document. | codealmanac |
| E6 | **Relative page-link resolver.** Accepts only relative, extensionless, non-anchor, non-URL hrefs; resolves `./x`, `../y`, `A/B` against the page base with README and folder-landing special cases — yielding typed doc-to-doc edges and powering broken-link checks. | codealmanac |
| E7 | **Graph-integrity health findings**, not counts: orphan pages, **dead file refs verified against the filesystem**, broken page links, broken cross-wiki links, empty topics, missing source citations, unused sources, duplicate sources. | codealmanac |
| E8 | **Interval algebra with overlap-as-error.** `earliest_deadline_sort` (end-ascending, start-descending) that **returns false on overlap**, plus `get_top_level_intervals` via reverse sweep. Compositional: two sorted sets merge or fail loudly. | gritql |
| E9 | **Effects collected then resolved.** Matchers record intended mutations as effects; application filters by range, sorts by earliest deadline, **errors on any overlap**, and extracts only top-level intervals (outer wins). Overlapping writes are a hard error, not last-wins. | gritql |
| E10 | **Binding history, not just current binding** — every assignment a candidate match made is recorded, so a variable's full range set is reportable as provenance. | gritql |
| E11 | **Chunk validation framework** — span containment, byte/line consistency, identity uniqueness, parent-child coherence, no span outside file bounds, run at pack build. | treesitter-chunker |
| E12 | **Deterministic acceptance guards on model output, retried once** — non-empty, no paragraph break, ≤110 words, exact sentence count; and `memory_unverifiable` decided **by code, never by a model**. | rag-rat |
| E13 | **Forward-compatible op decoding** — an op whose kind this binary doesn't know decodes to `Unknown { tag, raw }` with raw bytes retained, so an old reader round-trips a new writer's data instead of dropping it. | rag-rat |
| E14 | **Doc-parity as required-substring assertions** with named error codes — the cheap CI version of full regeneration. | signum |

### 4.3 Runtime, safety and ops

| # | Absorption | Source |
|---|---|---|
| O1 | **Crash-supervisor quarantine.** Files that fault or hang are appended to a quarantine list; the extractor short-circuits them to empty results so no pass can crash on them again, and the skip is **reported with a phase** (`crash` vs `hang`) via a parallel-safe marker journal. Converts "index crashed on file X" from a wall into a note. | codebase-memory-mcp |
| O2 | **Two-phase commit primitive** with the invariant "after return, either BOTH succeeded or NEITHER has visible side effects", and a **durable crash ledger** for cross-store cleanup: pending remote deletions persisted **in the same transaction** as the local delete, drained later with orphan-vs-revived partitioning and per-group failure isolation. | Code-Index-MCP |
| O3 | **Circuit breaker with a neutral outcome.** Closed→Open on consecutive failures, cooldown, then Half-Open admitting **exactly one probe** (concurrent callers get `CircuitOpenError` with `retryAfterMs`). Three-way classification — success / failure / **neutral** (cancellations, timeouts, 4xx) — so a single timeout can't permanently park the breaker half-open. | GitNexus |
| O4 | **Degrading cache ladder** — corrupt cache → delete and recreate → fall back to an in-memory dict → never fail the run. Cache keyed by mtime for tag extraction and namespaced by a `CACHE_VERSION` bumped when the extraction shape changes. | aider |
| O5 | **Fail-open utility layer** — `failOpenReadJson`, `atomicWriteJson` as the default I/O primitives. | react-doctor |
| O6 | **Bounded-resource watcher with degrade latches** — explicit resource bounds with documented degradation when exceeded. | colbymchenry |
| O7 | **Offline-safe tokenizer probe** — probe once in a thread, fall back permanently on failure, never retry per-call. | praisonai |
| O8 | **Task registry with submit-join, semaphore, TTL reaping and cache short-circuit** — a concurrent trigger for the same key **joins** the in-flight task rather than duplicating it. | deepwiki-open |
| O9 | **Serialized access for embedded DBs** — one writer lock, retry with backoff on lock contention, explicit close on shutdown, documented "not thread-safe; serialize all access" invariant. | CodeGraphContext |
| O10 | **Striped mutex, 512 stripes** for fine-grained concurrent index locking. | lsif-go |
| O11 | **Generated-file detection**, persisted at index time from two separate signals (path convention + content banner) so readers never re-derive it. | colbymchenry |
| O12 | **Self-gitignored data directory** and a pathspec that excludes the tool's own state (`:(exclude).agent`) from every git read — so Cortex's own files can never make the graph look dirty. | serena, Understand-Anything |
| O13 | **NUL-delimited git plumbing** (`-z`) with a hard timeout, buffer cap and typed `GitCommandError` carrying exit code and timed-out flag. | Understand-Anything |
| O14 | **Test-environment network tripwire** — an assertion that hard-fails any network call inside a test environment, with an explicit benchmark opt-out. | mentat |

### 4.4 Extraction and language

| # | Absorption | Source |
|---|---|---|
| X1 | **Kind-set bitset pruning.** Every matcher advertises the node kinds it can match; the DFS skips any candidate whose kind isn't in the set before invoking the matcher. Combinators derive composite sets by algebra — And/All intersect, Or/Any union, Not drops to unconstrained. The universal performance primitive for structural queries. | ast-grep |
| X2 | **Transactional match environments** — probe on a borrowed copy, commit only on success. Regression tests are named after the leak classes they prevent (`test_not_does_not_leak_env`, `test_or_revert_env`). | ast-grep |
| X3 | **Match-strictness modes** — `Smart | Cst | Signature | Template` — one pattern language with multiple precision dials rather than a second DSL. | ast-grep |
| X4 | **Minimal node trait with default traversal** — a ~10-method interface with `ancestors`/`dfs`/`next`/`child` implemented once as defaults, so swapping a syntax backend is a one-adapter change. | ast-grep |
| X5 | **Position-sorted, non-overlapping edit application** with nested-match skipping (outer wins). | ast-grep |
| X6 | **Scope-stack qualified paths with scope-only frames.** Rust `impl_item` contributes scope but doesn't emit, so a type with several impl blocks doesn't collide; `mod_item` likewise; generics stripped. Directly reduces duplicate symbol rows for Rust. | brain0 |
| X7 | **Hand-rolled gitignore matcher with correct `**` segment semantics** — negation, dir-only, root-anchored, character classes, and `**` bridging whole segments only (`docs/**` doesn't swallow `docstring.py`), last-match-wins over merged defaults. ~150 lines with tests. Paired with **prune-ignored-dirs in place during the walk**. | CodeGraphContext |
| X8 | **Generic-file node tier** — `.toml`, `.yaml`, `.md`, `Dockerfile`, `Makefile` become minimal File nodes rather than "no parser found" warnings. | CodeGraphContext |
| X9 | **Byte-offset source slicing** — read exactly the symbol's bytes, never the whole file. | code-compress, johnhuang |
| X10 | **Three-way subtree comparison for changed ranges** and input-edit application on persistent trees — the runtime's own answer to "what do I re-extract". | tree-sitter |
| X11 | **Error costs as an integer-ordered metric** and language ABI versioning with a compatibility range. | tree-sitter |
| X12 | **Static checker with quantifier-aware flow** and **locality semantics as a soundness contract** for any extraction DSL. | tree-sitter-graph |
| X13 | **Markdown fallback chunker with true byte offsets** via a line→byte prefix map (the code documents the bug where section chunks used `byte_start=0`). | treesitter-chunker |
| X14 | **Fallback strategy family** — line-based, sliding-window, markdown, log — selected by file-type detection, so "which fallback produced this evidence" is answerable. | treesitter-chunker |
| X15 | **Callable-over-value tie-break** — when defs share a name, prefer Function/Class/Interface over Variable/Property, because TS emits both for `const fn = () => {}`. | GitNexus |
| X16 | **Ambiguity-decline on re-exports** — a name published to two targets is **dropped**, not guessed, because first-wins is only sound where duplicate exports are illegal and last-wins is wrong for `try/except ImportError` pairs. | GitNexus |
| X17 | **WeakMap-memoised indexes keyed on frozen input arrays** — zero invalidation cost when inputs are documented immutable. | GitNexus, colbymchenry |
| X18 | **Var-hoisting binder with scope-ancestor walk**, and **rules-as-data with full metadata and schema**. | oxc |
| X19 | **Table-driven language-spec extractors** with per-language "voices" as the extension surface. | sense |
| X20 | **ABI-versioned extractor contract** — a per-language extractor ABI version folded into a global extractor version, so any adapter change provably invalidates every cached extract. | signum |

---

## 5. Cheap, self-contained lifts

Items the pass describes precisely enough to implement in hours.

1. **Sampled token estimator** — every `n//100`-th line, extrapolate by byte ratio (aider).
2. **Identifier salience multipliers** — the documented table (aider).
3. **Special-files always-include tier** — ~150 curated config/manifest paths (aider).
4. **Interval algebra + `path:1-10,20-30` grammar** with whole-file supersede (mentat).
5. **Edge identity as a UNIQUE index** with `IFNULL(line,-1)` (colbymchenry).
6. **Reverse-edge index** `(project, target_id, type)` (codebase-memory-mcp).
7. **Generated-column indexes** on JSON properties (codebase-memory-mcp).
8. **Batched inserts with halving statement sizes** (Sourcetrail).
9. **`complete = (errors_for_file == 0)`** per-file bit (Sourcetrail).
10. **Edge-type bitmask families** (Sourcetrail).
11. **RRF k=60 with `3× limit` pool capped at 300** (axon).
12. **Post-FTS rank adjustment table** (Code-Index-MCP).
13. **Two-pass test demotion**, pre-pool and post-rerank (codebase-graph).
14. **Builtin blocklist before resolution**, ~90 entries (axon).
15. **Coupling guardrails** — >50-file commit skip, ≥3 co-changes (axon).
16. **Layer segment table** with plural forms and Core default (Understand-Anything).
17. **NUL-delimited git wrapper** with timeout and buffer cap (Understand-Anything).
18. **Self-excluding pathspec** for Cortex's own state dir (Understand-Anything).
19. **Bounded JSON loader** — duplicate-key rejection + depth cap (roam-code).
20. **Reviewer-trailer parsing** from HEAD commit messages (brain0).
21. **Root-only manifest detection** at depth ≤ 2 (repo-lens).
22. **Analyzer empty-result default** — always well-formed, never partial (repo-lens).
23. **Fixed RNG seed** on any state object with tie-breaking (gritql).
24. **`maxDepth` cap and `dot:true` with `.git` pruned** in the walker (repo-lens).
25. **Test-environment network tripwire** (mentat).

---

## 6. The ranked program

Top 22 by (leverage × evidence quality) ÷ effort. S ≤ 1 day, M ≤ 1 week, L > 1 week.

| Rank | Item | Ref | Effort | Why |
|---|---|---|---|---|
| 1 | Cheap-lift register (all 25) | §5 | S–M | Days of work; several close real gaps; no design risk. |
| 2 | Batched inserts + dedupe maps + edge UNIQUE index | C4 | M | Highest performance-per-effort in the corpus; dedupe moves into the schema. |
| 3 | Staged build → quarantine → atomic swap → verify | C4 | M | Extends "readers see complete generations" from rows to the whole store file. |
| 4 | Freshness state machine with `unknown ≠ fresh` + read-repair | C2 | M | Makes edit-then-query honest and staleness actionable. |
| 5 | Per-edge resolution tier + confidence + diagnostics stream | C5 | M | Six-repo convergence; every unresolved reference becomes a recorded fact. |
| 6 | Per-file parse-completeness flags with error ranges | E1 | S | Small; carries a doctrine Cortex already holds in prose. |
| 7 | Cosmetic-vs-structural change classification | C3 | M | Stops comment edits costing full re-extraction; details list is a finding. |
| 8 | Structural fingerprints + `definitionId` alongside `contentHash` | C1 | M | Rename-tolerant identity; unlocks cross-version evidence linking. |
| 9 | Personalized PageRank over def/ref edges | G1 | M | Named the single biggest ranking upgrade available. *(verify-first)* |
| 10 | Closed verdict enum + allowlist-on-pass + published denominator | C8 | M | "No false clean" made arithmetic; the unanalyzable-input gate is the sharp edge. |
| 11 | Tiered watch with starvation guard | C3 | M | Cleanest incremental discipline in the corpus; correct under continuous writes. |
| 12 | Budget assembly: residual fill, write-back, named drop-steps | C9 | M | Turns omissions from a blanket list into per-step attributed reasons. |
| 13 | Grounded-claim lifecycle: citation strength → truth lattice | C7 | M | Cortex's evidence doctrine as executable code. |
| 14 | Bounded fixpoint + SCC ordering + mark-unresolved | C6 | M | Termination guarantee for cross-file convergence; no guessing at the cap. |
| 15 | Builtin blocklist + callable-over-value + ambiguity-decline | C5, X15, X16 | S | Three small rules that prevent whole classes of wrong edges. |
| 16 | Occurrence relation + typed locations | G5, G6 | M | Structural "usages" lane; declaration-vs-usage becomes distinguishable. |
| 17 | Crash-supervisor quarantine | O1 | M | A crashing file stops being a wall. |
| 18 | Detector-based redaction at ingest with kind-only audit | C12 | S | Hardens the trust model mechanically, including the content-hash exclusion. |
| 19 | Co-change edges + evolution phase + reviewer trailers | C10 | M | Signals static analysis cannot produce, all bounded. |
| 20 | Library-identity edge classification + string-literal typing | G2, G3 | M | Route/config/env impact queries without a model. |
| 21 | Passes as a registered DAG with `producer` tags | C11 | M–L | Makes incremental pass-skipping and per-pass tests possible. |
| 22 | Section projection + page-link resolver + graph-integrity checks | E5, E6, E7 | M | Drift-resistant doc evidence plus real doc-to-doc edges. |

---

## 7. Contingent

**If a semantic/embedding lane lands**: model-and-dimension metadata recorded so a model change
triggers full re-embedding (axon); embedding batch validation before any write (claude-context);
provider abstraction with runtime dimension detection; crash-resilient RAM-aware checkpointed
embedding (CodeGraph); spectral clustering with the eigengap heuristic (contextplus);
auto-similarity edges at a documented threshold; bounded batched embedding with lazy/eager modes.

**If an LLM-assisted surface lands**: the recovery ladder (deepwiki-open); bounded JSON with
duplicate-key rejection (roam-code); per-item validate-and-drop (Understand-Anything); deterministic
acceptance guards with one retry (rag-rat); per-item retry with error-placeholder so a batch stays
total (deepwiki-open); versioned prompt templates; `strip_think` handling; token-limit pre-check
with context-free retry.

**If dataflow lands**: shape-based field-sensitive taint and interprocedural taint via function
signatures (opengrep); type-tracking as a step algebra with forward/backward trackers, and the
Taint → DataFlow → AccessPath layering (codeql); effect classification with transitive propagation.

**If a structural query language lands**: kind-set pruning and combinator algebra (ast-grep);
scoped binding registry with value history, `Contains`-with-`until` pruning, `Maybe`/`Where`/`Match`
combinators (gritql); metavariable regex/pattern/comparison as post-filters and equivalences
applied before matching (opengrep); safe interpreter for a restricted pattern language.

**If stack-graph resolution lands**: bindings as validated paths with symbol/scope stacks; partial
paths with pre/postconditions; cross-file stitching at typed boundary nodes (**the sanctioned way to
connect repo slices without raw-merging**); the exhaustive `PathResolutionError` taxonomy; frontier
dedupe by stack signature.

**If schema-driven codegen lands**: one YAML schema generating DB schema, classes and bindings with
a hash-verified generated-file registry; signature-module spec/implementation separation (codeql).

**If agent-applied edits land**: shadow git branch for reversible changes (contextplus); the
literal-first repair ladder ending in a git-tempdir three-way merge (aider); `rewriteIsSafe`
triple-guard (roam-code).

---

## 8. What only this pass provides

- **Algorithm-level detail with named constants** throughout — thresholds, weights, caps and score
  tables are transcribed rather than paraphrased, so most items are implementable as written.
- **Competitors' documented bug histories** — the specific failures that motivated a design
  (roam-code's layout-pack denominator bug, aider's cache-corruption fallback, treesitter-chunker's
  `byte_start=0` fix, gritql's overlapping-effects error, praisonai's compaction thrashing).
- **Doctrine sentences worth adopting verbatim**: *"the absence of a flag is NOT a completeness
  guarantee"*; *"unknown is intentionally distinct from fresh"*; *"a file this scan could not read
  is dropped from the numerator AND the denominator"*; *"never replace one broken link with
  another"*.
- **The C4 store-hygiene cluster** — batching, dedupe maps, quarantine-and-swap, classified storage
  failures — barely appears in the other three passes and is the fastest real win available.
- **Two capabilities nobody else surfaces**: library-identity edge classification (G2) and channels
  and infra bindings as graph citizens (G4).
- **Correct identification of `semantic` and `semantica`**, independently confirming the Qwen pass
  against M3 and DSv4-Pro.

---

## 9. Four-way agreement and conflict

**Unanimous across all four passes** — the strongest signals in the entire corpus:

- Git co-change edges as a ranking and relationship signal.
- Per-edge resolution confidence with a documented tier ladder.
- Freshness as a typed multi-state verdict, not a boolean.
- Coverage honesty: what could not be indexed or read stays visible and stays in the denominator.
- Agent-outcome benchmarking over graph-fact checking.
- RRF k=60 for heterogeneous lane fusion.
- A canonical span/interval type with merge algebra.
- Rename-tolerant identity alongside content hashing.
- Community detection, dead-code and clone/similarity as named analytics.
- The declarative extraction DSL as the long-term architectural bet.
- Baseline-of-known-violations for rules.
- Glossary discipline enforced in CI.

**Settled disputes:**

- **`semantic` / `semantica`.** Qwen and Flash read the vendored `repos/` copies and agree;
  M3 and DSv4-Pro resolved GitHub org names and got both wrong. **Use the Qwen/Flash
  identification; void the four entries in the other two files.**
- **Backend pluralism.** M3 and DSv4-Pro propose swappable graph stores from nine repos; Qwen
  rated every one of those backends weaker than SQLite; this pass proposes **store hygiene**
  instead — batching, dedupe, quarantine, atomic swap. That is the better answer and it settles
  the question: **improve the store you have; take the port only for the in-memory test double.**
- **Determinism.** Qwen made it a top finding; M3 barely mentioned it; this pass supplies the
  missing mechanisms (fixed RNG seeds, canonical JSON, position-sorted edits, transactional match
  environments, deterministic tie-breaks). Qwen's checklist plus this pass's primitives is the
  complete picture.

**Where this pass is thinner:**

- **No packaging, installer or distribution layer** — that is M3 and DSv4-Pro's contribution
  (marker-anchored installers, multi-host plugin manifests, scoped tool surfaces, skill bundles).
- **No cross-repo prioritisation of its own.** Per-file sequencing only; §3 and §6 above are the
  triage the pass omitted.
- **Epistemic envelopes** — Qwen's nine-repo convergence on `exact | lower-bound` impact results
  with machine-readable cause counters has only partial counterparts here (E1, C8).

**Reading order for the four documents**: this one for *what to build and how it works*; Qwen for
*how to know it's right* (determinism, epistemic honesty, evaluation); DSv4-Pro for *where each
change lands*; M3 for *how to arrange the system and package it*.
