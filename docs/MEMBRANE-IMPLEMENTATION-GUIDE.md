# Membrane — Canonical Implementation Guide

**Status:** single implementation authority for `Orthic-Labs/Membrane`
**Date:** 2026-08-18 · **Baseline:** `main` at `e640aaa7`
**Supersedes:** `research/competitors/sources/MEMBRANE-CANONICAL-MASTER-IMPROVEMENT-GUIDE.md`, `research/competitors/sources/MEMBRANE-IMPLEMENTATION-GUIDE.md`, `research/competitors/sources/MEMBRANE-ABSORPTION-LEDGER.md`, and the August 12 plan (`docs/plans/2026-08-12-membrane-crypt-database-hygiene-and-performance.md`) as implementation authority. Those documents remain as research provenance; nothing in them outranks this one.
**Does not replace:** `docs/architecture.md` (generated product truth), `README.md` (product contracts), `.claude/AGENTS.md`.
**Subordinate execution plan:** `docs/plans/2026-08-17-contextplan-recallcircuit.md` (ContextPlan + Cortex RecallCircuit + layout v2) is the file-exact P0/P1 slice of Phases 1, 5, and 6 below; it executes under this guide's invariants and does not compete with it.
**Evidence basis:** 60-repository competitor corpus, four independent registers, four consolidations, spot-checked against the tree. No implementation or completion is claimed here.

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
- Application / Control / Data planes; a real Membrane ↔ Cortex boundary.

Every source agrees the corpus asks Membrane to **finish and connect** this spine, not replace it. Thirty of thirty examined competitors record what *changed*; none records **what was dropped from a context packet and why**. That receipt/evidence layer is the product differentiator, and the highest-return work is making it reachable, exercised, and measured.

Target:

> **Membrane is a local-first, evidence-aware context control plane and context compiler.** It governs how information is admitted, represented, stored, indexed, related, retrieved, transformed, delivered, remembered, forgotten, and verified — and it produces evidence for every material decision.

Memory is only one class of governed context. Documents, decisions, taste, gotchas, procedures, episodes, temporal facts, artifacts, rules, audit evidence, live Git state, and Cortex code semantics all participate in the same economy without being flattened into one blob.

The design shift the corpus asks for is not *larger* but **more discriminating**: more possible evidence → better discrimination → less delivered context → higher evidence density. Optimize `Context Utility ÷ Delivered Attention Cost` under hard constraints of scope, authority, freshness, truth, security, and deadline.

---

## 1. Locked architectural invariants

Breaking one requires an explicit architecture decision, migration, compatibility proof, and a new evaluation baseline.

