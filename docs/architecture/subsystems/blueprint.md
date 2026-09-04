# Blueprint Architecture Canon v2

**Status:** Proposed normative architecture  
**Date:** 2026-09-04  
**Applies to:** `Orthic-Labs/Membrane/blueprint`

---

## 1. Purpose

Blueprint exists to answer a coding agent's most important repository questions with **current, source-anchored, provenance-preserving evidence** without forcing the agent to rediscover the codebase through repeated Glob/Grep/Read traversal.

Blueprint is not merely a graph renderer or static index. Its product contract is:

> **The watcher makes the evidence current. The freshness gate proves it. The semantic stack makes it precise. The agent integration makes it discoverable. The MCP/CLI make it useful.**

Blueprint must remain local-first, deterministic by default, low-overhead at rest, explicit about uncertainty, and small enough that it can be enabled continuously rather than invoked only for special analyses.

Implementation donor guidance is intentionally kept out of the normative architecture. See `04_BLUEPRINT_DONOR_REFERENCE_V2.md` for repository-specific prior art, donor→BPT mappings and license cautions.

---

# 2. Ownership boundary

## 2.1 Blueprint owns

Blueprint owns repository **evidence and structural intelligence**, including:

- repository/revision/worktree identity;
- code/document artifact identity;
- source occurrences and semantic entities;
- deterministic syntax facts;
- compiler/SCIP and live semantic facts;
- symbol/reference/import/call/type relationships;
- framework-derived structural facts;
- provenance, evidence, freshness and resolution-frontier information;
- structural impact and affected-test evidence;
- repository entry points and evidence-backed execution/process projections;
- explicit service/API/tool contracts and cross-repository bridges;
- code/document truth reconciliation;
- exact/lexical/optional semantic candidate retrieval;
- bounded context views and source-centered graph/navigation output;
- continuous freshness and operational readiness.

## 2.2 Blueprint does not own

The following belong elsewhere in Membrane or in an agent/action layer:

- general durable agent memory;
- unrestricted project decision history unrelated to evidence reconciliation;
- final context-packet planning for an LLM session;
- policy/effect authorization;
- semantic editing/refactoring actions;
- arbitrary shell/build/test execution;
- generic always-on CPG/PDG/dataflow/taint infrastructure;
- silent LLM-generated structural code truth.

Blueprint may expose evidence to systems that own those functions. It must not absorb those responsibilities merely because a donor repository combines them.

---

# 3. Architectural invariants

## INV-001 — Source is canonical truth

Current repository source and explicit external semantic producers are the basis of code truth. Documents, summaries, embeddings, architecture projections and inferred relationships may enrich retrieval but may not silently outrank current source evidence.

## INV-002 — Canonical facts are distinct from projections

Blueprint's durable semantic substrate is a **canonical typed fact ledger**. Derived representations are rebuildable.

Canonical substrate:

```text
Repo / Revision / Worktree
Artifact
Entity
Occurrence
Relation
Evidence
Claim (where Blueprint owns document/reconciliation claims)
Provider / Analyzer
Generation
Freshness / provenance metadata
```

Rebuildable projections include:

```text
structural graph indexes
architecture components
entry-point/process/step views
contract linkage views
BM25 indexes
vector indexes
communities / hubs / cycles
Mermaid/SVG/export artifacts
risk/test recommendation projections
```

Loss or migration of a projection must not destroy semantic facts.

## INV-003 — Entity identity and occurrence identity are different

A semantic entity is not its line number. A definition/reference/call at a particular source span is an occurrence of an entity.

Line/column/byte positions are occurrence properties, never semantic identity.

## INV-004 — Provenance is categorical; confidence is inferential

Authoritative/deterministic facts use provenance classes, not fake probability scores.

Correct:

```text
provenance = AUTHORITATIVE_SEMANTIC
confidence = null
```

For inferred information:

```text
provenance = HEURISTIC_BRIDGE
confidence = 0.78
```

Confidence exists only where uncertainty is actually modeled.

## INV-005 — Freshness precedes authority

