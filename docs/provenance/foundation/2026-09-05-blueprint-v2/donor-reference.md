# Blueprint v2 — Donor Reference

**Status:** Non-normative implementation reference  
**Date:** 2026-09-04  
**Applies to:** `Orthic-Labs/Membrane/blueprint`

This document answers a narrow implementation question:

> When a Blueprint atom or subsystem is being implemented, which donor project should an engineer inspect first, what should be learned from it, and what must not be copied or imported into Blueprint's ownership model?

It is intentionally **non-normative**. If this reference conflicts with `01_BLUEPRINT_ARCHITECTURE_CANON_V2.md` or `03_BLUEPRINT_ATOM_AND_DECISION_REGISTER_V2.md`, the canon/register win.

---

# 1. Why this document exists

The reviewed repositories are not interchangeable competitors. They occupy different layers:

- some are agent-facing code-intelligence products;
- some are semantic/indexing foundations;
- some are specialized deep-analysis engines;
- some are project-context/knowledge systems;
- some are artifact-oriented graph generators.

Treating all of them as equivalent “graph tools” encourages bad architectural borrowing. Blueprint should study each donor only for the layer in which it is strongest.

---

# 2. Repository-name disambiguation

There are two unrelated projects that are easy to confuse:

| Short name used here | Repository | Notes |
|---|---|---|
| **CodeGraph** | `colbymchenry/codegraph` | Rust/local/agent-facing structural code intelligence. |
| **codebase-graph** | `Phoenixrr2113/codebase-graph` | Node/TypeScript + FalkorDB-family storage; published in the ecosystem as `@agntk/codegraph-mcp`. |

Never use the unqualified label `CodeGraph` in implementation notes when the Phoenix repository is intended.

---

# 3. Resolved source discrepancy — Graphify license

The supplied comparative HTML listed Graphify as MIT. The current repository `Graphify-Labs/graphify` `LICENSE` was checked directly on 2026-09-04 and is **Apache License 2.0**.

Canonical Blueprint donor records therefore use:

```text
Graphify-Labs/graphify — Apache-2.0
```

Do not propagate the MIT entry from the comparison artifact.

This is a useful reminder that comparison documents are design inputs, not license authority. Before any direct code absorption, inspect the current repository license and relevant dependency/file notices.

---

# 4. Donor taxonomy

## 4.1 Agent-facing structural/code-intelligence systems

These are the best references for how coding agents discover, query and consume repository intelligence:

- Infigraph
- CodeGraph (`colbymchenry/codegraph`)
- Sense
- GitNexus
- Octocode
- Serena
- Potpie
- codebase-graph (`Phoenixrr2113/codebase-graph`)

They are not equally useful for semantic truth. Their strongest value is usually one or more of:

- setup/discovery;
- MCP/CLI ergonomics;
- watcher/freshness behavior;
- graph navigation;
- framework enrichment;
- retrieval;
- process/impact UX.

## 4.2 Semantic/indexing foundations

These should be studied when implementing identity, resolution, compiler-grade facts or fact-store semantics:

- SCIP
- stack-graphs
- Kythe
- Glean

These projects are more important to Blueprint's semantic substrate than their lack of polished agent UX might suggest.

## 4.3 Specialized deep-analysis/context systems

- Joern — CPG/CFG/PDG/dataflow/taint prior art.
- Semantica — provenance, ontology, conflict/reasoning/governance prior art.
- Potpie also overlaps the context plane.

Blueprint should not absorb these entire ownership models. Their value is primarily conceptual/provider-level.

## 4.4 Artifact-oriented donor

- Graphify — deterministic extracted/inferred provenance, portable graph artifacts, reports/visual output and doc/media enrichment patterns.

---

# 5. Semantic acquisition models

The donor set confirms that “code intelligence” is not one extraction technique. Blueprint deliberately combines several precision tiers.