1. **Five public shapes stay stable.** Enrich internally, keyed by existing candidate/source/trace IDs. Version deliberately (V2) only when a real consumer cannot be served by existing fields. No donor-shaped public envelopes.
2. **One planner owns final policy**, in order: grant validity → eligibility/scope → authority → freshness → provider-local relevance + bounded fusion → dedupe/diversity → global token/byte admission → representation/lane → publication revalidation → omissions + receipt. Providers describe evidence; Membrane decides attention.
3. **Never flatten unrelated scores.** Cosine, BM25, graph support, Cortex confidence, rule priority, freshness, and feedback are not one calibrated scale. Use hard policy classes → rank fusion (RRF) → bounded utility modifiers within equal policy classes → deterministic canonical-ID tie-breaks. Keep *generation score* (could be relevant), *policy score* (may be admitted), and *utility* (worth its tokens) as distinct dimensions.
4. **One cross-provider attention budget.** Lanes reconcile to one ceiling; provider ceilings bound fan-out cost only.
5. **SQLite/Crypt is canonical durable truth; everything else is a projection.** FTS, vectors, relation graph, derived summaries are rebuildable. Markdown is source evidence or export, never a round-trip database. Git + Cortex supply freshness/repository evidence. The artifact store holds immutable content-addressed raw payloads.
6. **Cortex owns code semantics** — parsing, ASTs, symbols, references, imports/calls/types, entry points, blast radius, rename/move continuity, structural fingerprints, snapshots/diffs, failure-signal resolution. Membrane consumes evidence and decides scope, currency, authority, budget, and delivery mode. No second parser/index stack in Membrane.
7. **Application / Control / Data planes stay separate.** No fourth plane; no SQLite ownership in MCP handlers; no hidden mutation in request routing.
8. **Local-first.** No remote vector/graph store, Redis, distributed queue, or hosted retrieval as a required dependency. Add an abstraction only when two real implementations need it.
9. **Provider failure is typed degradation, not packet fiction.** A failed provider degrades its own lane; healthy providers continue unless the failed one is a hard prerequisite.
10. **No silent omission.** Every cap, timeout, dedupe, fallback, merge, and budget drop emits a typed reason.
11. **No feedback event becomes truth because an agent produced it.** Feedback may move retrieval pressure; it never alters authority.
12. **No background mutation without idempotency and reason-bearing receipts. No graph traversal without bounds. No destructive forgetting without quarantine or tombstone.**
13. **No feature promotes without an evaluation delta. The simplest sufficient implementation wins.**
14. **Generated docs stay generated.** Never hand-edit `docs/architecture.md`; competitor clones stay research inputs, never dependencies.

---

## 2. Ownership map

| Capability | Owner | Membrane action |
|---|---|---|
| Grant validation, final freshness/authority policy | Membrane planner/runtime | Keep central, fail closed |
| Fusion, budget, lanes, omissions, publication, receipts | `membrane-core` + runtime/MCP | Strengthen; never delegate to providers |
| Durable knowledge, temporal facts, lifecycle, feedback, relations | Crypt (`crypt-core` / `crypt-store`) | Implement locally |
| Lexical + semantic + temporal memory retrieval | Crypt | Implement locally |
| AST/symbol/reference/call graph, stable code identity, impact | Cortex | Consume via `engine/federation/providers/cortex.py` |
| Code/document claim validation | Cortex + Audit | Feed resolution status to planner |
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
        │ parallel candidate generation
   ┌────┼────────┬──────────┬────────┬─────────┐
 Cortex  Crypt  Live/Git   Rules    Docs    Audit
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
| source/code anchor | where it resolves; Cortex-backed for code; resolution state may change without rewriting history |
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

---

## 5. Retrieval — canonical stage order

**Stage 0 — normalize.** One monotonic request deadline propagated everywhere; nested stages consume remaining time only (~25% headroom convention). Deterministic query-signal extraction (paths, identifiers, stack traces, quoted strings, error codes, hashes, dates, *why/when/previous/decision/changed*) emits weights that decide which channels get budget — never scope or authority. Recorded in the receipt. No model in the hot path.

**Stage 0b — ContextPlan before fan-out.** Membrane decides which computations are worth running before providers execute: a deterministic, conservative plan (`engine/federation/context_plan.py`) classifies the task (security / migration / architecture / impact / debug / local_edit / docs / general), assigns risk, selects the provider subset, and sets the Cortex traversal policy (`policy_id`, `max_hops`, `max_paths`). Not an LLM router. `general` keeps today's full provider set until evaluation proves narrowing safe; only obvious low-risk classes skip advisory/history lanes. Rolled out shadow ("would run" vs "did run") → low-risk classes → broader classes. Receipts carry `taskClass/risk/providers/cortexPolicyId`, never task text. Retrieval is staged: identity/rules/live → Cortex structural recall → *sufficiency evaluator* → audit/architect/memory/skills only if justified → expensive semantic escalation only if still justified. The evaluator (`retrieval_evaluator.py`, P1) returns `sufficient | insufficient | ambiguous | contradictory | stale | unsafe | provider_failure` plus typed reasons/missing; it decides whether another stage is justified, never answers the task. **Stop condition:** do not retrieve more because budget remains; retrieve more when expected decision value is positive. Rule-based first; no LLM call to decide whether an LLM should receive more context.

