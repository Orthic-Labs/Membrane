# Blueprint v2 — Atom and Decision Register

**Status:** Proposed canon reconciliation  
**Date:** 2026-09-04

This register separates four different concepts that were previously easy to conflate:

- **Committed ownership** — Blueprint should ultimately own the capability.
- **Delivered implementation** — the observable capability is currently proven.
- **Implementation gate** — when the capability should be scheduled.
- **Exploratory** — plausible value, but not yet committed to product behavior.

A committed atom is therefore **not automatically immediate work**.

---

# 1. Current canon repair

Current committed baseline: **68 atoms**.

Reported status in the reconciliation:

- **46 delivered**;
- **20 partial**;
- **2 missing**.

## 1.1 Partial atoms to close

`BPT-003, BPT-010, BPT-011, BPT-012, BPT-013, BPT-014, BPT-017, BPT-020, BPT-021, BPT-026, BPT-027, BPT-033, BPT-034, BPT-038, BPT-039, BPT-041, BPT-042, BPT-043, BPT-044, BPT-057`

These are repaired before declaring the new canon “implemented.”

### Reclassified 2026-09-05 — BPT-021, BPT-043

Both were previously counted as delivered. Live evidence from the Blueprint daemon (`method: "status"` over `\.\pipe\membrane-blueprint-<sha256(USERPROFILE)[:16]>`) shows otherwise:

```text
sourceClock 188 == appliedClock 188, eventGap false, pendingEvents 0
barrier.barrierResult "caught_up"          <- the barrier agrees
outer barrierResult   "timeout"            <- yet the gate refuses
domainsPending        ["doc"]
freshness             "changed_since_generation"
generation.indexed_revision 39b6ec6f  vs  current 5af9b0d0 (dirty)
```

- **BPT-021** (incremental ≡ full build) — convergence is never reached, so incremental is not semantically equivalent to a full build.
- **BPT-043** (watcher/reconciler) — the watcher runs and applies deltas but never advances a generation; `indexed_revision` stays pinned.

**BPT-019 remains delivered.** Honest freshness is behaving correctly by refusing to claim `current`; the defect is upstream convergence, not the freshness contract. Its refusal is the symptom that exposed BPT-021/BPT-043, not a failure of its own observable behavior.

Root cause for both: `blueprint/src/graph/delta-store.mjs:392` marks the `doc` domain pending inside the watcher on any `.md` change, while the only clear is `blueprint/scripts/blueprint.mjs:3729` inside `phase2 seal`, which takes a `one_shot` lease on the manual path. See INV-024.

## 1.2 Missing existing atoms

- `BPT-051`
- `BPT-052`

The exact existing atom definitions remain authoritative in the current Blueprint canon; this document does not silently redefine them.

---

# 2. New committed Blueprint atoms

Of the 29 originally proposed atoms `BPT-072` through `BPT-100`, **27 remain committed destination ownership**. `BPT-093/094` return to exploratory status after final reconciliation with Blueprint's explicit exact-first/no-semantic-correctness-dependency doctrine. `BPT-106` is promoted from exploratory to committed because the final subsystem deep-dive supplies the missing weak-evidence contract for descriptive convention mining.

