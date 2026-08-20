# Membrane — Canonical Architecture and Implementation Doctrine

**Status:** proposed successor canonical authority for `Orthic-Labs/Membrane`  
**Architecture review baseline:** `main` at `50e6bb22ab7518a98d3b5bc730c6913d338c7d21` (2026-08-19 review)  
**Baseline policy:** the SHA above is an evidence snapshot, not a permanent “current main” claim. Re-read the tree and freeze a new baseline before implementation tickets.  
**Supersedes as architecture/implementation authority:** prior Membrane improvement guides, absorption guides, workplans, and the attached `MEMBRANE-IMPLEMENTATION-GUIDE-FINAL.md` once this successor is adopted.  
**Does not replace runtime product truth:** `README.md`, generated `docs/product.md`, generated `docs/architecture.md`, `docs/protocol.md`, or `AGENTS.md`. Those must continue to describe landed code.  
**Research provenance:** earlier competitor ledgers and research documents remain evidence inputs, not parallel implementation authorities.

---

## 0. Executive decision

Membrane does not need a larger architecture. It needs a sharper one.

The repository already contains the differentiated spine:

- five typed public protocol shapes;
- a provider-neutral admission planner;
- Push / Pull / Persist as one context economy;
- typed freshness, authority, degradation, omissions, and receipts;
- one cross-provider attention ceiling;
- local-first Crypt storage and retrieval;
- Cortex as the repository/code evidence provider;
- a resident federation path;
- Application / Control / Data process planes;
- host adapters, MCP, hooks, and CLI surfaces.

The final system should make one idea mechanically true:

> **Membrane decides what deserves the agent's limited attention now, in what form, under whose authority, and records exactly why.**

Cortex determines repository evidence and repository truth.  
Crypt preserves durable knowledge.  
Other providers own their evidence.  
Hub owns OS/process lifecycle.  
Legion / OmniRouter / hosts own agent execution and orchestration.  
Membrane owns context policy.

The product objective is:

> **Return the smallest sufficient, current, authoritative evidence set for the task, under a hard attention and deadline budget, with a receipt for every material inclusion, omission, transformation, and degradation.**

Memory is one context source. Retrieval is one mechanism. Compression is one mechanism. The product is the governed context decision.

---

# 1. What Membrane is

Membrane is a **local-first context control plane and context compiler** between an agent and heterogeneous evidence sources.

Given:

- a task;
- a `ScopeGrant`;
- explicit anchors;
- task/session/repository identity;
- the current source state;
- a model/host attention budget;
- a request deadline;

Membrane:

1. determines what evidence classes are required to answer safely and usefully;
2. chooses which provider capabilities are worth invoking;
3. acquires evidence under bounded staged retrieval;
4. normalizes evidence without flattening source semantics;
5. rejects evidence that is out of scope, unauthoritative, stale, unsafe, invalid, revoked, or otherwise ineligible;
6. determines whether the evidence is sufficient;
7. fuses and deduplicates eligible evidence;
8. fills the attention budget breadth-first, then spends remaining budget on marginal utility;
9. chooses the cheapest faithful representation for each admitted unit;
10. revalidates authority immediately before publication;
11. emits a `ContextPacket`;
12. emits a content-free `ContextReceipt`;
13. records later outcome signals without allowing feedback to become truth.

Membrane therefore optimizes:

```text
maximize:
    task success
    required-evidence coverage
    authority
    freshness
    evidence fidelity

minimize:
    delivered tokens
    redundant evidence
    irrelevant evidence
    retrieval/tool calls
    latency
    unrecoverable transformation

subject to:
    ScopeGrant
    repository confinement
    authority
    freshness
    epistemic validity
    security / DLP
    deadline
    attention ceiling
    no silent omission
```

A useful shorthand is:

```text
Context Utility
───────────────
Attention Cost
```

but utility is not one global score. It is constrained by hard policy first.

---

# 2. What Membrane is not

Membrane is not:

- a coding-agent harness;
- a model router;
- a multi-agent orchestrator;
- a PTY/runtime framework;
- a second code parser or code graph;
- a second Cortex;
- a general graph database;
- a remote RAG platform;
- a conversation-transcript owner;
- a universal memory operating system;
- a generic background-job platform;
- a prompt-optimization product;
- a browser-automation system;
- a hosted retrieval dependency;
- an ontology/RDF/SPARQL platform;
- a multimedia ingestion platform.

The architecture must reject scope growth that does not improve the core objective.

---

# 3. Canonical ownership map

| Capability | Canonical owner | Membrane responsibility |
|---|---|---|
| Task authority / grant validation | Membrane | Validate and fail closed |
| Evidence requirements | Membrane planner | Determine what must/usefully may be known |
| Provider acquisition plan | Membrane planner | Choose provider capabilities, bounds, stages |
| Final eligibility | Membrane planner | Scope, trust, authority, freshness, validity, security |
| Sufficiency | Membrane planner | Determine whether required evidence dimensions are covered |
| Fusion / dedupe / diversity | Membrane planner | One final policy path |
| Attention admission | Membrane planner | One global budget; coverage floor then depth |
| Representation choice | Membrane Push / planner boundary | Native/rendered/resolver/metadata and faithful reduction |
| Publication revalidation | Membrane runtime | Recheck authority/policy immediately before bytes leave |
| Omissions / receipts | Membrane | Explain every material decision |
| Durable knowledge | Crypt | Store governed long-lived knowledge |
| Lexical/vector/temporal durable-memory retrieval | Crypt | Produce typed evidence candidates |
| Memory conflict / lifecycle / supersession | Crypt | Persist and expose typed state |
| Code parsing / AST / symbols / references / calls / imports / types | Cortex | Membrane consumes typed evidence only |
| Repository stable identity / source spans / generations | Cortex | Consume and policy-evaluate |
| Code-anchor relocate / re-anchor / moved/ambiguous/missing | Cortex | Call Cortex resolution; do not reimplement |
| Code/document truth comparison | Cortex | Consume contradictions and coverage state |
| Current Git/worktree facts | Git/live provider | Consume current evidence |
| Rules / policy evidence | Rule provider / workspace policy owner | Consume without allowing text to self-authorize |
| Audit findings | Audit | Consume typed findings |
| Architectural decisions/plans | Decision/architecture provider | Consume as evidence, never current-code truth |
| OS startup / child processes / restart/backoff | Hub | Expose readiness/drain/identity only |
| Agent execution / model routing | Legion / OmniRouter / host | Out of scope |
| Host conversation compaction | Host | Membrane may supply artifacts/context, not own transcript |
| Immutable raw reduction artifacts | Membrane | Govern content-addressed recoverability |

