# Cortex — Canonical Source of Truth

**Status:** Canonical · final architecture · implementation authority  
**Repository:** `Orthic-Labs/Cortex`  
**Architecture review baseline:** `bd46965d6738657db6ed95afad1dc622ce1c5b95`  
**Tree:** `541046eebab5fca67e487ce87f88047626e414da`  
**Date:** 2026-08-19  
**Supersedes:** `CORTEX_SOURCE_OF_TRUTH_REVISED.md` and all earlier Cortex absorption / implementation planning documents

---

## 0. Authority

This document is the single architectural and implementation authority for Cortex.

It defines:

- what Cortex is;
- what Cortex is not;
- the required end-state architecture;
- the canonical ownership of each concern;
- the required runtime and consumer seams;
- the required implementation sequence;
- the capabilities that are part of Cortex;
- the capabilities that are excluded from Cortex;
- the qualification gates that define completion.

Earlier planning documents are provenance only. They may explain why a decision was considered, but they do not create requirements after this document is adopted.

The repository remains the authority for current implementation reality. When the repository and this document diverge, the divergence is a defect: either the code must be brought back to this architecture or this document must be deliberately replaced in the same architectural decision.

There are no hidden companion specifications. A missing external document is never a Cortex-local prerequisite.

### 0.1 Binary architecture rule

Every capability in this document is assigned a binary architectural state:

- **IN** — required in the completed Cortex system;
- **OUT** — excluded from Cortex.

An OUT capability does not become part of Cortex because an implementation agent finds it interesting, a competitor has it, or a library makes it easy. Adding an OUT capability requires a new canonical architectural decision that replaces the relevant decision here.

A capability marked IN is not complete because a stub, schema, flag, or unused module exists. It is complete only when its behavior, integration, failure semantics, measurement, and consumer path are qualified.

---

# 1. Product purpose

Cortex is a local repository truth and evidence engine for agents and developer tooling.

The core user interaction is:

> Ask one task-shaped question about a repository and receive the smallest complete, fresh, evidence-backed answer Cortex can prove — including relevant relationships, source evidence, uncertainty, and disagreement between declared intent and current code.

Cortex exists to make repository understanding:

- more accurate than ad hoc grep/search;
- cheaper than repeated model-driven exploration;
- faster across repeated agent turns;
- evidence-backed rather than impressionistic;
- explicit about what it cannot prove;
- sensitive to the current worktree, not only the last index;
- capable of distinguishing what documentation declares from what the code currently does.

Cortex is not primarily a graph database product. The graph is its canonical evidence substrate.

Cortex is not primarily a search product. Search is one seed mechanism.

Cortex is not primarily a documentation generator. Generated understanding is a derived view over evidence.

Cortex is not an agent harness. Agents consume Cortex.

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

If Cortex builds an excellent graph but does not improve that loop, Cortex is not complete.

---

# 2. Architecture decisions — IN / OUT

## 2.1 IN — required Cortex capabilities

The completed Cortex system includes all of the following:

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
12. Deterministic ranking and result bounding owned by Cortex.
13. Phase 2 claim verification, declared-vs-done truth, component synthesis and flow synthesis.
14. Explicit fact-to-claim truth binding.
15. Typed contradictions, stale declarations, unsupported declarations, ambiguous declarations and citation failures.
16. Impact, liveness, diff-scoped reasoning, failure-signal resolution and recommended test selection.
17. Named-generation/treeish semantic diff and a history evidence lane.
18. Lower-authority co-change evidence for change reasoning.
19. Explainable change risk composed from named evidence-backed factors.
20. Task-scoped admission/orientation decisions that consume recall and truth.
21. A resident Cortex service daemon as the primary machine-to-machine query path.
22. CLI, SDK, MCP and legacy-candidate adapters over the same application behavior.
23. A watcher/freshness subsystem owned by Cortex but separate from query serving.
24. Incremental builds proven semantically equivalent to full builds.
25. Crash-safe generation publication, last-known-good recovery and provider isolation.
26. Security boundaries for repository text, paths, external processes, configuration and data export.
27. Doctor/diagnostic surfaces that expose actionable blind spots rather than vanity counts.
28. Frozen correctness benchmarks, query-plan benchmarks, resource benchmarks and agent-outcome evaluation.
29. Product SLOs and release ratchets.
30. Generated architecture/understanding artifacts as disposable, evidence-cited views — never as truth stores.

