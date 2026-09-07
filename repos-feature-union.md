# Repos Feature Union (26 repos — code-only extraction)

Source: code-only extraction across 26 repos under the network share (no README/docs/prose consulted).

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
| 14 | plastic-labs/honcho | | |

## Semantic buckets (orientation)

The 22 domains group into five buckets: Ingestion & Representation (1-4) covers getting code/memories in, parsing, extracting symbols, and graph construction; Retrieval & Search (5-7) covers embeddings, lexical, and hybrid ranked retrieval; Code Intelligence (8-9) covers LSP navigation and static/taint analysis; Agent Memory (10-13) covers memory models, lifecycle, temporal reasoning, and multi-tenant scoping; and Platform & Ops (14-22) covers storage, APIs, LLM integration, IDE/UI, security, observability, extensibility, and deployment — bridging the memory-systems and code-intelligence halves of the union.

## 1. Ingestion & Indexing

- Index a codebase into a persistent graph / vector store — [1,3,4,5,7,8,15,16,21,24,25,26] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("index_project")
- Ingest documents, memories, or conversation files — [2,6,9,10,11,13,14,15,17,18,19,21,22,23,24] — MemTensor__MemOS/src/memos/api/mcp_serve.py:add_memory
- Watch / auto-reindex on file change — [3,5,7,15,26] — Muvon__octocode/src/commands/watch.rs:execute
- Differential / incremental / branch-delta indexing — [3,5,7,16] — Muvon__octocode/src/indexer/differential_processor.rs
- Index git / VCS commit history — [3,4,8,26] — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/history/GitRepository.java:GitRepository
- Generate ignore / scope filters to bound indexing — [1] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/ignore-filter.ts:createIgnoreFilter
- Import external indexes / connectors (SCIP, GitHub, Notion, other memory systems) — [15,19,21,24] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("scip_import")
- Emit a starter ignore/scope config file — [1] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/ignore-generator.ts:generateStarterIgnoreFile
- Reindex / backfill content on demand — [4,17,24] — openviking/ingest/orchestrator.py:IngestOrchestrator.backfill

## 2. Parsing & Chunking

- Parse source code via tree-sitter across many languages — [1,3,4,5,7,15,16,25,26] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/plugins/tree-sitter-plugin.ts:TreeSitterPlugin
- Parse documents via pluggable parsers (Markdown/YAML/JSON/code) — [1,2,6,11,13,24] — MemTensor__MemOS/src/memos/parsers/markitdown.py:MarkItDownParser
- Chunk content into nodes/chunks for embedding — [2,5,6,8,11,13,17] — MemTensor__MemOS/src/memos/chunkers/factory.py
- Extract atomic facts / structured entities from text — [9,22] — emulo.py:mine_files
- Register pluggable language / format analyzer plugins — [4,24,26] — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/analysis/AnalyzerGuru.java:AnalyzerGuru
- Parse repositories into a structured context graph — [4,21] — Phoenixrr2113__codebase-graph/packages/core/src/documentIngestion.ts:add

## 3. Symbol/Entity Extraction

- Extract symbols/functions/classes via tree-sitter / LSP — [1,3,4,5,7,15,16,25,26] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("get_symbols_in_file")
- Extract entities + relationships from text via LLM — [10,13,17,24] — getzep__graphiti/graphiti_core/graphiti.py:add_episode
- Extract behavioral assertions / facts from tool usage / sessions — [9,18,22] — mnemon/internal/memory/graph/entity.go:ExtractEntities
- Resolve symbol references / callers / callees (360° reference resolution) — [5,8,15,20] — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:context
- Detect HTTP routes / API surface / endpoints — [5,15] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("detect_routes")
- Regex / similarity entity extraction — [6,19] — mnemosyne-oss__mnemosyne/mnemosyne/core/entities.py:extract_entities_regex

## 4. Graph Construction

- Build a code/entity knowledge graph — [1,3,4,5,7,15,16,21,22,23,25] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/graph-builder.ts:GraphBuilder
- Persist memory as a property graph (Neo4j / Kuzu / FalkorDB / Cozo) — [2,10,15,21,23] — MemTensor__MemOS/src/memos/graph_dbs/factory.py
- Build entity/relation/triple graph from text — [6,11,13,17,18,19] — mnemosyne-oss__mnemosyne/mnemosyne/core/triples.py:TripleStore
- Reference / call graph between symbols — [8,20] — oraios__serena/src/serena/symbol.py:ReferenceInLanguageServerSymbol
- Detect communities / relation reasoning over graph — [2,5,22] — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:cypher
- Query graph via Cypher / traversal API — [4,15,25] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("query_graph")
- Compose layered Code Property Graph (AST / CFG / DDG / CDG) — [25] — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/DefaultOverlays.scala:DefaultOverlays.create
- Pluggable graph DB driver (FalkorDB / Neo4j / Memgraph / LanceDB) — [4] — Phoenixrr2113__codebase-graph/packages/graph/src/driver-registry.ts:registerDriver
- Export graph (graphml / json / cypher / DOT) — [15,25] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("export_graph")
- VikingDB vector+graph index management — [24] — openviking/storage/vikingdb_manager.py:VikingDBManager
- Detect relations and reasoning between memory nodes — [2] — MemTensor__MemOS/src/memos/memories/textual/tree_text_memory/organize/relation_reason_detector.py:RelationAndReasoningDetector

## 5. Embeddings & Vector Search

- Generate embeddings via pluggable providers (OpenAI / Ollama / Sentence-Transformers / etc.) — [1,2,3,6,8,10,11,13,14,15,16,17,18,19,21,22,23,24,25] — MemTensor__MemOS/src/memos/embedders/factory.py
- Semantic / vector similarity search over nodes/memories — [1,3,4,5,8,10,11,13,15,16,17,19,22,23,24] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/embedding-search.ts:SemanticSearchEngine
- On-device / bundled embedding (FastEmbed, ONNX, feature-hash) — [8,16,25] — cq27-dev__rag-rat/crates/rag-rat-llm/src/fastembed.rs:FastEmbedEmbedder
- Cosine similarity ranking — [1,18] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/embedding-search.ts:cosineSimilarity

## 6. Keyword/Lexical Search

- BM25 full-text / lexical search over index — [1,2,5,6,7,11,15,23] — MemTensor__MemOS/src/memos/memories/textual/tree_text_memory/retrieve/bm25_util.py:EnhancedBM25
- Regex / grep / AST code search — [3,4,8,15,16,20,21,24,26] — Muvon__octocode/src/commands/grep.rs:execute
- Full-text search over indexed code via REST / API — [6,24,26] — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/SearchController.java:SearchController.search
- FTS5 full-text search (SQLite) — [7,11] — colbymchenry__codegraph/src/db/queries.ts:nodes_fts
- Lexical memory / fact search — [13,22] — src/superlocalmemory/mcp/tools_core.py:search
- List memories by container/tag — [23] — apps/mcp/src/server/tools/list-memories.ts:register
- Workspace/session/peer lexical search — [14] — honcho/src/routers/workspaces.py:search_workspace
- Token/Jaccard/CJK keyword scoring — [18] — mnemon/internal/memory/search/keyword.go:KeywordSearch
- Keyword/semantic search CLI — [16] — luuuc__sense/cmd/sense/main.go

## 7. Hybrid & Ranked Retrieval