| Atom | Capability | Canon status | Implementation gate | Primary design influence |
|---|---|---|---|---|
| BPT-072 | Semantic authority precedence | Commit | Gate A/0 | SCIP/Kythe + deterministic resolvers; LSP verifier |
| BPT-073 | Semantic indexer orchestration | Commit | Gate B/2 | SCIP, Infigraph |
| BPT-074 | On-demand LSP semantic verification/cross-check | Commit | Gate B/2 | Serena, Octocode |
| BPT-075 | Generalized lexical/scope resolution | Commit | Gate B/2 | stack-graphs concept |
| BPT-076 | Type hierarchy/MRO/override/implementation facts | Commit | Gate B/2 | compiler/SCIP + deterministic fallback; LSP cross-check |
| BPT-077 | Resolution-frontier reporting | Commit | Gate B/2 | CodeGraph-style frontier honesty |
| BPT-078 | Explicit dynamic-dispatch seams | Commit | Gate B/2 | CodeGraph + Blueprint provenance |
| BPT-079 | First-class test identity/classification | Commit | Gate C/3 | Sense/Infigraph structural test facts |
| BPT-080 | Static test-reachability evidence | Commit | Gate C/3 | TESTED_BY-style projection |
| BPT-081 | Entry-point registry | Commit | Gate C/3 | CodeGraph/GitNexus/Infigraph |
| BPT-082 | Process/Step projection | Commit | Gate C/3 | GitNexus concept, disposable projection |
| BPT-083 | Named federation groups | Commit | Gate C/3 | GitNexus/Infigraph concept |
| BPT-084 | Service/API contract registry | Commit | Gate C/3 | GitNexus/Infigraph |
| BPT-085 | Cross-repo consumer→provider evidence | Commit | Gate C/3 | explicit contract bridge |
| BPT-086 | Cross-repository trace stitching | Commit | Gate C/3 | bridge-only traversal |
| BPT-087 | Dependency-injection facts | Commit | Gate C/3 | framework provider |
| BPT-088 | ORM/query-target facts | Commit | Gate C/3 | framework provider |
| BPT-089 | Configuration-binding facts | Commit | Gate C/3 | framework provider |
| BPT-090 | RPC/MCP/tool definition→handler facts | Commit | Gate C/3 | route/tool provider model |
| BPT-091 | UI screen/navigation facts | Commit | Gate C/3 | router/framework providers |
| BPT-092 | BM25 lexical code index | Commit | Gate D/4 | Infigraph/Octocode/Sense class |
| BPT-095 | AST structural search | Commit | Gate D/4 | Octocode-style structural search |
| BPT-096 | Compact symbol/signature projection | Commit | Gate D/4 | Serena/Octocode |
| BPT-097 | Cold-start repository orientation | Commit | Gate A/1 | Sense summary idea |
| BPT-098 | Query-time dirty-file repair | Commit | Gate A/1 | Sense freshness behavior |
| BPT-099 | Branch-switch coalesced reconciliation | Commit | Gate A/1 | Sense/Git-aware reconciliation |
| BPT-100 | Liveness vs readiness contract | Commit | Gate A/1 | Potpie-style health separation |
| BPT-106 | Project convention detection as descriptive weak evidence | Commit | Gate C/3 | Sense |

Destination committed canon after final reconciliation: **96 committed atoms**.

Important: 96 is the ownership count, not the immediate implementation count. `BPT-093/094` remain tracked but exploratory; this avoids silently overturning an explicit Blueprint canon exclusion without a new decision.

### Loom synthesis reconciliation

The later Loom synthesis does **not** add new BPT product-capability atoms at this point. Its genuinely useful deltas are engineering/correctness obligations applied to existing atoms and providers:

- executable schema/indexer conformance verification;
- source-anchored golden assertions with positive/negative/ambiguity cases;
- completeness-safe publication / last-known-complete protection;
- incremental shrink guards;
- pinned/frozen evaluation methodology.

These are recorded as ADRs and implementation gates below rather than inflating the capability count.

### Code-only implementation audit reconciliation

The subsequent code-only review likewise adds **no new destination atoms**. It converts several broad atoms/ADRs into concrete current-code obligations:

- relationship producer/consumer parity, prompted by a current `TYPES` vs registry mismatch;
- automatic extractor fingerprints for parse-cache validity;
- one canonical SCIP normalizer instead of parallel drifting implementations;
- richer symbol/type/receiver metadata underneath BPT-076;
- modern JS/TS package/workspace resolution underneath BPT-075;
- canonical domain entity identity underneath contract/framework atoms;
- portable semantic identity for federation/interchange;
- reuse of Blueprint's existing vector table/cosine implementation rather than introducing a new vector database.