No provider may become a second final-policy owner.

---

# 4. Locked architectural invariants

Breaking an invariant requires an explicit architecture decision, migration/compatibility analysis, and a new frozen evaluation baseline.

1. **One final planner.** Providers generate evidence; Membrane decides attention.

2. **Five public V1 shapes remain stable until a real consumer requires V2.**
   - `ScopeGrantV1`
   - `ContextCandidateSetV1`
   - `ContextPacketV1`
   - `ContextReceiptV1`
   - `KnowledgeEmissionV1`

3. **Use a richer internal IR instead of casually expanding the public protocol.**

4. **Hard policy precedes relevance.**
   Scope, authority, freshness, validity, revocation, quarantine, influence, and sensitivity cannot be repaired by a high similarity score.

5. **Unrelated scores are never flattened into one probability.**
   BM25, cosine, Cortex confidence, rule priority, graph completeness, feedback, and freshness are different signals.

6. **One attention ceiling.**
   Provider caps bound acquisition cost. Final admission is global.

7. **Final attention protection is evidence-class based, not permanently provider-name based.**
   Existing memory/skill reserved lanes remain a migration control until the evidence-class policy is qualified.

8. **Cortex owns repository semantics and re-anchoring.**
   Membrane never implements a second parser, symbol graph, structural fingerprint engine, or code re-anchor ladder.

9. **Crypt owns durable knowledge.**
   Membrane does not turn every event, tool result, candidate, or transcript turn into durable memory.

10. **Admission occurs before persistence.**
    “Store everything, clean later” is not allowed.

11. **Conflict is not overwrite.**
    Exact duplicate, supersession, simultaneous conflict, weak inference, and uncertain identity are distinct dispositions.

12. **Feedback is not truth.**
    `used`, `ignored`, `helped`, `contradicted` may change retrieval pressure only inside policy/authority bounds.

13. **No silent omission.**
    Every timeout, cap, rejection, dedupe, fallback, unavailable provider, transformation, or budget drop has a typed reason.

14. **No lossy transformation without recoverability or an explicit incomplete result.**

15. **No prompt-critical model dependency.**
    Ordinary planning, sufficiency, admission, reduction, and publication are deterministic.

16. **One absolute request deadline.**
    Every stage consumes remaining time; no nested stage invents a new independent deadline.

17. **Provider failure is local degradation.**
    A failed provider does not fabricate context and does not erase healthy lanes unless it is a declared hard prerequisite.

18. **Current user authority wins.**
    Repository text, memory, model output, generated docs, and remote content cannot self-elevate to instruction authority.

19. **Generated docs remain generated.**

20. **No adaptive feature promotes without a frozen evaluation delta.**
    The simplest sufficient implementation wins.

21. **Current-state claims are perishable.**
    Re-read the source tree before tickets.

---

# 5. The canonical internal model

The public protocol remains small. Internally, Membrane needs enough structure to make correct policy decisions without flattening evidence.

## 5.1 EvidenceRequirement

A task is not assigned one mutually-exclusive semantic label and then routed from that label.

Instead the planner derives a multidimensional requirement set.

```text
EvidenceRequirement
├─ id
├─ dimension
├─ necessity: required | useful | optional | irrelevant
├─ acceptable authority classes
├─ acceptable freshness classes
├─ exactness requirement
├─ coverage requirement
├─ max acquisition cost
├─ deadline sensitivity
└─ reason code
```

Initial dimensions should remain small and behavior-bearing:

```text
current_code
change_impact
exact_source
current_worktree
policy
prior_decision
durable_knowledge
document_truth
audit_findings
history
explicit_anchor
```

A task may require several simultaneously.

Example:

```text
"Update the authentication migration docs and verify they match implementation"

current_code       required
document_truth     required
policy             useful
prior_decision     useful
history            optional
```

A diagnostic `taskClass` may still exist for telemetry, fixtures, and conservative defaults. It is not the canonical semantic routing primitive.

## 5.2 AcquisitionPlan

```text
AcquisitionPlan
├─ requirement set
├─ provider capabilities selected
├─ stage order
├─ provider bounds
├─ Cortex policy/hops/paths
├─ acquisition cost ceilings
├─ deadline reserve
└─ fallback policy
```

Providers are chosen because they can satisfy requirements, not because their names are permanently embedded in ranking policy.

## 5.3 EvidenceUnit

The internal compiler IR:

```text
EvidenceUnit
├─ canonical unit id
├─ provider
├─ source kind
├─ semantic / atomic group id
├─ exact source/evidence refs
├─ source hash
├─ authority
├─ trust class
├─ instruction policy
├─ sensitivity
├─ freshness
├─ epistemic state
├─ resolution state
├─ provider-local ranks/signals
├─ evidence completeness
├─ coverage dimensions
├─ representation alternatives
├─ token/byte costs by representation
├─ recoverability
├─ omission/degradation metadata
└─ provenance / derivation
```

A complete Cortex path is one atomic `EvidenceUnit`, not independent graph nodes.

## 5.4 EvidenceCoverage

```text
EvidenceCoverage
├─ requirement id
├─ status: satisfied | partial | missing | contradictory | stale | unsafe | unavailable
├─ supporting evidence ids
├─ authority/freshness assessment
└─ typed reason
```

Sufficiency is computed from coverage, not a generic “relevance probability.”

---

# 6. Canonical Pull stage order

This stage order is the core product.

## Stage 0 — Normalize request and authority

Resolve:

- task/session/request/trace identity;
- repository/worktree;
- `ScopeGrant`;
- explicit files/symbols/anchors;
- host/model budget;
- one absolute deadline;
- publication policy epoch.

Extract deterministic query signals:

- identifiers;
- paths;
- hashes;
- quoted strings;
- stack traces;
- error codes;
- dates;
- change/impact/why/previous/decision signals.

These signals may influence acquisition. They never create authority.

## Stage 1 — Derive evidence requirements

Build the multidimensional `EvidenceRequirement[]`.

No LLM in the hot path.

The safe fallback for ambiguity is broader evidence requirements, not a confidently wrong single label.

## Stage 2 — Build acquisition plan

Map requirements to available provider capabilities.

The plan declares:

- initial providers;
- staged providers;
- Cortex traversal policy if needed;
- per-provider acquisition bounds;
- fallback behavior;
- deadline reserve.

P0 migration may use current provider names. End-state planning is capability/requirement based.