A producer's theoretical precision does not permit stale evidence to outrank current evidence describing a newer workspace state.

The resolution/admission order is:

```text
1. admissibility / scope
2. source-state coherence and freshness
3. semantic authority
4. resolution specificity
5. inferential confidence, if applicable
```

For example, a stale SCIP fact cannot be treated as current for a dirty workspace merely because SCIP has higher semantic authority. The dirty source must be repaired/reparsed first; an optional LSP cross-check may confirm or contradict the resulting resolution but does not silently replace Blueprint's canonical fact.

## INV-006 — Normal edits are incremental

An ordinary create/modify/delete/rename event must update affected facts and a bounded dependent frontier. It must not trigger a full repository rebuild.

## INV-007 — Queries may repair narrowly, never reconcile unboundedly

A query may synchronously repair the exact dirty file(s) required to answer within a strict budget. It must never perform an unbounded repository reconciliation on the read path.

## INV-008 — One logical writer per repository generation

Hub watcher, MCP-session fallback, explicit repair and manual build/update operations coordinate through the existing lease/single-writer model.

## INV-009 — Idle cost is negligible

No periodic full repository scan. No permanently loaded embedding model. No permanent LSP fleet merely because a repository is enrolled.

## INV-010 — Multi-repo is an overlay, not a merged megagraph

Each repository keeps independent generation, evidence and identity spaces. Cross-repository traversal crosses an explicit contract/bridge relation with evidence.

Never connect repositories because names merely match.

## INV-011 — LLM output is not deterministic structural truth

LLM-derived relations may assist document/context enrichment when clearly segregated and provenance-marked. They are not silently admitted as compiler/AST/framework code edges.

## INV-011A — Providers describe capability; they do not own policy

A provider may declare facts, omissions, cost, readiness, precision and evidence. It does not decide whether those facts are admitted as repository truth. Admission, authority, freshness and policy remain Blueprint application/canon concerns.

## INV-011B — Architecture truth is binary, not approximate

Declared architecture and observed code may agree, contradict, or remain unresolved/stale. Physical co-location or heuristic similarity must never collapse a contradiction into an approximately-correct state. If code violates the declared boundary, preserve the typed violation.

## INV-012 — Agent tool surface remains small

Blueprint may have many internal methods. The default agent-visible MCP interface remains a small set of strong semantic operations.

The current six tools remain canonical:

1. `blueprint_recall`
2. `blueprint_search`
3. `blueprint_expand`
4. `blueprint_impact`
5. `blueprint_doc_truth`
6. `blueprint_status`

A convenience `blueprint_explore` facade may be added later if benchmarks show agents benefit, but it must compose—not erase—the explicit semantics above.

## INV-013 — Schema evolution is explicit and mechanically verified

Entity/relation kinds, provider payloads and persisted projection schemas are versioned contracts.

- new relation/entity kinds require registration in the canonical schema;
- breaking changes require an explicit migration/compatibility decision;
- unknown schema versions are never silently reinterpreted;
- provider/indexer output is checked against executable conformance fixtures before it is treated as supported;
- schema documentation examples should be reusable as verifier fixtures where practical.

Blueprint may borrow Kythe/Glean/SCIP-style verification discipline without committing to any particular verifier implementation or query language.

## INV-014 — Publication is completeness-safe

A failed, truncated or partial extraction must not replace a known-complete published generation merely because it is newer.

Likewise, incremental repair of one file/frontier must not silently remove facts owned by unrelated unchanged files.

Publication therefore requires explicit completeness/convergence evidence. If a run cannot prove that evidence, it may remain staged/degraded but must not be promoted as the current complete generation.

## INV-015 — Correctness is executable

Semantic correctness is not established only by unit tests over parser functions.

For every supported language/provider class, Blueprint maintains source-anchored fixtures that can assert:

- definition/reference identity;
- call/import/type/override relationships;
- expected unresolved/ambiguous frontiers;
- framework facts;
- source spans/provenance;
- negative assertions preventing false edges.

A compact human-readable assertion format is preferred. An embedded Datalog engine or new test DSL is optional implementation technique, not an architectural requirement.

