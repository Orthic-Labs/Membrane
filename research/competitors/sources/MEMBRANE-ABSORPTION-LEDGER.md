# Membrane Absorption Ledger

**Status:** reconciled absorption register — one list, dependency-ordered
**Date:** 2026-08-17 · **Applies to:** `Orthic-Labs/Membrane`
**Inputs:** 60 competitor repositories (`competitor.md`); four independent registers (`qwen.md`, `m3.md`, `dsv4pro.md`, `dsv4flash.md`); four consolidations of those registers; ~600 raw findings
**Subordinate to:** `deepseek.md` — §2 constraints are the filter, §10 is the schedule, §11 is the definition of done. Where this document and `deepseek.md` disagree on *what Membrane is*, `deepseek.md` wins.

---

## 0. What this document is

The eight source documents overlap heavily and contradict each other in places. This is the single reconciled list: every item deduplicated across all eight, ranked by how many independent passes found it, ordered by dependency rather than by any one pass's enthusiasm, and checked against the actual tree.

### 0.1 What all eight agree on

Stripped of vocabulary differences, all four registers and all four consolidations reach the same executive decision in near-identical words: **Membrane does not need another architecture.** The five typed shapes, the one context economy, the four delivery lanes, authority-before-freshness-before-similarity, and receipts for absence are the right substrate. The corpus asks Membrane to *complete* that substrate, not replace it.

The second point of agreement is the one worth defending. Thirty of thirty examined competitors record what *changed*; several record what was *accessed*; several record derivation lineage. **None records what was dropped from a context packet and why.** No `DroppedSpanV1` equivalent, no compression receipt binding a transform to the spans it removed, no freshness-ranked admission paired with a typed grant protocol. That is measured across the corpus, not asserted.

Its planning consequence is the most important line in the exercise:

> **The return on making the existing evidence layer reachable, exercised, and measured exceeds the return on closing any individual gap below.**

Tier 0 is not throat-clearing before the real work. It *is* the highest-leverage work, and it is what every item after it is scored against.

### 0.2 How to read the consensus marks

The standing rule comes from the passes themselves (`m3.md` §7):

| Mark | Meaning |
|---|---|
| **Floor** | Reached independently by three or four passes, from different code in different repositories. Non-negotiable; do not relitigate. |
| **Candidate** | Two passes. Strong, but verify the mechanism before committing schema. |
| **Bet** | One pass only. Often the most interesting items, and the most likely to be wrong. Ship behind a flag. |
| **Hold** | Deliberately deferred or rejected. Listed so the next agent does not re-import it under a new name. |

### 0.3 Three corrections to carry forward

- **M3's cross-cutting summaries are unusable.** They fabricate repositories that do not appear in `competitor.md` and misstate Membrane's tree. Its per-repo findings held up under spot-checking; nothing here derives from its summary index.
- **The `MemoryOS-bailab` dispute is resolved 3–1** as BAI-LAB/MemoryOS. Heat-promotion and decay findings are safe to use. The genuinely ambiguous entry is the *plain* `MemoryOS` line.
- **Some "current state" claims are already stale.** `membrane-core` is now in `engine/Cargo.toml` members (S-5 closed). `server.json` exists — the gap is publication, not authorship. Re-verify against the tree before writing a ticket.

---

## Tier 0 — Prerequisite: make what already exists true

Nothing below Tier 0 should ship before this tier is green. Four of the eight documents open with this instruction; the fifth spends its first section arguing for it.

### 1. Context-quality fixture corpus, frozen baseline, and a regression gate
**Floor (8/8)** · `tests/context-quality/` · `crypt-core/src/eval_gate.rs`

~20 cases covering exact symbol lookup, stale-memory conflict, anchor survival, dirty worktree overlay, provider timeout, duplicate-across-providers, oversized result needing compression, superseded fact, cross-scope isolation, and the no-relevant-context case. Each records required evidence, **forbidden** evidence, expected authority ordering, token ceiling, expected degradation, and expected receipt properties.

Block a change on scope regression, anchor loss, reconciliation imbalance, evidence-recall loss, or transform corruption. Performance trade-offs pass only through an explicitly checked-in threshold change.

### 2. Guarantee suite driven from the published artifact, zero mocks
**Candidate** · `benchmarks/` against the npm package and built engine binary

Import the *installed* artifact, never workspace source. Cover erasure completeness, receipt reconciliation atomicity, schema migration and downgrade, and multitenant isolation of `POST /federate`. Commit the result JSON.

This is the strongest answer in the corpus to `deepseek.md` §5.2: a suite that imports the shipped artifact *structurally cannot pass* while a surface is written-but-unwired. It turns the reachability sweep from a manual audit into a permanent gate.

### 3. Two-layer evaluation — free deterministic markers gate paid judging
**Candidate** · `mcp/calibration-harness.test.mjs`

Assert schema validity, lane budgets, token counts, byte sizes, and prefix stability *before* any model-graded check, with a free-tier flag so CI runs the deterministic layer only. Add a "faithful" tier: wherever Membrane claims passthrough or lossless normalization, assert byte-identical output against hostile-unicode fixtures.

### 4. Condition-isolated benchmarking with captured-output replay
**Candidate** · `evidence/runs/*.jsonl`

Every benchmark cell in a scratch directory with an explicit injected-context manifest and a scrubbed environment, so ambient store state cannot contaminate it. Persist full provider outputs as append-only typed JSONL so analysis re-runs from disk and judge prompts can be iterated at zero cost.

### 5. One implementation authority; supersede the rest
**Bet** · `docs/plans/2026-08-12-*.md`

Adopt one canonical implementation guide, stamp the August 12 plan *Superseded by*, and regenerate the current-state manifest from source rather than by hand. Two live implementation authorities is the condition that produced eight overlapping documents in the first place.

### 6. Typed request-context carrier in task-local storage
**Candidate** · closes **B-3**

One carrier holding request, trace, session, task, parent-task, repository, worktree/overlay, and provider-invocation ids — set once at entry, read downstream, with no id in scope without being declared. This is exactly the B-3 defect ("envelopes do not survive the live path"), and it independently pays for triageable federation failures.

### 7. Single canonical writer with lease, heartbeat, and operation-id-scoped recovery
**Floor (26/60)** · `crypt-store` · `mcp/host/delivery-ledger-store.cjs`

Membrane currently has two writers over shared state. Serialize through one connection-owning path with typed refusals (`OwnershipRequired`, `QueueOverloaded`, `WriteDeadlineExceeded`, `CommandConflict`), and clear a stale lock only when the caller supplies the exact operation id — not by PID. The stale-PID pattern is the corpus's most-repeated operational bug, and **B-5 is one instance of it**.

### 8. Two-class durability
**Bet** · `crypt-store`

Keep `synchronous = NORMAL` for reconstructable derived writes; raise to `FULL` inside a Drop-guarded transaction for authored, irreplaceable writes that return success to the caller. Rare in the corpus and exactly right — it strengthens delivery proof without paying fsync on every projection.