## Stage 3 — Acquire the cheapest mandatory evidence first

Start with low-cost, high-authority evidence that commonly satisfies hard requirements:

- grant / explicit anchors;
- applicable rules/policy;
- live Git/worktree identity;
- exact-path/identifier evidence;
- Cortex structural recall when repository semantics are required.

Do not blindly fan out to every provider because the provider exists.

## Stage 4 — Normalize to EvidenceUnits

Provider-specific internal schemas terminate at the adapter.

No provider database layout leaks into the planner.

Preserve:

- source semantics;
- provenance;
- omissions;
- typed degradation;
- atomic grouping;
- generation/revision;
- exact evidence refs.

## Stage 5 — Hard eligibility

Before any soft fusion:

- repository scope;
- grant;
- ACL/policy;
- trust;
- instruction influence;
- sensitivity;
- quarantine/revocation/deletion;
- temporal validity/expiry;
- source resolution state;
- freshness constraints;
- required authority.

Nothing ineligible can be resurrected by ranking.

## Stage 6 — Authority and freshness ordering

Within eligible evidence:

- current direct evidence outranks stale derived evidence;
- explicit current user policy outranks remembered preference;
- code-verified repository facts outrank stale design prose for current-state questions;
- inferred evidence remains retractable and below its premises.

Authority and freshness remain separate fields even when policy orders them jointly.

## Stage 7 — Sufficiency / coverage gate

Evaluate `EvidenceCoverage` for every required dimension.

Possible requirement states:

```text
satisfied
partial
missing
contradictory
stale
unsafe
provider_failure
unavailable
```

The planner decides whether another acquisition stage has positive expected decision value.

Do not retrieve more merely because budget remains.

If a required dimension cannot be satisfied inside the deadline, return a typed incomplete/degraded state rather than filling the packet with generic text.

## Stage 8 — Conditional escalation

Only when coverage says more evidence is justified, invoke additional sources such as:

- Crypt durable knowledge;
- skills;
- audit findings;
- prior architecture decisions;
- document evidence;
- more expensive semantic retrieval.

Re-evaluate coverage after each stage.

## Stage 9 — Fusion

Fuse once after eligibility.

Rules:

- provider-local scores remain provider-local;
- exact evidence flags are preserved;
- rank fusion is deterministic;
- absent optional channels are score-neutral;
- canonical IDs break ties deterministically;
- adaptive modifiers apply only within equivalent policy classes.

RRF is the standing deterministic baseline until a measured alternative wins.

## Stage 10 — Diversity and redundancy suppression

After eligibility and fusion:

- suppress duplicate source/hash/lineage/artifact families;
- preserve unique authority;
- preserve exact evidence;
- preserve atomic multi-hop evidence units;
- never destroy required coverage for aesthetic diversity.

## Stage 11 — Attention admission: coverage floor, then depth

This is the canonical budget algorithm.

### Phase A — coverage floor

Place the minimum faithful representation needed to satisfy every required evidence dimension.

Protect:

- binding constraints;
- explicit anchors;
- exact task-critical evidence;
- required contradiction pairs;
- source/citation identity.

### Phase B — marginal-utility depth

Spend remaining budget on upgrades or additional useful evidence based on:

- unresolved dimensions;
- authority;
- freshness;
- evidence completeness;
- novelty;
- risk reduction;
- task-specific value;
- token cost;
- latency cost.

Do not create a universal relevance probability.

### Reserved-lane migration

Current fixed reserved lanes (for example memory/skills) stay until the evidence-class coverage policy wins frozen qualification.

They are not the end-state architecture.

## Stage 12 — Representation planning

For each admitted `EvidenceUnit`, choose the cheapest faithful form:

```text
native
rendered/full
rendered/excerpt
skeleton
precomputed_summary
resolver_backed
metadata_only
```

Representation decisions do not alter authority or semantic eligibility.

A ranker does not secretly render.  
A renderer does not secretly rank.

## Stage 13 — Publication fence

Immediately before publication:

- revalidate grant;
- revalidate policy epoch;
- revalidate revocation/deletion;
- revalidate resolver availability when required;
- revalidate final budget reconciliation.

On material change:

- retry once under the new epoch when safe;
- otherwise emit typed `policy_changed` / revoked / unavailable state.

Never publish stale-authority bytes.

## Stage 14 — Packet + receipt

Emit the public packet.

The receipt must explain:

- what was generated;
- what was ineligible;
- what was ranked;
- what was selected;
- what was transformed;
- what was delivered;
- what was omitted;
- provider degradation;
- requirement coverage;
- token/byte reconciliation;
- recoverability;
- typed reasons.

Receipts remain content-free.

## Stage 15 — Outcome signal

After delivery, record bounded outcome events:

```text
delivered
resolved
used
ignored
contradicted
verified_used
verified_helped
superseded
```

These events feed evaluation and future retrieval pressure.

They never change source authority.

---

# 7. The feedback/outcome ledger is foundational, not Phase 12 polish

The original sequence placed the closed loop at the end even though several earlier mechanisms depend on it.

Correct the sequence.

A minimal content-free outcome ledger ships with the planner convergence work.

It records, keyed by stable packet/candidate/trace identities:

```text
candidate/provider/sourceKind
requirement dimensions
freshness class
authority class
estimated tokens
selected/dropped reason
delivered representation
resolved/refetched
used/ignored
verified contradicted/helped
task outcome marker where available
latency
```

It must not record raw prompt text or source content in telemetry surfaces.

The ledger enables later calibration of:

- sufficiency;
- acquisition staging;
- lifecycle reinforcement;
- marginal utility;
- redundancy;
- representation choices;
- provider invocation value.

No adaptive mechanism may pretend to optimize “usefulness” before this substrate exists.

---

# 8. Persist — durable knowledge, selectively

Persist is a governed input to future Pull, not the whole product.

## 8.1 Admission before storage

Canonical order:

```text
schema
→ scope
→ producer
→ sensitivity / DLP
→ epistemic classification
→ identity
→ novelty
→ exact duplicate
→ near duplicate
→ conflict / supersession
→ durability / expected utility
→ disposition
→ receipt
```

The model may propose ambiguous classifications off the hot path. Deterministic policy decides durable effects.

## 8.2 Minimal durable record model

Keep dimensions only when they cause distinct behavior:

```text
KnowledgeRecord
├─ logical id
├─ canonical content hash
├─ scope / repository / session lineage
├─ semantic kind
├─ evidence / provenance refs
├─ epistemic state
├─ authority
├─ influence
├─ sensitivity
├─ temporal validity
├─ lifecycle state
├─ supersession
├─ derivation refs
└─ relation refs
```

