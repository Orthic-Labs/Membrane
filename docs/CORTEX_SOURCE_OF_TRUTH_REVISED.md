# Cortex — Source of Truth: Final Shape

**Status:** Canonical · absorption-complete · implementation authority  
**Repository:** `Orthic-Labs/Cortex`  
**Pinned and current `main` baseline:** `a91909c80c4c85fbb236a790f7f36bec8e034ce3`  
**Tree:** `aed07343ca1600d0f6e62f77aaa055db536015f4`  
**Date:** 2026-08-18  
**Layout note:** the repository check behind this document was performed at `a91909c`. Source directories were later collapsed under `src/` (`344e098`), so every path below is `src/`-prefixed; the evidence and findings are unchanged.

This document is the **single planning and implementation authority** for the supplied Cortex competitor-absorption corpus. It absorbs the decisions, mechanisms, sequencing, acceptance gates, file ownership, rollback rules, and Definition of Done from all four supplied planning documents. The source documents become provenance after this document is accepted; an implementation agent must not need to reopen them to discover a missing requirement.

**Inputs absorbed**

| Document | Canonical contribution retained here |
|---|---|
| `CORTEX_MASTER_COMPETITOR_ABSORPTION_GUIDE.md` | System boundary, invariants, canonical contracts, provider/resolution architecture, source-span discipline, history/version archaeology, truth/grounding, diagnostics, generated ownership, runtime/security, file ownership, qualification and Definition of Done |
| `2026-08-17-unified-absorption-improvement-list.md` | Repo-checked reality corrections, globally ranked 53-item program, constants/mechanics, wave sequencing, standing policies, rejected and parked decisions |
| `CORTEX_IMPLEMENTATION_GUIDE.md` | Exact deterministic RecallCircuit design, file set, rollout, rollback, lean CLI seam, correctness/adversarial/performance gates |
| `cortex_master_improvement_list.md` | Ten-priority product skeleton and final completion criteria |

**Corpus accounting.** The competitor index contains **55 repositories**. DSv4Flash is the broadest 55-report coverage ledger; Qwen explicitly covered 45 per-repository reports plus the prior guide/repository; M3/DSC4Pro are a RecallCircuit implementation baseline rather than an independent 55-repo coverage pass. This authority uses the **union** of the guides. A feature is never considered absent merely because it is missing from the Qwen subset.

**Authority rule.** Where source documents disagree, §2 settles the conflict. Where any source document disagrees with the repository at the pinned baseline, the repository wins and §3 records the correction. Where a named future module does not exist yet, the implementation program below defines whether to **ADD**, **EXTEND**, or **AUDIT** it. No nonexistent companion document is a hidden prerequisite.

### 0. How to execute this document

- Items **1–53** retain the Unified Improvement List numbering for provenance and priority traceability.
- Work packages **RES-1–RES-8** restore mandatory provider/resolution/source-precision capabilities that were present in the Master Guide but were compressed out of the prior Source of Truth. They are **not optional**.
- Work packages **TH-1–TH-4** restore mandatory truth/history capabilities that were likewise under-specified. They are **not optional**.
- §7 is the canonical RecallCircuit implementation contract. The older implementation guide is historical input only; do not implement its snippets where §7 differs.
- Release trains in §10 are the executable dependency order. Numeric item order expresses global priority; release-train gates decide when work may actually land.
- A feature is not complete because code exists. It is complete only when its named acceptance gate is green on the pinned/frozen corpus and the relevant Definition-of-Done clause in §12 is satisfied.
- **BUILD** = absent at the pinned baseline. **EXTEND** = a useful primitive exists but the required contract is incomplete. **AUDIT** = the source corpus claimed an absence that is likely already covered; prove it before changing code.
- Effort labels remain: **S** ≤ 1 day, **M** ≤ 1 week, **L** > 1 week. They are planning estimates, not permission to weaken gates.

---

## 1. Executive conclusion

Cortex does not need a redesign. Its **engine** — content-hashed evidence, merkle generations, per-edge confidence tiers, transactional generation publication, local-first SQLite — was rated stronger than every one of the 55 competitors studied.

What is behind is three layers on top of that engine:

1. **Honesty** — every answer must state how it was derived, what it missed, and whether it is exact or a floor.
2. **Packing** — assemble-to-budget, fusion, skeleton tiers, bounded rendering.
3. **Measurement** — frozen corpus, graded metrics, incremental≡full proof, agent A/B harness.

Plus one large unexploited capability already in files Cortex opens: **SCIP**.

The single biggest architectural move is turning Cortex from a graph database/query layer into a **deterministic evidence execution engine**: seed resolution → bounded predicate-aware traversal → complete evidence paths → provenance → omissions → confidence → deterministic ranking. That primitive is **RecallCircuit**.

---

## 2. Conflicts, settled

| # | Conflict | Decision |
|---|---|---|
| 1 | **RecallCircuit first (Master Guide P0) vs. prerequisites first** | Build RecallCircuit as the traversal primitive, but only with the ten mandatory corrections (§7.4) and the minimum honesty/packing prerequisites: items **13, 14, 18, and 30**. Item 30 is deliberately pulled forward as the narrow circuit assembler; the rest of packing remains later. Measurement items **19–22** must be available for shadow qualification (§7.7). **Do not implement the original P0 patch as written.** |
| 2 | **Backend pluralism** (M3/DSv4-Pro: swappable graph stores) vs. **store hygiene** (Qwen/Flash) | Take the port seam and an in-memory test double. **Never ship a second production store.** Improve SQLite. |
| 3 | **Determinism** (Qwen top finding) vs. M3 proposals cutting against it (`learned/`, spectral clustering, wall-clock thresholds) | Every item touching ranking or graph content inherits the determinism checklist (§5.7). `learned/` stays opt-in, out of the graph, never authoritative. |
| 4 | **Tool surface** (six frozen MCP tools vs. 60+) | Keep the six-tool MCP freeze. New capability lands on `src/lib/application/service.mjs` and the lean CLI; reaches MCP only on a deliberate version bump. |
| 5 | **Budget ownership** (Cortex vs. Membrane) | Cortex may cap, paginate, estimate wire cost, prefer complete paths, emit omission receipts, accept caller caps. Cortex must **not** decide the model's final prompt budget, reserve answer tokens, rewrite facts as prose, or silently delete evidence to hit a guessed target. Membrane/caller owns final context selection. |
| 6 | **PageRank** ("biggest ranking upgrade" vs. "not a requirement") | Already exists in `src/graph/neighborhood.mjs`. Preserve it; upgrade to adaptive damping + guaranteed termination (small item); do not expand its authority without a frozen benchmark win. |
| 7 | **`semantic` / `semantica` identification** | M3 and DSv4-Pro misidentified both. Void those four entries. |
| 8 | **DSv4-Flash weighting** | Highest authority for *how a mechanism works*; lowest for *what Cortex lacks* (three of its top-10 rest on absences that do not exist). |
| 9 | **Rollout vs. replacement** | RecallCircuit ships beside `scripts/cortex-candidates.mjs`; the legacy candidate path remains a version-skew/rollback fallback until shadow qualification wins. P0 does not require a store migration, so rollback is intentionally cheap. |
| 10 | **Partial provider/batch failure** | Successful branches/items survive. Return per-item/provider failures plus aggregate `partial`; one provider/file failure must not erase valid evidence from independent branches. A cancelled build is distinct and may never publish a partial generation. |
| 11 | **External seam prerequisite** | The prior Source of Truth named `docs/plans/orthic/SEAM-CONTRACT.md`, but that path is absent at the pinned repository baseline. It is **not** a Cortex-local blocker. The complete Cortex-side seam contract is embedded in §7.8; any external Membrane contract may add caller obligations but cannot silently alter Cortex ownership. |

---

## 3. What the repo check changed

Corrections that override every input's rankings:

| Claimed gap | Reality at `a91909c` | Real gap |
|---|---|---|
| Precision only at provider tier | `src/graph/confidence-tiers.mjs` stamps four-value `confidenceTier` on every edge (`edges.confidence_tier`, indexed); `src/graph/precision-tiers.mjs` is the orthogonal provider axis | No persisted `resolutionStrategy`, no `AMBIGUOUS` class, no diagnostics stream |
| No PageRank | `src/graph/neighborhood.mjs` has local PageRank, tier-weighted, `DAMPING = 0.85` | Adaptive damping, termination cap, seed personalisation |
| No FTS lane | `fts5` in `src/graph/store-sqlite.mjs` | No fusion (`rrf` appears nowhere) |
| No parse cache | `src/graph/parse-cache.mjs` exists | Key composition (grammar/extractor/schema versions); never-cache-empty |
| No cancellation | `src/graph/barrier.mjs` threads `AbortSignal` with typed `request_cancelled` | Thread through provider parse loops |
| No attestation/redaction/confinement | `schemas/run-attestation-v1.schema.json`, `src/lib/redaction.mjs`, `src/lib/path-confinement.mjs`, `src/lib/sarif.mjs` exist | Audits only |
| Migration needed for edge strategy | `edges.extra` is an existing nullable JSON column | No migration |
| `docs/plans/orthic/SEAM-CONTRACT.md` is a required Cortex-local prerequisite | Path does not exist at `a91909c` | Inline the Cortex seam obligations in §7.8; do not block implementation on a phantom file |
| Supplied planning files live under `docs/compshop/` in the pinned repo | That directory is not present at `a91909c` | Treat the supplied attachments as external planning provenance, not runtime/repository dependencies |