The 343-atom composite matrix is evidence inventory, not a replacement Blueprint architecture. Its Kùzu/LanceDB, editing, memory and mandatory deep-analysis recommendations are not adopted merely for donor parity.

### Final Membrane subsystem reconciliation

The final `membrane-blueprint.md` deep-dive resolves four remaining ambiguities:

- `BPT-106` conventions is promoted because its contract is now explicit: descriptive, coverage-backed, counterexample-preserving, `WeakEvidence`, never policy;
- `BPT-074` is narrowed to an on-demand LSP **cross-check** that emits agreement/conflict receipts rather than canonical edges;
- `BPT-093/094` return to exploratory status; semantic similarity is retrieval-only and never becomes a resolution tier;
- rules-as-data, deterministic emit-side semantic export and generation-history temporal reads are implementation obligations under existing resolver/export/history ownership, not new product atoms.


---

# 3. Exploratory atoms

These remain exploratory until a benchmark, product use case or architecture review promotes them.

| Atom | Capability | Why not committed yet |
|---|---|---|
| BPT-093 | Local semantic-code embeddings | Explicit canon change required; benchmark only as optional retrieval lane. |
| BPT-094 | Hybrid retrieval fusion | Same; candidate discovery only, never resolution/admission authority. |
| BPT-101 | Code-aware reranker | Must beat simpler retrieval on code-specific benchmark. |
| BPT-102 | Evidence-preserving output compression | Valuable, but must not become final context planning. |
| BPT-103 | Community/subsystem clustering | Useful orientation, heuristic derived projection. |
| BPT-104 | Hub/centrality analysis | Structural signal, not semantic truth. |
| BPT-105 | Cycle analysis | Useful and derivable; not foundational. |
| BPT-107 | Complexity metrics | Refactor/risk enrichment, not core evidence. |
| BPT-108 | Coupling metrics | Same. |
| BPT-109 | Clone/near-duplicate detection | Useful specialized analysis. |
| BPT-110 | OSV vulnerability projection | Valuable but adjacent to structural intelligence. |
| BPT-111 | API producer/consumer shape checking | Specialized structural rule after contract model matures. |
| BPT-112 | Route-aware API pre-change impact | Derivable after route/contract semantics mature. |
| BPT-113 | Git-derived ownership/hotspot analytics | Weak historical/derived evidence; read-time projection over current graph + history. |

---

# 4. Explicitly out of Blueprint ownership

Do not add these merely because the 251-capability donor matrix includes them:

- general persistent session/agent memory;
- general project learnings/decision graph;
- final LLM context compiler/planner ownership;
- semantic editing/refactoring operations;
- shell/build/test execution ownership;
- effect/policy authorization;
- generic always-on CPG/PDG/taint system;
- unrestricted SDLC knowledge graph;
- silent self-learning graph mutation.

Blueprint can expose evidence to adjacent systems that own these concerns.

---

# 5. Reconciled architectural decisions

## ADR-BP-001 — Keep SQLite; redefine its role

**Decision:** Keep SQLite/WAL as local canonical fact ledger and storage substrate. Do not replace it with a graph DB solely to match donors.

**Reason:** Blueprint's immutable-generation/local-first discipline is a stronger foundation than making a specialized traversal store the source of truth. Specialized graph/search representations may be projections later.

---

## ADR-BP-002 — Canonical facts vs rebuildable projections

**Decision:** Entity/Occurrence/Relation/Evidence/Generation identities and source-backed facts are canonical; architecture/process/search/vector/community representations are projections.

**Reason:** Analyzer and index evolution must not mutate the semantic source of truth.

---

## ADR-BP-003 — Freshness-aware semantic authority

**Decision:** Semantic authority is not a static producer ranking. Source-state coherence/freshness is evaluated before authority.

Order:

```text
admissibility
→ source-state coherence/freshness
→ semantic authority
→ resolution specificity
→ confidence if inferred
```