## INV-016 — No discovered input disappears silently

Every discovered source, provider result, event and repair candidate must terminate in a typed disposition. A path may be indexed, ignored by policy, unsupported, deferred, superseded, rejected, failed or otherwise classified by the existing Membrane disposition contract, but it must not vanish between discovery and publication without an inspectable terminal reason.

The exact disposition vocabulary is an implementation/schema contract and should reuse the existing Membrane taxonomy rather than invent a parallel Blueprint-only enum.

## INV-017 — Freshness relation is explicit; unknown is never fresh

The public readiness state may remain compact (`unwatched`, `catching_up`, `current`, `stale`, `degraded`), but Blueprint must preserve the more precise source-state relation underneath it: whether indexed evidence is behind, ahead of, diverged from, equal to, or unable to establish relation with the observed source state.

`unknown` must never collapse to `current`. A stale result should expose the relation/reason that made it stale when determinable.

## INV-018 — Continuations and writer ownership are source-state bound

Any cursor, continuation token or paged traversal whose correctness depends on a graph generation is bound to that generation and fails closed if the generation changes incompatibly.

Store leases must distinguish process incarnation, not merely PID. Lease identity therefore includes process-start identity (or an equivalent monotonic incarnation token) so PID reuse cannot create two valid writers.

## INV-019 — Declared truth and observed truth remain distinguishable

Documentation, configuration, contracts and other declarations may state what the system is supposed to do; source/runtime evidence establishes what Blueprint can currently prove it does. `blueprint_doc_truth` and related projections must retain the binding state between the two rather than flattening them.

At minimum the model must be able to represent grounded agreement, contradiction, stale reference and unresolved/unknown grounding.

## INV-020 — New sophistication must improve agent outcomes

A capability that adds persistent resource cost, tool-selection complexity or semantic risk is not promoted merely because it improves an isolated retrieval metric. Where practical, promotion requires a frozen agent-task comparison against the simpler baseline.

This gate is especially important for embeddings, rerankers, extra MCP tools, background providers and derived intelligence layers.

---

## INV-021 — Relationship vocabulary parity is executable

The canonical relationship registry is the sole first-party vocabulary authority. A provider may not emit an undeclared relationship kind, and every graph consumer—traversal, persistence, API/SDK serialization and export—must either handle each public relationship or declare an explicit tested exemption.

This is a CI invariant, not documentation discipline. Silent producer/consumer drift is silent partial truth.

## INV-022 — Cached semantic output is bound to extractor semantics

Source bytes alone do not determine extraction validity. Cached lexical/AST/compiler/provider output is reusable only when both the source content identity and the semantic extractor fingerprint match.

The fingerprint must cover only inputs capable of changing semantic output: cache/schema contract, provider/version, language-table version, grammar manifest and provider-specific extraction contract. Unrelated application releases must not invalidate the entire cache.

## INV-023 — Internal identity and portable semantic identity are distinct

Blueprint may keep path-qualified/internal entity IDs optimized for local generations, but semantic providers may attach a portable identity (for example SCIP-native or Kythe/VName-like identity) when evidence supports one.

Portable identity is additive. It exists for federation, external symbols and interchange; it must never force two locally distinct entities to merge merely because names look similar.

## INV-024 — Every automatic pending mark has an automatic clear

If a code path that runs automatically can mark a domain, phase or work item pending, a path that also runs automatically must be able to clear it. A pending mark whose only consumer lives behind a manual command is a canon violation, not a configuration choice.

This applies to watcher domains, phase-2 completion, deferred repair queues and any other latch that participates in a convergence or freshness decision.

The failure mode this forbids is specific and has occurred: the watcher marks a `doc` domain pending on any document change, the only clear runs inside a manual seal step, and every markdown edit therefore pins the convergence barrier permanently. Freshness then reports `changed_since_generation` forever and dependent context returns nothing — while each component passes its own tests.

Producer and consumer reachability is a CI assertion, not documentation discipline. For each domain a producer can mark, a test must demonstrate an automatic path that clears it.

