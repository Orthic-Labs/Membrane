# Master Feature Union — 26 Agent-Memory / Code-Intelligence Repos

**Source:** all checkouts under `\\192.168.1.7\d\claude\repos\membrane`
**Method:** each repo was read from its checked-out source (README, docs, MCP tool defs, CLI/route/SDK surfaces). This document is the **set-union** of every distinct capability any of them provides — a "sum of features" across the collection.
**Interpretation of the request:** "add into the next one" was treated as building a *running union* — repo 1's capabilities, plus whatever repo 2 adds that's new, etc. The repos themselves were not modified (they're external checkouts); only this consolidated list was produced.

---

## Legend (repo tags used below)

| # | Repo | # | Repo |
|---|------|---|------|
| 1 | Egonex-AI/Understand-Anything | 14 | plastic-labs/honcho |
| 2 | MemTensor/MemOS | 15 | intuit/infigraph |
| 3 | Muvon/octocode | 16 | luuuc/sense |
| 4 | Phoenixrr2113/codebase-graph | 17 | alibaizhanov/mengram |
| 5 | abhigyanpatwari/GitNexus | 18 | mnemon-dev/mnemon |
| 6 | caura-ai/caura | 19 | mnemosyne-oss/mnemosyne |
| 7 | colbymchenry/codegraph | 20 | oraios/serena |
| 8 | cq27-dev/rag-rat | 21 | potpie-ai/potpie |
| 9 | ohad6k/emulo | 22 | qualixar/superlocalmemory (SLM) |
| 10 | getzep/graphiti | 23 | supermemoryai/supermemory |
| 11 | Glitch-Cat-Club/graph-memory-starter | 24 | volcengine/OpenViking |
| 12 | github/codeql | 25 | joernio/joern |
| 13 | vectorize-io/hindsight | 26 | oracle/opengrok |

---

## Semantic buckets (orientation)

The 22 domains below group into five higher-level buckets — this is the "code-intelligence vs memory vs everything-else" split. Use it to navigate the union and to scope a prune.

| Bucket | Domains | What it covers |
|--------|---------|----------------|
| **Ingestion & Representation** | 1 Ingestion, 2 Chunking, 3 Symbol/Entity Extraction, 4 Graph Construction | getting code/docs in, parsing, extracting symbols, building graphs |
| **Retrieval & Search** | 5 Embeddings/Vector, 6 Keyword/Lexical, 7 Hybrid/Ranked | how content is embedded, lexically searched, and fused/reranked |
| **Code Intelligence** | 8 Code Navigation, 9 Code Analysis | LSP refs/defs, taint/dataflow, CPG, vulnerability paths |
| **Agent Memory** | 10 Memory Model, 11 Memory Lifecycle, 12 Temporal, 13 Multi-tenant/Scope | memory types, consolidation/decay, bi-temporal reasoning, isolation |
| **Platform & Ops** | 14 Storage, 15 APIs/Protocols, 16 LLM/Agent, 17 IDE/CI, 18 Web UI, 19 Security, 20 Observability, 21 Extensibility, 22 Ops/Deployment | backends, MCP/REST/LSP surfaces, agent frameworks, IDE/CI, UI, security, eval, deployment |

**Cross-cutting split:** repos #1–#9, #15, #25, #26 are predominantly *Code Intelligence + Retrieval* engines; repos #10–#24 are predominantly *Agent Memory* systems; #11 (minimal RAG starter) and #12 (QL query library) are special-purpose. The companion `repos-gap-analysis.md` re-projects this union into per-repo coverage, redundancy, and prune strategies.

---

## 1. Ingestion & Indexing

- Index a codebase into a persistent graph / vector store / knowledge base — [1,2,3,4,5,6,7,8,10,11,15,16,20,21]
- Incremental (fingerprint / content-hash / branch-delta) re-indexing — [1,3,5,7,8,10,15,16]
- File-system watcher auto-reindex (debounced) in background — [3,5,7,8,15,16]
- Git history / commit / PR / branch ingestion — [4,5,8,10,15,21,24]
- GitHub PR + issue + review ingestion — [4,21]
- Documents / files / URLs / raw text ingestion — [1,2,4,6,10,13,17,23]
- Multi-modal ingestion (images, audio, video, PDF, DOCX) — [2,17,22,23]
- Conversation / chat transcript ingestion + entity extraction — [4,9,14,17,23]
- Coding-agent session-log mining into a private profile — [9]
- 1-click/zero-config auto-ingest from agent history (no command) — [13,17,23,24]
- Wiki / knowledge-base ingestion (Confluence, Notion, Google Docs, Figma) — [1,4,15,21,23]
- Project structural scan with language/framework detection — [1,4,5,7,16]
- Scope indexing to a subdirectory / exclude globs — [1,4,7,15,26]
- Per-branch / per-repo delta indexes — [3,8]
- Multi-repo federation / cross-repo indexing — [5,15,21,25,26]
- Auto reindex on PR merge via webhook — [15,26]
- Bulk / batched episode ingestion — [10]
- Import Claude Code / ChatGPT / Obsidian history — [9,17]
- Data pipeline ingestion (dbt, Airflow) via structured schemas — [15]
- OSV / dependency vulnerability scanning during ingest — [15]

## 2. Parsing & Chunking

- Tree-sitter parsing (10–62 languages, varies per repo) — [1,3,4,5,7,15,16]
- ANTLR / custom DSL grammar plugins — [15,25]
- Heading-aware / structural Markdown chunking — [4,11]
- Character / token / sentence / region chunking — [2,3,5,11]
- AST-aware code chunking — [3,23]
- Figma / design-file parsing — [1]
- Non-code formats (Dockerfile, SQL, YAML, JSON, protobuf, env, config) — [1,4,26]
- LLM contextual chunk descriptions at index time — [3]
- Auto-generate project ignore / exclude rules — [1,7]

## 3. Symbol / Entity Extraction

- Extract functions, classes, interfaces, types, variables, params — [1,3,4,5,7,16]
- Extract calls / imports / inheritance / type refs / JSX renders — [4,5,7,16]
- COBOL / mainframe / legacy-language extraction — [5,26]
- Framework wiring detection (Spring, HTTP/gRPC, route constants) — [5,7,15,16]
- LLM-based entity + relation extraction with schemas — [2,4,6,10,13,19,22]
- Custom Pydantic entity/edge types — [10]
- GLiNER2 local extraction (no LLM) — [10]
- Closed-vocabulary entity extraction (PERSON/ROLE/POLICY…) — [11]
- Aliasing / name-variant resolution — [4,6,11]
- Entity resolution (exact / canonical / semantic) — [6,22]
- DI / dependency-injection extraction — [5]
- Definition + reference resolution (ctags/LSP) — [20,26]

## 4. Graph Construction

- Code property graph (CPG) / AST+CFG+DDG+CDG — [4,5,7,15,20,25]
- Knowledge graph with typed nodes + edges — [1,2,4,6,10,11,17,19,22,24]
- Bi-temporal graph (valid_at / invalid_at) — [4,10]
- Call / import / inheritance / extends / implements edges — [3,4,5,7,16]
- Community detection (Louvain / Leiden) + summaries — [1,4,5,10]
- Symbol graph (callers/callees) generated on demand — [3,7,16]
- Live in-memory vs persisted GraphRAG — [3]
- Episodic / Entity / Community / Saga node model — [10]
- Memory graph (temporal, entity, semantic, causal edges) — [18]
- Directed call graph + impact/process/cluster grouping — [5]
- Export graph as Neo4j / GraphML / JSON / Cypher / DOT — [4,15,25]
- Configurable graph + vector projections (CozoDB, LanceDB) — [22]

## 5. Embeddings & Vector Search

- Local on-device embeddings (no API key) — [1,3,5,7,8,11,15,16]
- Pluggable embedding providers (OpenAI, Ollama, Voyage, Jina, BGE, Cohere, …) — [2,3,4,8,10,13,17]
- pgvector / LanceDB / Qdrant / Milvus / sqlite-vec backends — [2,6,13,22]
- HNSW / IVF / RaBitQ / int8 vector quantization — [4,15,16]
- Cosine / semantic similarity search — [1,2,3,4,10,16]
- Approximate nearest-neighbor at large scale — [15]
- Multilingual embeddings (23+ languages) — [17,23]
- Vision / OCR embedding (images, video transcription) — [23]
- Multi-vector per repo (code/text/docs/commits/graph nodes) — [3]

## 6. Keyword / Lexical Search

- BM25 full-text search — [2,3,4,5,6,7,8,11,13,15,16,22,26]
- FTS5 (SQLite) search — [7,11]
- CJK / multi-script segmentation — [5,11]
- ripgrep / regex pattern fallback — [16,20]
- Commit / history / annotation search — [8,26]
- Git blame per chunk / symbol — [8]

## 7. Hybrid & Ranked Retrieval

- Vector + keyword (RRF fusion) — [1,3,5,6,7,8,11,13,14,15,16,22]
- Graph + semantic + temporal + lexical multi-channel — [4,10,13,22]
- Hopfield / spreading-activation associative recall — [22]
- Re-ranking (cross-encoder / LLM / BGE / Cohere / local) — [2,3,4,6,13,15,16,22]
- Query-shape auto-weighting / adaptive budget — [3,7]
- Graph-expansion of candidates before rerank — [3]
- Dual-retrieval consensus scoring — [4]
- RAG + memory hybrid query (one call) — [23]
- Directory-recursive retrieval (vector-locate-dir then drill) — [24]
- Temporal / recency boosting — [13,19,22]
- Source-anchored provenance + confidence per result — [8,22]

## 8. Code Navigation (LSP / refs / defs)

- goto-definition, find-references, hover, document/workspace symbols — [3,20]
- Find callers / callees — [3,4,5,7,16]
- Trace shortest call / dependency path between symbols — [5]
- Multi-file coordinated rename — [5,20]
- Impact / blast-radius from a symbol — [3,4,5,7,8,15,16]
- Dead-code detection — [5,7,15,16]
- Cross-language FFI / bridge resolution (Swift↔ObjC, JNI, gRPC, WASM) — [7,15]
- Diagnostics / inspections — [20]
- Interactive debugging (breakpoints, REPL) — [20]

## 9. Code Analysis (static / dataflow / taint)

- Interprocedural taint / dataflow tracking — [8,12,15,25,26]
- Source→sink vulnerability paths — [8,12,25,26]
- Control-/data-dependence (PDG) queries — [5,8]
- API route ↔ component mapping + shape checks — [5]
- MCP/RPC tool-definition discovery — [5]
- Security query packs (CWE-classified) — [12,15,26]
- QL / CPGQL / Cypher / custom query languages — [4,12,15,25]
- Custom extractors / query packs for new languages — [12,15,25]
- Design-pattern detection (Singleton, Factory, …) — [15]
- Complexity / coupling / hotspot analysis — [4,5,15]
- Near-duplicate / clone detection — [8,15]
- Refactor analysis + affected tests — [4,7,15,16]
- PR-type classification + CI gating — [4,15]
- LLM/AI SDK taint sources/sinks — [12]
- Multi-repo variant analysis (MRVA) — [12,15]

## 10. Memory Model & Types

- Episodic memory (events, decisions, outcomes) — [2,6,10,13,17,19,22]
- Semantic memory (facts, preferences) — [6,13,17,19,22,23]
- Procedural memory (workflows that evolve) — [17]
- World facts / experiences / observations / mental models — [13]
- Parametric / activation (KV-cache) / plaintext memory — [2]
- Working / long-term / user / outer memory tiers — [2,19]
- Typed memories (14–15 classes: fact, decision, preference, skill, risk, bug-pattern…) — [6,8,22]
- MemCube / bank isolation (per user/agent/project) — [2,11,13,19]
- Knowledge pages / mental models (DB-read, no LLM) — [13]
- Cognitive profile (system prompt from memory) — [17]
- User profiles (static + dynamic activity) — [13,23,24]
- Skills (evolving, versioned) — [6,22]
- Memory types: preference / requirement / procedure / topic / person… — [10]
- Source-anchored repo memories (Invariant, Decision, Risk…) — [8]
- Three-layer context (L0/L1/L2) loading — [24]
- Peer model (users/agents/groups/projects as entities) — [14]

## 11. Memory Lifecycle (consolidation / decay / forgetting / promotion)

- Consolidation / deduplication / merge of memories — [2,6,8,13,18,19,22]
- Decay / forgetting / self-archive of stale memory — [4,6,13,19,22]
- Contradiction detection + supersede (not delete) — [4,6,10,23]
- Crystallization into canonical facts — [6]
- Importance / confidence weighting + decay — [6,18,19]
- Auto memory extraction from sessions in background — [9,13,17,24]
- "Dream" maintenance loop (coverage gaps, stale refs) — [2,8]
- Feedback-driven reinforcement / correction — [2,6,23]
- Promotion/demotion of memory tiers — [2]
- Expiry / TTL on memories — [19]
- Redaction / forget PII before storage — [9,13,22]
- Skill evolution from failures — [17,22]
- Procedural self-improvement on failure — [17]

## 12. Temporal Reasoning

- Bi-temporal facts (ingest time + reference time) — [4,10]
- Recall "as of" a point in time — [4,10,19]
- Recency / time-range filtering — [13,19,22]
- Git history reasoning (per-path/symbol/commit) — [8]
- Decision records from issues/PRs — [8]
- Scene / entity timelines — [22]
- Event timestamps preserved end-to-end — [6,9,22]

## 13. Multi-tenant / Scoping / Namespaces

- Per-user / per-agent / per-project isolation — [2,6,13,17,18,19,22,23,24]
- Personal / shared / global memory scopes (default-deny cross-profile) — [22]
- Group_id partitioning of graph — [10]
- Workspace / tenant isolation — [6,14]
- Cross-repo namespaces + links — [5,15,21]
- Role-based access / trust tiers — [6,22]
- Peers: who-knows-whom modeling — [14]

## 14. Persistence & Storage Backends

- SQLite (incl. sqlite-vec, FTS5) — [7,8,11,19,22]
- PostgreSQL + pgvector — [6,13]
- Neo4j / FalkorDB / Kuzu / PolarDB — [2,4,10]
- Qdrant / Milvus — [2]
- Oracle AI DB 23ai — [13]
- LadybugDB / WASM in-browser — [5]
- LanceDB (Arrow) — [3,22]
- CozoDB graph projection — [22]
- Redis (cache / queue) — [2,6,14]
- Flatgraph / overflowdb graph store — [25]
- Lucene index (disk) — [26]
- VikingDB (managed) — [24]

## 15. APIs & Protocols (MCP / REST / CLI / LSP / gRPC)

- MCP server (stdio / SSE / Streamable HTTP) — [2,3,4,7,8,10,13,16,17,18,19,20,22,23,24]
- Remote / HTTP MCP with auth — [4,10,15,22]
- MCP tools count: up to 94 (SLM) / 82 (infigraph) / 69 / 61 / 47 (rag-rat) / 31 / 22 / 17 / 16 / 13 / 5 / 4 / 3 / 1 — [3,4,5,7,8,10,13,15,16,19,20,22,23,24]
- REST API — [2,4,6,10,13,14,17,23,26]
- CLI (rich subcommands) — [1–26 nearly all]
- Python / TypeScript / Go SDK — [2,4,6,10,13,14,17,19,23,24]
- LSP server integration — [3,16,20,25]
- gRPC — (rare; none dominant) —
- OpenAPI client / Swagger — [2,26]
- Webhooks (lifecycle events, PR reindex) — [13,15,22,26]
- TOON / JSON output formats — [8]

## 16. LLM & Agent Integration

- 25+ / many LLM providers (OpenAI, Anthropic, Gemini, Ollama, local, OpenAI-compatible) — [2,4,6,10,13,15,17,22,24]
- LLM-wrapper that auto stores/retrieves (no code change) — [13,23]
- LangChain / LangGraph / LlamaIndex / CrewAI / Pydantic-AI / OpenAI Agents / Agno / Vercel AI SDK / Mastra / n8n — [6,13,17,23]
- Agent frameworks (AutoGen, Microsoft Agent, Google ADK, Strands, Haystack) — [13]
- Deepsearch / query-rewrite agent — [2]
- Reflect / reasoning pass over memories — [13]
- LLM supervision of memory decisions (host LLM as judge) — [18]
- Multi-agent orchestration (VikingBot, OpenViking Helper) — [24]

## 17. IDE / Editor / Chat / CI Integration

- Claude Code — [1,3,5,7,8,9,13,15,16,17,18,19,20,21,23,24]
- Cursor — [1,3,5,7,9,13,15,16,17,20,21,23,24]
- Codex — [1,3,8,9,16,18,20,21,23,24]
- OpenCode — [1,3,8,9,13,16,18,20,21,23,24]
- Windsurf / Cline / Kiro / Gemini / Antigravity / others — [1,3,5,7,9,13,15,16,17,23]
- OpenClaw / Hermes — [13,14,19,23,24]
- GitHub Copilot — [5,13,20]
- VS Code / JetBrains / web clients — [1,3,20,23]
- Pi — [18,19,24]
- ZCode / MiniMax / TRAE / Qoder / CodeBuddy / WorkBuddy / Kimi — [18,24]
- CI integration (agent test selection, gates) — [4,7,12,15,16]
- PreToolUse / PostToolUse / SessionStart hooks — [1,5,7,8,9,13,17,18,19,24]

## 18. Web UI & Visualization

- Interactive graph / knowledge-graph explorer — [1,3,4,15,21,22]
- Dashboard (health, memories, brain, entities) — [13,22,23]
- Cold-start codebase summary map — [16]
- Sequence diagrams from call graphs — [15]
- MCP graph-explorer UI resource — [4]
- Studio playground (hosted) — [24]
- Query-help docs / benchmarks UI — [12,13]
- Desktop helper console — [24]

## 19. Security & Access Control

- API-key auth (hashed, scoped, rotatable) — [2,4,15,22]
- Rate limiting — [2,6]
- PII / secret redaction & blocking — [6,9,13,22]
- Read-only mode for MCP / source — [4,5,8]
- Repo allowlists / policy — [5]
- LDAP / group / user authorization plugins — [4,26]
- GDPR / EU AI Act retention + audit chain — [22]
- Hash-chained tamper-evident audit log — [6,22]
- CSRF / origin allowlist — [4,26]
- Cypher / label-injection guards — [10]
- Air-gapped offline operation — [7,15,24]

## 20. Observability & Evaluation

- Benchmark harnesses (LoCoMo, LongMemEval, BEAM, ConvoMem) — [2,6,13,16,17,19,22,23,24]
- Retrieval eval (Hit@k, MRR, NDCG, Recall@k) — [2,3,4,8,16]
- Agent eval / workflow benchmarks + fixtures — [5,7]
- Prometheus / StatsD / OpenTelemetry metrics — [2,10,13,26]
- Traces / receipts / operation state machine — [8,22]
- Doctor / health / diagnostics commands — [4,13,21,22,24]
- Usage-report / coach (no model) — [9]
- Coverage metrics over time — [12]
- Provenance + confidence on every result — [8,22]

## 21. Extensibility (plugins / providers / custom)

- Pluggable LLM / embedder / reranker registries — [2,3,4,6,10,13,15,17,22,24]
- Plugin / entry-point loader (MemOS plugins, Serena modes) — [2,20,22]
- Custom languages / frameworks / grammars — [1,7,12,15,16,25]
- Custom entity/edge types (Pydantic) — [10]
- Custom query packs / extractors — [12,15,25]
- Pipeline plugins (data formats) — [15]
- Provider adapters / connectors (Drive, Gmail, Notion…) — [21,22,23]
- Composable YAML config fragments — [20]
- Config-driven behavior (TOML/YAML) — [7,8,18,20]

## 22. Ops & Deployment

- Docker / Docker Compose — [2,3,5,6,12,13,15,20,21,24,25,26]
- Helm / Kubernetes — [2,13,25]
- Bare-metal / pip / uv install — [2,6,13,14,17,19,20,22,23,24]
- One-binary / zero-config local — [7,12,16,23,24]
- Managed SaaS / Cloud option — [13,22,23,24]
- Self-managed / BYOC / offline air-gapped — [15,22,24]
- Hot-upgrade / in-place server upgrade — [8,22]
- Export / import / backup (portable archives) — [3,19,22]
- Auto-update / self-upgrade — [7,16]
- Mirror / sync upstream repos before index — [26]
- SLM-Mesh peer coordination (auth, inbox/outbox, locks) — [22]

---

## Repo → categories present (coverage matrix)

*Derived from the `[tags]` on each capability atom above (the source of truth), not hand-estimated. A repo "covers" a domain if it is tagged on ≥1 atom in that domain's section.*

| # | Repo | Domains covered |
|---|------|-----------------|
| 1 | Understand-Anything | 1,2,3,4,5,7,15,17,18,21 |
| 2 | MemOS | 1,2,3,4,5,6,7,10,11,13,14,15,16,19,20,21,22 |
| 3 | octocode | 1,2,3,4,5,6,7,8,14,15,17,20,21,22 |
| 4 | codebase-graph | 1,2,3,4,5,6,7,8,9,11,12,14,15,16,17,18,19,20,21 |
| 5 | GitNexus | 1,2,3,4,5,6,7,8,9,13,14,15,17,19,20,22 |
| 6 | caura | 1,3,4,5,6,7,10,11,12,13,14,15,16,19,20,21,22 |
| 7 | codegraph (colbymchenry) | 1,2,3,4,5,6,7,8,9,14,15,17,19,20,21,22 |
| 8 | rag-rat | 1,5,6,7,8,9,10,11,12,14,15,17,19,20,21,22 |
| 9 | emulo | 1,11,12,17,19,20 |
| 10 | graphiti | 1,3,4,5,7,10,11,12,13,14,15,16,19,20,21 |
| 11 | graph-memory-starter | 1,2,3,4,5,6,7,10,14 |
| 12 | codeql | 9,17,18,20,21,22 |
| 13 | hindsight | 1,3,5,6,7,10,11,12,13,14,15,16,17,18,19,20,21,22 |
| 14 | honcho | 1,7,10,13,14,15,17,22 |
| 15 | infigraph | 1,2,3,4,5,6,7,8,9,13,15,16,17,18,19,21,22 |
| 16 | sense | 1,2,3,4,5,6,7,8,9,15,17,18,20,21,22 |
| 17 | mengram | 1,4,5,10,11,13,15,16,17,20,21,22 |
| 18 | mnemon | 4,11,13,15,16,17,21 |
| 19 | mnemosyne | 3,4,7,10,11,12,13,14,15,17,20,22 |
| 20 | serena | 1,3,4,6,8,15,17,21,22 |
| 21 | potpie | 1,13,17,18,20,21,22 |
| 22 | superlocalmemory | 1,3,4,5,6,7,10,11,12,13,14,15,16,18,19,20,21,22 |
| 23 | supermemory | 1,2,5,7,10,11,13,15,16,17,18,20,21,22 |
| 24 | OpenViking | 1,4,7,10,11,13,14,15,16,17,18,19,20,21,22 |
| 25 | joern | 1,2,4,9,14,15,21,22 |
| 26 | opengrok | 1,2,3,6,9,14,15,19,20,22 |

*Domain legend: 1 Ingestion · 2 Chunking · 3 Symbol/Entity Extraction · 4 Graph Construction · 5 Embeddings/Vector · 6 Keyword/Lexical · 7 Hybrid/Ranked Retrieval · 8 Code Navigation · 9 Code Analysis · 10 Memory Model · 11 Memory Lifecycle · 12 Temporal · 13 Multi-tenant/Scope · 14 Storage · 15 APIs/Protocols · 16 LLM/Agent · 17 IDE/CI · 18 Web UI · 19 Security · 20 Observability/Eval · 21 Extensibility · 22 Ops/Deployment*

---

## Best-in-class per feature (winner map)

For every feature atom listed above, the **single repo that implements it best**, with the concrete implementation evidence and the reason for selection. Tags reference the legend. Where a feature is unique to one repo, that repo wins by default; otherwise the pick favors depth of implementation, distinctiveness, and documented maturity.

### 1. Ingestion & Indexing
- Index a codebase into a persistent graph/vector store → **★ #15 infigraph** — tree-sitter + ANTLR parse of 62 languages into a Kuzu graph with Cypher, one-time <1 min index, fully offline — broadest + fastest language coverage.
- Incremental re-indexing → **★ #8 rag-rat** — content-hash fingerprints + worktree overlays for dirty files + `heal_index`/`reconcile` — keeps graph provably consistent with live source.
- FS-watcher auto-reindex (debounced) → **★ #7 codegraph** — native OS watchers (FSEvents/inotify/ReadDirectoryChangesW) + connect-time catch-up — OS-native, no polling.
- Git history / commit / PR / branch ingestion → **★ #8 rag-rat** — per-path/symbol git history, `distill` turns merged PRs into decision records — deepest source-history reasoning.
- GitHub PR + issue + review ingestion → **★ #21 potpie** — GitHub/Linear/Jira/Confluence connectors index PRs, reviews, issues — only one with full SDLC ingestion.
- Documents/files/URLs/raw-text ingestion → **★ #23 supermemory** — multi-modal extractors (PDF/image-OCR/video-transcription/code) + connectors — most input formats.
- Multi-modal ingestion → **★ #23 supermemory** — images (OCR), videos (transcription), PDF/DOCX — richest modality support.
- Conversation/chat transcript ingestion + entity extraction → **★ #14 honcho** — peer/workspace model with background reasoning producing conclusions + representations — reasoning-first, not just chunking.
- Coding-agent session-log mining into a profile → **★ #9 emulo** — mines 5 agents' logs into `you.md` with dated verbatim receipts, PII-redacted — unique, purpose-built.
- 1-click/zero-config auto-ingest from agent history → **★ #13 hindsight** — coding-agents package auto-builds per-repo bank from git history + 60+ integrations — widest zero-config coverage.
- Wiki/KB ingestion (Confluence/Notion/Google Docs/Figma) → **★ #21 potpie** — Confluence/Notion connectors — only repo covering enterprise wikis.
- Project structural scan + language/framework detection → **★ #5 GitNexus** — per-language + framework rule files, Spring/HTTP/gRPC/Thrift detection — most framework awareness.
- Scope indexing to subdir / exclude globs → **★ #15 infigraph** — `.infigraph/structured-schemas/` + exclude config — most explicit scoping controls.
- Per-branch / per-repo delta indexes → **★ #3 octocode** — per-branch delta indexes so branch search doesn't duplicate main — cleanest branch isolation.
- Multi-repo federation / cross-repo indexing → **★ #5 GitNexus** — Contract Registry + cross-repo trace/impact across repository boundaries — explicit cross-repo model.
- Auto reindex on PR merge via webhook → **★ #15 infigraph** — documented WEBHOOK-REINDEX pipeline — built-in, secure webhook flow.
- Bulk / batched episode ingestion → **★ #10 graphiti** — `add_episode_bulk` — native batched ingest.
- Import Claude Code / ChatGPT / Obsidian history → **★ #17 mengram** — `mengram import claude-code` + ChatGPT/Obsidian — most import sources.
- Data-pipeline ingestion (dbt, Airflow) → **★ #15 infigraph** — PIPELINE_PLUGINS, runtime-extensible metadata extraction — unique pipeline support.
- OSV dependency vulnerability scanning during ingest → **★ #15 infigraph** — scans deps against OSV DB — unique at ingest time.

### 2. Parsing & Chunking
- Tree-sitter parsing (10–62 languages) → **★ #15 infigraph** — 62 languages + ANTLR grammar plugins for custom DSLs, zero config — widest + extensible.
- ANTLR / custom DSL grammar plugins → **★ #15 infigraph** — drop `.g4` + `plugin.toml`, no Rust compile — easiest DSL onboarding.
- Heading-aware Markdown chunking → **★ #11 graph-memory-starter** — chunks by `#`–`###` headings with line ranges — cleanest structural chunking.
- Character/token/sentence/region chunking → **★ #2 MemOS** — chunker factory (char/sentence/markdown/simple) — most strategies.
- AST-aware code chunking → **★ #23 supermemory** — AST-aware chunking in multi-modal extractor — purpose-built for code.
- Figma / design-file parsing → **★ #1 Understand-Anything** — Figma REST API → design graph (pages/screens/components/tokens) — only one.
- Non-code formats (Dockerfile/SQL/YAML/JSON/protobuf/env) → **★ #1 Understand-Anything** — dedicated parsers for 11 non-code formats — broadest non-code coverage.
- LLM contextual chunk descriptions at index time → **★ #3 octocode** — `contextual_descriptions` bridges query/code vocab — improves retrieval precision.
- Auto-generate project ignore/exclude rules → **★ #1 Understand-Anything** — `generate-ignore.mjs` — automatic.

### 3. Symbol / Entity Extraction
- Extract functions/classes/interfaces/types/vars/params → **★ #4 codebase-graph** — per-language tree-sitter extractors (TS/JS/Py/Go/Rust/MD + generic) — most complete symbol model.
- Extract calls/imports/inheritance/type-refs/JSX → **★ #5 GitNexus** — call/class/field/DI/import extractors + framework wiring — deepest relationship extraction.
- COBOL / mainframe extraction → **★ #5 GitNexus** — dedicated COBOL ingestion + extractor — unique legacy coverage.
- Framework wiring detection (Spring/HTTP/gRPC) → **★ #5 GitNexus** — spring/gRPC/thrift/route-constant extractors — most framework-aware.
- LLM-based entity + relation extraction with schemas → **★ #6 caura** — entity linking with exact/canonical/semantic resolution + relation inference — strongest resolution.
- Custom Pydantic entity/edge types → **★ #10 graphiti** — register custom Pydantic models for entities and edges — cleanest typed extension.
- GLiNER2 local extraction (no LLM) → **★ #10 graphiti** — GLiNER2 model with Pydantic docstrings as labels — offline extraction.
- Closed-vocabulary entity extraction → **★ #11 graph-memory-starter** — fixed PERSON/ROLE/POLICY/PROCESS/DOCUMENT prompt — deterministic.
- Aliasing / name-variant resolution → **★ #11 graph-memory-starter** — uuid5(type+normalized-name) canonical identity — ML-free collapse.
- Entity resolution (exact/canonical/semantic) → **★ #6 caura** — three-mode resolution keeping every surface form as alias — most robust.
- DI / dependency-injection extraction → **★ #5 GitNexus** — `di-extractors` — unique.
- Definition + reference resolution (ctags/LSP) → **★ #20 serena** — LSP backend over 40+ languages, find-declaration/implementations — broadest reliable resolution.

### 4. Graph Construction
- Code property graph (CPG: AST+CFG+DDG+CDG) → **★ #25 joern** — true CPG with overlays (data-flow, PDG) over flatgraph — research-grade standard.
- Knowledge graph with typed nodes + edges → **★ #10 graphiti** — 5-node/5-edge model (Episodic/Entity/Community/Saga) — richest schema.
- Bi-temporal graph (valid_at/invalid_at) → **★ #10 graphiti** — validity windows + `reference_time`, invalidates rather than deletes — canonical temporal graph.
- Call/import/inheritance/extends/implements edges → **★ #4 codebase-graph** — explicit edge types (CALLS/EXTENDS/IMPLEMENTS/USES_TYPE…) — most explicit.
- Community detection (Louvain/Leiden) + summaries → **★ #1 Understand-Anything** — graphology + Louvain community detection — documented summaries.
- Symbol graph (callers/callees) on demand → **★ #3 octocode** — live in-memory symbol graph, no index/LLM needed — fastest on-demand.
- Live in-memory vs persisted GraphRAG → **★ #3 octocode** — optional persisted GraphRAG with LLM enrichment — both modes.
- Episodic/Entity/Community/Saga node model → **★ #10 graphiti** — saga with ordered NEXT_EPISODE links — unique narrative grouping.
- Memory graph (temporal/entity/semantic/causal edges) → **★ #18 mnemon** — four-graph store (2150 edges across 4 types) — richest memory graph.
- Directed call graph + impact/process/cluster grouping → **★ #5 GitNexus** — clusters + precomputed execution flows/processes — most usable groupings.
- Export graph as Neo4j/GraphML/JSON/Cypher/DOT → **★ #25 joern** — Dot/Neo4jCsv/GraphML/Graphson interchange — most export formats.
- Configurable graph + vector projections → **★ #22 SLM** — parity-gated CozoDB graph + LanceDB vector projections (prepare→verify→promote→rollback) — safest promotion.

### 5. Embeddings & Vector Search
- Local on-device embeddings (no API key) → **★ #15 infigraph** — bundled `potion-base-8M` (29 MB), works air-gapped — zero-dependency.
- Pluggable embedding providers → **★ #13 hindsight** — 25+ providers (hosted/local/OpenAI-compatible/gateways) — widest.
- pgvector/LanceDB/Qdrant/Milvus/sqlite-vec backends → **★ #2 MemOS** — Qdrant/Milvus + Neo4j/PolarDB/Postgres factories — most swap-able.
- HNSW / IVF / RaBitQ / int8 quantization → **★ #15 infigraph** — HNSW sidecar (~2 ms / 500K symbols) + 16× faster repeat search — fastest at scale.
- Cosine / semantic similarity search → **★ #10 graphiti** — embeddings over edges/nodes/communities + cosine — tightest integration.
- Approximate nearest-neighbor at large scale → **★ #15 infigraph** — HNSW ~2 ms for 500K symbols — demonstrated scale.
- Multilingual embeddings (23+ languages) → **★ #17 mengram** — Cohere multilingual + rerank, 23 languages — explicit multilingual.
- Vision / OCR embedding → **★ #23 supermemory** — image OCR + video transcription embeddings — unique multimodal.
- Multi-vector per repo (code/text/docs/commits/graph) → **★ #3 octocode** — LanceDB tables for code/text/docs/commits/graph nodes — most granular.

### 6. Keyword / Lexical Search
- BM25 full-text search → **★ #15 infigraph** — BM25 disk cache + binary HNSW sidecar, 16× faster repeat MCP/CLI searches — fastest BM25.
- FTS5 (SQLite) search → **★ #11 graph-memory-starter** — SQLite FTS5 virtual table with stopword removal — minimal + effective.
- CJK / multi-script segmentation → **★ #5 GitNexus** — CJK segmentation in BM25/FTS — unique.
- ripgrep / regex pattern fallback → **★ #16 sense** — ripgrep text fallback when structural search can't help — honest fallback.
- Commit / history / annotation search → **★ #26 opengrok** — 15 SCM backends + per-line blame/annotation — deepest history search.
- Git blame per chunk / symbol → **★ #8 rag-rat** — `git_blame_chunk` ties memories to blame — provenance-rich.

### 7. Hybrid & Ranked Retrieval
- Vector + keyword (RRF fusion) → **★ #13 hindsight** — 4-strategy parallel recall (semantic/BM25/graph/temporal) + RRF + cross-encoder rerank — best-measured fusion.
- Graph + semantic + temporal + lexical multi-channel → **★ #22 SLM** — 5 candidate channels (semantic/BM25/temporal/Hopfield/spreading-activation) + graph score — most channels.
- Hopfield / spreading-activation associative recall → **★ #22 SLM** — Hopfield + spreading-activation layers — unique associative recall.
- Re-ranking (cross-encoder / LLM / BGE / Cohere / local) → **★ #13 hindsight** — cross-encoder reranker in recall pipeline — benchmark-backed.
- Query-shape auto-weighting / adaptive budget → **★ #3 octocode** — query-shape auto-weighting of hybrid RRF — adaptive.
- Graph-expansion of candidates before rerank → **★ #3 octocode** — `graph_expansion` pulls related files before rerank — improves recall.
- Dual-retrieval consensus scoring → **★ #4 codebase-graph** — `consensus.ts` dual-retrieve + computeConsensus — explicit consensus.
- RAG + memory hybrid query (one call) → **★ #23 supermemory** — `searchMode: hybrid` returns KB docs + personal memory together — cleanest one-call.
- Directory-recursive retrieval → **★ #24 OpenViking** — vector-locates highest-scoring directory then drills L0→L2 — unique filesystem metaphor.
- Temporal / recency boosting → **★ #13 hindsight** — temporal retrieval strategy + recency filtering — measured.
- Source-anchored provenance + confidence per result → **★ #8 rag-rat** — per-edge confidence tiers + provenance + `explain` — most transparent.

### 8. Code Navigation (LSP / refs / defs)
- goto-definition / find-references / hover / symbols → **★ #20 serena** — LSP backend (40+ langs) + JetBrains plugin — broadest reliable.
- Find callers / callees → **★ #7 codegraph** — `codegraph_callers`/`callees` with overload grouping — clean.
- Trace shortest call / dependency path between symbols → **★ #5 GitNexus** — `trace` shortest directed path — unique.
- Multi-file coordinated rename → **★ #20 serena** — rename symbols/files/directories atomically across graph + text — IDE-grade.
- Impact / blast-radius from a symbol → **★ #15 infigraph** — `impact` + affected-tests + PR review — most complete.
- Dead-code detection → **★ #15 infigraph** — dead-code analysis ranked by impact/effort — most actionable.
- Cross-language FFI / bridge resolution → **★ #15 infigraph** — Delphi↔COM, C#↔JNI, gRPC, WASM bridges — unique.
- Diagnostics / inspections → **★ #20 serena** — LSP diagnostics + JetBrains inspections — deepest.
- Interactive debugging (breakpoints, REPL) → **★ #20 serena** — persistent REPL debugging via JetBrains plugin — unique.

### 9. Code Analysis (static / dataflow / taint)
- Interprocedural taint / dataflow tracking → **★ #25 joern** — `sink.reachableBy(source)` with semantic models + depth control — research-grade.
- Source→sink vulnerability paths → **★ #25 joern** — path queries showing full exploit path — clearest.
- Control-/data-dependence (PDG) queries → **★ #5 GitNexus** — `pdg_query` over statement-level PDG — unique.
- API route ↔ component mapping + shape checks → **★ #5 GitNexus** — `route_map`/`shape_check`/`api_impact` — unique.
- MCP/RPC tool-definition discovery → **★ #5 GitNexus** — `tool_map` finds MCP/RPC tools + handlers — unique.
- Security query packs (CWE-classified) → **★ #12 codeql** — per-language CWE-classified query suites — industry standard.
- QL / CPGQL / Cypher / custom query languages → **★ #12 codeql** — QL declarative logic language (types/predicates/recursion) — most expressive.
- Custom extractors / query packs for new languages → **★ #12 codeql** — tree-sitter extractor framework + create-extractor-pack — most documented.
- Design-pattern detection → **★ #15 infigraph** — Singleton/Factory/Observer/Strategy/Builder detection — unique.
- Complexity / coupling / hotspot analysis → **★ #15 infigraph** — complexity hotspots + coupling — most complete.
- Near-duplicate / clone detection → **★ #8 rag-rat** — `find_clones` ranked by refactor ROI — unique + prioritized.
- Refactor analysis + affected tests → **★ #15 infigraph** — `review` surfaces affected tests for changed code — most actionable.
- PR-type classification + CI gating → **★ #15 infigraph** — auto-detects PR type + configurable CI check gates — unique.
- LLM/AI SDK taint sources/sinks → **★ #12 codeql** — `anthropic.model.yml`/`google-genai.model.yml` model AI SDKs as sources/sinks — unique.
- Multi-repo variant analysis (MRVA) → **★ #12 codeql** — MRVA across up to 1000 repos from VS Code — unique scale.

### 10. Memory Model & Types
- Episodic memory → **★ #13 hindsight** — "experiences" pathway with temporal data — SOTA-measured.
- Semantic memory → **★ #13 hindsight** — "world facts" pathway, multilingual preservation — best.
- Procedural memory (workflows that evolve) → **★ #17 mengram** — procedures evolve across failures with recorded violated assumptions — unique.
- World facts / experiences / observations / mental models → **★ #13 hindsight** — biomimetic four-type model + observations with proof count — richest.
- Parametric / activation (KV-cache) / plaintext memory → **★ #2 MemOS** — unique support for LoRA weights + reusable KV-cache — only one.
- Working / long-term / user / outer memory tiers → **★ #2 MemOS** — WorkingMemory/LongTerm/User/Outer + Skill/Preference — most tiers.
- Typed memories (14–15 classes) → **★ #6 caura** — 14 typed memories (fact/episode/decision/preference/task/intention…) — most explicit typing.
- MemCube / bank isolation → **★ #2 MemOS** — MemCube + composite/single views + share/dump/load — most flexible.
- Knowledge pages / mental models (DB-read, no LLM) → **★ #13 hindsight** — knowledge pages are plain DB reads, zero retrieval/LLM — fastest cold start.
- Cognitive profile (system prompt from memory) → **★ #17 mengram** — `get_profile()` one call → system prompt — unique.
- User profiles (static + dynamic activity) → **★ #23 supermemory** — `profile.static`/`profile.dynamic`, ~50 ms — cleanest.
- Skills (evolving, versioned) → **★ #6 caura** — Skills Inbox with candidate→staged→active lifecycle — governed.
- Memory types preference/requirement/procedure/person… → **★ #10 graphiti** — 10 default entity types (Preference/Requirement/Procedure…) — explicit ontology.
- Source-anchored repo memories (Invariant/Decision/Risk…) → **★ #8 rag-rat** — 15 `MemoryKind`s each bound to symbol/chunk/path/edge/commit — most rigorous anchoring.
- Three-layer context (L0/L1/L2) loading → **★ #24 OpenViking** — abstract/overview/details tiered on-demand loading — unique token-saving design.
- Peer model (users/agents/groups/projects as entities) → **★ #14 honcho** — workspace/peer/session/scope with cross-peer observation — richest relational model.

### 11. Memory Lifecycle (consolidation / decay / forgetting / promotion)
- Consolidation / deduplication / merge → **★ #13 hindsight** — observations refined (not overwritten) with proof count + evidence — most evidence-backed.
- Decay / forgetting / self-archive → **★ #13 hindsight** — observations weaken/extend over time — measured.
- Contradiction detection + supersede (not delete) → **★ #10 graphiti** — facts invalidated, temporal history preserved — canonical.
- Crystallization into canonical facts → **★ #6 caura** — crystallizer retires stale rows into canonical facts on cron — unique.
- Importance / confidence weighting + decay → **★ #18 mnemon** — importance-decay on four-graph store — explicit.
- Auto memory extraction from sessions in background → **★ #13 hindsight** — `retain` LLM-extracts facts/entities/time, normalize to canonical — most automatic.
- "Dream" maintenance loop → **★ #2 MemOS** — Dream pipeline builds context nodes, motives, insights, human-readable diary — most developed.
- Feedback-driven reinforcement / correction → **★ #2 MemOS** — feedback handler refines/corrects/supplements/replaces memories — explicit.
- Promotion/demotion of memory tiers → **★ #2 MemOS** — scheduler compresses/forgets/type-converts across tiers — unique.
- Expiry / TTL on memories → **★ #19 mnemosyne** — `valid_until` expiry on `remember` — clean.
- Redaction / forget PII before storage → **★ #9 emulo** — redacts secrets/PII (API keys, JWT, emails, IPs) before any write — most thorough.
- Skill evolution from failures → **★ #17 mengram** — procedure_feedback drives versioned evolution — unique.
- Procedural self-improvement on failure → **★ #17 mengram** — records violated assumption + precondition per failure — unique.

### 12. Temporal Reasoning
- Bi-temporal facts (ingest + reference time) → **★ #10 graphiti** — `valid_at`/`invalid_at` + `reference_time` — canonical.
- Recall "as of" a point in time → **★ #10 graphiti** — query truth at any timestamp — unique.
- Recency / time-range filtering → **★ #13 hindsight** — temporal retrieval strategy — measured.
- Git history reasoning (per-path/symbol/commit) → **★ #8 rag-rat** — `git_history_for_path`/`git_history_for_symbol`/`commits_touching_query` — deepest.
- Decision records from issues/PRs → **★ #8 rag-rat** — `distill` extracts root cause/approach/rejected alternatives from merged PRs — unique.
- Scene / entity timelines → **★ #22 SLM** — scene + entity timelines from lifecycle — explicit.
- Event timestamps preserved end-to-end → **★ #9 emulo** — every rule carries dated verbatim receipts — strongest provenance.

### 13. Multi-tenant / Scoping / Namespaces
- Per-user / per-agent / per-project isolation → **★ #22 SLM** — personal/shared/global scopes with default-deny cross-profile recall — strictest.
- Personal / shared / global memory scopes → **★ #22 SLM** — explicit scope policy / per-call opt-in — clearest.
- Group_id partitioning of graph → **★ #10 graphiti** — `group_id` scopes ingestion/search/communities/deletion — native.
- Workspace / tenant isolation → **★ #14 honcho** — workspace top-level container isolating data — clean.
- Cross-repo namespaces + links → **★ #5 GitNexus** — Contract Registry + cross-repo trace/impact — explicit.
- Role-based access / trust tiers → **★ #6 caura** — four agent trust tiers gating cross-fleet reads/writes/deletes — unique.
- Peers: who-knows-whom modeling → **★ #14 honcho** — per-peer representation of what one peer knows about another — unique.

### 14. Persistence & Storage Backends
- SQLite (incl. sqlite-vec, FTS5) → **★ #19 mnemosyne** — SQLite + BEAM (working/episodic/triplestore), flat storage growth — most SQLite-native.
- PostgreSQL + pgvector → **★ #13 hindsight** — pgvector or Oracle AI DB 23ai, full parity — production-grade.
- Neo4j / FalkorDB / Kuzu / PolarDB → **★ #10 graphiti** — Neo4j/FalkorDB/Kuzu(embedded)/Neptune + single-process FalkorDB container — most backends.
- Qdrant / Milvus → **★ #2 MemOS** — Qdrant/Milvus factories — most vector backends.
- Oracle AI DB 23ai → **★ #13 hindsight** — enterprise Oracle backend, full feature parity — unique.
- LadybugDB / WASM in-browser → **★ #5 GitNexus** — LadybugDB native CLI + WASM in web — unique.
- LanceDB (Arrow) → **★ #3 octocode** — LanceDB tables for all index kinds — most Arrow-native.
- CozoDB graph projection → **★ #22 SLM** — parity-gated CozoDB projection with rollback — safest.
- Redis (cache / queue) → **★ #2 MemOS** — Redis Streams queue (prod) / local queue (dev) — documented.
- Flatgraph / overflowdb graph store → **★ #25 joern** — flatgraph migration, query-compatible — research-grade.
- Lucene index (disk) → **★ #26 opengrok** — Apache Lucene index with 40+ format analyzers — mature.
- VikingDB (managed) → **★ #24 OpenViking** — VikingDB managed scale — unique.

### 15. APIs & Protocols (MCP / REST / CLI / LSP / gRPC)
- MCP server (stdio / SSE / Streamable HTTP) → **★ #22 SLM** — all transports + `core`/`code`/`mesh`/`full`/`power`/`whole` profiles — most complete.
- Remote / HTTP MCP with auth → **★ #22 SLM** — HTTP/stdio MCP with API-key + bearer auth — most secured.
- MCP tools count → **★ #22 SLM** — up to 94 tools (`whole`) — most.
- REST API → **★ #6 caura** — ~90-endpoint REST (memories/search/agents/fleets/audit/lifecycle/reports) — broadest.
- CLI (rich subcommands) → **★ #15 infigraph** — 80+ CLI options + 69–82 MCP tools — most complete CLI.
- Python / TypeScript / Go SDK → **★ #23 supermemory** — npm + pip + framework wrappers (Vercel/LangChain/Mastra/Agno/n8n) — most SDK surface.
- LSP server integration → **★ #20 serena** — LSP backend (40+ langs) + JetBrains plugin — deepest.
- OpenAPI client / Swagger → **★ #26 opengrok** — ~40-endpoint OpenAPI spec — most documented.
- Webhooks (lifecycle / PR reindex) → **★ #15 infigraph** — GitHub webhook auto-reindex pipeline — unique.
- TOON / JSON output formats → **★ #8 rag-rat** — TOON (Token-Oriented Object Notation) default + `--json` — unique token-efficient format.

### 16. LLM & Agent Integration
- 25+ / many LLM providers → **★ #13 hindsight** — 25+ providers incl. local/gateways + existing subscriptions — widest.
- LLM-wrapper that auto stores/retrieves (no code change) → **★ #13 hindsight** — `wrap_openai`/`wrap_anthropic` auto recall+retain — cleanest.
- LangChain / LangGraph / LlamaIndex / CrewAI / Pydantic-AI / OpenAI Agents / Agno / Vercel AI SDK / Mastra / n8n → **★ #13 hindsight** — 60+ integrations covering all of these — broadest.
- Agent frameworks (AutoGen, Microsoft Agent, Google ADK, Strands, Haystack) → **★ #13 hindsight** — listed integrations — unique coverage.
- Deepsearch / query-rewrite agent → **★ #2 MemOS** — deepsearch agent + query-goal parser — explicit.
- Reflect / reasoning pass over memories → **★ #13 hindsight** — `reflect` builds new connections / deep answers — measured.
- LLM supervision of memory decisions (host LLM as judge) → **★ #18 mnemon** — LLM-supervised pattern (host LLM makes judgment calls) — unique protocol.
- Multi-agent orchestration → **★ #24 OpenViking** — VikingBot framework + Helper console — built-in.

### 17. IDE / Editor / Chat / CI Integration
- Claude Code → **★ #13 hindsight** — auto-installs coding-agents package for 12+ agents incl. Claude Code — broadest.
- Cursor → **★ #13 hindsight** — same auto-install — broadest.
- Codex → **★ #13 hindsight** — same — broadest.
- OpenCode → **★ #13 hindsight** — same — broadest.
- Windsurf / Cline / Kiro / Gemini / Antigravity / others → **★ #13 hindsight** — 60+ integrations — broadest.
- OpenClaw / Hermes → **★ #13 hindsight** — native plugins — broadest.
- GitHub Copilot → **★ #5 GitNexus** — Copilot CLI integration + api_impact — unique.
- VS Code / JetBrains / web clients → **★ #20 serena** — LSP + JetBrains plugin for all JetBrains IDEs — deepest IDE integration.
- Pi → **★ #24 OpenViking** — native pi extension + skill — unique.
- ZCode / MiniMax / TRAE / Qoder / CodeBuddy / WorkBuddy / Kimi → **★ #18 mnemon** — setup targets for all of these — widest exotic-agent coverage.
- CI integration (agent test selection, gates) → **★ #15 infigraph** — `affected` drives CI test selection + configurable CI check gates — most complete.
- PreToolUse / PostToolUse / SessionStart hooks → **★ #13 hindsight** — coding-agents hooks auto-inject bank at session start — broadest.

### 18. Web UI & Visualization
- Interactive graph / knowledge-graph explorer → **★ #15 infigraph** — built-in graph explorer at :9749 + web UI — richest.
- Dashboard (health, memories, brain, entities) → **★ #22 SLM** — multi-tab dashboard (Brain, KG, Entity Explorer, Mesh, Optimize) — most operational.
- Cold-start codebase summary map → **★ #16 sense** — `.sense/summary.md` with hub symbols/entry points/conventions — unique cold-start map.
- Sequence diagrams from call graphs → **★ #15 infigraph** — auto Mermaid sequence diagrams — unique.
- MCP graph-explorer UI resource → **★ #4 codebase-graph** — graphExplorer UI resource — unique.
- Studio playground (hosted) → **★ #24 OpenViking** — browser Studio with semantic search + multi-agent hub — unique.
- Query-help docs / benchmarks UI → **★ #12 codeql** — generated query-help documentation — unique.
- Desktop helper console → **★ #24 OpenViking** — OpenViking Helper desktop app (macOS/Windows) — unique.

### 19. Security & Access Control
- API-key auth (hashed, scoped, rotatable) → **★ #2 MemOS** — hashed scoped keys + admin create/list/revoke + master-key rotation — most complete.
- Rate limiting → **★ #2 MemOS** — middleware rate-limit — explicit.
- PII / secret redaction & blocking → **★ #9 emulo** — redacts 45+ secret/PII classes before any text written — most thorough.
- Read-only mode for MCP / source → **★ #8 rag-rat** — MCP read-only over source; only SQLite index written — strictest.
- Repo allowlists / policy → **★ #5 GitNexus** — repo allowlists + default-repo policy — explicit.
- LDAP / group / user authorization plugins → **★ #26 opengrok** — LDAP attr/filter/user/group plugins + webhook authz — most enterprise.
- GDPR / EU AI Act retention + audit chain → **★ #22 SLM** — explicit GDPR posture, retention policies, hash-chained audit — most explicit.
- Hash-chained tamper-evident audit log → **★ #6 caura** — audit hash-chain migration — unique.
- CSRF / origin allowlist → **★ #26 opengrok** — CSRF Origin checking + cookie/response filters — most web-secure.
- Cypher / label-injection guards → **★ #10 graphiti** — node-label whitelist validation — unique graph-injection guard.
- Air-gapped offline operation → **★ #15 infigraph** — offline-first, no network calls, bundled model — proven air-gapped.

### 20. Observability & Evaluation
- Benchmark harnesses (LoCoMo, LongMemEval, BEAM, ConvoMem) → **★ #13 hindsight** — SOTA LongMemEval + live continuously-updated benchmarks — most credible.
- Retrieval eval (Hit@k, MRR, NDCG, Recall@k) → **★ #8 rag-rat** — commit-replay eval reporting recall@3/10 + MRR@10 as CI gate — most rigorous.
- Agent eval / workflow benchmarks + fixtures → **★ #5 GitNexus** — eval harness with workflow benchmarks + ground-truth fixtures — most complete.
- Prometheus / StatsD / OpenTelemetry metrics → **★ #10 graphiti** — OpenTelemetry distributed traces — most tracing.
- Traces / receipts / operation state machine → **★ #22 SLM** — durable raw→queryable→enriching→complete state machine with receipts — most transparent.
- Doctor / health / diagnostics commands → **★ #22 SLM** — `doctor`/`health`/`trace` across surfaces — most operational.
- Usage-report / coach (no model) → **★ #9 emulo** — model-free `--coach` flags repeat sends/reword loops with receipts — unique.
- Coverage metrics over time → **★ #12 codeql** — CSV coverage metrics over time — unique.
- Provenance + confidence on every result → **★ #8 rag-rat** — confidence tiers + source provenance + `explain` — most transparent.

### 21. Extensibility (plugins / providers / custom)
- Pluggable LLM / embedder / reranker registries → **★ #13 hindsight** — 25+ LLM + multiple embedder/reranker providers — widest.
- Plugin / entry-point loader → **★ #2 MemOS** — community plugins via entry points + MEMOS_ENABLED_PLUGINS + hook defs — most developed.
- Custom languages / frameworks / grammars → **★ #15 infigraph** — 62 languages + ANTLR `.g4` plugins + grammar registry — widest + easiest.
- Custom entity/edge types (Pydantic) → **★ #10 graphiti** — register custom Pydantic models — cleanest.
- Custom query packs / extractors → **★ #12 codeql** — packs/query-suites/libraries + create-extractor-pack — most documented.
- Pipeline plugins (data formats) → **★ #15 infigraph** — PIPELINE_PLUGINS for dbt/Airflow — unique.
- Provider adapters / connectors (Drive, Gmail, Notion…) → **★ #23 supermemory** — Google Drive/Gmail/Notion/OneDrive/GitHub real-time webhooks — most connectors.
- Composable YAML config fragments → **★ #20 serena** — multi-level composable YAML (global/project/mode) — most flexible.
- Config-driven behavior (TOML/YAML) → **★ #8 rag-rat** — `rag-rat.toml` controls indexing/watch/oracle/memory/LLM — most comprehensive.

### 22. Ops & Deployment
- Docker / Docker Compose → **★ #13 hindsight** — Docker + docker-compose + scripted installs — broadest.
- Helm / Kubernetes → **★ #13 hindsight** — Helm chart with pgvector enabled — production.
- Bare-metal / pip / uv install → **★ #13 hindsight** — pip/`hindsight-all` embedded + Helm + Cloud — most options.
- One-binary / zero-config local → **★ #23 supermemory** — `curl | bash` one binary, zero config, offline Ollama — simplest.
- Managed SaaS / Cloud option → **★ #13 hindsight** — Hindsight Cloud with dashboard/backups/SLA — most managed.
- Self-managed / BYOC / offline air-gapped → **★ #15 infigraph** — offline-first, no Docker/Python/Node required — proven air-gapped.
- Hot-upgrade / in-place server upgrade → **★ #8 rag-rat** — `upgrade.rs` hot-upgrades running MCP server — unique.
- Export / import / backup (portable archives) → **★ #3 octocode** — `export`/`import` compressed portable datasets — cleanest.
- Auto-update / self-upgrade → **★ #16 sense** — `sense upgrade` self-upgrades in place — unique.
- Mirror / sync upstream repos before index → **★ #26 opengrok** — mirror.py/sync.py before indexing — unique.
- SLM-Mesh peer coordination → **★ #22 SLM** — authenticated peer messages/inbox/outbox/locks/discovery — unique.

---

## Notes & caveats

- **Memory vs code-intelligence split:** repos #10–#24 are agent-memory / knowledge systems; #1–#9, #15, #25, #26 are code-knowledge / static-analysis engines; #11, #12 are special-purpose (minimal RAG, QL query lib). The union deliberately mixes both — that's the requested "sum of features."
- **Capability counts are approximate.** "N MCP tools" figures come from each repo's own README and may be version-specific.
- **No repos were modified.** The task was interpreted as producing the consolidated union list, not patching the external checkouts.
- **Largest coverage:** #22 (superlocalmemory), #15 (infigraph), #5 (GitNexus), #4 (codebase-graph) and #8 (rag-rat) touch the most domains; pure analyzers (#12 codeql, #25 joern, #26 opengrok) add the deepest *code analysis* domain that the memory tools mostly lack.
