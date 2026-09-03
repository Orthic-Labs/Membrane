# Gap & Redundancy Analysis — 26 Agent-Memory / Code-Intelligence Repos

**Companion to:** `repos-feature-union.md` (the 22-domain, 236-atom master feature union)
**Source of truth:** all counts below were parsed programmatically from `repos-feature-union.md` — the winner map (best-in-class per atom) and the per-repo coverage (derived from the `[tags]` on each capability atom). No new repo reads were needed; this is a re-projection of that data.
**All 26 repos are external checkouts under `\\192.168.1.7\d\claude\repos\membrane`; none were modified.**

---

## Headline findings

1. **The union is almost maximally non-redundant.** Of 236 feature atoms, **only 2 repos (#7 codegraph, #21 potpie) are fully subsumable** — every capability they offer is also provided by at least one other repo (they have **0 sole-provider atoms**). The other **24 repos each uniquely provide ≥1 capability**. → Pruning to 5–10 repos therefore *always* drops real features; the question is *which* capabilities you are willing to lose, not *which repos are weakest*.

2. **A top-10 prune by value index covers 196/236 atoms (83%)** but **29 of the 40 dropped atoms are sole-provider** — i.e. they vanish completely. The losses cluster into coherent themes (deep code-analysis/taint, IDE edit/debug, multi-agent orchestration, evolutionary memory), not random leftovers.

3. **Two repos are the clear "hubs":** #13 hindsight (33 best-in-class wins) and #15 infigraph (17/22 domain coverage, 27 wins). They are the only repos that are both broad *and* deep.

---

## 1. Scorecard — every repo ranked by value index

**Value index = domains covered (of 22, derived from `[tags]`) + best-in-class wins (of 236).** "Sole" = number of feature atoms only this repo provides. "Missing" = count of the 22 domains the repo does not touch.

| # | Repo | Cov | Wins | Sole | ValIdx | Missing domains (count) |
|---|------|----:|-----:|-----:|-------:|--------------------------|
| 13 | hindsight | 18 | 33 | 5 | **51** | Chunking, Graph, CodeNav, CodeAnalysis (4) |
| 15 | infigraph | 17 | 27 | 6 | **44** | Memory, Lifecycle, Temporal, Storage, Observ (5) |
| 5 | GitNexus | 16 | 17 | 7 | **33** | Memory, Lifecycle, Temporal, LLM, WebUI, Ext (6) |
| 22 | superlocalmemory | 18 | 15 | 7 | **33** | Chunking, CodeNav, CodeAnalysis, IDE (4) |
| 2 | MemOS | 17 | 14 | 4 | **31** | CodeNav, CodeAnalysis, Temporal, IDE, WebUI (5) |
| 10 | graphiti | 15 | 16 | 8 | **31** | Chunking, Lexical, CodeNav, CodeAnalysis, IDE, WebUI, Ops (7) |
| 8 | rag-rat | 16 | 14 | 5 | **30** | Chunking, SymEx, Graph, Scope, LLM, WebUI (6) |
| 6 | caura | 17 | 8 | 1 | **25** | Chunking, CodeNav, CodeAnalysis, IDE, WebUI (5) |
| 3 | octocode | 14 | 9 | 4 | **24** | CodeAnalysis, Memory, Lifecycle, Temporal, Scope, LLM, Sec (7) |
| 4 | codebase-graph | 19 | 4 | 2 | **23** | Memory, Scope, Ops (3) |
| 23 | supermemory | 14 | 9 | 2 | **23** | SymEx, Graph, Lexical, CodeNav, CodeAnalysis, Temporal, Storage, Sec (8) |
| 24 | OpenViking | 15 | 7 | 6 | **22** | Chunking, SymEx, Vector, Lexical, CodeNav, CodeAnalysis, Temporal (7) |
| 7 | codegraph | 16 | 2 | 0 | **18** | Memory, Lifecycle, Temporal, Scope, LLM, WebUI (6) |
| 16 | sense | 15 | 3 | 1 | **18** | Memory, Lifecycle, Temporal, Scope, Storage, LLM, Sec (7) |
| 17 | mengram | 12 | 6 | 3 | **18** | Chunking, SymEx, Lexical, Hybrid, CodeNav, CodeAnalysis, Temporal, Storage, WebUI, Sec (10) |
| 20 | serena | 9 | 8 | 3 | **17** | Chunking, Vector, Hybrid, CodeAnalysis, Memory, Lifecycle, Temporal, Scope, Storage, LLM, WebUI, Sec, Observ (13) |
| 26 | opengrok | 10 | 6 | 2 | **16** | Graph, Vector, Hybrid, CodeNav, Memory, Lifecycle, Temporal, Scope, LLM, IDE, WebUI, Ext (12) |
| 1 | Understand-Anything | 10 | 4 | 1 | **14** | Lexical, CodeNav, CodeAnalysis, Memory, Lifecycle, Temporal, Scope, Storage, LLM, Sec, Observ, Ops (12) |
| 12 | codeql | 6 | 8 | 2 | **14** | Ingestion, Chunking, SymEx, Graph, Vector, Lexical, Hybrid, CodeNav, Memory, Lifecycle, Temporal, Scope, Storage, APIs, LLM, Sec (16) |
| 19 | mnemosyne | 12 | 2 | 1 | **14** | Ingestion, Chunking, Vector, Lexical, CodeNav, CodeAnalysis, LLM, WebUI, Sec, Ext (10) |
| 11 | graph-memory-starter | 9 | 4 | 1 | **13** | CodeNav, CodeAnalysis, Lifecycle, Temporal, Scope, APIs, LLM, IDE, WebUI, Sec, Observ, Ext, Ops (13) |
| 25 | joern | 8 | 5 | 1 | **13** | SymEx, Vector, Lexical, Hybrid, CodeNav, Memory, Lifecycle, Temporal, Scope, LLM, IDE, WebUI, Sec, Observ (14) |
| 14 | honcho | 8 | 4 | 2 | **12** | Chunking, SymEx, Graph, Vector, Lexical, CodeNav, CodeAnalysis, Lifecycle, Temporal, LLM, WebUI, Sec, Observ, Ext (14) |
| 9 | emulo | 6 | 5 | 2 | **11** | Chunking, SymEx, Graph, Vector, Lexical, Hybrid, CodeNav, CodeAnalysis, Memory, Scope, Storage, APIs, LLM, WebUI, Ext, Ops (16) |
| 18 | mnemon | 7 | 4 | 2 | **11** | Ingestion, Chunking, SymEx, Vector, Lexical, Hybrid, CodeNav, CodeAnalysis, Memory, Temporal, Storage, WebUI, Sec, Observ, Ops (15) |
| 21 | potpie | 7 | 2 | 0 | **9** | Chunking, SymEx, Graph, Vector, Lexical, Hybrid, CodeNav, CodeAnalysis, Memory, Lifecycle, Temporal, Storage, APIs, LLM, Sec (15) |

*Domain key: 1 Ingestion · 2 Chunking · 3 SymEx · 4 Graph · 5 Vector · 6 Lexical · 7 Hybrid · 8 CodeNav · 9 CodeAnalysis · 10 Memory · 11 Lifecycle · 12 Temporal · 13 Scope · 14 Storage · 15 APIs · 16 LLM · 17 IDE · 18 WebUI · 19 Sec · 20 Observ · 21 Ext · 22 Ops*

---

## 2. Union-critical repos (cannot be dropped without losing a feature)

Sorted by number of **sole-provider atoms** — the unique capabilities only this repo contributes to the master union.

| # | Repo | Sole atoms | What only it provides |
|---|------|-----------:|------------------------|
| 10 | graphiti | 8 | bulk episode ingest; Pydantic entity/edge types; GLiNER2 local (no-LLM) extraction; Episodic/Entity/Community/**Saga** node model; memory-type ontology; `group_id` graph partitioning |
| 5 | GitNexus | 7 | DI extraction; directed call graph + impact clusters; **trace shortest call path**; API-route↔component mapping + shape checks; MCP/RPC tool discovery; LadybugDB/WASM-in-browser storage |
| 22 | superlocalmemory | 7 | CozoDB graph + LanceDB vector projections (parity-gated); **Hopfield/spreading-activation recall**; scene/entity timelines; personal/shared/global scopes (default-deny); GDPR/EU-AI-Act audit; SLM-Mesh peer coordination |
| 15 | infigraph | 6 | dbt/Airflow pipeline ingestion; **OSV vuln scan at ingest**; ANN at scale (HNSW ~2 ms/500k); design-pattern detection; Mermaid sequence diagrams; pipeline plugins |
| 24 | OpenViking | 6 | directory-recursive retrieval; **L0/L1/L2 tiered context loading**; VikingDB managed; **multi-agent orchestration** (VikingBot); hosted Studio; desktop Helper console |
| 8 | rag-rat | 5 | git-blame-per-chunk; source-anchored repo memories (Invariant/Decision/Risk); per-path/symbol git-history reasoning; PR→decision records (`distill`); TOON token-efficient output |
| 13 | hindsight | 5 | "world facts/experiences/observations" biomimetic model; knowledge pages (DB-read, no LLM); Oracle AI DB 23ai backend; broadest agent frameworks; `reflect` reasoning pass |
| 2 | MemOS | 4 | **parametric/activation (KV-cache/LoRA) memory**; tier promotion/demotion; Qdrant/Milvus backends; deepsearch agent |
| 3 | octocode | 4 | LLM contextual chunk descriptions; live in-memory vs persisted GraphRAG; multi-vector per repo; graph-expansion rerank |
| 20 | serena | 3 | LSP diagnostics/inspections; **interactive REPL debugging**; composable multi-level YAML |
| 17 | mengram | 3 | **procedural/evolving memory**; cognitive profile (`get_profile()`→system prompt); procedural self-improvement on failure |
| 4 | codebase-graph | 2 | dual-retrieval consensus scoring; MCP graph-explorer UI |
| 23 | supermemory | 2 | vision/OCR + video-transcription embeddings; one-call RAG+memory hybrid query |
| 9 | emulo | 2 | coding-agent session-log mining → `you.md`; model-free `--coach` usage report |
| 14 | honcho | 2 | **peer model** (users/agents/groups/projects as entities); who-knows-whom modeling |
| 26 | opengrok | 2 | Apache Lucene disk index (40+ analyzers); mirror/sync upstream before index |
| 18 | mnemon | 2 | four-graph memory store (temporal/entity/semantic/causal); LLM-supervised memory decisions |
| 12 | codeql | 2 | **LLM/AI-SDK taint sources/sinks**; coverage metrics over time |
| 1 | Understand-Anything | 1 | **Figma/design-file parsing** → design graph |
| 6 | caura | 1 | crystallization of stale rows into canonical facts |
| 11 | graph-memory-starter | 1 | closed-vocabulary (PERSON/ROLE/POLICY…) deterministic extraction |
| 16 | sense | 1 | cold-start codebase summary map (`.sense/summary.md`) |
| 25 | joern | 1 | flatgraph/overflowdb graph store (research-grade CPG) |
| 19 | mnemosyne | 1 | memory TTL / `valid_until` expiry |

**Takeaway:** 24 rows above are each non-optional for a *complete* union. The only two repos with zero unique capability are **#7 codegraph** and **#21 potpie** (see §3).

---

## 3. Fully subsumable repos (redundancy candidates)

These two are covered end-to-end by other repos in the collection — dropping them loses **no** unique feature.

| # | Repo | Why it's redundant |
|---|------|---------------------|
| 7 | codegraph (colbymchenry) | Every capability (callers/callees, OS-native FS watcher, AST chunking, impact/blast-radius) is also present in #3 octocode, #5 GitNexus, #15 infigraph, #16 sense. Sole atoms = 0. |
| 21 | potpie | SDLC ingestion (PRs/issues/Confluence) overlaps #4/#8/#15; agent/prompt builders overlap #13/#24. Sole atoms = 0. Useful as a ready-made product, not as a *unique* capability source. |

*Note:* "subsumable" means capability coverage, not product value. potpie is a polished turnkey agent-builder; codegraph is a clean lightweight navigator. They may still be the *easiest* way to get a capability even if not the *only* way.

---

## 4. What a top-10 prune actually costs

**Keep-set (top-10 by value index):** #13, #15, #5, #22, #2, #10, #8, #6, #3, #4
**Result:** 196/236 atoms retained (**83%**). **40 atoms lost, of which 29 are sole-provider → they disappear entirely.**

The 29 forced losses, grouped by capability theme:

**A. Deep code analysis & taint (the biggest gap)**
- #12 codeql — LLM/AI-SDK taint sources/sinks; coverage metrics over time
- #25 joern — flatgraph/overflowdb CPG store
- #26 opengrok — Lucene disk index; mirror/sync upstream before index

**B. IDE editing & debugging**
- #20 serena — LSP diagnostics/inspections; interactive REPL debugging; composable YAML

**C. Multi-agent orchestration & UX surface**
- #24 OpenViking — directory-recursive retrieval; L0/L1/L2 context; VikingDB; **multi-agent orchestration**; hosted Studio; desktop Helper

**D. Evolutionary / procedural memory**
- #17 mengram — procedural memory; cognitive profile; procedural self-improvement on failure
- #18 mnemon — four-graph memory store; LLM-supervised memory decisions

**E. Relational / peer memory**
- #14 honcho — peer model; who-knows-whom modeling

**F. Niche but unique**
- #1 Understand-Anything — Figma/design-file parsing
- #9 emulo — coding-agent session-log mining; model-free coach
- #11 graph-memory-starter — closed-vocabulary extraction
- #16 sense — cold-start codebase summary map
- #19 mnemosyne — memory TTL/expiry
- #23 supermemory — vision/OCR + video-transcription embeddings; one-call RAG+memory hybrid query

**Conclusion:** a breadth-first top-10 keeps all the *general-purpose* memory + code-intel hubs but **surrenders the specialist depth** — security-grade taint analysis (codeql/joern/opengrok), live IDE debugging (serena), and multi-agent orchestration (OpenViking). If those matter, the prune must be goal-scoped instead (§5).

---

## 5. Prune strategies

**Strategy A — Breadth (top-10 by value index).** 83% atom coverage. Best when you want one stack that does "most things." Loses themes A–F above.

**Strategy B — Capability-complete (keep all 24 sole-provider repos).** 100% of the union. Only #7 and #21 are dropped. Best when no capability may be lost; cost is operational complexity of 24 systems.

**Strategy C — Goal-scoped (recommended for real use).** Pick the keep-set by objective:

| Goal | Keep | Why |
|------|------|-----|
| **Memory / agent context only** | #13, #2, #22, #10, #6, #17, #14, #18, #19, #23 | covers memory model, lifecycle, temporal, multi-tenant, governance; drops pure code-analysis repos |
| **Code intelligence / static analysis only** | #15, #5, #25, #12, #26, #8, #4, #20, #16, #3 | covers indexing, CPG/taint, navigation, IDE debug, refactor; drops pure memory repos |
| **Security / vuln research** | #12, #25, #26, #15, #8, #5 | codeql + joern + opengrok + infigraph for taint/CPG; rag-rat for source-anchored provenance |
| **Turnkey SaaS / managed** | #13, #23, #22, #24 | hindsight/supermemory/SLM clouds + OpenViking Studio — least self-hosting |
| **Offline / air-gapped** | #15, #7, #24, #3 | infigraph (offline-first), codegraph, OpenViking, octocode — no external calls |

---

## 6. Notes & caveats

- All figures are re-projections of `repos-feature-union.md` (itself derived from each repo's README/docs/MCP tool defs). "N MCP tools" and capability claims are version-specific per each repo's own docs.
- **Coverage is derived from the `[tags]` on each capability atom** (the authoritative per-atom evidence), not hand-estimated; the coverage matrix in the union doc was aligned to match.
- "Sole-provider" means *unique within this 26-repo set*. A capability marked sole may still exist in some other repo not in this collection.
- Value index weights breadth (domain count) and depth (best-in-class wins) equally; adjust weights if a goal favors one (e.g. depth-only → rank by Wins; breadth-only → rank by Cov).
- No repos were modified. This document is analysis only.