### 9. Cause-chain error classification into a small closed code set, shared Node↔Rust
**Floor** · `membrane-protocol` · `mcp/server.mjs`

Membrane collapses failures to `tool_execution_failed`. Replace with a cycle-guarded walk of the cause chain into stable codes (`provider_auth_failed`, `provider_rate_limited`, `provider_timeout`, `provider_unavailable`, `datastore_unavailable`), implemented once and used by both planes so the supervisors agree on what is recoverable. Classify typed instances first, so nothing in a payload can out-vote the transport layer.

Carry the asymmetry rule with it: **when a classification triggers a destructive action, false positives cost more than misses — require conjunctive evidence and default to unknown on conflict.**

### 10. Caps as named constants, env-overridable, each asserted by a test
**Floor (30/59)**

A ceiling that is not asserted in a test is a suggestion. Pair with grep-checkable sensitivity annotations on schema fields (`[PII]` / `[INTERNAL]` / `[PUBLIC]` in doc comments), which gives §2.7 content-free telemetry a mechanical enforcement point instead of a review convention.

---

## Tier 1 — Retrieval: candidate generation and ranking

All four consolidations independently name the lexical arm as Membrane's clearest retrieval weakness. **Verified:** the production lexical path is keyword-exact ×2 plus substring occurrences plus a stored score. The only FTS5 in the tree is in `engine/crates/crypt/tests/doc_recall_index_contract.rs`.

### 11. A real sparse lexical index — FTS5/BM25 with identifier-aware tokenization
**Floor (4/4 consolidations)** · `crypt-store/src/lexical.rs` (new)

BM25 scoring, field weighting, phrase and exact bonuses, document-frequency statistics, scope and authority filters, incremental update. Tokenize `snake_case`, `camelCase`, `PascalCase`, `kebab-case`, paths, and `module::names`, so `ContextCandidateSet` yields *context / candidate / set / ContextCandidateSet*.

Keep the current scorer as a deterministic fallback when FTS is unavailable or corrupt. Retrieval must still work with zero embedding models — one competitor ships a serious memory product on lexical search alone, so FTS5-as-mandatory-baseline is not a compromise position.

Deciding criteria between SQLite FTS5 and an in-process Rust index are quality, resident memory, incremental update cost, warm latency, binary complexity, and failure surface — not familiarity. Do not introduce Tantivy, Elasticsearch, or Qdrant for this.

### 12. Retrieval channel registry, not an ever-growing ranking function
**Floor** · `crypt-core/src/retrieval/`

One trait, one file per channel: `exact`, `lexical`, `semantic`, `temporal`, `relation`, `working`. Not all channels run for every query. Turning any optional channel off must not break retrieval, and each must be independently ablatable in evaluation — otherwise Membrane accumulates ranking mechanisms nobody dares remove.

### 13. Deterministic query-intent classification — no model in the hot path
**Candidate** · `crypt-core/src/query_intent.rs` (new)

Signals: paths, identifiers, stack traces, quoted strings, error codes, git hashes, dates, and the words *why / when / previous / decision / changed*. Emit weights, not a brittle single label. Intent decides which channels get budget and how much — **never source authority**. Record it in the receipt.

### 14. Two-phase budget fill: breadth-first floor placement, then depth upgrades
**Floor (4/4 passes)** · `mcp/context-renderer-lib.cjs`

First-fit lets one high-scoring hit eat the budget. Place every candidate at its category's minimum detail tier under a per-entry cap of ~2× the average share, then run a separate upgrade pass spending leftover budget in score order. Emit `floor_tokens`, `upgrades_by_lane`, and `spare_upgrades` into the context receipt. Add a golden fixture asserting starvation-free allocation when one provider returns oversized items.

> Ranked #1 in one register, reinforced as "the most production-considered design of any repo in this list" in a second, independently surfaced in a third, contradicted by none. Membrane's four lanes are exactly the structure the algorithm assumes. **This is the single highest-confidence item in the entire exercise.**

### 15. Weighted RRF with per-channel provenance and deterministic tie-breaks
**Floor (18/60)** · `crypt-core/src/ranking.rs`

Keep RRF — heterogeneous score scales make raw-score fusion dangerous. Add three details other teams paid for in bugs:

- an explicit `max_rank_penalty` for absent documents rather than omission;
- a deterministic tie-break on item id, because set-iteration/hash-seed nondeterminism made rankings unreproducible elsewhere;
- **fuse once** — triple re-fusion destroyed rankings in at least one recorded case.

Retain per-lane rank and score as first-class receipt metadata instead of discarding them into a diagnostics blob. Carry a versioned ranking-config constant on every result, so a benchmark regression is attributable to a revision.

### 16. Separate relevance from policy — never one global weighted score
**Floor (4/4 consolidations)**

Keep relevance, freshness, authority, veracity, effectiveness, exactness, scope distance, redundancy, and estimated tokens as **distinct dimensions**, and distinguish:

- **generation score** — could this be relevant?
- **policy score** — should this be admitted?
- **utility** — is it worth its token cost?

All four consolidations flag collapsing these into one number as the change that would destroy Membrane's strongest architectural property. A cosine from Crypt, a graph score from Cortex, a rule priority, and a Git freshness signal are not on one calibrated scale.

### 17. Exact-evidence pinning — ordering-only, still vetoable
**Candidate**

A verified exact match on a path, symbol, error string, commit SHA, or rule identifier is pinned to the top of its lane, after every adaptive layer, **reordering only** so budget math stays valid. Scope, authorization, freshness, invalidity, and supersession can still veto it: exactness is strong evidence, not permission to bypass policy.

This is §2.4 given a mechanism — authority gates already run before ranking; this is the symmetric floor *after* ranking.

### 18. Capability degradation with score-neutral optional stages
**Floor (29/60)** · `mcp/retrieval-contracts.mjs` + the Rust store trait

Optional capability flags with no-op defaults, so lane policy degrades per backend instead of branching on concrete types. The critical refinement: an absent optional stage must be **score-neutral** — ordering preserved, no synthetic scores — otherwise receipts become incomparable across capability sets.

### 19. Full ranking trace into the ContextReceipt
**Floor (31/60)** · `schemas/context-receipt.v1.schema.json`

For every candidate: which channels found it, each channel rank, fused score, freshness, authority, runtime modifiers, policy disposition. For every rejection, a typed reason — `redundant_with`, `stale`, `lower_authority`, `scope_mismatch`, `budget`, `timeout`, `superseded`, `unsafe`, `low_utility`. Add per-lane latency fields.

Do not create a second debug protocol; enrich the receipt that exists.

### 20. Diversity suppression after fusion, never before eligibility
**Candidate · benchmark-gated**

Ten near-identical memories are worse than five complementary pieces of evidence. Penalize same source, same fact lineage, same content hash, same artifact family, near-identical embedding, same graph neighbourhood — *after* the first winner, and never aggressively against exact evidence or unique authority evidence. Do not use a model for ordinary dedupe.