- Reciprocal-Rank-Fusion (RRF) hybrid retrieval — [3,4,5,8,10,11,13,15,17,18,19,21,22,24,26] — abhigyanpatwari__GitNexus/gitnexus/src/core/search/hybrid-search.ts:mergeWithRRF
- Rerank with cross-encoder / MLX / Jina reranker — [2,10,13,24] — getzep__graphiti/graphiti_core/cross_encoder/
- MMR / maximal-marginal-relevance reranking — [10,19] — mnemosyne-oss__mnemosyne/mnemosyne/core/mmr.py:mmr_rerank
- Graph-enriched retrieval (graph + vector) — [4,17] — Phoenixrr2113__codebase-graph/packages/core/src/enrichedSearchV2.ts:enrichFromGraph
- Intent-aware / hierarchical retriever (vector + lexical tiers) — [18,24] — mnemon/internal/memory/search/recall.go:IntentAwareRecall

## 8. Code Navigation (LSP/refs/defs)

- LSP go-to-definition / hover / find-references / completions — [3,15,20] — Muvon__octocode/src/mcp/server.rs:lsp_goto_definition
- Trace callers / callees / impact over the graph — [1,4,5,7,8,15,16,22,25,26] — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:impact
- Find symbol / definition references via IDE (JetBrains) — [20,26] — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/FileController.java:FileController.getDefinitions
- AST / CFG traversal for navigation — [25] — joernio__joern/semanticcpg/src/main/scala/io/shiftleft/semanticcpg/language/types/expressions/generalizations/AstNodeTraversal.scala:AstNodeTraversal.ast
- Onboarding tour / heuristic walkthrough of codebase — [1] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/tour-generator.ts:generateHeuristicTour
- Filesystem ls/tree/stat over virtual URIs — [24] — openviking/server/routers/filesystem.py
- IDE local-definitions / local-references (CodeQL) — [12] — github__codeql/ql/ql/src/ide-contextual-queries/localDefinitions.ql
- Rename + symbol-body edits — [20] — oraios__serena/src/serena/tools/symbol_tools.py:RenameSymbolTool

## 9. Code Analysis (static/dataflow/taint)

- Taint / dataflow analysis (sources → sinks) — [5,12,15,25] — joernio__joern/dataflowengineoss/src/main/scala/io/joern/dataflowengineoss/language/ExtendedCfgNode.scala:ExtendedCfgNode.reachableByFlows
- Dead-code / unused-symbol detection — [4,7,15,16] — luuuc__sense/internal/dead/dead.go:FindDead
- Blast-radius / change-impact analysis — [4,8,16,22] — src/superlocalmemory/mcp/tools_code_graph.py:get_blast_radius
- Security vulnerability scan / CWE query packs — [12,15] — github__codeql/python/ql/src/Security/CWE-089/SqlInjection.ql
- Cyclomatic complexity / code metrics — [4,15] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("get_complexity")
- Clone / near-duplicate detection — [8,15] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("detect_clones")
- AI code review / diff / explain / commit — [1,3] — Muvon__octocode/src/commands/review.rs:execute
- Load-bearing symbol ranking (PageRank) — [8] — cq27-dev__rag-rat/crates/rag-rat-query/src/pagerank.rs
- Import-cycle / dependency-cycle detection — [4,5] — abhigyanpatwari__GitNexus/gitnexus/src/core/graph/import-cycles.ts
- LSP diagnostics — [20] — oraios__serena/src/serena/tools/symbol_tools.py:GetDiagnosticsForFileTool

## 10. Memory Model & Types

- Typed node/edge schema (Entity / Edge / Community / Episode) — [1,4,10,11,17,18,19] — getzep__graphiti/graphiti_core/nodes.py:EntityNode
- Textual memory types (General / Tree / Preference / Activation / Parametric) — [2] — MemTensor__MemOS/src/memos/memories/textual/tree_text_memory/organize/reorganizer.py:GraphStructureReorganizer
- Working / short-term memory stores — [6,19] — mnemosyne-oss__mnemosyne/mnemosyne/core/beam.py:BeamMemory
- Source-anchored repo memories (create/rebind/update/edges) — [8] — cq27-dev__rag-rat/crates/rag-rat-mcp/src/tools/catalog.rs:memory_create
- Session / peer memory representations — [9,14,15] — honcho/src/routers/peers.py:get_peer_card
- Fact types (world / experience / observation / directive / mental-model) — [13,22,23] — hindsight/hindsight-api-slim/hindsight_api/api/http.py
- Memory brain orchestration (vault / graph / vector) — [17] — mengram/mengram/engine/brain.py:MengramBrain
- Project-scoped memory manager — [20] — oraios__serena/src/serena/memories/memory_manager.py:MemoryManager
- Typed request/record / pot model — [21] — potpie/context-engine/src/potpie_context_engine/requests.py
- Store messages as memories — [24] — openviking/server/mcp_endpoint.py:remember
- Conclusions / directives (derived facts) — [13,14] — honcho/src/routers/conclusions.py:create_conclusions

## 11. Memory Lifecycle (consolidation/decay/forgetting/promotion)

- Dream / consolidation of memories — [2,8,13,14,17,22] — MemTensor__MemOS/src/memos/dream/contextualization.py:DreamContextualizer
- Forget / soft-delete / purge memories — [2,8,9,10,15,18,19,22,24] — MemTensor__MemOS/src/memos/api/routers/server_router.py:/delete_memory_by_record_id
- Memory CRUD (write/read/list/delete) — [20] — oraios__serena/src/serena/tools/memory_tools.py:WriteMemoryTool
- Decay / strength scoring (Weibull, hotness) — [19,24] — mnemosyne-oss__mnemosyne/mnemosyne/core/weibull.py:weibull_decay_factor
- Refresh / reindex stale memories (staleness detection) — [1,4,15] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/staleness.ts:isStale
- Promote / evolve procedures & skills from experience — [6,17,22] — src/superlocalmemory/mcp/tools_evolution.py:evolve_skill
- Hygiene / archive / purge expired — [6,19] — mnemosyne-oss__mnemosyne/mnemosyne/core/hygiene.py:AuditReport
- Flush windowed batches / record durable context — [21] — potpie/context-engine/src/potpie_context_engine/application/use_cases/record_durable_context.py

## 12. Temporal Reasoning

- Bi-temporal / reference-time memory model (valid_at / invalid_at) — [2,10,13,17] — getzep__graphiti/mcp_server/src/graphiti_mcp_server.py:search_memory_facts
- Detect changed files / stale graph vs source — [1,4,5,8] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/staleness.ts:isStale
- Temporal / causal ordering of recall — [18] — mnemon/internal/memory/graph/engine.go:TemporalMode
- Natural-language date / temporal expression parsing — [19] — mnemosyne-oss__mnemosyne/mnemosyne/core/temporal_parser.py:parse_nl_date
- Activity timeline of a pot — [21] — potpie/context-engine/src/potpie_context_engine/application/readers/timeline_reader.py
- Lifecycle status & retention stats over time — [22] — src/superlocalmemory/mcp/tools_v28.py:get_lifecycle_status
- Session archives & context history — [24] — openviking/server/routers/sessions.py
- Browse VCS version history / annotations — [26] — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/HistoryController.java:HistoryController.get
- Track dream signals / lifecycle snapshots — [2] — MemTensor__MemOS/src/memos/dream/signal_store.py:DreamSignalStore

## 13. Multi-tenant / Scoping / Namespaces

- Tenant / user / workspace isolation (scoped memory) — [2,6,13,14,19,20,23,24] — honcho/src/routers/workspaces.py:get_or_create_workspace
- Multi-repo / org / group scoping — [4,5,15,21,26] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("group_create")
- Profile / namespace switching (namespaced memory) — [17,18,22] — src/superlocalmemory/mcp/tools_core.py:switch_profile
- Group-id scoping across all memory ops — [10] — getzep__graphiti/mcp_server/src/graphiti_mcp_server.py:add_memory
- Account authority / peer federation admission — [8,18] — mnemon/internal/agency/authority/admission.go:Admit
- Repository-scoped access policy — [5] — abhigyanpatwari__GitNexus/gitnexus/src/mcp/repository-policy.ts:RepositoryPolicy
- Fleet / multi-node scoping — [6] — caura-ai__caura/core-api/src/core_api/routes/fleet.py:/fleet