**Reason:** stale evidence cannot become current merely because its producer is stronger. Dirty source is repaired/reparsed first. Optional LSP may verify or contradict the result but does not silently replace canonical Blueprint truth.

---

## ADR-BP-004 — Confidence only for inference

**Decision:** Authoritative/deterministic facts do not carry synthetic `1.00` confidence.

**Reason:** Provenance category and probabilistic uncertainty are different concepts. Conflating them damages epistemic clarity.

---

## ADR-BP-005 — SCIP becomes an active semantic tier

**Decision:** Blueprint owns semantic producer orchestration—discovery, invocation, validation, versioning and ingestion—not only SCIP parsing.

**Reason:** Passive ingestion leaves precision dependent on an external manual step and fails the “setup-and-forget” product goal.

---

## ADR-BP-006 — LSP is an on-demand cross-check, not a canonical producer

**Decision:** Blueprint may query an available LSP only to verify bounded resolved/current-source questions. Agreement emits a receipt; disagreement emits a typed `resolution_conflict`. The LSP answer is not silently promoted into canonical graph truth.

**Reason:** this preserves Blueprint's exact-first/local-truth doctrine while gaining Serena-style semantic verification without a resident fleet or parallel truth universe.

---

## ADR-BP-007 — Resolution returns frontiers instead of guessing

**Decision:** If binding cannot be proven, return `UNRESOLVED` plus reason/evidence/candidates.

**Reason:** Blueprint's value is trustworthy evidence, not maximum apparent graph connectivity.

---

## ADR-BP-008 — Keep six explicit MCP tools

**Decision:** Retain `recall/search/expand/impact/doc_truth/status` as default visible semantics.

**Reason:** Six tools is already a small menu, and each verb carries useful Blueprint-specific meaning. Generic `explore/analyze/change` may exist only as composed convenience later.

---

## ADR-BP-009 — Recall ranking remains non-compensatory

**Decision:** BM25/dense/RRF discover candidates only. Blueprint evidence/admissibility determines trusted output.

**Reason:** Semantic similarity cannot compensate for weak truth/provenance.

---

## ADR-BP-010 — Semantic vectors remain exploratory and retrieval-only

**Decision:** `BPT-093/094` are exploratory. They may be benchmarked as optional local candidate-generation lanes, but semantic similarity never becomes a resolution tier and cannot affect canonical truth/admission. Promotion requires an explicit canon decision plus the agent-outcome gate.

**Reason:** current Blueprint canon explicitly excludes semantic/hybrid vector search from correctness. The useful donor idea is optional conceptual recall, not a new truth mechanism.

---

## ADR-BP-011 — Process/Step/Architecture are derived

**Decision:** Materialize reusable process/step/architecture structures for speed, but mark them as rebuildable evidence-backed projections.

**Reason:** A structural execution path is not equivalent to a runtime trace or canonical source fact.

---

## ADR-BP-012 — Multi-repo crosses explicit contracts

**Decision:** Federation groups overlay independent repo graphs. Cross-repo paths cross exact contract/bridge evidence only.

**Reason:** Avoid identity pollution and false relationships from global same-name matching.

---

## ADR-BP-013 — Split framework semantics into provider contracts

**Decision:** DI, ORM, config, RPC/tool, UI navigation and related semantics get independent provider contracts.

**Reason:** A broad generic framework atom cannot express different evidence, invalidation and degradation rules cleanly.

---

## ADR-BP-014 — Operational reliability precedes intelligence breadth

**Decision:** Destination canon does not dictate implementation order. The first release gate is watcher/init/MCP/freshness reliability.

**Reason:** Blueprint is not useful if the graph is semantically rich but stale, unwatched or undiscoverable.

---

## ADR-BP-015 — One canonical init path

**Decision:** `blueprint init` replaces/deprecates divergent installer behavior.

**Reason:** Configuration drift is a product reliability bug.

---

