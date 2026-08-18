# Membrane — Canonical Master Implementation Guide

**Date:** 2026-08-17  
**Repository:** `Orthic-Labs/Membrane`  
**Repository baseline inspected:** `main` at `e640aaa77b6d51ddeaf6d1bc825770b6bf7264bd`  
**Status:** canonical synthesis and implementation authority candidate; **no implementation/completion claim**  
**Primary source corpus:** `competitor.md` (60 repositories) plus the four supplied consolidated guides:

- `dsv4flashMEMBRANE_MASTER_IMPLEMENTATION_GUIDE.md`
- `m3MEMBRANE_CANONICAL_IMPLEMENTATION_GUIDE.md`
- `dsv4proMEMBRANE-CANONICAL-IMPLEMENTATION-GUIDE.md`
- `qwenmembrane-canonical-implementation-guide.md`

**Repository truth used to reconcile the guides:** current `main`, especially `AGENTS.md`, `docs/architecture.md`, `docs/MEMBRANE-CURRENT-STATE-MANIFEST.json`, the August 12 plan, the protocol/core/runtime/Crypt source, and the current provider/store layout.

> This is a semantic synthesis, not a fifth pass that blindly re-ranks all 600-ish donor recommendations. The source guides disagree on raw candidate counts (roughly 588–600) because their upstream memo coverage differed. Coverage authority in this master is the exact 60-repository `competitor.md` index, while implementation decisions are resolved against the current Membrane codebase.

---

# 0. Executive decision

Membrane does **not** need a new top-level architecture. It already has the difficult architectural spine:

- one **Push / Pull / Persist** context economy;
- five typed public protocol shapes: `ScopeGrantV1`, `ContextCandidateSetV1`, `ContextPacketV1`, `ContextReceiptV1`, and `KnowledgeEmissionV1`;
- heterogeneous provider federation behind one planner;
- authority and freshness kept distinct from provider-local relevance;
- one global attention budget with `native`, `rendered`, `resolver_backed`, and `metadata_only` lanes;
- typed omissions, degradation, and receipts;
- repository confinement and signed grants;
- local-first SQLite-backed Crypt persistence;
- deterministic context transforms and compression primitives;
- resident federation and local vector retrieval;
- temporal facts with immutable supersession semantics;
- explicit Application / Control / Data process planes;
- a real separation between Membrane context control and Cortex repository semantics.

The competitor corpus does not justify replacing those foundations. It says Membrane should **finish** them.

The canonical target is:

> **Keep Membrane's contract and planner spine. Make Crypt a selective, lifecycle-aware, evidence-addressed knowledge substrate; make Pull a real indexed, explainable, multi-channel evidence retriever; make Push reversible through governed artifact references; let Cortex own code semantics and source drift; and make every inclusion, omission, transformation, lifecycle transition, recovery action, and adaptive policy measurable and reversible.**

The work therefore collapses into twelve programs:

1. one canonical documentation/evaluation authority;
2. evidence, epistemic identity, and immutable content identity;
3. persistence admission, conflict handling, and lifecycle state;
4. production lexical retrieval plus explainable channel fusion;
5. Cortex-backed source anchors and drift verification;
6. reversible artifact-backed Push/context editing;
7. narrow temporal relations/entity aliases, not a graph platform;
8. session continuity plus bounded background curation;
9. trust, DLP, influence, publication fencing, and erasure;
10. storage identity, repair, backup/export/import/restore/wipe;
11. evidence-gated adaptive retrieval and staged multimodal extraction;
12. observability, closed-loop learning, native qualification, and release proof.

If this guide is adopted into the repository, the recommended canonical path is:

`docs/MEMBRANE-IMPLEMENTATION-GUIDE.md`

It should supersede the dangling August 12 execution plan as implementation authority. It must **not** replace generated product truth: `docs/architecture.md` remains generated from source and should change only when implementation changes land.

---

# 1. What the four source guides disagree about — and the canonical resolution

The four guides are more convergent than they first appear. Most differences are sequencing, abstraction granularity, or how aggressively to adopt optional intelligence.

| Question | Source-guide divergence | Canonical resolution |
|---|---|---|
| What is the source count? | 588, 590, or ~600 recommendations depending on memo set | Ignore recommendation-count arithmetic. The exact coverage set is the 60 repositories in `competitor.md`; this guide carries a 60-row disposition ledger. |
| What comes first? | M3 puts evaluation first; Pro puts baseline then epistemics; Qwen makes retrieval work more prominent; Flash combines documentation freeze + baseline | **Phase 0 is authority + reproducible baseline/eval.** No ranking, lifecycle, compression, or graph change ships without a frozen comparison surface. |
| Should evidence become a new public protocol family immediately? | Pro proposes explicit evidence protocol types; M3/Flash are more conservative | Build the richer evidence model **internally first**, keyed by existing IDs. Preserve the five V1 public shapes. Introduce V2 only when a real downstream consumer cannot be represented by existing source/hash/resolver/receipt fields. |
| How should retrieval be structured? | Qwen proposes a retrieval-channel registry; M3/Flash describe a staged pipeline | Use a **small internal channel abstraction**, not a plugin ecosystem. Start with exact, lexical, semantic, temporal, relation, and active-working channels. Do not add generic backend factories. |
| What replaces current lexical search? | Qwen allows FTS5 or an in-process Rust index; Flash strongly favors FTS5/BM25 | Implement **SQLite FTS5/BM25 first**, because SQLite is already canonical durable truth and current lexical scoring is simplistic. Keep the current deterministic lexical path as fallback. Replace FTS5 only if measured quality/resource evidence clearly favors an in-process alternative. |
| Should query classification be core? | Qwen promotes it strongly; Flash makes adaptive routing gated | Add a deterministic, inspectable query-signal stage only after Phase 0. It may influence channel execution, never scope/authority. More aggressive retrieve/no-retrieve gating remains benchmark-gated. |
| Should graph retrieval be high priority? | Some guides emphasize graph expansion; Flash is narrower | **Relation storage and evidence identity are high value; broad graph traversal is not.** Use bounded expansion only after strong seeds. Repository code graph work stays in Cortex. |
| Should decay/retention formulas be copied? | Donors offer Ebbinghaus, Weibull, heat, maturity, frequency, etc. | Do not canonize a donor formula. Model stable lifecycle signals, hysteresis, and policy versions; calibrate the simplest useful policy on held-out outcomes. |
| Is Dream the lifecycle engine? | Older Membrane framing can imply this; Pro/Qwen explicitly object | No. Dream/consolidation becomes **one reversible background phase** inside an explicit lifecycle policy. Admission, reinforcement, conflict, supersession, expiry, quarantine, restore, and forget are separate contracts. |
| Where does source drift live? | Pro emphasizes source anchors; many donor memos suggest AST/symbol infrastructure | Membrane stores/uses evidence identity and resolution state. **Cortex owns parsing, symbols, structural fingerprints, rename/move identity, reference/call graphs, and impact.** |
| How aggressive should compression be? | Qwen frames context assembly as a compiler; M3 stresses loss-bounded transforms; Flash emphasizes artifact externalization | Use one ordered **reversible Push ladder**: exact dedupe → content-addressed externalization → structure-preserving reduction → optional compression → explicit truncation last, with protected spans and exact restoration. |
| When does Hub lifecycle work happen? | The August 12 plan makes it an early work package; other guides focus on Membrane internals | Keep it early only at the **seam**: Membrane must expose readiness/drain/health/identity and stop owning user-facing OS lifecycle. Hub remains the OS/start-at-login owner. Do not turn this guide into a Hub rewrite. |
| Where does multimodal belong? | August 12 plan gives it a package; Flash demotes advanced multimodal embeddings | Artifact/evidence identity must land first. Then add deterministic extractors in stages. Multimodal embeddings remain optional and evidence-gated. |
| How much LLM inference belongs in memory/retrieval? | Some donors use LLM extraction, reflection, query expansion, reranking | Deterministic/local/inspectable first. LLM-assisted extraction, expansion, reflection, and reranking stay optional, shadowed, provenance-bearing, and never become mandatory truth or recall dependencies. |

This resolution matters because it prevents the final plan from becoming a union of four independently coherent but overlapping architectures.

---

# 2. Locked architectural invariants

These are constraints on all implementation below. Breaking one requires an explicit architecture decision, migration, compatibility proof, and new evaluation baseline.

## 2.1 Keep the five public protocol shapes stable

The Rust protocol types are the current source of truth. Do not introduce public donor-shaped envelopes such as generic `MemoryCube`, graph-node results, universal documents, or backend-specific search objects.

Prefer internal enrichment keyed by existing candidate/source/trace IDs. When new wire semantics are genuinely needed, version them deliberately rather than mutating V1 invisibly.

## 2.2 One planner owns final policy

The canonical Membrane planner owns, in order:

1. request/grant validity;
2. eligibility/security/scope;
3. authority class;
4. freshness class;
5. provider-local relevance ordering and bounded fusion;
6. cross-source dedupe/diversity policy;
7. global token/byte admission;
8. delivery representation/lane;
9. final publication revalidation;
10. omissions and receipt reconciliation.

