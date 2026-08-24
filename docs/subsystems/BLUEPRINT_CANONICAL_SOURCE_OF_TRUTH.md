# Blueprint — Canonical Source of Truth

**Status:** Canonical · final architecture · implementation authority  
**Repository:** `Orthic-Labs/Membrane`  
**Subsystem path:** `blueprint/`
**Package:** `@membrane/blueprint`
**Date:** 2026-08-19  
**Authority:** this file supersedes every other Blueprint architecture or implementation plan.

---

## 0. Authority

This document is the single architectural and implementation authority for Blueprint.

It defines:

- what Blueprint is;
- what Blueprint is not;
- the required end-state architecture;
- the canonical ownership of each concern;
- the required runtime and consumer seams;
- the required implementation sequence;
- the capabilities that are part of Blueprint;
- the capabilities that are excluded from Blueprint;
- the qualification gates that define completion.

Earlier planning documents are provenance only. They may explain why a decision was considered, but they do not create requirements after this document is adopted.

The repository remains the authority for current implementation reality. When the repository and this document diverge, the divergence is a defect: either the code must be brought back to this architecture or this document must be deliberately replaced in the same architectural decision.

There are no hidden companion specifications. A missing external document is never a Blueprint-local prerequisite.

### 0.1 Binary architecture rule

Every capability in this document is assigned a binary architectural state:

- **IN** — required in the completed Blueprint system;
- **OUT** — excluded from Blueprint.

An OUT capability does not become part of Blueprint because an implementation agent finds it interesting, a competitor has it, or a library makes it easy. Adding an OUT capability requires a new canonical architectural decision that replaces the relevant decision here.

A capability marked IN is not complete because a stub, schema, flag, or unused module exists. It is complete only when its behavior, integration, failure semantics, measurement, and consumer path are qualified.

### 0.2 Physical co-location does not change ownership

Blueprint is one of the six named subsystems within the Membrane system. That product/system hierarchy does not make Blueprint an in-process module: Blueprint remains independently runnable and retains its own package, process, protocol, storage, watcher/service, testing, and qualification boundaries.

Blueprint and Membrane share one repository so their seam can evolve atomically.

They retain separate:

- package and publish surfaces;
- process/runtime boundaries;
- protocol ownership;
- SQLite stores;
- watcher/service responsibilities;
- tests and qualification;
- semantic ownership.

Membrane may consume Blueprint only through Blueprint-owned public protocol/service surfaces. `engine/**` and `mcp/**` do not import `blueprint/src/**`. Blueprint does not import Membrane engine internals.

Final packaging uses Blueprint as a separately versioned external service. Membrane ships one typed native client, never opens Blueprint SQLite, and typed-degrades Blueprint-dependent requests when service is absent while unrelated Membrane functions continue.

The sibling subsystem named Cortex is Membrane's durable-knowledge engine. Blueprint does not read or write the Cortex store and does not depend on Cortex memory semantics.

> **Physical co-location does not imply semantic ownership.** Blueprint and Membrane share a repository so their seam can evolve atomically; they retain separate package, process, protocol, storage, testing, and responsibility boundaries.

---

# 1. Product purpose

Blueprint is a local repository truth and evidence engine for agents and developer tooling.

The core user interaction is:

> Ask one task-shaped question about a repository and receive the smallest complete, fresh, evidence-backed answer Blueprint can prove — including relevant relationships, source evidence, uncertainty, and disagreement between declared intent and current code.

Blueprint exists to make repository understanding:

- more accurate than ad hoc grep/search;
- cheaper than repeated model-driven exploration;
- faster across repeated agent turns;
- evidence-backed rather than impressionistic;
- explicit about what it cannot prove;
- sensitive to the current worktree, not only the last index;
- capable of distinguishing what documentation declares from what the code currently does.

Blueprint is not primarily a graph database product. The graph is its canonical evidence substrate.

Blueprint is not primarily a search product. Search is one seed mechanism.

Blueprint is not primarily a documentation generator. Generated understanding is a derived view over evidence.

Blueprint is not an agent harness. Agents consume Blueprint.

The completed product loop is:

```text
repository state
    ↓
deterministic evidence
    ↓
truth / declared-vs-done understanding
    ↓
task-shaped RecallCircuit
    ↓
admission / change intelligence when relevant
    ↓
agent or host action
    ↓
measured task outcome
```

If Blueprint builds an excellent graph but does not improve that loop, Blueprint is not complete.

---

# 2. Architecture decisions — IN / OUT

## 2.1 IN — required Blueprint capabilities

The completed Blueprint system includes all of the following:

1. VCS-aware repository observation, dirty-worktree awareness, treeish/baseline inputs and deterministic discovery accounting.
2. Stable repository, file, entity, occurrence, claim and evidence identity.
3. A canonical local SQLite evidence store with atomic generations.
4. Deterministic Phase 1 extraction and graph construction.
5. One provider capability system covering lexical, Tree-sitter, SCIP and repository-domain providers.
6. Exact-first cross-file resolution with explicit ambiguity and unsupported states.
7. Canonical source addresses, spans, content hashes, citation strength and conservative re-anchoring.
8. Typed freshness, invalidation, ingestion and partial-failure semantics.
9. One bounded graph traversal primitive.
10. RecallCircuit as the canonical task-shaped retrieval/execution primitive.
11. Complete evidence paths as the semantic recall unit.
12. Deterministic ranking and result bounding owned by Blueprint.
13. Phase 2 claim verification, declared-vs-done truth, component synthesis and flow synthesis.
14. Explicit fact-to-claim truth binding.
15. Typed contradictions, stale declarations, unsupported declarations, ambiguous declarations and citation failures.
16. Impact, liveness, diff-scoped reasoning, failure-signal resolution and recommended test selection.
17. Named-generation/treeish semantic diff and a history evidence lane.
18. Lower-authority co-change evidence for change reasoning.
19. Explainable change risk composed from named evidence-backed factors.
20. Task-scoped admission/orientation decisions that consume recall and truth.
21. A resident Blueprint service daemon as the primary machine-to-machine query path.
22. CLI, SDK, MCP and legacy-candidate adapters over the same application behavior.
23. A watcher/freshness subsystem owned by Blueprint but separate from query serving.
24. Incremental builds proven semantically equivalent to full builds.
25. Crash-safe generation publication, last-known-good recovery and provider isolation.
26. Security boundaries for repository text, paths, external processes, configuration and data export.
27. Doctor/diagnostic surfaces that expose actionable blind spots rather than vanity counts.
28. Frozen correctness benchmarks, query-plan benchmarks, resource benchmarks and agent-outcome evaluation.
29. Product SLOs and release ratchets.
30. Generated architecture/understanding artifacts as disposable, evidence-cited views — never as truth stores.
31. Bounded cross-repository federation of independently scoped repository slices with independent `repoId`, generation, omissions and receipts.

## 2.2 OUT — excluded from Blueprint

The following are not part of the canonical Blueprint architecture:

1. A second production graph/store backend.
2. A generic graph database abstraction intended to support Neo4j, Kùzu, FalkorDB, RocksDB or other production stores.
3. Vector/embedding retrieval as a Blueprint retrieval lane.
4. Learned rankers.
5. Spectral clustering, graph communities, PageRank/centrality ranking — global or local — as a production evidence-priority mechanism.
6. Model-driven graph traversal.
7. LLM extraction that can create observed code facts.
8. LLM-generated facts that can override deterministic evidence.
9. Live language-server or compiler process invocation as a required query/build dependency.
10. A generic LSP runtime inside Blueprint.
11. A full CodeQL/Joern-style program analysis platform.
12. General statement-level taint/dataflow analysis.
13. A generic structural query language.
14. An AST rewrite or code transformation runtime.
15. Durable conversational memory.
16. Transcript harvesting as repository truth.
17. Final prompt assembly.
18. Final model token-budget policy.
19. Model selection.
20. Agent orchestration.
21. Workflow execution.
22. Host prompt injection.
23. Policy enforcement inside editor/shell/tool runtimes.
24. Merged cross-repository node spaces.
25. A remote/cloud service required for correctness.
26. Full bitemporal storage across the entire graph.
27. A plugin marketplace.
28. Popularity telemetry as ranking authority.
29. Cross-machine mutable graph sharing.
30. Generic generated wikis as an authoritative source.
31. Semantic/hybrid additive weighted ranking that lets vector similarity, centrality or other soft scores compensate for weaker evidence.

If evidence later shows one of these capabilities is necessary, the correct action is an architectural revision, not a hidden sidecar implementation.

---

# 3. System boundary

## 3.1 Blueprint owns

Blueprint owns:

- repository/source observation;
- source identity;
- deterministic extraction;
- provider execution;
- graph construction;
- entities and occurrences;
- source spans;
- evidence;
- provenance;
- generation identity;
- freshness;
- invalidation;
- atomic publication;
- exact-first resolution;
- graph relationships;
- traversal;
- RecallCircuit;
- truth binding;
- claim verification;
- declared-vs-done drift;
- component and flow understanding;
- impact;
- liveness;
- change intelligence;
- historical evidence;
- semantic treeish diff;
- deterministic ranking of its own evidence;
- result bounds;
- omissions;
- diagnostics;
- admission decisions;
- service/CLI/SDK/MCP contracts;
- qualification of its own product value.

## 3.2 Blueprint does not own

Blueprint does not own:

- the caller's final context selection;
- final prompt rendering;
- final model budget;
- the agent's plan;
- model routing;
- conversation memory;
- edit execution;
- shell execution;
- code rewriting;
- approval policy in the host;
- enforcement of an admission decision.

## 3.3 Consumer contract

The primary end-state contract is:

```text
caller / Membrane / host
    → task-shaped Blueprint request

Blueprint resident service
    → generation-bound RecallCircuit
    → truth findings
    → change/admission information when requested

caller / Membrane / host
    → chooses final context
    → chooses enforcement policy
    → renders/injects prompt/context
    → executes the agent/tool action
```

Blueprint remains an independently bounded subsystem within the Membrane system.

The parent-system relationship does not collapse runtime or semantic boundaries.

They do not share a store.

They do not duplicate ranking policy.

They do not duplicate traversal policy.

They do not merge memory semantics.

## 3.4 Cross-repository federation

Cross-repository use is IN only as bounded federation of independent repository slices.

Each repository retains:

- its own graph generation;
- its own `repoId`;
- its own freshness state;
- its own evidence identities;
- its own omissions and coverage;
- its own receipt boundary.

A caller may request several independently scoped slices and combine them at the consumer layer. Blueprint does not raw-merge node spaces, collapse identities across repositories, compute cross-repository PageRank/centrality, or create a shared mutable graph.

When Membrane is the consumer, Membrane owns the cross-repository attention decision.

---

# 4. Locked invariants

These are architectural invariants, not tuning suggestions.

1. Phase 1 is deterministic.
2. Phase 2 is derived and receipt-backed.
3. SQLite is the canonical local truth/evidence store.
4. Derived indexes and generated files are rebuildable and disposable.
5. A generation is atomic.
6. Readers never observe a half-built generation.
7. Exact evidence outranks heuristic relevance.
8. Ambiguity fails closed.
9. A weaker resolution stage cannot break a stronger-stage tie.
10. Repository content is untrusted data, never instruction.
11. Uncertainty is output.
12. Unsupported is distinct from unresolved.
13. Ambiguous is distinct from unresolved.
14. Stale is distinct from missing.
15. Cancelled is distinct from failed.
16. Partial independent successes survive independent failures.
17. Historical evidence never mutates current truth.
18. Generated understanding never becomes observed truth.
19. Blueprint traverses the graph; the model does not infer missing hops.
20. Complete evidence paths outrank disconnected high-scoring fragments.
21. Blueprint may bound its own output but does not own final prompt budgeting.
22. There is one canonical owner for each semantic concern.
23. Adapters may translate contracts; they may not duplicate policy.
24. Incremental execution may reduce work; it may never reduce soundness.
25. Every result is generation-bound.
26. Every durable fact has evidence/provenance sufficient to explain its authority.
27. Every bounded result exposes why it is incomplete.
28. Every sophisticated mechanism must beat the simpler deterministic behavior it replaces.
29. Agent-facing benefit is a release property, not a marketing claim.
30. No IN capability is considered complete while its primary consumer path still bypasses it.

---

# 5. Canonical architecture

Blueprint is organized as interacting planes, not one linear pipeline.

```text
┌──────────────────────────────────────────────────────────────────────┐
│                         CONSUMER / HOST                              │
│          Membrane · agents · IDE · CI · CLI · other hosts           │
└───────────────────────────────┬──────────────────────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────────────┐
│                      APPLICATION / ADMISSION                         │
│ service API · recall · impact · documentTruth · architecture         │
│                         status · resolve                             │
└───────────────────────────────┬──────────────────────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────────────┐
│                            RECALL PLANE                              │
│ seed resolution → traversal policy → bounded traversal              │
│ → complete paths → truth/analysis attachment → ranking → bounds      │
│                         → RecallCircuit                              │
└───────────────────────┬───────────────────────┬──────────────────────┘
                        │                       │
                        ▼                       ▼
┌──────────────────────────────────┐  ┌────────────────────────────────┐
│     TRUTH / UNDERSTANDING        │  │      DERIVED ANALYSIS          │
│ claims · grounding · drift       │  │ impact · liveness · tests      │
│ contradictions · components      │  │ diff · history · change risk   │
│ flows · citations · findings     │  │ semantic treeish diff          │
└───────────────────────┬──────────┘  └──────────────────┬─────────────┘
                        │                                │
                        └──────────────┬─────────────────┘
                                       ▼
┌──────────────────────────────────────────────────────────────────────┐
│                           GRAPH CORE                                 │
│ SQLite · generations · identity · entities · occurrences · evidence  │
│ relationships · provenance · resolution · traversal · freshness      │
└───────────────────────────────┬──────────────────────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────────────┐
│                         PROVIDER PLANE                               │
│ lexical · Tree-sitter · SCIP · framework · schema · IaC · rules/docs │
│        one capability contract · typed evidence · typed failure       │
└───────────────────────────────┬──────────────────────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────────────┐
│                           SOURCE PLANE                               │
│ VCS discovery · files · dirty overlay · treeish · merge-base         │
│ source hashes · ingestion dispositions · repository confinement      │
└──────────────────────────────────────────────────────────────────────┘

Cross-cutting:
determinism · security · invalidation · diagnostics · measurement
```

The dependency direction is downward for evidence creation and upward for query composition.

No upper plane may reach around the graph core to invent facts.

---

# 6. Source plane

## 6.1 Purpose

The source plane answers:

> What repository state exists, what changed, and what exact bytes/treeish does this request refer to?

It does not rank context.

It does not assemble model candidates.

It does not own graph traversal.

It does not decide relevance.

## 6.2 Canonical responsibilities

The source plane owns:

- repository root identity;
- Git-aware discovery;
- deterministic filesystem fallback outside Git;
- HEAD/index/worktree distinction;
- untracked files;
- named treeish;
- merge-base / PR target;
- dirty overlays;
- file content hashes;
- source observation digests;
- ignore/exclusion handling;
- binary/oversize/unreadable classification;
- nested-repository classification;
- symlink/path confinement;
- ingestion input accounting.