Mutable retrieval/usefulness signals live in a sidecar.

Do not create orthogonal taxonomies merely because a donor system used them.

Product-facing views such as:

- Decisions
- Memories
- Taste
- Gotchas
- Procedures
- Sessions
- Conflicts
- Archived
- Quarantined

may exist without each view becoming another architectural axis.

## 8.3 Write dispositions

Every write ends in exactly one:

```text
retain
update_metadata_only
supersede
merge
conflict
no_op
proposal
quarantine
reject
expire
forget
restore
```

`no_op` is a successful outcome.

## 8.4 Conflict

Never overwrite incompatible evidence silently.

Separate:

- identity match;
- semantic relationship;
- cause;
- evidence strength;
- action;
- row/lifecycle status.

Temporal supersession is not simultaneous contradiction.

Multi-value predicates are not conflict.

Uncertain subject identity does not create contradiction.

## 8.5 Temporal truth

Keep observed time, valid time, expiry, and transaction/recorded time distinct where behavior needs them.

Historical evidence identity remains immutable when current resolution changes.

## 8.6 Lifecycle

Lifecycle is:

- deterministic;
- versioned;
- reversible where possible;
- per semantic family only when behavior genuinely differs;
- archive-first, not delete-first.

Do not canonize arbitrary reinforcement constants in architecture.

Calibrate numerical thresholds from frozen evaluation and keep them versioned outside doctrine.

A retriever repeatedly selecting a memory must not make that memory immortal.

## 8.7 Dream / curation

Dream is a reversible maintenance stage:

```text
deterministic hygiene
→ cluster
→ derived consolidation proposal
→ provenance/evidence check
→ adopt or quarantine
→ archive parents only when recoverability is preserved
```

Dream never creates authority.

Ties go to quarantine/review.

## 8.8 Session continuity

At task/session close, allow one bounded episodic packet containing:

- task identity/goal;
- repository/branch/worktree/revision;
- decisions;
- open work;
- failed approaches;
- verification results;
- exact identifiers;
- artifact refs;
- contradictions;
- evidence refs.

It is episodic, not automatically semantic.

Do not persist the full transcript as truth.

---

# 9. Push — reduction must be wired into the real host loop

Push already has useful primitives. The final architecture must specify where they execute.

## 9.1 Canonical interception points

Membrane-owned or Membrane-integrated Push interception happens at these boundaries when the host supports them:

### A. Tool/MCP result egress

Before a large tool result becomes rendered agent context:

```text
tool/provider result
→ classify
→ preserve protected spans
→ artifact externalization
→ faithful reduction
→ resolver
→ packet/receipt
```

This is the primary portable integration point.

### B. Host post-tool hook

When a host exposes `PostToolUse` or an equivalent tool-result hook, route large result payloads through the same Push transform contract.

Host adapters remain thin; they do not implement separate compression algorithms.

### C. Source/file-read response

Large source reads can be:

- emitted natively when already present in host context;
- excerpted under exact span preservation;
- skeletonized when structure is sufficient;
- externalized and resolver-backed when not immediately needed.

### D. Provider-to-planner payload boundary

Providers may cap acquisition and externalize raw artifacts, but they do not make final attention decisions.

The planner retains final representation authority.

### E. Final renderer

The renderer performs only the selected deterministic representation and final byte/char enforcement.

It does not invent new ranking or policy.

## 9.2 Ordered reversible ladder

```text
1 exact dedupe
2 content-address raw artifact
3 deterministic noise removal
4 structure-preserving reduction
5 extractive faithful reduction
6 use a valid precomputed derived summary if already available
7 resolver-backed reference / metadata
8 explicit truncation last
```

Do not call a model in the prompt-critical Push path merely to summarize.

Model-derived summaries may be produced asynchronously as provenance-bound, invalidatable representations and reused only when valid.

## 9.3 Query-critical verifier

Protected elements include:

- identifiers;
- exact errors/codes;
- failing test names;
- explicitly requested values;
- cited spans;
- policy/constraint text;
- task entities;
- tool-call/result integrity pairs;
- decision/rationale integrity pairs;
- diff header/hunk integrity.

If a required span is lost:

- restore it exactly from the artifact/resolver;
- otherwise return typed incomplete/unachievable state.

## 9.4 Never-worse-than-raw accounting

Persist/emit a typed balance:

```text
TokenBalanceV1
original
materialized
delivered
provider_billed
```

with required accounting invariants.

Token savings are never claimed without a paired evidence-preservation assertion.

## 9.5 Push adoption is a product metric

Measure:

- eligible Push opportunities;
- Push executions;
- passthrough reasons;
- artifacts externalized;
- tokens/bytes avoided;
- resolver refetches;
- protected-span restore rate;
- transform failures;
- task non-regression.

An excellent unused primitive is not a finished Push system.

---

# 10. Cortex boundary — no duplicate drift engine

Cortex owns code/repository identity and current repository truth.

Membrane consumes:

- stable node/definition identity;
- exact locations/ranges;
- source hashes;
- repository revision;
- graph generation;
- dirty-overlay identity;
- typed relations;
- impact;
- coverage;
- contradictions;
- source resolution state;
- re-anchored locations.

## 10.1 RecallCircuit

For multi-hop repository questions:

```text
Membrane:
    task-shaped recall request
    policy / bounds / expected generation

Cortex:
    generation-bound RecallCircuit
    complete paths + evidence
    unresolved/omission states

Membrane:
    treats each complete path as one atomic EvidenceUnit
```

Membrane does not traverse the Cortex graph itself.

An incomplete path cannot masquerade as exact complete evidence.

Generation/schema mismatch fails closed for that Cortex lane.

No relevant seed produces a typed abstention, not generic repository filler.

Legacy flattened Cortex candidates remain only a bounded version-skew/rollback path during migration.

## 10.2 Code-anchor resolution

Delete the duplicate Membrane re-anchor ladder from the end-state architecture.

Membrane stores a memory-side reference state such as:

```text
resolved
moved
ambiguous
drifted
missing
unsupported
inaccessible
revoked
```

but for a code anchor it obtains the resolution from Cortex.

Membrane may decide:

```text
Cortex says resolved/current   → eligible under policy
Cortex says moved              → update current locator, preserve historical evidence identity
Cortex says ambiguous          → cannot satisfy required exact evidence
Cortex says missing            → treat as missing only when Cortex reports sufficient coverage
Cortex says unsupported        → indeterminate / blind spot
Cortex unavailable             → typed degraded Cortex lane
```