Providers return typed candidates, evidence, and provider-local relevance information. They do not decide final packet policy.

## 2.3 Never flatten unrelated scores

A vector cosine, lexical BM25 score, graph support, Cortex confidence, policy authority, and freshness are not calibrated probabilities on one shared scale.

Use:

- hard policy classes for eligibility/authority/freshness;
- rank fusion such as RRF for incomparable retrieval channels;
- bounded utility modifiers only within equivalent policy classes;
- stable canonical-ID tie breaking.

Do not create one global magic weighted sum.

## 2.4 Preserve one cross-provider attention budget

The existing `native`, `rendered`, `resolver_backed`, and `metadata_only` lanes remain mutually exclusive and reconcile to one global ceiling.

Provider ceilings may bound fan-out cost. They may not become separate final attention budgets.

## 2.5 Preserve Application / Control / Data planes

No fourth process plane. Do not move SQLite ownership into MCP/application handlers, networking into the data plane, or hidden file mutation into request routing.

## 2.6 SQLite is canonical durable truth; indexes are projections

Use the following authority model:

```text
SQLite canonical rows/events      = durable truth
FTS / vector indexes              = rebuildable retrieval projections
memory relation graph             = rebuildable/derivable projection over canonical IDs
Markdown                          = export / human-readable interchange
Git + Cortex                      = source freshness / repository evidence
artifact store                    = immutable raw payloads addressed by content identity
```

No second authoritative memory corpus.

## 2.7 Cortex owns repository semantics

Membrane must not become a second parser/indexer. Cortex owns:

- language parsing and adapters;
- symbols and stable definition identity;
- source ranges;
- import/call/reference/type edges;
- entry points and impact/blast radius;
- rename/move continuity;
- diff/snapshot semantics;
- failure-signal-to-symbol resolution;
- structural fingerprints and code claim verification inputs.

Membrane owns whether that evidence is in scope, current, authoritative, worth context budget, and still resolvable under the current grant.

## 2.8 Local-first remains a product constraint

Do not make remote vector stores, graph stores, Redis, distributed queues, or hosted retrieval a required dependency. Add an abstraction only when at least two real implementations need it or a measured scale failure requires it.

## 2.9 Provider failure is typed degradation, not packet fiction

One provider timing out or being unavailable must not silently claim full context. It also should not suppress unrelated healthy providers unless the failed provider is a hard prerequisite for the specific request.

## 2.10 Generated documentation stays generated

`docs/architecture.md` is generated product truth. Do not hand-edit it to make the implementation appear complete.

## 2.11 Competitor repositories remain research inputs

`competitor.md` is the canonical coverage index. Local clones under `/repos/` are intentionally ignored by Git and should not become vendored product dependencies. The master plan absorbs mechanisms, not donor source trees.

---

# 3. Ownership map

| Capability | Canonical owner | Membrane action |
|---|---|---|
| Scope grant validation, authorization, final freshness/authority policy | Membrane planner/runtime | Keep central and fail closed |
| Cross-provider fusion, budget, lanes, omissions, publication, receipts | `membrane-core` + runtime/MCP | Strengthen; never delegate to providers |
| Durable knowledge records, temporal facts, lifecycle, feedback, memory relations | Crypt | Implement in `crypt-core` / `crypt-store` |
| Production lexical + semantic + temporal memory retrieval | Crypt | Implement/refine locally |
| AST/symbol/reference/call/type graph, stable code identity, blast radius | Cortex | Consume through `engine/federation/providers/cortex.py`; do not duplicate |
| Code/document claim validation | Cortex + Audit | Feed evidence/resolution status to planner |
| Large immutable raw payload/object storage | Membrane runtime under Hub-controlled local storage | Add governed content-addressed artifact abstraction |
| Tool-result/context reduction | Membrane Push/runtime | Converge existing `runc`/`skel`/`compress`/truncation behavior under one transform contract |
| Session working/hot context | Membrane runtime + Crypt episodic persistence | Bound it; do not own full host conversation history |
| Background memory curation | Crypt jobs, scheduled/supervised by Hub lifecycle | Bounded, idempotent, never prompt-critical |
| OS start-at-login, child process ownership, restart/backoff | Hub | Membrane exposes readiness/drain/health/identity only |
| Agent execution, PTYs, autonomous coding loop | Legion/harness | Out of Membrane scope |
| Generic model routing | OmniRouter/harness | Out of Membrane scope except optional internal gated inference |
| Web/TUI shell | Hub or separate UI | Typed Membrane data only; no core dependency |
| Remote vector/graph database | None by default | Reject as required architecture |
| RDF/SPARQL/general ontology reasoner | None | Reject |
| KV-cache/model-weight memory | Model/runtime research | Defer |

---

# 4. Target architecture

## 4.1 Pull — retrieve the smallest useful current evidence set

```text
host / client / MCP
        │
        ▼
ScopeGrant + task + session identity
        │
        ▼
┌──────────────────────────────────────────────────────────┐
│               CANONICAL MEMBRANE PLANNER                │
│ grant/scope → authority → freshness → fusion → budget    │
│ representation → publication fence → omissions/receipt  │
└──────────────────────────────────────────────────────────┘
        │ parallel candidate generation
        ├──────────┬──────────┬──────────┬──────────┬───────────┐
        ▼          ▼          ▼          ▼          ▼           ▼
     Cortex      Crypt      Live/Git   Rules      Docs       Audit/etc.
 code semantics knowledge  current     policy    evidence    findings
                   │
                   ├─ exact / ID / anchor
                   ├─ FTS5 / BM25 lexical
                   ├─ vector semantic
                   ├─ temporal fact
                   ├─ bounded relation/entity
                   └─ active working/session overlay
        │
        ▼
normalized eligible candidates
        │
        ▼
rank fusion → bounded utility → diversity → global admission
        │
        ▼
ContextPacket + ContextReceipt
        │
        ├─ native
        ├─ rendered
        ├─ resolver_backed
        └─ metadata_only
```

## 4.2 Persist — remember selectively, not indiscriminately

```text
observation / explicit memory request / verified outcome / session end
        │
        ▼
validate scope + producer + influence + sensitivity + DLP
        │
        ▼
normalize family / epistemic state / candidate atoms
        │
        ▼
logical ID + content hash + evidence identity + source/version refs
        │
        ▼
exact duplicate? → no-op success
        │
        ├─ conflict? → preserve both + conflict relation / proposal
        ├─ correction? → superseding immutable transition
        ├─ low-trust/unsafe? → reject or quarantine
        └─ eligible? → canonical record + evidence links
                           │
                           ├─ temporal fact/lineage if applicable
                           ├─ relation edges if useful
                           └─ mutable ranking signals in sidecar
```

## 4.3 Push — reduce context without losing recoverability

```text
large tool/document/context payload
        │
        ▼
classify content + identify protected/atomic spans
        │
        ▼
1. exact dedupe
2. persist/hash-address raw payload and create governed resolver
3. structure-preserving reduction
4. optional compression/summarization if still needed
5. explicit truncation only as final fallback
        │
        ▼
query-critical verifier / protected-span check
        │
        ├─ safe reduced view
        └─ restore exact raw spans from artifact when required
        │
        ▼
existing global budget/lane planner
```

---

# 5. Canonical knowledge and evidence model

The key model change is not “more memory types.” It is separating **what a durable thing is**, **why it should be believed**, **when it was true**, and **how useful it has been**.

## 5.1 Separate identities by purpose

Never overload one hash/ID to answer five different questions.

| Identity | Answers | Canonical behavior |
|---|---|---|
| Logical record ID | “Which durable knowledge object is this?” | Stable across non-semantic metadata changes; not reused for unrelated content |
| Content hash | “Are these exact canonical bytes/content equivalent?” | `sha256`; immutable version/content identity |
| Evidence ID | “Which observation/source event supports this claim?” | Identifies source observation/range/commit/event; can have multiple per record |
| Source/code anchor ID | “Where in source does the evidence resolve?” | Cortex-backed for code; may drift/resolution-state change without rewriting history |
| Derivation/pipeline fingerprint | “How was this derived?” | Transform/extractor/model/prompt/config/version identity for rebuildable derived views |
| Artifact ID | “Which immutable raw payload is this?” | Content-addressed; resolver policy checked at read time |

## 5.2 Durable knowledge record

Do not force every field into the hot `MemoryEntry` projection. Introduce a richer canonical record behind the current retrieval structure.

Conceptually:

```text
KnowledgeRecord
├─ id
├─ canonical_content
├─ content_sha256
├─ scope / repository / session lineage
├─ family
├─ epistemic_kind
├─ truth_state
├─ authority
├─ influence_class
├─ sensitivity
├─ lifecycle_state
├─ created_at / updated_at
├─ valid-time / expiry / supersession refs
├─ evidence refs
├─ derivation refs
└─ relation refs
```

Operationally meaningful families only:

- `observation`
- `episode`
- `semantic_fact`
- `procedure`
- `preference`
- `entity_summary`
- `evolving_belief`
- `artifact_reference`

Do **not** invent more families until they require distinct write/retrieval/lifecycle behavior.

## 5.3 Epistemic state