A conforming design keeps per-stage completion state rather than one shared latch — for example distinct content and semantic fingerprints per artifact, so each pass knows exactly what it still owes and no pass depends on a later manual step to release the previous one.

# 4. Canonical data model

The schema may evolve physically, but the logical concepts below are normative.

## 4.1 Repo

Stable repository identity independent of local checkout path.

Required logical attributes:

- `repo_id`
- canonical remote identity when known;
- local roots/worktrees;
- enrollment status;
- default branch metadata;
- provider capability summary.

## 4.2 Revision / source state

Represents committed or workspace source state.

Required concepts:

- commit/tree identity where applicable;
- dirty workspace fingerprint;
- source clock;
- applied/index clock;
- graph generation;
- provider/indexer versions.

## 4.3 Artifact

Source file, documentation file, configuration artifact or other repository-owned item.

## 4.4 Entity

Stable semantic thing: module, namespace, type, function, method, field, route, test, configuration property, handler, contract endpoint, etc.

The entity ID should prefer producer-native stable semantic identity when available and otherwise use Blueprint's deterministic fallback identity rules.

Portable semantic identity is separate from internal entity identity. Where an exact producer-native identifier exists, store it as an additive `portableId`/equivalent rather than coupling local storage identity, display path and federation identity into one field.

## 4.5 Occurrence

Source-anchored appearance of an entity.

Common roles:

- definition;
- reference;
- call;
- import;
- implementation;
- override;
- type reference;
- read/write;
- declaration/registration.

Occurrence contains source range and revision/generation ownership.

## 4.6 Relation

Typed relationship between entities/artifacts, with exact supporting occurrences/evidence and provenance.

Examples:

- `IMPORTS`
- `CALLS`
- `IMPLEMENTS`
- `OVERRIDES`
- `READS`
- `WRITES`
- `REGISTERED_AS_HANDLER`
- `TESTED_BY`
- `BINDS_CONFIG`
- `PROVIDES_CONTRACT`
- `CONSUMES_CONTRACT`

## 4.7 Evidence

Every admitted result must be traceable to evidence:

- artifact/source address;
- source span/hash where applicable;
- provider/analyzer identity and version;
- generation/source state;
- provenance class;
- freshness state;
- inference confidence only where appropriate.

## 4.8 Derived projections

Derived records may be persisted for speed but must carry derivation metadata and remain rebuildable.

### EntryPoint

A statically identified ingress such as HTTP route, CLI command, worker, scheduler, executable module, MCP/RPC tool, UI screen/route, test entry or handler.

### Process / Step

Evidence-backed, disposable execution-flow projection. It represents a structural path the code admits, not a guarantee of runtime execution.

### Contract

Normalized protocol boundary such as HTTP method/path, RPC method, event/topic, MCP tool, package/API signature or schema identity.

A provider fact may be canonical if directly extracted from source. A consumer→provider match across repositories is a derived bridge and must retain evidence from both sides.

### Component / Architecture Flow

Derived cited views over lower-level facts, entry points, processes and contracts.

---

# 5. Semantic authority and resolution lattice

Blueprint must not use “winner by confidence” across heterogeneous producers.

## 5.1 Producer classes

### A. `AUTHORITATIVE_SEMANTIC`

Compiler/indexer-produced semantic facts, typically SCIP/Kythe-class output.

Examples: definitions, references, exact symbol identity, type relations, compiler-resolved calls.

`confidence = null`.

### B. `LIVE_VERIFICATION`

Optional on-demand LSP/IDE cross-check results against already-resolved/current source facts. LSP is not a resident index and is not a canonical graph producer by default.

Allowed outcomes are explicit verification receipts such as agreement, disagreement/conflict, unavailable, timeout or unsupported. A disagreement triggers `resolution_conflict`/degradation and investigation; it does not silently rewrite a canonical edge.

`confidence = null`; this is a verification state, not probabilistic truth.

### C. `RULE_RESOLVED`

Blueprint-owned deterministic name/scope resolution equivalent to a stack-graphs-style or language-specific resolution engine.

`confidence = null` when the rule produces a unique deterministic result; otherwise emit candidates/frontier rather than fabricate certainty.