## 2.2 OUT — excluded from Cortex

The following are not part of the canonical Cortex architecture:

1. A second production graph/store backend.
2. A generic graph database abstraction intended to support Neo4j, Kùzu, FalkorDB, RocksDB or other production stores.
3. Vector/embedding retrieval as a Cortex retrieval lane.
4. Learned rankers.
5. Spectral clustering, graph communities or PageRank-like global ML substitutes for evidence.
6. Model-driven graph traversal.
7. LLM extraction that can create observed code facts.
8. LLM-generated facts that can override deterministic evidence.
9. Live language-server or compiler process invocation as a required query/build dependency.
10. A generic LSP runtime inside Cortex.
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

If evidence later shows one of these capabilities is necessary, the correct action is an architectural revision, not a hidden sidecar implementation.

---

# 3. System boundary

## 3.1 Cortex owns

Cortex owns:

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

## 3.2 Cortex does not own

Cortex does not own:

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
    → task-shaped Cortex request

Cortex resident service
    → generation-bound RecallCircuit
    → truth findings
    → change/admission information when requested

caller / Membrane / host
    → chooses final context
    → chooses enforcement policy
    → renders/injects prompt/context
    → executes the agent/tool action
```

Cortex and Membrane remain separate systems.

They do not share a store.

They do not duplicate ranking policy.

They do not duplicate traversal policy.

They do not merge memory semantics.

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
19. Cortex traverses the graph; the model does not infer missing hops.
20. Complete evidence paths outrank disconnected high-scoring fragments.
21. Cortex may bound its own output but does not own final prompt budgeting.
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

Cortex is organized as interacting planes, not one linear pipeline.

```text
┌──────────────────────────────────────────────────────────────────────┐
│                         CONSUMER / HOST                              │
│          Membrane · agents · IDE · CI · CLI · other hosts           │
└───────────────────────────────┬──────────────────────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────────────┐
│                      APPLICATION / ADMISSION                         │
│ service API · orient · impact · documentTruth · architecture         │
│                     recall · status · resolve                        │
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

Cortex currently contains two overlapping provider concepts:

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

Capabilities include the semantic operations Cortex actually uses:

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

The completed Cortex system contains:

- deterministic lexical extraction;
- Tree-sitter structural extraction;
- SCIP ingestion and resolution;
- framework providers that emit deterministic repository evidence;
- schema/IaC providers that emit deterministic repository evidence;
- rules/document providers that emit declarations/claims with source evidence.

Cortex does not require live compiler or LSP processes.

SCIP is the exact-analysis upgrade path where an index exists.

## 7.4 Provider permissions

The trusted provider path defaults to:

- repository read;
- no network;
- no arbitrary process execution;
- bounded memory/work;
- explicit timeouts;
- typed crash/hang/cancel outcomes.

A provider that requires behavior outside that trust boundary is not part of canonical Cortex.

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

Cortex distinguishes:

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

Cortex uses one source-address grammar and one span algebra.

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

Cortex supports the exact module semantics required for the languages it claims at each declared capability level.

The provider capability matrix is the product truth.

Cortex does not pretend that one universal import/MRO heuristic works across languages.

Where a language/provider cannot prove a semantic relationship, Cortex reports the lower capability rather than guessing.

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

Cortex achieves honest edit→query behavior through:

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

Tree-sitter subtree changed-range optimization is OUT. Cortex uses deterministic file/entity-level incremental invalidation and closure repair rather than introducing a second correctness path whose value has not been established.

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

RecallCircuit is Cortex's canonical query execution primitive.

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

Unknown vocabulary does not justify a vector lane. Cortex uses its indexed lexical, structural, documentation, component/flow and graph evidence.

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

No learned ranker exists in Cortex.

## 12.7 Bounds

Cortex owns the size of the evidence structure it returns.

When a cap binds:

```text
complete high-authority path
> complete lower-ranked path
> partial path
> disconnected node
```

Omissions are reported by reason.

Cortex does not reserve answer tokens or decide the caller's final prompt layout.

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

Cortex preserves both sides:

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

If Cortex cannot support a synthesized relationship with evidence, it is omitted or marked unsupported.

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

---

# 14. Change intelligence

Cortex includes change reasoning because the consumer is an agent that edits code.

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