## 14. Persistence & Storage Backends

- SQLite / embedded local store — [1,5,7,8,9,11,16,17,18,19,20,22] — colbymchenry__codegraph/src/db/index.ts:SqliteBackend
- Postgres / pgvector primary store — [6,13,14,21] — caura-ai__caura/core-storage-api/src/core_storage_api/services/postgres_service.py
- Graph DB (Neo4j / Kuzu / FalkorDB / Cozo) — [2,4,10,15,21] — potpie/context-engine/src/potpie_context_engine/adapters/outbound/graph/backends/falkordb_backend.py:FalkorDBGraphBackend
- Vector DB (Qdrant / Milvus / LanceDB / VikingDB) — [2,3,4,21,24] — MemTensor__MemOS/src/memos/vec_dbs/factory.py
- Lucene inverted index storage — [26] — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/index/IndexDatabase.java:IndexDatabase
- CPG flatgraph binary persistence — [25] — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/CpgBasedTool.scala:CpgBasedTool.loadFromFile

## 15. APIs & Protocols (MCP/REST/CLI/LSP/gRPC)

- MCP server (tool registry) for agents — [2,3,4,5,6,7,8,9,10,13,14,15,16,17,19,20,21,22,23,24] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:dispatch_tool
- REST / HTTP API — [2,4,5,6,13,14,17,21,24,25,26] — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/RestApp.java:RestApp
- CLI (subcommands) — [1,3,4,5,6,7,8,9,11,12,13,14,15,16,17,18,19,21,22,24,25,26] — Muvon__octocode/src/main.rs:Commands
- LSP server for symbol / ref retrieval — [3,15,16,20] — Muvon__octocode/src/mcp/server.rs:lsp_document_symbols
- Webhooks (HMAC-signed) — [13,21] — hindsight/hindsight-control-plane/src/app/api/banks/[bankId]/webhooks/route.ts:POST
- SDK clients (TS / Python) — [13,14,17] — honcho/sdks/python/src/honcho/http/client.py:HonchoHTTPClient
- Query-pack / extension system — [12] — github__codeql/codeql-workspace.yml
- Interactive CPGQL REPL / HTTP server — [25] — joernio__joern/console/src/main/scala/io/joern/console/BridgeBase.scala:BridgeBase.startHttpServer

## 16. LLM & Agent Integration

- Pluggable LLM providers (OpenAI / Anthropic / Ollama / etc.) — [2,6,8,10,13,14,19,24] — MemTensor__MemOS/src/memos/llms/factory.py
- LLM-augmented graph / memory enrichment — [1,4,5,11,15,17] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/llm-analyzer.ts:buildFileAnalysisPrompt
- Agent memory-context assembly for coding agents — [15,16,21,22,24] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("memory_context")
- Deep-search / reasoning agent — [2,21] — MemTensor__MemOS/src/memos/mem_agent/deepsearch_agent.py
- AI code explanation / review / diff (LLM reasoning) — [3] — Muvon__octocode/src/commands/explain.rs:execute
- Load mined user profile into agent via MCP — [9] — emulo.py:load_emulo_profile
- Memory middleware / SDK for agent frameworks — [17,23] — mengram/mengram/mengram_middleware.py:AutoMemory.chat
- Agent orchestration / durable work for peers — [18,20] — oraios__serena/src/serena/agent.py:SerenaAgent
- Reflect / reason with identity — [13] — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_register_reflect
- Compile / scope agent tasks — [24] — bot/vikingbot/agent/tools/compile.py:CompileScopedTool
- 50+ agent-framework integrations — [13] — hindsight/hindsight-integrations/

## 17. IDE / Editor / Chat / CI Integration

- Editor plugins (Claude Code / Cursor / Copilot / Codex) — [1,5,9,22,23] — Egonex-AI__Understand-Anything/.claude-plugin/plugin.json
- Git hooks / CI integration — [7,8,12] — github__codeql/java/ql/src/codeql-suites/java-code-scanning.qls
- MCP connector for coding agents — [4,15,16,24] — intuit__infigraph/crates/infigraph-cli/src/main.rs:Commands::Install
- Claude Code / Cursor hook templates (skill) — [21] — potpie/cli/templates/claude_plugin/skills/potpie-source-ingestion/SKILL.md
- Agent/editor integration apps — [2,6,13,17,18,20] — mengram/mengram/cli.py:sub.add_parser("hook")
- Install memory into agent/IDE memory files — [9,17] — emulo.py:install_profile
- Claude Code UserPromptSubmit hook — [11] — graph-memory-starter/src/build_graph.py:main

## 18. Web UI & Visualization

- React / web dashboard for graph or memory exploration — [1,4,5,13,15,17,20,21,23,24] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/dashboard/src/App.tsx
- Graph visualization export (DOT / GraphML / vis.js) — [15,22,25] — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("visualize")
- Profile / memory card rendering — [9] — emulo.py:show_card
- Web UI serving file content / cross-references — [26] — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/GetFile.java:GetFile
- Interactive TUI explorer — [13] — hindsight/hindsight-cli/src/main.rs:Commands::Explore

## 19. Security & Access Control

- API-key / bearer / OAuth auth — [2,13,14,19,21,24] — MemTensor__MemOS/src/memos/api/middleware/auth.py:verify_api_key
- RBAC / ABAC / access policies — [6,17,22,24] — src/superlocalmemory/access/rbac.py
- Read-only / tool-capability permission gating — [5,20] — abhigyanpatwari__GitNexus/gitnexus/src/mcp/read-only-policy.ts:MCP_READ_ONLY_TOOLS
- Redact secrets / PII from ingested text — [9,17] — emulo.py:redact
- Audit logging of operations — [13,22] — src/superlocalmemory/compliance/audit.py
- Path-based / source authorization filter — [4,26] — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/filter/PathAuthorizationFilter.java:PathAuthorizationFilter
- Encrypted sync transport / continuity kit — [9,19] — mnemosyne-oss__mnemosyne/mnemosyne/core/sync.py:SyncEncryption
- Admission authority verification — [8,18] — cq27-dev__rag-rat/crates/rag-rat-sync/src/auth.rs
- CPGQL HTTP basic-auth — [25] — joernio__joern/console/src/main/scala/io/joern/console/BridgeBase.scala:BridgeBase.Config.serverAuthUsername
- CSRF guard for web API — [4] — Phoenixrr2113__codebase-graph/packages/api/src/csrf-guard.ts

## 20. Observability & Evaluation

- Structured logging / telemetry / tracing — [2,6,10,13,16,17,19,22,24] — MemTensor__MemOS/src/memos/log.py
- Prometheus / metrics endpoints — [13,16,20,24,26] — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/servlet/MetricsServlet.java:MetricsServlet
- Evaluation / benchmark harness — [2,5,8,9,17,19,21,22,24] — MemTensor__MemOS/evaluation/
- Recall / usage diagnostics — [9,18,19] — mnemosyne-oss__mnemosyne/mnemosyne/core/recall_diagnostics.py
- Token counting / tool usage stats — [20] — oraios__serena/src/serena/analytics.py:TokenCountEstimator
- Posthog analytics on tool usage — [23] — apps/mcp/src/server/analytics.ts
- Queue / dream status — [14] — honcho/src/routers/workspaces.py:get_queue_status
- Telemetry subsystem — [7] — colbymchenry__codegraph/src/telemetry
- Health / version endpoints — [13] — hindsight/hindsight-control-plane/src/app/api/health/route.ts:GET
- GDPR data export / erasure — [22] — src/superlocalmemory/compliance/gdpr.py
- Embedding benchmark suite — [8] — cq27-dev__rag-rat/crates/rag-rat-core/src/eval.rs

