# Membrane — Final Canonical Implementation Book

**Status:** FINAL — single implementation and absorption authority for `Orthic-Labs/Membrane`  
**Date:** 2026-08-18  
**Baseline snapshot:** `main` at `175b47e1fea7fb9a277fe4392da21206e15de2a8`  
**Supersedes:** every earlier Membrane improvement guide, implementation guide, absorption ledger, focused ContextPlan/RecallCircuit guide, and the August 12 plan as implementation authority.  
**Self-contained:** this file includes the architectural doctrine, implementation sequence, file-exact P0/P1 execution spec, evidence-confidence register, 60-repository absorption coverage, rejection/defer list, rollout/rollback, and Definition of Done. No companion planning document is required.  
**Does not replace runtime product truth:** generated `docs/architecture.md`, `README.md`, and `AGENTS.md` remain product/source documentation and must reflect landed code.  
**Implementation state:** this is implementation-ready authority; it does not claim the work is already implemented.

---

## 0. Executive decision

Membrane does **not** need a new architecture. It already owns the hard, differentiated part:

- five typed public shapes — `ScopeGrantV1`, `ContextCandidateSetV1`, `ContextPacketV1`, `ContextReceiptV1`, `KnowledgeEmissionV1`;
- one Push / Pull / Persist context economy behind one planner;
- authority and freshness kept distinct from similarity;
- one global attention budget across `native` / `rendered` / `resolver_backed` / `metadata_only` lanes;
- typed omissions, degradation, receipts; repository confinement and signed grants;
- local-first SQLite Crypt with temporal facts and immutable supersession;
- deterministic transforms and compression primitives; resident federation and local vectors;
- Application / Control / Data planes; a real Membrane ↔ Blueprint boundary.

Every source agrees the corpus asks Membrane to **finish and connect** this spine, not replace it. Thirty of thirty examined competitors record what *changed*; none records **what was dropped from a context packet and why**. That receipt/evidence layer is the product differentiator, and the highest-return work is making it reachable, exercised, and measured.

Target:

> **Membrane is a local-first, evidence-aware context control plane and context compiler.** It governs how information is admitted, represented, stored, indexed, related, retrieved, transformed, delivered, remembered, forgotten, and verified — and it produces evidence for every material decision.

Memory is only one class of governed context. Documents, decisions, taste, gotchas, procedures, episodes, temporal facts, artifacts, rules, audit evidence, live Git state, and Blueprint code semantics all participate in the same economy without being flattened into one blob.

The design shift the corpus asks for is not *larger* but **more discriminating**: more possible evidence → better discrimination → less delivered context → higher evidence density. Optimize `Context Utility ÷ Delivered Attention Cost` under hard constraints of scope, authority, freshness, truth, security, and deadline.

---

## 1. Locked architectural invariants

Breaking one requires an explicit architecture decision, migration, compatibility proof, and a new evaluation baseline.

1. **Five public shapes stay stable.** Enrich internally, keyed by existing candidate/source/trace IDs. Version deliberately (V2) only when a real consumer cannot be served by existing fields. No donor-shaped public envelopes.
2. **One planner owns final policy**, in order: grant validity → eligibility/scope → authority → freshness → provider-local relevance + bounded fusion → dedupe/diversity → global token/byte admission → representation/lane → publication revalidation → omissions + receipt. Providers describe evidence; Membrane decides attention.
3. **Never flatten unrelated scores.** Cosine, BM25, graph support, Blueprint confidence, rule priority, freshness, and feedback are not one calibrated scale. Use hard policy classes → rank fusion (RRF) → bounded utility modifiers within equal policy classes → deterministic canonical-ID tie-breaks. Keep *generation score* (could be relevant), *policy score* (may be admitted), and *utility* (worth its tokens) as distinct dimensions.
4. **One cross-provider attention budget.** Lanes reconcile to one ceiling; provider ceilings bound fan-out cost only.
5. **SQLite/Crypt is canonical durable truth; everything else is a projection.** FTS, vectors, relation graph, derived summaries are rebuildable. Markdown is source evidence or export, never a round-trip database. Git + Blueprint supply freshness/repository evidence. The artifact store holds immutable content-addressed raw payloads.
6. **Blueprint owns code semantics** — parsing, ASTs, symbols, references, imports/calls/types, entry points, blast radius, rename/move continuity, structural fingerprints, snapshots/diffs, failure-signal resolution. Membrane consumes evidence and decides scope, currency, authority, budget, and delivery mode. No second parser/index stack in Membrane.
7. **Application / Control / Data planes stay separate.** No fourth plane; no SQLite ownership in MCP handlers; no hidden mutation in request routing.
8. **Local-first.** No remote vector/graph store, Redis, distributed queue, or hosted retrieval as a required dependency. Add an abstraction only when two real implementations need it.
9. **Provider failure is typed degradation, not packet fiction.** A failed provider degrades its own lane; healthy providers continue unless the failed one is a hard prerequisite.
10. **No silent omission.** Every cap, timeout, dedupe, fallback, merge, and budget drop emits a typed reason.
11. **No feedback event becomes truth because an agent produced it.** Feedback may move retrieval pressure; it never alters authority.
12. **No background mutation without idempotency and reason-bearing receipts. No graph traversal without bounds. No destructive forgetting without quarantine or tombstone.**
13. **No feature promotes without an evaluation delta. The simplest sufficient implementation wins.**
14. **Generated docs stay generated.** Never hand-edit `docs/architecture.md`; competitor clones stay research inputs, never dependencies.
15. **Evidence confidence is part of the implementation contract.** `Floor` means independently convergent and non-negotiable; `Candidate` means strong but mechanism-verify before schema commitment; `Bet` means one-pass or low-convergence evidence and must be feature-flagged/benchmark-gated; `Hold` means rejected or deferred and must not be re-imported under a new name. The complete register is embedded in Appendix B.
16. **Current-state claims are perishable.** Re-verify the source tree before turning any plan line into a ticket. Phase 0 regenerates the manifest and freezes the new baseline before code changes.

---

## 2. Ownership map

| Capability | Owner | Membrane action |
|---|---|---|
| Grant validation, final freshness/authority policy | Membrane planner/runtime | Keep central, fail closed |
| Fusion, budget, lanes, omissions, publication, receipts | `membrane-core` + runtime/MCP | Strengthen; never delegate to providers |
| Durable knowledge, temporal facts, lifecycle, feedback, relations | Crypt (`crypt-core` / `crypt-store`) | Implement locally |
| Lexical + semantic + temporal memory retrieval | Crypt | Implement locally |
| AST/symbol/reference/call graph, stable code identity, impact | Blueprint | Consume via `engine/federation/providers/blueprint.py` |
| Code/document claim validation | Blueprint + Audit | Feed resolution status to planner |
| Immutable raw artifacts | Membrane runtime under Hub-controlled storage | Add governed content-addressed artifact abstraction |
| Tool-result/context reduction | Membrane Push/runtime | Converge `runc`/`skel`/`compress`/truncate under one transform contract |
| Session working context | Membrane runtime + Crypt episodic | Bound it; never own full host transcript |
| Background curation | Crypt jobs, scheduled by Hub lifecycle | Bounded, idempotent, never prompt-critical |
| OS start-at-login, child processes, restart/backoff | Hub | Expose readiness/drain/health/identity only |
| Agent execution, PTYs, model routing, UI shell | Legion / OmniRouter / Hub | Out of Membrane scope |
| Remote vector/graph DB, RDF/SPARQL, KV-cache memory | None | Reject / defer |

---

## 3. Target architecture

### 3.1 Pull — smallest useful current evidence set

```text
host / MCP / hooks
        ▼
ScopeGrant + request/task/session identity + one deadline
        ▼
┌───────────────── CANONICAL PLANNER ─────────────────┐
│ grant/scope → authority → freshness → fusion → budget │
│ representation → publication fence → omissions/receipt│
└───────────────────────────────────────────────────────┘
        │ ContextPlan-selected / staged candidate generation
   ┌────┼────────┬──────────┬────────┬─────────┐
 Blueprint  Crypt  Live/Git   Rules    Docs    Audit
 code    knowledge current  policy  evidence findings
         ├ exact/ID/anchor
         ├ FTS5/BM25 lexical
         ├ vector semantic
         ├ temporal (as-of)
         ├ bounded relation/entity
         └ active working/session
        ▼
eligible candidates → rank fusion → bounded utility → diversity
        ▼
two-phase budget fill (breadth floor → depth upgrade)
        ▼
native / rendered / resolver_backed / metadata_only
        ▼
ContextPacket + ContextReceipt → model → used/ignored/contradicted → signal ledger
```

### 3.2 Persist — remember selectively

```text
observation / explicit request / verified outcome / session end
        ▼
validate scope + producer + influence + sensitivity + DLP
        ▼
normalize family / kind / epistemic state
        ▼
logical ID + content hash + evidence identity + source refs
        ▼
exact duplicate → no-op (success)
conflict        → preserve both + conflict record
correction      → immutable supersession
low-trust/unsafe→ quarantine / reject
eligible        → canonical record + evidence links + sidecar signals
```

### 3.3 Push — reduce without losing recoverability

```text
large payload
        ▼
classify content; mark protected/atomic spans
        ▼
1 exact dedupe → 2 hash-address raw artifact → 3 structure-preserving reduction
→ 4 optional compression/summarization → 5 explicit truncation last
        ▼
query-critical verifier (restore exact spans from artifact on loss)
        ▼
global budget / lane planner
```

---

## 4. Canonical knowledge and evidence model

Separate **what a thing is**, **why it is believed**, **when it was true**, and **how useful it has been**.

### 4.1 Identities by purpose — never one ID for five questions

| Identity | Answers |
|---|---|
| `logical_id` | which durable object is this (stable across metadata changes) |
| `content_sha256` | exact immutable content/version |
| `evidence_id` | which observation/source event supports it (many per record) |
| source/code anchor | where it resolves; Blueprint-backed for code; resolution state may change without rewriting history |
| derivation fingerprint | how derived: transform/extractor/model/prompt/config version |
| `artifact_id` = `art:<sha256>` | which immutable raw payload |
| `event_id` | which lifecycle/write event |

Key derived data by its invalidation key: `(source_id, content_hash, prompt/code version)`. Re-mining reproduces the same ids; dedup is free; a version bump re-queues everything. Keep two hashes for code anchors: an exact source hash (bytes identical?) and a structural fingerprint (same code after formatting/movement?).

### 4.2 KnowledgeRecord (internal, behind the hot `MemoryEntry` projection)

```text
KnowledgeRecord
├─ logical id · canonical content · content_sha256
├─ scope / repository / session lineage
├─ family · kind · epistemic truth_state · intent_state
├─ authority · influence_class · sensitivity
├─ lifecycle_state
├─ observed_at · valid_from/valid_until · expires_at · recorded_at
├─ supersession refs · evidence refs · derivation refs · relation refs
└─ (mutable signals live in a sidecar, not here)
```

Three orthogonal axes — do not conflate:

| Axis | Values |
|---|---|
| **Family** (what it means) | observation, episode, semantic_fact, procedure, preference, entity_summary, evolving_belief, artifact_reference |
| **Kind** (product vocabulary) | document, section, decision, constraint, memory, taste, gotcha, insight, lesson, procedure, failure, success, entity, session, task, artifact, rule_reference, code_claim |
| **Lifecycle state** | active, warm, cold, archived, superseded, expired, quarantined, tombstoned |

Plus tier (working / episodic / semantic) for recall aggressiveness. **States without distinct behavior are taxonomy, not architecture.** Do not add families or states until they need distinct write/retrieval/lifecycle behavior.

Epistemic state: `truth_state ∈ {source_verified, code_verified, observed, derived, inferred, asserted, unverified, contradicted}`. A model-generated summary or relation never becomes authoritative because its prose sounds certain. **An inferred fact never silently overrides an explicit one**, and it stays retractable when its premises change (derivation edges exist for revalidation).

### 4.3 Sidecar of mutable signals (keyed by logical id)

`retrieval_count, selected_count, delivered_count, verified_used_count, ignored_count, contradicted_count, last_retrieved/used/contradicted_at, base_importance, retention_strength/hotness, decay_state, last_verified, score_epoch, ranking_policy_version`. Rebuildable and recalibratable; canonical content is not rewritten because retrieval value changed.

### 4.4 Resolution states for source-backed evidence

`resolved | moved | ambiguous | drifted | missing | unsupported | inaccessible | revoked`. Resolution is current-state; historical evidence identity is immutable.

### 4.5 Write dispositions — every write ends in exactly one

`retain | update_metadata_only | supersede | merge | conflict | no_op | proposal | quarantine | reject | expire | forget | restore`

**`no_op` is a first-class success.** The store is not a scratchpad for unresolved intake, question lists, inventories, or activity logs.

### 4.6 Special knowledge classes