Cortex returns `recommendedTestSet`, never "minimal tests" without proof.

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

Cortex supports:

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

Admission is Cortex's task/change decision surface.

Cortex answers:

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

`orient()` consumes the same canonical RecallCircuit and truth findings used elsewhere.

It does not build an independent candidate/ranking system.

Task orientation can therefore include:

- relevant code path;
- relevant docs/ADR/rules;
- declared-vs-done drift;
- stale evidence;
- ambiguity;
- impact/test information when the request includes a proposed/current change.

## 16.4 Blocking semantics

Cortex may return `block` for evidence-backed conditions such as:

- missing/incomplete graph;
- generation mismatch;
- unresolved required anchor;
- explicit repository rule violation;
- configured hard architectural constraint violation;
- required evidence unavailable under the requested policy.

Informational contradictions do not automatically become policy.

The host owns whether an admission action is enforced in the editor, shell, CI or agent runtime.

Cortex returns the decision.

The host enforces it.

---

# 17. Runtime architecture

## 17.1 Resident service is primary

The primary machine-to-machine Cortex path is the resident service daemon.

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

The watcher and daemon may share lifecycle under `cortex service run`, but they remain separate responsibilities.

## 17.3 Required daemon methods

The service protocol includes at minimum:

```text
status
search
resolve
orient
expand
impact
architecture
documentTruth
recall
build
```

`recall` is a first-class protocol method.

## 17.4 Application service

`src/lib/application/service.mjs` is the canonical public application behavior.

CLI, MCP, SDK and daemon call it.

Business/query policy is not copied into transport adapters.

## 17.5 CLI and legacy candidate compatibility

CLI remains a supported human/machine adapter.

`scripts/cortex-recall.mjs` is a lean command-line adapter over the canonical recall behavior.

`scripts/cortex-candidates.mjs` remains only as a compatibility adapter and must flatten/translate RecallCircuit rather than maintain an independent candidate algorithm.

There is one recall implementation.

## 17.6 Membrane seam

Membrane's primary Cortex path is:

```text
Membrane
→ persistent Cortex daemon client
→ recall
→ RecallCircuit
```

The subprocess path is the compatibility fallback for:

- daemon unavailable;
- version skew;
- explicit standalone execution.

The end state does not spawn a new Node process for every normal Cortex query.

The seam remains generation-pinned and fail-closed on mismatch.

---

# 18. Result and error contracts

## 18.1 Canonical result

All bounded application responses converge on a result envelope equivalent to:

```text
CortexResult<T> {
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

Cortex diagnostics answer:

> What can Cortex currently prove about this repository, and exactly where are the blind spots?

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

Generated files are never imported back as authoritative evidence merely because Cortex generated them.

The canonical truth remains the evidence graph plus receipt-backed derived truth state.

---

# 21. Security and trust boundary

Cortex is local-first and treats repository content as untrusted input.

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

Cortex exhausts a bounded retry/recovery path before destructive rebuild.

Durable operation state records enough information to recover or report the interrupted operation.

Provider workers are isolated where a crash/hang can otherwise take down the main build/service process.

Cancellation reaches long provider loops and is awaited before cleanup.

---

# 23. Product measurement and SLOs

Measurement is part of the architecture because Cortex is an agent tool.

## 23.1 Frozen evaluation

Cortex maintains frozen fixture repositories and task corpora with:

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

Before release, Cortex maintains a versioned `evals/slo.json` (or one equivalently canonical SLO artifact) containing numeric acceptance targets for the frozen reference hardware and fixture scales.

A release cannot be declared complete without numeric targets.

The SLO file must cover at least:

- 10k-edge repository;
- 100k-edge repository;
- 500k-edge repository;
- 1-file edit;
- 10-file edit;
- 100-file edit;
- warm resident recall;
- daemon-unavailable CLI fallback.

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

Cortex maintains an A/B harness:

```text
same task corpus
same model
same starting repository
same tool policy except Cortex availability