| Acquisition model | Strength | Weakness | Blueprint use |
|---|---|---|---|
| Tree-sitter AST | Broad, fast, build-free, deterministic | semantic binding varies by language/framework | mandatory baseline |
| Compiler/SCIP/Kythe-class semantics | strongest definition/reference/type identity where producer exists | may require build/indexer environment; can lag dirty workspace | preferred authoritative persisted semantic tier |
| Live LSP/IDE semantics | current-workspace definitions/references/types/diagnostics | process cost, lifecycle fragility, language-server dependency | on-demand current/dirty precision |
| stack/scope resolution | incremental build-independent binding | language rules must be authored/maintained | deterministic fallback between compiler and generic AST resolution |
| Framework resolvers | captures routes, DI, ORM, config, UI/RPC conventions | framework-specific and evidence-sensitive | explicit provider contracts |
| Explicit heuristic bridges | useful at dynamic/cross-language seams | inferred, incomplete | last-resort bounded relationships with evidence/confidence |
| CPG/PDG/dataflow | deep program-analysis precision | heavy and unnecessary for most coding-agent queries | optional/deferred provider only |

The key architectural rule remains:

```text
source-state coherence / freshness
        ↓
semantic authority
        ↓
resolution specificity
        ↓
confidence only if inferred
```

No donor's internal confidence score or graph rank overrides Blueprint's admissibility/evidence contract.

---

# 6. Later synthesis delta — Loom

The supplied **Loom — a Best-of-the-Survey Code Intelligence System** is a synthesis of the same donor set, not an additional code donor. It adds value where it turns scattered donor practices into explicit engineering disciplines.

## 6.1 Accepted into Blueprint v2

| Loom idea | Blueprint decision | Source lineage to study |
|---|---|---|
| mechanized graph/index correctness | adopt: source-anchored conformance fixtures | Kythe verifier + SCIP golden/caret test discipline |
| schema as a versioned product | adopt: explicit entity/relation/provider schema versions and migrations | Glean + Kythe |
| partial-run guard | adopt: failed/truncated extraction cannot replace last known complete generation | Graphify-style guard + Blueprint generation/convergence model |
| incremental shrink guard | adopt: changed-file repair cannot delete unrelated facts | Graphify incremental safety concept |
| frozen benchmark methodology | adopt: pinned corpora/tasks/config, preserve negative results | Sense + Octocode benchmark discipline |
| fail-closed optional capabilities | reinforce existing readiness doctrine | Potpie |
| rules as data for scope resolution | already represented by `BPT-075`; implementation remains Blueprint-owned | stack-graphs concept |

## 6.2 Useful, but not adopted as mandatory architecture

### Always-on lazy live graph + persisted graph

Octocode/Loom's two-tier model is useful prior art. Blueprint does **not** add a second mandatory live truth plane now.

Current preference:

```text
persisted canonical generation
        +
native watcher
        +
bounded query-time dirty-file repair
```

Revisit a no-index live fallback only if cold/uninitialized-repo measurements demonstrate a real product gap.

### Datalog-backed verifier

Blueprint adopts **verification semantics**, not the implementation dependency. Start with the simplest assertion engine that can prove schema/provider behavior. Introduce Datalog only if recursive semantic rules make it materially better.

### Typed query language / raw Cypher

Useful expert tooling, but not required for agent product correctness. Do not prioritize ahead of the six semantic MCP operations or the current CLI.

## 6.3 Explicitly rejected from Loom for Blueprint

- confidence labels on authoritative facts — conflicts with `ADR-BP-004`;
- durable work memory / lessons / decision graph — outside Blueprint ownership;
- semantic editing/write door — effect ownership belongs elsewhere;
- always-on general PDG/taint — outside the mandatory Blueprint plane;
- bundled vectors as core/default dependency — conflicts with benchmark-gated `BPT-093/094`;
- strict hooks that block raw source reads — Blueprint may nudge/reroute, not enforce source-access policy;
- a new universal Loom-style graph schema/storage stack — Blueprint retains its canonical-fact/projection contract and existing SQLite foundation.

---

# 7. Donor-by-donor implementation reference

## 7.1 Infigraph

**Repository:** `intuit/infigraph`  
**License:** Apache-2.0  
**Role:** broad local-first code-intelligence engine.

**Study it for**

- semantic/indexer orchestration;
- SCIP/compiler integration;
- modular language/framework extraction passes;
- BM25 + optional semantic retrieval architecture;
- cross-service/route analysis ideas;
- plugin boundaries;
- optional context compression;
- broad analysis capability behind one product.

**Relevant Blueprint atoms**

`BPT-073, BPT-081, BPT-083, BPT-084, BPT-087, BPT-089, BPT-092, BPT-093, BPT-094`

**Do not copy architecturally**

- an 80+-tool default MCP surface;
- always-on optional analyses in Blueprint's mandatory hot path;
- storage/query-engine choices merely for parity;
- generic taint/deep-analysis ownership.