**Stage 1 — candidate generation.** Crypt channels behind one trait, one file each, independently ablatable, optional ones off without breaking retrieval:
1. `exact` — canonical IDs, anchors, paths, entities, error/quoted text;
2. `lexical` — FTS5/BM25;
3. `semantic` — existing local vector path;
4. `temporal` — valid/as-of/supersession-aware;
5. `relation` — bounded, seeded by already-relevant records;
6. `working` — task/session-local blocks.
Cortex structural retrieval enters as provider candidates.

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

Fallback ladder: FTS+vector → hybrid; FTS only → exact+FTS+temporal; FTS down → legacy lexical (+vector); relations down → skip expansion; Cortex degraded → keep healthy providers, type the missing evidence.

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

## 8. Cortex bridge and source-drift verification

"Fresh code evidence outranks stale memory" is currently a policy with no detector. Membrane-side anchor: `repository_id, path, symbol/definition_id, range, content hash, base commit/Cortex generation, structural fingerprint (from Cortex), resolution state, last verified`.

Verification hierarchy: exact content/range at current source → current; Cortex stable id resolves elsewhere → moved; one strong structural match → resolved with updated anchor; multiple → ambiguous; absent → missing/drifted; outside grant → inaccessible, never auto-resolved. Five-way identifier resolution: `symbol | file | text_present | absent | unresolvable` — collapsing text-present and absent into `NOT_FOUND` is the root of the divergence false-positive class. **Authoritative absence requires proof of coverage**: emit not-found only when the binding proves the domain is indexed; otherwise downgrade to indeterminate and surface blind spots as receipt fields.

**RecallCircuit is the unit of Cortex evidence.** Membrane sends a task-shaped request with policy/hops/paths; Cortex returns a generation-bound `RecallCircuitV1` (`paths`, `nodes`, `edges`, `unresolved`). Membrane does not traverse the graph. Each **complete** path becomes **one atomic candidate** (`sourceKind: repo_code_circuit`, `id: cortex-circuit:<circuitId>:<pathId>`, `sourceHash` over the path descriptor, `trustClass: workspace_tracked`, `instructionPolicy: data_only`, `scoreComponents: path_complete / evidence_complete / evidence_coverage / hop_efficiency`, rendered as `A --[KIND]--> B --[KIND]--> C` + evidence refs). The path is the semantic unit; top-k admission must never split `A→B→C` into independently admitted nodes. Generation mismatch or schema mismatch → no candidate + typed warning, never reinterpreted as legacy output. Empty circuit / `no_relevant_seed` → zero candidates + loud typed abstention, never generic repository text. Legacy `cortex-candidates.mjs` remains the version-skew/rollback fallback. Cache key includes `policy_id/max_hops/max_paths`. Planner treats `repo_code_circuit` at repo-code priority and applies a bounded `circuit_quality` tie-break *within that source kind only* (complete/evidenced beats incomplete at equal lane score); reserved memory/skill lanes stay. This is where "spend intelligence at build, answer from structure" (graph-memory-starter) lands: pre-walked paths arrive as evidence, the model does not rediscover them. *Cortex-side dependency: RecallCircuit/`cortex-recall.mjs` are specified in the Cortex guides, not yet shipped; Membrane consumption lands with legacy fallback first.*

Cortex query modes Membrane requests: symbol lookup, references/callers, related context, impact, failure signal → symbols, entry points, change context, claim evidence. Response carries stable id, path/range, source hash, revision, dirty overlay, relationship, confidence, coverage, generation, verification status, resolver. Cortex failure degrades only the Cortex lane.