Every considered source reaches one terminal disposition.

Required dispositions include:

```text
indexed_exact
indexed_degraded
metadata_only
ignored_user
ignored_system
binary
oversized
unsupported
unreadable
nested_repo
external_link
parse_error
provider_timeout
provider_crash
```

There is no silent disappearance.

## 6.3 Current `src/sources/` correction

The current `src/sources/` tree mixes source observation with task/context candidate fan-out.

That is not the end-state boundary.

The required migration is:

- `dirty-files`, Git/worktree observation and live-overlay behavior stay in the source plane;
- task anchors move to Recall seed resolution;
- graph resolution moves to Recall/Graph Core;
- rules/documents move through the provider/truth ingestion path;
- candidate assembly stops being a source-plane responsibility;
- any existing `ContextCandidate[]` fan-out remains only as an adapter during migration and is deleted once all consumers use RecallCircuit.

---

# 7. Provider plane

## 7.1 One provider system

Blueprint currently contains two overlapping provider concepts:

- extraction/provider logic under `src/graph/`;
- a newer capability/permission provider system under `src/providers/` and `src/sdk/providers.mjs`.

The completed architecture has one provider system.

`src/providers/` is the canonical provider definition/registration layer.

`src/graph/` consumes provider results and owns graph semantics.

`src/sdk/providers.mjs` exposes the supported public provider registration/types.

Legacy graph provider modules are adapted into this system and then lose provider-registry ownership.

The package/export surface must include the canonical provider implementation. A public SDK module must never import an implementation path omitted from the published package.

## 7.2 Provider contract

The provider contract is capability-based rather than a giant interface requiring every provider to implement every operation.

Canonical shape:

```text
Provider {
  id
  version
  kind
  languages / domains
  protocolRange
  capabilities
  permissions
  probe()
  collect()
}

ProviderResult {
  facts[]
  evidence[]
  diagnostics[]
  omissions[]
  precision
  soundness
  fallback
  providerId
  providerVersion
}
```

Capabilities include the semantic operations Blueprint actually uses:

```text
definitions
references
imports
exports
relationships
types
symbolRoles
enclosingRanges
diagnostics
frameworkFacts
schemaFacts
iacFacts
documentClaims
ruleDeclarations
```

A provider must explicitly report a capability as supported or unsupported. Missing behavior cannot be inferred from the absence of output.

## 7.3 Required provider families

The completed Blueprint system contains:

- deterministic lexical extraction;
- Tree-sitter structural extraction;
- SCIP ingestion and resolution;
- framework providers that emit deterministic repository evidence;
- schema/IaC providers that emit deterministic repository evidence;
- rules/document providers that emit declarations/claims with source evidence.

Blueprint does not require live compiler or LSP processes.

SCIP is the exact-analysis upgrade path where an index exists.

## 7.4 Provider permissions

The trusted provider path defaults to:

- repository read;
- no network;
- no arbitrary process execution;
- bounded memory/work;
- explicit timeouts;
- typed crash/hang/cancel outcomes.

A provider that requires behavior outside that trust boundary is not part of canonical Blueprint.

---

# 8. Graph core

## 8.1 Canonical store

SQLite remains the only production graph/evidence store.

The graph core owns:

- generation metadata;
- nodes/entities;
- occurrences;
- edges/relationships;
- evidence;
- provenance;
- claims and truth bindings where persistence is required;
- diagnostics/findings where persistence is required;
- source/index metadata;
- indexes;
- transactional publication.

A graph generation is immutable once published.

A new generation is built/staged, verified, then atomically adopted.

A failed or cancelled build never replaces the last-known-good generation.

## 8.2 Identity

Blueprint distinguishes:

```text
repository identity
file/source identity
entity identity
occurrence identity
claim identity
evidence identity
generation identity
```

Entities are not occurrences.

Paths and line numbers are locations, not stable semantic identities.

Content hashes verify bytes; they are not a substitute for semantic continuity.

The completed identity system includes:

- exact content identity;
- route-derived definition identity;
- structural fingerprint;
- extraction fingerprint;
- deterministic rename/move reconciliation;
- explicit ambiguity events.

Equal candidates remain ambiguous.

## 8.3 Evidence

Canonical evidence contains:

```text
source address
source span when applicable
content hash
provider id/version
generation
confidence / precision
truth class
freshness
```

Truth classes include:

```text
observed
asserted
derived
historical
unknown
```

Freshness includes:

```text
current
dirty
stale
superseded
invalid
unknown
```

Provenance is set-valued. Independent evidence is preserved rather than overwritten.

## 8.4 Source address and span

Blueprint uses one source-address grammar and one span algebra.

A reusable span records:

- repository-relative address;
- file content hash;
- start/end convention;
- position encoding;
- span hash when available;
- enclosing symbol identity when available.

All path normalization funnels through one canonical POSIX-normalization function before identity/hash construction.

Re-anchoring follows this strict ladder:

```text
exact enclosing entity identity
→ exact quoted text near prior range
→ deterministic structural fingerprint
→ normalized text under a strict uniqueness threshold
→ stale / ambiguous
```

Re-anchoring never silently attaches old evidence to a different fact.

Citation strength is distinct from truth confidence.

---

# 9. Resolution

## 9.1 One exact-first resolution pipeline

All providers feed one shared resolution owner.

Canonical cascade:

```text
R0 exact SCIP/compiler-produced identity already present in evidence
R1 exact local lexical/scope identity
R2 explicit import/alias identity
R3 exact same-file definition
R4 deterministic module/package resolution supported by the provider/language
R5 bounded re-export/SCC closure
R6 unique project qualified-name resolution
R7 unique project bare-name heuristic
R8 ambiguous / unresolved / unsupported
```

Rules:

- stronger stages dominate weaker stages;
- same-tier ambiguity stops resolution;
- a weaker stage cannot select a winner after a stronger tie;
- every resolution records strategy and candidate count;
- unsupported language/package semantics remain typed unsupported;
- resolution diagnostics are durable enough to explain coverage and failures.

## 9.2 Language/module semantics

Blueprint supports the exact module semantics required for the languages it claims at each declared capability level.

The provider capability matrix is the product truth.

Blueprint does not pretend that one universal import/MRO heuristic works across languages.

Where a language/provider cannot prove a semantic relationship, Blueprint reports the lower capability rather than guessing.

## 9.3 SCIP

SCIP is fully consumed when present.

Required SCIP handling includes:

- document position encoding;
- symbol kind;
- relationships;
- symbol role bitset;
- enclosing range;
- definition/reference identity;
- diagnostics;
- referential integrity validation.

A SCIP fact may supersede a heuristic edge for authority/ranking, but the prior heuristic provenance is not silently destroyed.

---

# 10. Freshness and invalidation

## 10.1 Freshness state

Freshness is a typed state machine, not an mtime check.

The system distinguishes:

```text
fresh
dirty
stale_behind
stale_ahead
stale_diverged
unknown
missing
incomplete
```

`unknown` is never treated as fresh.

Every public answer includes generation and freshness state.

## 10.2 Read-time honesty

An edit followed immediately by a query must not silently return "fresh" old evidence.

Blueprint achieves honest edit→query behavior through:

- dirty overlay awareness;
- bounded read-repair/freshness verification;
- watcher-driven rebuild;
- generation pinning.

## 10.3 Invalidation DAG

There is one explicit invalidation dependency DAG:

```text
source bytes
→ parse artifact
→ extracted entities/occurrences/local facts
→ identity + import/export surface
→ resolution
→ graph edges
→ derived analyses
→ truth/understanding
→ RecallCircuit projections / generated artifacts
```