---

## Tier 2 — Persist: memory that is lean by construction

The largest cluster of genuine gaps. Membrane persists emissions and the pipeline stops there: **nothing in the pipeline is permitted to decide "this doesn't deserve to be remembered" and say so.** The best cleanup strategy is refusing bad memories before they enter durable storage.

### 21. An explicit admission pipeline before persistence
**Floor (4/4 consolidations)** · `crypt-core/src/admission.rs` (new)

Schema validity → scope validity → secret/PII policy → epistemic classification → novelty → near-duplicate → contradiction → durability/utility, yielding `ADMIT | MERGE | SUPERSEDE | QUARANTINE | REJECT` plus a receipt. Rules decide the obvious cases; a model touches only the ambiguous ones, off the latency-critical path.

Reasons worth typing: `ephemeral`, `conversational_filler`, `unsupported_claim`, `duplicate_exact`, `duplicate_near`, `superseded`, `contradicted`, `insufficient_evidence`, `scope_invalid`, `secret_detected`, `low_information`, `low_expected_utility`.

Reference, don't duplicate: the generic memory lifecycle should defer to the existing temporal supersession model rather than building a second one beside it.

### 22. "No-op is valid" as a first-class success
**Bet · cheap**

State it outright: no durable change is the correct outcome when the available material does not justify one, and the store is explicitly **not** a scratchpad for unresolved intake, temporary question lists, raw inventories, or routine activity logs.

A one-line contract change with outsized effect on signal-to-noise. It composes exactly with #24 — the guard decides *admissible*, this decides *worth admitting*.

### 23. A validation gate the write path must pass
**Bet (28/60)**

Twenty-eight of sixty competitors make the writing agent validate its own output before finishing. Membrane accepts emissions without a conformance check. A write path with no self-validation cannot supply the evidence-backed claims §11 requires.

### 24. Deterministic guards on every model-authored mutation; ties go to quarantine
**Floor** · `crypt-core/src/dream.rs`

Dream consolidation currently trusts its model pass outright. Add pure decidable checks before any UPSERT — non-empty, size ceiling, identifier-tolerant sentence counting so `files.generation` and `3.5x` do not split, reference resolution by set membership. Retry once, then write a typed failure stamp **inside the same transaction** as the summary write.

Add an evidence-coverage gate: ≥N distinct sessions and ≥M distinct time strata before auto-commit, with a hard-coded prohibited-category set forcing human review.

Three independent codebases refuse silent promotion on ambiguity. **Adopt the ties-to-quarantine rule verbatim.** This is §2.6 given teeth.

### 25. Canonical content held apart from mutable ranking signals
**Candidate** · signal sidecar keyed by logical memory id

Do not rewrite canonical memory content because its retrieval value changed. Put retrieval count, successful-use count, ignored count, contradicted count, last-retrieved/used/contradicted timestamps, base importance, current hotness, maturity state, and score epoch in a sidecar.

Canonical content stays auditable and diffable; the sidecar is rebuildable and recalibratable without corrupting truth.

### 26. Decay with retrieval reinforcement, archive never delete
**Floor (4/4 passes · 14/60)** · `crypt-core/src/lifecycle.rs` (new)

Two design rules recur across fourteen implementations:

- decay **multiplies into the retrieval score** rather than running as a separate sweep, so cold memories lose contention naturally;
- crossing the threshold **archives**, never deletes. Pinned items exempt, superseded items penalized.

Use per-family curves, not one universal half-life: stable preferences decay very slowly, architectural decisions slowly until superseded, repository observations moderately, transient debugging state fast, and temporal facts are governed by their validity interval instead of generic decay. Start with simple monotonic curves; a Weibull/stretched-exponential form is supportable if evaluation demonstrates value but is not intrinsically superior.

Encode retention as a pure function in `membrane-core` and assert monotonicity in elapsed time under fixed access history via `cargo test`. **A decay curve nobody tests is a decay curve nobody can defend.**

> `research/synthesis/INDEX.md` names the Ebbinghaus forgetting curve as "the one policy Membrane lacks today." Quoted verbatim and verified. This is the fourth pass to raise it; it should not survive another planning cycle unresolved.

### 27. Bounded asymptotic reinforcement, and recall separated from usefulness
**Candidate** · `crypt-core/src/effectiveness.rs`

Use `c ← c + α(1 − c)` with α ≈ 0.1. It is monotone, capped at 1.0, cannot overshoot, and its behavior at any hit count is legible without simulation.

Then keep the stages distinct: *generated → admitted → delivered → used → helped*. Only the later stages justify reinforcement. **A memory must not become immortal because a retriever keeps selecting it.**

Membrane already records `used / ignored / contradicted / verified` feedback — exploit it, as a slow-moving prior rather than immediate reinforcement, and never learn directly from unverified agent opinion. Keep `ignored` advisory: it is not a ranking punishment. Only `contradicted` is a verified veto.

### 28. Expiry checked before scoring, with declared expiry behavior
**Bet · cheap**

Stamp an expiration at add time and run the check **before** ranking, so expiry is an admission gate and not a ranking penalty — which keeps it consistent with §2.4. Sharper still: a policy should declare what happens when it *expires* — fail-closed, fail-open, or grace period — not only what happens when it is violated.

### 29. Three independent axes: family, tier, lifecycle state
**Floor (14/60)** · extends the existing `Working→Episodic→Semantic` tier

Competitors conflate these constantly.

| Axis | Values |
|---|---|
| **Family** — what it means | observation/evidence, episode/session, semantic fact, procedural lesson, preference/profile, entity summary, evolving belief, artifact reference |
| **Tier** — how aggressively recalled | working, episodic, semantic |
| **State** — whether active | active, warm, cold, archived, superseded, expired, quarantined, tombstoned |

A procedural lesson can be semantic-tier and archived; an episode can be working-tier while a task is live and episodic afterwards. Classification costs no model call — one competitor ships thirteen typed classes at zero LLM cost.

**States without distinct behavior are taxonomy, not architecture.** Do not copy tier counts from anyone.

### 30. Negative knowledge as a first-class class
**Candidate**

Failure, guardrail, invalidated assumption, known-bad approach. The retrieval rule is the whole point: negative memory surfaces when the **planned action matches the guardrail**, not on every generic retrieval.

Pair with failure-driven evolution: on a failure episode, force the extraction to name the `violated_assumption` — *the step that failed is rarely the root cause* — and emit a `precondition_check` appended to a bounded precondition list. Render preconditions inline when a previously-failed lane is re-served, so failures become checkable conditions rather than identical re-sends. Feeds C-7.

### 31. Three-layer conflict records, written to a sidecar rather than onto the row
**Bet (11/59)** · closes **C-4**

A conflict carries a semantic **relationship** (exact-value, negation, entailed, constraint, probabilistic, scope-apparent, refinement), a diagnosed **cause** (correction, temporal change, scope difference, entity mismatch, write error, unresolved), an **evidence strength**, a chosen **action** from a closed set, and a human-readable audit reason.

