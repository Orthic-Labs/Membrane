# Cortex — Unified Absorption Improvement List

**One list.** Produced by reading all seven distinct documents in the `cortex sol` set
(`m3CORTEX_IMPLEMENTATION_GUIDE.md` and `dsc4proCORTEX_IMPLEMENTATION_GUIDE.md` are
byte-identical — one file, not two), deduplicating on *mechanism*, resolving the conflicts
the syntheses left open, and then checking every claim against the actual repository at
`a91909c` before ranking it.

**Inputs**

| Document | Author | What it is |
|---|---|---|
| `synthesis-qwen.md` | consolidation of the Qwen pass (45/55 repos) | mechanism + determinism + epistemic honesty |
| `synthesis-dsv4flash.md` | consolidation of the DSv4-Flash pass (55/55) | mechanism with named constants; the densest pass |
| `synthesis-m3.md` | consolidation of the M3 pass (55/55) | architecture, packaging, CLI shape |
| `synthesis-dsv4pro.md` | consolidation of the DSv4-Pro pass (55/55) | seam mapping — every item routed to a real Cortex file |
| `qwenCORTEX_CANONICAL_IMPLEMENTATION_GUIDE.md` | Opus/Sol consolidation | RecallCircuit P0 patch + P1–P6 program |
| `dsv4flashCORTEX_UNIFIED_IMPLEMENTATION_GUIDE.md` | Opus/Sol consolidation | 16-capability map + workstreams A–P |
| `m3CORTEX_IMPLEMENTATION_GUIDE.md` (= `dsc4pro…`) | Opus/Sol consolidation | the original RecallCircuit P0 patch |
| `competitor.md` | index | 55 vendored repositories |

**The one-sentence conclusion of all seven documents, which they agree on:**

> Cortex's *engine* (content-hashed evidence, merkle generations, per-edge confidence tiers,
> transactional publication, local-first SQLite) was rated stronger than every one of the 55
> competitors. What is behind is the **honesty layer**, the **packing layer**, and the
> **measurement layer** — plus one large unexploited capability already sitting in files
> Cortex reads (SCIP).

---

## 0. How to read this list

- **Ranked once, globally.** Waves are sequencing, not separate backlogs. Do Wave 0 before Wave 1.
- **Status** is the result of checking the repo, not of trusting a synthesis:
  - `BUILD` — genuinely absent.
  - `EXTEND` — the primitive exists; the gap is narrower than the syntheses claim.
  - `AUDIT` — a synthesis asserted an absence that is wrong or partly wrong; the work is verification.
- **Support** counts how many of the four model passes independently surfaced the mechanism.
  Four-way agreement where Qwen was independent is the strongest signal in the corpus.
- **Effort**: S ≤ 1 day, M ≤ 1 week, L > 1 week.

### What the repo check changed

Several high-ranked synthesis items are already partly built, and the syntheses' own rankings
are wrong as a result. Corrections applied throughout:

| Synthesis claim | Reality at `a91909c` |
|---|---|
| "Cortex carries precision at the provider tier only — coarser than every competitor" (Qwen C1, #3) | **Wrong.** `graph/confidence-tiers.mjs` already stamps a four-value `confidenceTier` on **every** edge, persisted in `edges.confidence_tier` with an index, and `graph/precision-tiers.mjs` carries the orthogonal provider axis. The real gap is narrower: no persisted `resolutionStrategy`, no `AMBIGUOUS` class, no diagnostics stream. Ranked accordingly. |
| "No PageRank in ranking" (Flash G1, #9 — "single biggest ranking upgrade available") | **Wrong.** `graph/neighborhood.mjs` has local PageRank with tier-derived edge weights, `DAMPING = 0.85`. The gap is adaptive damping, guaranteed termination, and seed personalisation — a much smaller item. |
| "No FTS lane" (Flash) | **Wrong.** `fts5` is already in `graph/store-sqlite.mjs`. The gap is *fusion* (no RRF anywhere in the tree). |
| "Add a parse cache" (Qwen guide P1 20.1) | **Partly built.** `graph/parse-cache.mjs` exists. The gap is key composition (grammar/extractor/schema versions) and never-cache-empty. |
| "Cancellation is missing" (M3 C7, DSv4-Pro P8) | **Partly built.** `graph/barrier.mjs` already threads `AbortSignal` with a typed `request_cancelled`. The gap is threading it through provider parse loops. |
| "Attestation / redaction / path confinement missing" (M3 C8, C5) | **Wrong.** `schemas/run-attestation-v1.schema.json`, `lib/redaction.mjs`, `lib/path-confinement.mjs`, `lib/sarif.mjs` all exist. These are audits. |
| A P1 migration is needed for edge strategy metadata | **Not needed.** `edges.extra` is an existing nullable JSON column. |

### Conflicts, settled

1. **`semantic` / `semantica` identification.** Qwen and DSv4-Flash read the vendored `repos/`
   copies and agree (`semantic` = GitHub's archived Haskell tree-sitter analysis library;
   `semantica` = a ~178K-LOC Python provenance/KG framework). M3 and DSv4-Pro resolved GitHub
   org names instead and got both wrong — DSv4-Pro inherited the error by citing M3.
   **Void those four entries.** 2–0 on evidence, not 2–2 on count.
2. **Backend pluralism.** M3 and DSv4-Pro propose swappable graph stores from nine repos.
   Qwen read those backends and rated every one weaker than SQLite. DSv4-Flash proposes store
   *hygiene* instead. **Settled: take the port seam and the in-memory test double; never ship
   a second production store.** Improve the store you have.
3. **Determinism.** Qwen makes it a top finding from eleven repos with a concrete checklist;
   M3 barely mentions it and several M3 proposals cut against it (a `learned/` module, spectral
   clustering, wall-clock thresholds). **Settled: every item below that touches ranking or graph
   content inherits the Qwen determinism checklist. `learned/` stays opt-in, out of the graph,
   never authoritative.**
4. **Tool surface.** M3 wants both a six-verb minimal surface and a 60+ tool surface, in
   different files. **Settled: Cortex already froze six MCP tools deliberately. Keep the freeze;
   new capability lands on the application service and the lean CLI, and reaches MCP only on a
   deliberate version bump.** This is the guides' own position and it is correct.
5. **DSv4-Flash's self-assessment** ("this is the strongest of the four") is right on mechanism
   density and wrong on Cortex-state accuracy — three of its top-10 rest on absences that do not
   exist. Weight it highest for *how a mechanism works*, lowest for *what Cortex lacks*.

---

# THE LIST

## Wave 0 — Latent bugs and free wins (days, no dependencies)

**1. Honor SCIP `position_encoding` per document.** `BUILD` · S · Qwen E1 (#1), Flash, Pro R4
`graph/scip-provider.mjs` (137 lines) never reads `position_encoding`. SCIP indexers may emit
UTF-16 code-unit offsets; Cortex treats them as UTF-8. Every span on a line containing a non-ASCII
character is silently shifted — wrong evidence, wrong content hashes, wrong citations, no error.
This is the only item in the corpus that is a live correctness bug rather than a missing feature.
*Done when:* a fixture with emoji/CJK source produces byte-identical spans under both encodings.

**2. Safe-parse guard on every tree-sitter entry point.** `BUILD` · S · Pro S1 (#1, unique to that pass)
GitNexus forbids direct `parser.parse(content)` because the binding SIGSEGVs on Windows for
strings > 32,767 chars, and enforces it with a lint rule. Cortex uses `web-tree-sitter` on every
platform. Verify whether the limit applies; adopt the chunked helper plus the lint rule either way —
a crash class prevented by a rule beats one caught in review.

**3. String/comment masking before reference resolution, plus import-backing for cross-file
lexical edges.** `BUILD` · S · Qwen E12 (#7)
Kills a whole class of false-positive `CROSS_FILE_HEURISTIC` edges — identifiers found inside
string literals and comments currently resolve. One day's work, directly improves impact honesty.

**4. Builtin blocklist before resolution + callable-over-value tie-break + ambiguity-decline on
re-exports.** `BUILD` · S · Flash C5/X15/X16 (#15)
Three small rules preventing three distinct wrong-edge classes: a ~90-entry builtin name blocklist
applied *before* resolution so short common names never produce fuzzy edges; prefer
Function/Class/Interface over Variable when defs share a name (TS emits both for
`const fn = () => {}`); and **drop**, never guess, a name re-exported to two targets — first-wins
is only sound where duplicate exports are illegal, last-wins is wrong for `try/except ImportError`.

**5. Resolution-time stale-evidence guard.** `BUILD` · S · Qwen V2 (#9)
Hash the emitted span bytes against the stored `contentHash` before serving. Mismatch emits
`STALE_EVIDENCE` (dropped or downgraded), never silently stale code. Small, pure, protects every
query surface, and makes dirty-tree queries honest without a rebuild.

**6. Index-derived clock.** `BUILD` · S · Qwen R11
Any recency component computes age against the generation's own max observed timestamp, never
`Date.now()`. Deterministic by construction; goldens stay byte-stable. Cheap to do before any
churn/co-change signal lands (item 24) — retrofitting it later is much worse.

**7. Determinism checklist as a contract clause.** `EXTEND` · S · Qwen C9 (eleven repos)
Posix-normalise before hashing; every sort declares a total order; parallel work produces immutable
per-part deltas merged in sorted order; RNG seeded; env vars affecting output pinned in tests;
tie-breaks explicit, never map-iteration order. Cortex's doctrine already implies most of this;
enumerate it so it is checkable. Pairs with items 30–31.

**8. `locate path:line` — narrowest enclosing span for a line.** `BUILD` · S · Qwen Q3
Tiny surface, outsized agent value: the primitive an agent needs the instant it is staring at a
stack trace. Prerequisite for item 21.

**9. Adaptive-damping PageRank with guaranteed termination.** `EXTEND` · S · Qwen R1
`graph/neighborhood.mjs` has fixed `DAMPING = 0.85`. Interpolate 0.92 (DAG) → 0.82 (cyclic) by
SCC-node ratio; hard iteration cap returning best-so-far so it can never hang; degree-ranking
fallback preserving the score-sum contract. Low-risk upgrade to an existing core, *not* the
"biggest ranking upgrade available" the Flash pass called it.

**10. Redact tokens in subprocess error text, including URL-encoded forms.** `AUDIT` · S · Pro S2
`lib/redaction.mjs` covers egress payloads. A failing `git` subprocess surfaces its argv in the
error string, where a token appears both raw and `quote()`-encoded. Verify coverage; extend if not.

**11. Self-excluding pathspec for Cortex's own state.** `AUDIT` · S · Flash O12
`:(exclude).agent` on every git read, and `.agent/` self-gitignored, so Cortex's own output can
never make its own freshness verdict look dirty. Verify against `graph/ignored-prefixes.mjs`.

**12. Test-mode network tripwire.** `AUDIT` · S · Qwen O19, Flash O14
A guard that *hard-fails* any network path invoked under a test sentinel, with explicit opt-in for
network-disabled qualification gates. Turns the local-only doctrine from prose into a test failure.

---

## Wave 1 — The honesty layer (1–2 weeks) — *the differentiator*

All four passes converged here independently, and it is the one place where the corpus shows
Cortex is behind rather than ahead. These compose into one story: **every answer states how it
was derived, what it missed, and whether it is exact or a floor.**

**13. Per-edge resolution strategy + `AMBIGUOUS` class + diagnostics stream.** `EXTEND` · M
Support 4/4 (Qwen C1, Flash C5, M3, Pro)
Cortex stamps `confidenceTier` on every edge but discards *how* the edge was resolved
(`resolutionPath` in `relationship-kinds.mjs` is computed and thrown away) and has no ambiguity
class — an unresolvable name becomes `UNRESOLVED` with `target=null`, indistinguishable from
"no candidate" and "seven equally good candidates". Take:
- persist `resolutionStrategy` in the existing `edges.extra` JSON column — **no migration**;
- add `AMBIGUOUS` with a candidate count, distinct from `UNRESOLVED`;
- a `resolution_diagnostics` stream recording every resolution failure with reason, caller, file,
  line — CodeGraphContext's mechanism, and the one that makes coverage claims arithmetic;
- strict-vs-permissive traversal becomes a caller choice rather than a build-time decision.

**14. Epistemic response envelope: `exact | lower_bound` with machine-readable causes.** `BUILD` · M
Support 4/4 (Qwen C2 — nine repos, Flash C8, M3, Pro)
The single most-agreed mechanism in the corpus with no Cortex counterpart. Every bounded response
carries `epistemic`, `partial`/`truncated`, `*_total`, `readiness`, `provider_used`/`fallback_used`,
per-family coverage — **one shape, every surface**. GitNexus goes furthest and is the model: it
distinguishes "this impact set is exact" from "this is a floor" and *counts why* (dropped call
sites, DI boundaries, externals). This is what makes Cortex's impact claims defensible.

**15. Ingestion ledger: every considered source ends in exactly one terminal state.** `BUILD` · M
Support 4/4 (Qwen C7 — nine repos, Flash E1, M3, Pro; Qwen guide §20.5)
`INDEXED | INDEXED_DEGRADED | SKIPPED_IGNORED | SKIPPED_BINARY | SKIPPED_OVERSIZE |
SKIPPED_UNSUPPORTED | PARSE_ERROR | PROVIDER_TIMEOUT | PROVIDER_CRASH | READ_ERROR`, each with
path, generation, provider, machine-readable reason, fallback used, and whether coverage is
therefore a lower bound. Plus the doctrine, worth adopting verbatim from codebase-memory-mcp:
> *"The absence of a flag is NOT a completeness guarantee. Callers should treat a flagged file as
> 'prefer grep here', never treat an unflagged file as provably complete."*
Make the *gap* queryable through the same API as the data. Ship this **before** any incrementality
optimisation (Wave 4) so blind spots are visible before they are hidden by caching.

**16. Parse-health bits + guarded extraction fallback ladder.** `BUILD` · S–M · Qwen C8
Three parser-grade codebases (tree-sitter, oxc, semantic) converged on the identical contract from
three languages: **never return "no tree" — return a complete-but-degraded tree plus a health tag.**
Per-file `has_error` / `error_cost` / `is_missing` bits, `complete = (errors_for_file == 0)`,
a structural → positional-window fallback at LEXICAL tier, and one extractor's failure becomes a
finding rather than a build failure. Downstream branches on one bit instead of handling exceptions.

**17. Freshness as a typed state machine where `unknown ≠ fresh`, with read-repair.** `EXTEND` · M
Support 4/4 (Qwen I8, Flash C2, M3 C4, Pro C2)
`fresh | dirty | stale{behind|ahead|diverged} | unknown{reason}` computed from
`merge-base --is-ancestor` plus `rev-list --left-right --count`, with per-state remediation strings
and a freshness footer on every response. Two refinements only some passes have:
- **`unknown` is deliberately distinct from `fresh`** — if git metadata can't be read, warn softly,
  never imply the graph is current;
- **read-repair** (sense, surfaced only by DSv4-Pro): a bounded per-query stale sweep that inline
  re-indexes, so edit-then-immediately-query is honest *before* the debounce fires. Freshness as a
  repair action, not just a reported state.
Cortex has `graph/barrier.mjs`; this is the vocabulary and the repair path on top of it.

**18. Edge provenance and liveness on every impact row.** `BUILD` · M · Qwen Q4/Q5
Each impact row carries the edge that justified inclusion (`via`), hop depth, score, and an
entry-point-reachability attribute with a `live_only` filter. Structural edges
(contains/imports/defines) excluded from fan-out; multi-seed impact keeps the best score per node.
Liveness is **tri-state** (`LIVE | UNREACHED | UNKNOWN`) — never equate zero inbound edges with
dead code. Stops overstated impact and explains every row.

---

## Wave 2 — Measurement (1–2 weeks, parallelisable with Wave 1)

Every pass agrees: do not start Wave 3 without numbers. Cortex's `evals/` already has
equivalence, performance envelopes and AX scenarios — this extends rather than founds it.

**19. Frozen eval corpus with sha256 checksums.** `EXTEND` · S · Support 4/4
sha256 over the eval corpus, `HoldoutViolationError` on tamper, pinned upstream commit SHAs in
fixture metadata so "did the corpus move?" is a header check. One file; removes silent corpus
drift underneath every other measurement item. Do it first.

**20. Graded ranking metrics, computed after every post-pass.** `EXTEND` · M · Qwen X5, Flash, Pro C4
nDCG@k, MRR, recall@k, percentiles; set precision/recall at **node and span level** with
`allowedAlternates` counted as non-penalty true positives. The refinement from llama_index that
most harnesses miss: metrics computed **after** every post-processor, not on the raw ranker output.
This is the gate on items 26–29 and on any learned overlay, ever.

**21. Incremental ≡ full-rebuild equivalence, and commutativity.** `EXTEND` · M · Qwen X1/X2
Two tests: two random file orders produce byte-identical generations; and
`incremental_build(repo, edits) == clean_full_build(repo_after_edits)` on canonical semantic
output (not DB byte layout). GitNexus's equivalence test is the strongest incrementality proof
available, and it is the **gate on all of Wave 4**. Requires item 7 (seeded RNG everywhere).

**22. A/B agent-efficiency harness.** `BUILD` · M–L · Support 4/4 (Qwen C10, Flash, M3, Pro C4)
The metric that sells the tool, and the one Cortex's correctness-focused evals do not measure:
same task, no-Cortex arm vs Cortex-mounted arm, pinned model, fresh clone, isolated settings,
optional grep/glob disabling to isolate graph value, median + IQR, checkpoint/resume. Report
tokens delivered, Cortex calls, total tool calls and wall-clock **at equal task accuracy**.
Nine repos' worth of harness design; claude-context's published numbers (−39.4% tokens,
−36.3% tool calls at equal F1) are the shape of the claim this makes available.

**23. Whole-graph golden snapshots with a `bless` mode, per fixture repo.** `EXTEND` · M
Qwen X9, Pro O14/O15
Deterministically ordered, cross-platform normalised. Any extraction regression anywhere moves the
golden. Plus a per-language snapshot matrix so language coverage gaps become countable rather than
anecdotal.

---

## Wave 3 — Capability: mine what Cortex already reads

**24. Mine the rest of SCIP.** `BUILD` · M · Qwen E2 (#2) — *largest capability gain per line in
the corpus*
`graph/scip-provider.mjs` reads exactly one thing: occurrences whose `roles` include
`"reference"`. The format already carries, in files Cortex is already opening:
- `SymbolInformation.kind` — an 87-value enum, authoritative over descriptor-suffix guessing;
- `relationships` (four flags) → free `IMPLEMENTS` / `TYPE_DEF` edges at `EXACT_RESOLUTION`;
- the full `SymbolRole` bitset — Definition / Import / **Write** / **Read** / Generated / Test,
  which maps onto relationship kinds Cortex has already declared but cannot populate;
- `enclosing_range` — body spans, not just name ranges;
- per-occurrence indexer diagnostics.
Plus the framing all passes agree on: **SCIP is an upgrade layer, not a parallel provider** —
a SCIP edge supersedes a lexical edge for the same `(source, target)` with a recorded tier upgrade,
and the heuristic edge is retained as evidence of what the cheap path concluded.

**25. `scip lint` + `scip snapshot` at ingest.** `BUILD` · S–M · Pro R4/R5
Validate any repository-supplied SCIP export against schema and rules before trusting it; render an
index as a canonical text snapshot and diff it so ingestion changes are visible. A bad export must
degrade to other providers or fail typed — never silently introduce corrupt spans. Pairs with 1.

**26. Diff-range → span scoping, with a `ChangeTarget` primitive.** `BUILD` · M
Support 4/4 (Qwen Q1, Flash, M3, Pro C2)
`git diff --unified=0 <target>` hunk ranges overlaid on evidence-span line ranges, plus treeish
classification including merge-base resolution for PR branches. This turns `diff_impact` from
"blast radius **if** X changes" into "blast radius **of this diff**" — the difference between an
eval fixture and a product surface. Item 8 is the resolver underneath it.

**27. Failure-signal resolution: stack traces, `path::test` ids and diffs as query inputs.**
`BUILD` · M · Qwen Q2 (repo-graph)
The productised form of 26. An agent staring at a failing test hands Cortex the trace and gets
nodes and spans back deterministically.

**28. Co-change coupling from git history, bounded.** `BUILD` · M
Support 4/4 — one of the strongest four-way convergences in the corpus
A relationship static analysis cannot produce. All the passes agree on the guardrails, and they
matter more than the edge: minimum 3 co-changes, ~6-month window, skip commits touching >50 files
as merge/reformat noise, hard timeout, silent no-op without git. **Non-negotiable constraint from
the Qwen guide §28:** `CO_CHANGES_WITH` is a separate, lower-authority temporal family and must
never share an authority tier with compiler-resolved `CALLS`/`IMPORTS`. Ranking and risk signal
only. Requires item 6 first.

**29. Recommended test set + transitive test coverage.** `BUILD` · M · Qwen Q7/Q8, Flash, Pro
Derive `TESTED_BY` from "Test-kind symbol calls non-Test symbol"; traverse reverse dependency
edges from change seeds; return `selectedTests[]`, `uncoveredImpact[]`, `reasonsByTest{}` and a
`coverageState: exact | lower_bound | unknown`. The Qwen guide's naming discipline is right:
**do not call it "minimal"** unless the fixtures prove minimality — ship it as `recommendedTestSet`.

---

## Wave 4 — Packing and retrieval (2–4 weeks; gated on Wave 2)

**30. Assemble-to-budget as one named entry point, with typed omission receipts.** `BUILD` · M
Support 4/4 (Qwen C11 — seven repos, Flash C9, M3, Pro C1)
Cortex has the strongest token accounting in the corpus — `lib/token-budget.mjs`'s two-budget
slice-time/resolve-time system was rated superior to every competitor's `chars/4` scheme — and no
assembler on top of it. What Cortex lacks is not accounting but *shaping*. Take:
- a declarative route decision computed **before** assembly (`FITS | COMPACT | TRUNCATE`);
- residual budget = `ceiling − prompt − meta − already-included − buffer`, filled by lane priority;
- **named drop-steps**, each carrying its own reason code, so what lost the budget race is
  attributed to a step rather than a blanket omission (roam-code — the refinement most passes miss);
- admitted refs **written back** into the visible selection so auto-context is inspectable (mentat);
- a usage footer and per-candidate cost with a running cumulative total.

**31. Reciprocal rank fusion, k=60, for independently-ranked lanes.** `BUILD` · M
Support 4/4 — seven repos, one constant; `rrf` appears nowhere in the tree
Cortex has FTS5 and PageRank and no way to fuse them. RRF for merging independently-ranked lanes;
keep the weighted sum for aligned components. Two refinements worth taking: octocode's
(**fusion order is rank-based; displayed and thresholded scores are recomputed on a comparable
scale afterwards**) and per-source timeouts producing a `DEGRADED_SOURCE` receipt rather than a
stall. Candidate pool `3× limit` capped at 300.

**32. Skeleton and outline tiers admitted before full bodies.** `BUILD` · M · Qwen C12, Pro C9
A cheap-to-expensive ladder: project outline → module API → symbol signature → full body, where an
oversized symbol emits **signature + child list + expansion pointers** rather than a truncation.
Aider's scope-skeleton renders at ~1/50th the tokens. One thing Cortex can do that none of them
can: attach content hashes to the skeleton, so a summary is verifiable.

**33. Lexical query-shape classifier steering weights.** `BUILD` · S · Qwen C4 (six repos)
~200 lines, deterministic, offline, no model: `SYMBOL | FILE | IMPACT | PATH | TEST | CONFIG |
ARCHITECTURE | DOC_TRUTH | FAILURE | EXPLORE`. Tilts component weights and traversal policy; never
invents facts; explicit caller intent always wins. Report the classification as part of the
explanation. Cortex currently uses fixed weights regardless of query shape.

**34. Canonical `Interval`/`Span` type with one path normaliser.** `EXTEND` · M · Pro C1 (six repos)
`path:12-34,50-60` as the universal reference grammar with parse/format/contains/intersects/merge
algebra and a whole-file sentinel that supersedes intervals. The half that matters most is the
**single idempotent POSIX path-normalisation function every identity and serialisation path funnels
through** — it is what makes graph identities stable across operating systems. Spans currently
appear in `sourceRef`, evidence, truncation receipts and protected anchors, each constructed
independently.

**35. Ranking component expansion + non-compensatory tiering.** `EXTEND` · M · Qwen R3–R9
Cortex's named-component output is the shape competitors are converging toward; the gap is signal
*diversity*, not architecture. Add: identifier-salience multipliers (informative names ≥8 chars ×10,
`_private` ×0.1, idents defined in >5 places ×0.1, query-mentioned ×10, sqrt-damped reference
counts); test/infra demotion in two passes (pre-pool and post-rerank) with a `queryTargetsTests()`
opt-out and a sign guard; dependency-origin demotion; special-files prior. And the guard the Qwen
guide states as Correction 7, which is the important part:
> **A high lexical seed score is a prior, not proof.** Evidence mode and minimum edge-confidence
> tier are resolved **lexicographically first**, so a strong lexical match can never compensate for
> a low-confidence path. A scalar score may be returned for convenience; the hard tiers stay
> non-compensatory. Arithmetic mean, never geometric — a single zero degrades, it does not veto.

**36. Bounded rendering with pagination bound to generation + circuit digest.** `BUILD` · M
Qwen guide P4.7, Qwen T5, Pro C6
`minimal | standard | verbose`; token estimate against the **actual wire encoding**; suppress rather
than truncate partial lists; labelled truncation on every bounded output; **generation-checked
cursors** so a stale cursor after a reindex fails closed instead of returning wrong pages;
never orphan a node from the edge that justified it.

---

## Wave 5 — Identity and incrementality (3–6 weeks) — *most valuable, most dangerous*

Gate **every** step on item 21. The syntheses agree this cluster is where correctness is most
easily lost for a speed win that is never measured.

**37. Route-derived `definitionId` alongside `contentHash`.** `BUILD` · M
Support 4/4 (Qwen C6, Flash C1 — eight repos)
Two identities per span, answering two different questions: `contentHash` answers *"is this
byte-identical"* (keep unchanged — it is the verification primitive and rated stronger than every
competitor's); a name/route-derived `definitionId` answers *"is this the same symbol"*. Plus
brain0's structural fingerprint — hashing **only named node kinds with depth annotations**, never
identifier text or literals, so renaming a variable leaves it unchanged — with 3-gram shingles over
the kind stream for Jaccard similarity. Overload collisions fall back to a position-sensitive id.

**38. Rename/move continuity as a deterministic reconciler.** `BUILD` · M (depends on 37)
Cortex treats a rename as delete + add; every edge and claim referencing the old symbol goes stale.
Ordered passes, and **never choose among equal candidates**: exact stable id → exact content hash
for a moved definition → structural fingerprint within the repo → unique qualified-route match →
otherwise a new identity, with an ambiguity record emitted rather than a guess. Typed
`SYMBOL_RENAMED` / `SYMBOL_MOVED` events with similarity scores, kept in a separate
`identity_events` table — **not** mixed into semantic dependency edges. The biggest single
freshness win in the corpus.

**39. Cosmetic-vs-structural change classification with an update-scope matrix.** `BUILD` · M
Support 4/4 (Qwen I4/I5, Flash C3, M3, Pro)
`UNCHANGED | COSMETIC | STRUCTURAL_LOCAL | STRUCTURAL_INTERFACE | UNKNOWN` from content hash plus a
structural signature (params/returns/exported/methods/imports/exports), with a **named details
list** ("params changed: Y", "size 10 → 40 lines"). A comment-only edit stops costing a full
re-extraction. Two rules that make it safe: if either side lacks structural analysis, any content
change is STRUCTURAL; and `UNKNOWN` always takes the full path.

**40. Header-aware invalidation, then closure repair.** `BUILD` · M–L · Qwen I1/I2/I3
`filesToClear = changed ∪ getReferencing(changed)` first (Sourcetrail — editing a header re-indexes
what includes it). Then the refined form: persist per-file resolution surfaces (`surface_sha` of
definitions + a referenced-identifier bloom filter) and *probe* whether a change set is
resolution-closed before applying a delta, else escalate — route enum
`NOOP / FORCED_FULL / LEGACY_PARTIAL / CLOSURE_REPAIR`. Plus GitNexus's separation:
**parse scope ≠ write scope** — re-parse everything cross-file resolution needs, write back only
the effective write set expanded one hop across writable boundaries (this is what fixes barrel
re-export staleness), with an escalation gate at >50% of repo **and** ≥50 files, and an
`incrementalInProgress` dirty flag forcing a full rebuild after a crash.

**41. `ts_subtree_get_changed_ranges` for narrowed re-extraction.** `BUILD` · M · Qwen E4
The highest-leverage tree-sitter API Cortex isn't calling: re-extract only the spans touched
inside a changed file; unchanged symbols keep their evidence. Plus CoW `ts_tree_edit` with old-tree
reuse for cheaper reparses. Feature-flagged until fixture equivalence is demonstrated, under the
invariant the Qwen guide states well:
> *Incrementality may reduce work; it may never reduce soundness relative to the full-file provider.*

**42. Version-stamped cache keys, never-cache-empty, schema-revalidate-on-read.** `EXTEND` · S–M
Support 4/4 (Qwen C13 — nine repos, M3 C5 ranked #2, Pro R1 ranked #3)
`graph/parse-cache.mjs` exists; the gap is the key. Compose it as
`sha256(contentHash + providerId + providerVersion + languageId + grammarVersion +
extractionSchemaVersion)`. Then rag-rat's `NORM_VERSION` trick, which two passes independently
ranked top-3: a **version filter on read** auto-excludes stale-shaped rows and the next reindex
produces the new version — **schema evolution with no migration**. Never cache empty unless the
parser explicitly returned a successful empty parse; cache corruption is a miss, not evidence.

**43. Watcher hardening.** `EXTEND` · M · Qwen I13 (four independent repos), Flash C3
Generation-counter debounce so stale timers no-op; debounce on the **last** event, not the first;
drain in-flight callbacks on stop; prefix-removal for directory deletes where per-file events never
fire; exclude Cortex's own output paths (item 11); `mkdir`-based cross-process lock. Plus axon's
tiered discipline with the piece the other passes miss — a **`MAX_DIRTY_AGE` starvation guard**, so
continuous writes cannot defer the global pass forever.

---

## Wave 6 — Store hygiene, truth and operations

**44. Store hygiene cluster.** `BUILD` · M · Flash C4 — *barely appears in the other three passes
and is the fastest real performance win in the corpus*
Batched inserts with **halving statement sizes** precompiled at N, N/2 … 1 to fit SQLite's
999-variable cap, greedily filling the largest that fits (Sourcetrail documents ~2×); in-memory
dedupe maps held for the build transaction; **edge identity as a UNIQUE index** rather than
application logic; a reverse-edge covering index `(generation_id, target, kind, source)` pinned by
an `EXPLAIN QUERY PLAN` test; generated-column indexes on JSON properties; and a WAL checkpoint
valve, because default auto-checkpointing rewrites ~95% of hot pages during a bulk index.

**45. Staged build → verify → quarantine → atomic swap.** `EXTEND` · M · Flash C4, Pro P1, Qwen S14
Cortex already publishes generations transactionally at the row level. Extend the guarantee to the
whole store file: build into a temp DB, validate durable rows, **quarantine** the old DB and its
`-wal`/`-shm` sidecars timestamped for forensics, `os.replace`, record provenance after, verify
post-swap. A failed build then cannot corrupt a sealed store. Add classified storage failures
(disk-full / IO error) that flip the store read-only with typed diagnostics rather than crashing.

**46. Lock-vs-corruption discrimination before any destructive recovery.** `BUILD` · S–M · Qwen O1
Classify store-open failures — lock contention / transient read / genuine corruption — with
graduated backoff and one hard rule: **never wipe before the retry budget is exhausted.**
infigraph's regression tests for this exist because a transient mid-checkpoint read failure once
caused a data-loss wipe. Cheap insurance on the one failure mode that loses user data.

**47. Findings registry with deterministic ids and supersession.** `BUILD` · M
Qwen V7/C14, Flash C7, M3, Pro
One cross-detector evidence layer for parse errors, stale evidence, dead refs, drift and doctor
issues: ids as `hash(kind + subject + content)`, UNIQUE for idempotent upsert, `supersedes_id`
chains, a suppressions column, and — the discipline that matters — **stale facts degrade rather
than vanish**, and claims are re-checked after a rebuild rather than silently dropped. Build it
only once two or more producers need it; a registry with one client is a table.

**48. Citation re-location: distrust stated line numbers by design.** `BUILD` · M · Qwen V1
Search the quoted snippet verbatim in the actual file bytes and **overwrite** the stated span with
the true one; failure → typed `UNVERIFIED_QUOTE`. Converts externally-produced or model-produced
spans from trusted into verified, mechanically. With a cascaded anchor ladder — exact →
case-insensitive → stripped → blank-lines-removed — where a hit at a later stage emits at a
**degraded confidence tier** with an `anchor_relocated` note. Never silently.

**49. Evidence-coverage admission gate for claims.** `EXTEND` · M · Qwen V3, Flash C7
A claim's cited `path:start-end` must match an evidence span whose `contentHash` is present in the
referenced generation; violations become `UNSUPPORTED_SPAN`. This is Cortex's own
evidence-before-claims invariant made executable — the corpus's strongest single endorsement of a
doctrine Cortex already holds in prose. Plus agentic-codebase's truth lattice with its precise rule:
*citations still resolvable but claim ungrounded → **Stale**, not Deleted.*

**50. Doctor reports findings, not counts.** `EXTEND` · S–M · Flash guide §14, Pro O1
Replace `nodes: 12,345 / edges: 31,220` with
`18 unresolved imports (12 heuristic-eligible, 6 unsupported) · 4 symbols with ambiguous same-tier
targets · 2 current documents cite superseded files · 93.2% exact/AST relationship coverage`.
Errors carry `reason`, `source` and `suggestions[]` rendered as a "Suggested fixes:" block. Counts
stay, but secondary. `lib/operations/doctor.mjs` is 160 lines — this is where items 13, 14 and 15
become visible to a human.

**51. Crash-isolated provider execution with quarantine.** `BUILD` · M · Qwen O2, Flash O1
A fork/exec'd worker contains SIGSEGV and hangs and returns RSS to the OS; a crash synthesises a
typed `PROVIDER_CRASH` finding naming the affected file set so a *targeted* retry fixes exactly the
broken subset. Files that fault or hang are appended to a quarantine list and short-circuited to
empty results with the phase recorded (`crash` vs `hang`). A crashing file stops being a wall.

**52. Thread cancellation through provider parse loops.** `EXTEND` · M · M3 C7, Pro P8
`graph/barrier.mjs` already has `AbortSignal` and a typed `request_cancelled`. Extend to provider
parse loops and long analyses, with await-before-drop on shutdown and — the invariant seven repos
share — **a cancellation is not a failure**, it is its own typed outcome. Never publish a partially
built generation.

**53. Glossary discipline enforced in CI, plus a metric ratchet.** `BUILD` · S
Support 4/4 — all four passes surfaced this from signum
One JSON file of canonical terms and aliases, one check script, applied across code, docs, claims
and receipts. Cortex's own writing uses "decision" / "verdict" / "recipe" for what is sometimes the
same thing, and this document's inputs use "absorb" / "adopt" / "strengthen" interchangeably. The
ratchet is the other half: assert that named metrics (exact-resolution %, claim fidelity) are
non-decreasing across tagged commits.

---

## Wave 7 — The RecallCircuit question (decide separately)

Three of the four consolidation guides are, in substance, one proposal: a
`RecallCircuit` primitive — seed resolution → predicate-aware bounded traversal → complete evidence
paths → ranked circuit — exposed on the application service and a lean CLI, with MCP's six-tool
surface deliberately frozen.

**It is a coherent and well-specified proposal, and it is not the same kind of item as anything
above.** Everything in Waves 0–6 improves facts Cortex already computes. This adds a new public
contract, a new schema, and a second retrieval path alongside `scripts/cortex-candidates.mjs`.
Two considerations before committing:

- The guides' own P0 patch has **ten specified defects**, and the Qwen guide (correctly) supplies
  them as mandatory corrections rather than cleanup: `maxNodes`/`maxEdges` declared but never
  enforced (a dense hub traverses and hydrates unbounded, then slices — exactly the failure the
  policy claims to prevent); explicit seeds/anchors hydrated without generation membership checks;
  `evidenceRequired: true` ambiguous between "every edge" and "any edge"; terminal filtering applied
  *after* `circuitId` is computed, so the id no longer describes the visible projection; omission
  counts conflating five distinct causes; no path deduplication before ranking; compensatory
  scoring letting a lexical seed outrank an evidenced path; untyped public arrays in a schema
  proposed for the catalog. **Do not implement the patch as written.**
- Its Correction 9 is the real decision: *one traversal primitive, not two future algorithms.*
  If RecallCircuit ships, `indexedNeighbors`, `impact` and recall must converge on the same bounded
  traversal, and `cortex-candidates.mjs` must become an adapter rather than remain a second
  independently evolving retrieval algorithm.

**Recommendation:** treat item 13 (per-edge strategy), 14 (epistemic envelope), 18 (edge provenance
+ liveness) and 30 (assembler) as the prerequisites they actually are. Most of what makes a
RecallCircuit valuable is those four; the circuit is the packaging. Ship them first, then decide
whether the new contract earns its schema — with item 22's numbers, not by argument.

---

## Rejected — recorded so the evaluation is not redone

- **A second production graph backend.** FalkorDB / Kùzu / Neo4j / LanceDB / Milvus / RocksDB.
  Qwen read all of them and rated every one weaker than SQLite. Take the port seam and the
  in-memory test double; ship no second store. (Settles the M3 ↔ Qwen conflict.)
- **Identity containing line numbers or paths in content hashes.** Shifts on edit; breaks move
  stability. Same for UUID chunk ids, rebuild-unstable autoincrement ids, and `(name, path, line)`
  tuples — all collide or drift.
- **mtime-only staleness as a primary signal.** Acceptable only as a fast path with hash
  confirmation (the ninja stat-first/hash-repair pattern).
- **`chars/4` token estimation as the primary budget mechanism.** Cortex's two-budget system is
  strictly stronger. Keep `chars/4` documented as the floor never to regress to.
- **LLM-delegated analysis with no static core**; LLM-driven extraction or curation; whole-repo
  dumps into a single QA prompt. Outside deterministic scope.
- **A full AST rewrite engine** (ast-grep/GritQL machinery), a **CodeQL/Joern clone**, and a
  general-purpose query DSL. Borrow the abstractions and the test methodology; not the runtimes.
- **Generic writable MCP**, **raw graph copied into durable agent memory**, **freeform semantic
  summaries as truth**, **popularity telemetry as ranking authority**.
- **Build-system filler** — Nix flakes, devcontainers, Bazel, goreleaser, Docker matrices, k8s
  manifests (~30 items across ~15 studies). Two exceptions worth keeping: per-change changelog
  fragments, and marker-fenced git hooks.
- **The four `semantic` / `semantica` entries in the M3 and DSv4-Pro passes.** Misidentified;
  re-study from the vendored `repos/` directories if the repos matter.

## Parked — shapes decided now, cheap to implement when the lane exists

Deciding these in advance is most of the value; several competitors paid for the lesson through a
migration incident.

- **If a vector lane lands:** vectors in the same SQLite file (`sqlite-vec vec0`), never an external
  service; model-ID watermark in the ledger forcing re-embedding on model change; key vectors on
  evidence-text hashes so rebuilds never re-embed unchanged text; brute force below ~200K vectors,
  HNSW above; **on load, quarantine mismatched dimensions rather than mixing them.** Semantic score
  never becomes evidence confidence, and the vector branch failing must never make the
  deterministic branches unavailable.
- **If an LLM-assisted surface lands:** the salvage ladder (strict parse → truncation salvage from
  the open tag → entity cleanup → regex fallback → typed `PARSE_DEGRADED`); bounded JSON with
  **duplicate-key rejection** and a depth cap; per-item validate-and-drop; the verified-skeleton
  rule — the graph-and-evidence skeleton always completes, enrichment failure emits
  `ENRICHMENT_DEGRADED` and never invalidates it.
- **If statement-level analysis lands:** an opt-in overlay with explicit budgets and a registry
  declaring supported languages, producer version, cost class, soundness claim, invalidation unit
  and edge kinds added. Expensive data-dependence facts materialised as build-time edges with
  bailout thresholds, not computed at query time.
- **If cross-repo ingestion lands:** typed contract edges with match-cascade provenance,
  cross-boundary fan-out capped with `riskEpistemic: 'lower_bound'`, and **never merge node
  spaces** — the sanctioned form of Cortex's existing independent-scoping doctrine.

## Standing policy for every item above

**Relationship vocabulary.** The corpus proposes many new nouns — co-change, data flow, routes,
taint, process nodes, effects. Each needs a mini-RFC before it exists: name, direction, source and
target node kinds, exact semantics, producers, confidence derivation, evidence representation,
invalidation unit, traversability under `dependency.forward` and `impact.reverse`, whether it can
affect truth, an adversarial fixture, and measured user value. If those answers are vague, the
relationship is not ready. Statistical relations never share an authority tier with compiler-resolved
ones.

**Ranking.** No single magical relevance number. Keep interpretable layers — seed score, path
evidence, edge confidence, structural distance, query-shape fit, optional priors — and combine at
the last responsible moment. Any learned overlay consumes those features, returns diagnostics, and
may rerank valid candidates but may never resurrect a filtered generation, cross a grant boundary,
convert unresolved into exact, or bypass an evidence requirement.

**Budget ownership.** Cortex may cap, paginate, return minimal/standard/verbose views, emit
skeletons and exact windows, record omissions and estimate serialised size. Cortex must not decide
a model's final prompt budget, rewrite facts into prompt prose, or delete evidence silently to hit
a guessed token target. When a cap binds, prefer a complete high-confidence path over a complete
lower-ranked one over disconnected high-score nodes — and mark a partial path partial.