## 21. Extensibility (plugins/providers/custom)

- Pluggable provider factories (embedders / LLMs / vec / rerank / chunkers) — [2,3,6,10,13,14,16,18,19,24] — MemTensor__MemOS/src/memos/embedders/factory.py
- Plugin manager / hook system — [2,9,19] — MemTensor__MemOS/src/memos/plugins/manager.py:PluginManager
- Custom tool / analyzer registration — [1,20,25] — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/plugins/registry.ts:PluginRegistry
- Multi-language extractor / grammar framework — [1,4,7,15,16,26] — intuit__infigraph/crates/infigraph-languages/
- Pluggable graph DB drivers — [4,21] — Phoenixrr2113__codebase-graph/packages/graph/src/driver-registry.ts:registerDriver
- Custom knowledge-graph source / extraction — [11,17] — graph-memory-starter/src/build_graph.py:main
- Skill management / installable skills — [21,22,24] — potpie/cli/commands/skills.py:skills_app
- Oracle backends (rust-analyzer / SCIP / custom) — [8] — cq27-dev__rag-rat/crates/rag-rat-oracle/src/backend
- Custom entity/edge type config + schema migrations — [10] — getzep__graphiti/mcp_server/src/config/schema.py:EntityTypeConfig
- Multi-language LSP backends — [20] — oraios__serena/src/serena/solidlsp/language_servers/
- Pluggable auth / VCS / authorization plugins — [26] — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/authorization/AuthorizationFramework.java:AuthorizationFramework
- Pluggable importer providers — [19] — mnemosyne-oss__mnemosyne/mnemosyne/core/importers/base.py:BaseImporter

## 22. Ops & Deployment

- Docker / container deployment — [2,3,4,5,8,9,10,12,13,14,17,18,19,20,21,24,25,26] — MemTensor__MemOS/Dockerfile
- Docker Compose / Helm / K8s — [4,6,13,20,21] — Phoenixrr2113__codebase-graph/docker-compose.yml
- Installer / setup scripts (cross-platform) — [1,7,15] — Egonex-AI__Understand-Anything/install.sh
- Auto-reindex / watch daemon — [15,16,21] — intuit__infigraph/crates/infigraph-cli/src/main.rs:Commands::Watch
- Self-update mechanism — [15,16] — intuit__infigraph/crates/infigraph-cli/src/main.rs:Commands::Update
- Health / diagnostics endpoints — [13,16,24] — hindsight/hindsight-control-plane/src/app/api/health/route.ts:GET
- DB migration tooling — [19,22] — src/superlocalmemory/cli/db_migrate.py
- Local peer mesh / sync — [22] — src/superlocalmemory/mcp/tools_mesh.py:mesh_send
- Background scheduler service — [2,21] — MemTensor__MemOS/src/memos/mem_scheduler/
- Cloud push/pull of context — [21] — potpie/cli/commands/cloud.py:cloud_app
- Language database extractors / preflight — [12,24] — github__codeql/unified/codeql-extractor.yml

## Repo → categories present (coverage matrix)

| # | Repo | Domains covered |
|---|------|-----------------|
| 1 | Understand-Anything | 1,2,3,4,5,6,8,9,10,11,12,14,15,16,17,18,21,22 |
| 2 | MemOS | 1,2,4,5,6,7,10,11,12,13,14,15,16,17,19,20,21,22 |
| 3 | octocode | 1,2,3,4,5,6,7,8,9,14,15,16,21,22 |
| 4 | codebase-graph | 1,2,3,4,5,6,7,8,9,10,11,13,14,15,16,17,19,21,22 |
| 5 | GitNexus | 1,2,3,4,5,6,7,8,9,12,13,14,15,16,17,19,20,21,22 |
| 6 | caura | 1,2,3,4,5,6,7,10,11,13,14,15,16,17,19,20,21,22 |
| 7 | codegraph | 1,2,3,4,6,8,9,14,15,17,18,20,21,22 |
| 8 | rag-rat | 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,19,20,21,22 |
| 9 | emulo | 1,2,10,11,14,15,16,17,18,19,20,21,22 |
| 10 | graphiti | 1,3,4,5,7,10,11,12,13,14,15,16,20,21,22 |
| 11 | graph-memory-starter | 1,2,4,5,6,7,10,14,15,16,17,21 |
| 12 | codeql | 8,9,15,17,22 |
| 13 | hindsight | 1,2,3,4,5,6,7,10,11,12,13,14,15,16,17,18,19,20,21,22 |
| 14 | honcho | 1,5,6,10,11,13,14,15,16,19,20,21,22 |
| 15 | infigraph | 1,2,3,4,5,6,7,8,9,10,11,13,14,15,16,17,18,21,22 |
| 16 | sense | 1,2,3,4,5,6,8,9,14,15,16,17,20,21,22 |
| 17 | mengram | 1,2,3,4,5,7,10,11,12,13,14,15,16,17,18,19,20,21,22 |
| 18 | mnemon | 1,3,4,5,6,7,10,11,12,13,14,15,16,17,18,19,20,21,22 |
| 19 | mnemosyne | 1,3,4,5,7,10,11,12,13,14,15,16,19,20,21,22 |
| 20 | serena | 3,4,6,8,9,10,11,13,14,15,16,17,18,19,20,21,22 |
| 21 | potpie | 1,2,4,5,6,7,10,11,12,13,14,15,16,17,18,19,20,21,22 |
| 22 | superlocalmemory | 1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22 |
| 23 | supermemory | 1,4,5,6,10,13,15,16,17,18,20 |
| 24 | OpenViking | 1,2,3,4,5,6,7,8,10,11,12,13,14,15,16,17,18,19,20,21,22 |
| 25 | joern | 1,2,3,4,5,8,9,14,15,18,19,21,22 |
| 26 | opengrok | 1,2,3,6,7,8,12,13,14,15,18,19,20,21,22 |

## Best-in-class per feature (winner map)

### 1. Ingestion & Indexing
- Index a codebase into a persistent graph / vector store → **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("index_project") — tree-sitter + 60+ languages into a Kuzu graph, fastest offline index.
- Ingest documents, memories, or conversation files → **★ #2 MemOS** — MemTensor__MemOS/src/memos/api/mcp_serve.py:add_memory — unified add_memory from text/docs/chat with cube registration.
- Watch / auto-reindex on file change → **★ #3 octocode** — Muvon__octocode/src/commands/watch.rs:execute — native file-watcher reindex loop.
- Differential / incremental / branch-delta indexing → **★ #3 octocode** — Muvon__octocode/src/indexer/differential_processor.rs — branch-delta differential processing.
- Index git / VCS commit history → **★ #26 opengrok** — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/history/GitRepository.java:GitRepository — mature VCS history indexing across SCMs.
- Generate ignore / scope filters to bound indexing → **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/ignore-filter.ts:createIgnoreFilter — language/framework-aware ignore filters.
- Import external indexes / connectors (SCIP, GitHub, Notion, other memory systems) → **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("scip_import") — SCIP import for compiler-accurate graphs.
- Emit a starter ignore/scope config file → **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/ignore-generator.ts:generateStarterIgnoreFile — unique to #1.
- Reindex / backfill content on demand → **★ #24 OpenViking** — openviking/ingest/orchestrator.py:IngestOrchestrator.backfill — configurable backfill/reindex orchestration.