Drift audit as deterministic substrate → bounded model verdict → deterministic gate: pass 0 builds a churn-skipped queue with no model; pass 1 asks only `current | diverged` (`unverifiable` deliberately absent from the vocabulary — pass 0 owns it). Parser: `VERDICT: current | diverged` selects nothing; reset fields on each new verdict line. Derive findings from **persisted** verdicts, never the current run; queue priority: broken anchors → never-checked → content churn → inputs churn → prompt churn last. Embedding provenance: store the hash of the text actually embedded beside the content hash; NULL means unknown, not stale.

---

## 9. Relations and entities — deliberately narrow

Relation row: `relation_id, src, dst (record or entity), kind, valid_from/valid_until, observed_at, producer/evidence refs, confidence, supersession state`. Vocabulary starts with what changes recall/explanation: `supports, contradicts, supersedes, derived_from, part_of, about_entity/mentions, caused_by, applies_to, depends_on, implements, same_as, related_to`. Relations preserve evidence; an embedding similarity is not a relation. Expansion: depth 1, global and per-seed caps, allowed kinds, cycle detection, same scope, provenance on every expanded candidate, no expansion from stale/weak seeds, still subject to authority/freshness/budget. Aliases first; destructive entity merge only when identity is proven. Community detection, PageRank, DRIFT, spreading activation are experiments. Code call/import/reference graphs stay Cortex-owned.

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
- **Surface**: one operation registry emitting frozen tool/parameter catalogs, `.claude-plugin/plugin.json` `contracts.tools[]`, install-time manifest ≡ live surface check; a token budget on the tool surface pinned by golden baseline; effect class (`read/write/execute/network/destructive`) declared at registration, misuse a startup error, authorization carried in the tool; result envelopes with tagged error codes + `structuredContent` + compact model-facing projection; one plugin core with N thin host reflection directories (host list as enum); verb-shaped skills; complete `server.json` publication (+ `smithery.yaml`, `glama.json`, `llms-install.md`) — the gap is publication, not authorship; forward-compatible receipt kinds preserved verbatim.

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
| **5 Cortex anchors + drift** | verifiable code claims, no duplicate Cortex | **RecallCircuit → atomic path candidates** in `cortex.py` with legacy fallback; `repo_code_circuit` priority + bounded circuit-quality tie-break in `planner.rs`; anchor/resolution state; consume stable ids + fingerprints; move/rename/ambiguous/missing; bind generation; query modes; embedding provenance column | multi-hop fixtures need fewer model tool calls; generation mismatch fails closed; deterministic resolution states; no Membrane parser |
| **6a Layout + working set** (may run with 6) | position-aware delivery | layout v2 behind flag in `context-renderer-lib.cjs`; working-set classes in JS/Rust twins with schema bump + digest fixtures | flag-off bytes unchanged; parity exact; answer-quality non-regression before graduation |
| **6 Artifact-backed Push** | loss-bounded, recoverable reduction | `artifact.rs`, `context_edit.rs`; converge `runc`/skel/compress/truncate; externalize before lossy; protected/atomic spans; query-critical restore; `TokenBalanceV1`; representation planner; deterministic prefix; citation-by-construction | no protected corruption; raw resolvable; reduction measured at non-inferior evidence quality |
| **7 Relations + aliases** | recall-aware relations, no graph platform | `relations.rs`; evidence on edges; depth-1 bounded expansion; alias canonicalization | scoped, capped, cycle-safe, disable-able |
| **8 Sessions + curation** | strengthen useful, retire stale, no guesswork | session packets; working capacity/expiry; hysteretic retention; Job/Run model; scheduled idempotent maintenance; curation with undo; offline session mining (proposal-only) | resume improves without transcript duplication; no oscillation; maintenance never blocks prompts |
| **9 DLP, influence, erasure** | close trust boundaries | DLP at both boundaries; producer-based authority; path-jail tests; publication revalidation; erasure fence; tombstones without payload; HMAC receipt markers; hook integrity | zero cross-scope leaks; erased content cannot reappear |
| **10 Storage + operations** | operationally trustworthy | store resolver/identity; doctor inspect/repair split; read-only inventory; projection rebuild; backup/restore drill; export/import; wipe; migration preflight/backout; crash-boundary tests; sync stays event-based | backup/restore preserves logical keys, lineage, recall equivalence; corruption is typed, never "delete the DB" |
| **11 Gated experiments** | intelligence only where measured gap | in order: retrieve/no-retrieve gate; rule-based query expansion; MMR; relation variants; local cross-encoder in shadow (pure provider, sub-second deadline, kill-switch off); LLM extraction/expansion/reflection; staged document/image/audio/video; multimodal embeddings last | frozen holdout non-inferiority; latency/RSS/token budget; auto-rollback |
| **12 Closed loop + qualification** | prove what ships | candidate journey + verified outcomes; calibration by task class/family/provider; ablation cohorts; all suites; installed Mac+Windows 10/10 matrix (install→discovery→tools→grant→context→resolve→proposal→feedback→checkpoint→restart/degrade→upgrade→uninstall); real p50/p95/RSS/token/index data; competitor claims from receipts only | supported paths pass; no new external surface before qualification |

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
| `engine/federation/providers/cortex.py`, `test_cortex_provider.py` | existing | consume stable evidence/query modes; drift/degradation tests |
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
| Duplicating Cortex (parser, LSP, SCIP, symbol/code graph) | stable anchors, fingerprints, blind-spot reporting via provider contract |
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