Use explicit distinctions such as:

```text
kind: fact | requirement | decision | preference | procedure | lesson | failure | risk | question
truth_state: source_verified | code_verified | observed | derived | inferred | asserted | unverified | contradicted
intent_state: intended | accidental | unknown
```

The exact enum spelling is implementation detail; the invariant is not.

A model-generated relation or summary never becomes authoritative source evidence merely because its prose sounds certain.

## 5.4 Canonical content vs mutable usefulness signals

Move retrieval/use dynamics to a sidecar keyed by logical record ID. The canonical content should not be rewritten because its retrieval value changed.

Suggested signal fields:

```text
retrieval_count
selected_count
delivered_count
resolved_count
verified_used_count
ignored_count
contradicted_count
last_retrieved_at
last_used_at
last_contradicted_at
base_importance
retention_strength / hotness
lifecycle_score_epoch
ranking_policy_version
```

This sidecar is recalibratable/rebuildable. Truth is not.

## 5.5 Evidence references

Every durable claim that can affect context should be able to answer:

- what is it?
- where did it come from?
- who/what produced it?
- what exact source/content hash supports it?
- at which source revision/commit/generation was it observed?
- is it direct or derived?
- can the raw evidence still be resolved under the current grant?
- has it drifted, been superseded, or been contradicted?

Use normalized evidence tables/records rather than an ever-growing opaque JSON blob.

## 5.6 Resolution states

For source-backed evidence, normalize states such as:

```text
resolved
ambiguous
missing
drifted
unsupported
inaccessible
revoked
```

Resolution is a current-state property; historical evidence identity remains immutable.

## 5.7 Persist/write dispositions

Every write attempt ends in one explicit disposition:

```text
retain
update-metadata-only
supersede
conflict
no-op
quarantine
reject
expire
forget
restore
```

`no-op` is a successful outcome when equivalent durable truth already exists.

---

# 6. Retrieval: exact canonical order

The strongest common retrieval lesson from the corpus is not “use more algorithms.” It is **stage different kinds of evidence so cheap deterministic signals, hard policy, and explanation survive**.

## Stage 0 — normalize request and carry one deadline

Create one request deadline/budget and propagate remaining time to every provider/channel. Child work cannot invent new time budgets.

Extract deterministic signals:

- exact path/symbol/ID/error/commit patterns;
- temporal language;
- change/history intent;
- decision/why intent;
- broad conceptual intent;
- explicit user anchors.

The signal classifier cannot widen scope or authority.

## Stage 1 — candidate generation

Crypt canonical channels:

1. **exact** — canonical IDs, exact anchors, exact paths/entities/fact subjects, exact error/quoted text where indexed;
2. **lexical** — production FTS5/BM25 with code-aware tokenization where appropriate;
3. **semantic** — existing local vector path;
4. **temporal** — valid/as-of/supersession-aware facts;
5. **relation/entity** — bounded only, seeded by already-relevant records;
6. **active working** — task/session-local blocks.

Repository structural retrieval remains a Cortex provider concern and enters Membrane as provider candidates.

## Stage 2 — hard eligibility

Before soft relevance:

- signed grant and repository scope;
- ACL/capability;
- influence/instruction policy;
- quarantine/deletion/revocation state;
- temporal validity/supersession;
- source availability/resolution state;
- current generation/freshness constraints;
- sensitivity routing constraints.

No reranker can resurrect an ineligible candidate.

## Stage 3 — authority and freshness classes

Group/order candidates by the existing authority and freshness contract. Fresh current source can outrank highly similar stale memory.

## Stage 4 — provider/channel-local rank fusion

For incomparable channel scores, keep RRF as the deterministic baseline.

A result should retain:

```text
channel_id
channel_rank
optional raw channel score for diagnostics
fused rank contribution
exact-evidence flag
```

Do not pretend raw scores are calibrated probabilities.

## Stage 5 — bounded utility modifiers

Only within equivalent policy classes, apply bounded features such as:

- verified historical effectiveness;
- recency where semantically appropriate;
- retention strength/hotness;
- relation support;
- contradiction penalty;
- exact-match survival preference.

Outcome feedback is not relevance itself. It is a bounded usefulness signal.

## Stage 6 — diversity/redundancy suppression

After eligibility/fusion, suppress near duplicates for broad queries using cheap deterministic duplicate lineage/content hashes first. MMR may be tested later.

Do not diversity-penalize protected/exact evidence aggressively.

## Stage 7 — global attention admission

Estimate token/byte cost, choose representation, and fill the existing single global budget.

## Stage 8 — final publication fence

Immediately before bytes leave the process, revalidate the active grant/policy epoch for anything that can be revoked or whose scope changed during assembly.

If the epoch changed, retry once under the new epoch or emit typed `policy_changed`/degraded behavior. Never publish stale-authority bytes.

## Stage 9 — receipt and feedback binding

Every candidate should have a compact journey:

```text
generated → eligible/ineligible → ranked → selected/dropped → delivered
          → resolved/fetched → verified_used/ignored/contradicted → superseded
```

Use stable delivered IDs so later verified feedback updates the signal sidecar without rewriting canonical content.

---

# 7. Production lexical retrieval

Current Crypt hybrid retrieval is already better than vector-only RAG because it combines lexical ranking, vector similarity, RRF, an indexed vector path, and fallback. The gap is that the lexical arm is still keyword equality + substring occurrence + stored score.

## 7.1 Implement FTS5/BM25 as the first production sparse index

Add a rebuildable SQLite FTS projection over eligible searchable fields. At minimum support:

- BM25 ranking;
- exact phrase bonus/pinning path;
- prefix support where bounded;
- field weighting;
- scope/family/authority filters outside or alongside the FTS query;
- deterministic canonical-ID tie breaks;
- incremental upsert/delete driven by canonical record events;
- schema/index version identity;
- rebuild/repair command;
- fallback to current lexical logic when FTS is unavailable/corrupt.

For code-ish identifiers stored in memory, tokenize or normalize:

- `snake_case`
- `camelCase`
- `PascalCase`
- `kebab-case`
- path segments
- `module::symbol`
- punctuation-bearing identifiers/errors

Do not add Elasticsearch, a remote search daemon, or a separate database merely to get BM25.

## 7.2 Candidate limit is not final K

Separate:

- per-channel candidate overfetch;
- filtered/reranked provider result limit;
- final cross-provider admitted packet size.

Overfetch must be bounded, visible in diagnostics, and justified by filtering/expansion needs.

## 7.3 Fallback ladder

```text
FTS + vector available → hybrid rank fusion
FTS available, vectors unavailable → exact + FTS + temporal
FTS unavailable/corrupt → existing deterministic lexical + optional vector
relation projection unavailable → continue without relation expansion
Cortex degraded → preserve healthy providers, type the missing structural evidence
```

Graceful degradation must never silently claim equivalent evidence coverage.

---

# 8. Persistence admission and lifecycle

## 8.1 Admission happens before durable truth

Canonical retain pipeline:

```text
validate producer/scope
→ classify sensitivity/influence
→ DLP/redaction policy
→ normalize family/epistemic type
→ content identity + evidence identity
→ exact duplicate/no-op check
→ contradiction/conflict/supersession check
→ quality/admission policy
→ retain / propose / quarantine / reject / no-op
```

Do not persist everything and hope Dream cleans it later.

## 8.2 Negative/failure knowledge is first-class

Failures, rejected approaches, contradictions, and lessons can be high-value durable knowledge. They must not automatically become instruction authority.

## 8.3 Conflict is not overwrite

When two credible observations disagree:

- preserve both evidence histories;
- mark conflict/contradiction explicitly;
- prefer direct/current/higher-authority evidence during retrieval;
- use supersession only when there is a justified replacement relation;
- never destroy the losing historical claim merely to simplify ranking.

## 8.4 Preserve current temporal fact semantics

The existing temporal fact model already has observation time, validity intervals, expiry, authority/veracity, and supersession. Extend it; do not create a second temporal subsystem.

Add transaction/system-time metadata only when it solves a real historical query or audit requirement.

## 8.5 Lifecycle is an explicit state machine

Operational states may include:

```text
candidate
active
reinforced
consolidated
dormant
quarantined
superseded
deleted/tombstoned
```

States must have behavioral meaning. Avoid decorative tiers.

## 8.6 Reinforcement is bounded and verified

A successful retrieval is not automatically “used.” Prefer verified signals tied to observable outcomes/actions. Do not let one successful turn radically rewrite global ranking.

## 8.7 Retention policy is pure, versioned, and calibratable

Compute promotion/demotion/decay decisions from explicit signals in a pure function. Use hysteresis so records do not oscillate around thresholds.

Do not canonize Ebbinghaus/Weibull/heat merely because a donor uses it. Fit the simplest policy that wins held-out evaluation.

## 8.8 Dream becomes a reversible maintenance phase

Dream may:

- find deterministic duplicates;
- propose consolidations;
- create derived summaries with complete parent/evidence identity;
- verify stale anchors;
- propose promotion/demotion/quarantine.

It must not silently mutate source truth or promote derived prose into authority.

## 8.9 Session continuity

At session end, create a bounded episode/checkpoint containing only:

- task identity/goal;
- important decisions and outcomes;
- unresolved questions;
- explicit durable proposals;
- pointers to relevant raw artifacts/evidence.

Do not mirror the host's entire transcript as canonical memory.

---

# 9. Reversible Push and governed artifacts

## 9.1 Add one internal artifact primitive

A governed artifact reference should minimally carry:

```text
artifact_id = art:<sha256>
content_sha256
mime_type
byte_length
origin/source identity
scope/repository identity
sensitivity/influence class
created_at
extractor/transform identity if derived
integrity/availability state
resolver capability
current-policy check metadata
```

Public V1 packet shapes do not need a new field immediately: the existing resolver-backed lane can carry a stable resolver/source reference while the internal artifact model matures.

## 9.2 Ordered reduction ladder

1. exact duplicate suppression;
2. raw immutable externalization;
3. structure-preserving reduction;
4. optional semantic compression/summarization;
5. explicit truncation last.

Every transform records before/after bytes/tokens, transform kind/version, source hash, output hash, and recoverability.

## 9.3 Never-worse verifier

Protected content classes include at least:

- exact identifiers and paths requested by the task;
- error messages/codes involved in diagnosis;
- citations/source references;
- authority/security/policy statements;
- structured sequences where partial delivery changes semantics;
- explicit user anchors.

If reduction removes task-critical evidence, restore the exact span from the artifact or choose a less aggressive representation.

## 9.4 Cache views, not truth

Compressed/skeletonized/extracted views are rebuildable projections keyed by source hash + transform fingerprint. The raw source/artifact remains authoritative.

## 9.5 Documents and multimodal content

Only after artifact identity/resolution is reliable:

1. PDF/text/document extraction;
2. image metadata and OCR where needed;
3. audio transcription;
4. video metadata/keyframe references;
5. multimodal embeddings only if held-out tasks justify them.

Derived text remains cited, scoped, versioned, rebuildable, and independently expirable.

---

# 10. Cortex bridge and source-drift verification

The competitor corpus contains many excellent code-intelligence mechanisms. They belong in Cortex, with Membrane consuming typed evidence.

## 10.1 Membrane-side source anchor

A source/code evidence reference should be able to retain:

```text
repository_id
path
symbol/definition_id when available
start/end range when available
source/content hash
base commit / Cortex generation
structural fingerprint when Cortex provides one
resolution state
last verified time
```

## 10.2 Two hashes answer different questions

- content hash: “are these exact bytes/content unchanged?”
- structural/source anchor identity: “is this still the same logical code entity after benign movement/rename?”

Do not substitute one for the other.

## 10.3 Verification hierarchy

For code-backed durable knowledge:

1. same exact content/range at current source → resolved/current;
2. Cortex stable symbol/definition identity resolves elsewhere → moved but continuous;
3. structural fingerprint/semantic resolver gives one strong match → resolved with updated anchor metadata;
4. multiple plausible targets → ambiguous;
5. target absent → missing/drifted;
6. source outside current grant → inaccessible, never auto-resolved.

## 10.4 Membrane requests; Cortex reasons

Useful Cortex query modes may include:

- exact symbol/source resolution;
- references/callers/callees;
- route→handler→schema path;
- test↔implementation relation;
- changed-symbol impact;
- failure signal→candidate symbols;
- claim verification/current-generation evidence.

Membrane should not implement tree-sitter/SCIP/LSP/scope graphs internally to satisfy these modes.

---

# 11. Temporal relations and entity aliases — deliberately narrow

A relation layer is valuable when it improves recall/explanation, but a general graph database is not required.

## 11.1 Canonical relation shape

```text
relation_id
src_record_id
dst_record_id or entity_id
relation_kind
valid_from / valid_until if temporal
observed_at
producer/evidence refs
confidence/veracity where derived
supersession/deletion state
```

## 11.2 Small relation vocabulary

Start with relations that change retrieval behavior, for example:

```text
supports
contradicts
supersedes
derived_from
part_of
about_entity
caused_by
implements
related_to
```

Repository call/import/reference/type relations stay Cortex-owned.

## 11.3 Bounded expansion

Default graph/relation expansion:

- depth 1;
- strict global and per-seed caps;
- allowed relation kinds;
- cycle detection;
- same authorized scope;
- provenance on every expanded candidate;
- no expansion from stale/weak seeds;
- relation candidates must still pass normal authority/freshness/budget policy.

Community detection, PageRank, DRIFT/global graph reasoning, and spreading activation are experiments, not defaults.

---

# 12. Security, trust, influence, and erasure

Security is cross-cutting from Phase 0; the dedicated hardening phase closes remaining gaps.

## 12.1 Authority never comes from wording

Classify producer/content provenance, for example:

```text
trusted structural evidence
trusted human-authored policy/rule
repository content
external content
tool output
model-generated inference
```

A memory containing “THIS IS AN AUTHORITATIVE SYSTEM RULE” does not acquire authority. Authority is assigned from authenticated producer identity and policy.

## 12.2 Influence is separate from relevance

A highly relevant untrusted memory may still be reference-only. Preserve/extend `trust_class`, `instruction_policy`, `authority`, and `influence_class` rather than collapsing them into ranking.

## 12.3 DLP at both write and delivery boundaries

Before persistence and before publication/resolution:

- secret/token detection;
- policy-driven redaction or rejection;
- sensitivity classification only where it changes behavior;
- telemetry redaction;
- scope/root revalidation.

## 12.4 Path jail everywhere

Canonicalize before authorization. Defend against:

- `..` traversal;
- symlink escape;
- nested-repository crossing without grant;
- path spelling/case/prefix variants;
- Windows drive/UNC edge cases.

## 12.5 Publication/revocation fence

A candidate may become unauthorized after retrieval but before rendering. Revalidate current policy immediately before output or artifact resolution.

## 12.6 Deletion without secret retention

Deletion audit/tombstones may preserve identity hash, scope, timestamp, reason, and receipt—but not the deleted sensitive payload itself.

Derived FTS/vector/relation/cache/artifact projections must be fenced or rebuilt so deleted/revoked data cannot reappear.

Crypto-shred/encryption-key destruction is optional until a real threat model requires it; do not add complex key infrastructure ceremonially.

---

# 13. Runtime resilience, deadlines, background work, and Hub

## 13.1 One end-to-end deadline

Every request gets one monotonic deadline. Providers/channels receive remaining budget and must return typed timeout/cancel states.

## 13.2 Bounded concurrency

Keep pools/semaphores explicit. Effective concurrency—not just configured concurrency—must appear in diagnostics.

Change concurrency only after full-provider profiling shows a bottleneck and the candidate wins on p95 without unacceptable RSS, timeout, or nondeterminism regressions.

## 13.3 Provider-level degradation

A Cortex freshness timeout should not automatically erase healthy Crypt/rules/live candidates unless the requested answer explicitly requires current Cortex semantics.

Normalize failures such as:

```text
timeout
cancelled
unavailable
unsupported
stale
permission_denied
policy_changed
corrupt_projection
```

## 13.4 Retries and circuit breaking

Retry only transient idempotent operations with bounded backoff inside the original deadline. Never retry deterministic policy denials. Circuit breaking is useful only for repeatedly failing external/resident providers; local deterministic paths should fail directly and visibly.

## 13.5 Background work is idempotent and non-critical-path

Parsing, corpus embedding, FTS rebuild, backup, compaction, migration, reflection, and heavy extraction must not run synchronously on a prompt request.

Every background job has:

- stable job/input identity;
- bounded work cap;
- cancellation;
- crash-safe checkpoint/transaction semantics;
- no-op success when already current;
- typed result/receipt.

## 13.6 Hub owns persistent OS lifecycle

Hub is the user-facing lifecycle owner. Membrane should expose:

- executable/artifact identity;
- readiness/health;
- drain/shutdown;
- child/process identity if needed;
- current storage/runtime identity;
- compatible lifecycle protocol/version.

Do not add a competing standalone “Membrane daemon manager” product surface.

---

# 14. Storage identity, maintenance, recovery, and sync

## 14.1 One storage resolver

Rust runtime, installer, Hub, doctor, backup, and operations should agree on one absolute canonical Crypt/catalog/artifact location and installation/store identity.

## 14.2 Derived projections are disposable; canonical data is not

FTS/vector/relation projections must have explicit rebuild/version metadata. A corrupted projection should trigger typed rebuild/degraded behavior, not database deletion.

## 14.3 Maintenance ownership

One writer/maintenance owner controls migrations/checkpoints/compaction. Readers do not opportunistically migrate or checkpoint.

## 14.4 Required operations

Implement and test:

- read-only inventory that never creates missing DBs;
- integrity/doctor checks;
- live consistent backup;
- clean-machine restore;
- deterministic export/import with schema/version metadata;
- explicit wipe/forget semantics;
- migration preflight and backout;
- WAL/checkpoint diagnostics;
- copy→compact/rebuild→logical-equivalence→atomic-adopt→health→rollback for destructive maintenance.

## 14.5 Sync after local correctness

Current/team sync concepts should remain event/op-based and identity-aware. Do not expand into peer-to-peer federation until:

- one-machine durability is green;
- conflict/supersession semantics are explicit;
- revocation/erasure propagation is proven;
- there is a real multi-machine/team product requirement.

---

# 15. Observability and evaluation are the promotion system

## 15.1 Phase-0 context-quality fixture suite

Freeze cases covering at least:

- exact symbol/path/error lookup;
- conceptual code lookup;
- explicit anchor preservation;
- current source vs stale memory conflict;
- dirty overlay vs committed snapshot;
- cross-file structural question;
- route→handler→schema and test→implementation relations;
- current decision vs superseded decision;
- durable preference/procedure recall;
- negative/failure memory;
- contradictory memory;
- temporal “what was true at T?”;
- lexical-only fallback with embeddings unavailable;
- provider timeout and partial result;
- duplicate content across providers;
- oversized result requiring reduction/externalization;
- resolver-backed source;
- no-relevant-context case;
- secret-bearing source;
- cross-scope isolation;
- revoked/deleted source;
- full provider timeout.

## 15.2 Do not collapse quality into one number

Track separately:

- required-evidence recall@K;
- precision@K where meaningful;
- MRR/nDCG where order matters;
- forbidden/stale evidence admission;
- contradiction miss rate;
- temporal/as-of accuracy;
- source-resolution success;
- explicit-anchor survival (**100%** target);
- scope/ACL violation (**0** target);
- budget reconciliation failure (**0** target);
- transform corruption (**0** target);
- receipt completeness;
- selected/delivered tokens and characters;
- bytes externalized/avoided;
- resolver/refetch rate;
- compaction regret/fidelity;
- p50/p95 warm latency;
- CPU/RSS and DB/index growth;
- provider timeout degradation correctness;
- deterministic replay variance;
- whole-task success where observable.

## 15.3 Local trace + content-free telemetry

Keep rich local traces for diagnosis. Keep exported/aggregate telemetry content-free by default.

Per candidate, retain enough to explain:

```text
which channels found it
channel ranks
policy/authority/freshness class
bounded utility modifiers
estimated/delivered cost
selected/dropped reason
resolver/externalization behavior
verified later outcome when available
```

## 15.4 Controlled experiments and ablation

Every adaptive policy has a version, control, candidate, and rollback. Required experiments include ablation:

- lexical off/on;
- vectors off/on;
- relation expansion off/on;
- outcome modifier off/on;
- diversity off/on;
- new lifecycle thresholds vs current;
- new transform vs exact/raw control.

Do not move thresholds or relax fixtures to make a candidate pass.

## 15.5 Standard/stateful evaluation

Use existing source-ready harnesses and current installed artifacts for:

- LoCoMo;
- LongMemEval;
- BEAM;
- commit-reveal whole-task holdouts;
- poisoning/influence escalation;
- source drift/rename/change;
- session resume;
- backup/restore/crash-boundary recovery;
- Mac/Windows resource tests.

Competitor claims remain vendor-reported until reproduced under the same model/hardware/budget/scoring protocol.

## 15.6 Promotion law

> **No adaptive mechanism ships because it sounds intelligent. It ships because it beats or meets the deterministic control under a frozen quality, safety, latency, and resource gate.**

---

# 16. Exact implementation sequence

The order below reconciles all four guides and the current codebase. Security and observability are cross-cutting, but their dedicated phases close the full operational contracts.

## Phase 0 — one authority, current-state truth, and frozen baseline

**Goal:** prevent architecture churn and make every later change comparable.

**Do:**

1. adopt this guide as `docs/MEMBRANE-IMPLEMENTATION-GUIDE.md`;
2. add a short supersession header to `docs/plans/2026-08-12-membrane-crypt-database-hygiene-and-performance.md` rather than resurrecting deleted `sol.md` lineage;
3. regenerate `docs/MEMBRANE-CURRENT-STATE-MANIFEST.json` from current source/installed truth—it is older than the inspected baseline;
4. freeze current V1 protocol JSON/golden fixtures;
5. freeze packet/order/omission/grant/budget/reconciliation behavior;
6. create the canonical context-quality/eval fixture suite and baseline metrics;
7. freeze feature flags for experimental ranking/lifecycle/transform changes;
8. inventory existing runtime/store modules before creating any new abstraction.

**Primary paths:**

- `docs/MEMBRANE-IMPLEMENTATION-GUIDE.md` — new canonical plan;
- `docs/plans/2026-08-12-membrane-crypt-database-hygiene-and-performance.md` — supersession marker only;
- `docs/MEMBRANE-CURRENT-STATE-MANIFEST.json` — regenerate, do not hand-invent;
- `engine/crates/membrane-testkit/`;
- `engine/crates/crypt-core/src/eval_gate.rs`;
- new `tests/context-quality/` or the nearest existing canonical fixture surface.

**Gate:** baseline is reproducible; security/anchor/budget invariants fail the candidate build if regressed.

---

## Phase 1 — close planner/publication/runtime seams before adding intelligence

**Goal:** ensure there is exactly one final policy/publication path and honest per-provider degradation.

**Do:**

- audit duplicate eligibility/authority/freshness/budget decisions across Python providers, runtime, MCP, and Rust planner;
- make providers stop at typed candidate/evidence output;
- preserve one global budget/lane reconciliation;
- bind a request to one grant/policy epoch and revalidate immediately before output;
- carry one deadline through provider fan-out;
- degrade providers independently where semantically safe;
- expose Hub lifecycle readiness/drain/identity without creating a second OS lifecycle owner.

**Primary existing paths:**

- `engine/crates/membrane-core/src/fusion.rs`
- `engine/crates/membrane-core/src/lane.rs`
- `engine/crates/membrane-core/src/budget.rs`
- `engine/crates/membrane-core/src/reconcile.rs`
- `engine/crates/membrane-runtime/src/admission_policy.rs`
- `engine/crates/membrane-runtime/src/delivery_trace_view.rs`
- `engine/federation/providers/`
- `mcp/server.mjs`
- `mcp/context-renderer-lib.cjs`
- `engine/crates/membrane-protocol/src/types.rs`

**Gate:** the same typed candidate inputs produce equivalent policy decisions across supported entry paths; revoke-during-publication cannot emit stale-authority bytes; one provider timeout is reflected honestly without unnecessary global suppression.

---

## Phase 2 — evidence identity, canonical knowledge records, and ranking-signal sidecar

**Goal:** establish the truth substrate before changing retention/ranking.

**Do:**

- create internal canonical knowledge/evidence types;
- separate logical ID/content hash/evidence ID/source anchor/derivation fingerprint;
- add normalized evidence and record metadata tables;
- add sidecar runtime ranking/use signals;
- backfill existing rows conservatively with explicit legacy/unattributed state;
- preserve existing temporal facts and memory IDs where possible;
- keep V1 public shapes stable unless a consumer requires V2.

**Likely new internal modules:**

- `engine/crates/crypt-core/src/record.rs`
- `engine/crates/crypt-core/src/ranking_signals.rs`
- `engine/crates/crypt-core/src/conflict.rs`

**Existing paths:**

- `engine/crates/crypt-core/src/types.rs`
- `engine/crates/crypt-store/src/memdb.rs`
- `engine/crates/crypt-store/src/temporal.rs`
- `engine/crates/crypt-store/src/scope.rs`
- `engine/crates/crypt-store/src/installation_identity.rs`
- `engine/crates/crypt-store/src/context_telemetry.rs`
- `adapters/provenance/index.mjs`

**Gate:** any recalled durable record can mechanically explain content identity, evidence origin, current resolution state, authority, derivation status, and supersession without depending on mutable ranking fields.

---

## Phase 3 — write admission, conflict/no-op semantics, and explicit lifecycle

**Goal:** make memory lean by construction.

**Do:**

- implement deterministic admission before persistence;
- implement explicit write dispositions including no-op/conflict/supersede/quarantine/reject;
- model negative/failure knowledge without granting instruction authority;
- add lifecycle state and pure policy functions;
- make Dream a reversible consolidation/proposal stage;
- move reinforcement into verified/bounded sidecar signals;
- calibrate thresholds only on held-out data.

**Primary paths:**

- new `engine/crates/crypt-core/src/lifecycle.rs`
- `engine/crates/crypt-core/src/dream.rs`
- `engine/crates/crypt-core/src/effectiveness.rs`
- `engine/crates/crypt-core/src/calibration.rs`
- `engine/crates/crypt-core/src/planner.rs`
- `engine/crates/crypt-store/src/memdb.rs`

**Gate:** duplicate writes are no-op; conflicts preserve evidence; lifecycle transitions are versioned/reversible; no derived summary silently becomes authoritative truth.

---

## Phase 4 — production FTS5/BM25 and explainable retrieval fusion

**Goal:** fix the largest verified retrieval-quality gap without replacing the vector/RRF foundation.

**Do:**

- add FTS5/BM25 projection and migration/rebuild path;
- retain current deterministic lexical fallback;
- separate candidate overfetch from final result limit;
- expose exact, lexical, semantic, temporal, relation, and active-working channels internally;
- preserve RRF baseline;
- retain channel ranks and score decomposition;
- add exact-evidence survival behavior subject to hard policy vetoes;
- add bounded duplicate/diversity suppression after fusion;
- emit retrieval explanation/latency trace;
- add fallback/ablation tests.