A: no Cortex
B: Cortex
```

Measure:

- task accuracy;
- input tokens;
- Cortex calls;
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

No Cortex rule may claim a missing `docs/plans/orthic/SEAM-CONTRACT.md` is a required local dependency.

Cortex's side of every consumer seam is specified in-repo.

## 24.8 Generated architecture truth

A generated `docs/architecture.md` that reports no synthesized components/flows is an honest incomplete artifact, not proof that the architecture capability exists.

Completion requires the Phase 2 implementation and qualification in this document.

---

# 25. Canonical file ownership

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
| CLI | `scripts/cortex.mjs`, lean adapters |
| MCP | MCP adapter only |
| candidate compatibility | `scripts/cortex-candidates.mjs` translating RecallCircuit |
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

## Train D — full Phase 2 understanding

Deliver:

- claim ingestion/evaluation;
- contradiction/staleness/unsupported/ambiguity findings;
- component synthesis;
- flow synthesis;
- architecture understanding;
- evidence-cited generated views;
- Phase 2 provider receipts;
- invalidation from Phase 1 evidence changes.

Exit:

- generated architecture/component/flow views are populated on the frozen corpus;
- every synthesized statement has evidence;
- Phase 2 failure cannot corrupt Phase 1;
- declared-vs-done fidelity benchmark is green.

## Train E — resident seam + admission

Deliver:

- daemon protocol `recall`;
- server/client support;
- persistent resident query path;
- Membrane daemon-first seam;
- CLI subprocess fallback;
- candidate compatibility adapter;
- admission consumes RecallCircuit + truth;
- generation/freshness/claim boundaries in admission receipts.

Exit:

- normal machine-to-machine recall creates no per-query Node process;
- resident path beats subprocess path;
- admission uses no independent ranking/candidate policy;
- host-enforcement boundary is preserved.

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

- independent candidate ranking in `cortex-candidates`;
- candidate planning inside `src/sources`;
- provider registration duplicated under graph and provider trees;
- provider-specific resolution finalization;
- primary `%LIKE%` symbol scans;
- mtime/path-only correctness caches;
- duplicate ranking policy in CLI/MCP;
- opaque truncation;
- duplicate evidence/result shapes;
- any missing-file seam prerequisite;
- any generated document treated as authoritative truth.

Compatibility adapters may remain only when they call canonical owners.

---

# 28. Definition of Done

Cortex is complete only when every item below is true.

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
- [ ] `cortex-candidates` has no independent recall algorithm.

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

- [ ] `orient` consumes canonical recall and truth.
- [ ] Decisions are allow/continue/block/noop with reason codes.
- [ ] Receipts bind task/repo/generation/scope/evidence.
- [ ] Missing/ambiguous/unsafe evidence can block when the contract requires it.
- [ ] Cortex does not enforce host behavior.

## 28.8 Runtime

- [ ] Daemon is the primary machine-to-machine path.
- [ ] `recall` exists in the service protocol.
- [ ] Daemon read requests use generation-pinned sessions.
- [ ] Deadlines/cancellation/queues are bounded.
- [ ] Watcher is not the query server.
- [ ] Normal Membrane recall does not spawn Node per query.
- [ ] CLI fallback remains functional.
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
- [ ] No duplicate store, provider system, recall algorithm, ranking policy or truth system exists.
- [ ] No hidden prerequisite points to a missing file.
- [ ] No IN capability exists only as an unused module/schema.
- [ ] All trains A–H are green.

---

# 29. Final architecture statement

```text
Cortex observes repository state deterministically.

Cortex records what it saw, what produced it, how fresh it is,
and what it could not prove.

Cortex preserves identity conservatively.

Cortex resolves exact-first and fails closed on ambiguity.

Cortex stores one local generation-bound evidence graph in SQLite.

Cortex traverses that graph itself.

Cortex returns complete evidence paths through RecallCircuit.

Cortex binds declarations to code evidence explicitly.

Cortex reports where declared intent and current implementation disagree.

Cortex synthesizes components and flows only as evidence-backed derived understanding.

Cortex understands diffs, failures, impact, tests, liveness and history
because its consumer is an agent that changes code.

Cortex uses a resident service for repeated agent queries.

Cortex makes admission decisions from the same recall and truth primitives,
while the host owns enforcement.

Cortex treats historical evidence as evidence, not current truth.

Cortex exposes uncertainty, omissions and partial failure as data.

Cortex does not own prompt assembly, memory, orchestration, model selection,
code rewriting or host enforcement.

Cortex contains no vector retrieval, learned ranker, second graph backend,
generic LSP/compiler runtime, deep taint engine, rewrite DSL or merged
cross-repository graph.

Cortex is complete only when it measurably improves agent work at equal or
better correctness — not merely when the graph is sophisticated.
```