Keeping diagnosis separate from action separate from the row's resulting status makes verdicts revocable and recomputable without mutating authored knowledge. Store vocabularies as text plus a check constraint rather than native enums, so the vocabulary grows by a one-line change.

Composes with #36: supersession is the *action*; this is the *record of why*.

### 32. Declare single-value vs multi-value predicates
**Bet · nearly free**

The single most important modeling distinction for contradiction handling, and it costs almost nothing. Predicates holding one value (a file's owner, a config's current setting) conflict on change; predicates that accumulate (which modules touch a symbol, which tests cover it) are additive and skip the conflict path entirely.

Declaring which is which makes the common case deterministic and reserves model spend for the rest. The refinement worth taking alongside: the declared merge operation (patch / replace / sum / immutable) should also constrain what the model is allowed to emit.

### 33. The non-conflict taxonomy, with gates that override the model
**Bet**

Seven ways two statements look contradictory while both hold: temporal supersession, list-valued predicate, refinement, scope mismatch, same-name-distinct-subject, conditional-unrealized, event restatement. This is the false-positive class that makes naive contradiction detection unusable.

The transferable pattern is the gate: force the classifier to commit to `same_subject` **before** `contradicts`, then reject any contradiction claim carrying `same_subject: false`. **A structured-output classifier must be checkable against itself, and the check must win.**

One implementation detail worth carrying verbatim: compare by identity against `True`, because `bool("false")` is truthy and a model returning the *string* `"false"` would otherwise bypass the gate. That is the shape of bug that silently corrupts a truth store.

Wrap the boolean verdict in a confidence score reflecting how coherently the model answered — high for clean agreement, lower when a gate fired, lowest for malformed output. A model that asserts "different subjects **and** contradicts" is reporting its own confusion.

### 34. Symmetric conflict detection with late attribution
**Bet**

Check both orderings; decide direction **only after** a conflict is confirmed; attribute by timestamp with a stable-id tiebreak. Any system that decides who wins by write order produces order-dependent state — the same determinism requirement rank fusion already imposes, applied to truth.

### 35. Inference lineage — derived beliefs remember their premises and stay retractable
**Bet (6/59 — rarest structural pattern in the corpus)** · closes part of **C-4**

An explicit/derived flag plus derivation edges recording every upstream source, for the express purpose of revalidation: when an upstream source is corrected or deleted, the inferred memory and any conflict it drove get re-checked or retracted.

Two invariants: **an inferred fact must never silently override an explicit one**, and it must be retractable when its premises change.

> This is the mechanism behind "fresh code evidence outranks stale documents and memory" for the case where the memory was *inferred*. Today there is no way to tell.

### 36. Append-only bitemporal supersession with `as_of` queries
**Floor (4/4 passes · 20/60)** · `crypt-store/src/temporal.rs`

Membrane's temporal fact model is already one of the strongest parts of the implementation — **extend it, do not build a second one beside it.**

Add `recorded_at` separately from `observed_at` / `valid_from` / `valid_until`, which separates "what did Membrane believe on August 2?" from "what was actually valid on August 2 given what we know now?"

Normalize `supersedes` out of an optional comma-separated string into a transition table (`from_fact_id`, `to_fact_id`, `effective_at`, `reason`, `transition_sha256`). That buys graph traversal, provenance, multiple predecessors, cleaner indexing, and no string parsing. Maintain backwards-compatible migration semantics.

### 37. Session-close episodic packet, pinned and decay-exempt
**Floor** · closes **C-5**

Goal and task identity, repo/branch/worktree/revision, decisions made, open work, failed approaches and dead ends, verification results, exact identifiers and artifact refs, unresolved contradictions, evidence refs. It is **episodic**, not automatically semantic truth.

Fill the tier that already exists; do not invent a new memory family. Key the bookkeeping on message ids — that is what makes re-running compression a no-op rather than a re-summarization.

**Anti-pattern to refuse:** one reference implementation's retrieval-feedback write-behind fires under `except Exception: pass`. Membrane's write-behind must route through a bounded, logged queue, and a failure increments a typed degradation counter. Silent swallow violates §2.5.

### 38. A curation pass with undo: hygiene → cluster → distill → archive
**Floor (11/59)** · closes part of **C-8**

Isolated hygiene checks (orphans, near-duplicates via approximate search, missing embeddings, expired-still-active, stale-never-recalled, too-short content, broken links), then deterministic union-find clustering, then per-cluster distillation with a model on **pre-clustered** input, then batch archival of sources with crystallized-from provenance and skip reporting. Each check isolated so one failure does not kill the sweep.

Two details the other passes missed: record the **motive** — why the loop fired now — which makes the loop auditable as §2.6 requires; and ship **undo**. Undo is the detail that makes an automated curation pass safe to enable at all.

Worth noting as a competitive opening: one competitor verifies existing memories well but consolidates poorly; another consolidates well but verifies reactively. **Freshness and distillation are two halves no single competitor combines.**

### 39. A job and run lifecycle that survives the process
**Bet (14/60)** · the missing substrate under **C-8**

Queue → local worker → durable job record, with list, show, logs, live-attach, cancel, and JSON output — and status, summaries, changes, and event logs that survive the terminal that started the job closing.

Membrane has no job abstraction at all. A gardener with no job record is a gardener nobody can audit or stop, so this is the substrate under #38, #26, and #40.

> **Declared dependency:** `docs/plans/orthic/SEAM-CONTRACT.md` (watcher lifecycle, scheduling). Not present in this repository.

### 40. Retroactive session mining — proposal-only, mined off disk
**Bet (11/60)**

Persist fires only when a provider emits in the moment; nothing ever goes back over what the host agent already did. Close it narrowly: discover local conversation stores, queue past sessions as ingest jobs with per-app discovery adapters, and treat everything mined as a **proposal, never an auto-write** (§2.6).

*Mine off disk, never intercept* — that keeps the hot path untouched and the ingest auditable.

---

## Tier 3 — Evidence and drift: making freshness mechanically provable

§2's "fresh code evidence outranks stale documents and memory" is currently a policy with **no detector**. This tier is the detector — and it contains Membrane's most defensible claim.

### 41. Citation by construction, plus a citation-fabrication guard
**Candidate (4/59 — rarest and highest value in the corpus)**

Two complementary designs, and Membrane should ship **both**.

**Construction:** resolver-backed blocks carry `(source_id, byte_span)` and the renderer materializes quoted text by slicing — the materialized quote for a unit is exactly `&text[start..end]`. The model cites by unit id and never retypes the quote, so drift is structurally impossible.

**Verification:** every evidence line a model cites must appear verbatim, whitespace-normalized, in a **content line** of the rendered pack — not a header, not boilerplate — above a minimum length, and never a bare `path:line` locator naming no source text. A fabricated citation rejects the completion and retries once; a second discards the verdict.