### 2. Parsing & Chunking
- Parse source code via tree-sitter across many languages → **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/plugins/tree-sitter-plugin.ts:TreeSitterPlugin — 60+ languages via tree-sitter.
- Parse documents via pluggable parsers (Markdown/YAML/JSON/code) → **★ #2 MemOS** — MemTensor__MemOS/src/memos/parsers/markitdown.py:MarkItDownParser — MarkItDown multi-format parsing.
- Chunk content into nodes/chunks for embedding → **★ #2 MemOS** — MemTensor__MemOS/src/memos/chunkers/factory.py — char/sentence/markdown/simple strategies.
- Extract atomic facts / structured entities from text → **★ #9 emulo** — emulo.py:mine_files — atomic-fact extraction pipeline.
- Register pluggable language / format analyzer plugins → **★ #26 opengrok** — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/analysis/AnalyzerGuru.java:AnalyzerGuru — 50+ pluggable jflex analyzers.
- Parse repositories into a structured context graph → **★ #21 potpie** — potpie/context-engine/src/potpie_context_engine/adapters/outbound/graph/context_graph_service.py — repo→context-graph parser.

### 3. Symbol/Entity Extraction
- Extract symbols/functions/classes via tree-sitter / LSP → **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("get_symbols_in_file") — broad symbol surface incl. API surface.
- Extract entities + relationships from text via LLM → **★ #10 graphiti** — getzep__graphiti/graphiti_core/graphiti.py:add_episode — LLM entity/edge fact extraction.
- Extract behavioral assertions / facts from tool usage / sessions → **★ #22 superlocalmemory** — src/superlocalmemory/learning/assertion_miner.py — mines behavioral assertions from tool use.
- Resolve symbol references / callers / callees (360° reference resolution) → **★ #5 GitNexus** — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:context — 360° caller/callee/import/override resolution.
- Detect HTTP routes / API surface / endpoints → **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("detect_routes") — route/endpoint detection tool.
- Regex / similarity entity extraction → **★ #19 mnemosyne** — mnemosyne-oss__mnemosyne/mnemosyne/core/entities.py:extract_entities_regex — regex + similarity matching.

### 4. Graph Construction
- Build a code/entity knowledge graph → **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/graph-builder.ts:GraphBuilder — schema-validated KG builder.
- Persist memory as a property graph (Neo4j / Kuzu / FalkorDB / Cozo) → **★ #10 graphiti** — getzep__graphiti/graphiti_core/driver/neo4j_driver.py — multi-graph-backend driver (Neo4j/FalkorDB/Kuzu/Neptune).
- Build entity/relation/triple graph from text → **★ #19 mnemosyne** — mnemosyne-oss__mnemosyne/mnemosyne/core/triples.py:TripleStore — triple store + episodic graph.
- Reference / call graph between symbols → **★ #20 serena** — oraios__serena/src/serena/symbol.py:ReferenceInLanguageServerSymbol — LSP-backed reference graph.
- Detect communities / relation reasoning over graph → **★ #5 GitNexus** — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:cypher — Leiden community detection.
- Query graph via Cypher / traversal API → **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("query_graph") — Cypher query over code graph.
- Compose layered Code Property Graph (AST / CFG / DDG / CDG) → **★ #25 joern** — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/DefaultOverlays.scala:DefaultOverlays.create — canonical CPG overlays.
- Pluggable graph DB driver (FalkorDB / Neo4j / Memgraph / LanceDB) → **★ #4 codebase-graph** — Phoenixrr2113__codebase-graph/packages/graph/src/driver-registry.ts:registerDriver — driver registry unique to #4.
- Export graph (graphml / json / cypher / DOT) → **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("export_graph") — multi-format graph export.
- VikingDB vector+graph index management → **★ #24 OpenViking** — openviking/storage/vikingdb_manager.py:VikingDBManager — managed VikingDB index, unique to #24.
- Detect relations and reasoning between memory nodes → **★ #2 MemOS** — MemTensor__MemOS/src/memos/memories/textual/tree_text_memory/organize/relation_reason_detector.py:RelationAndReasoningDetector — unique to #2.

### 5. Embeddings & Vector Search
- Generate embeddings via pluggable providers (OpenAI / Ollama / Sentence-Transformers / etc.) → **★ #2 MemOS** — MemTensor__MemOS/src/memos/embedders/factory.py — widest provider factory.
- Semantic / vector similarity search over nodes/memories → **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/embedding-search.ts:SemanticSearchEngine — semantic search engine over KG.
- On-device / bundled embedding (FastEmbed, ONNX, feature-hash) → **★ #8 rag-rat** — cq27-dev__rag-rat/crates/rag-rat-llm/src/fastembed.rs:FastEmbedEmbedder — on-device FastEmbed.
- Cosine similarity ranking → **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/embedding-search.ts:cosineSimilarity — unique to union basis.

### 6. Keyword/Lexical Search
- BM25 full-text / lexical search over index — **★ #2 MemOS** — MemTensor__MemOS/src/memos/memories/textual/tree_text_memory/retrieve/bm25_util.py:EnhancedBM25 — EnhancedBM25 retrieval.
- Regex / grep / AST code search — **★ #3 octocode** — Muvon__octocode/src/commands/grep.rs:execute — ast-grep structural search.
- Full-text search over indexed code via REST / API — **★ #26 opengrok** — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/SearchController.java:SearchController.search — mature Lucene REST search.
- FTS5 full-text search (SQLite) — **★ #7 codegraph** — colbymchenry__codegraph/src/db/queries.ts:nodes_fts — FTS5 bm25 ranking.
- Lexical memory / fact search — **★ #22 superlocalmemory** — src/superlocalmemory/mcp/tools_core.py:search — memory lexical search.
- List memories by container/tag — **★ #23 supermemory** — apps/mcp/src/server/tools/list-memories.ts:register — container-tag listing.
- Workspace/session/peer lexical search — **★ #14 honcho** — honcho/src/routers/workspaces.py:search_workspace — workspace-scoped lexical search.
- Token/Jaccard/CJK keyword scoring — **★ #18 mnemon** — mnemon/internal/memory/search/keyword.go:KeywordSearch — CJK-aware scoring.
- Keyword/semantic search CLI — **★ #16 sense** — luuuc__sense/cmd/sense/main.go — unified keyword/semantic CLI.

### 7. Hybrid & Ranked Retrieval
- Reciprocal-Rank-Fusion (RRF) hybrid retrieval — **★ #5 GitNexus** — abhigyanpatwari__GitNexus/gitnexus/src/core/search/hybrid-search.ts:mergeWithRRF — BM25+semantic RRF.
- Rerank with cross-encoder / MLX / Jina reranker — **★ #10 graphiti** — getzep__graphiti/graphiti_core/cross_encoder/ — bge/gemini/openai cross-encoders.
- MMR / maximal-marginal-relevance reranking — **★ #19 mnemosyne** — mnemosyne-oss__mnemosyne/mnemosyne/core/mmr.py:mmr_rerank — MMR reranker.
- Graph-enriched retrieval (graph + vector) — **★ #4 codebase-graph** — Phoenixrr2113__codebase-graph/packages/core/src/enrichedSearchV2.ts:enrichFromGraph — graph-enriched retrieval.
- Intent-aware / hierarchical retriever (vector + lexical tiers) — **★ #18 mnemon** — mnemon/internal/memory/search/recall.go:IntentAwareRecall — intent-aware blend.