Status vocabulary used below: **BUILD** (absent) · **EXTEND** (primitive exists, gap narrower) · **AUDIT** (verify, likely present). Effort: S ≤ 1 day, M ≤ 1 week, L > 1 week.

---

## 4. System boundary and invariants

### 4.1 Cortex owns
Repository/document observation; stable repo/file/symbol/claim/occurrence identity; deterministic graph construction; evidence and provenance; generation identity, freshness, invalidation, atomic publication; truth state (`current | stale | superseded | contradicted | invalid | unknown`); deterministic traversal; exact static-analysis facts; impact/path/flow/test-selection/liveness/change intelligence; query-time ranking signals; graph-health and coverage honesty; bounded, typed, evidence-backed results.

### 4.2 Cortex does not own
Final prompt assembly; final model token-budget policy; host prompt injection; durable conversational memory; model selection; agent orchestration; model-specific formatting; a code-rewrite runtime.

### 4.3 Contract
```
caller / Membrane  -> task-shaped recall request
Cortex             -> structured, generation-bound, evidence-backed RecallCircuit
caller / Membrane  -> selects, budgets, renders, injects final context
```
Never merge Cortex and Membrane stores or policies.

### 4.4 Invariants (locked)
1. Phase 1 (parse, graph, structure, identity, hashes, generations, freshness) is deterministic.
2. Phase 2 (judgment) keeps receipts: evidence, provider id/version, generation, confidence, invalidation inputs.
3. SQLite is the canonical local store; derived indexes are disposable.
4. Exact evidence outranks semantic relevance; learned retrieval may nominate/rerank valid candidates, never create truth or upgrade confidence.
5. A generation is atomic; readers never see a half-built graph.
6. Repositories stay independently scoped; federation composes slices.
7. Repository content is data, not instruction (security boundary).
8. Uncertainty is output — unsupported, unresolved, ambiguous, missing-provider, truncated, stale, partial are all representable.
9. No duplicate core: no second store, second recall algorithm, second ranking policy, second truth system.
10. Ambiguity fails closed; never pick an arbitrary winner.

---

## 5. Canonical end-state architecture

```
OBSERVE   VCS-aware discovery · content identity · changed ranges · ingestion ledger
IDENTIFY  stable identity · occurrences ≠ entities · rename/move continuity
RESOLVE   exact-first cascade · provider ladder · SCC/re-export closure · ambiguity fails closed
TRAVERSE  indexed seeds · bounded predicate-aware execution · complete evidence paths
ANALYZE   impact / flow / liveness / tests · history / co-change / risk · optional overlays
RECALL    RecallCircuit · exact + lexical + structural + graph · optional semantic · explainable fusion · bounded rendering
VERIFY    span/drift checks · truth binding · contradictions / staleness / findings
MEASURE   frozen fixtures · correctness / latency / RSS / query plans · agent accuracy / tokens / tool calls
```

Rule: **deterministic structure and evidence first; learned/semantic assistance only as an optional overlay after admissible evidence exists.**

### 5.1 Canonical contracts (land before proliferation)

- **`SourceAddressV1`** — `repo://<repoId>/path#L10-L30`, `repo://<repoId>/path::qualified.symbol#L10-L30`. One grammar; CLI shorthand may remain.
- **`SourceSpanV1`** — address, start/end, position encoding, file content hash, optional span hash, optional symbol identity. One documented inclusive/exclusive convention.
- **`EvidenceRefV2`** — `{ source, span?, contentHash, provider, generation, confidence, truthClass: observed|derived|asserted|historical|unknown, freshness: current|stale|superseded|invalid|unknown }`. Nodes, edges, claims, findings, diagnostics all reuse it.
- **Provenance is a set** — same fact + independent source → merge; contradiction → preserve both; unchanged rerun → no duplicate; rollback removes only that run's contribution.
- **Entities ≠ occurrences** — one symbol may have declaration, definition, reference, generated, test, doc, historical occurrences.
- **`CortexResultV2<T>`** — `{ invocation: {status: ok|partial|failed|cancelled, durationMs, generation}, outcome, evidence[], omissions[], diagnostics[], claimBoundary, nextActions? }`.
- **Error taxonomy** — `INVALID_INPUT OUTSIDE_REPO NOT_INDEXED STALE_GENERATION PROVIDER_UNAVAILABLE PROVIDER_FAILED UNSUPPORTED AMBIGUOUS RESOURCE_LIMIT CANCELLED STORE_CORRUPT STORE_BUSY PERMISSION_DENIED INTERNAL`; each declares retryable, guidance, partial availability.
- **Provider contract `CodeProviderV2`** — id, version, language, capability vector; parse/symbols/references/imports/relationships/diagnostics?/semanticFacts?; every result reports provider/version, evidence, tier, omission/fallback reason, soundness declaration. Fallback states: `provider_unavailable | unsupported | failed | skipped | incomplete`.

### 5.2 Canonical `Interval`/`Span` algebra
`path:12-34,50-60` universal reference grammar; parse/format/contains/intersects/merge; whole-file sentinel; **one idempotent POSIX path normaliser every identity and serialisation path funnels through** — the thing that keeps identities stable across OSes. Spans currently appear in `sourceRef`, evidence, truncation receipts and protected anchors, each constructed independently; unify.

### 5.3 Relationship-vocabulary mini-RFC (standing policy)
Every new edge kind requires: name, direction, source/target node kinds, exact semantics, producers, confidence derivation, evidence representation, invalidation unit, allowed traversal policies, can-it-affect-truth, adversarial fixture, measured user value. Statistical/temporal relations (`CO_CHANGES_WITH`) never share an authority tier with compiler-resolved `CALLS`/`IMPORTS`.

### 5.4 Ranking policy (standing)
No single magical score. Inspectable layers: seed exactness, path admissibility, evidence coverage, min/mean edge confidence, structural distance, query-shape fit, optional lexical rank, optional graph prior, liveness/tests/config relevance, optional semantic rank. **Hard tiers are non-compensatory** — evidence mode and minimum edge tier resolve lexicographically before any scalar. A high lexical seed score is a prior, not proof. Arithmetic mean, never geometric. A learned overlay may rerank valid candidates but never resurrect a filtered generation, cross a grant boundary, convert unresolved→exact, bypass evidence, or override hard exclusions.

### 5.5 Budget ownership (standing)
See §2 conflict 5. When a cap binds: complete high-confidence path > complete lower-ranked path > disconnected high-score nodes; mark partial paths partial.

### 5.6 Cache identity (standing)
Correctness-sensitive caches key on content hash(es) + provider id/version + config/ruleset hash + schema version (+ query params where relevant). mtime/path alone is a fast-path prefilter only, confirmed by hash.