### D. `STRUCTURAL_RESOLVED`

Tree-sitter plus deterministic import/module/scope resolver.

### E. `FRAMEWORK_RESOLVED`

Framework-specific deterministic extractors for routes, DI, ORM, config, RPC/tools, screen/navigation and similar conventions.

### F. `HEURISTIC_BRIDGE`

Explicitly inferred dynamic/cross-language boundary supported by static evidence.

Examples: callback/event-channel linkage that cannot be proven by a higher semantic tier.

`confidence` allowed.

### G. `UNRESOLVED`

Resolution stopped. Blueprint returns the frontier instead of silently dropping the edge or guessing.

## 5.2 Resolution procedure

For a target source state:

```text
fresh compiler/SCIP/Kythe fact if coherent with source state
        ↓ otherwise
Blueprint deterministic lexical/scope resolver
        ↓ otherwise
current import/module structural resolver
        ↓ otherwise
framework-specific resolver
        ↓ otherwise
explicit heuristic bridge
        ↓ otherwise
UNRESOLVED + reason + bounded candidates

OPTIONAL AFTER RESOLUTION:
LSP/IDE cross-check → agreement receipt | resolution_conflict | unavailable/unsupported
```

## 5.3 Resolution frontier contract

When Blueprint cannot prove a relationship, it should retain:

- source occurrence/address;
- requested relation category;
- last successful semantic tier;
- failure reason;
- dynamic-dispatch category if relevant;
- bounded candidates, if deterministically derivable;
- provider/indexer availability information.

“Unknown” is an acceptable and often superior answer to a plausible fabricated edge.

If the implementation uses a numbered resolution cascade, lower tiers may only fill unresolved space left by stronger tiers. A lower-authority match must never compensate for or override a coherent higher-authority result through scalar scoring.

---

# 6. Freshness model

## 6.1 States

Blueprint distinguishes at least:

- `unwatched`
- `catching_up`
- `current`
- `stale`
- `degraded`

Operational liveness and semantic readiness are separate dimensions.

Blueprint additionally preserves a diagnostic source-state relation beneath the public state: `equal | behind | ahead | diverged | unknown` (or the equivalent existing internal enum). Public `current` requires `equal`; `unknown` is never treated as fresh.

A daemon can be alive while:

- graph generation is missing;
- journal events are pending;
- compiler semantic provider is unavailable;
- an optional projection is stale;
- the repository is otherwise structurally queryable.

## 6.2 Freshness proof

A result may be labeled `current` only when Blueprint can demonstrate:

1. repository/source state is identified;
2. relevant graph generation exists;
3. no known watcher gap applies;
4. observed relevant events through the answer's source clock are applied;
5. bounded dirty-file check does not reveal a newer relevant source state.

If not, return a typed receipt describing the degraded/stale dimension.

## 6.3 File events

Preferred path:

```text
native filesystem event
→ canonical path + excludes
→ persistent event journal
→ debounce/coalesce by path
→ stable read/content digest
→ lexical + Tree-sitter extraction
→ apply owned file delta
→ bounded dependent frontier
→ re-resolve/reparse dependents
→ WAL transaction
→ applied clock
→ convergence check
```

## 6.4 Git transitions

Checkout/merge/rebase/rewrite must not be treated as thousands of unrelated watcher events.

Preferred path:

```text
old tree/source state
→ new tree/source state
→ git diff --name-status
→ add/modify/delete/rename set
→ one journal transaction/batch
→ normal bounded incremental machinery
```

Filesystem events remain the fallback if lifecycle hooks are unavailable.

## 6.5 Query-time repair

For a query that touches recently modified source:

```text
identify involved artifact(s)
→ cheap digest/metadata verification
→ unchanged: answer immediately
→ dirty: repair target file + tiny dependent frontier within hard budget
→ budget exceeded: return stale/degraded receipt; watcher continues independently
```

No full repository traversal is permitted from this path.

---

# 7. Retrieval architecture

Blueprint retrieval is evidence-first, not similarity-first.

## 7.1 Candidate generation tiers