Do not implement structural matching twice.

---

# 11. Ranking, budgeting, and the reserved-lane transition

## 11.1 Current migration control

The repository currently uses provider/source-kind reserved lanes to prevent incompatible raw scores from starving memory and skills.

Keep them until a replacement is proven.

## 11.2 End-state policy

The final policy protects **evidence needs**, not provider names.

Example evidence classes:

```text
binding_constraints
explicit_anchors
current_exact_evidence
required_supporting_evidence
contradiction_evidence
prior_knowledge
background_context
```

Provider identity remains provenance, not budget ontology.

A memory item that satisfies a required prior-decision requirement can receive coverage-floor protection.

A useless memory item receives no tokens merely because `provider == crypt`.

## 11.3 Promotion gate for lane removal

Do not remove fixed reserved lanes until the evidence-class planner proves:

- no reduction in required-evidence recall;
- no increase in memory/skill starvation where those are required;
- no increase in stale/low-authority admission;
- exact global ceiling reconciliation;
- equal or better whole-task success;
- rollback tested.

---

# 12. Position-aware layout

Position can affect model use of evidence, but the architecture must not hardcode placement by provider name.

Use semantic placement classes:

```text
binding_constraint
supporting_evidence
active_task_state
```

Typical deterministic layout:

```text
front:
    system/project constraints
    applicable policy
    protected anchors

middle:
    current code evidence
    Cortex circuits
    documents
    prior decisions
    memory
    audit findings
    skills

late / near task boundary:
    current dirty/live working state
    active task/session context
```

A provider may emit evidence into different placement classes.

Placement changes representation order only. It does not alter authority or ranking.

Roll out behind a flag until whole-task evaluation shows non-regression.

---

# 13. Latency and deadline architecture

Staged retrieval saves work only if sequential staging does not consume the host deadline.

Latency is therefore a first-class promotion dimension.

## 13.1 One absolute deadline

The request carries one monotonic deadline.

Every provider/stage receives remaining time.

No stage resets the clock.

## 13.2 Stage accounting

Receipts/traces record content-free:

- stage;
- provider;
- started;
- duration;
- remaining deadline;
- timeout/cancel/degrade reason;
- whether the stage changed coverage;
- whether another stage was invoked.

## 13.3 Fast path and escalation path

Qualification measures separately:

```text
fast path:
    normalize
    requirements
    mandatory evidence
    sufficiency
    admission
    publication

one-escalation path:
    fast path
    + one additional provider stage

worst supported bounded path:
    all allowed stages within host deadline
```

Each supported host/client has a frozen deadline budget and required publication reserve in qualification configuration.

Do not bake arbitrary universal milliseconds into doctrine.

## 13.4 Stop rules

Do not start a stage when:

- remaining time is below its measured safe execution envelope plus publication reserve;
- it cannot satisfy any currently missing/useful requirement;
- its expected decision value is non-positive.

Return typed incomplete/degraded coverage instead.

---

# 14. Security, trust, influence, and erasure

## 14.1 Text cannot self-authorize

Keep distinct:

```text
trust_class
instruction_policy
authority
influence_class
sensitivity
```

Repository source, README text, memory, generated docs, audit prose, and model output are data unless an external trusted policy grants instruction authority.

A Cortex relationship does not upgrade source text into an instruction.

## 14.2 DLP at both boundaries

Check at:

- persistence admission;
- final publication/resolution.

## 14.3 Path confinement

Canonicalize before authorize.

Test:

- `..`;
- symlinks;
- nested repositories;
- case variants;
- prefix tricks;
- Windows drive/UNC;
- Mac/Windows path normalization.

## 14.4 Erasure

Erasure must remove payload from:

- canonical durable content;
- FTS;
- vectors;
- relation projections;
- artifact refs/content;
- exports;
- caches;
- in-flight publishability.

A tombstone may retain only non-payload identity/audit metadata required by policy.

Erased content must not reappear after projection rebuild or restore.

## 14.5 Corruption

Corruption becomes typed quarantine/repair state.

Never silently delete the database and regenerate as if nothing happened.

---

# 15. Process planes and operations

Preserve the existing three-plane contract:

```text
Application
Control
Data
```

No fourth process plane without a deliberate versioned architecture change.

## Application plane

Owns:

- CLI;
- stdio MCP;
- loopback API;
- request routing;
- typed packets/receipts.

It does not own direct durable storage mutation outside typed APIs.

## Control plane

Owns:

- supervisor child;
- leases;
- heartbeat;
- bounded maintenance ownership;
- restart/coordination protocol.

It does not become a second data store.

## Data plane

Owns:

- SQLite;
- canonical durable records;
- projections;
- storage identity;
- durable receipts/events as defined by the data contract.

It does not own network transport.

## Hub boundary

Hub owns:

- start-at-login;
- OS child lifecycle;
- installer/updater lifecycle;
- process restart/backoff.

Membrane exposes:

- readiness;
- health;
- drain/shutdown;
- process identity;
- storage identity;
- lifecycle protocol version.

---

# 16. Maintenance execution — narrow, not a general job platform

Membrane needs durable bounded maintenance for its own work:

```text
projection rebuild
curation
lifecycle recalculation
anchor verification request
offline session mining
integrity check
backup/export/restore
```

Use a constrained `MaintenanceRun` model rather than treating Membrane as a general job scheduler.

Required properties:

- idempotent;
- bounded;
- cancellable;
- crash-visible;
- checkpointable when necessary;
- reason-bearing receipt;
- never prompt-critical;
- scheduled externally by Hub/control lifecycle.

If a general Job/Run abstraction later has two real non-Membrane consumers, factor it then.

---

# 17. Human-readable inspection

Anything Membrane durably retains must be inspectable without reverse-engineering SQLite.

Membrane owns a read-only governed knowledge/query surface exposing:

- safe content/projection;
- semantic kind;
- source;
- evidence;
- authority;
- freshness;
- validity;
- lifecycle;
- relations;
- supersession;
- resolution;
- derivation;
- why retained.

Hub/UI owns presentation.

Inspection never upgrades authority.

Deterministic Markdown export is an export/read model, not the database.

---

# 18. Observability and evaluation are the promotion system

## 18.1 Measure separate dimensions

Never collapse everything into one “quality” number.

Track:

- required-evidence recall;
- forbidden/stale evidence admission;
- precision@K / Recall@K / MRR / nDCG where appropriate;
- contradiction miss rate;
- temporal accuracy;
- code/source resolution success;
- explicit-anchor survival;
- scope violation;
- receipt completeness;
- budget reconciliation;
- transform corruption;
- delivered tokens/chars;
- provider calls;
- tool calls after delivery;
- resolver/refetch rate;
- p50/p95/p99;
- CPU/RSS;
- DB/index growth;
- deterministic replay variance;
- whole-task success.

Hard safety/contract targets remain exact where appropriate:

```text
cross-scope leak = 0
budget reconciliation failure = 0
protected evidence corruption = 0
explicit required anchor loss = 0
```

## 18.2 Four proofs per capability

A capability is not complete with unit tests alone.

Require:

1. **source proof** — focused tests;
2. **integration proof** — real request path consumes it;
3. **behavior proof** — frozen task demonstrates the effect;
4. **operational proof** — installed artifact under realistic failure/resource conditions.

## 18.3 Whole-task promotion law

No feature promotes unless:

```text
task correctness is non-inferior or better
AND required-evidence coverage is non-inferior or better
AND authority/scope safety does not regress
AND receipt integrity does not regress
AND at least one meaningful cost improves:
    fewer delivered tokens
    OR fewer provider/retrieval/tool calls
    OR lower latency/resource cost
```

A feature that increases complexity without an end-to-end gain does not ship.

## 18.4 Adaptive features

Every adaptive policy needs:

- version;
- frozen control;
- candidate;
- holdout;
- rollback;
- ablation.

Do not move thresholds to make a candidate pass.

---

# 19. Canonical implementation sequence

The sequence is deliberately core-first.

Phases 0–8 produce the finished Membrane product.  
Phases 9–10 are conditional breadth/experiments and are not completion prerequisites unless frozen evaluation proves the need.  
Phase 11 qualifies the installed product.

## Phase 0 — Authority and frozen baseline

Goal: stop architecture churn and make every change comparable.

Do:

- adopt one canonical implementation authority;
- refresh source baseline from current `main`;
- regenerate current-state manifest from source;
- freeze V1 protocol/golden fixtures;
- freeze packet/order/omission/grant/budget behavior;
- build context-quality fixtures;
- freeze current latency/RSS/token/provider-call baseline;
- inventory duplicate policy decisions;
- freeze feature flags/experimental controls.

Gate:

- reproducible baseline;
- one source of implementation authority;
- current source claims verified.

## Phase 1 — Planner convergence + outcome ledger + deadline

Goal: establish the actual product spine and measurement loop first.

Do:

- one typed request-context carrier;
- one absolute deadline;
- converge duplicate scope/authority/freshness/budget/publication decisions;
- keep providers as candidate/evidence producers;
- add minimal candidate journey/outcome ledger;
- central publication fence;
- typed provider degradation;
- content-free per-stage latency/accounting;
- preserve current planner behavior under a control flag.

Gate:

- same input yields same final policy decision across entry paths;
- outcome events can be joined to delivered candidate IDs;
- provider failures remain isolated;
- revoke/policy change before publication cannot emit stale bytes.

## Phase 2 — Evidence requirements + staged acquisition + sufficiency

Goal: stop blind nine-provider fan-out without replacing it with brittle keyword routing.

Do:

- `EvidenceRequirement` derivation;
- capability-based acquisition planning;
- keep a diagnostic task class only as secondary metadata;
- stage mandatory low-cost evidence first;
- coverage/sufficiency evaluator inside planner policy;
- shadow “would invoke” vs current provider set;
- activate narrowing first on proven low-risk/easy cases;
- preserve broad fallback for ambiguity.

Gate:

- fewer provider calls where evidence is unnecessary;
- no correctness/coverage regression;
- fast path and escalation path fit supported host deadlines.

## Phase 3 — Retrieval quality + evidence-class attention policy

Goal: fix verified lexical weakness and remove provider identity from permanent budget ontology.

Do:

- production FTS5/BM25 rebuildable projection;
- identifier-aware tokenization;
- exact / lexical / vector / temporal channel registry;
- deterministic fusion trace;
- two-phase coverage-floor/depth admission;
- evidence-class coverage-floor policy in shadow;
- keep current memory/skill reservations as control;
- post-fusion diversity.

Gate:

- lexical quality improves frozen cases;
- vector-off fallback works;
- evidence-class policy meets or beats reserved-lane control;
- no starvation of required durable knowledge/skills;
- global budget exact.

## Phase 4 — Cortex bridge, RecallCircuit, and delegated resolution

Goal: obtain complete repository evidence without duplicating Cortex.

Do:

- prefer generation-bound RecallCircuit when available;
- complete path → one atomic internal EvidenceUnit / V1 candidate adapter;
- legacy flattened candidates only for rollback/version skew;
- generation/schema fail closed for Cortex lane;
- no-seed abstention;
- call Cortex-owned locate/re-anchor for code references;
- remove end-state Membrane structural re-anchor ladder;
- preserve memory-side resolution state and historical evidence identity.

Gate:

- multi-hop tasks need fewer model-driven search calls;
- no Membrane parser/graph/re-anchor implementation;
- moved/ambiguous/missing/unsupported distinctions remain typed;
- poisoning text remains `data_only`.

## Phase 5 — Push wiring + artifact-backed reversible reduction

Goal: make Push operate on real accumulated context, not merely exist as utilities.

Do:

- one transform contract;
- wire tool/MCP result egress;
- wire host post-tool hooks where supported;
- wire large source reads;
- converge `runc`/`skel`/`compress`/truncate behavior under the contract;
- content-address raw artifacts;
- query-critical verifier;
- representation planner;
- exact restoration;
- `TokenBalanceV1`;
- Push opportunity/adoption telemetry;
- position-aware semantic layout behind flag;
- deterministic prefix/cache tests.

Gate:

- measurable Push adoption on real eligible events;
- zero protected corruption;
- raw evidence resolvable;
- task-quality non-regression;
- savings paired with fidelity.

## Phase 6 — Persistence admission + canonical knowledge substrate

Goal: keep durable knowledge lean by construction.

Do:

- minimal canonical record/evidence model;
- signal sidecar;
- admission before persistence;
- exact no-op;
- conflicts/supersession;
- temporal semantics;
- negative knowledge;
- bounded session packets;
- deterministic lifecycle;
- Dream as reversible proposal/consolidation only.

Gate:

- any durable item explains origin/evidence/authority/validity/derivation;
- duplicates no-op;
- conflicts preserve evidence;
- no derived summary self-promotes to truth.