- **Documents / Markdown spine** — first-class evidence source. Identity = path + content hash + parser version + doc type + authority + scope + revision. Structure = headings, sections, anchors, references, frontmatter, code/file refs, exact source ranges. Index hierarchically (document → section → claim → named item); derived decisions/constraints/gotchas/taste point back to exact ranges; recompute only what file/section hash + parser version says changed.
- **Taste / preferences** — `family: Preference, kind: taste` with subject, claim, scope (global/domain/project/task/temporary), authority (`explicit_user` etc.), confidence, observed_at, supersedes. Explicit current preference outranks weak inferred; does not decay merely with time.
- **Gotchas / insights** — procedural knowledge with `trigger, applies_to, avoid, prefer, severity, confidence, source, verification, last_applicable, source_drift_state`. Surfaces when the **planned action matches the trigger**, not on prose similarity. On a failure episode, extract the `violated_assumption` and emit a `precondition_check`.
- **Negative knowledge** (failures, guardrails, invalidated assumptions, known-bad approaches) is durable and high value, and never gains instruction authority.
- **Working-set classes** (P1, `mcp/working-context.mjs` + `membrane-runtime/src/working_context.rs`, JS/Rust twins bumped in one commit): `pinned` (not evictable while its task constraint is active), `resident` (current high-value set), `prefetched` (likely useful, lower priority), `reconstructable` (keep resolver/metadata, drop text), `quarantined` (untrusted/stale/contradicted; never auto-inject). Replaces plain in-order walk under `max_blocks/max_bytes/max_recent_turns`.
- **Session packets** — at session/task close, one bounded episodic record: task identity/goal, repo/branch/worktree/revision, decisions, open work, failed approaches, verification results, exact identifiers, artifact refs, contradictions, evidence refs. Pinned and decay-exempt. Episodic, not automatically semantic. Keyed on message ids so re-running is a no-op.


### 4.7 Human-readable knowledge inspection — Membrane read model, Hub presentation

Membrane must expose a **read-only inspection surface** for governed knowledge. Membrane owns the query/read model and provenance fields; Hub owns presentation. This is not a second database and not a new policy plane.

Required collections/views: `Documents`, `Decisions`, `Memories`, `Taste`, `Gotchas`, `Procedures`, `Sessions`, `Entities`, `Conflicts`, `Archived`, `Quarantined`.

For every record, the inspection contract can show: content or safe projection; family/kind; source; evidence; authority; freshness; validity interval; lifecycle state; relations; supersession history; resolution state; and **why it was retained**. It supports deterministic Markdown export for review/diff. Sensitive fields remain subject to current grant/DLP policy; inspection never upgrades authority or instruction influence.

This requirement closes the accountability loop: anything Membrane stores or retains must be inspectable without reverse-engineering SQLite or relying on generated prose.

---

## 5. Retrieval — canonical stage order

**Stage 0 — normalize.** One monotonic request deadline propagated everywhere; nested stages consume remaining time only (~25% headroom convention). Deterministic query-signal extraction (paths, identifiers, stack traces, quoted strings, error codes, hashes, dates, *why/when/previous/decision/changed*) emits weights that decide which channels get budget — never scope or authority. Recorded in the receipt. No model in the hot path.

**Stage 0b — ContextPlan before fan-out.** Membrane decides which computations are worth running before providers execute: a deterministic, conservative plan (`engine/federation/context_plan.py`) classifies the task (security / migration / architecture / impact / debug / local_edit / docs / general), assigns risk, selects the provider subset, and sets the Blueprint traversal policy (`policy_id`, `max_hops`, `max_paths`). Not an LLM router. `general` keeps today's full provider set until evaluation proves narrowing safe; only obvious low-risk classes skip advisory/history lanes. Rolled out shadow ("would run" vs "did run") → low-risk classes → broader classes. Receipts carry `taskClass/risk/providers/blueprintPolicyId`, never task text. Retrieval is staged: identity/rules/live → Blueprint structural recall → *sufficiency evaluator* → audit/architect/memory/skills only if justified → expensive semantic escalation only if still justified. The evaluator (`retrieval_evaluator.py`, P1) returns `sufficient | insufficient | ambiguous | contradictory | stale | unsafe | provider_failure` plus typed reasons/missing; it decides whether another stage is justified, never answers the task. **Stop condition:** do not retrieve more because budget remains; retrieve more when expected decision value is positive. Rule-based first; no LLM call to decide whether an LLM should receive more context.

**Stage 1 — candidate generation.** Crypt channels behind one trait, one file each, independently ablatable, optional ones off without breaking retrieval:
1. `exact` — canonical IDs, anchors, paths, entities, error/quoted text;
2. `lexical` — FTS5/BM25;
3. `semantic` — existing local vector path;
4. `temporal` — valid/as-of/supersession-aware;
5. `relation` — bounded, seeded by already-relevant records;
6. `working` — task/session-local blocks.
Blueprint structural retrieval enters as provider candidates.