**Primary paths:**

- `engine/crates/crypt-core/src/retriever.rs`
- new `engine/crates/crypt-core/src/lexical.rs`
- new `engine/crates/crypt-core/src/retrieval_trace.rs`
- `engine/crates/crypt-core/src/embed.rs`
- `engine/crates/crypt-core/src/graph.rs`
- `engine/crates/crypt-store/src/memdb.rs`
- `engine/crates/crypt-store/src/context_telemetry.rs`

**Gate:** lexical quality improves on frozen cases; vector-unavailable fallback is deterministic; stale/forbidden evidence does not improve rank through similarity; p95/RSS stay inside gate.

---

## Phase 5 — Cortex-backed source anchors and drift verification

**Goal:** make stored code claims mechanically verifiable without duplicating Cortex.

**Do:**

- define Membrane-side anchor/evidence resolution state;
- consume stable Cortex symbol/definition IDs and structural fingerprints;
- support move/rename continuity and ambiguity/missing states;
- bind Cortex generation/base commit to evidence;
- expose query modes for references/impact/failure resolution/claim verification;
- cache only resolution projections that are invalidated by generation/policy changes.

**Primary paths:**

- `engine/federation/providers/cortex.py`
- `engine/federation/providers/test_cortex_provider.py`
- `engine/crates/membrane-provider-sdk/`
- Crypt evidence/record persistence from Phase 2.

**Gate:** changed/renamed/missing/ambiguous code evidence produces deterministic resolution state; no Membrane parser/index stack is introduced.

---

## Phase 6 — artifact-backed reversible Push and context editing

**Goal:** make token reduction loss-bounded and recoverable.

**Do:**

- add a local content-addressed artifact abstraction;
- converge existing `runc`/skeleton/compress/truncate behaviors under one transform contract;
- externalize raw payload before lossy reduction;
- add protected/atomic span classification;
- add query-critical restoration;
- cache derived views by source hash + transform fingerprint;
- integrate resolver-backed accounting into the existing lane/budget receipt.

**Likely new modules:**

- `engine/crates/membrane-runtime/src/artifact.rs`
- `engine/crates/membrane-runtime/src/context_edit.rs`

**Existing paths to converge:**

- `engine/crates/membrane-runtime/src/compress.rs`
- `engine/crates/membrane-runtime/src/compression_provider.rs`
- current runtime transform/truncation/source-resolution code discovered in Phase 0
- `engine/crates/membrane-core/src/lane.rs`
- `engine/crates/membrane-core/src/budget.rs`
- `engine/crates/membrane-core/src/reconcile.rs`
- `mcp/context-renderer-lib.cjs`

**Gate:** no protected evidence corruption; exact raw content remains resolvable; token reduction is measured at unchanged/non-inferior task evidence quality.

---

## Phase 7 — narrow durable temporal relations and entity aliases

**Goal:** obtain relation-aware memory without creating a graph platform.

**Do:**

- preserve `membrane_temporal_fact` as temporal truth;
- add normalized relation rows only for relations that change recall/explanation;
- maintain evidence/validity on edges;
- add bounded one-hop expansion;
- add alias/entity canonicalization where it improves retrieval;
- keep graph as a projection over canonical IDs.

**Primary paths:**

- `engine/crates/crypt-core/src/graph.rs`
- new `engine/crates/crypt-core/src/relations.rs`
- new `engine/crates/crypt-store/src/relations.rs`
- `engine/crates/crypt-store/src/temporal.rs`
- `engine/crates/crypt-store/src/memdb.rs`

**Gate:** relation expansion is scoped, capped, cycle-safe, evidence-bearing, and can be disabled with deterministic fallback.

---

## Phase 8 — session continuity, retention economics, and bounded curation

**Goal:** make useful knowledge strengthen and stale/unused knowledge stop crowding context without destructive guesswork.

**Do:**

- create bounded session-end episodic packets/checkpoints;
- formalize working/hot capacity and expiry;
- implement hysteretic promotion/demotion and retention strength;
- schedule bounded idempotent maintenance;
- preserve reversible consolidation/derived lineage;
- keep host transcript ownership outside Membrane.

**Primary paths:**

- `engine/crates/membrane-runtime/src/checkpoint.rs`
- current working-context/scratchpad runtime surfaces
- `engine/crates/crypt-core/src/lifecycle.rs`
- `engine/crates/crypt-core/src/dream.rs`
- `engine/crates/crypt-store/src/maintenance_exec.rs`

**Gate:** session resume improves without transcript duplication; lifecycle cannot oscillate rapidly; maintenance never blocks prompt-critical path.

---

## Phase 9 — DLP, influence policy, publication race closure, and erasure

**Goal:** close trust boundaries before broader content modalities/sync.

**Do:**

- DLP/sensitivity checks at persistence and delivery;
- producer-based authority and influence enforcement;
- path-jail/symlink/case edge tests;
- revalidation at artifact/source publication;
- erasure fence across FTS/vector/relation/cache/artifact projections;
- delete audit without secret payload retention;
- explicit destructive capability checks.

**Primary paths:**

- `engine/crates/membrane-runtime/src/admission_policy.rs`
- `engine/crates/crypt-store/src/scope.rs`
- `engine/crates/crypt-store/src/memdb.rs`
- `mcp/authorization.mjs`
- source/artifact resolver surfaces
- security tests/threat model.

**Gate:** zero cross-scope leaks; revoked/deleted sensitive content cannot be republished through stale projections or resolvers.

---

## Phase 10 — storage identity, doctor/repair, backup/export/import/restore/wipe

**Goal:** make durable memory operationally trustworthy.

**Do:**

- one canonical store resolver/identity;
- enrich doctor/runtime receipts with schema/journal/WAL/store identity;
- read-only inventory that cannot create DBs;
- projection rebuild/repair;
- live backup and clean-machine restore;
- deterministic export/import;
- explicit wipe/forget policy;
- migration preflight/backout;
- crash-boundary and atomic-adopt tests;
- keep team sync event-based and subordinate to local correctness.

**Primary paths:**

- `engine/crates/crypt-store/src/installation_identity.rs`
- `engine/crates/crypt-store/src/maintenance_exec.rs`
- `engine/crates/crypt-store/src/db.rs`
- `engine/crates/crypt-store/src/memdb.rs`
- `engine/crates/crypt-store/src/team_sync.rs`
- runtime doctor/diagnostic bundle surfaces.

**Gate:** backup/restore preserves logical key sets, evidence/supersession/event continuity, and recall equivalence; corruption is a typed repairable state, not a reason to delete the DB.

---

## Phase 11 — benchmark-gated adaptive retrieval and staged multimodal

**Goal:** add intelligence only where the deterministic substrate has a measured gap.

Experiments, in order:

1. deterministic retrieve/no-retrieve gate;
2. local rule-based query expansion for temporal/pronoun/change intent;
3. MMR diversity over already-eligible fused candidates;
4. bounded relation expansion variants;
5. local cross-encoder reranker in shadow mode;
6. LLM-assisted extraction/query expansion/reflection only if local methods leave a measured gap;
7. staged document/image/audio/video extraction after artifact security/resolution is green;
8. multimodal embeddings last.

**Gate for every experiment:** frozen holdout non-inferiority on evidence/safety, explicit latency/RSS/token budget, versioned policy, automatic rollback/no-promotion on regression.

---

## Phase 12 — closed-loop evaluation, installed qualification, and release closure

**Goal:** prove the system that ships, not merely source-level capability.

**Do:**

- record candidate journey and verified outcomes;
- calibrate by task class/memory family/provider/score band;
- run ablations and control/candidate cohorts;
- run retrieval, stateful memory, poisoning, compression, source drift, recovery, and whole-task suites;
- run current installed Mac + Windows qualification;
- verify all ten MCP tools and supported adapter contracts from installed artifacts;
- measure p50/p95 latency, CPU/RSS, tokens/bytes, DB/index growth, backup age, degraded-provider behavior;
- publish support/competitor claims only from current receipts.

**Gate:** supported client/platform paths pass install→discovery→grant→context→source resolve→memory proposal→feedback→checkpoint→restart/degrade→upgrade→uninstall; no major external surface is added before the current surface is qualified.

---

# 17. Migration strategy

## 17.1 Additive first

Add new tables/columns/indexes without destroying current readable state. Prefer normalized side tables for evidence, ranking signals, relations, and lifecycle data.

## 17.2 Preserve stable IDs where possible

Backfill explicit `legacy_unattributed`/unknown states rather than manufacturing false provenance.

## 17.3 Dual-read only temporarily

When introducing FTS, richer records, or new relation projections, run old/new paths in replay/shadow until equivalence/quality gates pass. Do not keep two permanent authorities.

## 17.4 One cutover flag per capability

Examples:

```text
lexical_v2
knowledge_record_v2_internal
relation_retrieval_v1
artifact_externalization_v1
lifecycle_policy_v1
```

Flags are bounded migration tools, not permanent configuration sprawl.

## 17.5 Rollback preserves newly written data