## Phase 7 — Security, influence, erasure

Goal: close trust boundaries after the core paths exist, with fixtures present from Phase 0.

Do:

- DLP at persistence and publication;
- explicit influence/authority/sensitivity separation;
- path jail;
- receipt authenticity markers where justified;
- revoke/delete publication races;
- erasure across projections/artifacts;
- corruption quarantine;
- RecallCircuit poisoning fixtures.

Gate:

- zero cross-scope leak;
- erased content cannot reappear;
- repository/model text cannot self-authorize.

## Phase 8 — Storage, inspection, maintenance, operations

Goal: make the finished core operationally trustworthy.

Do:

- one store resolver/identity;
- read-only inventory;
- doctor inspect/allowlisted-repair split;
- knowledge inspection/read model;
- projection rebuild;
- backup/verify/restore;
- deterministic export/import;
- wipe;
- migration preflight/backout;
- constrained `MaintenanceRun`;
- Hub lifecycle boundary.

Gate:

- logical identities and recall equivalence survive backup/restore;
- corruption is typed/repairable;
- maintenance never blocks prompt path;
- retained knowledge is inspectable.

## Phase 9 — Conditional relations / curation expansion

Not a prerequisite unless evaluation demonstrates a gap.

Candidate work:

- narrow evidence-backed relations;
- aliases;
- depth-1 bounded relation expansion;
- more sophisticated session curation.

Promote only when an ablation shows whole-task value.

## Phase 10 — Gated experiments

Not architecture backlog.

Examples only when a frozen gap exists:

- alternative diversity method;
- retrieve/no-retrieve heuristic;
- local reranker;
- query expansion;
- other adaptive retrieval mechanisms.

Explicitly not pre-authorized:

- community/PageRank/DRIFT graph platform;
- HyDE by default;
- automatic self-improvement;
- remote vector/graph services;
- multimedia ingestion pipeline;
- general agent framework.

## Phase 11 — Installed-product qualification

Qualify supported hosts/platforms using current artifacts.

Require:

- install;
- discovery;
- tools;
- grant;
- context;
- source resolve;
- proposal/persist;
- feedback/outcome;
- checkpoint/session continuity;
- restart/degradation;
- upgrade;
- uninstall;
- Mac/Windows resource and latency evidence;
- whole-task comparison against frozen control.

No public capability is called shipped without installed-path proof.

---

# 20. Canonical file-level ownership map

Verify paths against current `main` before tickets.

| Area | Current / target ownership |
|---|---|
| `engine/crates/membrane-protocol/src/types.rs` | public V1 protocol source of truth; version only deliberately |
| `engine/crates/crypt-core/src/planner.rs` | current admission control; converge toward final planner policy without parallel planner |
| `engine/crates/membrane-core/` | global budget/reconciliation/final policy primitives |
| `engine/federation/gateway.py` | acquisition execution under one deadline; staged provider execution |
| `engine/federation/context_plan.py` | if retained as file, implements requirement/capability plan, not single-label semantic authority |
| `engine/federation/retrieval_evaluator.py` | if retained as file, computes evidence coverage for planner; not second policy owner |
| `engine/federation/providers/cortex.py` | Cortex adapter only; RecallCircuit + typed resolution consumption |
| `engine/federation/providers/crypt.py` | Crypt evidence adapter |
| `engine/federation/providers/*` | evidence producers; no final budget/authority decisions |
| `engine/crates/crypt-core/` | durable-memory retrieval/admission/lifecycle/conflict policy |
| `engine/crates/crypt-store/` | canonical durable store + rebuildable projections |
| `engine/crates/membrane-runtime/` | publication, Push/artifact/working context/runtime integration |
| `mcp/server.mjs` | thin MCP surface |
| `mcp/context-renderer-lib.cjs` | deterministic execution of planner-selected representation/layout; no hidden ranking |
| `mcp/working-context.mjs` | bounded active working context; schema changes mirrored with Rust |
| `schemas/*receipt*` | content-free explanations/reconciliation |
| `docs/architecture.md` | generated only |
| `docs/product.md` | generated only |
| `docs/MEMBRANE-IMPLEMENTATION-GUIDE.md` | canonical implementation authority after adoption |
| `tests/context-quality/` | frozen semantic/authority/safety fixtures |
| qualification/evidence paths | installed-artifact proof |

Do not add a new module until the tree is searched for an existing owner.

---

# 21. Migration and rollback

Use additive migration first.

Rules:

- preserve public V1;
- preserve stable IDs;
- never manufacture provenance during backfill;
- use explicit `legacy_unattributed` where history lacks evidence;
- projections remain rebuildable;
- new policy paths run `off → shadow → limited-on → on`;
- thresholds freeze during qualification;
- rollback disables reads/policies but does not discard valid durable records written by newer code;
- one cutover authority per concern.

Direct rollbacks:

```text
evidence-requirements plan fails
    → execute current broad provider plan

staged sufficiency regresses
    → disable staging, preserve unified final planner

RecallCircuit unavailable/version skew
    → bounded legacy Cortex candidate adapter

evidence-class coverage floor regresses
    → retain current reserved-lane control

layout regresses
    → disable semantic layout flag

Push reducer fails
    → use less reduction / raw / resolver-backed artifact

projection corrupt
    → quarantine projection and use fallback/rebuild

adaptive feature regresses
    → disable candidate policy and restore frozen control
```

---

# 22. Rejected / deferred scope

## Reject from canonical core

- second code parser/indexer;
- second code re-anchor ladder;
- independent Membrane code graph;
- external vector/graph DB as required infrastructure;
- Redis/Postgres/RocksDB backend matrix without two real needs;
- RDF/SPARQL/ontology platform;
- agent framework / PTY runtime;
- model router;
- autonomous multi-agent loops;
- browser automation;
- full transcript ownership;
- prompt optimization;
- P2P memory federation before local correctness;
- one global weighted relevance score;
- model-decided durable truth;
- prompt-critical LLM planning/summarization;
- generic Job/Run platform;
- multimedia ingestion as Membrane core;
- automatic self-improvement.

## Conditional only behind a measured gap

- local cross-encoder;
- query expansion;
- MMR alternative;
- relation expansion variants;
- more complex lifecycle curves;
- more complex semantic summarization;
- multimodal support if the product boundary itself changes through a new architecture decision.

---

# 23. Definition of Done

Membrane is not done because every research mechanism was implemented.

It is done when the product purpose is mechanically true.

## 23.1 Product identity