**Cortex boundary** — Cortex supplies identities/ranges/relations/impact/generation; Membrane does scope/authority/freshness/admission/resolution policy; moved/ambiguous/missing distinguished mechanically; no second parser.

**Security** — DLP/influence at persist and delivery; text cannot self-authorize; path/root/symlink/case tests green; revoke/delete races cannot publish stale bytes; erased content cannot reappear from any projection.

**Operations** — Hub owns lifecycle; prompts start no heavy jobs; jobs bounded/cancellable/idempotent/crash-safe; unambiguous storage identity; doctor/backup/export/import/restore/wipe tested; corruption is a typed repairable state.

**Evidence** — Phase-0 baseline checked in and reproducible; four proofs per capability; LoCoMo/LongMemEval/BEAM/whole-task/stateful/security/recovery suites against current artifacts; Mac + Windows installed paths qualified; real p50/p95/RSS/token/index numbers; competitor claims labeled until reproduced; **one** active implementation authority.

Every durable item can answer: what am I, where did I come from, what supports me, whose scope, how authoritative, when observed, when valid, what replaced me, what did I derive from, what state am I in. Every packet can answer: what was delivered, what was not, why, what was transformed, and how to recover the exact evidence.

---

## 18. Adoption steps

1. This file is the authority at `docs/MEMBRANE-IMPLEMENTATION-GUIDE.md`.
2. Add `Superseded by: ../MEMBRANE-IMPLEMENTATION-GUIDE.md` to the August 12 plan; keep its history.
3. Treat `docs/compshop/*` as research provenance; do not restore `sol.md`, `solimplement.md`, `final_absorption.md`, or `deepseek.md`.
4. Keep `competitor.md` as the 60-source coverage index and `/repos/` clones ignored.
5. Regenerate the current-state manifest, then freeze the Phase-0 baseline before any implementation.
6. Implement one phase at a time with additive migrations, rollback, and exit gates.
7. Regenerate `docs/architecture.md` only through its generator after source lands.
8. Retire this guide only when §17 is closed or a versioned successor replaces it.

> **The strongest context system is not the one with the most memory types, graph algorithms, or backends. It is the one that preserves exact evidence, retrieves the smallest current authoritative subset, reduces everything else reversibly, keeps derived text distinct from truth, recovers from failure without loss, and proves every inclusion, omission, transformation, lifecycle change, and degradation under the user's current authority.**