### 8. Code Navigation (LSP/refs/defs)
- LSP go-to-definition / hover / find-references / completions — **★ #3 octocode** — Muvon__octocode/src/mcp/server.rs:lsp_goto_definition — full LSP hover/refs/completion.
- Trace callers / callees / impact over the graph — **★ #5 GitNexus** — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:impact — caller/callee/impact tracing.
- Find symbol / definition references via IDE (JetBrains) — **★ #26 opengrok** — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/FileController.java:FileController.getDefinitions — cross-ref via web API.
- AST / CFG traversal for navigation — **★ #25 joern** — joernio__joern/semanticcpg/src/main/scala/io/shiftleft/semanticcpg/language/types/expressions/generalizations/AstNodeTraversal.scala:AstNodeTraversal.ast — AST traversal API.
- Onboarding tour / heuristic walkthrough of codebase — **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/tour-generator.ts:generateHeuristicTour — unique to #1.
- Filesystem ls/tree/stat over virtual URIs — **★ #24 OpenViking** — openviking/server/routers/filesystem.py — viking:// URI filesystem nav.
- IDE local-definitions / local-references (CodeQL) — **★ #12 codeql** — github__codeql/ql/ql/src/ide-contextual-queries/localDefinitions.ql — IDE contextual queries.
- Rename + symbol-body edits — **★ #20 serena** — oraios__serena/src/serena/tools/symbol_tools.py:RenameSymbolTool — rename + body edits.

### 9. Code Analysis (static/dataflow/taint)
- Taint / dataflow analysis (sources → sinks) — **★ #25 joern** — joernio__joern/dataflowengineoss/src/main/scala/io/joern/dataflowengineoss/language/ExtendedCfgNode.scala:ExtendedCfgNode.reachableByFlows — canonical dataflow engine.
- Dead-code / unused-symbol detection — **★ #16 sense** — luuuc__sense/internal/dead/dead.go:FindDead — dead-code detector.
- Blast-radius / change-impact analysis — **★ #22 superlocalmemory** — src/superlocalmemory/mcp/tools_code_graph.py:get_blast_radius — blast-radius tool.
- Security vulnerability scan / CWE query packs — **★ #12 codeql** — github__codeql/python/ql/src/Security/CWE-089/SqlInjection.ql — CWE security packs.
- Cyclomatic complexity / code metrics — **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("get_complexity") — complexity metric.
- Clone / near-duplicate detection — **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("detect_clones") — clone detection.
- AI code review / diff / explain / commit — **★ #3 octocode** — Muvon__octocode/src/commands/review.rs:execute — explain/diff/review/commit/release.
- Load-bearing symbol ranking (PageRank) — **★ #8 rag-rat** — cq27-dev__rag-rat/crates/rag-rat-query/src/pagerank.rs — PageRank load-bearing ranking.
- Import-cycle / dependency-cycle detection — **★ #5 GitNexus** — abhigyanpatwari__GitNexus/gitnexus/src/core/graph/import-cycles.ts — import-cycle detection.
- LSP diagnostics — **★ #20 serena** — oraios__serena/src/serena/tools/symbol_tools.py:GetDiagnosticsForFileTool — diagnostics tool.

### 10. Memory Model & Types
- Typed node/edge schema (Entity / Edge / Community / Episode) — **★ #10 graphiti** — getzep__graphiti/graphiti_core/nodes.py:EntityNode — rich typed node schema.
- Textual memory types (General / Tree / Preference / Activation / Parametric) — **★ #2 MemOS** — MemTensor__MemOS/src/memos/memories/textual/ — most memory-type variety.
- Working / short-term memory stores — **★ #19 mnemosyne** — mnemosyne-oss__mnemosyne/mnemosyne/core/beam.py:BeamMemory — BeamMemory working store.
- Source-anchored repo memories (create/rebind/update/edges) — **★ #8 rag-rat** — cq27-dev__rag-rat/crates/rag-rat-mcp/src/tools/catalog.rs:memory_create — source-anchored memories.
- Session / peer memory representations — **★ #14 honcho** — honcho/src/routers/peers.py:get_peer_card — peer-card model.
- Fact types (world / experience / observation / directive / mental-model) — **★ #13 hindsight** — hindsight/hindsight-api-slim/hindsight_api/api/http.py — world/experience/observation facts.
- Memory brain orchestration (vault / graph / vector) — **★ #17 mengram** — mengram/mengram/engine/brain.py:MengramBrain — unified brain orchestrator.
- Project-scoped memory manager — **★ #20 serena** — oraios__serena/src/serena/memories/memory_manager.py:MemoryManager — project-scoped memory.
- Typed request/record / pot model — **★ #21 potpie** — potpie/context-engine/src/potpie_context_engine/requests.py — typed pot model.
- Store messages as memories — **★ #24 OpenViking** — openviking/server/mcp_endpoint.py:remember — message memory store.
- Conclusions / directives (derived facts) — **★ #14 honcho** — honcho/src/routers/conclusions.py:create_conclusions — derived conclusions.

### 11. Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Dream / consolidation of memories — **★ #2 MemOS** — MemTensor__MemOS/src/memos/dream/contextualization.py:DreamContextualizer — full dream/consolidation pipeline.
- Forget / soft-delete / purge memories — **★ #2 MemOS** — MemTensor__MemOS/src/memos/api/routers/server_router.py:/delete_memory_by_record_id — soft-delete + recover.
- Memory CRUD (write/read/list/delete) — **★ #20 serena** — oraios__serena/src/serena/tools/memory_tools.py:WriteMemoryTool — memory CRUD tools.
- Decay / strength scoring (Weibull, hotness) — **★ #19 mnemosyne** — mnemosyne-oss__mnemosyne/mnemosyne/core/weibull.py:weibull_decay_factor — Weibull decay model.
- Refresh / reindex stale memories (staleness detection) — **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/staleness.ts:isStale — staleness detection.
- Promote / evolve procedures & skills from experience — **★ #22 superlocalmemory** — src/superlocalmemory/mcp/tools_evolution.py:evolve_skill — skill evolution.
- Hygiene / archive / purge expired — **★ #19 mnemosyne** — mnemosyne-oss__mnemosyne/mnemosyne/core/hygiene.py:AuditReport — hygiene audit + clean.
- Flush windowed batches / record durable context — **★ #21 potpie** — potpie/context-engine/src/potpie_context_engine/application/use_cases/record_durable_context.py — durable context recording.

### 12. Temporal Reasoning
- Bi-temporal / reference-time memory model (valid_at / invalid_at) — **★ #10 graphiti** — getzep__graphiti/mcp_server/src/graphiti_mcp_server.py:search_memory_facts — bi-temporal model.
- Detect changed files / stale graph vs source — **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/staleness.ts:isStale — change/staleness detection.
- Temporal / causal ordering of recall — **★ #18 mnemon** — mnemon/internal/memory/graph/engine.go:TemporalMode — temporal+causal ordering.
- Natural-language date / temporal expression parsing — **★ #19 mnemosyne** — mnemosyne-oss__mnemosyne/mnemosyne/core/temporal_parser.py:parse_nl_date — NL date parsing.
- Activity timeline of a pot — **★ #21 potpie** — potpie/context-engine/src/potpie_context_engine/application/readers/timeline_reader.py — pot timeline reader.
- Lifecycle status & retention stats over time — **★ #22 superlocalmemory** — src/superlocalmemory/mcp/tools_v28.py:get_lifecycle_status — lifecycle stats.
- Session archives & context history — **★ #24 OpenViking** — openviking/server/routers/sessions.py — session archives.
- Browse VCS version history / annotations — **★ #26 opengrok** — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/HistoryController.java:HistoryController.get — VCS history browser.
- Track dream signals / lifecycle snapshots — **★ #2 MemOS** — MemTensor__MemOS/src/memos/dream/signal_store.py:DreamSignalStore — unique to #2.