The two details that make it work are the bare-locator rejection and content-line-only matching. The source docstring is blunt about why: *small models measurably fabricate evidence, so this guard is load-bearing.*

Related rule from the same source: **presence cannot contradict.** A stored claim cannot be shown wrong by citing code that merely exists — only by citing something *missing* that the claim itself names. Preserve operator shape in the span match, so a model cannot pass the check by dropping a `!`.

> Citation-by-construction plus verification-at-reconciliation is a claim almost nobody in this field can make.

### 42. Code-anchor fingerprinting with five-way drift classification
**Candidate** · `crypt-store` anchor plane + an anchor-audit tool

Store a comment-free AST signature hashed over the anchored span, so reformatting does not drift but a literal `3 → 8` does. Audit classifies each anchor `missing_file | missing_symbol | ambiguous_symbol | unsupported_language | drifted`, and reports drift **only when a baseline exists**, never assumed. Wire results into receipts so the renderer can demote or tag drifted entries during reconciliation.

Keep two hashes for two questions: an **exact source hash** for "are these bytes identical?" and a **structural fingerprint** for "is this still the same code after comments, formatting, or movement?"

### 43. Five-way *resolution*: text-present is not absent
**Bet**

A different axis from #42, and both are needed. Each extracted identifier resolves to exactly one of:

- an indexed **symbol**;
- an indexed **file**;
- **text-present** — appears verbatim in source but is not a defined symbol (a table name, a local);
- **absent** — the actual divergence signal;
- **unresolvable** — a non-code span whose non-match is an artifact of span shape, never evidence of divergence.

> Collapsing text-present and absent into a blanket `NOT_FOUND` is named in the source as the root of the divergence false-positive class.

### 44. Authoritative absence requires proof of coverage
**Bet**

Emit a not-found verdict **only** when the memory's own binding proves the searched domain is live and indexed; if the binding points outside indexed coverage, downgrade the absence to indeterminate.

**You cannot claim a fact is stale unless you can prove you indexed the domain it lives in.** This is the soundness rule that keeps a partial index from fabricating divergences, and the honest form of §2's abstention requirement. Pair with surfacing coverage blind spots and liveness as first-class receipt fields, so a degraded packet advertises its degradation rather than requiring the caller to infer it.

### 45. Embedding provenance — one column, and a question stops being unanswerable
**Bet · cheapest high-value item in the corpus** · `crypt-store`

Record the hash of the text the embedding was **actually computed from**, alongside the row's current content hash. The problem statement from the source is the clearest in the corpus: *a vector left over from earlier content is byte-identical to a correct one, so a mis-embedded row is undetectable; `embedding IS NOT NULL` says only that something was embedded, never what.*

Staleness becomes a comparison of two hashes. Take the null semantics with it: **NULL means provenance unknown, not stale**, so pre-migration rows are not misreported as damaged.

Combined with keying embeddings by provider, model, and dimensions, a row can then state which model produced it, from what text, and whether that text has since changed.

### 46. Content-addressed identity — key derived data by its invalidation key
**Floor (27/60)**

Derive an `identity_hash` over a canonicalized projection excluding timestamps and the id itself, enforced in `crypt-store` with reject-or-match semantics. Re-mining the same corpus then reproduces the same ids, caching keys perfectly, and dedup is free.

Key every model-derived artifact (summaries, projections, embeddings) by `(source_id, content_hash, prompt_version)`. **The primary key *is* the reason to recompute**, which makes every derivation pass churn-skippable, self-invalidating, and free on cold start.

Add the version gate teams forget: a cache keyed on content but not on the *code version* that produced it will serve stale derivations across an upgrade. And use a three-hash churn skip — a content hash covering exactly what the prompts render (so a tag edit does not churn), an inputs hash fingerprinting the whole evidence preimage as a path-plus-sha multiset (so a same-content rebind still churns), and a prompt version that re-queues everything when bumped.

Take the dual-identity scheme for code anchors wholesale: one id that changes with content, one that survives it.

### 47. Deterministic substrate → bounded model verdict → deterministic gate
**Bet**

Pass 0 builds a churn-skipped queue and a citation-checkable evidence pack with **no model**. Pass 1 renders that pack into one prompt and asks only for `current | diverged` — and `unverifiable` is *deliberately absent from the model's vocabulary*, because pass 0 owns that decision deterministically, so a stray model `unverifiable` is discarded.

**Removing an option from the model's vocabulary because a deterministic pass owns it** is the transferable idea. It closes the entire "the model said it couldn't tell" ambiguity class.

Carry two parsing defenses with it:

- a model that emits `VERDICT: current | diverged` has **selected nothing** — taking the first word silently stores `current` and churn-skips a real divergence;
- the section parser must **reset accumulated fields on each new verdict line**, so a scratchpad or self-correction yields only the last answer.

Treating both as "no valid answer" rather than parsing the first token is the difference between failing safely and quietly corrupting state.

### 48. Derive findings from stored state, never from the current run
**Bet**

A subtle correctness rule with a nasty failure mode: under budget capping, a memory skipped for cost silently resolves its own finding. Derive findings every run from **persisted** verdicts still matching current content, inputs, and prompt version — a finding resolves only when a re-check flips the verdict.

Pair with a ranked, budget-capped verification queue: broken anchors first, never-checked second, content churn third, inputs churn fourth, **prompt churn last** — because a stale-prompt row's verdict is at least self-consistent. Verification is always budget-capped, so the priority order *is* the product.

---

## Tier 4 — Push: reversible compression as the default path

`deepseek.md` C-1, named as the #1 gap: the compression engines exist but are advisory, with measured adoption of one use in seven opportunities. Nothing compresses what the host actually accumulates.

### 49. One artifact primitive: compress → cache → retrieve, reversible by construction
**Floor (4/4 consolidations)** · closes **C-1**

Compressed content is replaced by a hash-addressed marker, the original is stored under its hash, and a retrieval tool expands on demand. **Reversibility becomes a property of construction rather than of policy.**

`ArtifactRefV1` should carry: an `art:<sha256>` identity, MIME/content class, byte length, source/origin hash, scope and current policy resolver, sensitivity/influence class, created and observed timestamps, extractor/transformation version, parent and derived refs, and integrity/availability state.

Use it for large tool output, logs, documents, OCR text, transcripts, generated reports, and any transformation where reversibility matters. Add a `membrane_retrieve` tool expanding `hash=` markers, with a golden compressed→expanded fixture pair. This extends the existing `compress`/`runc` spill store — first-party, deterministic, no new dependency.

Pair with a capped rotating raw-recovery store keyed by receipt id, specifically so compression can never destroy the evidence of a failure.

### 50. Never-worse-than-raw, typed as a persisted balance rather than a runtime guard
**Floor**

Two passes found the same invariant from opposite directions. One describes it as a twelve-line guard: if `estimate_tokens(filtered) > estimate_tokens(raw)`, emit raw. The other **types** it as `TokenBalanceV1 { original, materialized, delivered, provider_billed }` with a `validate()` enforcing `materialized ≤ original` and `delivered ≤ materialized`.