- [ ] Membrane is demonstrably a context control plane/compiler, not a memory platform or orchestrator.
- [ ] Cortex, Crypt, Hub, and host boundaries are explicit and enforced.
- [ ] One active implementation authority exists.
- [ ] Current product docs regenerate from landed source.

## 23.2 Planner / Pull

- [ ] One final planner owns grant, eligibility, authority, freshness, sufficiency, fusion, admission, representation policy, publication, omissions, and receipts.
- [ ] Providers stop at typed evidence.
- [ ] Task routing uses multidimensional evidence requirements; no mutually-exclusive keyword class is the final semantic authority.
- [ ] Retrieval is staged.
- [ ] More retrieval occurs only for missing/useful evidence dimensions.
- [ ] Ambiguous tasks fail broad/conservative, not confidently narrow.
- [ ] FTS5/BM25 exists in production as a rebuildable projection.
- [ ] Retrieval works with embeddings disabled.
- [ ] Unrelated scores are not treated as one probability.
- [ ] Evidence-class coverage-floor admission qualifies against current reserved lanes.
- [ ] Two-phase breadth/depth fill is deterministic.
- [ ] Every important rejection/omission is explained.

## 23.3 Feedback / usefulness

- [ ] Candidate journey/outcome ledger exists from the core planner phase.
- [ ] Delivered candidates can be joined to later use/ignore/contradiction/help signals.
- [ ] Feedback never changes authority.
- [ ] Any adaptive usefulness/lifecycle feature can cite the exact signal substrate it consumes.

## 23.4 Cortex boundary

- [ ] Cortex RecallCircuit is preferred when compatible.
- [ ] Complete paths stay atomic.
- [ ] Generation mismatch fails closed for Cortex.
- [ ] No relevant seed emits no fake repository context.
- [ ] Code anchor relocation/re-anchoring is delegated to Cortex.
- [ ] Membrane contains no duplicate structural re-anchor implementation.
- [ ] Unsupported/ambiguous/missing are not collapsed.
- [ ] Repository evidence remains `data_only`.

## 23.5 Push

- [ ] Push is wired into real tool/MCP result egress.
- [ ] Host post-tool integration is used where supported.
- [ ] Large source/file reads use the same transform contract.
- [ ] Raw artifacts are content-addressed and recoverable.
- [ ] Query-critical spans survive or restore exactly.
- [ ] Transform failure falls back to less reduction/raw.
- [ ] Token/byte savings are paired with evidence-fidelity proof.
- [ ] Push adoption/opportunity rate is measured.
- [ ] Position-aware layout is semantic, deterministic, and qualified.
- [ ] No prompt-critical LLM summarization dependency exists.

## 23.6 Persist

- [ ] Admission precedes durable truth.
- [ ] Exact duplicates return no-op.
- [ ] Conflicts preserve both evidence sets.
- [ ] Supersession is explicit.
- [ ] Derived text never silently becomes authoritative.
- [ ] Mutable usefulness signals are separate from canonical content.
- [ ] Lifecycle is deterministic/versioned/reversible where possible.
- [ ] Dream/curation preserves parents/evidence and supports undo.
- [ ] Session continuity does not duplicate full transcript truth.

## 23.7 Security

- [ ] Current user authority outranks remembered/repository/model text.
- [ ] Influence, authority, trust, instruction policy, and sensitivity remain distinct.
- [ ] DLP runs at persistence and publication.
- [ ] Cross-scope leak fixtures are zero.
- [ ] Revoke/delete races cannot publish stale bytes.
- [ ] Erased payload cannot reappear from projection, cache, artifact, backup restore, or in-flight publication.
- [ ] Corruption becomes typed quarantine/repair state.

## 23.8 Deadline and performance

- [ ] One absolute deadline is propagated end to end.
- [ ] Fast path, one-escalation path, and bounded worst path are measured separately.
- [ ] Every supported host path stays inside its frozen deadline including publication reserve.
- [ ] Provider/stage latency is receipt/trace-visible without task content.
- [ ] No maintenance/background job blocks prompt delivery.
- [ ] p50/p95/p99, CPU, RSS, tokens, provider calls, and tool calls are measured from current artifacts.

## 23.9 Receipts and recoverability

Every packet can answer:

- [ ] what was considered;
- [ ] what was eligible;
- [ ] what was selected;
- [ ] what was omitted;
- [ ] why;
- [ ] what transformed;
- [ ] what representation was delivered;
- [ ] which provider degraded;
- [ ] which requirement remained uncovered;
- [ ] how to recover exact evidence;
- [ ] whether token/byte accounting reconciles.

Every durable item can answer:

- [ ] what am I;
- [ ] where did I come from;
- [ ] what supports me;
- [ ] whose scope;
- [ ] how authoritative;
- [ ] when observed;
- [ ] when valid;
- [ ] what superseded me;
- [ ] what I derived from;
- [ ] what lifecycle/resolution state I am in;
- [ ] why I was retained.

## 23.10 Evaluation

- [ ] Source proof exists for every promoted capability.
- [ ] Integration proof exists.
- [ ] Frozen behavior proof exists.
- [ ] Installed operational proof exists.
- [ ] Whole-task correctness is non-inferior.
- [ ] Required-evidence recall is non-inferior.
- [ ] Authority/scope safety is non-inferior.
- [ ] Receipt integrity is exact.
- [ ] At least one meaningful attention/retrieval/latency/resource cost improves for every material adaptive feature.
- [ ] Mac and Windows supported installed paths qualify against current artifacts.

---

# 24. Final architecture statement

The finished Membrane system is deliberately narrower than the union of every context-engineering and memory mechanism studied.

Its durable advantage is not “more memory,” “more graph,” or “more retrieval.”

It is this:

```text
heterogeneous evidence
        ↓
required evidence
        ↓
bounded staged acquisition
        ↓
hard authority/freshness/trust eligibility
        ↓
sufficiency
        ↓
deterministic fusion
        ↓
coverage floor
        ↓
marginal-utility depth
        ↓
cheapest faithful representation
        ↓
publication revalidation
        ↓
smallest useful current context
        +
complete receipt
```

And across the three motions:

```text
PULL
    acquire only what can change the task decision

PUSH
    reduce what is already flowing without destroying recoverability

PERSIST
    retain only durable governed knowledge worth paying future attention to
```

The core ownership rule is:

> **Cortex determines repository evidence and repository truth. Crypt preserves durable knowledge. Providers produce typed evidence. Membrane determines what deserves the agent's attention now, in what form, under whose authority, and records exactly why.**

That is the canonical shape.