**Absorption status:** permissive donor, subject to file/dependency notice audit.

---

## 7.2 Potpie

**Repository:** `potpie-ai/potpie`  
**License:** Apache-2.0  
**Role:** living project/context graph and daemon-oriented agent system.

**Study it for**

- liveness vs readiness separation;
- capability/provider negotiation;
- setup/readiness UX;
- daemon lifecycle ideas;
- evidence envelopes/provenance patterns;
- controlled context-write concepts where relevant to adjacent Membrane systems.

**Relevant Blueprint atoms**

`BPT-100` primarily; provider/readiness patterns also inform semantic provider design.

**Do not absorb into Blueprint ownership**

- general durable memory;
- project decision history;
- SDLC context as code truth;
- agent-controlled persistent knowledge mutation.

**Absorption status:** permissive donor, but most context-plane features belong outside Blueprint.

---

## 7.3 GitNexus

**Repository:** `abhigyanpatwari/GitNexus`  
**License:** PolyForm Noncommercial 1.0.0  
**Role:** agent-native graph intelligence, process/contract prior art, rich resources/skills.

**Study it for**

- Process/Step modeling;
- entry-point and flow ideas;
- contract/service bridging;
- framework passes such as DI/ORM/tool handling;
- MCP resources and agent guidance;
- impact/affected-change workflows;
- precomputed higher-order projections.

**Relevant Blueprint atoms**

`BPT-076, BPT-079, BPT-080, BPT-081, BPT-082, BPT-083, BPT-084, BPT-085, BPT-086, BPT-087, BPT-088, BPT-090`

**Do not copy**

- source implementation into a commercial Blueprint build without a separately compatible license;
- large agent tool menus merely because functionality exists;
- merged/global node-space semantics that violate Blueprint federation rules.

**Absorption status:** **design prior art only** unless separately licensed. Reimplement behavior from specification/tests, not source-copying.

---

## 7.4 CodeGraph

**Repository:** `colbymchenry/codegraph`  
**License:** MIT  
**Role:** agent-first local structural code graph.

**Study it for**

- high-level exploration ergonomics;
- exact source-centered context;
- “where resolution stops” / frontier honesty;
- dynamic-dispatch/cross-language seam presentation;
- entry-point/flow UX;
- auto-configuration and cross-platform packaging;
- small visible agent surface.

**Relevant Blueprint atoms**

`BPT-077, BPT-078, BPT-081, BPT-090, BPT-091, BPT-096` plus setup/agent-surface implementation work.

**Do not copy architecturally**

- weaker provenance semantics if they conflict with Blueprint's evidence model;
- graph representation choices as canonical truth merely because they work for CodeGraph.

**Absorption status:** permissive donor.

---

## 7.5 Sense

**Repository:** `luuuc/sense`  
**License:** MIT  
**Role:** deliberately narrow, token-efficient structural context server.

**Study it for**

- background watcher behavior;
- branch-switch coalescing;
- query-time dirty-file repair;
- generated cold-start summary;
- convention detection;
- benchmark discipline;
- minimal MCP surface;
- setup across Claude/Cursor/Codex/OpenCode-class hosts.

**Relevant Blueprint atoms**

`BPT-092, BPT-097, BPT-098, BPT-099, BPT-106`; exploratory `BPT-093, BPT-094, BPT-101`; operational init/watcher work.

**Important architectural lesson**

Sense's optional semantic retrieval does not make semantic/vector machinery a requirement for structural correctness. Blueprint should retain this separation.

**Absorption status:** permissive donor.

---

## 7.6 codebase-graph

**Repository:** `Phoenixrr2113/codebase-graph`  
**License:** MIT  
**Role:** graph/MCP system with temporal facts and optional semantic retrieval.

**Study it for**

- temporal/change analytics;
- embedded vs external backend split;
- grouped MCP semantics;
- retrieval profile/version compatibility ideas;
- graph dashboard patterns.

**Relevant Blueprint areas**

Mostly future/exploratory topology/change analytics rather than immediate BPT-072–100 core.

**Do not copy architecturally**

- FalkorDB dependency merely for graph parity;
- broader temporal project knowledge into Blueprint's canonical code-fact scope.

**Absorption status:** permissive donor.

---

## 7.7 Graphify

**Repository:** `Graphify-Labs/graphify`  
**License:** **Apache-2.0** (verified from current repository LICENSE on 2026-09-04)  
**Role:** deterministic structural graph + portable artifacts + explicit provenance.