Default sequence:

```text
Tier 0 exact entity / stable identity / path lookup
Tier 1 structural graph relationships
Tier 2 identifier-aware BM25 / lexical search
Tier 3 optional dense semantic retrieval
```

## 7.2 Recall admission remains authoritative

BM25, embeddings and hybrid fusion may discover candidate seeds. They do **not** replace Blueprint's existing non-compensatory evidence/admissibility ranking.

A semantically similar candidate must never outscore a weaker truth class and become trusted evidence merely because vector similarity is high.

## 7.3 Embedding policy

Semantic embeddings and hybrid fusion are **exploratory, benchmark-gated retrieval capabilities**, not committed correctness dependencies and never resolution tiers. Promoting either requires an explicit canon decision plus the agent-outcome gate.

Before enabling a default embedding projection, prove material benefit over:

- exact lookup;
- structural graph expansion;
- BM25;
- compact signatures/AST structure.

If enabled:

- use a small local model;
- lazy-load it;
- version model/provider/dimensions;
- update only changed semantic units;
- treat vectors as disposable projection data;
- do not keep the model resident while idle unless benchmarks justify it.

---

# 8. Semantic/frontend architecture

## 8.1 Baseline structural frontend

Tree-sitter/lexical parsing remains the cheap universal baseline.

## 8.2 Semantic indexer orchestration

Blueprint should own:

```text
discover semantic producer
→ resolve compatible version
→ invoke/index
→ validate output
→ record producer/version/source state
→ ingest normalized facts
→ reconcile with existing producers
```

SCIP is the preferred transport where supported. BPT-006 ingestion remains useful, but ingestion alone is not the complete semantic feature.

**One canonical SCIP normalizer:** all first-party SCIP ingestion paths must share one role/symbol/relationship normalization contract. Language-specific adapters may enrich that normalized representation but may not implement divergent SCIP semantics. Preserve definitions, references, symbol information, relationships, external symbols, package identity and position encoding when present, even if some fields are not exposed immediately.

## 8.3 On-demand LSP verification/cross-check

LSP is optional verification, not a persistent canonical database, not a resident fleet and not a default resolution source.

Blueprint may ask an available host/project LSP to cross-check a bounded resolved symbol/question for:

- definition identity;
- references;
- type/implementation agreement;
- diagnostics relevant to semantic interpretation.

Agreement emits a verification receipt. Disagreement emits a typed `resolution_conflict` and may degrade readiness for the affected answer; Blueprint does not automatically choose the LSP answer. Editing/refactoring remains outside Blueprint ownership.

## 8.3A Declarative/versioned resolution rules

Where language resolution behavior can be expressed declaratively, move it from opaque hard-coded tables into versioned, diffable rule manifests consumed by the Blueprint resolver. Good candidates include exported-scope rules, lexical-parent traversal, import binding patterns, tie-break conditions and per-language feature switches.

The engine remains authoritative for invariants: freshness, tier dominance, admissibility, ambiguity handling and truth classes are **not** user-overridable. Per-repository overrides, if ever allowed, are limited to explicitly lower-tier/project-specific resolution parameters and must be receipted/versioned.

Rule manifests participate in extractor fingerprints and verifier fixtures.

## 8.4 Framework providers

New framework semantics use explicit provider contracts rather than indefinitely expanding one generic “framework facts” bucket.

Provider families include:

- routes/handlers;
- dependency injection;
- ORM/query targets;
- configuration binding;
- RPC/MCP/tool definition→handler;
- UI route/screen/navigation;
- event/callback registration.

Each provider must declare:

- supported framework/version patterns;
- facts/relations produced;
- evidence requirements;
- deterministic vs inferred behavior;
- incremental invalidation ownership;
- degradation/failure reasons.

---

## 8.5 Semantic symbol metadata

Blueprint's semantic layers should converge on a common optional symbol payload sufficient for member/receiver and API reasoning without becoming a statement-level CPG:

- signature and parameters;
- declared/raw declared type;
- return type;
- receiver/declaring type;
- parent semantic entity;
- visibility/export status;
- static/async/abstract/final/override modifiers;
- annotations/decorators;
- generic/type parameters;
- docstring/documentation reference when source-backed.

Whole function bodies need not be duplicated on entity rows when exact source slicing can reconstruct them cheaply.

## 8.6 Project convention facts

Blueprint owns descriptive convention detection when the evidence contract is explicit. Conventions describe recurring current-code behavior; they do not prescribe style or acquire policy authority.

A convention fact must include at least:

- convention kind/scope;
- observed pattern;
- coverage/support count;
- counterexamples;
- source generation/evidence;
- `WeakEvidence`/derived authority.

Examples include naming, error-handling shape, module layout and test placement. Convention facts may enrich orientation and declared-vs-done drift, but never override explicit architecture or exact code facts.

## 8.7 Interchange and historical reads

Blueprint should support deterministic emit-side semantic interoperability without inventing a pseudo-standard. When representable losslessly, export a **real SCIP semantic subset** using standard identities/occurrences. For Blueprint-specific evidence (resolution tiers, receipts, architecture/drift facts), use a versioned Blueprint-native export. Export ordering and schema/version metadata must be deterministic.

Generation history also supports bounded read-only temporal questions such as `fact_at(entity, generation)` and `changed_between(entity, g1, g2)`. This is **not full bitemporality** and requires no second store; reads are limited by retained generations and must preserve the distinction between current source-state truth and historical observation.

# 9. Test and impact semantics

Tests are first-class typed entities.

Structural test reachability is evidence, not measured runtime coverage.

Use explicit semantics:

```text
production entity
→ static TESTED_BY relation when structurally evidenced
→ impact analysis
→ affected-test recommendation
```

If Blueprint cannot establish a relation, return `UNKNOWN`; do not imply “not tested.”

Runtime coverage, if integrated later, is a separate evidence provider.

---

# 10. Entry points, processes and architecture

Architecture understanding should not be synthesized from raw file proximity alone.

Preferred derived substrate:

```text
EntryPoint
→ Process
→ Step
→ Contract
→ Component
```

All of these higher-order structures are disposable cited projections.

Blueprint's architecture APIs/views remain the public semantic capability; materialized projections exist underneath to make repeated queries fast, stable and explainable.

---

# 11. Multi-repository federation

Each repository remains independent:

```text
Repo A generation/evidence
Repo B generation/evidence
Repo C generation/evidence
```

A named federation group is metadata/overlay, not a merged graph.

Cross-repo linkage:

```text
Repo A consumer evidence
→ normalized Contract/Bridge
→ Repo B provider evidence
```

Cross-repository traces cross only explicit bridges. Same-name global matching is prohibited.

---

# 12. Agent integration architecture

## 12.1 Canonical public tool set

Keep:

- `blueprint_recall` — orientation/admission-aware retrieval before repository crawling;
- `blueprint_search` — exact/concept/symbol discovery;
- `blueprint_expand` — relevant structural neighborhood/source context;
- `blueprint_impact` — pre-change/reverse dependency/test/flow impact;
- `blueprint_doc_truth` — document↔code truth reconciliation;
- `blueprint_status` — freshness/readiness/trust diagnostics.

The distinction between semantic API and visible tool menu is deliberate: internal provider/query methods need not become MCP tools.

## 12.2 Live resources

Resources must be graph-backed and repository-scoped rather than placeholders.

Recommended resources:

```text
blueprint://repos
blueprint://repo/{id}/context
blueprint://repo/{id}/architecture
blueprint://repo/{id}/flows
blueprint://repo/{id}/claims
blueprint://repo/{id}/conflicts
blueprint://repo/{id}/receipts
blueprint://repo/{id}/schema
```

## 12.3 Cold-start projection

Generate a tiny deterministic projection such as `.agent/graph/context.json` containing:

- repo/generation/freshness;
- languages;
- major namespaces/components;
- entry points;
- hub symbols;
- recent structural changes;
- known architecture flows;
- doc conflicts;
- coverage/provider gaps.

It is a regenerable projection, not durable memory.

---

# 13. Storage architecture