### 13. Multi-tenant / Scoping / Namespaces
- Tenant / user / workspace isolation (scoped memory) — **★ #14 honcho** — honcho/src/routers/workspaces.py:get_or_create_workspace — workspace isolation.
- Multi-repo / org / group scoping — **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("group_create") — org-scoped groups.
- Profile / namespace switching (namespaced memory) — **★ #22 superlocalmemory** — src/superlocalmemory/mcp/tools_core.py:switch_profile — profile switching.
- Group-id scoping across all memory ops — **★ #10 graphiti** — getzep__graphiti/mcp_server/src/graphiti_mcp_server.py:add_memory — group-id scoping.
- Account authority / peer federation admission — **★ #18 mnemon** — mnemon/internal/agency/authority/admission.go:Admit — multi-agent admission.
- Repository-scoped access policy — **★ #5 GitNexus** — abhigyanpatwari__GitNexus/gitnexus/src/mcp/repository-policy.ts:RepositoryPolicy — repo policy.
- Fleet / multi-node scoping — **★ #6 caura** — caura-ai__caura/core-api/src/core_api/routes/fleet.py:/fleet — fleet scoping.

### 14. Persistence & Storage Backends
- SQLite / embedded local store — **★ #7 codegraph** — colbymchenry__codegraph/src/db/index.ts:SqliteBackend — node:sqlite WAL backend.
- Postgres / pgvector primary store — **★ #6 caura** — caura-ai__caura/core-storage-api/src/core_storage_api/services/postgres_service.py — Postgres+pgvector.
- Graph DB (Neo4j / Kuzu / FalkorDB / Cozo) — **★ #10 graphiti** — getzep__graphiti/graphiti_core/driver/neo4j_driver.py — 4 graph backends.
- Vector DB (Qdrant / Milvus / LanceDB / VikingDB) — **★ #2 MemOS** — MemTensor__MemOS/src/memos/vec_dbs/factory.py — Qdrant/Milvus factory.
- Lucene inverted index storage — **★ #26 opengrok** — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/index/IndexDatabase.java:IndexDatabase — Lucene data root.
- CPG flatgraph binary persistence — **★ #25 joern** — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/CpgBasedTool.scala:CpgBasedTool.loadFromFile — cpg.bin flatgraph.

### 15. APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server (tool registry) for agents — **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:dispatch_tool — broadest code MCP toolset.
- REST / HTTP API — **★ #26 opengrok** — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/RestApp.java:RestApp — JAX-RS /api/v1.
- CLI (subcommands) — **★ #3 octocode** — Muvon__octocode/src/main.rs:Commands — clap CLI surface.
- LSP server for symbol / ref retrieval — **★ #3 octocode** — Muvon__octocode/src/mcp/server.rs:lsp_document_symbols — LSP document symbols.
- Webhooks (HMAC-signed) — **★ #13 hindsight** — hindsight/hindsight-control-plane/src/app/api/banks/[bankId]/webhooks/route.ts:POST — HMAC webhooks.
- SDK clients (TS / Python) — **★ #14 honcho** — honcho/sdks/python/src/honcho/http/client.py:HonchoHTTPClient — HTTP SDK clients.
- Query-pack / extension system — **★ #12 codeql** — github__codeql/codeql-workspace.yml — query-pack system.
- Interactive CPGQL REPL / HTTP server — **★ #25 joern** — joernio__joern/console/src/main/scala/io/joern/console/BridgeBase.scala:BridgeBase.startHttpServer — CPGQL server.

### 16. LLM & Agent Integration
- Pluggable LLM providers (OpenAI / Anthropic / Ollama / etc.) — **★ #2 MemOS** — MemTensor__MemOS/src/memos/llms/factory.py — widest LLM factory.
- LLM-augmented graph / memory enrichment — **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/llm-analyzer.ts:buildFileAnalysisPrompt — LLM graph enrichment.
- Agent memory-context assembly for coding agents — **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("memory_context") — LM2 context assembly.
- Deep-search / reasoning agent — **★ #2 MemOS** — MemTensor__MemOS/src/memos/mem_agent/deepsearch_agent.py — deepsearch agent.
- AI code explanation / review / diff (LLM reasoning) — **★ #3 octocode** — Muvon__octocode/src/commands/explain.rs:execute — explain/diff/review.
- Load mined user profile into agent via MCP — **★ #9 emulo** — emulo.py:load_emulo_profile — profile injection, unique to #9.
- Memory middleware / SDK for agent frameworks — **★ #23 supermemory** — packages/ai-sdk — official agent SDKs.
- Agent orchestration / durable work for peers — **★ #20 serena** — oraios__serena/src/serena/agent.py:SerenaAgent — agent orchestration.
- Reflect / reason with identity — **★ #13 hindsight** — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_register_reflect — reflect tool.
- Compile / scope agent tasks — **★ #24 OpenViking** — bot/vikingbot/agent/tools/compile.py:CompileScopedTool — scoped task compile.
- 50+ agent-framework integrations — **★ #13 hindsight** — hindsight/hindsight-integrations/ — broadest integrations.

### 17. IDE / Editor / Chat / CI Integration
- Editor plugins (Claude Code / Cursor / Copilot / Codex) — **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/.claude-plugin/plugin.json — multi-editor plugin manifests.
- Git hooks / CI integration — **★ #12 codeql** — github__codeql/java/ql/src/codeql-suites/java-code-scanning.qls — code-scanning suites.
- MCP connector for coding agents — **★ #15 infigraph** — intuit__infigraph/crates/infigraph-cli/src/main.rs:Commands::Install — MCP config installer.
- Claude Code / Cursor hook templates (skill) — **★ #21 potpie** — potpie/cli/templates/claude_plugin/skills/potpie-source-ingestion/SKILL.md — skill templates.
- Agent/editor integration apps — **★ #2 MemOS** — MemTensor__MemOS/apps/ — OpenClaw/local/plugin apps.
- Install memory into agent/IDE memory files — **★ #9 emulo** — emulo.py:install_profile — installs into IDE memory files.
- Claude Code UserPromptSubmit hook — **★ #11 graph-memory-starter** — graph-memory-starter/src/recall_hook.py — recall hook.

### 18. Web UI & Visualization
- React / web dashboard for graph or memory exploration — **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/dashboard/src/App.tsx — React KG dashboard.
- Graph visualization export (DOT / GraphML / vis.js) — **★ #15 infigraph** — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("visualize") — vis.js visualization.
- Profile / memory card rendering — **★ #9 emulo** — emulo.py:show_card — profile card.
- Web UI serving file content / cross-references — **★ #26 opengrok** — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/GetFile.java:GetFile — cross-ref web UI.
- Interactive TUI explorer — **★ #13 hindsight** — hindsight/hindsight-cli/src/main.rs:Commands::Explore — TUI explorer.