**Study it for**

- extracted vs inferred relation semantics;
- evidence/provenance presentation;
- portable graph/report outputs;
- path/explain UX;
- lightweight post-change artifact generation;
- ADR/rationale linking concepts.

**Relevant Blueprint areas**

Provenance/inference doctrine, export/artifact UX, architecture explanations; exploratory topology work.

**Do not copy architecturally**

- Git hooks as the primary freshness mechanism;
- model-generated semantic relations into deterministic structural truth;
- the comparison artifact's incorrect MIT license label.

**Absorption status:** permissive donor, subject to file/dependency notice audit.

---

## 7.8 Glean

**Repository:** `facebookincubator/Glean`  
**License:** BSD-family project license  
**Role:** typed fact database/query architecture at scale.

**Study it for**

- typed fact schema discipline;
- immutable/source-derived vs derived-predicate separation;
- schema/versioning patterns;
- indexer contracts;
- fact ownership and deduplication concepts;
- large-scale query architecture prior art.

**Relevant Blueprint areas**

Canonical fact ledger, projection boundary, provider contracts, future query abstractions.

**Do not copy architecturally**

- heavy server-scale runtime into a local desktop hot path unless scale actually demands it.

**Absorption status:** permissive donor.

---

## 7.9 SCIP

**Repository:** `scip-code/scip`  
**License:** Apache-2.0  
**Role:** compiler-grade semantic index interchange protocol.

**Study it for**

- symbol identity;
- occurrence-vs-symbol separation;
- definition/reference/type relation transport;
- producer/version identity;
- streaming/file-incremental indexer design;
- cross-repo semantic identity principles.

**Relevant Blueprint atoms**

`BPT-072, BPT-073, BPT-076` and existing `BPT-006`, `BPT-013`, `BPT-014` repair.

**Do not mistake it for**

- a query engine;
- a persistent graph database;
- an editing/refactor subsystem.

**Absorption status:** permissive foundation.

---

## 7.10 Serena

**Repository:** `oraios/serena`  
**License:** MIT  
**Role:** LSP/IDE-backed semantic navigation and actions for agents.

**Study it for**

- LSP adapter ergonomics;
- on-demand semantic definitions/references/types;
- diagnostics/current-workspace semantics;
- provider capability negotiation;
- compact symbol/signature retrieval.

**Relevant Blueprint atoms**

`BPT-074, BPT-076, BPT-096`.

**Do not absorb into Blueprint**

- semantic editing/refactoring ownership;
- persistent agent/project memory;
- requirement for an always-running language-server fleet.

**Absorption status:** permissive donor for read/semantic-provider behavior.

---

## 7.11 Octocode

**Repository:** `Muvon/octocode`  
**License:** Apache-2.0  
**Role:** live AST graph, structural search, optional semantic retrieval/LSP.

**Study it for**

- structural AST search;
- signatures/outline retrieval;
- live no-index structural fallback;
- separation between deterministic live graph and optional semantic/GraphRAG layer;
- LSP as a precision augmentation.

**Relevant Blueprint atoms**

`BPT-074, BPT-092, BPT-093, BPT-094, BPT-095, BPT-096`.

**Important architectural lesson**

The live graph remains useful without embeddings/LLM enrichment. This supports Blueprint's benchmark-gated dense retrieval decision.

**Absorption status:** permissive donor.

---

## 7.12 Kythe

**Repository:** `kythe/kythe`  
**License:** Apache-2.0  
**Role:** compilation extraction, semantic identity/schema and indexer verification foundation.

**Study it for**

- stable semantic identity concepts;
- source anchors;
- compilation extraction pipelines;
- verifier/golden-fixture discipline;
- incomplete-better-than-incorrect indexer behavior.

**Relevant Blueprint atoms**

`BPT-072, BPT-073, BPT-076` and current identity/provenance repair.

**Do not copy architecturally**

- treating an interoperability schema as the agent UX/product layer.

**Absorption status:** permissive foundation.

---

## 7.13 stack-graphs

**Repository:** `github/stack-graphs`  
**License:** MIT OR Apache-2.0  
**Status caution:** repository is no longer actively supported/updated by GitHub.  
**Role:** incremental, build-independent name resolution.

**Study it for**