## ADR-BP-016 — Query-time repair is bounded

**Decision:** Repair relevant dirty source synchronously only within strict file/time budgets; never perform whole-repo reconcile on query.

**Reason:** Preserve both freshness and predictable agent latency.

---

## ADR-BP-017 — Git transitions are first-class source changes

**Decision:** checkout/merge/rewrite use Git diff batching where possible, with filesystem watcher fallback.

**Reason:** Prevent event storms and make work proportional to actual source delta.

---

## ADR-BP-018 — MCP resources must be live

**Decision:** advertised resources are repository-scoped graph-backed projections or explicitly unavailable/degraded; never static placeholders.

**Reason:** Placeholder resources create false capability and undermine agent trust.

---

## ADR-BP-019 — Correctness must be mechanically verifiable

**Decision:** supported semantic/provider behavior requires source-anchored executable conformance fixtures with positive, negative and ambiguity assertions.

**Reason:** parser/provider code compiling successfully is not proof that graph semantics remain correct across languages, schema changes and resolver evolution.

**Implementation note:** borrow Kythe/SCIP's verifier/golden-test discipline, but do not mandate an embedded Datalog engine or a new language unless simpler fixtures prove insufficient.

---

## ADR-BP-020 — Schema vocabulary is explicit and versioned

**Decision:** entity/relation kinds and provider payload contracts are registered/versioned; breaking reinterpretation requires explicit migration.

**Reason:** a continuously evolving graph becomes untrustworthy if old generations can be silently re-read under new semantic meanings.

---

## ADR-BP-021 — Publication protects last-known-complete truth

**Decision:** incomplete/truncated/failed extraction may remain staged or degraded but cannot replace a known-complete published generation.

**Reason:** freshness is not more important than completeness. “Newer but partial” must not masquerade as current truth.

---

## ADR-BP-022 — Incremental updates require shrink guards

**Decision:** a changed-file/provider update may replace facts it owns, but must not silently remove unrelated unchanged-file facts.

**Reason:** incremental corruption can otherwise look like legitimate graph shrinkage and escape freshness checks.

---

## ADR-BP-023 — No-index live graph is not a second mandatory truth plane

**Decision:** do not adopt Loom/Octocode's always-on lazy live graph as a mandatory parallel runtime at this stage. Blueprint's bounded dirty-file repair is the preferred zero-staleness mechanism.

**Reason:** maintaining two continuously queryable structural truth planes increases identity, freshness and reconciliation complexity. Revisit only if measurements show cold/unindexed repositories need a live fallback that `init` + micro-repair cannot satisfy.

---

## ADR-BP-024 — Agent integration nudges; it does not block raw source reads

**Decision:** do not install a strict pre-tool guard whose correctness depends on preventing an agent from reading source before Blueprint.

**Reason:** Blueprint is evidence infrastructure, not an effect/policy enforcement layer. Routing guidance may strongly prefer Blueprint first, but source access remains available.

---

## ADR-BP-025 — Benchmark methodology is pinned before sophistication is promoted

**Decision:** retrieval/provider sophistication is evaluated on pinned corpora/tasks/configuration and negative results are retained.

**Reason:** otherwise embeddings, rerankers or richer resolvers can be promoted from anecdotal wins while regressing exact code tasks or resident cost.

## ADR-BP-026 — Preserve Membrane-native lifecycle/truth contracts

**Decision:** donor-derived architecture changes must preserve existing Membrane contracts for typed ingestion disposition, detailed freshness relation, typed admission decisions with reason codes, generation-bound cursors, process-incarnation-aware writer leasing, and declared-vs-observed truth binding.

**Reason:** these are correctness properties, not incidental implementation details. Replacing them with simpler booleans, generic stale flags or PID-only leases would be a regression even if the surrounding graph becomes more sophisticated.

**Atom effect:** no new BPT atom. These strengthen existing freshness, ingestion, evidence, query and store-lifecycle atoms.