Provider version, rules/config, schema version and generation identity are correctness parents where applicable.

Every derived artifact declares its invalidation parents.

There are no feature-specific shadow invalidation systems.

## 10.4 Incremental equality

Incremental and full rebuilds must produce the same canonical semantics.

Required proof includes:

- randomized file order;
- add/remove/rename/move;
- provider-version changes;
- config/rules changes;
- crash recovery;
- dirty overlay;
- 1/10/100-file changes;
- repeated no-op builds.

Tree-sitter subtree changed-range optimization is OUT. Blueprint uses deterministic file/entity-level incremental invalidation and closure repair rather than introducing a second correctness path whose value has not been established.

---

# 11. One traversal primitive

The graph has one bounded indexed traversal primitive.

`neighbors`, `impact`, RecallCircuit and related graph walks converge on that primitive.

The primitive supports:

- generation pinning;
- direction;
- allowed relationship kinds;
- maximum hops;
- maximum nodes;
- maximum edges;
- cancellation;
- deterministic total ordering;
- evidence requirements;
- visited/hydrated/returned accounting.

Bounds are enforced during expansion, not after traversal.

Normal task-shaped lookup may not scan all symbols or all edges.

Critical seed/traversal SQL paths are protected by `EXPLAIN QUERY PLAN` tests.

---

# 12. RecallCircuit

## 12.1 Purpose

RecallCircuit is Blueprint's canonical query execution primitive.

It replaces:

```text
keyword search
→ flat candidates
→ model infers relationships
```

with:

```text
task
→ seed resolution
→ policy selection
→ bounded predicate-aware graph traversal
→ complete evidence paths
→ truth / analysis attachment
→ deterministic ranking
→ bounded circuit
→ caller decides how to use it
```

The model does not perform multi-hop graph retrieval.

## 12.2 Seed resolution

Seed precedence:

```text
generation-valid node/entity IDs
→ exact source addresses / paths / anchors
→ exact qualified symbols
→ indexed exact symbol terms
→ bounded indexed lexical terms
→ unresolved / ambiguous
```

Task text is not converted into an unbounded wildcard scan.

Unknown vocabulary does not justify a vector lane. Blueprint uses its indexed lexical, structural, documentation, component/flow and graph evidence.

## 12.3 Policies

Required policy families:

```text
dependency.forward
impact.reverse
callgraph.forward
test.coverage
config.consumers
architecture.boundary
explore.both
```

Each policy declares:

- direction;
- allowed relationship kinds;
- maximum hops;
- maximum seeds;
- maximum paths;
- maximum nodes;
- maximum edges;
- evidence requirement.

Callers may tighten these limits but may not silently widen them.

## 12.4 Complete path semantics

A complete evidence path is the primary recall unit.

Every path includes:

- stable path id;
- seed identity;
- terminal identity;
- ordered nodes;
- ordered edges;
- evidence references;
- minimum edge tier;
- evidence coverage;
- complete/partial state;
- omission reasons where relevant.

Equivalent paths are deduplicated before ranking.

Terminal filtering occurs before circuit identity is computed.

## 12.5 Circuit identity

A RecallCircuit is generation-bound and deterministically identified from its visible semantic projection.

A cursor binds to:

- generation;
- circuit digest;
- relevant request/policy identity.

A cursor from an old generation fails closed.

## 12.6 Ranking

Ranking is non-compensatory.

Order of authority:

```text
admissibility / evidence mode
→ minimum edge tier
→ seed exactness
→ evidence coverage
→ truth relevance
→ analysis relevance
→ mean edge confidence
→ hop count
→ indexed lexical/query-shape relevance
→ deterministic tie-break
```

No scalar lexical score may compensate for weak evidence.

No ranking signal may convert unresolved to exact.

No ranking signal may cross an admission boundary.

No learned ranker exists in Blueprint.

No vector similarity, hybrid weighted sum, PageRank or centrality score exists in the canonical RecallCircuit ranking path. Current semantic/hybrid ranking stubs and local-PageRank neighborhood ranking are migration residue and are removed under §27.

## 12.7 Bounds

Blueprint owns the size of the evidence structure it returns.

When a cap binds:

```text
complete high-authority path
> complete lower-ranked path
> partial path
> disconnected node
```

Omissions are reported by reason.

Blueprint does not reserve answer tokens or decide the caller's final prompt layout.

---

# 13. Truth and Phase 2 understanding

## 13.1 Phase distinction

Phase 1 produces deterministic observed repository evidence.

Phase 2 produces derived understanding over a sealed Phase 1 generation.

Phase 2 may interpret evidence.

Phase 2 may never manufacture Phase 1 facts.

## 13.2 Required Phase 2 outputs

The completed system produces and exposes:

- repository/document claims;
- claim evidence;
- claim grounding state;
- declared-vs-done findings;
- contradictions;
- stale declarations;
- unsupported declarations;
- ambiguous declarations;
- component synthesis;
- product/system flow synthesis;
- architecture understanding;
- generated understanding artifacts with citations.

## 13.3 Claim truth states

Grounding outcomes include:

```text
grounded_direct
grounded_indirect
unsupported
contradicted
ambiguous
stale_reference
```

Text similarity alone cannot create a truth binding.

A contradiction requires an explicit tested relationship between the declaration/claim and the graph evidence being compared.

## 13.4 Explicit truth binding

Claims enter RecallCircuit only through an explicit fact/occurrence/evidence join.

`claims[]` and `contradictions[]` are never populated from nearby text merely because it looks relevant.

The truth-binding owner is responsible for:

- claim identity;
- evidence links;
- citation validation;
- grounding outcome;
- invalidation;
- contradiction evidence;
- stale-reference evidence;
- provenance.

## 13.5 Declared-vs-done

Blueprint preserves both sides:

```text
declared intent
current deterministic code evidence
```

It does not rewrite one to match the other.

A drift finding identifies:

- the declaration;
- the current evidence;
- the mismatch classification;
- citation strength;
- generation;
- confidence;
- invalidation state.

## 13.6 Component and flow synthesis

Component/flow synthesis is a derived evidence-backed layer.

Every synthesized component or flow must expose the evidence from which it was derived.

If Blueprint cannot support a synthesized relationship with evidence, it is omitted or marked unsupported.

Generated `architecture.md` / understanding views are read surfaces over the derived state. They are never the canonical truth store.

## 13.7 Phase 2 provider receipts

Any model/provider used to perform judgment or synthesis records:

- provider/model identity;
- provider/model version;
- generation;
- input evidence digest;
- output digest;
- confidence;
- invalidation inputs.

A model judgment remains `derived`.

The absence/failure of a judgment provider produces a typed incomplete Phase 2 result; it does not change Phase 1 truth.

## 13.8 Phase 2 judgment execution contract

Phase 2 execution is bounded and explicit.

The deterministic substrate:

1. pins one sealed Phase 1 generation;
2. identifies the claims/dimensions whose inputs changed;
3. builds the exact evidence pack for each judgment;
4. computes input fingerprints;
5. reuses only judgments whose fingerprints remain valid.

Fresh judgment is executed by exactly one configured Phase 2 judgment provider for that run. The provider may be a local or remote model/provider, but it is never allowed to inspect arbitrary repository state outside the supplied evidence pack.

The execution owner is the Blueprint application/service layer. It is reachable through:

```text
Blueprint service
    phase2 plan
    phase2 verify
    phase2 synthesize
    phase2 seal
```

Operator CLI commands use the same integrated Blueprint implementation and do not provide a separate standalone fallback.

Every invocation is bounded by:

- absolute deadline;
- maximum claims/dimensions per run;
- maximum input evidence bytes/tokens;
- provider/model allowlist;
- maximum retry count;
- cancellation;
- structured output schema.

Failure states are typed:

```text
judgment_provider_unavailable
judgment_timeout
judgment_cancelled
judgment_invalid_output
judgment_budget_exhausted
judgment_incomplete
```

A failed or absent judgment produces incomplete Phase 2 output with receipts. It never creates Phase 1 facts, never silently reuses stale judgments, and never blocks deterministic Phase 1 publication.

---

# 14. Change intelligence

Blueprint includes change reasoning because the consumer is an agent that edits code.

## 14.1 Change seeds

Required change inputs:

- diff hunks;
- file:line;
- stack traces;
- failing tests;
- test IDs;
- changed files;
- named treeish / merge-base.

They resolve into graph seeds with exact evidence where available.

## 14.2 Impact

Impact answers expose:

- seed;
- relationship path;
- `via` edge;
- depth;
- confidence/authority;
- liveness;
- omissions;
- generation.

Structural adjacency is not automatically semantic impact.

## 14.3 Liveness

Liveness is tri-state:

```text
LIVE
UNREACHED
UNKNOWN
```

Zero inbound edges are not dead-code proof.

## 14.4 Test selection

Blueprint returns `recommendedTestSet`, never "minimal tests" without proof.

The result includes:

- selected tests;
- evidence/reason per test;
- uncovered impact;
- coverage state;
- omissions.

## 14.5 Co-change

Co-change is included as a lower-authority historical signal.

It may influence change risk and prioritization.

It may not satisfy an exact dependency or truth gate.

It is bounded and deterministic.

## 14.6 Change risk

Change risk is not a black-box score.

It is a decomposition of named factors such as:

- blast radius;
- relationship uncertainty;
- interface change;
- liveness;
- critical config touch;
- coverage blind spot;
- historical co-change;
- stale/contradicted declarations.

The caller can inspect each factor.

---

# 15. History evidence

History is included, but it is evidence, not current truth.

## 15.1 Required history capabilities

Blueprint supports:

- named sealed generations;
- named Git treeishes;
- merge-base/PR target;
- selected commit metadata;
- historical facts;
- semantic entity/relationship diff across baselines;
- derived evolution findings;
- co-change.

## 15.2 Current truth isolation

Historical facts cannot overwrite current fact state.

Evolution statements are `derived` and cite the exact historical facts/commits used.

## 15.3 No full bitemporality

Full graph-wide bitemporal storage is OUT.

Generation identity plus explicit historical evidence records are the canonical temporal mechanism.

Lifecycle timestamps are used only for records whose own lifecycle requires them, such as findings/suppressions/operations.

---

# 16. Admission / orientation

## 16.1 Purpose

Admission is Blueprint's task/change decision surface.

Blueprint answers:

> Given this task, repository state, requested scope and relevant truth, is the evidence state sufficient to proceed, does scope need expansion, or is there an evidence-backed reason to block?

## 16.2 Decision contract

Required actions:

```text
allow
continue
block
noop
```

A decision exposes:

- reason code;
- generation;
- freshness;
- RecallCircuit reference;
- truth findings;
- allowed scopes;
- omissions;
- claim boundary;
- receipt;
- next action.

## 16.3 Recall + truth integration

`recall()` consumes the same canonical RecallCircuit and truth findings used elsewhere.

It does not build an independent candidate/ranking system.

Task orientation can therefore include:

- relevant code path;
- relevant docs/ADR/rules;
- declared-vs-done drift;
- stale evidence;
- ambiguity;
- impact/test information when the request includes a proposed/current change.

## 16.4 Blocking semantics

Blueprint may return `block` for evidence-backed conditions such as:

- missing/incomplete graph;
- generation mismatch;
- unresolved required anchor;
- explicit repository rule violation;
- configured hard architectural constraint violation;
- required evidence unavailable under the requested policy.

Informational contradictions do not automatically become policy.

The host owns whether an admission action is enforced in the editor, shell, CI or agent runtime.

Blueprint returns the decision.

The host enforces it.

---

# 17. Runtime architecture

## 17.1 Resident service is primary

The primary machine-to-machine Blueprint path is the resident service daemon.

The daemon owns:

- IPC endpoint;
- request/response envelopes;
- generation-pinned read sessions;
- deadlines;
- cancellation;
- bounded queues;
- per-repository request serialization where required;
- read-only query handles;
- build singleflight coordination.

The current Unix socket / Windows named-pipe architecture is the correct runtime shape.

## 17.2 Watcher is not the query server

The watcher owns:

- repository change observation;
- debounce;
- dirty state;
- rebuild triggering;
- freshness progression.

The daemon owns queries.

The watcher and daemon may share lifecycle under `blueprint service run`, but they remain separate responsibilities.

## 17.3 Required daemon methods

The service protocol includes at minimum:

```text
status
search
resolve
expand
impact
architecture
documentTruth
recall
build
```

`recall` is a first-class protocol method.

Blueprint-owned wire schemas are canonical under `blueprint/schemas/**` after the monorepo migration. SDK/type bindings and any consumer-side generated bindings are projections of those schemas, not independent contract authorities.

## 17.4 Application service

`src/lib/application/service.mjs` is the canonical public application behavior.

CLI, MCP, SDK and daemon call it.

Business/query policy is not copied into transport adapters.

## 17.5 CLI candidate adapter

CLI remains a supported human/machine adapter.

`scripts/blueprint-recall.mjs` is a lean command-line adapter over the canonical recall behavior.

`scripts/blueprint-candidates.mjs` flattens/translates RecallCircuit & owns no independent candidate algorithm.

There is one recall implementation.

## 17.6 Membrane seam

Membrane's primary Blueprint path is:

```text
Membrane
→ persistent Blueprint daemon client
→ recall
→ RecallCircuit
```

Membrane never spawns a new Node process for normal Blueprint queries.

The seam remains generation-pinned and fail-closed on mismatch.

---

# 18. Result and error contracts

## 18.1 Canonical result

All bounded application responses converge on a result envelope equivalent to:

```text
BlueprintResult<T> {
  invocation {
    status: ok | partial | failed | cancelled
    generation
    freshness
    duration
  }
  outcome
  evidence[]
  omissions[]
  diagnostics[]
  claimBoundary
  nextActions[]
}
```

The exact wire version may be versioned, but semantics are canonical here.

## 18.2 Epistemic state

Bounded results expose whether they are:

```text
exact
lower_bound
```

and why.

Causes can include:

- unsupported provider capability;
- unresolved/ambiguous resolution;
- truncated traversal;
- external dependency;
- stale evidence;
- dirty overlay;
- provider failure;
- partial history;
- unavailable Phase 2 judgment.

## 18.3 Error taxonomy

Required stable classes include:

```text
INVALID_INPUT
OUTSIDE_REPO
NOT_INDEXED
STALE_GENERATION
PROVIDER_UNAVAILABLE
PROVIDER_FAILED
UNSUPPORTED
AMBIGUOUS
RESOURCE_LIMIT
CANCELLED
STORE_CORRUPT
STORE_BUSY
PERMISSION_DENIED
INTERNAL
```

Retryability and partial-result availability are fields, not guessed from prose.

---

# 19. Diagnostics and doctor

Blueprint diagnostics answer:

> What can Blueprint currently prove about this repository, and exactly where are the blind spots?

Doctor reports findings, not only counts.

Examples:

```text
18 unresolved imports
  12 heuristic-resolvable
  6 unsupported module semantics

4 same-tier ambiguous symbol resolutions

2 documentation references point to superseded source spans

93.2% exact/AST relationship coverage
```

Required diagnostic dimensions include:

- ingestion coverage;
- provider capability/fallback;
- parse health;
- resolution exactness;
- ambiguity;
- unsupported semantics;
- stale citations;
- truth drift;
- provider crashes/hangs;
- freshness;
- graph/store health;
- generated artifact drift.

A finding has stable semantic identity when multiple producers/lifecycle operations require a registry.

Suppressions are centralized and auditable.

---

# 20. Generated artifacts

Generated artifacts are included as derived read surfaces.

They obey:

- generation binding;
- evidence citations;
- deterministic rendering;
- render/input hashes;
- unchanged render = zero write;
- hand-edit detection;
- stale output reconciliation.

Generated files are never imported back as authoritative evidence merely because Blueprint generated them.

The canonical truth remains the evidence graph plus receipt-backed derived truth state.

---

# 21. Security and trust boundary

Blueprint is local-first and treats repository content as untrusted input.

Required controls:

- canonical/symlink-resolved path confinement;
- output-directory self-exclusion;
- binary/NUL/oversize classification before parsing;
- bounded parser input;
- bounded provider input;
- typed process timeout;
- typed provider crash/hang;
- secret redaction in diagnostic text;
- config values that affect truth participate in identity via non-secret digests;
- zero-egress trusted-path tests;
- no remote correctness dependency;
- export/shareability classification;
- safe handling of machine-local paths;
- refusal to treat cloud-synced mutable SQLite as a safe default store.

Provider crashes cannot corrupt or partially publish a generation.

---

# 22. Store/runtime recovery

Required durability behavior:

```text
build
→ stage
→ verify
→ adopt atomically
→ preserve last-known-good
```

Verification before adoption includes:

- schema version;
- generation envelope;
- referential integrity;
- Merkle/source observation consistency;
- provider-version compatibility;
- required indexes.

Recovery distinguishes lock/race from corruption.

Blueprint exhausts a bounded retry/recovery path before destructive rebuild.

Durable operation state records enough information to recover or report the interrupted operation.

Provider workers are isolated where a crash/hang can otherwise take down the main build/service process.

Cancellation reaches long provider loops and is awaited before cleanup.

---

# 23. Product measurement and SLOs

Measurement is part of the architecture because Blueprint is an agent tool.

## 23.1 Frozen evaluation

Blueprint maintains frozen fixture repositories and task corpora with:

- checksums;
- pinned upstream SHAs;
- allowed alternates;
- whole-graph goldens;
- per-language/provider snapshots.

Holdout mutation is a hard failure.

## 23.2 Correctness metrics

Required metrics include:

- exact resolution precision/recall;
- ambiguity honesty;
- unresolved honesty;
- node-level precision/recall;
- span-level precision/recall;
- path correctness;
- path completeness;
- claim grounding fidelity;
- contradiction fidelity;
- stale-reference fidelity;
- change impact correctness;
- test-selection coverage;
- incremental=full equality.

## 23.3 Retrieval metrics

Required metrics include:

- recall@k;
- MRR;
- nDCG where ranked sets apply;
- decoy resistance;
- dense-hub behavior;
- ambiguous seed behavior;
- no-seed abstention;
- SQL work;
- visited vs hydrated vs returned.

## 23.4 Resource metrics

Required metrics include:

- warm daemon recall p50/p95;
- cold command latency;
- daemon IPC overhead;
- build time;
- incremental build time;
- edit→truthful-query latency;
- peak RSS;
- steady daemon RSS;
- DB/index size;
- CPU;
- subprocess count;
- handle count;
- queue time;
- work avoided by incrementality.

## 23.5 Product SLO file

Before release, Blueprint maintains a versioned `evals/slo.json` (or one equivalently canonical SLO artifact) containing numeric acceptance targets for the frozen reference hardware and fixture scales.

A release cannot be declared complete without numeric targets.

The SLO file must cover at least:

- 10k-edge repository;
- 100k-edge repository;
- 500k-edge repository;
- 1-file edit;
- 10-file edit;
- 100-file edit;
- warm resident recall;
- daemon-unavailable typed failure with no subprocess fallback.

Targets may tighten through normal changes.

Weakening a target requires an explicit architectural/release decision with benchmark evidence.

## 23.6 Resident-query product gate

At equal RecallCircuit semantics, the resident daemon path must materially outperform the per-query subprocess path.

The release gate is:

- no correctness regression;
- no evidence/omission regression;
- lower p50 and p95 latency;
- no increase in per-query process creation;
- bounded steady-state RSS.

The subprocess path is not accepted as the normal performance baseline after daemon adoption.

## 23.7 Agent outcome gate

Blueprint maintains an A/B harness:

```text
same task corpus
same model
same starting repository
same tool policy except Blueprint availability

A: no Blueprint
B: Blueprint
```

Measure:

- task accuracy;
- input tokens;
- Blueprint calls;
- total tool calls;
- wall-clock;
- citation fidelity;
- overclaim rate.

A release affecting Recall, truth, ranking, admission, change intelligence or the consumer seam must:

1. not reduce task accuracy;
2. not worsen evidence/citation fidelity;
3. not increase overclaiming;
4. improve at least one agent-cost dimension materially;
5. avoid a material regression in the remaining cost dimensions.

A feature that makes the internal system more sophisticated but does not pass this gate does not justify its complexity.

---

# 24. Current-repository corrections that are mandatory

The architecture review of current `main` exposed several concrete convergence tasks.

## 24.1 Baseline authority

The prior source document pinned `a91909c...` as "current main".

That is stale.

This document is reviewed against:

```text
bd46965d6738657db6ed95afad1dc622ce1c5b95
```

Architecture documents use the phrase **architecture review baseline**, not "current main forever".

## 24.2 Provider convergence

Current provider ownership is split across:

```text
src/graph/*provider*
src/providers/**
src/sdk/providers.mjs
```

This must converge per §7.

No third provider contract is introduced.

## 24.3 Package integration

The published package surface must include every implementation path required by public SDK exports.

`src/providers/` must not be omitted while `src/sdk/providers.mjs` imports it.

## 24.4 Sources boundary

`src/sources/` currently contains context-candidate/planner-era responsibilities.

Those responsibilities must be separated per §6.3.

## 24.5 Recall service

The daemon already exists and is the correct runtime substrate.

Add `recall` to:

```text
src/lib/application/service.mjs
src/service/protocol.mjs
src/service/server.mjs
src/service/client.mjs
```

and route all adapters through that behavior.

## 24.6 Admission integration

Current admission/orientation logic uses candidate-set behavior.

It must consume canonical RecallCircuit + truth findings.

## 24.7 Phantom seam dependency

No Blueprint rule may claim a retired phantom seam-contract path is a required local dependency.

Blueprint's side of every consumer seam is specified in-repo.

## 24.8 Generated architecture truth

A generated `docs/architecture.md` that reports no synthesized components/flows is an honest incomplete artifact, not proof that the architecture capability exists.

Completion requires the Phase 2 implementation and qualification in this document.

---

# 25. Canonical file ownership

All Blueprint-internal paths in this document are subsystem-relative. In the monorepo, prefix them with `blueprint/`.

One semantic concern has one canonical owner.