- lexical/scope binding;
- per-file reusable partial paths;
- resolution with pre/postconditions;
- query-time stitching;
- deterministic unresolved-frontier semantics.

**Relevant Blueprint atoms**

`BPT-075`, with concepts also informing `BPT-077`.

**Implementation rule**

If stack-graphs-style resolution becomes critical infrastructure, Blueprint should own the implementation/fork and golden tests instead of relying blindly on an archived dependency.

**Absorption status:** permissive concepts/code, but maintenance ownership required.

---

## 7.14 Joern

**Repository:** `joernio/joern`  
**License:** Apache-2.0  
**Role:** deep static-analysis plane using CPG/CFG/PDG/dataflow/taint.

**Study it for**

- optional statement-level deep-analysis provider design;
- control/data-flow representation;
- source-to-sink tracing;
- robust partial-code program analysis.

**Relevant Blueprint atoms**

None in the committed BPT-072–100 tranche. It is prior art only if Blueprint later exposes an optional deep-analysis provider boundary.

**Do not absorb into Blueprint core**

- always-on CPG/PDG generation;
- JDK/Scala runtime dependency in the resident hot path;
- general taint/security ownership.

**Absorption status:** permissive prior art/optional sidecar candidate, not foundational.

---

## 7.15 Semantica

**Repository:** `semantica-agi/semantica`  
**License:** MIT  
**Role:** general context/knowledge graph, provenance, ontology and deterministic reasoning.

**Study it for**

- provenance/conflict semantics;
- explicit inference vs source fact distinction;
- audit/export concepts;
- ontology/rule ideas if an adjacent Membrane system needs them.

**Relevant Blueprint areas**

Provenance doctrine only. General governance/decision/memory capabilities are outside Blueprint ownership.

**Do not absorb into Blueprint**

- durable general knowledge graph;
- project decision memory;
- ontology/governance platform responsibilities;
- broad RDF/LPG/vector polyglot architecture without a Blueprint-specific need.

**Absorption status:** permissive prior art, mostly out-of-boundary functionality.

---

# 8. BPT-072–100: first donor to inspect

This table is the practical lookup for implementation agents.

| Atom | Capability | Inspect first | Secondary reference | Main caution |
|---|---|---|---|---|
| BPT-072 | semantic authority precedence | SCIP / Kythe | Serena | freshness precedes producer tier |
| BPT-073 | semantic indexer orchestration | Infigraph | SCIP / Kythe | orchestration is Blueprint-owned; SCIP is transport |
| BPT-074 | on-demand LSP semantic cross-check | Serena | Octocode | agreement/conflict receipts only; not canonical producer |
| BPT-075 | generalized lexical/scope resolution | stack-graphs | CodeGraph | own maintenance/tests if implementation becomes critical |
| BPT-076 | type hierarchy/MRO/override facts | SCIP/Kythe-class producers | Serena | deterministic fallback only when authoritative tier unavailable |
| BPT-077 | resolution-frontier reporting | CodeGraph | stack-graphs concepts | return UNKNOWN/unresolved rather than guess |
| BPT-078 | dynamic-dispatch seams | CodeGraph | Infigraph | only explicit statically evidenced channels |
| BPT-079 | first-class test identity | Sense / Infigraph | GitNexus design prior art | test identity is structural, not runtime coverage |
| BPT-080 | static test reachability | Sense / Infigraph | GitNexus design prior art | UNKNOWN when reachability cannot be established |
| BPT-081 | entry-point registry | CodeGraph | Infigraph / GitNexus design prior art | framework evidence must be typed |
| BPT-082 | Process/Step projection | GitNexus design prior art | CodeGraph | derived projection, not runtime trace |
| BPT-083 | federation groups | Infigraph | GitNexus design prior art | overlay only; never merge repo node spaces |
| BPT-084 | contract registry | Infigraph | GitNexus design prior art | normalize explicit protocol identity |
| BPT-085 | consumer→provider evidence | Infigraph | GitNexus design prior art | bridge only explicit contracts |
| BPT-086 | cross-repo trace stitching | GitNexus design prior art | Infigraph | no same-name global linking |
| BPT-087 | DI facts | Infigraph | GitNexus design prior art | framework-specific provider/evidence contract |
| BPT-088 | ORM/query-target facts | GitNexus design prior art | Infigraph | reimplement; NC source is not a commercial code donor |
| BPT-089 | config-binding facts | Infigraph | GitNexus design prior art | avoid convention-only certainty without evidence |
| BPT-090 | RPC/MCP/tool handler facts | CodeGraph / Infigraph | GitNexus design prior art | model as entry-point/contract family |
| BPT-091 | UI screen/navigation facts | CodeGraph | Infigraph | computed/dynamic navigation may remain unresolved |
| BPT-092 | BM25 lexical code index | Infigraph | Octocode / Sense | identifier-aware; keep exact lookup first |
| BPT-093 | local semantic embeddings | Sense | Infigraph / Octocode | benchmark-gated, disposable projection |
| BPT-094 | hybrid retrieval fusion | Infigraph / Octocode | Sense | candidate discovery only; Recall admission still rules |
| BPT-095 | AST structural search | Octocode | Tree-sitter ecosystem | deterministic syntax query, not semantic inference |
| BPT-096 | compact symbol/signature projection | Serena / Octocode | SCIP | source/provenance/freshness remain attached |
| BPT-097 | cold-start orientation | Sense | GitNexus design prior art | deterministic regenerable summary, not memory |
| BPT-098 | query-time dirty-file repair | Sense | current Blueprint watcher | strict time/file budget; no full reconcile on query |
| BPT-099 | branch-switch coalesced reconciliation | Sense | Graphify hooks as secondary prior art | Git diff as one transition; watcher remains fallback |
| BPT-100 | liveness vs readiness | Potpie | codebase-graph | alive != current/ready |