## 13.1 Keep SQLite/WAL locally

Blueprint should retain SQLite/WAL because it matches:

- local-first deployment;
- portability;
- atomic generations;
- simple backup/rebuild semantics;
- zero-server operation;
- reliable crash recovery.

## 13.2 Change the conceptual boundary

SQLite is the **canonical fact ledger** and may also host efficient local projections. Storage implementation convenience must not collapse semantic distinctions between canonical facts and derived views.

Do not introduce another graph database merely because donor systems use one.

If a future specialized projection requires a different backend, it must be rebuildable from the fact ledger and accessed behind a projection/store interface.

---

# 14. Operational architecture

Preferred resident mode:

```text
Membrane Hub
    ↓
one Blueprint watcher process
    ↓
multiple repository actors
    ↓
one logical writer/lease per repo
```

Fallback when Hub is unavailable but an MCP session is active:

```text
MCP process
    ↓
session-scoped watcher for current repo only
    ↓
dies with MCP session
```

Do not create another permanent daemon solely to emulate this fallback.

---

# 15. Performance doctrine

Blueprint's product value depends on being safe to leave on.

Initial engineering targets—not claims about current performance:

- idle watcher CPU: effectively zero;
- no periodic full repository walks for normal operation;
- resident watcher base RSS target: `<150 MB` on a representative enrolled-repo fleet, measured separately from test runner;
- one-file event → structurally current: p95 `<1.5 s` with the existing 1 s debounce;
- internal one-file repair after debounce: p95 `<500 ms` on representative medium repo;
- no-op relevant-file freshness check: p95 `<50 ms`;
- branch switch work proportional primarily to Git diff rather than number of watcher notifications;
- heavy cold reconcile concurrency: `1` by default unless resource-aware policy permits more;
- no resident embedding model or LSP fleet by default.

Targets must be measured on actual resident processes, not inferred from a subprocess or the test runner RSS.

---

# 16. What Blueprint deliberately will not become

Do not build:

1. one universal graph schema for every program-analysis/context use case;
2. a vector database as truth;
3. line-number-based semantic identity;
4. a 20–80-tool default MCP menu;
5. name-match call edges presented as truth;
6. LLM-generated structural code edges without explicit inference provenance;
7. always-on whole-repository CPG/PDG/taint analysis;
8. durable agent memory inside Blueprint;
9. cross-repo links based on textual coincidence;
10. stale answers labeled current;
11. an always-running LSP fleet;
12. commercial code absorption from incompatible donors.

---

# 17. Architectural definition of done

Blueprint v2 architecture is correctly realized when:

- all facts can identify provenance, source state and evidence;
- canonical facts survive projection rebuilds;
- authoritative facts do not use fake confidence values;
- entity/occurrence identity is formally separate;
- freshness is evaluated before semantic-authority ordering;
- current dirty-file semantics can outrank stale compiler output for that source state;
- unresolved relationships return a frontier rather than fabricated edges;
- normal edits update bounded owned/dependent facts;
- Git transitions are handled as coherent source transitions;
- query-time repair is strictly bounded;
- process/architecture/contracts are recognized as derived projections;
- multi-repo traversals cross explicit bridges only;
- agent-visible tools remain compact;
- the system remains useful with embeddings/LSP/deep analysis disabled;
- optional sophistication cannot contaminate or replace deterministic evidence admission;
- schema/indexer compatibility is mechanically verified rather than assumed;
- partial/truncated extraction cannot replace a known-complete generation;
- incremental repair cannot silently shrink unrelated fact ownership;
- supported semantic relations have source-anchored positive, negative and ambiguity fixtures.


---

# 18. Non-normative implementation references

`04_BLUEPRINT_DONOR_REFERENCE_V2.md` maps the reviewed open-source projects to Blueprint atoms and implementation tasks. It is advisory only: donor architecture, terminology, storage, tool surfaces and scoring models never override this canon.

The later Loom synthesis is treated the same way: its verifier/golden-test/publication-safety ideas are adopted above, while its memory/editing/mandatory-vector/deep-analysis scope is not.