### 5.7 Determinism checklist (standing; enumerate as a checkable contract clause)
POSIX-normalise before hashing; every sort declares a total order; parallel work → immutable per-part deltas merged in sorted order; RNG seeded; env vars affecting output pinned in tests; tie-breaks explicit, never map-iteration order; any recency uses an **index-derived clock** (generation's max observed timestamp), never `Date.now()`.



### 5.8 Provider and cross-file resolution contract (standing)

Resolution is one shared pipeline, not provider-specific folklore. Every lexical, Tree-sitter, SCIP, compiler, or future LSP lane implements `CodeProviderV2` and reports capability/fallback state explicitly.

Canonical resolution cascade:

```text
R0 exact compiler/SCIP identity
R1 exact local lexical/scope identity
R2 explicit import/alias identity
R3 exact same-file definition
R4 module/package resolution
R5 re-export/SCC closure
R6 unique project qualified-name resolution
R7 unique project bare-name heuristic
R8 unresolved/ambiguous
```

Every resolved edge records `resolutionStrategy`, confidence tier, provider/version, evidence span(s), and candidate count where ambiguity existed. **If a stage has multiple equally valid targets, stop. A weaker stage may not choose a winner.** Module/package behavior is ecosystem-specific; MRO/inheritance rules are language-specific hooks, never one universal heuristic.

### 5.9 Source-span drift and citation contract (standing)

A reusable span carries file hash, span hash, position encoding, start/end convention, and symbol identity where available. Re-anchor only through the conservative ladder:

```text
exact enclosing symbol identity
→ exact text near original range
→ structural fingerprint
→ normalized/fuzzy text under a strict threshold
→ stale/ambiguous failure
```

Re-anchoring never silently attaches old evidence to a different location. Citation *strength* is separate from successful location: exact qualified/stable identity > exact path/span+hash > normalized identity > conservative re-anchor > ambiguous/unresolved.

### 5.10 Partial-failure contract (standing)

For batch, multi-provider, federation, or multi-branch work:

- preserve each independently valid result;
- emit per-item/provider error or omission records;
- aggregate status is `ok | partial | failed | cancelled`;
- provider failure may degrade its own lane but cannot erase valid output from other lanes;
- `cancelled` is not `failed`;
- a cancelled/failed build never publishes a partial generation;
- retryability, safe guidance, and partial-result availability are explicit fields, not inferred from message text.

### 5.11 History is evidence, not current truth (standing)

Named generations/treeishes, merge-base/PR targets, selected commit metadata, historical facts and co-change may be queried as an evidence lane. They do **not** pollute the current truth graph. Evolution statements such as “migration began here” remain `derived` and retain the exact commits/facts used to infer them.

### 5.12 Rollback and compatibility (standing)

New core behavior lands beside existing surfaces until it wins qualification. P0 RecallCircuit specifically preserves `scripts/cortex-candidates.mjs`, existing six MCP tools, current search/expand/impact/path/architecture/doc-truth contracts, and the existing graph schema. Deprecation happens only after a measured release window. A future migration must carry its own rollback proof; P0's no-migration rollback guarantee must not be generalized to later trains.

---

## 6. THE PROGRAM — ranked once, globally

Waves are sequencing, not separate backlogs. Do Wave 0 before Wave 1.

### Wave 0 — Latent bugs and free wins (days, no dependencies)

| # | Item | Status·Effort |
|---|---|---|
| 1 | **Honor SCIP `position_encoding` per document.** `src/graph/scip-provider.mjs` treats UTF-16 offsets as UTF-8; every span on a line with non-ASCII silently shifts. **The only live correctness bug in the corpus.** Done when an emoji/CJK fixture yields byte-identical spans under both encodings. | BUILD·S |
| 2 | Safe-parse guard on every tree-sitter entry point (chunked helper + lint rule; verify whether the 32,767-char SIGSEGV applies to `web-tree-sitter`). | BUILD·S |
| 3 | String/comment masking before reference resolution + import-backing for cross-file lexical edges. Kills a class of false `CROSS_FILE_HEURISTIC` edges. | BUILD·S |
| 4 | Builtin blocklist (~90 names) before resolution; callable-over-value tie-break; **drop, never guess** re-exports with two targets. | BUILD·S |
| 5 | Resolution-time stale-evidence guard: hash emitted span bytes vs. stored `contentHash`; mismatch → `STALE_EVIDENCE`. | BUILD·S |
| 6 | Index-derived clock (§5.7). Do before any churn/co-change signal. | BUILD·S |
| 7 | Determinism checklist as contract clause (§5.7). | EXTEND·S |
| 8 | `locate path:line` — narrowest enclosing span for a line. Prerequisite for 26/27. | BUILD·S |
| 9 | Adaptive-damping PageRank (0.92 DAG → 0.82 cyclic by SCC ratio), hard iteration cap returning best-so-far, degree-rank fallback. | EXTEND·S |
| 10 | Redact tokens in subprocess error text incl. URL-encoded forms. | AUDIT·S |
| 11 | Self-excluding pathspec `:(exclude).agent` on git reads; `.agent/` self-gitignored. Verify vs `src/graph/ignored-prefixes.mjs`. | AUDIT·S |
| 12 | Test-mode network tripwire that hard-fails any network path under a test sentinel. | AUDIT·S |

### Wave 1 — The honesty layer (1–2 weeks) — *the differentiator*

| # | Item | Status·Effort |
|---|---|---|
| 13 | **Per-edge `resolutionStrategy` + `AMBIGUOUS` class + `resolution_diagnostics` stream.** Persist strategy in `edges.extra` (no migration); `AMBIGUOUS` carries candidate count, distinct from `UNRESOLVED`; every resolution failure recorded with reason/caller/file/line; strict-vs-permissive traversal becomes a caller choice. | EXTEND·M |
| 14 | **Epistemic response envelope `exact | lower_bound`** with machine-readable causes on every bounded response: `epistemic`, `partial`/`truncated`, `*_total`, `readiness`, `provider_used`/`fallback_used`, per-family coverage. One shape, every surface. Counts *why* it is a floor (dropped call sites, DI boundaries, externals). Most-agreed mechanism with no Cortex counterpart. | BUILD·M |
| 15 | **Ingestion ledger** (`src/graph/ingestion-ledger.mjs`): every considered source ends in exactly one terminal state — `INDEXED · INDEXED_DEGRADED · SKIPPED_IGNORED · SKIPPED_BINARY · SKIPPED_OVERSIZE · SKIPPED_UNSUPPORTED · PARSE_ERROR · PROVIDER_TIMEOUT · PROVIDER_CRASH · READ_ERROR` with path, generation, provider, reason, fallback, lower-bound flag. VCS-first discovery dispositions feed it. Doctrine: *absence of a flag is not a completeness guarantee.* Ship **before** Wave 5. | BUILD·M |
| 16 | Parse-health bits + guarded extraction fallback ladder: never "no tree" — degraded tree + `has_error`/`error_cost`/`is_missing`; structural → positional-window fallback at LEXICAL tier; extractor failure becomes a finding. | BUILD·S–M |
| 17 | **Freshness as a typed state machine**: `fresh | dirty | stale{behind|ahead|diverged} | unknown{reason}` from `merge-base --is-ancestor` + `rev-list --left-right --count`; per-state remediation; footer on every response; **`unknown ≠ fresh`**; **read-repair** (bounded per-query stale sweep) so edit-then-query is honest before debounce. Dirty overlays (HEAD/index/worktree/treeish/merge-base) are part of request identity. | EXTEND·M |
| 18 | Edge provenance + liveness on every impact row: `via` edge, hop depth, score, tri-state `LIVE | UNREACHED | UNKNOWN` with `live_only`; structural edges excluded from fan-out; multi-seed keeps best score. Zero inbound ≠ dead. | BUILD·M |

Discovery accounting feeding item 15 is VCS-first where Git is available. Pre-ingestion dispositions must cover at least `indexed_exact | indexed_fallback | metadata_only | ignored_user | ignored_system | binary | oversized | unsupported | unreadable | nested_repo | external_link`, mapped into the terminal ledger without silent disappearance. Non-Git repositories retain a deterministic filesystem fallback. Dirty-query identity distinguishes `HEAD`, index, working tree, untracked files, named treeish and merge-base.

### Wave 2 — Measurement (1–2 weeks, parallel with Wave 1)

| # | Item | Status·Effort |
|---|---|---|
| 19 | Frozen eval corpus with sha256 checksums; `HoldoutViolationError` on tamper; pinned upstream SHAs. Do first. | EXTEND·S |
| 20 | Graded ranking metrics — nDCG@k, MRR, recall@k, node- and span-level P/R with `allowedAlternates` — computed **after every post-processor**. Gate on Waves 4–5 and any learned overlay. | EXTEND·M |
| 21 | **Incremental ≡ full-rebuild equivalence + commutativity** on canonical semantics. **Gate on all of Wave 5.** Requires 7. | EXTEND·M |
| 22 | A/B agent-efficiency harness: no-Cortex vs Cortex arm, pinned model, fresh clone, median+IQR; tokens, Cortex calls, tool calls, wall-clock **at equal task accuracy**. | BUILD·M–L |
| 23 | Whole-graph golden snapshots per fixture repo with `bless`; per-language snapshot matrix. | EXTEND·M |

Additional benchmark families to maintain (`bench/recall/`): exact-symbol, ambiguous-symbol, same-name-cross-module, diff-impact, test-selection, config-consumer, architecture-boundary, failure-stacktrace, long-chain, dense-hub, cycle, rename-move, stale-generation, unsupported-language, mixed-provider. Track separately: resolution P/R and ambiguity/unresolved honesty; freshness latency; resource envelope (peak RSS, p50/p95, cold/warm build, DB size, subprocess/handle counts, work avoided). Ranking changes fail if aggregate rises while protected exact-identifier cases regress.

### Wave 3 — Capability: mine what Cortex already reads

| # | Item | Status·Effort |
|---|---|---|
| 24 | **Mine the rest of SCIP** — largest capability gain per line. `SymbolInformation.kind` (87-value enum), `relationships` → `IMPLEMENTS`/`TYPE_DEF` at `EXACT_RESOLUTION`, full `SymbolRole` bitset (Write/Read/Import/Generated/Test), `enclosing_range`, per-occurrence diagnostics. SCIP is an **upgrade layer**: a SCIP edge supersedes a lexical edge for the same `(source,target)` with recorded tier upgrade; heuristic edge retained as evidence. Referential-integrity validation before publication. | BUILD·M |
| 25 | `scip lint` + `scip snapshot` at ingest; bad export degrades or fails typed. Pairs with 1. | BUILD·S–M |
| 26 | Diff-range → span scoping with a `ChangeTarget` primitive (`git diff --unified=0` hunks over evidence spans; treeish/merge-base classification). Turns `diff_impact` into "blast radius **of this diff**". | BUILD·M |
| 27 | Failure-signal resolution: stack traces, `path::test` ids, diffs as query inputs (`src/graph/change-seeds.mjs`). | BUILD·M |
| 28 | Co-change coupling from git history (`src/graph/history/cochange.mjs`): min 3 co-changes, ~6-month window, skip commits >50 files, hard timeout, no-op without git. Separate lower-authority family; ranking/risk only. Requires 6. | BUILD·M |
| 29 | Recommended test set + transitive coverage (`src/graph/test-selection.mjs`): `TESTED_BY` from Test-kind → non-Test calls; `selectedTests[]`, `uncoveredImpact[]`, `reasonsByTest{}`, `coverageState: exact|lower_bound|unknown`. Name it `recommendedTestSet`, never "minimal". | BUILD·M |

Also in this wave when justified: explainable change risk (`src/graph/change-risk.mjs`) as additive components — blast radius, edge uncertainty, interface change, liveness, critical config touch, churn/co-change, coverage blind spots; no black-box score.


---

### Wave 3R — Provider resolution and source precision — *mandatory restored workstream*

This workstream restores the Master Guide capability that the previous Source of Truth named in architecture/file ownership but did not put into the executable program. It is mandatory before broad new-language expansion and before Cortex can claim resolution completeness. **Do not duplicate items 13, 24, or 25**: those provide ambiguity metadata and SCIP mechanics; RES-1–RES-8 provide the shared architecture they plug into.

**Execution order note:** this subsection is placed after the original Wave 3 table to preserve the 1–53 source numbering. §10 is authoritative for execution: Train D (`RES-*` + items 24–25) completes before Train E items 26–29.

| ID | Work package | Action · primary files | Gate |
|---|---|---|---|
| **RES-1** | **One provider capability contract and doctor-visible matrix.** Lexical, Tree-sitter, SCIP and future compiler/LSP lanes expose `id`, `version`, `language`, capability vector, parse/symbol/reference/import/relationship operations, evidence/tier, soundness declaration and explicit fallback state (`provider_unavailable | unsupported | failed | skipped | incomplete`). | ADD/EXTEND `src/lib/contracts/**`, `schemas/**`, `src/sdk/types.d.ts`, `src/graph/language-registry.mjs`; adapt providers | One registration + fixtures adds a language/provider; unsupported capability is visible, never guessed |
| **RES-2** | **Shared exact-first resolution pipeline.** Implement §5.8 R0–R8 once. Persist strategy in `edges.extra`; same-tier ambiguity fails closed; weaker stages cannot override stronger ambiguity. | ADD `src/graph/resolution-pipeline.mjs`; MODIFY `src/graph/static-provider.mjs`, `src/graph/treesitter-provider.mjs`, `src/graph/scip-provider.mjs`, `src/graph/relationship-kinds.mjs` | Name-collision fixtures abstain; exact/SCIP edges outrank heuristics; diagnostics reconcile candidate counts |
| **RES-3** | **Import-map prepass.** Deterministic build artifact `local binding → imported module → imported/exported name → evidence`; intermediate artifact, not public ontology. | ADD `src/graph/import-map.mjs`; MODIFY language extractor/provider finalization | Alias/import fixtures resolve without repo-wide guessing; import evidence is inspectable |
| **RES-4** | **Bounded SCC/re-export closure.** Build module registry, identify SCCs, precompute re-export closure, resolve to bounded fixpoint, return ambiguous/unresolved unless unique convergence exists. | ADD under `src/graph/resolution/**` or canonical resolution owner; MODIFY import map/pipeline | Cycles terminate; double re-export ambiguity never becomes first/last-wins |
| **RES-5** | **Per-ecosystem module-resolution adapters.** Add adapters only with deterministic fixtures: Node/TypeScript package+relative resolution, Python package/module, Go module/package, Rust crate/module basics. Unsupported bundler/runtime semantics remain typed unsupported. | ADD `src/graph/module-resolution/**`; MODIFY registry/pipeline | Each adapter has positive, collision, missing, alias and unsupported fixtures; no adapter is required for unrelated languages |
| **RES-6** | **Language-specific inheritance/MRO hooks.** Type/method dispatch order belongs to provider/language hooks; no universal MRO. | EXTEND provider hook tables / language registry; relationship producers | Multiple-inheritance/override fixtures are correct for each supported language; unsupported dispatch remains explicit |
| **RES-7** | **Canonical span algebra, checksums and conservative re-anchor.** One inclusive/exclusive convention; union/intersection/contains/overlap/consolidate/clip; byte/line conversion where encoding allows; file+span hash; §5.9 re-anchor ladder. | ADD/EXTEND `src/graph/source-span.mjs`, `src/graph/source-address.mjs`; MODIFY evidence packs, snapshots, SDK/result renderers | Drift fixture relocates only uniquely; ambiguous relocation returns stale/ambiguous; old evidence never silently moves |
| **RES-8** | **Resolution/source-precision qualification matrix.** Per-language/provider snapshots and doctor coverage: exact-target P/R, ambiguity honesty, unresolved honesty, import/re-export closure, position encoding, unsupported module semantics. | EXTEND `tests/**`, `evals/**`, `bench/recall/**`, `src/lib/operations/doctor.mjs` | Protected exact identifiers cannot regress; every provider/language cell has capability/fallback state |

Implementation order inside the train: **RES-1 → RES-2/RES-3 → RES-4/RES-5/RES-6 → RES-7 → RES-8**. RES-2 and RES-3 may be built together. RES-5/RES-6 graduate one language at a time; do not create a universal language mega-branch.

### Wave 4 — Packing and retrieval (2–4 weeks; gated on Wave 2)

| # | Item | Status·Effort |
|---|---|---|
| 30 | **Assemble-to-budget** as one named entry point on `src/lib/token-budget.mjs`: route decision before assembly (`FITS | COMPACT | TRUNCATE`); residual = ceiling − prompt − meta − included − buffer, filled by lane priority; **named drop-steps** each with a reason code; admitted refs written back into visible selection; usage footer + per-candidate cost. **Train C pulls the narrow circuit assembler portion forward as a RecallCircuit prerequisite; Train F completes the broader packing behavior.** | BUILD·M |
| 31 | **RRF (k=60)** for independently-ranked lanes (`src/graph/ranking/rrf.mjs`); weighted sum for aligned components; rank-based fusion then rescaled display scores; per-source timeouts → `DEGRADED_SOURCE`; pool 3× limit capped at 300. | BUILD·M |
| 32 | Skeleton/outline tiers before full bodies: outline → module API → signature → body; oversized symbol emits signature + child list + expansion pointers; content hashes attached so summaries are verifiable. | BUILD·M |
| 33 | Lexical query-shape classifier (`src/graph/query-shape.mjs`): `SYMBOL FILE IMPACT PATH TEST CONFIG ARCHITECTURE DOC_TRUTH FAILURE EXPLORE`; steers weights/traversal, never facts; explicit intent wins; reported in explanation. | BUILD·S |
| 34 | Canonical `Interval`/`Span` + one path normaliser (§5.2). | EXTEND·M |
| 35 | Ranking component expansion + non-compensatory tiering (§5.4): identifier salience multipliers, two-pass test/infra demotion with `queryTargetsTests()` opt-out, dependency-origin demotion, special-files prior. | EXTEND·M |
| 36 | Bounded rendering (`src/graph/recall-render.mjs`): `minimal | standard | verbose`; token estimate on actual wire encoding; suppress not truncate partial lists; **generation-checked cursors** fail closed after reindex; never orphan a node from its justifying edge. | BUILD·M |

Also: search strategy router (exact → direct index; literal → FTS; prefix; sanitized glob; structural → matcher; relationship → graph), FTS query sanitization, minimal structural matcher (node-kind pruning, ancestry predicates, metavariables, boolean match sets — **no rewrite runtime**), query-sensitive priors and diversity controls.

### Wave 5 — Identity and incrementality (3–6 weeks) — *most valuable, most dangerous*

Gate **every** step on item 21.

| # | Item | Status·Effort |
|---|---|---|
| 37 | Route-derived `definitionId` alongside `contentHash` (keep the latter unchanged — it is the verification primitive). Structural fingerprint hashes only named node kinds with depth, never identifiers/literals; 3-gram shingles for Jaccard; overload collision → position-sensitive id. | BUILD·M |
| 38 | Rename/move continuity as a deterministic reconciler (`src/graph/identity-reconciler.mjs`): exact stable id → exact content hash → structural fingerprint → unique qualified route → new identity; **never choose among equal candidates**; typed `SAME/MOVED/RENAMED/MOVED_RENAMED/AMBIGUOUS` events in a separate `identity_events` table (P1 migration justified here). Biggest single freshness win. | BUILD·M |
| 39 | Change classification with update-scope matrix: `UNCHANGED COSMETIC STRUCTURAL_LOCAL STRUCTURAL_INTERFACE ADDED REMOVED RENAMED_OR_MOVED PROVIDER_INVALIDATED CONFIG_INVALIDATED UNKNOWN`, with named details; missing structural analysis on either side → STRUCTURAL; `UNKNOWN` takes the full path. Maintain three distinct reusable fingerprints: **content fingerprint** (exact verification), **structural fingerprint** (AST shape independent of identifiers/literals), and **extraction fingerprint** (deterministic graph-extraction output for the entity/file). Structural sameness alone never proves graph-fact sameness. | BUILD·M |
| 40 | Header-aware invalidation then closure repair: `filesToClear = changed ∪ getReferencing(changed)`; per-file `surface_sha` + referenced-identifier bloom; route enum `NOOP / FORCED_FULL / LEGACY_PARTIAL / CLOSURE_REPAIR`; **parse scope ≠ write scope**; escalation at >50% repo ∧ ≥50 files; `incrementalInProgress` dirty flag forces full rebuild after crash. Maintain **one explicit invalidation dependency DAG**: source bytes → parse artifact → entity extraction → identity/occurrences/local facts/import-export surface → resolution → edges → derived analyses/RecallCircuit candidates/optional semantic overlays, with provider/version/config/schema/generation as additional correctness parents where applicable. Every derived artifact declares its invalidation parents; invalidation propagates through this DAG rather than through duplicated feature-specific rebuild rules. | BUILD·M–L |
| 41 | `ts_subtree_get_changed_ranges` for narrowed re-extraction + CoW `ts_tree_edit`; feature-flagged until fixture equivalence. After narrowed extraction, compare the deterministic **extraction fingerprint**: unchanged extraction may reuse downstream graph deltas; changed extraction repairs only its declared invalidation descendants. *Incrementality may reduce work; never soundness.* | BUILD·M |
| 42 | Version-stamped cache keys `sha256(contentHash + providerId + providerVersion + languageId + grammarVersion + extractionSchemaVersion)`; `NORM_VERSION` filter-on-read (schema evolution without migration); never-cache-empty unless explicit successful empty parse; corruption is a miss. | EXTEND·S–M |
| 43 | Watcher hardening: generation-counter debounce, debounce on last event, drain in-flight on stop, prefix removal for dir deletes, exclude own output, `mkdir` lock, **`MAX_DIRTY_AGE` starvation guard**. | EXTEND·M |

### Wave 6 — Store hygiene, truth, operations

| # | Item | Status·Effort |
|---|---|---|
| 44 | Store hygiene: halving-size batched inserts under the 999-var cap; in-transaction dedupe maps; edge identity as UNIQUE index; reverse-edge covering index `(generation_id, target, kind, source)` pinned by `EXPLAIN QUERY PLAN` test; generated-column JSON indexes; WAL checkpoint valve. Fastest real perf win. | BUILD·M |
| 45 | Staged build → verify → quarantine → atomic swap at the store-file level; quarantine old DB + `-wal`/`-shm`; classified storage failures flip read-only with typed diagnostics. Durable op state machine `started → committing → complete/failed`; flush-before-metadata. | EXTEND·M |
| 46 | Lock-vs-corruption discrimination before any destructive recovery; graduated backoff; **never wipe before retry budget exhausted**. | BUILD·S–M |
| 47 | Findings registry: ids `hash(kind+subject+content)`, UNIQUE upsert, `supersedes_id`, suppressions; **stale facts degrade, never vanish**; re-check after rebuild. Build only when ≥2 producers need it. | BUILD·M |
| 48 | Citation re-location: search quoted snippet in file bytes, overwrite stated span; ladder exact → case-insensitive → stripped → blank-lines-removed at degraded tier with `anchor_relocated`; failure → `UNVERIFIED_QUOTE`. | BUILD·M |
| 49 | Evidence-coverage admission gate: cited `path:start-end` must match a span whose `contentHash` is in the referenced generation, else `UNSUPPORTED_SPAN`. Citations resolvable but claim ungrounded → **Stale**, not Deleted. Truth binding (`src/graph/truth-binding.mjs`) — claims enter a circuit only via explicit tested join; no text-similarity contradictions. Grounding evaluator outcomes: `grounded_direct | grounded_indirect | unsupported | contradicted | ambiguous | stale_reference`. Typed invalidation reasons, never bare `stale`. | EXTEND·M |
| 50 | Doctor reports findings, not counts (`src/lib/operations/doctor.mjs`): "18 unresolved imports (12 heuristic-eligible, 6 unsupported) · 4 ambiguous same-tier · 2 docs cite superseded files · 93.2% exact/AST coverage"; `reason`/`source`/`suggestions[]`. Rule metadata (`id, category, severity, precision, requires, fixability`); analyzer planning; auditable suppressions. | EXTEND·S–M |
| 51 | Crash-isolated provider execution: out-of-process worker; `PROVIDER_CRASH` finding naming affected files; quarantine list with `crash` vs `hang` phase. | BUILD·M |
| 52 | Thread cancellation through provider parse loops; await-before-drop; **cancellation is its own typed outcome, not failure**; never publish a partial generation. Bounded worker pools per class; singleflight. | EXTEND·M |
| 53 | Glossary discipline in CI + metric ratchet (exact-resolution %, claim fidelity non-decreasing across tagged commits). | BUILD·S |

Also: generated-artifact registry (`path, generator, inputDigest, renderHash, formattedHash, generation`; unchanged render → zero writes; hand-edit → typed failure; CI double-run proves zero bytes); shareability classification (`public_structure | repository_content | sensitive_source | secret_material | machine_local`); provider locality classes with remote providers excluded from the default trusted path; refuse cloud-synced mutable store paths.

### Wave 6T — Truth and history completion — *mandatory restored workstream*

| ID | Work package | Action · primary files | Gate |
|---|---|---|---|
| **TH-1** | **Citation-strength ladder.** Normalize reference strength independently from truth confidence: exact qualified identity → exact stable symbol → exact path/span+matching hash → normalized identity → conservative re-anchor candidate → ambiguous/unresolved. Lower strength remains visible. | EXTEND `src/graph/source-span.mjs`, evidence/result contracts, truth binding | Same fact reached through weaker addressing cannot be presented as equally strong citation evidence |
| **TH-2** | **Declared-vs-done drift.** Compare architectural/implementation declarations in docs/plans against current deterministic code evidence; preserve both declaration and current-code evidence; emit typed drift finding, never silently rewrite either side. | ADD/EXTEND truth/findings modules, likely `src/graph/truth-binding.mjs` + diagnostic producer | Fixture distinguishes current, stale declaration, contradicted declaration and unsupported declaration with exact evidence links |
| **TH-3** | **Named snapshot/treeish semantic diff.** Compare named generations/treeishes and return changed semantic entities/relationships, not only files; merge-base/PR target is a first-class input. | EXTEND `src/graph/snapshots.mjs`; ADD `src/graph/history/**` as needed; service/CLI adapter | Same semantic change is visible across commit/treeish boundaries with generation-bound evidence |
| **TH-4** | **History evidence lane + derived evolution.** Search/recall selected commit metadata and historical facts without mutating current truth; evolution inference is derived and cites the commits/facts; co-change stays lower authority. Selective bitemporality (`valid_generation`, `recorded_at`, `superseded_at`) only where generation identity alone is insufficient. | ADD/EXTEND `src/graph/history/**`, evidence contracts, query routing | Historical recall cannot overwrite current fact state; inferred evolution is visibly derived; co-change cannot satisfy exact dependency gates |

---

## 7. RecallCircuit — the execution primitive

### 7.1 Goal
From `search → nearby nodes → model reconstructs relationships` to `seed resolution → deterministic predicate-aware traversal → complete evidence paths → ranked circuit → caller decides`. **The model does not perform multi-hop graph retrieval.**

### 7.2 File set
Add: `src/graph/traversal-policy.mjs`, `src/graph/seed-resolver.mjs`, `src/graph/recall-circuit.mjs`, `scripts/cortex-recall.mjs`, `schemas/recall-circuit-v1.schema.json`, `tests/recall-circuit{,-adversarial,-benchmark}.test.mjs`.
Modify: `src/graph/traverse-store.mjs` (indexed lookup, no primary `%LIKE%`), `src/graph/store-sqlite.mjs` (shared bounded traversal, generation-scoped membership), `src/lib/application/service.mjs` (`recall()`), `schemas/catalog.json` (register V1 only after full typing).
Do not change: builder architecture, SQLite schema (for this), MCP tool count, existing search/expand/impact/path/architecture/doc-truth contracts, vector defaults, Membrane schemas.

### 7.3 Policies and seeds
Policy table: `dependency.forward · impact.reverse · callgraph.forward · test.coverage · config.consumers · architecture.boundary · explore.both`; each declares direction, allowed kinds, max hops/seeds/paths/nodes/edges. Callers may tighten, never silently expand.
Seed precedence: generation-valid node IDs → exact anchors/paths/addresses → indexed symbol terms → exact qualified/name/path matches → bounded file-path fallback → unresolved/ambiguous. No repo-wide wildcard scan on the normal path.

### 7.4 Ten mandatory corrections (the original patch is not to be implemented as written)
1. Enforce `maxNodes`/`maxEdges` **during** frontier expansion.
2. Generation-check every explicit seed/anchor before hydration.
3. `evidenceRequired` → explicit `complete | partial | off`; public `true` = `complete`.
4. Terminal filtering inside projection, **before** `circuitId`.
5. Omissions counted by reason, not one difference.
6. Deduplicate equivalent paths before ranking/slicing.
7. Seed score is a prior; evidence/tier-guarded ordering first.
8. Type all public arrays/objects before catalog registration.
9. **One traversal primitive** — `indexedNeighbors`, `impact`, recall converge; `cortex-candidates.mjs` becomes an adapter.
10. Benchmark SQL work, not row counts.

### 7.5 Path ranking (non-compensatory)
admissibility/evidence mode → min edge tier → seed exactness → evidence coverage → mean edge confidence → hop count → deterministic tiebreak.

### 7.6 Test corpus
Three-hop chain, unknown vocabulary, predicate filtering, direction, generation pinning, evidence filtering, dense hubs, decoy high-text-match nodes, ambiguous same-name, cycles, stale generations, terminal projection, duplicate paths, budget truncation. Record seed/SQL/hydration/total time, visited vs hydrated vs returned, truncation reason, query plans. No O(all symbols)/O(all edges) on normal paths.

### 7.7 Qualification
Shadow-compare vs legacy candidates on task accuracy, path correctness, tokens, latency, SQL work, ambiguous/stale handling (item 22 numbers). Do not retire legacy until it wins.

### 7.8 Consumer seam (Membrane) — obligations Cortex must meet
This subsection is the complete Cortex-side seam authority at the pinned baseline. `docs/plans/orthic/SEAM-CONTRACT.md` is not present in Cortex at `a91909c`, so it is not a hidden prerequisite. `CORTEX_IMPLEMENTATION_GUIDE.md` is already absorbed here and is historical input only. Membrane remains a separate system; what follows is only what Cortex must emit at the seam.

- **Thesis** (graph-memory-starter): spend intelligence at build; answer from structure; the model receives completed chains, not search results. Cortex traverses; Membrane plans, admits, budgets, renders. Neither copies the other's policy or store.
- `scripts/cortex-recall.mjs` ships **alongside** `scripts/cortex-candidates.mjs`; Membrane prefers recall when present and falls back to candidates for version skew. Lean import graph only (no MCP SDK, install, build provider, Phase-2).
- `--expected-generation <id>` mismatch → non-zero exit, no circuit. Fail closed.
- **A complete path is the semantic unit.** Consumers treat one path as one atomic candidate, so Cortex must emit per path: `complete`, `evidenceComplete`, `evidenceCoverage`, stable `path.id`, and a stable `circuitId` computed over the visible projection.
- Empty circuit → `paths: []`, `unresolved: [{reason: "no_relevant_seed"}]`. Never pad with generic nodes; abstention is a valid result.
- `claims[]`/`contradictions[]` stay empty until the explicit node/edge → claim join exists (item 49). Never fill from unrelated doc-truth rows.
- Cortex knows nothing of the caller's planning, admission, lanes, or rendering. Policy id / hops / paths arrive as request parameters; Cortex clamps them to policy maxima and reports what it used.
- Never claim "constant time regardless of corpus size." Claim bounded output + indexed seed + indexed traversal, proven by `EXPLAIN QUERY PLAN` and the 10k/100k/500k-edge benchmark.

### 7.9 Rollout and rollback contract

1. **C1 — indexed seed path:** change `src/graph/traverse-store.mjs`; add seed/query-plan tests; no public surface change.  
2. **C2 — RecallCircuit core:** add policy, seed resolver, shared bounded traversal, circuit, schema and tests; no Membrane dependency.  
3. **C3 — lean entrypoint/service:** add `scripts/cortex-recall.mjs` and `service.recall()`; MCP remains frozen.  
4. **C4 — shadow qualification:** legacy candidates vs RecallCircuit on path recall/correctness, false/decoy paths, no-seed abstention, dense hubs, tokens, latency and SQL work.  
5. **C5 — consumer adoption:** only after Cortex contract tests and shadow gates pass.

Rollback remains deliberately cheap for this train: the caller falls back to the existing `scripts/cortex-candidates.mjs`; existing graph/query surfaces remain untouched; P0 has no required DB migration, so no store downgrade is necessary. Do not retire or rewrite the fallback until a qualified release window proves RecallCircuit superior.

Canonical baseline test commands are the repository scripts: `pnpm test`, `pnpm run test:all`; use targeted hardening/security/benchmark commands in addition where the train touches those domains.

---

## 8. Rejected — recorded so it is not re-evaluated

- Second production graph backend (FalkorDB/Kùzu/Neo4j/LanceDB/Milvus/RocksDB); mandatory hosted vector service.
- Identity containing line numbers/paths in content hashes; UUID chunk ids; autoincrement ids; `(name,path,line)` tuples.
- mtime-only staleness as primary signal; `chars/4` as primary budget (documented floor only).
- LLM-delegated analysis with no static core; LLM extraction/curation; remote LLM indexing as correctness dependency; whole-repo QA dumps; LLM-generated docs/architecture as truth; prompt-shaped strings as graph facts.
- Full AST rewrite engine (ast-grep/GritQL runtimes); CodeQL/Joern clone; general query DSL; universal CPG rewrite.
- Generic writable MCP; raw graph in durable agent memory; freeform semantic summaries as truth; popularity telemetry as ranking authority; repo-wide semantic ranking overriding exact evidence; learned correction bypassing sentinels.
- Generic orchestration runtime; one parser replacing all fallback tiers; cloud/shared mutable SQLite under forced locking.
- Build-system filler (Nix, devcontainers, Bazel, goreleaser, Docker matrices, k8s) — keep only changelog fragments and marker-fenced git hooks.
- The four misidentified `semantic`/`semantica` entries.

## 9. Parked / experimental — shape decided now, built when the lane exists

- **Vector lane:** same SQLite file (`sqlite-vec vec0`), model-ID watermark forcing re-embed, keyed on evidence-text hashes, brute force <~200K then HNSW, quarantine mismatched dimensions on load; semantic score never evidence confidence; failing branch never disables deterministic branches; exact-search non-regression, RSS/cold-start proof, measurable gain before default.
- **LLM-assisted surface:** salvage ladder (strict → truncation salvage → cleanup → regex → `PARSE_DEGRADED`); duplicate-key rejection + depth cap; verified-skeleton rule (`ENRICHMENT_DEGRADED` never invalidates the graph).
- **Statement-level analysis / dataflow / taint:** opt-in overlay registry (`src/graph/analysis/overlay-registry.mjs`) declaring languages, producer version, cost class, soundness, invalidation unit, edge kinds; def-use first where local bindings are deterministic; build-time materialised edges with bailout thresholds.
- **Compiler/LSP providers:** one language at a time on measured gain; never require a language server to support a language.
- **Cross-repo:** typed contract edges with match-cascade provenance, capped fan-out with `riskEpistemic: 'lower_bound'`, **never merge node spaces**.
- **Structural DSL, generated wiki/summaries (disposable, cited), cross-machine delta exchange, learned ranker, MinHash clone edges, graph communities:** benchmark-only until proven.
- **Other benchmark-only retrieval/memory ideas:** random-indexing/no-model semantic vectors and agent-transcript harvesting may be evaluated only as non-authoritative derived inputs; they cannot become durable conversational memory or graph truth.

---

## 10. Release trains — executable dependency order

The Unified List is globally ranked; the release trains below are the implementation dependency graph. A later-ranked prerequisite may be pulled forward narrowly when a core train needs it. That is why item 30's assembler lands with RecallCircuit while the rest of packing remains later.

| Train | Content | Entry gate | Exit gate |
|---|---|---|---|
| **A — correctness/free wins** | Items **1–12** | none | Wave 0 tests green; no known live correctness bug left unaddressed |
| **B — honesty + contracts + measurement** | Items **13–23**; §5.1 contracts; §5.10 partial-failure semantics | A | honesty envelope works across bounded surfaces; frozen corpus/metrics available; adapters bridge legacy shapes |
| **C — RecallCircuit** | §7; items **13,14,18,30** must be available; service + lean CLI; MCP frozen | B foundations + items 19–22 available | §7.7 shadow qualification wins; rollback path proven; no legacy retirement yet |
| **D — resolution/source precision** | **RES-1–RES-8** + SCIP items **24–25** | B; C may proceed in parallel after contract stabilization | exact-first shared resolution, module/re-export closure, source re-anchor and per-language/provider qualification green |
| **E — change intelligence** | Items **26–29** + explainable change risk | D resolution semantics available | diff/failure seeds, impact provenance, tests, liveness and lower-authority co-change qualified |
| **F — packing/retrieval** | Item **30** if not already complete, then **31–36**, query router, minimal structural matcher | items 19–21 green; C circuit shape stable | RRF/query-shape/rendering benchmarks green; complete-path/omission semantics preserved |
| **G — identity/incrementality** | Items **37–43** | item 21 green continuously; item 15 shipped | every step preserves incremental≡full semantics; crash recovery route proven |
| **H — store/truth/history/ops** | Items **44–53**, **TH-1–TH-4**, generated registry, shareability/locality, §5.10 runtime partial-failure rules | relevant earlier contracts stable | truth/history/diagnostic/security/recovery DoD clauses green; last-known-good and rollback behavior proven |
| **I — experiments** | §9 bakeoffs only | A–H core baseline complete enough to compare fairly | each experiment graduates independently on measured gain; no mega-branch |

### Train discipline

- **One canonical owner, one migration path, one benchmark.** Do not create parallel implementations to “try both” unless the work is explicitly an experiment.
- **Adapters are temporary.** They bridge old contracts while callers migrate; once qualification and a release window are complete, delete the duplicate shape/policy.
- **No hidden prerequisites.** If a train depends on an external consumer contract, Cortex's required side of that contract must be duplicated here or in a repository file that actually exists.
- **No false completion.** A train is not complete while a mandatory RES/TH work package or its gate is outstanding.

### Baseline verification commands

These commands exist in the pinned repository `package.json` and are the default verification surface; add focused tests/benchmarks for the train rather than replacing them:

```sh
pnpm test
pnpm run test:all
pnpm run test:hardening
pnpm run test:security
pnpm run bench:retrieval
pnpm run soak
```

`pnpm test` is the normal root suite; `test:all` includes workspace coverage. Hardening/security/benchmark/soak commands are mandatory when the changed train touches those domains. Qualification claims still require the specific frozen-corpus/query-plan/AX gates named in this document; a green generic test suite alone is insufficient.

Deprecate **after qualification**: primary `%LIKE%` scans; independent legacy candidate logic; provider-specific duplicated resolution heuristics; mtime/path-only correctness caches; duplicate ranking policy in CLI/MCP; opaque truncation; duplicate evidence/result shapes.

Keep: SQLite graph, generation envelope, freshness barrier, current query surfaces, Tree-sitter/SCIP/lexical ladder, Phase-2 verification, evidence/provenance, Merkle reconciliation, blast-radius/path/architecture primitives, scoped federation, off-by-default embeddings.

---

## 11. Canonical file ownership and implementation map

A cross-cutting feature gets **one canonical owner plus adapters**. Do not copy policy into CLI, MCP, SDK and internal callers.

| Concern | Canonical owner / baseline action |
|---|---|
| store/generation schema | `src/graph/store-sqlite.mjs` + migrations — EXTEND only when a train truly needs persistent new shape |
| source address/span contracts | ADD/EXTEND `src/graph/source-address.mjs`, `src/graph/source-span.mjs`; mirror through `src/lib/contracts/**`, `schemas/**`, `src/sdk/types.d.ts` |
| evidence/provenance/result contracts | `src/lib/contracts/**`, schemas, SDK; `EvidenceRefV2`, `CortexResultV2`, omissions/errors/claim boundary are canonical wire shapes |
| freshness/admission | `src/graph/barrier.mjs` + freshness modules; dirty overlay/treeish is request identity |
| discovery/ingestion accounting | ADD `src/graph/ingestion-ledger.mjs`; integrate VCS-first discovery dispositions |
| delta/change classification | `src/graph/delta-store.mjs`; ADD/EXTEND `src/graph/change-classifier.mjs` |
| parse artifact cache | EXTEND `src/graph/parse-cache.mjs` with complete correctness key and never-cache-failed/ambiguous semantics |
| identity continuity | `src/graph/generation-identity.mjs`; ADD `src/graph/identity-reconciler.mjs`; identity events remain separate from dependency edges |
| provider registry/contract | `src/graph/language-registry.mjs`, providers, `src/lib/contracts/**`; one capability matrix |
| provider extraction | `src/graph/static-provider.mjs`, `src/graph/treesitter-provider.mjs`, `src/graph/scip-provider.mjs` |
| cross-file resolution | ADD `src/graph/resolution-pipeline.mjs`, `src/graph/import-map.mjs`, `src/graph/module-resolution/**`; SCC/re-export finalization under this owner |
| relationship vocabulary/confidence | `src/graph/relationship-kinds.mjs`, `src/graph/confidence-tiers.mjs`, `src/graph/precision-tiers.mjs`; every new edge obeys §5.3 |
| seed lookup | ADD `src/graph/seed-resolver.mjs`; indexed, generation-scoped, no normal repo-wide wildcard scan |
| traversal policy | ADD `src/graph/traversal-policy.mjs`; callers may tighten but never silently widen bounds |
| shared traversal primitive | store/traversal modules; RecallCircuit, indexed neighbors and impact converge on it |
| recall execution/rendering | ADD `src/graph/recall-circuit.mjs`, `src/graph/recall-render.mjs`; `scripts/cortex-recall.mjs`; service adapter |
| analysis overlays | `src/graph/analysis/**` with overlay registry; deterministic def-use first where justified |
| change intelligence | ADD/EXTEND `src/graph/change-seeds.mjs`, `src/graph/test-selection.mjs`, `src/graph/change-risk.mjs`, liveness/history modules |
| history/snapshots | EXTEND `src/graph/snapshots.mjs`; ADD `src/graph/history/**`; historical evidence never mutates current truth |
| ranking/query shape | ADD `src/graph/ranking/**`, `src/graph/query-shape.mjs`; preserve/benchmark `src/graph/neighborhood.mjs` |
| structural query | `src/graph/query/**` / `src/graph/generic-ast-walker.mjs`; matcher only, no rewrite runtime |
| truth binding/findings | ADD/EXTEND `src/graph/truth-binding.mjs`, findings modules; explicit fact↔claim joins only |
| diagnostics/doctor | `src/lib/operations/doctor.mjs`, `src/graph/analytics/**`, diagnostic schemas; findings over vanity counts |
| generated artifacts | `src/lib/generated-docs.mjs` + generated registry/verifier; unchanged render → zero write |
| public application API | `src/lib/application/service.mjs`; one service-layer behavior, adapters above it |
| lean machine entrypoint | ADD `scripts/cortex-recall.mjs`; keep `scripts/cortex-candidates.mjs` during rollout/rollback |
| MCP | `scripts/cortex-mcp.mjs` / MCP modules; six-tool freeze until deliberate version bump |
| budget | `src/lib/token-budget.mjs`; Cortex bounds result, caller owns final prompt budget |
| concurrency/provider isolation | ADD/EXTEND `src/lib/concurrency/limiter.mjs`, worker/provider boundary; singleflight and per-class limits |
| security/redaction/confinement | `src/lib/redaction.mjs`, `src/lib/path-confinement.mjs`, export/shareability modules; audit existing primitives before adding |
| qualification | `tests/**`, `evals/**`, `bench/**`; frozen holdout, graph goldens, query plans, AX/resource metrics |

### Implementation rules

1. Search the pinned/current tree before adding a named module. If equivalent behavior already exists under another canonical owner, **extend and consolidate** rather than clone it.
2. Public schemas register only after every nested array/object is typed and adapters/tests are ready.
3. Store migrations are justified only by durable semantics, not convenience. `edges.extra` already handles resolution-strategy metadata; RecallCircuit P0 itself needs no migration.
4. Any new cache/index/provider records the exact versions/configuration that affect correctness and is rebuildable.
5. Any new relationship kind passes the §5.3 mini-RFC before code lands.
6. Any new sophisticated ranker/analysis lane ships behind a benchmark gate and cannot change hard truth/admissibility semantics.
7. Tests assert **behavior and epistemic honesty**, not just that text/fields exist.

---

## 12. Definition of Done — absorption program complete only when all are true

### 12.1 Graph truth and provenance

- [ ] Every durable fact has source, provider/version, confidence, generation and truth/freshness semantics.
- [ ] Multi-source provenance is set-merged; one rerun does not overwrite independent evidence.
- [ ] Contradictions coexist and remain queryable.
- [ ] Learned/semantic output cannot masquerade as observed truth or upgrade evidence confidence.
- [ ] Entities remain distinct from declaration/definition/reference/test/doc/historical occurrences.

### 12.2 Coverage and epistemic honesty

- [ ] Every considered/discoverable input reaches exactly one terminal ingestion disposition.
- [ ] Every language/provider cell reports capability and fallback state.
- [ ] Every bounded answer reports `exact | lower_bound`, omissions by reason, readiness/provider/fallback and freshness; `unknown ≠ fresh`.
- [ ] Doctor reports blind spots, ambiguity, unsupported semantics and degraded lanes as actionable findings, not only aggregate counts.
- [ ] Batch/multi-provider work preserves independent successes and exposes partial failure per §5.10.

### 12.3 Recall/query execution

- [ ] RecallCircuit performs generation-pinned, bounded, predicate-aware traversal with bounds enforced during expansion.
- [ ] Exact/indexed identifiers dominate fuzzy/semantic retrieval; no normal symbol query scans all symbols.
- [ ] Dense hubs are bounded during traversal; cycles terminate; equivalent paths dedupe deterministically.
- [ ] Every returned path exposes exact node/edge identity, `complete`, `evidenceComplete`, evidence coverage and stable path/circuit identity.
- [ ] Unknown/ambiguous seed and generation mismatch fail closed; empty recall abstains rather than padding generic nodes.
- [ ] No LLM performs graph traversal; Cortex emits evidence structures, not prompt prose.
- [ ] Legacy candidate fallback remains available until RecallCircuit wins its release-window qualification.

### 12.4 Provider resolution and source precision

- [ ] `CodeProviderV2`/equivalent capability contract and fallback taxonomy cover all supported lanes.
- [ ] Shared R0–R8 resolution pipeline owns finalization; provider-specific duplicate heuristics are removed after migration.
- [ ] Same-tier ambiguity never selects a target; a weaker stage cannot override a stronger-stage ambiguity.
- [ ] Import aliases/maps resolve deterministically; SCC/re-export cycles converge only under bounded rules.
- [ ] Module-resolution unsupported cases are explicit; language-specific inheritance/MRO behavior is tested where supported.
- [ ] SCIP position encoding, kind/roles/relationships/enclosing ranges and referential integrity are validated.
- [ ] Canonical span algebra has one position convention; reusable spans carry file/span hashes and encoding.
- [ ] Re-anchoring follows §5.9 and never silently binds old evidence to a different location.
- [ ] Every new relationship kind has complete semantics, evidence, invalidation and adversarial fixtures.

### 12.5 Incrementality and identity

- [ ] Incremental builds equal clean full builds semantically and are deterministic under randomized file order.
- [ ] Correctness-sensitive caches are keyed by content + provider/config/schema versions; corruption is a miss, not truth.
- [ ] Provider/config/schema changes invalidate exactly the affected scope.
- [ ] Content, structural and extraction fingerprints are distinct and tested; structural sameness cannot suppress a changed deterministic extraction. Every derived incremental artifact declares invalidation parents, and dependency-DAG propagation produces the same canonical semantics as a clean rebuild.
- [ ] Rename/move continuity is evidence-based; equal identity candidates remain ambiguous.
- [ ] Changed-range extraction remains feature-gated until it preserves full-build semantics.
- [ ] Watcher/crash paths cannot strand a dirty partial state that later masquerades as complete.

### 12.6 Change intelligence

- [ ] Diffs, stack traces, tests and file:line anchors resolve to narrow graph seeds.
- [ ] Impact rows expose exact `via` provenance, distance and confidence/epistemic state.
- [ ] `recommendedTestSet` returns reasons and uncovered impact; it is never called “minimal” without proof.
- [ ] Liveness is `LIVE | UNREACHED | UNKNOWN`; zero inbound is not dead-code proof.
- [ ] Co-change/history signals are visibly lower authority than semantic dependency truth.
- [ ] Change risk is decomposable into named components rather than an opaque learned scalar.

### 12.7 Retrieval, ranking and bounded output

- [ ] Exact/lexical/graph/structural branches have frozen benchmarks and selected query strategy is inspectable.
- [ ] RRF fuses independent rankings without converting rank into evidence confidence.
- [ ] Ranking contributions are inspectable and hard tiers/exclusions are non-compensatory.
- [ ] Semantic/vector branches may fail or be disabled without correctness loss.
- [ ] Complete evidence paths are preferred under caps; no trim creates orphan nodes/edges or hidden partial lists.
- [ ] Pagination cursors bind to generation+circuit digest and fail closed after reindex.
- [ ] Cortex result caps and cost estimates never become final model prompt policy.

### 12.8 Truth, citations, history and diagnostics

- [ ] Selected graph facts join to claims/findings only through explicit tested evidence/occurrence links.
- [ ] Grounding distinguishes direct, indirect, unsupported, contradicted, ambiguous and stale-reference states.
- [ ] Text similarity alone cannot create truth binding or contradiction.
- [ ] Citation-strength tier is visible independently from claim confidence; weak re-anchor cannot masquerade as exact identity.
- [ ] Declared-vs-done drift preserves both declaration and current-code evidence.
- [ ] Named snapshot/treeish diff returns semantic entity/relationship changes and accepts merge-base/baseline inputs.
- [ ] Historical evidence can be recalled without mutating current truth; inferred evolution is marked derived with receipts.
- [ ] Findings use stable semantic IDs/lifecycle where ≥2 producers justify a shared registry; suppressions are centralized/auditable.
- [ ] Diagnostic/fallback/omission reason codes are stable wire contracts; baselines may classify known findings but never hide them.

### 12.9 Runtime, recovery and rollback

- [ ] Provider failures are isolated and typed; crash/hang scope names affected files/provider.
- [ ] Cancellation reaches long provider loops, returns `cancelled`, and cannot publish a partial generation.
- [ ] Bounded worker pools/singleflight prevent uncontrolled RSS/subprocess multiplication and expose queue vs execution time.
- [ ] Candidate generations pass schema/envelope/referential/Merkle/provider-version checks before adoption.
- [ ] Lock/race is distinguished from corruption before destructive repair; retry budget is exhausted before wipe/rebuild.
- [ ] Last-known-good generation survives failed builds; durable operation state is recoverable on startup.
- [ ] Derived indexes have versioned identities plus explicit rebuild/repair commands, can be rebuilt independently and never become truth stores.
- [ ] RecallCircuit rollback to `scripts/cortex-candidates.mjs` is proven before consumer migration; later schema migrations carry separate rollback proof.

### 12.10 Security, generated ownership and release

- [ ] Default trusted path passes a zero-egress network tripwire.
- [ ] Repository strings are treated as untrusted data; path confinement uses canonical/symlink-resolved paths.
- [ ] Binary/NUL/oversize inputs are classified before parser invocation; subprocess/external-tool arguments are bounded and timeouts are typed.
- [ ] Config/env values that affect truth participate in generation/cache identity without exposing secret values.
- [ ] Stable repo-relative identities are used instead of leaking machine checkout roots where public contracts permit.
- [ ] Export/shareability classification is explicit; “redacted” is not assumed publishable.
- [ ] Remote providers are excluded from the default trusted path unless explicitly enabled.
- [ ] Generated artifacts are hash-owned: second generation run writes zero bytes, hand edits fail typed, stale outputs reconcile atomically.
- [ ] Public contracts/schemas are versioned; generated/provider tables cannot silently drift from code.
- [ ] Clean-host Mac and Windows proof exists; published artifacts tie back to source/release provenance.
- [ ] Mutable graph stores are not forced onto unsafe cloud-synced/shared paths.

### 12.11 Evaluation and product proof

- [ ] Frozen holdout has checksums and pinned upstream SHAs; mutation raises a hard qualification failure.
- [ ] Whole-graph goldens and per-language snapshots catch extraction regressions.
- [ ] Resolution P/R, ambiguity honesty, unresolved honesty, retrieval nDCG/MRR/recall and span/node P/R are measured after post-processing.
- [ ] Critical `EXPLAIN QUERY PLAN` assertions prove indexed seed/traversal behavior; realistic graph sizes track SQL work, not only returned rows.
- [ ] Resource envelope tracks peak RSS, p50/p95, cold/warm build, DB/index size, CPU, subprocess/handle counts, queue time and work avoided.
- [ ] Agent A/B tracks task accuracy, tokens, Cortex calls, total tool calls and wall-clock at equal accuracy.
- [ ] Freshness benchmark tracks edit→generation-barrier latency, dirty-overlay freshness, no-op rebuild cost, 1/10/100-file scaling and old-generation residue.
- [ ] AX benchmark additionally tracks correct Cortex operation/tool choice, no-tool accuracy where appropriate, citation fidelity, stale/ambiguous recovery, overclaim rate, behavior under partial provider failure and unsupported/decoy evidence rate.
- [ ] Protected exact/evidence cases cannot regress merely because an aggregate metric improved.
- [ ] PageRank expansion, vectors, learned rankers, deep analysis and other sophistication ship only after beating the simpler deterministic baseline at comparable correctness and acceptable latency/RSS.

### 12.12 Completion gate

- [ ] Release trains **A–H** are green; every mandatory **RES-1–RES-8** and **TH-1–TH-4** package is either implemented and qualified or explicitly removed by a new canonical decision that also updates this document.
- [ ] No active implementation requirement exists only in one of the superseded source documents.
- [ ] No active Cortex requirement depends on a repository path/file that does not exist unless this document explicitly marks it external and non-blocking.
- [ ] Agents receive evidence, not guesses.

---

## 13. Absorption-completeness ledger

This ledger is the guard against future semantic loss. It records where each source's unique contribution lives in the canonical authority.

| Source | Unique/high-detail mechanisms | Canonical location |
|---|---|---|
| `CORTEX_IMPLEMENTATION_GUIDE.md` | deterministic RecallCircuit file set, traversal-policy shape, indexed seed path, lean JSON entrypoint, explicit non-changes, staged rollout/rollback, correctness/adversarial/performance tests | §7, §5.12, Train C, §12.3/12.9/12.11 |
| Unified Absorption Improvement List | repo reality corrections; items 1–53; honesty, measurement, SCIP, change intelligence, packing, identity, store/truth/ops; determinism/ranking/budget policies; rejected/parked decisions | §2–§6, §8–§10, §12 |
| Master Competitor Absorption Guide — evidence/contracts | `SourceAddressV1`, `SourceSpanV1`, `EvidenceRefV2`, set provenance, entities≠occurrences, `CortexResultV2`, error/partial-failure contract | §5.1, §5.9–§5.10, §11, §12.1–12.2 |
| Master Guide — provider/resolution | provider capability matrix/fallbacks, exact-first R0–R8, import maps, SCC/re-export closure, module adapters, MRO hooks, SCIP precision | §5.8, RES-1–RES-8, §11, §12.4 |
| Master Guide — source precision | interval algebra, span hashes, drift detection, conservative re-anchor | §5.2, §5.9, R7, §12.4 |
| Master Guide — analysis/change | overlay registry, change seeds, provenance-aware impact, tests, liveness, co-change, explainable risk | items 18, 26–29, §9 experimental boundary, Train E, §12.6 |
| Master Guide — retrieval | query router/sanitization, minimal structural matcher, FTS/RRF, query shape, interpretable ranking, bounded renderer | items 30–36, §5.4–5.5, Train F, §12.7 |
| Master Guide — truth/history | typed invalidation, explicit fact↔claim binding, grounding outcomes, citation strength, declared-vs-done drift, findings lifecycle, selective bitemporality, named treeish diff/history evidence | items 47–50, TH-1–TH-4, §5.11, §12.8 |
| Master Guide — runtime/security/generated | cache identity, singleflight/workers, cancellation, crash isolation, quarantine/recovery, generated registry, zero-egress, shareability | items 42, 44–53, Wave 6 extras, §5.10/5.12, §12.9–12.10 |
| `cortex_master_improvement_list.md` | ten-priority product skeleton: RecallCircuit; source/provenance; identity; resolution; retrieval; truth; change intelligence; diagnostics; runtime; advanced optional capabilities | Canonical architecture §5; program §6; R/T packages; experiments §9; DoD §12 |

**Absorption rule:** donor/project names are provenance, not architecture. The canonical system contains one Cortex-native capability per semantic need, not one subsystem per competitor. The source documents may remain archived for provenance, but no implementation agent should need them to discover a requirement.

---

## 14. Final decision

```
Cortex observes deterministically.
Cortex preserves identity conservatively.
Cortex resolves exact-first through one shared provider/resolution pipeline and fails closed on ambiguity.
Cortex keeps source identity, span drift, citation strength, and historical evidence explicit rather than fuzzying them into confidence.
Cortex traverses its graph itself rather than asking a model to infer hops.
Cortex keeps every result generation-bound and evidence-backed.
Cortex treats omissions, blind spots, partial failure, and uncertainty as data.
Cortex ranks and bounds its own evidence result without owning final prompt policy.
Cortex adds deeper analysis as versioned providers without rewriting its core.
Cortex treats history as evidence, not current truth, and declared-vs-done drift as a typed finding with receipts.
Cortex treats semantic/learned methods as optional ranking assistance, never truth authority.
Cortex measures every sophisticated absorption against a simpler deterministic baseline.
```