A rollback may disable new reads/policies; it must not silently discard records written under the new schema. Migration/backout strategy is part of the feature definition.

## 17.6 Rebuildable projections

FTS/vector/relation/derived transform caches can be rebuilt from canonical rows/artifacts. Their corruption cannot invalidate durable truth.

## 17.7 Freeze thresholds during qualification

Never lower recall/security/latency sample requirements or change decay/ranking coefficients mid-run to obtain green results.

---

# 18. File-level implementation map

The map below is intentionally conservative: it names verified existing source files plus a small set of new modules that create real ownership boundaries. Do not create a new crate merely for aesthetics.

| Path | State at inspected baseline | Canonical change |
|---|---|---|
| `engine/crates/membrane-protocol/src/types.rs` | existing | Preserve five V1 shapes; add versioned wire semantics only when required |
| `engine/crates/membrane-core/src/fusion.rs` | existing | Keep authority/freshness discipline; integrate channel/provider rank evidence without raw-score flattening |
| `engine/crates/membrane-core/src/lane.rs` | existing | Represent artifact/context-edit results through existing delivery lanes |
| `engine/crates/membrane-core/src/budget.rs` | existing | Account for protected content and resolver-backed economics without second budget |
| `engine/crates/membrane-core/src/reconcile.rs` | existing | Reconcile new omission/resolution/transform states |
| `engine/crates/crypt-core/src/retriever.rs` | existing | Stage exact/lexical/vector/temporal/relation/working retrieval; preserve fallback/RRF baseline |
| `engine/crates/crypt-core/src/lexical.rs` | **new** | FTS5/BM25 adapter, code-aware lexical normalization, deterministic fallback integration |
| `engine/crates/crypt-core/src/retrieval_trace.rs` | **new** | Local per-channel ranks/latency/explanation trace |
| `engine/crates/crypt-core/src/record.rs` | **new** | Canonical rich knowledge-record model separate from hot retrieval projection |
| `engine/crates/crypt-core/src/ranking_signals.rs` | **new** | Mutable usefulness/retention sidecar policy |
| `engine/crates/crypt-core/src/conflict.rs` | **new** | Duplicate/no-op/conflict/supersession dispositions |
| `engine/crates/crypt-core/src/lifecycle.rs` | **new** | Pure versioned lifecycle/retention decisions and hysteresis |
| `engine/crates/crypt-core/src/dream.rs` | existing | Reversible consolidation/proposal phase only |
| `engine/crates/crypt-core/src/effectiveness.rs` | existing | Bind verified outcomes to sidecar usefulness, not canonical content |
| `engine/crates/crypt-core/src/calibration.rs` | existing | Calibrate policy thresholds from held-out data |
| `engine/crates/crypt-core/src/graph.rs` | existing | Keep bounded memory relation projection; do not absorb code graph |
| `engine/crates/crypt-core/src/relations.rs` | **new if graph.rs cannot cleanly own typed policy** | Small relation vocabulary/query controls; avoid duplicate graph layers |
| `engine/crates/crypt-core/src/eval_gate.rs` | existing | Membrane-level regression promotion gate |
| `engine/crates/crypt-store/src/memdb.rs` | existing | Migrations for record/evidence/signal/FTS/relation projections |
| `engine/crates/crypt-store/src/temporal.rs` | existing | Preserve temporal fact semantics; integrate with richer record/evidence model |
| `engine/crates/crypt-store/src/relations.rs` | **new** | Durable typed temporal relation rows if not kept in `memdb.rs` initially |
| `engine/crates/crypt-store/src/scope.rs` | existing | Scope/influence/sensitivity support and publication checks |
| `engine/crates/crypt-store/src/context_telemetry.rs` | existing | Content-free retrieval/lifecycle/economics telemetry |
| `engine/crates/crypt-store/src/maintenance_exec.rs` | existing | Bounded lifecycle/index/backup jobs |
| `engine/crates/crypt-store/src/installation_identity.rs` | existing | Canonical storage/artifact identity in receipts/backup/import |
| `engine/crates/crypt-store/src/team_sync.rs` | existing | Keep event/op semantics; no P2P expansion before local gates |
| `engine/crates/membrane-runtime/src/admission_policy.rs` | existing | Persist/publish influence, sensitivity, and policy epoch enforcement |
| `engine/crates/membrane-runtime/src/compress.rs` | existing | Integrate under one reversible transform ladder |
| `engine/crates/membrane-runtime/src/compression_provider.rs` | existing | Keep optional/bounded and receipt-visible |
| `engine/crates/membrane-runtime/src/delivery_trace_view.rs` | existing | Surface channel/admission/representation explanation |
| `engine/crates/membrane-runtime/src/checkpoint.rs` | existing | Bounded explicit session continuity |
| `engine/crates/membrane-runtime/src/artifact.rs` | **new** | Governed content-addressed artifact identity/resolution |
| `engine/crates/membrane-runtime/src/context_edit.rs` | **new** | Ordered externalize/reduce/restore contract |
| `engine/federation/providers/cortex.py` | existing | Consume stable code evidence/query modes; no local parser |
| `engine/federation/providers/test_cortex_provider.py` | existing | Drift/ambiguity/generation/degradation contract tests |
| `mcp/server.mjs` | existing | Remain thin; expose typed capability/degradation, not duplicate ranking policy |
| `mcp/context-renderer-lib.cjs` | existing | Render/reconcile resolver-backed/protected views under one budget |
| `mcp/authorization.mjs` | existing/current surface expected | Publication/root/scope authorization seam; keep policy centralized |
| `docs/MEMBRANE-CURRENT-STATE-MANIFEST.json` | existing but stale to inspected main | Regenerate from source/installed state |
| `docs/architecture.md` | generated existing | Never hand-edit; regenerate after implementation |
| `docs/plans/2026-08-12-membrane-crypt-database-hygiene-and-performance.md` | existing, dangling retired authority | Mark superseded by this guide |
| `tests/context-quality/` | **new or merge into existing canonical eval tree** | Frozen context-quality/behavior fixtures and baseline results |

Before creating any “new” module, Phase 0 must check whether the current repo already has the same ownership under another filename. Reuse wins over taxonomy.

---

# 19. Explicitly rejected or deferred architecture

These ideas may be good in their source systems and still be wrong for Membrane now.

## Reject as Membrane architecture

- generic agent/workflow/orchestration framework;
- PTY/coding-agent execution loop;
- duplicate Cortex parser/AST/symbol/call graph;
- external vector database as required storage;
- external graph database as required storage;
- Redis/distributed queue infrastructure for the local workstation product;
- general RDF/SPARQL/ontology/reasoner platform;
- generic backend factory/plugin explosion without two real implementations;
- separate per-provider final attention budgets;
- raw cross-provider weighted score arithmetic;
- mandatory LLM planning before local recall;
- mandatory LLM summarization/reflection in prompt-critical path;
- memory content self-elevating to instruction authority;
- full host conversation-history ownership;
- dozens of memory tiers/states without distinct behavior;
- vendor competitor source trees as product dependencies.

## Defer behind evidence

- learned cross-encoder reranker;
- LLM query expansion;
- LLM memory extraction/reflection;
- MMR as default;
- broad graph/community/PageRank/DRIFT traversal;
- HyDE-like query synthesis;
- multimodal embeddings;
- product-quantized vectors solely for scale before measured pressure;
- crypto-shred/key hierarchy beyond current threat model;
- peer-to-peer/team memory federation;
- remote graph/vector backends;
- automatic “self-improvement” application.

The default is not “never.” The default is **not until a frozen benchmark demonstrates a material gap and the candidate closes it within safety/resource constraints**.

---

# 20. 60-repository semantic absorption ledger

This ledger is the coverage proof. It maps every exact entry from `competitor.md` to the semantic capability it contributes. It is deliberately not a 600-row backlog: equivalent donor mechanisms are implemented once under Membrane ownership.