| Concern | Canonical owner |
|---|---|
| source observation / VCS / dirty overlays | `src/sources/**` after boundary cleanup |
| provider definitions / permissions / capabilities | `src/providers/**` |
| public provider types/registration | `src/sdk/providers.mjs` + schemas/types |
| lexical / Tree-sitter / SCIP adapters | provider modules; graph legacy adapters removed after migration |
| graph store / generations | `src/graph/store-sqlite.mjs` and generation/store modules |
| source addresses/spans | `src/graph/source-address.mjs`, `src/graph/source-span.mjs` |
| identity / reconciliation | graph identity modules |
| import map / resolution | `src/graph/resolution/**` / canonical resolution owner |
| relationship vocabulary / confidence | graph relationship/confidence modules |
| traversal primitive | graph traversal/store layer |
| seed resolution | `src/graph/seed-resolver.mjs` |
| traversal policy | `src/graph/traversal-policy.mjs` |
| RecallCircuit | `src/graph/recall-circuit.mjs` |
| recall rendering / wire projection | recall renderer/contracts |
| truth binding | `src/graph/truth-binding.mjs` + truth modules |
| claims / drift / component-flow understanding | truth/Phase 2 owner |
| change seeds / impact / tests / liveness | graph analysis/change modules |
| history / semantic diff / co-change | `src/graph/history/**` + snapshot owner |
| findings / diagnostics | findings owner + `src/lib/operations/doctor.mjs` |
| freshness | barrier/freshness + source observation |
| admission | `src/lib/admission.mjs` consuming application recall/truth |
| public application API | `src/lib/application/service.mjs` |
| daemon | `src/service/**` |
| watcher | watcher subsystem; no query policy |
| CLI | `scripts/blueprint.mjs`, lean adapters |
| MCP | MCP adapter only |
| candidate compatibility | `scripts/blueprint-candidates.mjs` translating RecallCircuit |
| security/redaction/path confinement | `src/lib/**` security owners |
| generated artifacts | generated-artifact owner |
| qualification | `tests/**`, `evals/**`, `bench/**` |

A named file may change if the repository already has an equivalent canonical owner. The semantic ownership in this table may not be duplicated.

---

# 26. Implementation program

This program is dependency order, not a wishlist.

Each train is required.

No train may be declared complete while its required consumer integration is missing.

## Train A — correct the substrate

Deliver:

- fix SCIP position encoding;
- harden parser entry points;
- eliminate string/comment false-reference paths;
- builtin filtering and fail-closed ambiguous re-exports;
- stale-evidence span/hash checks;
- deterministic index-derived clock;
- canonical path/span behavior;
- source discovery accounting;
- typed freshness;
- typed result/partial failure;
- remove phantom seam requirement.

Exit:

- no known evidence correctness bug;
- every considered input has disposition;
- `unknown != fresh`;
- ambiguity is explicit;
- every result is generation-bound.

## Train B — converge providers and resolution

Deliver:

- one provider system under the canonical ownership;
- package/export integration;
- capability matrix;
- adapt lexical/Tree-sitter/SCIP/domain providers;
- one exact-first resolution pipeline;
- import maps;
- bounded SCC/re-export closure;
- provider/language capability truth;
- full SCIP field consumption;
- source-span/citation strength/re-anchor.

Exit:

- no duplicate provider registry;
- no provider-specific final-resolution folklore;
- same-tier ambiguity abstains;
- exact/provider evidence wins;
- doctor reports exact/ambiguous/unsupported coverage.

## Train C — RecallCircuit + minimum truth together

Deliver:

- indexed seed resolver;
- one traversal primitive;
- traversal policies;
- RecallCircuit;
- complete path semantics;
- generation-bound circuit/cursor identity;
- deterministic ranking;
- bounded result assembly;
- explicit fact-to-claim truth binding;
- citation validation;
- grounding states;
- declared-vs-done finding attachment;
- `service.recall()`.

Exit:

- RecallCircuit beats legacy candidates on correctness/path completeness;
- claims/contradictions can appear only through explicit truth binding;
- the flagship doc↔code capability exists on the main recall path;
- no model performs graph traversal;
- legacy candidate logic is no longer independent.

## Train D — resident seam + admission

Deliver:

- daemon protocol `recall`;
- server/client support;
- persistent resident query path;
- Membrane daemon-first seam;
- typed daemon-unavailable failure with no subprocess fallback;
- candidate compatibility adapter;
- admission consumes RecallCircuit + truth;
- generation/freshness/claim boundaries in admission receipts.

Exit:

- normal machine-to-machine recall creates no per-query Node process;
- resident path beats subprocess path;
- admission uses no independent ranking/candidate policy;
- host-enforcement boundary is preserved.

## Train E — full Phase 2 understanding

Deliver:

- claim ingestion/evaluation;
- contradiction/staleness/unsupported/ambiguity findings;
- component synthesis;
- flow synthesis;
- architecture understanding;
- evidence-cited generated views;
- Phase 2 provider receipts;
- invalidation from Phase 1 evidence changes;
- bounded Phase 2 execution owner in the application/service layer;
- exact evidence-pack construction;
- provider/model allowlist and schema validation;
- typed timeout/unavailable/invalid-output/incomplete states.

Exit:

- generated architecture/component/flow views are populated on the frozen corpus;
- every synthesized statement has evidence;
- Phase 2 failure cannot corrupt Phase 1;
- declared-vs-done fidelity benchmark is green.

## Train F — change and history intelligence

Deliver:

- diff-range seeds;
- stack-trace/failure seeds;
- impact provenance;
- liveness;
- recommended test set;
- explainable change risk;
- co-change lower-authority history;
- named treeish/generation semantic diff;
- history evidence recall;
- derived evolution findings.

Exit:

- a proposed/current change can be evaluated against code, docs, tests and historical evidence;
- historical evidence cannot override current truth;
- test selection exposes uncovered impact;
- risk remains decomposable.

## Train G — incrementality, runtime and recovery

Deliver:

- stable definition identity;
- rename/move reconciliation;
- change classification;
- one invalidation DAG;
- closure repair;
- versioned correctness cache keys;
- watcher starvation/crash hardening;
- provider crash isolation;
- cancellation through provider loops;
- bounded worker pools;
- staged verify/adopt;
- last-known-good recovery;
- lock-vs-corruption discrimination;
- generated artifact ownership.

Exit:

- incremental = full across frozen fixtures;
- crash/cancel cannot publish a partial generation;
- no-op work is avoided deterministically;
- service remains bounded under repeated parallel calls.

## Train H — qualification and release proof

Deliver:

- frozen corpus/goldens;
- resolution/truth/recall/change metrics;
- query-plan assertions;
- resource benchmark matrix;
- versioned numeric SLO file;
- agent A/B harness;
- clean-host Mac/Windows validation;
- release provenance.

Exit:

- all Definition of Done clauses are green;
- product SLOs are met;
- agent outcome gate is met;
- no OUT architecture capability exists in the production path;
- no legacy adapter owns independent semantics.

---

# 27. Explicit removals / deprecations

After migration and qualification, remove or neutralize the following duplicate semantics:

- independent candidate ranking in `blueprint-candidates`;
- candidate planning inside `src/sources`;
- provider registration duplicated under graph and provider trees;
- provider-specific resolution finalization;
- primary `%LIKE%` symbol scans;
- mtime/path-only correctness caches;
- duplicate ranking policy in CLI/MCP;
- opaque truncation;
- duplicate evidence/result shapes;
- any missing-file seam prerequisite;
- any generated document treated as authoritative truth;
- `src/providers/ranking/semantic.mjs` and the semantic vector retrieval lane;
- `src/providers/ranking/hybrid.mjs` as an additive weighted ranking algorithm;
- local PageRank/centrality ranking in neighborhood/recall production paths, replaced by evidence tier, path completeness, exactness, coverage and bounded hop ordering.

Compatibility adapters may remain only when they call canonical owners.

---

# 28. Definition of Done

Blueprint is complete only when every item below is true.

## 28.1 Evidence and graph