**The typed form is strictly better:** it makes "did this behave as documented?" a single query against a stored row rather than a property you hope held at the time.

Persist it beside every packet receipt. It is the cheapest possible foundation for §2.8 measurement-class discipline — "saved" becomes arithmetic on a stored row. Every passthrough carries a typed `skip_reason`, so the fail-open decision is telemetry rather than silence. The three inequalities deserve a machine-checked argument (Kani or property tests), not a reviewer's confidence.

### 51. A query-critical verifier that restores exact spans
**Candidate**

A compact representation must not merely fit a token budget; it must preserve what the current task needs. Before delivery, verify that these survive: exact identifiers, error messages and status codes, failing test names, cited source ranges, task/query entities, authority-bearing instructions and rules, and explicitly requested details.

If verification fails, restore exact spans through the artifact resolver. **Compression is not allowed to silently erase the only evidence needed to answer the task.**

### 52. Signal preservation as a mandatory paired metric
**Candidate**

Every efficiency claim ships paired with a semantic retention assertion — required entity and file references still present — and token reduction is **never reported alone**, tested and surfaced as a receipt field.

This is the guardrail that keeps the PUSH work honest: without it, "39.9–59.7% reduction" is a number with no denominator.

### 53. Referential-integrity closures, a protected tail, and an explicit unachievable-budget state
**Floor**

Atomic groups that are evicted together or not at all: tool call + tool result, decision + rationale, error + stack frame context, diff header + changed hunk, citation + source locator. Protect the live tail and pin the last real user turn.

When the budget is unreachable under those protections, return a typed failure — `budget_unachievable_with_protections`, receipt `outcome: incomplete` (§7.1) — rather than emitting a broken transcript.

Add an `actions[]` audit array to the receipt (`kind` / `path` / `reason` / `originalSize` / `finalSize`), and distinguish "source-capped at ingress" from "dropped to fit lane budget" in `schemas/host-delivery-receipt.v1.schema.json`. Truncation goes through a single byte- and surrogate-safe primitive so no boundary can produce invalid UTF-8, with notices at the **edges** because a middle-cut backstop eats anything placed centrally.

The rationale worth keeping: every retained character re-enters every subsequent request, so cost compounds over a run.

### 54. A representation planner: pick the cheapest form that preserves the required evidence
**Floor (4/4 consolidations)** · the admission ↔ renderer boundary

Inclusion need not be binary. Per admitted source, choose *native / full / excerpt / skeleton / summary / resolver reference / metadata-only*, and degrade in that order — never straight from full source to an arbitrary model summary. Prefer downgrading the detail tier over truncating the text. The receipt reconciles selected representation against delivered.

Order the transforms: deterministic noise stripping → structured JSON/table/log reduction → code skeleton and signature extraction → Markdown structural thinning → bounded extractive compression → only then optional model summarization for compressible prose. **Never regex-parse source code when a structural parser is available.**

The planner asks one question: *what is the cheapest representation that preserves the evidence required for this task?* The renderer should not make hidden ranking decisions, and the ranking system should not make rendering decisions.

### 55. Determinism-first rendering, for prompt-cache validity
**Bet · large, cheap, measurable**

Rewriting a cached prefix trades a ~90% read discount for a ~25% write penalty. So the render must be **prefix-stable**: deterministic receipt ordering, stable separators, append-only sections, and volatile fields (timestamps, request ids) *relocated out of the prefix* rather than tolerated inside it.

Two invariants make cross-turn dedup cache-safe: a block matches only strictly *earlier* blocks, so earlier output stays byte-identical regardless of what comes later; and the original a pointer names is always physically present.

Add a regression test asserting byte-identical rendered prefixes across consecutive turns, and cache-miss attribution as a typed field on reconciliation receipts. Membrane's receipt discipline is unusually well-positioned to *prove* this one.

### 56. One deadline end-to-end, one concurrency primitive, dropped-work accounting
**Floor** · `mcp/deadline.mjs` · `membrane-runtime`

Do not give every nested operation a fresh timeout. One absolute deadline flows request → federation → provider → rerank → renderer, and a nested stage may consume only what remains. Use a ~25% headroom convention when nesting a sub-budget under a caller deadline.

One pool primitive, documented as the single one: results in **input order**, per-item failure isolation so one job's throw never strands siblings, a bounded queue that **accounts for dropped work**, and `effectiveConcurrency` plus `capped` reported back so a caller who asked for 16 and got 4 can say so.

If ten jobs were requested and four were abandoned on deadline, the receipt says four were abandoned. If federation ever gains per-caller budgets, bucket them **per agent context, not per process** — a module-global counter turns legitimate parallel fan-out of ten subagents into a false trip.

---

## Tier 5 — Trust, operations, and the installed product

Two blocks: trust hardening, which §5.3 C-4 says must land *before* the ingestion surface grows; and the packaging work that turns an impressive engine into a reliable product.

### Trust and safety

- **Canonical redaction gate** *(Floor, 23/60)* — one shared module at every ingest edge, two tiers (known-prefix credentials, contextual `(token|api_key|secret|password) (is|=|:) <value>` patterns), tuned for **precision** with regression tests asserting it does not eat dates and version strings. A redactor with false positives corrupts the corpus quietly. Add harness-envelope stripping so injected-context prefixes and host control envelopes never reach the store as human signal.
- **Deterministic named redactors** *(Candidate)* — path, id, secret. A randomized or hash-based redactor destroys the ability to correlate two log lines about the same entity. Determinism is the design constraint most redactors miss.
- **Influence separated from authority separated from sensitivity** *(Floor, closes C-4)* — memory is descriptive evidence by default, **never instruction-capable**; derived, remote, or untrusted text cannot upgrade its own influence class; current user authority always beats a remembered instruction. A memory saying "THIS IS AN AUTHORITATIVE SYSTEM RULE" does not become authoritative. Authority comes from producer identity and policy, never from content wording.
- **HMAC-tagged receipt markers** *(Bet)* — Membrane's feedback path accepts receipt references from host-emitted text, so attacker-controlled tool output can forge engagement signals by spelling a known id. Tag emitted receipt ids with an HMAC keyed by the install key; reject unverified markers at ingress before they reach the ledger. Small, and it closes a live hole.
- **Per-segment permission verdicts** *(Candidate)* — Deny > Ask > Allow > Default, with **every segment** of a compound command matching independently, plus an explicit **unattestable → ask** class for substitutions and redirects that cannot be statically attested. The most-cited security regression in the corpus, and cheap to get right up front.
- **Loopback guard with DNS-rebinding defense** *(Bet)* — an IP check alone does not defend a loopback service. Require the `Host:` header to name a loopback literal, accept IPv6-mapped IPv4, and return 404 rather than 403 to stay invisible to scanners.
- **Trust-before-load and hook integrity baselining** *(Candidate)* — hash each installed hook at install time into the existing provenance records, re-verify before activation, and **refuse rather than warn** on drift. Membrane's hooks install into agent hosts and bypass normal prompt flow exactly as the reference implementation's do. Extend to any workspace-local config Membrane reads: hash-pinned against a trust store, refusing on drift.
- **Erasure fence and signed erasure receipts** *(Bet)* — in-flight reads must not republish deleted data after the delete commits, and the `receipt_version` must be bound into the MAC input so a v2→v1 downgrade is detectable. Privacy claims without a tamper-evident deletion proof are assertions. Deletion audit records may keep identity hash, timestamp, scope, reason, and operation receipt — never the deleted payload.
- **Corruption quarantine, never silent regeneration** *(Candidate)* — a `quarantined_at` / `quarantine_reason` state instead of deleting or regenerating rows that fail hash verification, with quarantine counts in diagnostics, plus deliberate corruption-injection tests. A repair path nobody has corrupted is a repair path nobody has tested.