---

# 9. Operational donor lookup outside BPT-072–100

Several critical Stage 1 implementation tasks are not represented only by the new atoms. Use these references:

| Blueprint implementation task | Primary donor(s) | What to study |
|---|---|---|
| canonical `blueprint init` | Sense, CodeGraph, Potpie | host detection, idempotent configuration, readiness output |
| agent host configuration | Sense, CodeGraph | Claude/Cursor/Codex/OpenCode-class setup |
| watcher branch behavior | Sense | Git transition batching |
| watcher freshness repair | Sense | per-file query-time repair |
| MCP resource model | GitNexus as design prior art | repo discovery/context resource patterns; reimplement independently |
| cold-start context | Sense | generated small repository summary |
| tool-surface discipline | Sense, CodeGraph | small, intentional menu |
| provenance UX | Graphify | extracted/inferred explanations |
| indexer verification | Kythe, SCIP | producer fixtures/conformance checks |
| canonical fact/projection split | Glean | fact/derived-predicate discipline |

---

# 10. License/absorption policy

## 9.1 Design idea vs code absorption

A public implementation may be studied for architecture regardless of whether its code is a suitable commercial donor. Direct code reuse is a separate decision.

For every donor-derived implementation:

1. identify the exact repository and current revision studied;
2. inspect the repository `LICENSE` directly;
3. inspect relevant subdirectory/file headers and vendored dependencies;
4. record whether implementation was copied, adapted, or independently reimplemented from behavior/specification;
5. retain required notices for copied/adapted permissive code;
6. never use incompatible source as a shortcut for a commercial Blueprint implementation.

## 9.2 Current high-level status

| Donor | High-level license status | Blueprint use status |
|---|---|---|
| Infigraph | Apache-2.0 | permissive donor |
| Potpie | Apache-2.0 | permissive donor; scope caution |
| GitNexus | PolyForm Noncommercial | design prior art only unless separately licensed |
| CodeGraph | MIT | permissive donor |
| Sense | MIT | permissive donor |
| codebase-graph | MIT | permissive donor |
| Graphify | Apache-2.0 | permissive donor; current LICENSE verified 2026-09-04 |
| Glean | BSD | permissive donor |
| SCIP | Apache-2.0 | permissive foundation |
| Serena | MIT | permissive donor |
| Octocode | Apache-2.0 | permissive donor |
| Kythe | Apache-2.0 | permissive foundation |
| stack-graphs | MIT OR Apache-2.0 | permissive but unmaintained/ownership caution |
| Joern | Apache-2.0 | permissive optional-provider prior art |
| Semantica | MIT | permissive prior art; most functionality out of scope |

This table is not a substitute for a file-level dependency audit before copying code.

---

# 11. Anti-copy rules

Donor research must not silently mutate Blueprint's architecture. In particular:

1. do not replace canonical typed facts with a donor-specific graph DB model;
2. do not copy a donor's visible tool menu as Blueprint's semantic API;
3. do not promote vector similarity into evidence authority;
4. do not import memory/editing/effects simply because a donor combines them with code intelligence;
5. do not merge repositories into a global same-name graph;
6. do not add confidence to authoritative facts because a donor uses numeric scoring everywhere;
7. do not turn LSP into a permanent fleet requirement;
8. do not turn Git hooks into the primary freshness system;
9. do not make CPG/PDG/taint mandatory for normal repository navigation;
10. do not copy GitNexus implementation code into commercial Blueprint without compatible licensing.

---

# 12. Agent checklist before using a donor

Before implementing a Blueprint atom from donor research, the coding agent should answer:

```text
1. Which BPT atom / implementation task am I solving?
2. Which donor is strongest specifically for that layer?
3. Is the donor agent UX, semantic foundation, deep-analysis, or context-plane prior art?
4. What exact behavior/algorithm/interface am I borrowing?
5. Does that behavior fit Blueprint's ownership boundary?
6. Does it preserve canonical fact vs projection separation?
7. Does it preserve Blueprint freshness/provenance/admissibility rules?
8. Is the donor license compatible with the intended type of reuse?
9. Can I reimplement the behavior independently if source reuse is unsafe?
10. What Blueprint-native acceptance test proves the resulting behavior?
```

If those questions cannot be answered, donor code should not be absorbed yet.

---

# 13. Summary

The donor set supports Blueprint's v2 direction rather than changing it.

The useful synthesis is:

```text
Sense / CodeGraph
    → agent/runtime ergonomics and freshness

SCIP / Kythe / stack-graphs / Glean
    → semantic identity, resolution and fact discipline

Infigraph
    → broad orchestration/framework/retrieval prior art

Serena / Octocode
    → live LSP precision and compact structural retrieval

GitNexus
    → process/contracts/resources ideas, design-only unless licensed

Graphify
    → provenance/artifact UX

Joern
    → optional deep-analysis provider prior art

Potpie / Semantica
    → readiness/provenance/context concepts without importing their ownership domains
```

Blueprint remains its own architecture: continuously fresh, evidence-first, canonical-fact-backed, projection-oriented and deliberately small in its resident/runtime dependencies.

The Loom synthesis therefore changes **verification and safety discipline**, not Blueprint's ownership boundary or runtime architecture.


# Code-only evidence audit

The later 343-atom code-only matrix and Blueprint code audit are implementation evidence supplements, not donor-architecture authority.

They add several concrete lessons:

- **Sense:** make relation vocabulary producer/consumer parity executable in CI.
- **Graphify:** key deterministic extraction caches by extractor/schema semantics, not source bytes alone.
- **SCIP/Kythe:** preserve one canonical semantic normalization contract and portable identities/anchors.
- **CodeGraph/Infigraph:** prioritize receiver/member/type/hierarchy precision and modern JS/TS resolution.
- **GitNexus:** distinguish canonical broker/route/datastore identities from source occurrences.
- **Octocode/codebase-graph:** retrieval ranking is valuable only as an optional candidate lane around exact graph/Recall semantics.

Do **not** import the composite matrix's storage prescription (`SQLite + Kùzu + LanceDB`) as a Blueprint requirement. Current Blueprint already has SQLite/FTS/vector persistence and measured brute-force cosine rationale; another graph/vector engine requires a demonstrated workload failure, not donor popularity.


# Final Membrane subsystem reconciliation

The final `membrane-blueprint.md` deep-dive is treated as a **doctrine constraint**, not another donor design. It changes the donor-use interpretation in four ways:

1. **Sense conventions:** now committed as descriptive `WeakEvidence` with support/coverage and counterexamples; never a policy source.
2. **stack-graphs:** use the rules-as-data idea selectively so language resolution can be inspected/versioned/tested, while Blueprint core retains freshness/admission/tier-dominance invariants.
3. **Serena:** LSP is a bounded cross-check/verifier only. It may produce agreement/conflict receipts, not canonical graph edges or edit authority.
4. **Octocode/codebase-graph semantic retrieval:** embeddings/hybrid remain exploratory, local and retrieval-only. Similarity is not a resolution tier and cannot override Recall/admission.

Additional useful ideas are absorbed as implementation obligations rather than atoms: deterministic standard SCIP export for the losslessly representable semantic subset, Blueprint-native export for richer evidence, and bounded point-in-generation/history queries over retained generations. Git-derived hotspots/ownership remain exploratory weak analytics.