- [ ] SQLite is the only production truth/evidence store.
- [ ] Generations are atomic and immutable after publication.
- [ ] Every durable fact has evidence, provider/version and generation.
- [ ] Independent provenance is preserved.
- [ ] Entities and occurrences are distinct.
- [ ] Ambiguity fails closed.
- [ ] Every considered source reaches a terminal ingestion disposition.
- [ ] Incremental and full builds are semantically equal.
- [ ] Failed/cancelled builds cannot replace the last-known-good generation.

## 28.2 Providers and resolution

- [ ] One provider capability system exists.
- [ ] Public SDK imports resolve from the published package.
- [ ] Lexical, Tree-sitter, SCIP and domain providers use the canonical provider contract.
- [ ] Every provider declares supported/unsupported capability state.
- [ ] One exact-first resolution pipeline owns finalization.
- [ ] Import/re-export cycles terminate deterministically.
- [ ] Same-tier ambiguity never selects a winner.
- [ ] SCIP position encoding and semantic fields are validated.
- [ ] Source spans/citations carry hashes and position semantics.
- [ ] Re-anchoring cannot silently move evidence.

## 28.3 Freshness and honesty

- [ ] Every response exposes generation and freshness.
- [ ] `unknown` is never presented as `fresh`.
- [ ] Dirty edit→query behavior is honest before watcher rebuild.
- [ ] Partial provider failures preserve independent successes.
- [ ] Omissions are machine-readable and reason-coded.
- [ ] Exact vs lower-bound state is explicit.
- [ ] Doctor exposes blind spots and unsupported semantics.

## 28.4 Recall

- [ ] RecallCircuit is the canonical task-shaped query primitive.
- [ ] Bounds are applied during traversal.
- [ ] Normal seed lookup is indexed.
- [ ] Complete paths are the primary returned unit.
- [ ] Equivalent paths deduplicate deterministically.
- [ ] Cycles/dense hubs terminate under bounds.
- [ ] Circuit and cursors bind to generation.
- [ ] Ranking is deterministic and non-compensatory.
- [ ] No learned/vector ranking path exists.
- [ ] No model performs graph traversal.
- [ ] `blueprint-candidates` has no independent recall algorithm.

## 28.5 Truth / understanding

- [ ] Claims bind to facts only through explicit tested joins.
- [ ] Grounding distinguishes direct/indirect/unsupported/contradicted/ambiguous/stale.
- [ ] Text similarity alone cannot create contradiction.
- [ ] Declared-vs-done drift preserves declaration and current evidence.
- [ ] Component synthesis is evidence-backed.
- [ ] Flow synthesis is evidence-backed.
- [ ] Architecture understanding is evidence-backed.
- [ ] Generated understanding is derived, not truth.
- [ ] Phase 2 receipts bind judgment to provider/version/generation/input evidence.
- [ ] Phase 2 judgment execution is bounded, schema-validated, deadline/cancellation aware, and reachable through the canonical service/application owner.
- [ ] Recall can return relevant truth findings on the main consumer path.

## 28.6 Change / history

- [ ] Diffs, stack traces and file:line anchors resolve to narrow seeds.
- [ ] Impact exposes path/provenance.
- [ ] Liveness is LIVE/UNREACHED/UNKNOWN.
- [ ] Recommended tests expose reasons and uncovered impact.
- [ ] Change risk is decomposable.
- [ ] Co-change remains lower authority.
- [ ] Named treeish/generation semantic diff works.
- [ ] History evidence cannot overwrite current truth.
- [ ] Derived evolution cites historical evidence.

## 28.7 Admission

- [ ] `recall` consumes canonical recall and truth.
- [ ] Decisions are allow/continue/block/noop with reason codes.
- [ ] Receipts bind task/repo/generation/scope/evidence.
- [ ] Missing/ambiguous/unsafe evidence can block when the contract requires it.
- [ ] Blueprint does not enforce host behavior.

## 28.8 Runtime

- [ ] Daemon is the primary machine-to-machine path.
- [ ] `recall` exists in the service protocol.
- [ ] Daemon read requests use generation-pinned sessions.
- [ ] Deadlines/cancellation/queues are bounded.
- [ ] Watcher is not the query server.
- [ ] Normal Membrane recall does not spawn Node per query.
- [ ] Daemon unavailability fails typed; no subprocess fallback runs.
- [ ] Provider crashes/hangs are isolated and typed.
- [ ] Last-known-good survives failures.

## 28.9 Security

- [ ] Repository text is treated as untrusted data.
- [ ] Path confinement is canonical/symlink-aware.
- [ ] Binary/oversize inputs are classified before parsing.
- [ ] Trusted path has zero network egress.
- [ ] Provider/process arguments are bounded.
- [ ] Secret-bearing diagnostics are redacted.
- [ ] Config affecting truth participates in digested identity.
- [ ] Mutable SQLite is not forced onto unsafe shared/cloud-synced paths.

## 28.10 Measurement / product proof

- [ ] Frozen fixture and task corpora are checksum protected.
- [ ] Graph/provider goldens catch extraction drift.
- [ ] Exact resolution and ambiguity honesty are measured.
- [ ] Recall path correctness/completeness is measured.
- [ ] Truth/contradiction/citation fidelity is measured.
- [ ] Change/test-selection correctness is measured.
- [ ] Critical SQL paths have query-plan assertions.
- [ ] Resource metrics cover cold/warm and 10k/100k/500k scales.
- [ ] Numeric SLO targets exist in the canonical SLO artifact.
- [ ] Resident recall beats subprocess recall at equal semantics.
- [ ] Agent A/B does not regress accuracy/fidelity and improves agent cost.
- [ ] Protected exact/evidence cases cannot regress because an aggregate improved.

## 28.11 Architecture integrity

- [ ] Every concern has one canonical owner.
- [ ] No OUT capability exists in the production architecture.
- [ ] No semantic/vector retrieval provider remains in the production Blueprint tree.
- [ ] No additive hybrid weighted ranker remains in the production Blueprint tree.
- [ ] No PageRank/centrality ranking participates in production recall/neighborhood admission.
- [ ] Cross-repository federation returns independently scoped slices and never a merged node space.
- [ ] No duplicate store, provider system, recall algorithm, ranking policy or truth system exists.
- [ ] No hidden prerequisite points to a missing file.
- [ ] No IN capability exists only as an unused module/schema.
- [ ] All trains A–H are green.

---

# 29. Final architecture statement

```text
Blueprint observes repository state deterministically.

Blueprint records what it saw, what produced it, how fresh it is,
and what it could not prove.

Blueprint preserves identity conservatively.

Blueprint resolves exact-first and fails closed on ambiguity.

Blueprint stores one local generation-bound evidence graph in SQLite.

Blueprint traverses that graph itself.

Blueprint returns complete evidence paths through RecallCircuit.

Blueprint binds declarations to code evidence explicitly.

Blueprint reports where declared intent and current implementation disagree.

Blueprint synthesizes components and flows only as evidence-backed derived understanding.

Blueprint understands diffs, failures, impact, tests, liveness and history
because its consumer is an agent that changes code.

Blueprint uses a resident service for repeated agent queries.

Blueprint makes admission decisions from the same recall and truth primitives,
while the host owns enforcement.

Blueprint treats historical evidence as evidence, not current truth.

Blueprint exposes uncertainty, omissions and partial failure as data.

Blueprint does not own prompt assembly, memory, orchestration, model selection,
code rewriting or host enforcement.

Blueprint contains no vector retrieval, additive hybrid/learned ranker, PageRank/centrality production ranking, second graph backend,
generic LSP/compiler runtime, deep taint engine, rewrite DSL or merged
cross-repository graph.

Blueprint is complete only when it measurably improves agent work at equal or
better correctness — not merely when the graph is sophisticated.
```