## ADR-BP-027 — Feature promotion is gated by agent outcomes, not component metrics alone

**Decision:** optional sophistication that adds persistent cost or agent-facing complexity must demonstrate improved end-to-end agent task outcomes against the simpler baseline before becoming default.

**Reason:** retrieval Hit@K, graph density or analyzer recall can improve while agent tool selection, latency, token use or edit correctness gets worse. Blueprint optimizes the agent workflow, not isolated subsystem scores.

**Applies especially to:** embeddings/hybrid fusion, rerankers, additional MCP tools, always-on semantic providers, communities/centrality and deep analysis.

**Atom effect:** no new BPT atom; this is a promotion gate for committed-but-deferred and exploratory capabilities.

---

## ADR-BP-032 — Resolution rules may be declarative; invariants may not

**Decision:** language-specific resolution behavior should become versioned/diffable rule data where this improves inspectability and testability. Freshness, authority, tier dominance, admission and ambiguity semantics remain hard architectural invariants and cannot be overridden by project rule files.

**Reason:** stack-graphs-style data-driven rules improve reviewability without turning truth policy into user configuration.

---

## ADR-BP-033 — Conventions are descriptive weak evidence

**Decision:** promote `BPT-106`. Convention mining may emit naming/error-handling/layout/test-placement patterns only with support/coverage, counterexamples and generation evidence. It never becomes policy by itself.

**Reason:** the final subsystem review supplies the missing evidence contract that previously kept this exploratory.

---

## ADR-BP-034 — Historical reads use generations, not a second temporal store

**Decision:** add bounded `fact_at`/`changed_between`-class queries over retained generations. Do not adopt a duplicate bitemporal database solely for this use case.

**Reason:** Blueprint already owns generation history; temporal forensics can be derived from that source while preserving current-code truth as primary.

---

## ADR-BP-035 — Emit standard semantic interchange where possible

**Decision:** extend export so representable semantic facts can be emitted as actual SCIP, while Blueprint-specific evidence uses a versioned Blueprint-native format. Do not invent an incompatible “SCIP-like” pseudo-standard.

**Reason:** interoperability is valuable, but lossless semantics and standard compliance matter more than superficial format resemblance.

---

# 6. “Who was right?” reconciliation

There were two valid scopes:

### Earlier runtime implementation plan

Correct about:

- not replacing watcher;
- not replacing SQLite merely because donors use graph DBs;
- canonical init;
- host/MCP integration;
- watcher liveness verification;
- Git transition batching;
- query-time micro-repair;
- live MCP resources;
- real resident SLOs;
- cold-start context;
- keeping the small MCP philosophy.

Its limitation was scope: it assumed more of the current semantic architecture/canon rather than re-deriving it from the donor matrix.

### Later donor/canon analysis

Correct about:

- reconciling existing BPT atoms before adding features;
- adding semantic producer orchestration;
- adding a precision/resolution ladder;
- making tests/entry points/process/contracts first-class capabilities;
- separating generic framework semantics;
- defining canonical fact ledger vs derived projections;
- keeping Recall evidence admission above hybrid retrieval;
- preserving Blueprint's ownership boundary.

Its required refinements are captured by this register:

1. semantic authority is freshness-aware;
2. confidence applies only to inference;
3. the six MCP tools remain explicit;
4. process/contracts distinguish source-backed facts from derived link/projection semantics;
5. the final destination canon is 96 committed atoms; implementation remains staged rather than one batch;
6. embeddings/hybrid are exploratory and require both benchmark success and an explicit canon decision before promotion;
7. conventions are committed only as descriptive weak evidence;
8. LSP is verification/cross-check, not a canonical graph producer.

The resulting v2 design is therefore a synthesis, not a vote for one document over the other.

---

# 7. Donor-use policy