| Repository | Primary absorption into canonical plan | Disposition / owner |
|---|---|---|
| `AbanteAI/archive-old-cli-mentat` | undo/redo, context controls, end-task eval | Adapt reversibility/evaluation; coding agent/TUI out of scope |
| `AlmanacCode/codealmanac` | scheduled knowledge lifecycle, transcript mining, evidence per claim, validation, no-op success | Strong absorb into lifecycle/retain/provenance |
| `Brain0-ai/brain0` | line/source provenance, drift detection, DLP, stable symbol identity, attestations, crypto-shred | Absorb provenance/DLP; stable code symbols via Cortex |
| `Consiliency/treesitter-chunker` | AST chunks, token budgets, stable symbol graph, incremental/parallel chunking, packing priority | Cortex owner; Membrane consumes outputs |
| `DeusData/codebase-memory-mcp` | deep semantic code extraction, graph analysis, coverage honesty, incremental reindex | Absorb through Cortex, not Crypt |
| `Ivy-Interactive/Ivy-Tendril` | process/job supervision, retries, usage accounting, health | Absorb through Hub; agent execution itself out of scope |
| `James-Chahwan/repo-graph` | failure-signal resolution, blast radius, cross-stack tracing, entry points, coverage honesty, PageRank | High-value via Cortex; PageRank benchmark-gated |
| `LangbaseInc/baseai` | local resident server, unified local/prod pipe, typed boundaries, streaming | Adapt resident/typed runtime concepts; generic agent loop/UI out of scope |
| `Lucas2944/prpack` | task-specific context packaging, base/head completeness, adjacent tests, content hygiene, spend gate | Absorb as evaluation/task-pack design; PR product features not Membrane core |
| `MCrank/code-compress` | production FTS5, incremental symbol indexing, budgeted context, references/blast radius | FTS absorb in Membrane; symbol graph via Cortex |
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
| `codegraph-ai/CodeGraph` | bi-temporal memory, memory↔code links, BM25+vector+graph fusion, design claim verification | Adapt temporal/link ideas; code graph owner Cortex |
| `cq27-dev/rag-rat` | deterministic dream guards, verify pass, SCIP/LSP oracles, signed op-log, evidence distill, peer sync | Absorb lifecycle guards; SCIP/LSP through Cortex; op-log ideas for sync |
| `deepset-ai/haystack` | token-aware compaction, tool-result pruning/offload, typed state, filter protocol, structured eval | Strong absorb reversible Push/eval; generic framework out of scope |
| `drona23/claude-token-efficient` | real provider-usage A/B/C benchmarking, behavior-targeted tests, pre-compaction save | Strong absorb into evaluation and session continuity |
| `emulo` | content-addressed identity, strict validation, policy gates, fail-closed redaction, atomic storage, proof harness | Strong absorb into identity/security/eval |
| `getzep/graphiti` | episodes, bi-temporal edges, multi-strategy search, bounded BFS, resolution, sagas | Absorb episodes/temporal relations narrowly; graph driver sprawl rejected |
| `getzep/zep` | ingest pipeline, boundary-aware splitting, provenance episodes, alias canonicalization, injection hardening, indexing-lag tolerance | Strong absorb retain/entity/security/retrieval-fallback concepts |
| `greplica` | code-anchored claims, fingerprints/drift, parent-chain memory commits, proposal writes, reconciliation | Absorb evidence/claim lifecycle; code anchors via Cortex |
| `headroomlabs-ai/headroom` | CCR reversible compression, per-tool interception, proactive context expansion, fidelity eval, savings audit | Core source for ArtifactRef/query verifier/Push economics |
| `hindsight` | sentence/fact typing, temporal ranges, entity resolution, causal links, DLP, async retain, export/audit | Strong absorb into Crypt model/lifecycle/security |
| `honcho` | explicit vs derived observations, bounded derivation/Dreamer, trigger gates, telemetry | Absorb derived-record distinction + schedule gates |
| `juspay/code-review-graph-rescript` | typed code graph, diff impact, graph snapshots, flow tracing, RRF, graph evals | Absorb through Cortex; RRF/eval concepts in Membrane |
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
| `quantmew/context8` | AST hierarchical chunks, hash-based incremental indexing, cancellation, commit pinning | Absorb through Cortex; cancellation/freshness into provider contract |
| `rohitg00/agentmemory` | validated observation compression, hard working-memory budgets, leases, provenance verification, eval discipline | Absorb compression/eval/locking patterns; Crypt/runtime |
| `rtk-ai/rtk` | never-worse filters, data-class truncation, savings economics, hook integrity | Strong absorb Push guards/economics/security |
| `run-llama/llama_index` | memory blocks, transformation hashes, ingestion cache, property-graph subretrievers | Absorb transformation identity; backend/LLM graph zoo rejected |
| `semantic` | generic AST, scope/reference resolution, LSP tags | Cortex-only; do not absorb parser into Membrane |
| `semantica` | PROV provenance, conflict workflows, temporal layer, version checksums | Absorb provenance/conflict/temporal concepts; reasoner/ontology platform rejected |
| `shihanwan/memonto` | ontology/triples, delta updates, vector→graph expansion | Delta/typed relation concept; open ontology/SPARQL/script writes rejected |
| `supermemoryai/supermemory` | spaces/scopes, save-or-forget semantics, typed citations, document-centric memory | Adapt scope/write/citation ideas; extension/UI ecosystem out of scope |
| `topoteretes/cognee` | pre-store sanitization, progressive retrieval disclosure, session distillation, feedback weighting | Absorb sanitization/distillation/feedback; avoid graph-platform expansion |
| `vanna-ai/vanna` | tool-usage memory, lifecycle hooks, error recovery, audit hashing, expected-outcome eval | Adapt feedback/procedural memory/eval; UI/integration count not a goal |
| `volcengine/OpenViking` | patch-merge updates, hierarchical retrieval, hotness, observers, privacy lifecycle, benchmark suite | Absorb update/hotness/metrics patterns; custom RAG filesystem rejected |
| `yvgude/lean-ctx` | token envelopes, capability/policy compatibility, normalized failures, routing receipts, holdout gates, savings ledger | Strong absorb into contracts/experiments/receipts |

---

# 21. Definition of done

Membrane is in the intended “best shape” only when all of the following are simultaneously true.

## Planner / Pull

- one canonical planner owns grant/scope, authority/freshness, fusion, global budget, publication, omissions, and receipts;
- production lexical retrieval is indexed BM25/FTS5 or a benchmark-proven local replacement;
- exact, lexical, semantic, temporal, relation, and working channels degrade independently and explainably;
- cross-provider raw scores are never treated as one probability;
- adaptive retrieval features are enabled only where holdouts prove value;
- every selected and important rejected candidate has an explanation path.

## Persist

- canonical content, evidence, temporal state, lifecycle state, and mutable usefulness signals are separated;
- admission happens before durable storage;
- no-op, conflict, supersession, quarantine, expiry, forget, restore, and rejection are first-class outcomes;
- negative/failure knowledge is representable without becoming instruction authority;
- consolidation is reversible and derived records retain exact parents/evidence;
- session continuity works without storing the whole host transcript as truth.

## Push

- large content is externalized/reduced through one reversible ladder;
- raw source is governed and recoverable;
- task-critical identifiers/errors/citations/policy spans survive or are exactly restored;
- transforms have source/output hashes, fingerprints, savings, and failure receipts;
- token savings are evaluated against task/evidence quality, not compression ratio alone.

## Cortex boundary

- Cortex supplies stable code identities/ranges/relations/impact and current generation evidence;
- Membrane performs scope/authority/freshness/admission and source-resolution policy;
- changed/moved/ambiguous/missing code evidence is mechanically distinguished;
- no second code parser/graph stack exists in Membrane.

## Security

- DLP/influence policy exists at persistence and delivery/resolver boundaries;
- provider/model text cannot self-authorize;
- path/root/symlink/case escape tests are green;
- revoke/delete races cannot publish stale bytes;
- erased sensitive content cannot reappear from FTS/vector/relation/cache/artifact projections.

## Operations

- Hub owns persistent user-facing lifecycle;
- prompt requests do not start heavyweight corpus/index/backup/migration jobs;
- background jobs are bounded, cancellable, idempotent, and crash-safe;
- storage identity is unambiguous;
- doctor/repair, backup/export/import/restore/wipe are tested operations;
- corruption/degraded projections are typed operational states, not “delete the DB and hope.”

## Evidence and qualification

- the Phase-0 baseline and regression suite are checked in and reproducible;
- every major capability has source, behavior, operational, and rollback proof;
- LoCoMo/LongMemEval/BEAM/whole-task/stateful/security/recovery suites run against current artifacts;
- Mac and Windows installed paths are qualified;
- performance reports real p50/p95/RSS/token/byte/index data;
- competitor claims are labeled vendor-reported until reproduced;
- the repository has one active implementation authority rather than a lineage of competing plan documents.

---

# 22. Adoption steps

When this document is accepted as the implementation authority:

1. place it at `docs/MEMBRANE-IMPLEMENTATION-GUIDE.md`;
2. add `Superseded by: ../MEMBRANE-IMPLEMENTATION-GUIDE.md` (correct relative path as appropriate) to the August 12 plan without deleting its historical content;
3. do not restore `sol.md`, `solimplement.md`, or `final_absorption.md`;
4. keep root `competitor.md` as the exact 60-source coverage index;
5. keep local `/repos/` clones ignored and outside release/product dependency graphs;
6. regenerate the current-state manifest before implementation begins;
7. freeze the Phase-0 evaluation baseline;
8. implement one phase at a time, with additive migrations, rollback, and explicit exit gates;
9. update generated `docs/architecture.md` only by its generator after source changes land;
10. retire this guide only when its Definition of Done is closed or an explicitly versioned successor replaces it.

---

## Final implementation doctrine

The four syntheses and 60-source corpus point to the same design principle:

> **The strongest context system is not the one with the most memory types, graph algorithms, agents, tools, or vector backends. It is the one that can preserve exact evidence, retrieve the smallest current authoritative subset, reduce everything else reversibly, maintain durable knowledge without confusing derived text with truth, recover from failure without data loss, and prove every inclusion, omission, transformation, lifecycle change, and degradation under the user's current authority.**

Membrane already has the right control-plane spine. The path to “best possible shape” is disciplined completion of that spine, not architectural replacement.