### 19. Security & Access Control
- API-key / bearer / OAuth auth — **★ #2 MemOS** — MemTensor__MemOS/src/memos/api/middleware/auth.py:verify_api_key — API-key auth + scopes.
- RBAC / ABAC / access policies — **★ #22 superlocalmemory** — src/superlocalmemory/access/rbac.py — RBAC + ABAC.
- Read-only / tool-capability permission gating — **★ #5 GitNexus** — abhigyanpatwari__GitNexus/gitnexus/src/mcp/read-only-policy.ts:MCP_READ_ONLY_TOOLS — read-only allow-list.
- Redact secrets / PII from ingested text — **★ #9 emulo** — emulo.py:redact — secrets/PII redaction.
- Audit logging of operations — **★ #22 superlocalmemory** — src/superlocalmemory/compliance/audit.py — audit logging.
- Path-based / source authorization filter — **★ #26 opengrok** — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/filter/PathAuthorizationFilter.java:PathAuthorizationFilter — path auth filter.
- Encrypted sync transport / continuity kit — **★ #19 mnemosyne** — mnemosyne-oss__mnemosyne/mnemosyne/core/sync.py:SyncEncryption — encrypted sync.
- Admission authority verification — **★ #8 rag-rat** — cq27-dev__rag-rat/crates/rag-rat-sync/src/auth.rs — sync auth.
- CPGQL HTTP basic-auth — **★ #25 joern** — joernio__joern/console/src/main/scala/io/joern/console/BridgeBase.scala:BridgeBase.Config.serverAuthUsername — HTTP basic-auth.
- CSRF guard for web API — **★ #4 codebase-graph** — Phoenixrr2113__codebase-graph/packages/api/src/csrf-guard.ts — CSRF guard.

### 20. Observability & Evaluation
- Structured logging / telemetry / tracing — **★ #2 MemOS** — MemTensor__MemOS/src/memos/log.py — structured logging.
- Prometheus / metrics endpoints — **★ #26 opengrok** — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/servlet/MetricsServlet.java:MetricsServlet — Prometheus servlet.
- Evaluation / benchmark harness — **★ #2 MemOS** — MemTensor__MemOS/evaluation/ — evaluation harness.
- Recall / usage diagnostics — **★ #19 mnemosyne** — mnemosyne-oss__mnemosyne/mnemosyne/core/recall_diagnostics.py — recall diagnostics.
- Token counting / tool usage stats — **★ #20 serena** — oraios__serena/src/serena/analytics.py:TokenCountEstimator — token estimator.
- Posthog analytics on tool usage — **★ #23 supermemory** — apps/mcp/src/server/analytics.ts — Posthog analytics.
- Queue / dream status — **★ #14 honcho** — honcho/src/routers/workspaces.py:get_queue_status — queue status.
- Telemetry subsystem — **★ #7 codegraph** — colbymchenry__codegraph/src/telemetry — telemetry subsystem.
- Health / version endpoints — **★ #13 hindsight** — hindsight/hindsight-control-plane/src/app/api/health/route.ts:GET — health endpoint.
- GDPR data export / erasure — **★ #22 superlocalmemory** — src/superlocalmemory/compliance/gdpr.py — GDPR export/erasure.
- Embedding benchmark suite — **★ #8 rag-rat** — cq27-dev__rag-rat/crates/rag-rat-core/src/eval.rs — embedding benchmark.

### 21. Extensibility (plugins/providers/custom)
- Pluggable provider factories (embedders / LLMs / vec / rerank / chunkers) — **★ #2 MemOS** — MemTensor__MemOS/src/memos/embedders/factory.py — provider factories everywhere.
- Plugin manager / hook system — **★ #2 MemOS** — MemTensor__MemOS/src/memos/plugins/manager.py:PluginManager — plugin manager + hooks.
- Custom tool / analyzer registration — **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/plugins/registry.ts:PluginRegistry — plugin registry.
- Multi-language extractor / grammar framework — **★ #15 infigraph** — intuit__infigraph/crates/infigraph-languages/ — 60+ language grammars.
- Pluggable graph DB drivers — **★ #4 codebase-graph** — Phoenixrr2113__codebase-graph/packages/graph/src/driver-registry.ts:registerDriver — driver registry.
- Custom knowledge-graph source / extraction — **★ #11 graph-memory-starter** — graph-memory-starter/extraction/*.json — JSON KG source.
- Skill management / installable skills — **★ #21 potpie** — potpie/cli/commands/skills.py:skills_app — installable skills.
- Oracle backends (rust-analyzer / SCIP / custom) — **★ #8 rag-rat** — cq27-dev__rag-rat/crates/rag-rat-oracle/src/backend — oracle backends.
- Custom entity/edge type config + schema migrations — **★ #10 graphiti** — getzep__graphiti/mcp_server/src/config/schema.py:EntityTypeConfig — schema config.
- Multi-language LSP backends — **★ #20 serena** — oraios__serena/src/serena/solidlsp/language_servers/ — solidlsp backends.
- Pluggable auth / VCS / authorization plugins — **★ #26 opengrok** — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/authorization/AuthorizationFramework.java:AuthorizationFramework — pluggable authz.
- Pluggable importer providers — **★ #19 mnemosyne** — mnemosyne-oss__mnemosyne/mnemosyne/core/importers/base.py:BaseImporter — importer providers.

### 22. Ops & Deployment
- Docker / container deployment — **★ #2 MemOS** — MemTensor__MemOS/Dockerfile — broad container assets.
- Docker Compose / Helm / K8s — **★ #13 hindsight** — hindsight/helm/ — Helm/K8s deployment.
- Installer / setup scripts (cross-platform) — **★ #1 Understand-Anything** — Egonex-AI__Understand-Anything/install.sh — installer script.
- Auto-reindex / watch daemon — **★ #15 infigraph** — intuit__infigraph/crates/infigraph-cli/src/main.rs:Commands::Watch — watch daemon.
- Self-update mechanism — **★ #15 infigraph** — intuit__infigraph/crates/infigraph-cli/src/main.rs:Commands::Update — self-update.
- Health / diagnostics endpoints — **★ #13 hindsight** — hindsight/hindsight-control-plane/src/app/api/health/route.ts:GET — health endpoint.
- DB migration tooling — **★ #22 superlocalmemory** — src/superlocalmemory/cli/db_migrate.py — DB migrate.
- Local peer mesh / sync — **★ #22 superlocalmemory** — src/superlocalmemory/mcp/tools_mesh.py:mesh_send — peer mesh.
- Background scheduler service — **★ #2 MemOS** — MemTensor__MemOS/src/memos/mem_scheduler/ — scheduler service.
- Cloud push/pull of context — **★ #21 potpie** — potpie/cli/commands/cloud.py:cloud_app — cloud push/pull.
- Language database extractors / preflight — **★ #12 codeql** — github__codeql/unified/codeql-extractor.yml — DB extractors.

## Notes & caveats

- The union splits cleanly into two camps: code-intelligence repos (octocode, codebase-graph, GitNexus, codegraph, rag-rat, serena, infigraph, sense, joern, opengrok, codeql) focused on graph/retrieval/static-analysis, and agent-memory repos (MemOS, graphiti, hindsight, honcho, mengram, mnemon, mnemosyne, superlocalmemory, supermemory, emulo, caura, OpenViking, potpie, graph-memory-starter) focused on memory models/lifecycle.
- MCP-tool counts cited in citations (e.g. rag-rat ~47, infigraph dispatch, MemOS 16, GitNexus 17, caura 12, codegraph 8) come from code registrations (`@server.tool` / `tool_def` / `TOOL_NAMES`), NOT from README prose.
- Repos were treated read-only; no source file was opened, modified, or moved — only the pre-extracted inventory lines were merged.
- A few raw citations are directories or lack an explicit `:symbol` (e.g. `infigraph-languages/`, `extraction/*.json`, `packages/ai-sdk`); these were kept verbatim as ground-truth tokens and every atom still carries at least one `file:symbol` token.
- Coverage matrix rows are derived directly from the `[tags]` assigned per domain; a repo "covers" a domain iff it appears in any atom's `[tags]` for that domain.