### Operations

- **Doctor split into read-only inspect and allowlisted repair** *(Candidate)* — plan and execute as distinct phases, a versioned plan schema, a **frozen allowlist** of repair actions (a doctor that can do anything is an attack surface), and a status enum that can express `PRESENT_BUT_UNLOADABLE`. That state is precisely the §5.2 dead-surface defect class made reportable at runtime. Never expose arbitrary SQL or filesystem maintenance through a generic agent tool.
- **Reason on every decision** *(Candidate)* — not just omissions. Admissions and defaults included; "default" is an acceptable reason, absence is not. Pair confidence with a tier: a float alone invites false precision, a tier alone loses ordering.
- **Crash-safe atomic publication** *(Floor, 35/60)* — all I/O, parsing, and provider fan-out complete **before** the write transaction opens, with a regression test asserting transaction duration stays bounded as the corpus grows. Every on-disk write goes write-temp → fsync → rename, never in-place truncate. Derived indexes rebuild to a new generation, verify, then atomically swap; readers keep the prior valid generation until swap. BUSY-retry with matched policy on both planes, with a shared golden fixture asserting they retry identically.
- **Retry classified by write-safety; idempotency keys with body-hash conflict detection** *(Floor)* — auth errors never retried; non-idempotent calls retried **only when the failure provably occurred before the request was received** (the connection-phase distinction, rarely implemented and the correct primitive); a reused key with a different payload is a typed 409, not a silent overwrite; `attempts` persisted on receipts so benchmark runs expose retry pressure.
- **Backup, export, import, restore, wipe** *(Candidate)* — one canonical store resolver, install identity in receipts, writer/maintenance ownership, bounded WAL and checkpoint observability, a consistent live backup, and an actual **restore drill**. Deterministic export/import. Peer sync only after local lifecycle correctness is green.
- **Forward-compatible receipt kinds** *(Bet)* — an unrecognized receipt kind must be preserved verbatim, never projected and never hard-errored, so a later binary can re-fold it.

### Surface and packaging