Detailed donor→atom/task mappings live in `04_BLUEPRINT_DONOR_REFERENCE_V2.md`. This register keeps only governing use constraints. Two repository names must remain disambiguated: **CodeGraph** means `colbymchenry/codegraph`; **codebase-graph** means `Phoenixrr2113/codebase-graph` (which may appear in package naming as `@agntk/codegraph-mcp`).

A supplied comparison artifact labeled Graphify MIT. The current `Graphify-Labs/graphify` repository `LICENSE` was directly verified on 2026-09-04 as **Apache-2.0**; Apache-2.0 is the canonical Blueprint donor record.

| Donor | Permitted design role | Important caution |
|---|---|---|
| Infigraph | semantic/indexer orchestration, broad extractors, optional retrieval | avoid enormous default tool surface |
| Potpie | readiness semantics, capability/provider separation | durable memory/context beyond Blueprint boundary |
| GitNexus | process/contract prior art, resources/skills concepts | PolyForm Noncommercial — no commercial code absorption without compatible license |
| CodeGraph | agent UX, frontier reporting, packaging/setup | preserve Blueprint's stronger evidence model |
| Sense | watcher freshness, branch handling, query-time repair, summaries | deliberately narrow system; use as runtime donor |
| codebase-graph | temporal/change analytics prior art | graph backend not required for Blueprint |
| Graphify | extracted/inferred provenance, artifact/export ideas | **Apache-2.0 verified 2026-09-04**; hooks secondary to live watcher |
| Glean | typed immutable fact/derived-predicate principles | do not embed heavy server architecture by default |
| SCIP | semantic interchange/identity/occurrence design | transport, not storage/query engine |
| Serena | bounded LSP verification/cross-check ergonomics | no resident truth plane; editing/refactoring stays outside Blueprint |
| Octocode | structural search, live AST/semantic-search separation | LLM relations remain segregated |
| Kythe | stable semantic identity/source anchors/verifier discipline | interoperability substrate, not agent UX |
| stack-graphs | deterministic build-independent name-resolution concepts | archived/unmaintained; own implementation/fork/tests |
| Joern | optional deep-analysis conceptual provider | do not make CPG/PDG/taint resident foundation |
| Semantica | provenance/conflict ontology concepts | general memory/governance outside Blueprint |

---

# 8. Promotion criteria for exploratory atoms

An exploratory atom is promoted only if all are true:

1. capability clearly belongs in Blueprint's ownership boundary;
2. concrete user/agent workflow benefits;
3. deterministic/evidence contract is specified;
4. resource cost is measured;
5. simpler existing capability cannot satisfy the need;
6. provider/projection failure can degrade independently;
7. license-safe implementation path exists;
8. acceptance test can prove observable behavior.

This is the anti-bloat gate for future donor research.


## ADR-BP-028 — Relationship registry parity is a release gate

**Decision:** every first-party emitted relationship must be registered, and every public relationship must be handled by each relevant consumer or covered by an explicit tested exemption.

**Reason:** the code-only audit found an actual `TYPES` producer vs canonical-vocabulary mismatch. Silent edge drift is silent partial truth.

---

## ADR-BP-029 — Semantic caches use extractor fingerprints

**Decision:** cached parse/provider/semantic output is keyed by source identity plus an automatically derived extractor fingerprint, not by source bytes plus a manually remembered global version alone.

**Reason:** unchanged source can still require re-extraction after grammar/provider/extractor semantic fixes.

---

## ADR-BP-030 — SCIP has one normalization contract

**Decision:** all first-party SCIP paths share one role/symbol/occurrence/relationship normalizer. Language-specific providers enrich normalized output rather than parse SCIP semantics independently.

**Reason:** two semantic implementations for one interchange contract will drift and can assign different truth to the same index.

---

## ADR-BP-031 — Portable semantic identity is additive

**Decision:** keep Blueprint's internal generation/local IDs while allowing exact semantic producers to attach a portable SCIP/Kythe-class identity.

**Reason:** federation/interchange need stronger identity than display/path strings, but local storage identity must not be forced into a cross-repo global namespace.