**Stage 2 — hard eligibility** (before any soft relevance): grant + repository scope; ACL; influence/instruction policy; quarantine/deletion/revocation; temporal validity/**expiry checked here, not as a rank penalty**; source resolution state; freshness constraints; sensitivity routing. No reranker resurrects an ineligible candidate.

**Stage 3 — authority and freshness classes.** Fresh current source outranks similar stale memory.

**Stage 4 — rank fusion.** Weighted RRF, **fused once**, with explicit `max_rank_penalty` for absent docs, deterministic tie-break on canonical id, versioned ranking-config constant on every result. Retain per-channel rank/score and an exact-evidence flag as receipt metadata. Absent optional stages are **score-neutral** (ordering preserved, no synthetic scores).

**Stage 5 — bounded utility modifiers**, only within equivalent policy classes: verified effectiveness, recency where meaningful, retention strength, relation support, contradiction penalty. **Exact-evidence pinning:** a verified exact match on path/symbol/error/SHA/rule id is pinned to the top of its lane after adaptive layers — reordering only, still vetoable by policy.

**Stage 6 — diversity**, after fusion never before eligibility: penalize same source/lineage/content hash/artifact family/near-identical embedding after the first winner; never aggressively against exact or unique-authority evidence. Deterministic; no model for ordinary dedupe.

**Stage 7 — global admission with two-phase fill.** *Phase A breadth floor:* every admitted candidate at its category's minimum detail tier under a per-entry cap (~2× average share). *Phase B depth upgrade:* leftover budget spent in score order. Emit `floor_tokens`, `upgrades_by_lane`, `spare_upgrades`. This is the single highest-confidence algorithmic item in the corpus; add a starvation fixture with one oversized provider.

**Stage 8 — publication fence.** Revalidate grant/policy epoch immediately before bytes leave the process; on change, retry once under new epoch or emit typed `policy_changed`. Never publish stale-authority bytes.

**Stage 9 — receipt and feedback binding.** Journey per candidate: `generated → eligible/ineligible → ranked → selected/dropped → delivered → resolved → verified_used/ignored/contradicted → superseded`. Typed rejection reasons: `redundant_with, stale, lower_authority, scope_mismatch, budget, timeout, superseded, unsafe, low_utility`. Per-lane latency. Enrich the existing receipt; no second debug protocol.

### 5.1 Production lexical retrieval

The verified weakness: production lexical is keyword-exact + substring + stored score; FTS5 exists only in a test. Implement **SQLite FTS5/BM25** as a rebuildable projection: BM25, phrase/exact bonus, bounded prefix, field weighting, scope/family/authority filters, deterministic tie-breaks, incremental upsert/delete from canonical events, schema/index version, rebuild/repair command, fallback to current lexical path when unavailable/corrupt. Tokenize `snake_case`, `camelCase`, `PascalCase`, `kebab-case`, path segments, `module::symbol`. Retrieval must work with zero embedding models. No Tantivy/Elasticsearch/Qdrant. Separate per-channel overfetch from provider result limit from final admitted size; overfetch bounded and visible.

Fallback ladder: FTS+vector → hybrid; FTS only → exact+FTS+temporal; FTS down → legacy lexical (+vector); relations down → skip expansion; Blueprint degraded → keep healthy providers, type the missing evidence.

---

## 6. Persistence admission and lifecycle

### 6.1 Admission before durable truth

`schema → scope → producer → secret/PII/DLP → epistemic classification → identity → novelty → near-duplicate → contradiction/conflict/supersession → durability/utility → disposition + receipt`. Rules decide obvious cases; a model touches only ambiguous ones, off the hot path. Typed reasons: `ephemeral, conversational_filler, unsupported_claim, duplicate_exact, duplicate_near, superseded, contradicted, insufficient_evidence, scope_invalid, secret_detected, low_information, low_expected_utility`. Do not persist everything and hope Dream cleans it.

Admission questions: novel? useful? durable? evidenced? current? correctly scoped? merely conversational? redundant? instruction or description?

### 6.2 Conflict is not overwrite

Classify: exact duplicate → no-op/reinforce; new authoritative version → supersede; simultaneously incompatible evidence → preserve both + conflict; weak derived vs direct → quarantine/reduce influence; uncertain identity → keep separate. Conflict records live in a sidecar with **relationship** (exact-value, negation, entailed, constraint, probabilistic, scope-apparent, refinement), **cause** (correction, temporal change, scope difference, entity mismatch, write error, unresolved), evidence strength, action, audit reason — diagnosis separate from action separate from row status, so verdicts are recomputable.

Non-conflict taxonomy (both statements hold): temporal supersession, list-valued predicate, refinement, scope mismatch, same-name-distinct-subject, conditional-unrealized, event restatement. Declare predicates single-value vs multi-value; additive predicates skip the conflict path. Force `same_subject` before `contradicts` and reject contradictions carrying `same_subject: false`; compare booleans by identity (a string `"false"` is truthy). Detect symmetrically; attribute direction only after confirmation, by timestamp with stable-id tiebreak — never by write order.

### 6.3 Temporal semantics — extend the existing model, never build a second

Keep `membrane_temporal_fact` as temporal truth. Distinguish `observed_at` (when learned), `valid_from/valid_until` (when true), `expires_at` (when to stop treating as active), `recorded_at` (transaction time, for "what did Membrane believe on date D?"). Normalize `supersedes` from a comma string into a transition table `(from_fact_id, to_fact_id, effective_at, reason, transition_sha256)` with backward-compatible migration. Policies declare expiry behavior: fail-closed, fail-open, or grace.

### 6.4 Lifecycle: pure, versioned, hysteretic

Retention is a pure function in `crypt-core` over explicit signals, versioned, with hysteresis (promote at ≥A, demote at <B, B<A) and monotonicity asserted by `cargo test`. Per-family curves, not one half-life: preferences very slow (pinned/explicit do not decay by time); decisions slow until superseded; observations moderate; transient debugging fast; temporal facts governed by validity, not decay; derived summaries expire aggressively when supporting evidence changes; artifacts by retention policy. Decay multiplies into retrieval score; crossing threshold **archives, never deletes**. Do not canonize Ebbinghaus/Weibull; fit the simplest policy that wins held-out evaluation.

Reinforcement: `c ← c + α(1−c)`, α≈0.1, bounded. Stages `generated → admitted → delivered → used → helped` stay distinct; only later stages reinforce. `ignored` is advisory; `contradicted` is a verified veto. A memory must not become immortal because a retriever keeps selecting it.

### 6.5 Dream is a reversible maintenance phase, not the lifecycle engine

Dream may find deterministic duplicates, propose consolidations, create derived summaries with full parent/evidence identity, verify anchors, propose promotion/demotion/quarantine. Deterministic guards before any UPSERT (non-empty, size ceiling, identifier-tolerant sentence counting, reference resolution by set membership); retry once, then write a typed failure stamp in the same transaction. Evidence-coverage gate (≥N sessions, ≥M time strata) before auto-commit; prohibited categories force human review. **Ties go to quarantine.** Curation pass = hygiene → union-find cluster → distill on pre-clustered input → archive with `crystallized_from` provenance; record the motive; ship **undo**. Never destroy the only source records.

### 6.6 Session mining

Retroactive: discover host conversation stores, queue past sessions as ingest jobs, everything mined is a **proposal, never an auto-write**. Mine off disk; never intercept live tokens.

---

## 7. Push — reversible compression as the default path

Measured adoption of existing compression is one use in seven opportunities; nothing compresses what the host actually accumulates. Close it with one primitive.

- **`ArtifactRefV1`** (internal first; ride the existing resolver-backed lane): `art:<sha256>`, content class/MIME, byte length, origin hash, scope + policy resolver, sensitivity/influence, created/observed, extractor/transform version, parent/derived refs, integrity/availability. `membrane_retrieve` expands `hash=` markers; golden compressed→expanded fixture. Extends the existing `compress`/`runc` spill store. Capped rotating raw-recovery store keyed by receipt id, so compression can never destroy the evidence of a failure.
- **Ordered ladder:** dedupe → externalize → deterministic noise strip → structured JSON/table/log reduction → code skeleton/signatures → Markdown thinning → bounded extractive compression → optional model summarization → truncation last. Never regex-parse code when a structural parser exists.
- **Representation planner:** per admitted source choose `native / full / excerpt / skeleton / summary / resolver_ref / metadata_only`, degrading in that order; prefer downgrading tier over truncating text. Renderer makes no hidden ranking decisions; ranker makes no rendering decisions.
- **Query-critical verifier:** identifiers, error messages/codes, failing test names, cited ranges, task entities, authority-bearing rules, explicitly requested details must survive; on loss restore exact spans via resolver. Failure semantics: artifact write fails → keep raw; reducer fails → less compressed; verifier uncertain → restore; resolver fails → typed `artifact_unavailable`.
- **Never-worse-than-raw, typed:** persist `TokenBalanceV1 { original, materialized, delivered, provider_billed }` with `materialized ≤ original`, `delivered ≤ materialized`, plus typed `skip_reason` on passthrough. Property-test the inequalities. Token reduction is **never reported without** a paired signal-preservation assertion.
- **Referential-integrity closures:** tool call + result, decision + rationale, error + frames, diff header + hunk, citation + locator evicted together or not at all. Protect the live tail; pin the last real user turn. Unreachable budget → typed `budget_unachievable_with_protections`, `outcome: incomplete`. One byte- and surrogate-safe truncation primitive, notices at edges. `actions[]` audit array on the receipt; distinguish source-capped-at-ingress from dropped-to-fit.
- **Position-aware layout (v2, flag `MEMBRANE_CONTEXT_LAYOUT_V2`)**: hard constraints/rules/policy/protected anchors front; evidence (circuits, docs, memory, skills, audit, architecture) middle; live/dirty overlay and working context late, near the task boundary. Uses existing provider/sourceKind, no protocol field. Flag off == current bytes; flag on deterministic; char cap, reconciliation, and Forge/Membrane renderer parity stay exact. Graduate only on answer-quality non-regression.
- **Determinism-first rendering** for prompt-cache validity: stable ordering/separators, append-only sections, volatile fields out of the prefix, a block matches only strictly earlier blocks. Regression test asserting byte-identical prefixes across turns; cache-miss attribution on receipts.
- **Citation by construction + fabrication guard:** resolver-backed blocks carry `(source_id, byte_span)`; quotes materialize by slicing; models cite by unit id. Every model-cited evidence line must appear verbatim (whitespace-normalized) in a content line of the pack, above minimum length, never a bare `path:line`; fabricated → reject and retry once, then discard. **Presence cannot contradict** — only a named missing thing can; preserve operator shape (`!`).
- Documents/multimodal only after artifact identity is reliable: text/PDF extraction → image metadata/OCR → audio → video keyframes → multimodal embeddings last, all cited, scoped, versioned, rebuildable, expirable.

---

## 8. Blueprint bridge and source-drift verification

"Fresh code evidence outranks stale memory" is currently a policy with no detector. Membrane-side anchor: `repository_id, path, symbol/definition_id, range, content hash, base commit/Blueprint generation, structural fingerprint (from Blueprint), resolution state, last verified`.

Verification hierarchy: exact content/range at current source → current; Blueprint stable id resolves elsewhere → moved; one strong structural match → resolved with updated anchor; multiple → ambiguous; absent → missing/drifted; outside grant → inaccessible, never auto-resolved. Five-way identifier resolution: `symbol | file | text_present | absent | unresolvable` — collapsing text-present and absent into `NOT_FOUND` is the root of the divergence false-positive class. **Authoritative absence requires proof of coverage**: emit not-found only when the binding proves the domain is indexed; otherwise downgrade to indeterminate and surface blind spots as receipt fields.

**RecallCircuit is the unit of Blueprint evidence.** Membrane sends a task-shaped request with policy/hops/paths; Blueprint returns a generation-bound `RecallCircuitV1` (`paths`, `nodes`, `edges`, `unresolved`). Membrane does not traverse the graph. Each **complete** path becomes **one atomic candidate** (`sourceKind: repo_code_circuit`, `id: blueprint-circuit:<circuitId>:<pathId>`, `sourceHash` over the path descriptor, `trustClass: workspace_tracked`, `instructionPolicy: data_only`, `scoreComponents: path_complete / evidence_complete / evidence_coverage / hop_efficiency`, rendered as `A --[KIND]--> B --[KIND]--> C` + evidence refs). The path is the semantic unit; top-k admission must never split `A→B→C` into independently admitted nodes. Generation mismatch or schema mismatch → no candidate + typed warning, never reinterpreted as legacy output. Empty circuit / `no_relevant_seed` → zero candidates + loud typed abstention, never generic repository text. Legacy `blueprint-candidates.mjs` remains the version-skew/rollback fallback. Cache key includes `policy_id/max_hops/max_paths`. Planner treats `repo_code_circuit` at repo-code priority and applies a bounded `circuit_quality` tie-break *within that source kind only* (complete/evidenced beats incomplete at equal lane score); reserved memory/skill lanes stay. This is where "spend intelligence at build, answer from structure" (graph-memory-starter) lands: pre-walked paths arrive as evidence, the model does not rediscover them. *Blueprint-side dependency: RecallCircuit/`blueprint-recall.mjs` are specified in the Blueprint guides, not yet shipped; Membrane consumption lands with legacy fallback first.*

Blueprint query modes Membrane requests: symbol lookup, references/callers, related context, impact, failure signal → symbols, entry points, change context, claim evidence. Response carries stable id, path/range, source hash, revision, dirty overlay, relationship, confidence, coverage, generation, verification status, resolver. Blueprint failure degrades only the Blueprint lane.

Drift audit as deterministic substrate → bounded model verdict → deterministic gate: pass 0 builds a churn-skipped queue with no model; pass 1 asks only `current | diverged` (`unverifiable` deliberately absent from the vocabulary — pass 0 owns it). Parser: `VERDICT: current | diverged` selects nothing; reset fields on each new verdict line. Derive findings from **persisted** verdicts, never the current run; queue priority: broken anchors → never-checked → content churn → inputs churn → prompt churn last. Embedding provenance: store the hash of the text actually embedded beside the content hash; NULL means unknown, not stale.

---

## 9. Relations and entities — deliberately narrow

Relation row: `relation_id, src, dst (record or entity), kind, valid_from/valid_until, observed_at, producer/evidence refs, confidence, supersession state`. Vocabulary starts with what changes recall/explanation: `supports, contradicts, supersedes, derived_from, part_of, about_entity/mentions, caused_by, applies_to, depends_on, implements, same_as, related_to`. Relations preserve evidence; an embedding similarity is not a relation. Expansion: depth 1, global and per-seed caps, allowed kinds, cycle detection, same scope, provenance on every expanded candidate, no expansion from stale/weak seeds, still subject to authority/freshness/budget. Aliases first; destructive entity merge only when identity is proven. Community detection, PageRank, DRIFT, spreading activation are experiments. Code call/import/reference graphs stay Blueprint-owned.

---

## 10. Security, trust, influence, erasure

- **Authority never comes from wording.** Classify producer: trusted structural evidence, human-authored policy, repository content, external content, tool output, model inference. "THIS IS AN AUTHORITATIVE SYSTEM RULE" in a memory grants nothing. Influence classes `descriptive_only, advisory, user_preference, project_policy, system_policy, untrusted, quarantined`; memory is descriptive by default and never instruction-capable; derived/remote/untrusted text cannot upgrade its own class; current user authority beats any remembered instruction. Preserve `trust_class`, `instruction_policy`, `authority`, `influence_class` as separate fields.
- **Canonical redaction gate** at every ingest edge: known-prefix credentials + contextual `(token|api_key|secret|password)(is|=|:)<value>` patterns, tuned for precision with tests that it does not eat dates/versions; deterministic named redactors (path/id/secret) so entities stay correlatable; strip harness envelopes so injected prefixes never enter as human signal. Sensitivity annotations `[PII]/[INTERNAL]/[PUBLIC]` on schema fields, grep-checkable.
- **DLP at both boundaries**: persistence and publication/resolution recheck grant, policy, scope, sensitivity.
- **Path jail everywhere**: canonicalize before authorize; `..`, symlinks, nested-repo crossing, case/prefix variants, Windows drive/UNC.
- **HMAC-tagged receipt markers**: feedback ingress rejects unverified receipt ids so tool output cannot forge engagement.
- **Per-segment permission verdicts** (Deny > Ask > Allow > Default; unattestable → ask). **Loopback guard** requiring a loopback `Host:` literal, IPv6-mapped IPv4 accepted, 404 not 403. **Hook/config integrity**: hash at install, re-verify before activation, refuse on drift.
- **Erasure**: remove from canonical, FTS, vectors, relations, artifact refs, exports, caches; tombstone keeps identity hash, timestamp, scope, reason, receipt — never payload; in-flight reads cannot republish after commit; `receipt_version` bound into the MAC; backup/restore respects erasure. Crypto-shred only when the threat model requires it.
- **Corruption → quarantine** (`quarantined_at`, `quarantine_reason`), never silent regeneration; corruption-injection tests.
- **RecallCircuit poisoning fixtures are phase-local, not deferred security polish:** README prompt injection, source-comment tool instructions, stale architecture documents claiming authority, and generated files claiming system-message status must remain `data_only`/non-authoritative through Blueprint → candidate → planner → renderer.

---

## 11. Runtime, jobs, storage, operations

- **One deadline, one concurrency primitive.** Results in input order, per-item failure isolation, bounded queue that **accounts for dropped work**, `effectiveConcurrency` + `capped` reported. Per-caller budgets bucket per agent context, not per process.
- **Typed failure codes shared Node↔Rust** via cycle-guarded cause-chain walk: `provider_auth_failed, provider_rate_limited, provider_timeout, provider_unavailable, datastore_unavailable, cancelled, unsupported, stale, permission_denied, policy_changed, corrupt_projection`. Typed instances first. When a classification triggers a destructive action, require conjunctive evidence and default to unknown.
- **Retries** only for transient idempotent ops inside the original deadline; auth errors never; non-idempotent only when failure provably preceded receipt; idempotency keys with body-hash conflict → typed 409; `attempts` persisted on receipts. Circuit breakers only for repeatedly failing resident/external providers.
- **Caps as named constants**, env-overridable, each asserted by a test.
- **Single canonical writer** with lease/heartbeat, typed refusals (`OwnershipRequired, QueueOverloaded, WriteDeadlineExceeded, CommandConflict`), stale-lock clear only by exact operation id, never PID. Two-class durability: `synchronous=NORMAL` for derived writes, `FULL` in a Drop-guarded transaction for authored irreplaceable writes.
- **Crash-safe atomic publication**: I/O and fan-out complete before the write transaction; write-temp → fsync → rename; projections rebuild to a new generation then swap; BUSY-retry policy shared by both planes with a golden fixture.
- **Job/Run model** (`Job, Run, Checkpoint, RunReceipt`; states `queued, running, completed, failed, cancelled, interrupted`; fields job/run id, kind, status, timestamps, checkpoint, progress, items scanned/changed, error, cancellation, receipt) that survives the process; list/show/logs/attach/cancel. Substrate under decay, curation, drift audit. Every job: row/time/CPU cap, checkpoint/resume, cancellation, lock/lease, typed receipt, no-op when current. Never blocks the prompt path. *Declared dependency: `docs/plans/orthic/SEAM-CONTRACT.md` (parent workspace) for watcher lifecycle/scheduling and adapter lifecycle; nothing here seals a seam.*
- **Storage**: one canonical store resolver/identity shared by runtime, installer, Hub, doctor, backup; one writer/maintenance owner; readers never migrate/checkpoint. Operations, all tested: read-only inventory (never creates DBs), doctor split into inspect + frozen-allowlist repair with `PRESENT_BUT_UNLOADABLE` status, live backup + verify + clean-machine restore drill, deterministic export/import with schema/version, wipe scope/all, rebuild FTS/vectors/relations, migration preflight/backout, WAL/checkpoint diagnostics, copy→rebuild→verify→atomic-adopt→rollback for destructive maintenance. Team sync stays event/op-based and subordinate to local correctness; no P2P before one-machine durability, conflict semantics, and erasure propagation are proven.
- **Hub owns OS lifecycle.** Membrane exposes artifact identity, readiness/health, drain/shutdown, process identity, storage identity, lifecycle protocol version. No competing daemon manager.
- **Surface**: one operation registry emitting frozen tool/parameter catalogs, `plugin.json` `contracts.tools[]`, install-time manifest ≡ live surface check; a token budget on the tool surface pinned by golden baseline; effect class (`read/write/execute/network/destructive`) declared at registration, misuse a startup error, authorization carried in the tool; result envelopes with tagged error codes + `structuredContent` + compact model-facing projection; one plugin core with N thin host reflection directories (host list as enum); verb-shaped skills; complete `server.json` publication (+ `smithery.yaml`, `glama.json`, `llms-install.md`) — the gap is publication, not authorship; forward-compatible receipt kinds preserved verbatim.

---

## 12. Observability and evaluation are the promotion system

### 12.1 Phase-0 frozen fixture corpus (`tests/context-quality/` or nearest existing surface)

~20+ cases: exact symbol/path/error lookup; conceptual lookup; lexical-only (no embeddings); semantic-only; anchor preservation; current source vs stale memory; dirty overlay vs committed; superseded decision; cross-file/route→handler→schema/test↔impl; durable preference/procedure; negative memory; contradictory memory; temporal as-of; provider timeout/partial; duplicate across providers; oversized result needing reduction; resolver-backed source; no-relevant-context; secret-bearing source; cross-scope isolation; revoked/deleted source; document heading/section/superseded/cross-ref/unchanged-section; taste conflicts and scope; gotcha trigger present/absent/obsolete. Each records required evidence, **forbidden** evidence, authority ordering, token ceiling, expected degradation, expected receipt properties.

### 12.2 Metrics tracked separately, never one number

Recall@K, precision@K, MRR/nDCG, required-evidence recall, forbidden/stale-evidence admission, contradiction miss rate, temporal accuracy, source-resolution success, explicit-anchor survival (**100%**), scope/ACL violation (**0**), budget reconciliation failure (**0**), transform corruption (**0**), receipt completeness, delivered tokens/chars, bytes externalized/avoided, resolver/refetch rate, compaction fidelity, p50/p95/p99, CPU/RSS, DB/index growth, degradation correctness, deterministic replay variance, whole-task success.

### 12.3 Discipline

- Guarantee suite runs against the **installed artifact** (npm package + built binary), zero mocks — a written-but-unwired surface structurally cannot pass. Covers erasure completeness, receipt reconciliation atomicity, migration/downgrade, multitenant isolation of `POST /federate`.
- Two-layer eval: free deterministic markers (schema, lane budgets, tokens, bytes, prefix stability, byte-identical "faithful" passthrough on hostile-unicode fixtures) gate any paid judging.
- Condition-isolated benchmarks: scratch dir, injected-context manifest, scrubbed env; append-only JSONL provider outputs for zero-cost replay.
- Local traces rich; exported telemetry content-free. Reason on every decision, including admissions and defaults. Confidence paired with tier.
- Ablations: lexical, vectors, relations, outcome modifier, diversity, lifecycle thresholds, each transform vs raw. Every adaptive policy has version, control, candidate, rollback. Never move thresholds to make a candidate pass.
- LoCoMo / LongMemEval / BEAM / commit-reveal whole-task / poisoning / drift / session resume / recovery / Mac+Windows resource suites against current artifacts. Competitor claims stay vendor-reported until reproduced.
- **Promotion law:** nothing adaptive ships because it sounds intelligent; it ships because it meets or beats the deterministic control under a frozen quality/safety/latency/resource gate.

Four proofs per capability: source (tests pass), integration (real request path consumes it), behavior (frozen task shows the effect), operational (installed artifact under realistic resource/failure conditions).

---

## 13. Implementation sequence

Security and observability are cross-cutting from Phase 0; their dedicated phases close full contracts. Each phase: additive migration, rollback, explicit exit gate. Before creating any new module, check whether the repo already owns the concern under another name.

| Phase | Goal | Do | Gate |
|---|---|---|---|
| **0 Authority + baseline** | stop churn; make everything comparable | adopt this guide; supersede Aug-12 plan; regenerate `docs/MEMBRANE-CURRENT-STATE-MANIFEST.json` from source; freeze V1 goldens and packet/order/omission/grant/budget behavior; build fixture corpus + latency/RSS/token baseline; freeze experimental flags; inventory existing modules; typed request-context carrier (request/trace/session/task/repo/worktree/provider ids); caps as tested constants | reproducible baseline; security/anchor/budget regressions fail the build |
| **1 Planner/publication/runtime seams** | one policy path, honest degradation | audit duplicate eligibility/authority/budget decisions across Python providers, runtime, MCP, Rust; providers stop at typed candidates; one deadline through fan-out; grant/policy epoch + publication fence; independent provider degradation; typed error codes shared Node↔Rust; single canonical writer; Hub readiness/drain/identity; **ContextPlan** (`context_plan.py` + gateway subset execution) shadow → low-risk classes, `_collect_tasks_bounded()` untouched in the same patch; sufficiency evaluator + generic stage priority as P1 | same inputs → same decisions across entry paths; revoke-during-publication cannot emit stale bytes; fewer provider calls on low-risk tasks with zero correctness regression |
| **2 Evidence identity + records + sidecar** | truth substrate | `record.rs`, `ranking_signals.rs`, `conflict.rs`; separate identities; normalized evidence tables; backfill `legacy_unattributed`; preserve temporal facts/IDs; V1 stable | any record explains identity, origin, resolution, authority, derivation, supersession without mutable fields |
| **3 Admission + lifecycle** | memory lean by construction | `admission.rs`, `lifecycle.rs`; write dispositions incl. no-op; negative knowledge; Dream → reversible proposal stage with deterministic guards; bounded verified reinforcement; taste/gotcha semantics; held-out calibration | duplicates no-op; conflicts preserve evidence; transitions versioned/reversible; no derived summary becomes truth |
| **4 FTS5/BM25 + explainable fusion** | fix the largest verified retrieval gap | `lexical.rs`, `retrieval_trace.rs`; channel registry; overfetch ≠ final K; RRF baseline kept; exact pinning; post-fusion diversity; two-phase fill in `context-renderer-lib.cjs`; ablation/fallback tests | lexical quality up on frozen cases; vector-off fallback deterministic; stale evidence cannot rank up via similarity; p95/RSS in gate |
| **5 Blueprint anchors + drift** | verifiable code claims, no duplicate Blueprint | **RecallCircuit → atomic path candidates** in `blueprint.py` with legacy fallback; `repo_code_circuit` priority + bounded circuit-quality tie-break in `planner.rs`; anchor/resolution state; consume stable ids + fingerprints; move/rename/ambiguous/missing; bind generation; query modes; embedding provenance column | multi-hop fixtures need fewer model tool calls; generation mismatch fails closed; deterministic resolution states; no Membrane parser |
| **6a Layout + working set** (may run with 6) | position-aware delivery | layout v2 behind flag in `context-renderer-lib.cjs`; working-set classes in JS/Rust twins with schema bump + digest fixtures | flag-off bytes unchanged; parity exact; answer-quality non-regression before graduation |
| **6 Artifact-backed Push** | loss-bounded, recoverable reduction | `artifact.rs`, `context_edit.rs`; converge `runc`/skel/compress/truncate; externalize before lossy; protected/atomic spans; query-critical restore; `TokenBalanceV1`; representation planner; deterministic prefix; citation-by-construction | no protected corruption; raw resolvable; reduction measured at non-inferior evidence quality |
| **7 Relations + aliases** | recall-aware relations, no graph platform | `relations.rs`; evidence on edges; depth-1 bounded expansion; alias canonicalization | scoped, capped, cycle-safe, disable-able |
| **8 Sessions + curation** | strengthen useful, retire stale, no guesswork | session packets; working capacity/expiry; hysteretic retention; Job/Run model; scheduled idempotent maintenance; curation with undo; offline session mining (proposal-only) | resume improves without transcript duplication; no oscillation; maintenance never blocks prompts |
| **9 DLP, influence, erasure** | close trust boundaries | DLP at both boundaries; producer-based authority; path-jail tests; publication revalidation; erasure fence; tombstones without payload; HMAC receipt markers; hook integrity | zero cross-scope leaks; erased content cannot reappear |
| **10 Storage + operations** | operationally trustworthy | store resolver/identity; doctor inspect/repair split; **human-readable knowledge inspection/read-model API for Hub**; read-only inventory; projection rebuild; backup/restore drill; export/import; wipe; migration preflight/backout; crash-boundary tests; sync stays event-based | backup/restore preserves logical keys, lineage, recall equivalence; inspection explains source/evidence/authority/lifecycle/why-retained; corruption is typed, never "delete the DB" |
| **11 Gated experiments** | intelligence only where measured gap | in order: retrieve/no-retrieve gate; rule-based query expansion; MMR; relation variants; local cross-encoder in shadow (pure provider, sub-second deadline, kill-switch off); LLM extraction/expansion/reflection; staged document/image/audio/video; multimodal embeddings last | frozen holdout non-inferiority; latency/RSS/token budget; auto-rollback |
| **12 Closed loop + qualification** | prove what ships | candidate journey + verified outcomes; calibration by task class/family/provider; ablation cohorts; all suites; installed Mac+Windows 10/10 matrix (install→discovery→tools→grant→context→resolve→proposal→feedback→checkpoint→restart/degrade→upgrade→uninstall); **evidence-gated adapter serialization/capability-cache hardening only if race/reconnect tests reproduce the need**; real p50/p95/RSS/token/index data; competitor claims from receipts only | supported paths pass; no new external surface before qualification; adapter Bet promotes only on reproduced failure + measured fix |

---

## 14. File-level map

| Path | State | Change |
|---|---|---|
| `engine/crates/membrane-protocol/src/types.rs` | existing | preserve five V1 shapes; version deliberately |
| `engine/crates/membrane-core/src/{fusion,lane,budget,reconcile}.rs` | existing | authority/freshness discipline; artifact results via existing lanes; protected/resolver economics under one budget; new omission/transform states |
| `engine/crates/crypt-core/src/retriever.rs` | existing | stage exact/lexical/vector/temporal/relation/working; keep RRF/fallback |
| `engine/crates/crypt-core/src/{lexical,retrieval_trace,record,ranking_signals,conflict,lifecycle,admission}.rs` | **new** | FTS5 adapter; per-channel trace; canonical record; sidecar; dispositions; pure retention; admission pipeline |
| `engine/crates/crypt-core/src/{dream,effectiveness,calibration,graph,eval_gate,planner,embed}.rs` | existing | reversible Dream; sidecar-bound outcomes; held-out calibration; bounded relation projection; regression gate |
| `engine/crates/crypt-core/src/relations.rs` | new only if `graph.rs` cannot own typed policy | small relation vocabulary |
| `engine/crates/crypt-store/src/{memdb,temporal,scope,context_telemetry,maintenance_exec,installation_identity,team_sync,db}.rs` | existing | migrations for record/evidence/signal/FTS/relations; temporal transition table; influence/sensitivity; content-free telemetry; bounded jobs; store identity; sync gated |
| `engine/crates/crypt-store/src/relations.rs` | new if not in `memdb.rs` | durable relation rows |
| `engine/crates/membrane-runtime/src/{admission_policy,compress,compression_provider,delivery_trace_view,checkpoint}.rs` | existing | policy epoch; one transform ladder; receipt-visible; explanation surface; bounded session continuity |
| `engine/crates/membrane-runtime/src/{artifact,context_edit}.rs` | **new** | content-addressed artifacts; externalize/reduce/restore |
| `engine/federation/context_plan.py`, `engine/federation/test_context_plan.py` | **new** | deterministic task/risk/provider/Blueprint policy before fan-out; shadow then low-risk rollout |
| `engine/federation/gateway.py` | existing | execute ContextPlan-selected subset under the existing bounded/deadline machinery |
| `engine/federation/providers/blueprint.py`, `test_blueprint_provider.py` | existing | prefer generation-bound RecallCircuit atomic path candidates; legacy fallback; poisoning/generation/abstention tests |
| `mcp/server.mjs`, `mcp/context-renderer-lib.cjs`, `mcp/authorization.mjs`, `mcp/deadline.mjs`, `mcp/retrieval-contracts.mjs` | existing | thin; two-phase fill; centralized authorization; one deadline; score-neutral capability flags |
| `schemas/context-receipt.v1.schema.json`, `schemas/host-delivery-receipt.v1.schema.json` | existing | ranking trace, rejection reasons, lane latency, `actions[]`, ingress-cap vs budget-drop |
| `docs/MEMBRANE-CURRENT-STATE-MANIFEST.json` | stale | regenerate from source |
| `docs/architecture.md` | generated | regenerate only after source changes |
| `docs/plans/2026-08-12-…` | superseded | header only |
| `tests/context-quality/`, `benchmarks/`, `evidence/runs/*.jsonl` | new/merge | frozen fixtures; installed-artifact guarantees; replayable runs |

---

## 15. Migration strategy

Additive first (side tables, no destructive rewrites of `MemoryEntry`/temporal storage). Preserve stable IDs; backfill explicit `legacy_unattributed`, never manufactured provenance. Shadow/dual-read new paths (`lexical_v2`, `knowledge_record_v2_internal`, `relation_retrieval_v1`, `artifact_externalization_v1`, `lifecycle_policy_v1`: off → shadow → on) until gates pass; one cutover authority; flags are bounded migration tools, not config sprawl. Rollback disables reads/policies but never discards records written under the new schema. Projections always rebuildable. Freeze thresholds during qualification.

---

## 16. Rejected and deferred — do not re-import under new names

| Rejected | Take instead |
|---|---|
| External vector/graph DB, Redis, Postgres, RocksDB, multi-backend matrices | narrow internal interface only when two real implementations exist |
| Duplicating Blueprint (parser, LSP, SCIP, symbol/code graph) | stable anchors, fingerprints, blind-spot reporting via provider contract |
| Agent frameworks, PTYs, autonomous loops, multi-agent runtime, browser automation, LLM router, network-proxy interception, hosted/serverless architecture, prompt optimization, P2P memory federation | mechanisms, not scope |
| Markdown-as-database round-trip | export only; Crypt is typed truth |
| LLM deciding add/update/delete; model-generated executable code | model proposes; deterministic policy decides |
| One global weighted score; per-provider final budgets; mandatory LLM in prompt-critical path; memory self-elevating to authority; full transcript ownership; decorative tiers/states | invariants §1 |
| Observer/observed directional memory | "peer P asserts X"; revisit with team sync |
| Bandit-driven adaptive compression | revisit after cache stability measured |
| Unbounded pools/queues | bounded queue accounting for dropped work |

**Deferred behind a frozen benchmark:** cross-encoder reranker (seam left precisely shaped and empty), LLM query expansion/extraction/reflection, MMR default, HyDE, community/PageRank/DRIFT/spreading activation, multimodal embeddings, PQ vectors before measured pressure, crypto-shred, peer/team federation, automatic self-improvement, streamable-HTTP transports, npm-first channel.

---

## 17. Definition of done

**Planner/Pull** — one planner owns grant/scope, authority/freshness, fusion, budget, publication, omissions, receipts; production BM25/FTS5; six channels degrade independently and explainably; no raw cross-provider score arithmetic; adaptive features only where holdouts prove value; every selected and important rejected candidate explains itself.

**Persist** — content, evidence, temporal state, lifecycle, and mutable signals separated; admission before storage; no-op/conflict/supersede/quarantine/expire/forget/restore/reject first-class; negative knowledge without authority; reversible consolidation with exact parents; sessions continue without transcript-as-truth.

**Push** — one reversible ladder; raw governed and recoverable; task-critical spans survive or restore exactly; transforms carry hashes/fingerprints/savings/failure receipts; savings measured against evidence quality.

**Blueprint boundary** — Blueprint supplies identities/ranges/relations/impact/generation; Membrane does scope/authority/freshness/admission/resolution policy; moved/ambiguous/missing distinguished mechanically; no second parser.

**Security** — DLP/influence at persist and delivery; text cannot self-authorize; path/root/symlink/case tests green; revoke/delete races cannot publish stale bytes; erased content cannot reappear from any projection.

**Operations** — Hub owns lifecycle; prompts start no heavy jobs; jobs bounded/cancellable/idempotent/crash-safe; unambiguous storage identity; doctor/backup/export/import/restore/wipe tested; corruption is a typed repairable state.

**Evidence** — Phase-0 baseline checked in and reproducible; four proofs per capability; LoCoMo/LongMemEval/BEAM/whole-task/stateful/security/recovery suites against current artifacts; Mac + Windows installed paths qualified; real p50/p95/RSS/token/index numbers; competitor claims labeled until reproduced; **one** active implementation authority.

Every durable item can answer: what am I, where did I come from, what supports me, whose scope, how authoritative, when observed, when valid, what replaced me, what did I derive from, what state am I in. Every packet can answer: what was delivered, what was not, why, what was transformed, and how to recover the exact evidence.

---

## 18. Adoption and execution rule

1. Place **this file only** at `docs/MEMBRANE-IMPLEMENTATION-GUIDE.md` as the planning/implementation authority.
2. Regenerate `docs/MEMBRANE-CURRENT-STATE-MANIFEST.json` from the current tree before writing implementation tickets; if `main` moved, record the new baseline in the Phase-0 evidence run rather than silently assuming this snapshot is current.
3. Freeze V1 goldens, context-quality fixtures, resource baselines, and experimental flags.
4. Execute Phases 0→12 under their gates. Use Appendix A for the file-exact ContextPlan/RecallCircuit/layout slice.
5. Treat Appendix B confidence marks as implementation policy: Floor = do not relitigate; Candidate = verify mechanism; Bet = flag + benchmark; Hold = do not re-import.
6. Use Appendix C as the embedded 60-repository coverage proof. Earlier planning/absorption documents may be archived or deleted without losing implementation authority or absorption coverage.
7. Regenerate `docs/architecture.md` only through its generator after source changes land.
8. This book is done only when §17 and the Appendix-A P0/P1 gates are closed, or an explicitly versioned successor replaces it.

> **The strongest context system is not the one with the most memory types, graph algorithms, or backends. It is the one that preserves exact evidence, retrieves the smallest current authoritative subset, reduces everything else reversibly, keeps derived text distinct from truth, recovers from failure without loss, and proves every inclusion, omission, transformation, lifecycle change, and degradation under the user's current authority.**

---

## Appendix A — File-exact P0/P1 execution specification

This appendix is **normative implementation detail**, not a companion plan. It was originally derived against `e640aaa7`; current `175b47e1` is its direct child and changes only Right Release package references, so the architecture/file slice remains applicable. Phase 0 still re-verifies every referenced path before a ticket is written. Where this appendix and the main book differ, the main book's invariants and stage order win.

### 2. Exact P0 file change set

| Action | File | Change |
|---|---|---|
| ADD | `engine/federation/context_plan.py` | Deterministic provider/query plan before fan-out |
| MODIFY | `engine/federation/gateway.py` | Build provider tasks, then execute only the plan-selected subset |
| MODIFY | `engine/federation/providers/blueprint.py` | Prefer Blueprint RecallCircuit; convert each complete path into one atomic candidate; retain legacy fallback |
| MODIFY | `engine/crates/crypt-core/src/planner.rs` | Recognize `repo_code_circuit`; reward evidence/path completeness without deleting reserved lanes |
| ADD | `engine/federation/test_context_plan.py` | Deterministic planning tests |
| MODIFY | `engine/federation/providers/test_blueprint_provider.py` | RecallCircuit parsing, generation pinning and fallback tests |
| MODIFY | Rust planner tests in `planner.rs` | Atomic circuit ranking/admission tests |
| MODIFY | `mcp/context-renderer-lib.cjs` | Optional layout-v2 ordering: constraints front, evidence middle, dirty/live state late |
| MODIFY | `mcp/context-renderer.test.mjs` | Exact byte-order/layout tests |

#### Explicit P0 non-changes

Do **not** change in P0:

- `schemas/context-candidate-set.v1.schema.json`;
- canonical `CandidateV1` in `engine/crates/membrane-protocol/src/types.rs`;
- the five core Membrane contract shapes;
- Crypt/vector storage;
- Blueprint database or graph;
- working-context schema;
- reserved memory/skill lanes;
- prompt-hook ownership;
- ContextReceipt content-free policy.

A Blueprint path can fit inside the existing candidate contract as an **atomic candidate**, so do not create a protocol migration unless evidence requires it.

---

### 3. ADD `engine/federation/context_plan.py`

P0 planning is deterministic and conservative.

It is intentionally **not** an LLM router.

Create:

```python
from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable
import re


_WORD = re.compile(r"[a-z0-9_./:-]+")


@dataclass(frozen=True)
class BlueprintPlan:
    enabled: bool
    policy_id: str
    max_hops: int
    max_paths: int


@dataclass(frozen=True)
class ContextPlan:
    schema_version: int
    task_class: str
    risk: str
    providers: tuple[str, ...]
    blueprint: BlueprintPlan


def _terms(task: str) -> set[str]:
    return set(_WORD.findall((task or "").lower()))


def _has_any(text: str, needles: Iterable[str]) -> bool:
    lower = text.lower()
    return any(needle in lower for needle in needles)


def build_context_plan(
    task: str,
    *,
    blueprint_usable: bool,
    live_usable: bool,
    skills_usable: bool,
) -> ContextPlan:
    text = task or ""
    terms = _terms(text)

    security = _has_any(text, (
        "security", "auth", "authorization", "permission", "credential",
        "secret", "token", "vulnerability", "taint", "trust boundary",
    ))
    migration = _has_any(text, (
        "migration", "migrate", "schema change", "database change",
        "replace storage", "move from", "upgrade protocol",
    ))
    architecture = _has_any(text, (
        "architecture", "architect", "boundary", "component",
        "dependency", "coupling", "design", "interface",
    ))
    impact = _has_any(text, (
        "what breaks", "blast radius", "impact", "depends on",
        "dependency", "callers", "consumers",
    ))
    debug = _has_any(text, (
        "bug", "debug", "failing", "failure", "crash", "regression",
        "exception", "incorrect", "root cause",
    ))
    docs = _has_any(text, (
        "readme", "docs", "documentation", "document",
    ))
    local_edit = (
        _has_any(text, ("rename", "typo", "format", "comment", "small edit"))
        and not (security or migration or architecture or impact or debug)
    )

    if security:
        task_class, risk = "security", "high"
        policy, hops, paths = "impact.reverse", 5, 24
    elif migration:
        task_class, risk = "migration", "high"
        policy, hops, paths = "impact.reverse", 5, 24
    elif architecture:
        task_class, risk = "architecture", "high"
        policy, hops, paths = "architecture.boundary", 4, 24
    elif impact:
        task_class, risk = "impact", "medium"
        policy, hops, paths = "impact.reverse", 4, 20
    elif debug:
        task_class, risk = "debug", "medium"
        policy, hops, paths = "dependency.forward", 4, 20
    elif local_edit:
        task_class, risk = "local_edit", "low"
        policy, hops, paths = "explore.both", 2, 8
    elif docs:
        task_class, risk = "docs", "low"
        policy, hops, paths = "explore.both", 2, 8
    else:
        task_class, risk = "general", "medium"
        policy, hops, paths = "explore.both", 3, 16

    providers: list[str] = []

    # Always-admissible identity/policy lanes.
    providers.extend(("rules", "anchors", "git"))

    if live_usable:
        providers.append("live")

    if blueprint_usable and task_class != "docs":
        providers.append("blueprint")

    if task_class in {"security", "migration", "architecture"}:
        providers.extend(("audit", "architect"))
        if skills_usable:
            providers.append("skills")
        providers.append("crypt")

    elif task_class == "debug":
        providers.append("audit")
        if skills_usable:
            providers.append("skills")

    elif task_class == "impact":
        if skills_usable:
            providers.append("skills")

    elif task_class == "general":
        # Conservative default: preserve today's broad coverage for ambiguous
        # work until retrieval evaluation proves a narrower plan is safe.
        providers.extend(("audit", "architect", "crypt"))
        if skills_usable:
            providers.append("skills")

    # Low-risk local edits and docs deliberately skip expensive advisory/history
    # lanes unless the task text actually classifies into a stronger class.
    providers = list(dict.fromkeys(providers))

    return ContextPlan(
        schema_version=1,
        task_class=task_class,
        risk=risk,
        providers=tuple(providers),
        blueprint=BlueprintPlan(
            enabled=blueprint_usable and "blueprint" in providers,
            policy_id=policy,
            max_hops=hops,
            max_paths=paths,
        ),
    )
```

#### Why P0 is conservative

Do not "optimize" by skipping providers on ambiguous requests.

P0 narrows only obvious low-risk cases.

For `general`, preserve today's broad provider coverage.

This makes the rollout falsifiable and reversible.

---

### 4. MODIFY `engine/federation/gateway.py`

#### 4.1 Import the planner

Add:

```python
from federation.context_plan import build_context_plan
```

#### 4.2 Build all available task factories exactly once

Keep the existing provider adapters and freshness checks.

Replace the current direct `tasks = [...]` construction with:

```python
all_tasks: dict[str, Any] = {
    "audit": lambda: _adapter("audit", audit.produce, repo_root, task),
    "architect": lambda: _adapter("architect", architect.produce, repo_root, task),
    "crypt": lambda: _adapter(
        "crypt",
        crypt.produce_with_observability,
        repo_root,
        task,
        scope_grant_id,
        scope_descriptor,
    ),
    "git": lambda: _adapter("git", git_provider.produce, repo_root),
    "rules": lambda: _adapter("rules", rules.produce, repo_root, task, client),
    "anchors": lambda: _adapter(
        "anchors",
        anchors.produce,
        repo_root,
        explicit_anchors,
        task,
    ),
}
```

Then conditionally add live/skills/blueprint:

```python
blueprint_usable = bool(blueprint_state.get("usable"))
live_usable = bool(overlay_state.get("usable"))
skills_usable = bool(skills_state.get("usable"))

plan = build_context_plan(
    task,
    blueprint_usable=blueprint_usable,
    live_usable=live_usable,
    skills_usable=skills_usable,
)

if blueprint_usable:
    all_tasks["blueprint"] = lambda: _adapter(
        "blueprint",
        blueprint.produce_with_observability,
        repo_root,
        task,
        max_tokens,
        expected_generation=expected_blueprint_generation,
        policy_id=plan.blueprint.policy_id,
        max_hops=plan.blueprint.max_hops,
        max_paths=plan.blueprint.max_paths,
    )

if live_usable:
    all_tasks["live"] = lambda: _adapter(
        "live",
        live.produce,
        repo_root,
        base_commit=freshness.get("baseCommit"),
        overlay_digest=freshness.get("overlayDigest"),
        overlay_entries=verdict.get("overlayEntries") or [],
        prompt_fast=True,
    )

if skills_usable:
    all_tasks["skills"] = lambda: _adapter(
        "skills",
        skills.produce,
        repo_root,
        task,
        scope_grant_id,
    )

tasks = [
    (name, all_tasks[name])
    for name in plan.providers
    if name in all_tasks
]
```

#### 4.3 Keep current bounded executor in P0

Do **not** rewrite `_collect_tasks_bounded()` in the same patch.

It already:

- enforces one absolute deadline;
- isolates provider failures;
- gives Blueprint special scheduling because it is a structural dependency;
- emits typed timeout warnings.

Changing routing and concurrency semantics simultaneously would make regressions harder to attribute.

P1 may replace the Blueprint special-case with generic stages after ContextPlan is qualified.

#### 4.4 Do not put raw task text into receipts

If ContextPlan observability is emitted, expose only:

```json
{
  "schemaVersion": 1,
  "taskClass": "impact",
  "risk": "medium",
  "providers": ["rules", "anchors", "git", "live", "blueprint", "skills"],
  "blueprintPolicyId": "impact.reverse"
}
```

Do not duplicate `task` content into telemetry/receipt surfaces.

---

### 5. MODIFY `engine/federation/providers/blueprint.py`

The provider currently normalizes Blueprint's flat candidates. Change it to prefer `RecallCircuitV1`.

#### 5.1 Function signature

Extend the public provider path:

```python
def produce_with_observability(
    repo_root: Path,
    task: str,
    max_tokens: int,
    *,
    expected_generation: str,
    policy_id: str = "explore.both",
    max_hops: int = 3,
    max_paths: int = 16,
):
```

Apply the same optional args through the internal `_produce(...)` path.

#### 5.2 Cache key

Current cache identity already includes task/cap/generation.

Add:

```text
policy_id
max_hops
max_paths
```

A result produced under `explore.both` must never satisfy an `impact.reverse` cache lookup.

#### 5.3 Prefer the lean RecallCircuit script

Derive:

```python
recall_cli = Path(blueprint_cli).with_name("blueprint-recall.mjs")
```

If it exists, invoke:

```python
cmd = [
    node,
    str(recall_cli),
    "--root", str(repo_root),
    "--task", task,
    "--policy", policy_id,
    "--max-hops", str(max_hops),
    "--max-paths", str(max_paths),
    "--expected-generation", expected_generation,
]
```

If it does not exist, use the existing `blueprint-candidates.mjs` flow unchanged.

This gives version-skew compatibility.

#### 5.4 Validate before using

Require:

```python
document["schemaVersion"] == 1
document["generationId"] == expected_generation
isinstance(document["paths"], list)
isinstance(document["nodes"], list)
isinstance(document["edges"], list)
```

On mismatch:

- return no Blueprint candidate;
- emit a typed warning;
- do not silently reinterpret it as legacy candidate output.

#### 5.5 Convert one path into one atomic candidate

Add helpers:

```python
def _sha256_json(value: Any) -> str:
    encoded = json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode("utf-8")
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def _node_label(node: dict[str, Any]) -> str:
    return str(
        node.get("qualifiedName")
        or node.get("name")
        or node.get("path")
        or node.get("id")
        or "unknown"
    )


def _render_circuit_path(
    path: dict[str, Any],
    nodes_by_id: dict[str, dict[str, Any]],
    edges_by_id: dict[str, dict[str, Any]],
) -> str:
    node_ids = list(path.get("nodeIds") or [])
    edge_ids = list(path.get("edgeIds") or [])
    if not node_ids:
        return ""

    parts = [_node_label(nodes_by_id.get(node_ids[0], {}))]
    for index, edge_id in enumerate(edge_ids):
        edge = edges_by_id.get(edge_id, {})
        kind = str(edge.get("kind") or "RELATES_TO")
        target = node_ids[index + 1] if index + 1 < len(node_ids) else "unknown"
        parts.append(f"--[{kind}]--> {_node_label(nodes_by_id.get(target, {}))}")

    evidence_refs: list[str] = []
    for edge_id in edge_ids:
        for ev in edges_by_id.get(edge_id, {}).get("evidence") or []:
            p = ev.get("path")
            if not p:
                continue
            start = ev.get("startLine")
            end = ev.get("endLine")
            ref = str(p)
            if start:
                ref += f":{start}"
                if end and end != start:
                    ref += f"-{end}"
            evidence_refs.append(ref)

    text = " ".join(parts)
    if evidence_refs:
        text += "\nEvidence: " + ", ".join(dict.fromkeys(evidence_refs))
    return text
```

Then:

```python
def _circuit_candidates(
    document: dict[str, Any],
    *,
    expected_generation: str,
) -> list[dict[str, Any]]:
    nodes_by_id = {
        str(node.get("id")): node
        for node in document.get("nodes") or []
        if node.get("id")
    }
    edges_by_id = {
        str(edge.get("id")): edge
        for edge in document.get("edges") or []
        if edge.get("id")
    }

    candidates: list[dict[str, Any]] = []

    for path in document.get("paths") or []:
        if not path.get("complete"):
            continue

        text = _render_circuit_path(path, nodes_by_id, edges_by_id)
        if not text:
            continue

        seed_id = str(path.get("seedId") or "")
        terminal_id = str(path.get("terminalId") or "")
        path_id = str(path.get("id") or "")
        descriptor = {
            "generationId": expected_generation,
            "pathId": path_id,
            "seedId": seed_id,
            "terminalId": terminal_id,
            "nodeIds": list(path.get("nodeIds") or []),
            "edgeIds": list(path.get("edgeIds") or []),
        }

        score = float(path.get("score") or 0.0)
        evidence_coverage = float(path.get("evidenceCoverage") or 0.0)

        candidates.append({
            "id": f"blueprint-circuit:{document['circuitId']}:{path_id}",
            "layer": 3,
            "provider": "blueprint",
            "sourceKind": "repo_code_circuit",
            "sourceRef": (
                f"blueprint://circuit/{document['circuitId']}/{path_id}"
            ),
            "sourceHash": _sha256_json(descriptor),
            "trustClass": "workspace_tracked",
            "instructionPolicy": "data_only",
            "providerScore": max(0.0, min(1.0, score)),
            "scoreComponents": {
                "path_complete": 1.0,
                "evidence_complete": 1.0
                    if path.get("evidenceComplete") else 0.0,
                "evidence_coverage": max(
                    0.0, min(1.0, evidence_coverage)
                ),
                "hop_efficiency": 1.0 / (
                    1.0 + len(path.get("edgeIds") or [])
                ),
            },
            "freshnessClass": "current",
            "estimatedTokens": max(
                1,
                (len(text.encode("utf-8")) + 3) // 4,
            ),
            "protected": False,
            "exact": bool(
                path.get("complete")
                and path.get("evidenceComplete")
            ),
            "recoverable": True,
            "resolver": (
                f"blueprint graph path {seed_id} {terminal_id}"
            ),
            "text": text,
        })

    return candidates
```

#### Why the whole path is one candidate

Do not let Membrane admit:

```text
A
B
C
```

independently when the evidence is:

```text
A -> B -> C
```

The path is the semantic unit.

This avoids top-k admission splitting a required chain.

#### 5.6 Empty circuit behavior

If Blueprint returns:

```json
{
  "paths": [],
  "unresolved": [{"reason": "no_relevant_seed"}]
}
```

emit no Blueprint candidate.

Do not convert the unresolved state into generic repository text.

Preserve the current loud abstention/warning semantics.

---

### 6. MODIFY `engine/crates/crypt-core/src/planner.rs`

P0 does **not** replace the planner.

It improves how complete graph evidence is treated.

#### 6.1 Add circuit source-kind priority

Current `kind_priority()` starts:

```rust
"repo_code" | "repo_code_overlay" => 0,
```

Change to:

```rust
"repo_code" | "repo_code_overlay" | "repo_code_circuit" => 0,
```

#### 6.2 Add deterministic circuit-quality helpers

Add near `freshness_component()` / `kind_priority()`:

```rust
fn score_component(cand: &CandidateV1, key: &str) -> f64 {
    cand.score_components
        .get(key)
        .copied()
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

fn circuit_quality(cand: &CandidateV1) -> f64 {
    if cand.source_kind != "repo_code_circuit" {
        return 0.0;
    }
    let complete = score_component(cand, "path_complete");
    let evidence = score_component(cand, "evidence_coverage");
    let hop_efficiency = score_component(cand, "hop_efficiency");

    (complete * 0.45 + evidence * 0.45 + hop_efficiency * 0.10)
        .clamp(0.0, 1.0)
}
```

#### 6.3 Extend the deterministic sort

Current order is approximately:

```text
protected
provider_score
freshness
kind_priority
exact
id
```

Change to:

```rust
deduped.sort_by(|a, b| {
    let af = freshness_component(a);
    let bf = freshness_component(b);
    let aq = circuit_quality(a);
    let bq = circuit_quality(b);
    let ak = kind_priority(a);
    let bk = kind_priority(b);

    b.protected
        .cmp(&a.protected)
        .then_with(|| {
            bq.partial_cmp(&aq)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| {
            b.provider_score
                .partial_cmp(&a.provider_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| {
            bf.partial_cmp(&af)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then(ak.cmp(&bk))
        .then(b.exact.cmp(&a.exact))
        .then(a.id.cmp(&b.id))
});
```

#### 6.4 Do not delete reserved lanes

Keep:

```rust
const RESERVED_LANES: &[(&str, usize)] =
    &[("memory", 800), ("skill", 300)];
```

and the Git identity / repo-code protections.

Reason: current provider scores are explicitly not calibrated as one cross-provider probability.

A new paper does not justify deleting a working anti-starvation policy.

P1 can replace lanes only after calibration data shows a better global policy.

#### 6.5 Add tests

Add tests to the existing Rust module:

### Complete circuit beats incomplete circuit at equal lane score

```rust
#[test]
fn complete_blueprint_circuit_beats_incomplete_peer() {
    let mut complete = candidate(
        "circuit:complete",
        "repo_code_circuit",
        80,
        0.8,
        false,
    );
    complete.provider = Some("blueprint".into());
    complete.score_components.insert("path_complete".into(), 1.0);
    complete.score_components.insert("evidence_coverage".into(), 1.0);
    complete.score_components.insert("hop_efficiency".into(), 0.5);

    let mut incomplete = candidate(
        "circuit:partial",
        "repo_code_circuit",
        80,
        0.8,
        false,
    );
    incomplete.provider = Some("blueprint".into());
    incomplete.score_components.insert("path_complete".into(), 0.0);
    incomplete.score_components.insert("evidence_coverage".into(), 0.5);

    let out = plan(&empty_planner_input(
        vec![incomplete, complete],
    )).unwrap();

    assert_eq!(
        out.packet.blocks.first().unwrap().id,
        "circuit:complete"
    );
}
```

### Circuit stays atomic

Assert one path candidate creates exactly one admitted block; planner never splits node/edge components because they are not separate candidates.

### Reserved lanes remain

Existing memory/skill lane tests must continue passing.

---

### 7. MODIFY `mcp/context-renderer-lib.cjs` — layout v2

The renderer currently sorts blocks primarily by descending priority.

That is deterministic, but it ignores context-position effects.

Do not introduce a protocol field in P0. Use existing provider/source information.

#### 7.1 Add placement helper

```js
function contextPlacementRank(block) {
  const provider = String(block?.provider || "");
  const sourceKind = String(block?.sourceKind || block?.source_kind || "");

  // Front: hard constraints / policy / pinned anchors.
  if (
    provider === "rules" ||
    sourceKind === "rule" ||
    sourceKind === "policy" ||
    block?.protected === true
  ) {
    return 0;
  }

  // Late: dirty/live state should sit near the active task boundary.
  if (
    provider === "live" ||
    sourceKind === "repo_code_overlay" ||
    sourceKind === "working_context"
  ) {
    return 2;
  }

  // Middle: evidence, circuits, docs, memory, skills, audit, architecture.
  return 1;
}
```

#### 7.2 Put behind rollout flag first

Inside `finalize()` change the order creation to:

```js
const layoutV2 =
  process.env.MEMBRANE_CONTEXT_LAYOUT_V2 === "1";

const order = blocks
  .map((block, index) => ({ block, index }))
  .sort((left, right) => {
    if (layoutV2) {
      const placement =
        contextPlacementRank(left.block) -
        contextPlacementRank(right.block);
      if (placement !== 0) return placement;
    }

    return (
      Number(right.block.priority || 0) -
        Number(left.block.priority || 0) ||
      left.index - right.index
    );
  });
```

#### 7.3 Required tests

`mcp/context-renderer.test.mjs`:

- rules precede evidence under layout v2;
- Blueprint circuit is in evidence middle;
- live/dirty overlay follows ordinary evidence;
- same packet produces byte-identical output across repeated runs;
- layout v1 remains unchanged when flag off;
- char-budget accounting remains exact;
- renderer/Forge parity test remains exact.

Do not graduate the flag until answer-quality evaluation shows non-regression.

---

### 8. Do not change protocol v1 in P0

`ContextCandidateSetV1.candidate` is closed (`additionalProperties:false`) and the Rust type is `deny_unknown_fields`.

Do not casually add:

```text
groupId
workingSetClass
placementClass
pathIds
```

to only one language/schema.

For P0, encode the path as one atomic candidate using existing fields:

- `id`
- `sourceKind`
- `sourceRef`
- `sourceHash`
- `providerScore`
- numeric `scoreComponents`
- `resolver`
- `text`

This gets the system benefit without contract churn.

---

### 9. P1 — Retrieval sufficiency evaluator

After static ContextPlan ships, add:

- `engine/federation/retrieval_evaluator.py`
- tests.

Its job is **not** to answer the task.

It decides whether another retrieval stage is justified.

Output:

```json
{
  "schemaVersion": 1,
  "verdict": "sufficient",
  "reasons": [
    "fresh_structural_path",
    "evidence_complete"
  ],
  "missing": []
}
```

Allowed verdicts:

```text
sufficient
insufficient
ambiguous
contradictory
stale
unsafe
provider_failure
```

P1 staged flow:

```text
Stage 0: identity/rules/live
Stage 1: Blueprint structural recall
evaluate
Stage 2: audit/architect/memory/skills only if justified
evaluate
Stage 3: expensive semantic escalation only if still justified
```

#### Stop condition

Do not retrieve more merely because budget remains.

Retrieve more when expected decision value is positive.

P0 can approximate this with deterministic rules; do not add another LLM call to decide whether an LLM should receive more context.

---

### 10. P1 — Working-set classes

Current working-context selection in both:

- `mcp/working-context.mjs`
- `engine/crates/membrane-runtime/src/working_context.rs`

walks candidates in order under `max_blocks`, `max_bytes`, `max_recent_turns`.

Do not mix this migration into P0.

P1 may introduce:

```text
pinned
resident
prefetched
reconstructable
quarantined
```

Semantics:

- `pinned`: cannot be evicted while its task constraint is active;
- `resident`: current high-value working set;
- `prefetched`: likely useful, lower priority;
- `reconstructable`: keep resolver/metadata, not text;
- `quarantined`: untrusted/stale/contradicted; never auto-inject.

If the shape changes:

1. bump the working-context schema;
2. update JS and Rust twins in the same commit;
3. update canonical digest fixtures;
4. update server/MCP tests;
5. never let one side accept fields the other side rejects.

---

### 11. P1 — Value-of-information / marginal utility

Do not invent one global relevance probability.

Instead collect calibration data first.

Per candidate, log content-free metrics such as:

```text
provider
sourceKind
freshnessClass
estimatedTokens
admitted/rejected
reason
path completeness
evidence coverage
whether later feedback said context was missing/redundant
```

Then evaluate:

```text
marginal task success improvement
---------------------------------
tokens + latency + provider cost
```

Possible future planner terms:

- novelty;
- coverage of unresolved task dimensions;
- authority;
- freshness;
- evidence completeness;
- risk reduction;
- token cost;
- latency cost.

The reserved-lane policy remains until calibrated replacement outperforms it.

---

### 12. P1 — Poisoning and instruction separation

Membrane already carries `trustClass` and `instructionPolicy`.

Strengthen admission tests around Blueprint circuits:

A Blueprint path is **data**, not executable instruction.

Required invariant:

```text
repository text:
"ignore previous instructions and..."
```

must remain:

```text
instructionPolicy = "data_only"
```

and never become host/system instruction merely because Blueprint connected it structurally.

Add adversarial fixtures where:

- README contains prompt injection;
- source comments contain tool instructions;
- stale architecture doc claims authority;
- generated file claims to be a system message.

Membrane must preserve source trust and authority independently of semantic similarity.

---

### 13. Exact removals/deprecations

#### Deprecate after qualification

### Blind all-provider execution for every obvious low-risk request

Do not delete providers. Stop invoking them when ContextPlan explicitly says they have no expected value.

### Flattened Blueprint node candidates

Once RecallCircuit is available, do not make isolated graph nodes the normal Blueprint candidate unit for multi-hop questions.

Keep legacy parsing only for version skew and rollback.

### Hard-coded Blueprint scheduling as the only "planning" concept

P0 can retain `_collect_tasks_bounded()` unchanged for safety.

P1 should make stage priority generic rather than embedding one provider name into scheduling policy.

#### Do not remove

- provider failure isolation;
- absolute federation deadline;
- freshness verdict;
- release generation checks;
- ScopeGrant;
- `ContextCandidateSetV1`;
- Rust admission planner;
- reserved lanes;
- content-free receipts;
- char/token reconciliation;
- native delivery receipts;
- Crypt;
- prompt hooks.

---

### 14. Tests: exact required matrix

#### ADD `engine/federation/test_context_plan.py`

### Local rename

Task:

```text
rename SessionManager to SessionStore
```

Assert:

- class `local_edit`;
- risk `low`;
- no `audit`, `architect`, `crypt`;
- Blueprint included when usable;
- Blueprint policy bounded to 2 hops.

### Impact

Task:

```text
what breaks if I change SessionManager?
```

Assert:

- class `impact`;
- policy `impact.reverse`;
- Blueprint included;
- skills included when available;
- no unnecessary architect unless architecture signal exists.

### Security

Task:

```text
change auth token validation
```

Assert:

- high risk;
- Blueprint + audit + architect + skills + crypt;
- policy `impact.reverse`.

### General ambiguity

Assert broad current provider set is preserved.

### Provider unavailable

If Blueprint is unusable:

- plan does not include it;
- caller still receives non-Blueprint providers;
- no fake graph candidate is created.

---

#### MODIFY `engine/federation/providers/test_blueprint_provider.py`

Required new cases:

1. reads RecallCircuit v1;
2. rejects generation mismatch;
3. falls back to legacy candidate script if recall script absent;
4. one complete path => one candidate;
5. path source hash is deterministic;
6. cache key differs by policy/hops/paths;
7. no-seed circuit => zero candidates + typed abstention;
8. prompt-injection text in evidence remains `data_only`;
9. incomplete path is not emitted as exact;
10. evidence coverage reaches planner `scoreComponents`.

---

#### Rust planner tests

Required:

- `repo_code_circuit` has repo-code priority;
- complete/evidenced circuit beats incomplete peer at equal lane score;
- source-hash dedup still works;
- circuit remains one block;
- memory and skills reserved lanes still function;
- global token ceiling still holds.

---

#### Renderer tests

Required:

- flag off == current byte output;
- flag on is deterministic;
- rules front;
- ordinary evidence middle;
- live/dirty state late;
- packet char cap still enforced;
- budget reconciliation remains balanced;
- Forge and Membrane renderer remain byte-identical.

---

### 15. Benchmarks and qualification gates

Do not optimize only latency.

Measure:

#### Retrieval-plan metrics

- providers invoked per task class;
- provider timeout rate;
- provider failure rate;
- candidate count before admission;
- selected tokens;
- delivered tokens;
- missing-context feedback rate.

#### Blueprint-circuit metrics

- path completeness;
- evidence coverage;
- average hops;
- path candidate token cost;
- percentage of multi-hop tasks solved without model-driven search.

#### End-to-end

For each task category:

```text
legacy federation
vs
ContextPlan + RecallCircuit
```

Measure:

- task correctness;
- tool calls after context delivery;
- retrieval wall time;
- total tokens;
- context omissions;
- stale/incorrect evidence;
- model tier sensitivity.

### Graduation criteria

Do not pick arbitrary "5% faster" gates.

P0 graduates when:

1. no correctness regression on current qualification fixtures;
2. fewer unnecessary provider calls on low-risk tasks;
3. graph multi-hop fixtures need fewer model tool calls;
4. no rise in stale/poisoned context admission;
5. receipt/budget invariants remain exact;
6. rollback path is tested.

---

### 16. Rollout sequence

#### Commit M1 — ContextPlan in shadow

- add `context_plan.py`;
- call it;
- record selected provider names in test/shadow diagnostics;
- still execute current full set in production.

Purpose: compare "would run" vs "did run."

#### Commit M2 — activate planning for low-risk classes

Only narrow:

- local edit;
- docs.

Keep `general` broad.

#### Commit M3 — Blueprint RecallCircuit consumption

- prefer new lean script;
- path -> atomic candidate;
- legacy fallback retained.

#### Commit M4 — planner circuit quality

- add `repo_code_circuit`;
- add deterministic quality tie-break;
- keep reserved lanes.

#### Commit M5 — renderer layout v2 shadow

- feature flag only;
- qualification A/B.

#### Commit M6 — broader task classes

After evidence, allow planner narrowing for:

- impact;
- debug;
- architecture;
- migration;
- security.

Do not start here.

---

### 17. Rollback

Every P0 change has a direct rollback.

#### ContextPlan failure

Set planning off and execute current full provider set.

#### RecallCircuit failure/version skew

`blueprint.py` falls back to `blueprint-candidates.mjs`.

#### Planner regression

Remove `repo_code_circuit` tie-break; path candidate still fits existing v1 schema.

#### Renderer regression

Unset:

```text
MEMBRANE_CONTEXT_LAYOUT_V2
```

No stored-data migration is involved.

---

### 18. Definition of Done

P0 is done only when all are true:

- [ ] Membrane remains a separate system.
- [ ] No Blueprint graph/store logic is copied into Membrane.
- [ ] ContextPlan runs before provider execution.
- [ ] Low-risk requests can skip providers with no expected value.
- [ ] Ambiguous general requests preserve broad coverage.
- [ ] Blueprint RecallCircuit is preferred when available.
- [ ] Legacy Blueprint candidates remain a rollback/version-skew fallback.
- [ ] Each Blueprint path is atomic in admission.
- [ ] Generation mismatch fails closed.
- [ ] No-seed circuit creates no fake context.
- [ ] Repository content remains `data_only`.
- [ ] Reserved memory/skill lanes remain intact.
- [ ] Existing token and char ceilings remain exact.
- [ ] Receipts remain content-free.
- [ ] Provider failure isolation remains intact.
- [ ] Layout v2 is feature-flagged until qualified.
- [ ] No v1 protocol fields are added ad hoc.
- [ ] JS/Rust parity tests pass.
- [ ] `pnpm test` passes.
- [ ] `cargo test --workspace --features fastembed` passes.
- [ ] End-to-end qualification shows correctness non-regression.

---


---

## Appendix B — Absorption confidence register

These marks preserve the convergence evidence that would otherwise be lost when the research ledger is retired. They do **not** create a second backlog; each mechanism is implemented only through the canonical sections/phases above.

| # | Mechanism | Evidence class / convergence |
|---:|---|---|
| 1 | Context-quality fixture corpus, frozen baseline, and a regression gate | Floor (8/8)** · `tests/context-quality/` · `crypt-core/src/eval_gate.rs` |
| 2 | Guarantee suite driven from the published artifact, zero mocks | Candidate** · `benchmarks/` against the npm package and built engine binary |
| 3 | Two-layer evaluation — free deterministic markers gate paid judging | Candidate** · `mcp/calibration-harness.test.mjs` |
| 4 | Condition-isolated benchmarking with captured-output replay | Candidate** · `evidence/runs/*.jsonl` |
| 5 | One implementation authority; supersede the rest | Bet** · `docs/plans/2026-08-12-*.md` |
| 6 | Typed request-context carrier in task-local storage | Candidate** · closes **B-3 |
| 7 | Single canonical writer with lease, heartbeat, and operation-id-scoped recovery | Floor (26/60)** · `crypt-store` · `mcp/host/delivery-ledger-store.cjs` |
| 8 | Two-class durability | Bet** · `crypt-store` |
| 9 | Cause-chain error classification into a small closed code set, shared Node↔Rust | Floor** · `membrane-protocol` · `mcp/server.mjs` |
| 10 | Caps as named constants, env-overridable, each asserted by a test | Floor (30/59) |
| 11 | A real sparse lexical index — FTS5/BM25 with identifier-aware tokenization | Floor (4/4 consolidations)** · `crypt-store/src/lexical.rs` (new) |
| 12 | Retrieval channel registry, not an ever-growing ranking function | Floor** · `crypt-core/src/retrieval/` |
| 13 | Deterministic query-intent classification — no model in the hot path | Candidate** · `crypt-core/src/query_intent.rs` (new) |
| 14 | Two-phase budget fill: breadth-first floor placement, then depth upgrades | Floor (4/4 passes)** · `mcp/context-renderer-lib.cjs` |
| 15 | Weighted RRF with per-channel provenance and deterministic tie-breaks | Floor (18/60)** · `crypt-core/src/ranking.rs` |
| 16 | Separate relevance from policy — never one global weighted score | Floor (4/4 consolidations) |
| 17 | Exact-evidence pinning — ordering-only, still vetoable | Candidate |
| 18 | Capability degradation with score-neutral optional stages | Floor (29/60)** · `mcp/retrieval-contracts.mjs` + the Rust store trait |
| 19 | Full ranking trace into the ContextReceipt | Floor (31/60)** · `schemas/context-receipt.v1.schema.json` |
| 20 | Diversity suppression after fusion, never before eligibility | Candidate · benchmark-gated |
| 21 | An explicit admission pipeline before persistence | Floor (4/4 consolidations)** · `crypt-core/src/admission.rs` (new) |
| 22 | "No-op is valid" as a first-class success | Bet · cheap |
| 23 | A validation gate the write path must pass | Bet (28/60) |
| 24 | Deterministic guards on every model-authored mutation; ties go to quarantine | Floor** · `crypt-core/src/dream.rs` |
| 25 | Canonical content held apart from mutable ranking signals | Candidate** · signal sidecar keyed by logical memory id |
| 26 | Decay with retrieval reinforcement, archive never delete | Floor (4/4 passes · 14/60)** · `crypt-core/src/lifecycle.rs` (new) |
| 27 | Bounded asymptotic reinforcement, and recall separated from usefulness | Candidate** · `crypt-core/src/effectiveness.rs` |
| 28 | Expiry checked before scoring, with declared expiry behavior | Bet · cheap |
| 29 | Three independent axes: family, tier, lifecycle state | Floor (14/60)** · extends the existing `Working→Episodic→Semantic` tier |
| 30 | Negative knowledge as a first-class class | Candidate |
| 31 | Three-layer conflict records, written to a sidecar rather than onto the row | Bet (11/59)** · closes **C-4 |
| 32 | Declare single-value vs multi-value predicates | Bet · nearly free |
| 33 | The non-conflict taxonomy, with gates that override the model | Bet |
| 34 | Symmetric conflict detection with late attribution | Bet |
| 35 | Inference lineage — derived beliefs remember their premises and stay retractable | Bet (6/59 — rarest structural pattern in the corpus)** · closes part of **C-4 |
| 36 | Append-only bitemporal supersession with `as_of` queries | Floor (4/4 passes · 20/60)** · `crypt-store/src/temporal.rs` |
| 37 | Session-close episodic packet, pinned and decay-exempt | Floor** · closes **C-5 |
| 38 | A curation pass with undo: hygiene → cluster → distill → archive | Floor (11/59)** · closes part of **C-8 |
| 39 | A job and run lifecycle that survives the process | Bet (14/60)** · the missing substrate under **C-8 |
| 40 | Retroactive session mining — proposal-only, mined off disk | Bet (11/60) |
| 41 | Citation by construction, plus a citation-fabrication guard | Candidate (4/59 — rarest and highest value in the corpus) |
| 42 | Code-anchor fingerprinting with five-way drift classification | Candidate** · `crypt-store` anchor plane + an anchor-audit tool |
| 43 | Five-way *resolution*: text-present is not absent | Bet |
| 44 | Authoritative absence requires proof of coverage | Bet |
| 45 | Embedding provenance — one column, and a question stops being unanswerable | Bet · cheapest high-value item in the corpus** · `crypt-store` |
| 46 | Content-addressed identity — key derived data by its invalidation key | Floor (27/60) |
| 47 | Deterministic substrate → bounded model verdict → deterministic gate | Bet |
| 48 | Derive findings from stored state, never from the current run | Bet |
| 49 | One artifact primitive: compress → cache → retrieve, reversible by construction | Floor (4/4 consolidations)** · closes **C-1 |
| 50 | Never-worse-than-raw, typed as a persisted balance rather than a runtime guard | Floor |
| 51 | A query-critical verifier that restores exact spans | Candidate |
| 52 | Signal preservation as a mandatory paired metric | Candidate |
| 53 | Referential-integrity closures, a protected tail, and an explicit unachievable-budget state | Floor |
| 54 | A representation planner: pick the cheapest form that preserves the required evidence | Floor (4/4 consolidations)** · the admission ↔ renderer boundary |
| 55 | Determinism-first rendering, for prompt-cache validity | Bet · large, cheap, measurable |
| 56 | One deadline end-to-end, one concurrency primitive, dropped-work accounting | Floor** · `mcp/deadline.mjs` · `membrane-runtime` |

### Additional trust / installed-product mechanisms

| Mechanism | Evidence class |
|---|---|
| Canonical redaction gate | Floor, 23/60 |
| Deterministic named redactors | Candidate |
| Influence separated from authority separated from sensitivity | Floor, closes C-4 |
| HMAC-tagged receipt markers | Bet |
| Per-segment permission verdicts | Candidate |
| Loopback guard with DNS-rebinding defense | Bet |
| Trust-before-load and hook integrity baselining | Candidate |
| Erasure fence and signed erasure receipts | Bet |
| Corruption quarantine, never silent regeneration | Candidate |
| Doctor split into read-only inspect and allowlisted repair | Candidate |
| Reason on every decision | Candidate |
| Crash-safe atomic publication | Floor, 35/60 |
| Retry classified by write-safety; idempotency keys with body-hash conflict detection | Floor |
| Backup, export, import, restore, wipe | Candidate |
| Forward-compatible receipt kinds | Bet |
| One operation registry emitting frozen tool and parameter catalogs | Candidate |
| A token budget on the tool surface itself | Bet |
| One plugin core, N host reflection directories | Floor, 18/59 |
| Effect class declared at registration | Candidate |
| Result envelopes, dual-channel results, telemetry as a registration decorator | Candidate |
| Verb-shaped skill decomposition | Bet |
| Complete the `server.json` publication | Candidate |
| Installed-path qualification matrix at 10/10 | Floor |
| Per-adapter serialization and capability caching | Bet |


---

## Appendix C — 60-repository semantic absorption ledger

This ledger is the coverage proof. It maps every exact entry from the original 60-repository corpus index to the semantic capability it contributes. It is deliberately not a 600-row backlog: equivalent donor mechanisms are implemented once under Membrane ownership.

| Repository | Primary absorption into canonical plan | Disposition / owner |
|---|---|---|
| `AbanteAI/archive-old-cli-mentat` | undo/redo, context controls, end-task eval | Adapt reversibility/evaluation; coding agent/TUI out of scope |
| `AlmanacCode/codealmanac` | scheduled knowledge lifecycle, transcript mining, evidence per claim, validation, no-op success | Strong absorb into lifecycle/retain/provenance |
| `Brain0-ai/brain0` | line/source provenance, drift detection, DLP, stable symbol identity, attestations, crypto-shred | Absorb provenance/DLP; stable code symbols via Blueprint |
| `Consiliency/treesitter-chunker` | AST chunks, token budgets, stable symbol graph, incremental/parallel chunking, packing priority | Blueprint owner; Membrane consumes outputs |
| `DeusData/codebase-memory-mcp` | deep semantic code extraction, graph analysis, coverage honesty, incremental reindex | Absorb through Blueprint, not Crypt |
| `Ivy-Interactive/Ivy-Tendril` | process/job supervision, retries, usage accounting, health | Absorb through Hub; agent execution itself out of scope |
| `James-Chahwan/repo-graph` | failure-signal resolution, blast radius, cross-stack tracing, entry points, coverage honesty, PageRank | High-value via Blueprint; PageRank benchmark-gated |
| `LangbaseInc/baseai` | local resident server, unified local/prod pipe, typed boundaries, streaming | Adapt resident/typed runtime concepts; generic agent loop/UI out of scope |
| `Lucas2944/prpack` | task-specific context packaging, base/head completeness, adjacent tests, content hygiene, spend gate | Absorb as evaluation/task-pack design; PR product features not Membrane core |
| `MCrank/code-compress` | production FTS5, incremental symbol indexing, budgeted context, references/blast radius | FTS absorb in Membrane; symbol graph via Blueprint |
| `MemTensor/MemOS` | scheduled memory OS, RRF+MMR+recency, usefulness feedback, automatic state transitions | Absorb fusion/diversity/feedback behind gates |
| `MemoryOS` | Ebbinghaus lifecycle, temporal graph, query expansion, rerank, explain mode, LongMemEval, lineage | One of the strongest direct sources for lifecycle/retrieval/eval |
| `MemoryOS-bailab` | heat-based tiers, per-tier capacity, profile extraction, hard retrieval cap | Adapt capacity/heat; profile extraction proposal-only |
| `MervinPraison/PraisonAI` | multiple compaction strategies, quality promotion, tool-output pruning, session persistence, guardrails | Adapt Push/session/eval; planning/agent framework out of scope |
| `RasaHQ/rasa` | event-sourced session state, lifecycle events, brokers, locks, export, scope | Strong operational model for session/event/sync semantics |
| `Supercompress/Supercompress` | reversible CCR, query-critical verifier, content-specific preprocessors, budget policies, cross-encoder option | Core source for reversible Push; neural rerank benchmark-gated |
| `SynaLinks/synalinks` | typed knowledge schema, retrieval taxonomy, modular reranking, programs tested like code | Absorb retrieval taxonomy/testing; RL program framework out of scope |
| `byterover-cli` | sidecar ranking signals, hysteresis maturity, conflict-safe writes, HITL review, reversible dream | Strong absorb into Crypt lifecycle |
| `caura-ai/caura` | recall gating, atomic-fact enrichment, DLP, degrade-safe ranking, typed event bus | Absorb policy/retain/fallback patterns |
| `claude-subconscious` | lifecycle hooks, mid-task injection, sync dedup, nonblocking session push | Adapt hook reliability; do not depend on one host |
| `cline/cline` | projection-based compaction, Git checkpoints, staleness watchers, context mention expansion | Absorb projection fidelity/freshness concepts; editing checkpoints belong harness |
| `codegraph-ai/CodeGraph` | bi-temporal memory, memory↔code links, BM25+vector+graph fusion, design claim verification | Adapt temporal/link ideas; code graph owner Blueprint |
| `cq27-dev/rag-rat` | deterministic dream guards, verify pass, SCIP/LSP oracles, signed op-log, evidence distill, peer sync | Absorb lifecycle guards; SCIP/LSP through Blueprint; op-log ideas for sync |
| `deepset-ai/haystack` | token-aware compaction, tool-result pruning/offload, typed state, filter protocol, structured eval | Strong absorb reversible Push/eval; generic framework out of scope |
| `drona23/claude-token-efficient` | real provider-usage A/B/C benchmarking, behavior-targeted tests, pre-compaction save | Strong absorb into evaluation and session continuity |
| `emulo` | content-addressed identity, strict validation, policy gates, fail-closed redaction, atomic storage, proof harness | Strong absorb into identity/security/eval |
| `getzep/graphiti` | episodes, bi-temporal edges, multi-strategy search, bounded BFS, resolution, sagas | Absorb episodes/temporal relations narrowly; graph driver sprawl rejected |
| `getzep/zep` | ingest pipeline, boundary-aware splitting, provenance episodes, alias canonicalization, injection hardening, indexing-lag tolerance | Strong absorb retain/entity/security/retrieval-fallback concepts |
| `greplica` | code-anchored claims, fingerprints/drift, parent-chain memory commits, proposal writes, reconciliation | Absorb evidence/claim lifecycle; code anchors via Blueprint |
| `headroomlabs-ai/headroom` | CCR reversible compression, per-tool interception, proactive context expansion, fidelity eval, savings audit | Core source for ArtifactRef/query verifier/Push economics |
| `hindsight` | sentence/fact typing, temporal ranges, entity resolution, causal links, DLP, async retain, export/audit | Strong absorb into Crypt model/lifecycle/security |
| `honcho` | explicit vs derived observations, bounded derivation/Dreamer, trigger gates, telemetry | Absorb derived-record distinction + schedule gates |
| `juspay/code-review-graph-rescript` | typed code graph, diff impact, graph snapshots, flow tracing, RRF, graph evals | Absorb through Blueprint; RRF/eval concepts in Membrane |
| `kingjulio8238/Memary` | graph neighborhood recall, synonym expansion, graph-first routing | Relation retrieval idea only; graph-first default rejected |
| `krohling/bondai` | tiered memory, model-facing memory tools, event lifecycle, hierarchical conversation compression | Adapt memory tiers/lifecycle; generic toolkit/personas out of scope |
| `langchain-ai/langchain` | indexing RecordManager, typed message identity, rate limits, store abstractions | Adapt idempotent indexing/types; backend proliferation rejected |
| `langchain-ai/langmem` | permissioned memory management, namespacing, background reflection, two-layer memory | Absorb permission/lifecycle ideas; prompt optimization separate concern |
| `letta-ai/letta` | versioned memory blocks, optimistic locking, memory filesystem, token-aware compaction | Adapt versioning/locking; Git filesystem not canonical store |
| `mem0ai/mem0` | ADD/UPDATE/DELETE/NONE semantics, event history, expiry, scopes, optional rerank | Absorb typed write disposition/history/expiry; LLM unilateral truth rejected |
| `memory-lancedb-pro` | decay, adaptive retrieve/no-retrieve, BM25 expansion, reflection, scopes, traces | Strong absorb lifecycle/adaptive retrieval; Redis lock not needed |
| `memvid` | time-budgeted extraction, Tantivy lexical, PQ, PII masking, encryption, temporal confidence | Absorb budget/privacy/encryption ideas; PQ only if memory pressure demands |
| `mengram` | typed extraction, pre-LLM secret redaction, procedural evolution, regression gates | Strong absorb security/procedure/eval concepts |
| `microsoft/graphrag` | query-mode routing, entity/claim resolution, deterministic graph ops, incremental indexing | Adapt query classification/entity resolution; communities/DRIFT benchmark-gated |
| `mksglu/context-mode` | FTS5/BM25 content store, multi-source search, flood guards, tool-call stats, byte-safe truncation, hooks | Strong absorb retrieval/observability/truncation patterns |
| `mnemon` | immutable receipts, runtime boundary attachments, view-scoped access, graph recall | Absorb evidence/access ideas; peer-agent network not core |
| `mnemosyne-oss/mnemosyne` | typed memory, per-type decay, veracity consolidation, multi-voice recall, conflicts, MMR, encrypted sync | Strong absorb into lifecycle/retrieval/sync |
| `neuml/txtai` | score-aware hybrid fusion, sparse scoring family, explain search | Absorb lexical/explain/fusion ideas; workflow/agent/cloud surface rejected |
| `qualixar/superlocalmemory` | admission journal, hash-chain audit, ABAC, retention rules, erasure fence, multi-channel retrieval | Absorb journal/erasure/retrieval/security; P2P mesh later/optional |
| `quantmew/context8` | AST hierarchical chunks, hash-based incremental indexing, cancellation, commit pinning | Absorb through Blueprint; cancellation/freshness into provider contract |
| `rohitg00/agentmemory` | validated observation compression, hard working-memory budgets, leases, provenance verification, eval discipline | Absorb compression/eval/locking patterns; Crypt/runtime |
| `rtk-ai/rtk` | never-worse filters, data-class truncation, savings economics, hook integrity | Strong absorb Push guards/economics/security |
| `run-llama/llama_index` | memory blocks, transformation hashes, ingestion cache, property-graph subretrievers | Absorb transformation identity; backend/LLM graph zoo rejected |
| `semantic` | generic AST, scope/reference resolution, LSP tags | Blueprint-only; do not absorb parser into Membrane |
| `semantica` | PROV provenance, conflict workflows, temporal layer, version checksums | Absorb provenance/conflict/temporal concepts; reasoner/ontology platform rejected |
| `shihanwan/memonto` | ontology/triples, delta updates, vector→graph expansion | Delta/typed relation concept; open ontology/SPARQL/script writes rejected |
| `supermemoryai/supermemory` | spaces/scopes, save-or-forget semantics, typed citations, document-centric memory | Adapt scope/write/citation ideas; extension/UI ecosystem out of scope |
| `topoteretes/cognee` | pre-store sanitization, progressive retrieval disclosure, session distillation, feedback weighting | Absorb sanitization/distillation/feedback; avoid graph-platform expansion |
| `vanna-ai/vanna` | tool-usage memory, lifecycle hooks, error recovery, audit hashing, expected-outcome eval | Adapt feedback/procedural memory/eval; UI/integration count not a goal |
| `volcengine/OpenViking` | patch-merge updates, hierarchical retrieval, hotness, observers, privacy lifecycle, benchmark suite | Absorb update/hotness/metrics patterns; custom RAG filesystem rejected |
| `yvgude/lean-ctx` | token envelopes, capability/policy compatibility, normalized failures, routing receipts, holdout gates, savings ledger | Strong absorb into contracts/experiments/receipts |


---

## Final authority statement

This book is the complete Membrane implementation authority for the reviewed corpus. It intentionally preserves one product architecture, one implementation sequence, one exact early execution slice, one evidence-confidence register, and one embedded competitor-coverage proof. Do not create another synthesis document to reinterpret it. Change it only when source evidence, evaluation results, or a versioned architecture decision requires a successor.