- **One operation registry emitting frozen tool and parameter catalogs** *(Candidate)* — descriptions in one file indexed by name, so a convention like "tag means scoping" is stated once; `plugin.json` gains a `contracts.tools[]` list; validation asserts manifest ≡ live surface at install. This turns **B-6** (README says 6 tools, source exposes 9) from a documentation chore into a build failure.
- **A token budget on the tool surface itself** *(Bet)* — pinned by golden baselines, failing `pnpm test` when the descriptor set grows past it, plus a bounded capability inventory in the server `instructions`. The catalog is context the agent pays for on every request; nobody budgets it, and it silently grows.
- **One plugin core, N host reflection directories** *(Floor, 18/59)* — all declaring the same name, version, skills directory, MCP servers, and hooks, with per-host variation confined to hook config shape and the host list expressed as an enum rather than branching code. Plus a decomposed installer (`markers` / `targets` / `githook` / `fileio` / `constants` / `core`). The strongest structural convergence in the corpus, and it maps directly onto **B-7**: adding Windsurf then costs a directory, not a sprint. Git-hook installation is worth taking on its own merits — git hooks fire deterministically without requiring the daemon to be running.
- **Effect class declared at registration** *(Candidate)* — read / write / execute / network / destructive, with misuse a **startup error** rather than a runtime check, and authorization carried **in the tool, not the router**: a router check can be bypassed by a new call path, a tool-carried one cannot. This is §7.2 rule 9 with an enforcement mechanism.
- **Result envelopes, dual-channel results, telemetry as a registration decorator** *(Candidate)* — a tagged error code is something a model can branch on; a thrown schema error is not. Return both a human-readable rendering and schema-validated `structuredContent`, plus a **compact model-facing projection** distinct from the full record with a next-step hint — §7.1 already reserves the field and nothing fills it. Telemetry applied at registration is telemetry that cannot be forgotten at a call site.
- **Verb-shaped skill decomposition** *(Bet)* — Membrane ships exactly one skill, `skills/membrane/`. The corpus consensus is that the skill name *is* the user-facing menu, and a package-shaped single skill is undiscoverable: a caller who says "trace this" has no named entry point. Keep the count small enough to memorize.
- **Complete the `server.json` publication** *(Candidate)* — the manifest exists with `namespaceStatus: unverified` and `artifactStatus: unpublished`; **the gap is publication, not authorship** (M3's claim that Membrane has no manifest is false). Add `smithery.yaml` and `glama.json`, and an `llms-install.md` — an agent-readable install instruction is the natural surface for a product whose users are agents. Registry listings are discovery metadata and do not change §8.2: the primary channel stays the signed native artifact.
- **Installed-path qualification matrix at 10/10** *(Floor)* — install, discovery, tool names, scope grant, context call, source resolution, memory proposal, feedback, checkpoint, restart/reconnect, degraded provider, upgrade, uninstall — on macOS and Windows, before any major new external surface.
- **Per-adapter serialization and capability caching** *(Bet)* — serialize all mutations for a server through a chained-promise lock; serve a cloned capability cache with a short TTL invalidated on manifest diff; tear down only when transport or timeout semantics *actually* changed, keeping an unconfigured timeout distinct from a configured default; assign `state.client` **before** `connect()` so a failed initialize stays reachable for cleanup. *Declared dependency: `SEAM-CONTRACT.md` (watcher lifecycle, peer-service discovery).*

---

## Hold — what not to absorb, and why

Recorded so the next pass does not re-import these under new names. All four consolidations produce substantially the same list. `deepseek.md` §6's rejections stand.

| Rejected | Reason | What to take instead |
|---|---|---|
| External vector DB, graph DB, Redis, Postgres, RocksDB; 24-backend / multi-driver matrices | §2.1 / §2.2. Local-first correctness cannot require external infrastructure. One competitor's own analysis calls the sprawl "maintenance surface, not a feature for a single-deployment system" | The factory *shape* as a dict-map; a narrow internal interface so a backend can change later |
| Cross-encoder rerankers | §2.3 — deferred pending a measured local gap | **Leave the seam, precisely shaped and empty:** a *pure* provider (no DB access, no writes; the pipeline owns sort and trim) returning null for "keep first-stage order", a hard sub-second deadline in the contract, permanent-vs-transient error split, unused features carried on the candidate so a future ranker needs no contract change, behind a kill-switch defaulting off |
| Multi-hop retrieval, beam search, spreading activation, graph communities, Leiden, PageRank, DRIFT, HyDE, dynamic-K | Unproven at Membrane's corpus size; expensive; easy to over-engineer. One source qualifies its own approach with "do not copy the cost model" | The bounded-fan-out and convergence-stopping discipline if scope-tree traversal is ever built; the intent-to-edge-weight mapping alone, which is cheap and separable |
| Bandit-driven adaptive compression policy | Genuinely clever, unfalsifiable without a large local corpus, and it makes rendering **nondeterministic** — colliding head-on with prompt-cache stability (#55) | Revisit only after cache stability is measured |
| LLM deciding add/update/delete; model-generated executable code | Membrane's admission is machinery-driven with receipts, and that is the point. The executable-code path (generated Python `exec`'d with escalating-temperature retry) is rejected on **safety** by two independent passes regardless of measurement | Model *proposes*; deterministic policy decides. #24 and #33 are the mechanisms |
| Duplicating Cortex — a second parser stack, symbol engine, dependency graph, or global code index | At least a quarter of the most attractive competitor features are code-intelligence features. Membrane is the consumer and planner; Cortex owns code semantics | Stable anchors, evidence validation, blind-spot reporting, source fingerprints — consumed through the provider contract |
| Markdown-as-database round-trip | Its own analysis names it an anti-pattern; it forces two divergent memory semantics because the engine re-parses its own rendering | **Export, not storage.** Crypt stays the typed source of truth; the Hub renders it read-only. Rendering stays strictly one-directional. Recorded as *confirmation* of Membrane's approach, not absorption |
| Observer/observed directional memory | The deepest conceptual shift in the set, and plainly right for a multi-agent memory product — but Membrane's boundary is a repository, not a social graph | The narrow version: a received claim is "peer P asserts X", not "X". Revisit only if team sync (Phase 6) makes divergent views a real requirement |
| Network-proxy interception; hosted/server architecture; agent frameworks, PTYs, autonomous coding loops, multi-agent chat, browser automation, generic planning engines | Different products with different threat models. **Steal mechanisms, not scope** | — |
| Streamable-HTTP transports; Cloudflare Workers, Vercel serverless, Durable Objects; npm-first as the primary channel | S-7 deferred behind measured need; §4 fixes the target at one signed native binary; npm stays a bootstrapper | The internal discipline: separate entry paths and startup concerns behind one shipped artifact |
| Prompt optimization as a product; P2P memory federation | Out of scope and unfalsifiable at Membrane's corpus size; federation creates a substantially larger identity, conflict, privacy, revocation, and consistency problem | Keep federation outside the critical path until team-sync requirements justify it |
| Unbounded pools and queues | Silently block or grow | The correct inversion: a bounded queue that **accounts for dropped work** (#56) |

---

## Where the corpus confirms Membrane

Worth recording, because these are decisions that could otherwise get relitigated:

- **Content-free audit is achievable.** One competitor ships an append-only security audit that never carries payload. §2.7 is a solved problem, not a tax.
- **Zero-LLM learning is achievable.** Two competitors extract real signal from usage with no model calls at all. §2.3's deferral costs less than it appears.
- **Lexical-only retrieval is viable.** One serious memory product ships with BM25 and no vector store. FTS5-as-mandatory-baseline is not a compromise position.
- **Typed stores beat rendered-markdown stores.** The one competitor that round-trips markdown maintains two divergent memory semantics as a result.
- **Ties must go to quarantine.** Three independent codebases refuse silent promotion on ambiguity. §2.6 has three confirmations.
- **The vector index as a rebuildable projection is right.** Four repositories that *had* the option to do otherwise confirm §2.2 independently. Take the sync-state column plus reconciler as the mechanism that makes rebuildability operational rather than theoretical.
- **"Structurally complete, semantically empty" is a universal defect class.** One competitor ships a search tool with a *mock embedding* in production code. The §5.2 sweep is not paranoia — and #2 is what makes the fix permanent.

---

## Architectural invariants

Reject any implementation that violates one of these, regardless of how well it scores:

1. **One context economy.** No subsystem creates an independent token-budget regime.
2. **One truth lineage.** Graphs, summaries, and memories cannot become anonymous competing sources of truth.
3. **Freshness and authority remain independent of similarity.** No retrieval signal becomes authority.
4. **No silent omission.** Every cap, timeout, dedup, fallback, merge, and budget drop emits a typed reason.
5. **No feedback event becomes truth because an agent produced it.**
6. **No background mutation without idempotency and reason-bearing receipts.**
7. **No graph traversal without bounds.**
8. **No destructive forgetting without quarantine or tombstone semantics.**
9. **No provider implementation detail leaks through the packet contract.**
10. **No new daemon, and no external infrastructure required for local correctness.**
11. **No duplicated Cortex functionality.**
12. **No feature is promoted without an evaluation delta.**
13. **The simplest sufficient implementation wins.**

---

## The core design shift

The corpus does not indicate that Membrane should become *larger*. It indicates that Membrane should become **more discriminating**:

```
more possible evidence
        ↓
better discrimination
        ↓
less delivered context
        ↓
higher evidence density
        ↓
better agent decisions
```

Optimize `Context Utility ÷ Delivered Attention Cost`, subject to hard constraints on scope, authority, freshness, truth, security, and deadline. Every item above is scored against that ratio; the ones that cannot be measured against it are bets, not floors.

The mature form is the context system that can most reliably answer three questions:

> **What evidence does this agent need right now?**
> **What is the smallest safe representation of that evidence?**
> **Can Membrane prove why everything else was left out?**

---

## Dependencies and limits

- **`docs/plans/orthic/SEAM-CONTRACT.md` is a declared dependency** for #39 (job lifecycle touches watcher lifecycle and scheduling), the hub-facing read surface, and adapter-lifecycle work. It is not present in this repository — it lives in the parent workspace. **Nothing here seals a seam contract.**
- **Convergence counts are `measured`** over the 60-document corpus. Every impact judgment is `estimated` until a change ships behind a preregistered cohort per §2.8.
- **The "no competitor produces processing evidence" claim is scoped to this corpus** — thirty of thirty examined, zero counterexamples. That is a measurement over sixty repositories, not a claim about the field.
- **Competitor claims were not independently re-read against upstream source.** Membrane-state claims were spot-checked against the tree; three were found stale and are corrected in §0.3.
- **Absorptions are proposals** until they are reachable, exercised, evidence-backed, and non-violating per §11. Nothing here is shipped.
