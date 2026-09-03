# RAW CODE-ONLY INVENTORIES (7 subagent groups, repos 1-26)
# Each capability was extracted from SOURCE CODE ONLY (no README/docs/prose).
# Format per line: `- <capability> — <repo_dir>/<relative/file.ext>:<symbol>`
# Repo tag -> owner/repo -> dir on share //192.168.1.7/d/claude/repos/membrane/<dir>

# LEGEND (tag -> owner/repo -> dir)
# 1  Egonex-AI/Understand-Anything   Egonex-AI__Understand-Anything
# 2  MemTensor/MemOS                MemTensor__MemOS
# 3  Muvon/octocode                 Muvon__octocode
# 4  Phoenixrr2113/codebase-graph   Phoenixrr2113__codebase-graph
# 5  abhigyanpatwari/GitNexus       abhigyanpatwari__GitNexus
# 6  caura-ai/caura                 caura-ai__caura
# 7  colbymchenry/codegraph         colbymchenry__codegraph
# 8  cq27-dev/rag-rat               cq27-dev__rag-rat
# 9  ohad6k/emulo                   emulo
# 10 getzep/graphiti                getzep__graphiti
# 11 Glitch-Cat-Club/graph-memory-starter  graph-memory-starter
# 12 github/codeql                  github__codeql
# 13 vectorize-io/hindsight         hindsight
# 14 plastic-labs/honcho            honcho
# 15 intuit/infigraph              intuit__infigraph
# 16 luuuc/sense                   luuuc__sense
# 17 alibaizhanov/mengram          mengram
# 18 mnemon-dev/mnemon             mnemon
# 19 mnemosyne-oss/mnemosyne       mnemosyne-oss__mnemosyne
# 20 oraios/serena                 oraios__serena
# 21 potpie-ai/potpie              potpie-ai__potpie
# 22 qualixar/superlocalmemory     qualixar__superlocalmemory
# 23 supermemoryai/supermemory     supermemoryai__supermemory
# 24 volcengine/OpenViking         volcengine__OpenViking
# 25 joernio/joern                 joernio__joern
# 26 oracle/opengrok               oracle__opengrok

================================================================
## REPO #1 — Egonex-AI__Understand-Anything (remote: https://github.com/Egonex-AI/Understand-Anything)
lang: TypeScript (Claude Code / Cursor / Copilot plugin + React dashboard)
### Ingestion & Indexing
- Generate language/framework-aware ignore filters to scope what gets analyzed — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/ignore-filter.ts:createIgnoreFilter
- Emit a starter `.understand-ignore` config file — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/ignore-generator.ts:generateStarterIgnoreFile
- Scan a project and orchestrate batch graph builds — Egonex-AI__Understand-Anything/understand-anything-plugin/skills/understand/scan-project.mjs
### Parsing & Chunking
- Parse source code via tree-sitter with a pluggable analyzer plugin — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/plugins/tree-sitter-plugin.ts:TreeSitterPlugin
- Run built-in language extractors (symbol/structure extraction) — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/plugins/extractors/index.ts:builtinExtractors
- Parse non-code artifacts (Markdown, YAML, JSON, TOML, Env, Dockerfile, SQL, GraphQL, Protobuf, Terraform, Makefile, Shell) — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/plugins/parsers/index.ts:registerAllParsers
- Register tree-sitter configs for 16+ languages — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/languages/language-registry.ts:LanguageRegistry
### Symbol/Entity Extraction
- Extract function/class/import fingerprints from files — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/fingerprint.ts:extractFileFingerprint
- Build symbol/structure graphs from parsed source — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/plugins/tree-sitter-plugin.ts:TreeSitterPlugin
### Graph Construction
- Build a knowledge graph from analyzed files — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/graph-builder.ts:GraphBuilder
- Normalize/merge batch graph outputs — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/normalize-graph.ts:normalizeBatchOutput
- Validate, sanitize, and auto-fix a knowledge graph against its schema — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/schema.ts:validateGraph
### Embeddings & Vector Search
- Semantic/embedding similarity search over graph nodes — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/embedding-search.ts:SemanticSearchEngine
- Compute cosine similarity for ranking — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/embedding-search.ts:cosineSimilarity
### Keyword/Lexical Search
- Lexical/fuzzy search over the knowledge graph (fuse.js) — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/search.ts:SearchEngine
### Code Navigation (LSP/refs/defs)
- Generate an onboarding tour / heuristic walkthrough of a codebase — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/tour-generator.ts:generateHeuristicTour
### Code Analysis (static/dataflow/taint)
- Compute file/function fingerprints and classify changes (added/modified/removed) — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/fingerprint.ts:analyzeChanges
- Detect architectural layers in a project — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/layer-detector.ts:detectLayers
- Decide whether a change requires a graph update — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/change-classifier.ts:classifyUpdate
### Memory Model & Types
- Define the knowledge-graph node/edge type schema — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/schema.ts:KnowledgeGraphSchema
- Core graph/analysis TypeScript types — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/types.ts
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Track graph freshness and detect staleness vs. source — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/staleness.ts:getGraphFreshness
- Merge incremental graph updates — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/staleness.ts:mergeGraphUpdate
### Temporal Reasoning
- Determine which files changed and whether the graph is stale — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/staleness.ts:isStale
### Persistence & Storage Backends
- Persist/load knowledge graphs and analysis state — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/persistence/index.ts
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- Expose as Claude Code plugin (skills + agents) — Egonex-AI__Understand-Anything/.claude-plugin/plugin.json
- Expose as Cursor plugin — Egonex-AI__Understand-Anything/.cursor-plugin/plugin.json
- Expose as GitHub Copilot plugin — Egonex-AI__Understand-Anything/.copilot-plugin/plugin.json
- Skill command scripts — Egonex-AI__Understand-Anything/understand-anything-plugin/skills/*/SKILL.md
### LLM & Agent Integration
- Build LLM prompts and parse responses for file/project analysis — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/llm-analyzer.ts:buildFileAnalysisPrompt
- Generate layer-detection and language-lesson LLM prompts — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/analyzer/layer-detector.ts:buildLayerDetectionPrompt
- Ship agent definitions — Egonex-AI__Understand-Anything/understand-anything-plugin/agents/*.md
### IDE / Editor / Chat / CI Integration
- Distributed as editor plugins for Claude Code / Cursor / Copilot — Egonex-AI__Understand-Anything/.claude-plugin/plugin.json
### Web UI & Visualization
- React dashboard for exploring the knowledge graph — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/dashboard/src/App.tsx
- Figma-design ingestion, merge, and thumbnail extraction — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/figma/merge.ts
### Extensibility (plugins/providers/custom)
- Register custom analyzer/parser plugins — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/plugins/registry.ts:PluginRegistry
- Parse and serialize plugin configuration — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/plugins/discovery.ts:parsePluginConfig
- Register language/framework configs — Egonex-AI__Understand-Anything/understand-anything-plugin/packages/core/src/languages/index.ts:FrameworkRegistry
### Ops & Deployment
- Installer scripts for the plugin — Egonex-AI__Understand-Anything/install.sh

================================================================
## REPO #2 — MemTensor__MemOS (remote: https://github.com/MemTensor/MemOS) 
lang: Python (FastAPI + MCP server; agent/plugin apps in TypeScript)
### Ingestion & Indexing
- Add memories from text, documents, or chat messages — MemTensor__MemOS/src/memos/api/mcp_serve.py:add_memory
- Read/parse documents into memories — MemTensor__MemOS/src/memos/mem_reader/ ; MemTensor__MemOS/src/memos/parsers/markitdown.py
- Index/auto-register a memory cube — MemTensor__MemOS/src/memos/api/mcp_serve.py:register_cube
### Parsing & Chunking
- Parse documents with MarkItDown — MemTensor__MemOS/src/memos/parsers/markitdown.py:MarkItDownParser
- Chunk text via character/sentence/markdown/simple strategies — MemTensor__MemOS/src/memos/chunkers/factory.py
### Graph Construction
- Persist memory as a property graph in Neo4j / Neo4j-Community / PolarDB / Postgres — MemTensor__MemOS/src/memos/graph_dbs/factory.py
- Organize textual memory into a relation graph — MemTensor__MemOS/src/memos/memories/textual/tree_text_memory/organize/reorganizer.py:GraphStructureReorganizer
- Detect relations and reasoning between memory nodes — MemTensor__MemOS/src/memos/memories/textual/tree_text_memory/organize/relation_reason_detector.py:RelationAndReasoningDetector
### Embeddings & Vector Search
- Generate embeddings via Ark / Ollama / Sentence-Transformer / Universal-API — MemTensor__MemOS/src/memos/embedders/factory.py
- Store/query vector memories in Qdrant or Milvus — MemTensor__MemOS/src/memos/vec_dbs/factory.py
- Semantic search across textual memories — MemTensor__MemOS/src/memos/search/search_service.py:search_text_memories
### Keyword/Lexical Search
- BM25 lexical retrieval over memory — MemTensor__MemOS/src/memos/memories/textual/tree_text_memory/retrieve/bm25_util.py:EnhancedBM25
### Hybrid & Ranked Retrieval
- Rerank retrieved memories — MemTensor__MemOS/src/memos/memories/textual/tree_text_memory/retrieve/reranker.py:MemoryReranker
- Pluggable rerankers (concat / cosine-local / http-bge / noop) — MemTensor__MemOS/src/memos/reranker/factory.py
### Memory Model & Types
- Textual memory types (General / Naive / Tree / Preference / SimpleTree) — MemTensor__MemOS/src/memos/memories/textual/
- Activation memory (KV-cache, vLLM KV-cache) — MemTensor__MemOS/src/memos/memories/activation/
- Parametric memory (LoRA) — MemTensor__MemOS/src/memos/memories/parametric/lora.py:LoRAMemory
- Unified memory factory — MemTensor__MemOS/src/memos/memories/factory.py:MemoryFactory
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Dream/consolidation: contextualize memory groups — MemTensor__MemOS/src/memos/dream/contextualization.py:DreamContextualizer
- Heuristic memory enrichment — MemTensor__MemOS/src/memos/dream/enrichment.py:DreamHeuristicEnricher
- Consolidation pipelines (diary summary, motive formation, reasoning, recall) — MemTensor__MemOS/src/memos/dream/pipeline/
- Background memory scheduler (start/stop) — MemTensor__MemOS/src/memos/api/mcp_serve.py:control_memory_scheduler
- Soft-delete and recover memory nodes — MemTensor__MemOS/src/memos/api/routers/server_router.py:/delete_memory_by_record_id
### Temporal Reasoning
- Track dream signals / lifecycle snapshots — MemTensor__MemOS/src/memos/dream/signal_store.py:DreamSignalStore
### Multi-tenant / Scoping / Namespaces
- Create users with USER/ADMIN roles — MemTensor__MemOS/src/memos/api/mcp_serve.py:create_user
- Create and share memory cubes per user — MemTensor__MemOS/src/memos/api/mcp_serve.py:create_cube
- Persistent multi-user management (MySQL / Redis / file) — MemTensor__MemOS/src/memos/mem_user/
### Persistence & Storage Backends
- Vector backends Qdrant/Milvus — MemTensor__MemOS/src/memos/vec_dbs/
- Graph backends Neo4j/Neo4j-Community/PolarDB/Postgres — MemTensor__MemOS/src/memos/graph_dbs/
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server exposing 16 tools — MemTensor__MemOS/src/memos/api/mcp_serve.py:MOSMCPServer
- REST product API — MemTensor__MemOS/src/memos/api/routers/server_router.py:APIRouter
- Admin REST API (API-key + master-key management) — MemTensor__MemOS/src/memos/api/routers/admin_router.py:APIRouter
- CLI — MemTensor__MemOS/src/memos/cli.py:main
### LLM & Agent Integration
- LLM providers (OpenAI, DeepSeek, Qwen, MiniMax, Ollama, HF, vLLM) — MemTensor__MemOS/src/memos/llms/factory.py
- Deep-search agent — MemTensor__MemOS/src/memos/mem_agent/deepsearch_agent.py
- Memory-enhanced chat — MemTensor__MemOS/src/memos/api/mcp_serve.py:chat
### IDE / Editor / Chat / CI Integration
- Agent/editor integration apps (OpenClaw plugins, local plugin, openwork) — MemTensor__MemOS/apps/
### Web UI & Visualization
- Memory dashboard data endpoint — MemTensor__MemOS/src/memos/api/routers/server_router.py:/get_memory_dashboard
### Security & Access Control
- API-key auth (hashing, lookup, Bearer/Token parsing, scopes) — MemTensor__MemOS/src/memos/api/middleware/auth.py:verify_api_key
- Rate limiting middleware — MemTensor__MemOS/src/memos/api/middleware/rate_limit.py
- Admin API-key CRUD — MemTensor__MemOS/src/memos/api/routers/admin_router.py:create_key
### Observability & Evaluation
- Structured logging — MemTensor__MemOS/src/memos/log.py
- Evaluation harness — MemTensor__MemOS/evaluation/
### Extensibility (plugins/providers/custom)
- Plugin manager + hook system — MemTensor__MemOS/src/memos/plugins/manager.py:PluginManager
- Pluggable providers via factories (embedders, llms, vec_dbs, graph_dbs, reranker, chunkers, parsers) — MemTensor__MemOS/src/memos/
### Ops & Deployment
- Container/deploy assets — MemTensor__MemOS/Dockerfile
- Background scheduler service — MemTensor__MemOS/src/memos/mem_scheduler/

================================================================
## REPO #3 — Muvon__octocode (remote: https://github.com/Muvon/octocode)
lang: Rust (with rmcp MCP server; embedding/LLM logic in `octolib` dependency)
### Ingestion & Indexing
- Index the current codebase — Muvon__octocode/src/commands/index.rs:execute
- Watch and auto-reindex on change — Muvon__octocode/src/commands/watch.rs:execute
- Differential / branch-delta indexing — Muvon__octocode/src/indexer/differential_processor.rs
- Index git commit history — Muvon__octocode/src/indexer/commits/mod.rs
### Parsing & Chunking
- Tree-sitter parsing across 20+ languages — Muvon__octocode/Cargo.toml (tree-sitter-* deps); Muvon__octocode/src/indexer/languages/
- Code-region / signature / markdown extractors — Muvon__octocode/src/indexer/code_region_extractor.rs
### Symbol/Entity Extraction
- Extract function/method signatures — Muvon__octocode/src/mcp/server.rs:view_signatures
- List document/workspace symbols via LSP — Muvon__octocode/src/mcp/server.rs:lsp_document_symbols
### Graph Construction
- Build a code relationship graph (GraphRAG) — Muvon__octocode/src/indexer/graphrag/builder.rs:GraphBuilder
- Query graph relationships (calls/imports/inheritance/neighbors/paths) — Muvon__octocode/src/mcp/server.rs:graphrag
### Embeddings & Vector Search
- Generate code/text embeddings via provider — Muvon__octocode/src/embedding/mod.rs:generate_embeddings
- Semantic (embedding) code search — Muvon__octocode/src/mcp/server.rs:semantic_search
### Keyword/Lexical Search
- AST/structural pattern search (ast-grep) — Muvon__octocode/src/commands/grep.rs:execute
- Natural-language search command — Muvon__octocode/src/commands/search.rs:execute
### Hybrid & Ranked Retrieval
- Rerank code/text/doc/commit blocks — Muvon__octocode/src/reranker.rs:rerank_code_blocks_with_octolib
### Code Navigation (LSP/refs/defs)
- LSP go-to-definition / hover / find-references — Muvon__octocode/src/mcp/server.rs:lsp_goto_definition
- LSP completions — Muvon__octocode/src/mcp/server.rs:lsp_completion
### Code Analysis (static/dataflow/taint)
- AI code explanation — Muvon__octocode/src/commands/explain.rs:execute
- AI behavioral diff summary — Muvon__octocode/src/commands/diff.rs:execute
- AI code review — Muvon__octocode/src/commands/review.rs:execute
- AI commit generation — Muvon__octocode/src/commands/commit.rs:execute
- AI release/changelog generation — Muvon__octocode/src/commands/release.rs:execute
### Persistence & Storage Backends
- Vector store on LanceDB (code/text/doc/commit blocks) — Muvon__octocode/src/store/batch_converter.rs
- Export/import dataset archive — Muvon__octocode/src/commands/export.rs:execute
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server (rmcp) exposing 10 tools — Muvon__octocode/src/mcp/server.rs:McpServer
- CLI (clap) subcommands — Muvon__octocode/src/main.rs:Commands
### LLM & Agent Integration
- Embedding + LLM reasoning for explain/diff/review/commit/release — Muvon__octocode/src/llm/reasoning.rs
### Extensibility (plugins/providers/custom)
- Pluggable embedding providers — Muvon__octocode/src/embedding/mod.rs:EmbeddingGenerationConfig
### Ops & Deployment
- Dockerfile + install.sh — Muvon__octocode/Dockerfile

================================================================
## REPO #4 — Phoenixrr2113__codebase-graph (remote: https://github.com/Phoenixrr2113/codebase-graph)
lang: TypeScript (monorepo: Hono REST, MCP server, CLI, React dashboard; Neo4j-compatible graph via FalkorDB)
### Ingestion & Indexing
- Ingest documents/files into the graph — Phoenixrr2113__codebase-graph/packages/core/src/documentIngestion.ts:add
- Sync git history with a history window — Phoenixrr2113__codebase-graph/packages/core/src/gitSync.ts:syncGitHistory
- Extract/parse a project — Phoenixrr2113__codebase-graph/packages/cli/src/commands/extract.ts
### Parsing & Chunking
- Language-specific parsers — Phoenixrr2113__codebase-graph/packages/plugin-*/
- Document ingestion pipeline — Phoenixrr2113__codebase-graph/packages/core/src/documentIngestion.ts
### Symbol/Entity Extraction
- Per-language symbol/entity extraction plugins — Phoenixrr2113__codebase-graph/packages/plugin-languages/
- Graph knowledge operations — Phoenixrr2113__codebase-graph/packages/graph/src/knowledge-operations.ts
### Graph Construction
- Build/store a code knowledge graph — Phoenixrr2113__codebase-graph/packages/graph/src/operations.ts
- Query the graph via Cypher — Phoenixrr2113__codebase-graph/packages/mcp-server/src/tools/queryGraph.ts
- Pluggable graph DB driver (FalkorDB / FalkorDB-Lite / Neo4j / Memgraph / LanceDB) — Phoenixrr2113__codebase-graph/packages/graph/src/driver-registry.ts:registerDriver
### Embeddings & Vector Search
- Embed all graph nodes — Phoenixrr2113__codebase-graph/packages/core/src/embed-nodes.ts:embedAllNodes
- Scheduled embedding pass — Phoenixrr2113__codebase-graph/packages/core/src/embed-pass.ts:scheduleEmbeddingPass
### Keyword/Lexical Search
- Code search — Phoenixrr2113__codebase-graph/packages/mcp-server/src/tools/searchCode.ts
### Hybrid & Ranked Retrieval
- Reciprocal-rank-fusion hybrid retrieval — Phoenixrr2113__codebase-graph/packages/core/src/enrichedSearchV2.ts:rrfFuse
- Graph-enriched retrieval — Phoenixrr2113__codebase-graph/packages/core/src/enrichedSearchV2.ts:enrichFromGraph
### Code Navigation (LSP/refs/defs)
- Graph neighbor/reference/dependency traversal — Phoenixrr2113__codebase-graph/packages/api/src/routes/graph.ts
- Get contextual subgraph for a file/symbol — Phoenixrr2113__codebase-graph/packages/mcp-server/src/tools/getContext.ts
### Code Analysis (static/dataflow/taint)
- Static analyses: blast-radius, call-hierarchy, import-cycles, dead-code, hotspots, change-coupling, ownership — Phoenixrr2113__codebase-graph/packages/api/src/routes/analysis.ts
- Analyze command + analyze persona — Phoenixrr2113__codebase-graph/packages/cli/src/commands/analyze.ts
### Memory Model & Types
- Knowledge-graph entities/operations — Phoenixrr2113__codebase-graph/packages/graph/src/knowledge-operations.ts
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Incremental/full reindex — Phoenixrr2113__codebase-graph/packages/mcp-server/src/tools/reindex.ts:triggerReindex
- Staleness detection and background re-index — Phoenixrr2113__codebase-graph/packages/mcp-server/src/tools/router.ts:checkAndTriggerStalenessReindex
### Temporal Reasoning
- Git history window — Phoenixrr2113__codebase-graph/packages/core/src/gitSync.ts:syncGitHistory
### Multi-tenant / Scoping / Namespaces
- Configure/select active projects (scoped search) — Phoenixrr2113__codebase-graph/packages/mcp-server/src/tools/configureProjects.ts
### Persistence & Storage Backends
- Graph storage on FalkorDB / FalkorDB-Lite — Phoenixrr2113__codebase-graph/packages/graph/src/drivers/falkordb.ts
- Pluggable driver registry — Phoenixrr2113__codebase-graph/packages/graph/src/driver-registry.ts:createDriver
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server (default persona tools + raw-mode tools) — Phoenixrr2113__codebase-graph/packages/mcp-server/src/server.ts:CodeGraphMcpServer
- REST API (Hono) — Phoenixrr2113__codebase-graph/packages/api/src/routes/*.ts
- CLI (commander) — Phoenixrr2113__codebase-graph/packages/cli/src/cli.ts
### LLM & Agent Integration
- Chain-of-thought + enriched LLM retrieval — Phoenixrr2113__codebase-graph/packages/core/src/cotSearch.ts
- AI personas — Phoenixrr2113__codebase-graph/packages/mcp-server/src/personas/
### IDE / Editor / Chat / CI Integration
- MCP server for AI coding agents — Phoenixrr2113__codebase-graph/packages/mcp-server/src/server.ts
### Web UI & Visualization
- React dashboard — Phoenixrr2113__codebase-graph/packages/dashboard/
### Security & Access Control
- CSRF guard for the web API — Phoenixrr2113__codebase-graph/packages/api/src/csrf-guard.ts
- Source-access controls — Phoenixrr2113__codebase-graph/packages/api/src/source-access.ts
### Extensibility (plugins/providers/custom)
- Pluggable graph DB drivers — Phoenixrr2113__codebase-graph/packages/graph/src/driver-registry.ts:registerDriver
### Ops & Deployment
- Docker Compose + serve command — Phoenixrr2113__codebase-graph/docker-compose.yml

================================================================
## REPO #5 — abhigyanpatwari__GitNexus (remote: https://github.com/abhigyanpatwari/GitNexus)
lang: TypeScript / JavaScript (Node.js), tree-sitter via WASM
### Ingestion & Indexing
- Index a repository with full structural analysis (and watch mode) — abhigyanpatwari__GitNexus/gitnexus/src/cli/index.ts:analyze
- Incrementally (re)index one or more paths — abhigyanpatwari__GitNexus/gitnexus/src/cli/index.ts:index
- Ingest source through a tree-sitter pipeline — abhigyanpatwari__GitNexus/gitnexus/src/core/ingestion/pipeline.ts:pipeline
### Parsing & Chunking
- Tree-sitter extraction of symbols/edges — abhigyanpatwari__GitNexus/gitnexus/src/core/ingestion/tree-sitter-queries.ts
- Chunk symbol nodes for embedding — abhigyanpatwari__GitNexus/gitnexus/src/core/embeddings/chunker.ts:chunkNode
### Symbol/Entity Extraction
- Extract callable/type/field symbols and references — abhigyanpatwari__GitNexus/gitnexus/src/core/ingestion/workers/parse-worker.ts:parse-worker
- 360° symbol reference resolution (callers/callees/imports/overrides) — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:context
### Graph Construction
- Build the code knowledge graph — abhigyanpatwari__GitNexus/gitnexus/src/core/graph/graph.ts:createKnowledgeGraph
- Detect import cycles — abhigyanpatwari__GitNexus/gitnexus/src/core/graph/import-cycles.ts
- Auto-detect functional-area communities (Leiden) — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:cypher
### Embeddings & Vector Search
- Generate embeddings via transformers.js / ONNX runtime — abhigyanpatwari__GitNexus/gitnexus/src/core/embeddings/embedder.ts:initEmbedder
- Exact (no-index) semantic lookup — abhigyanpatwari__GitNexus/gitnexus/src/core/embeddings/exact-search.ts
### Keyword/Lexical Search
- BM25 full-text search over the index — abhigyanpatwari__GitNexus/gitnexus/src/core/search/bm25-index.ts:searchFTSFromLbug
### Hybrid & Ranked Retrieval
- Reciprocal-Rank-Fusion hybrid (BM25 + semantic) — abhigyanpatwari__GitNexus/gitnexus/src/core/search/hybrid-search.ts:mergeWithRRF
### Code Navigation (LSP/refs/defs)
- Caller/callee/impact tracing over the graph — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:impact
- CLI callers/callees/impact/node — abhigyanpatwari__GitNexus/gitnexus/src/cli/index.ts:callers
### Code Analysis (static/dataflow/taint)
- Program-dependence-graph dataflow + taint analysis — abhigyanpatwari__GitNexus/gitnexus/src/mcp/local/pdg-impact.ts:pdgStampForMode
- Taint call-summary codec — abhigyanpatwari__GitNexus/gitnexus/src/core/ingestion/taint/call-summary-codec.ts
- PDG query tool — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:pdg_query
- HTTP route / tool / schema / API-impact mapping — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:route_map
### Temporal Reasoning
- Detect impact of unstaged/staged/compare git changes — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:detect_changes
### Multi-tenant / Scoping / Namespaces
- Cross-repo group indexing, listing, sync — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:group_list
- Per-repo / group scoping policy — abhigyanpatwari__GitNexus/gitnexus/src/mcp/repository-policy.ts:RepositoryPolicy
### Persistence & Storage Backends
- Local embedded graph store (custom lbug store) — abhigyanpatwari__GitNexus/gitnexus/src/core/lbug/lbug-adapter.ts
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server exposing 17 tools — abhigyanpatwari__GitNexus/gitnexus/src/mcp/tools.ts:GITNEXUS_TOOLS
- REST HTTP API — abhigyanpatwari__GitNexus/gitnexus/src/server/api.ts:/api/query
- CLI surface — abhigyanpatwari__GitNexus/gitnexus/src/cli/index.ts
### LLM & Agent Integration
- LLM-augmented graph enrichment — abhigyanpatwari__GitNexus/gitnexus/src/core/augmentation/engine.ts:augment
### IDE / Editor / Chat / CI Integration
- Claude Code / Cursor / OpenCode / Codex / Qoder plugins — abhigyanpatwari__GitNexus/gitnexus/src/cli/index.ts:setup
### Web UI & Visualization
- Web UI with graph canvas — abhigyanpatwari__GitNexus/gitnexus-web/src/components/GraphCanvas.tsx
### Security & Access Control
- Read-only tool/resource allow-list — abhigyanpatwari__GitNexus/gitnexus/src/mcp/read-only-policy.ts:MCP_READ_ONLY_TOOLS
- Repository-scoped access policy — abhigyanpatwari__GitNexus/gitnexus/src/mcp/repository-policy.ts
### Observability & Evaluation
- Eval server + benchmark harnesses — abhigyanpatwari__GitNexus/gitnexus/src/cli/index.ts:eval-server
### Extensibility (plugins/providers/custom)
- Custom extraction via augment patterns — abhigyanpatwari__GitNexus/gitnexus/src/cli/augment.ts
### Ops & Deployment
- Docker / Render / compose deployment — abhigyanpatwari__GitNexus/gitnexus/docker-compose.yaml

================================================================
## REPO #6 — caura-ai__caura (remote: https://github.com/caura-ai/caura)
lang: Python (FastAPI services) + TypeScript (OpenClaw plugin)
### Ingestion & Indexing
- Ingest documents with preview/commit/file undo — caura-ai__caura/core-api/src/core_api/routes/memories.py:/ingest/preview
- Chunked ingest pipeline — caura-ai__caura/core-api/src/core_api/services/ingest_chunking.py
### Parsing & Chunking
- Document chunking/splitting — caura-ai__caura/core-api/src/core_api/services/ingest_chunking.py
### Symbol/Entity Extraction
- Entity extraction + worker — caura-ai__caura/core-api/src/core_api/services/entity_extraction.py
- Entity linking / relation inference — caura-ai__caura/core-storage-api/src/core_storage_api/routers/entities.py:/infer-relations
### Graph Construction
- Entity/relation graph storage and traversal — caura-ai__caura/core-storage-api/src/core_storage_api/routers/entities.py:/graph
### Embeddings & Vector Search
- Pluggable embedding providers (OpenAI, local, fake) — caura-ai__caura/common/embedding/providers/openai.py
- Embedding similarity search — caura-ai__caura/core-storage-api/src/core_storage_api/routers/entities.py:/embedding-similarity
### Keyword/Lexical Search
- Full-text (FTS) entity search — caura-ai__caura/core-storage-api/src/core_storage_api/routers/entities.py:/fts-search
### Hybrid & Ranked Retrieval
- Memory recall (hybrid search) — caura-ai__caura/core-api/src/core_api/routes/memories.py:/search
### Memory Model & Types
- Memory write/create model — caura-ai__caura/core-api/src/core_api/mcp_server.py:caura_write
- Short-term memory stores (in-memory / redis / sqlite) — caura-ai__caura/core-api/src/core_api/providers/inmemory_stm.py
- Keystones (core memories) — caura-ai__caura/core-api/src/core_api/routes/keystones.py:/keystones
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Crystallize memories into reports — caura-ai__caura/core-api/src/core_api/routes/crystallizer.py:/crystallize
- Memory evolution/scoring — caura-ai__caura/core-api/src/core_api/routes/evolve.py:/evolve/report
- Lifecycle archive/purge/stale — caura-ai__caura/core-storage-api/src/core_storage_api/routers/memories.py:/archive-expired
- Skill promotion lifecycle — caura-ai__caura/core-api/src/core_api/services/skill_promoter.py
### Temporal Reasoning
- Session trace capture — caura-ai__caura/core-api/src/core_api/services/session_trace.py
### Multi-tenant / Scoping / Namespaces
- Tenant context scoping — caura-ai__caura/core-api/src/core_api/tenant_context.py
- Fleet (multi-node) scoping — caura-ai__caura/core-api/src/core_api/routes/fleet.py:/fleet
### Persistence & Storage Backends
- Postgres + pgvector primary store — caura-ai__caura/core-storage-api/src/core_storage_api/services/postgres_service.py
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server with 12 tools — caura-ai__caura/core-api/src/core_api/mcp_server.py:caura_recall
- REST API — caura-ai__caura/core-api/src/core_api/routes/memories.py:/memories
- Worker CLI — caura-ai__caura/core-worker/src/core_worker/cli.py:backfill-embeddings
### LLM & Agent Integration
- Pluggable LLM providers (OpenAI, Gemini, Vertex, fake) — caura-ai__caura/common/llm/providers/openai.py
- Agent identity/interview/trust — caura-ai__caura/core-api/src/core_api/services/agent_service.py
### IDE / Editor / Chat / CI Integration
- OpenClaw plugin + SDK clients — caura-ai__caura/plugin/src/index.ts
### Security & Access Control
- Auth + agent trust — caura-ai__caura/core-api/src/core_api/auth.py
- Governance gate + PII patterns — caura-ai__caura/core-api/src/core_api/services/governance_gate.py
### Observability & Evaluation
- Observability + rate-limit/usage middleware — caura-ai__caura/core-storage-api/src/core_storage_api/observability.py
### Extensibility (plugins/providers/custom)
- Provider registries (embedding/llm/ranking) — caura-ai__caura/common/embedding/_registry.py
### Ops & Deployment
- Docker compose / Render deploy — caura-ai__caura/docker-compose.yml

================================================================
## REPO #7 — colbymchenry__codegraph (remote: https://github.com/colbymchenry/codegraph)
lang: TypeScript + Rust (codegraph-kernel tree-sitter)
### Ingestion & Indexing
- Initialize / index / sync a repository — colbymchenry__codegraph/src/bin/codegraph.ts:init
- Git-hook-driven incremental reindex — colbymchenry__codegraph/src/sync/git-hooks.ts
### Parsing & Chunking
- Tree-sitter extraction of symbols/edges (multi-language) — colbymchenry__codegraph/src/extraction/tree-sitter.ts:extractFromSource
### Symbol/Entity Extraction
- Per-language/framework extractors — colbymchenry__codegraph/src/extraction/index.ts
- Grammar registry (WASM grammars) — colbymchenry__codegraph/src/extraction/grammars.ts:WASM_GRAMMAR_FILES
### Graph Construction
- Graph queries and traversal — colbymchenry__codegraph/src/graph/queries.ts
- Type-hierarchy resolution — colbymchenry__codegraph/src/graph/type-hierarchy.ts
### Keyword/Lexical Search
- FTS5 full-text search with bm25 ranking — colbymchenry__codegraph/src/db/queries.ts:nodes_fts
### Code Navigation (LSP/refs/defs)
- MCP search/callers/callees/impact/node — colbymchenry__codegraph/src/mcp/tools.ts:codegraph_search
- CLI callers/callees/impact/affected — colbymchenry__codegraph/src/bin/codegraph.ts:callers
### Code Analysis (static/dataflow/taint)
- Dead-code detection — colbymchenry__codegraph/src/graph/dead-code.ts
### Persistence & Storage Backends
- Embedded SQLite (node:sqlite) with WAL — colbymchenry__codegraph/src/db/index.ts:SqliteBackend
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server, 8 tools — colbymchenry__codegraph/src/mcp/tools.ts:tools
- UI server (HTTP API) — colbymchenry__codegraph/src/ui-server/index.ts
- CLI — colbymchenry__codegraph/src/bin/codegraph.ts
### IDE / Editor / Chat / CI Integration
- Git hooks + prompt-hook integration — colbymchenry__codegraph/src/sync/git-hooks.ts
### Web UI & Visualization
- Web UI server + UI assets — colbymchenry__codegraph/src/ui-server
### Observability & Evaluation
- Telemetry subsystem — colbymchenry__codegraph/src/telemetry
### Extensibility (plugins/providers/custom)
- Multi-language extractor framework — colbymchenry__codegraph/src/extraction/languages
### Ops & Deployment
- Installer scripts (cross-platform) — colbymchenry__codegraph/install.sh

================================================================
## REPO #8 — cq27-dev__rag-rat (remote: https://github.com/cq27-dev/rag-rat)
lang: Rust
### Ingestion & Indexing
- Index repo (changed files / full) — cq27-dev__rag-rat/crates/rag-rat-cli/src/cli.rs:Index
- On-device embedding reconciliation — cq27-dev__rag-rat/crates/rag-rat-cli/src/cli.rs:Reconcile
### Parsing & Chunking
- Language-indexer / SCIP oracle ingestion — cq27-dev__rag-rat/crates/rag-rat-oracle/src/scip.rs
- Chunk text store + compression — cq27-dev__rag-rat/crates/rag-rat-db/src/chunk_text_store.rs
### Symbol/Entity Extraction
- Symbol resolution/selector — cq27-dev__rag-rat/crates/rag-rat-query/src/symbol.rs
- Library-usage extraction — cq27-dev__rag-rat/crates/rag-rat-oracle/src/library_usage.rs
### Graph Construction
- Code graph resolution/traversal — cq27-dev__rag-rat/crates/rag-rat-query/src/graph
### Embeddings & Vector Search
- On-device embeddings (FastEmbed) — cq27-dev__rag-rat/crates/rag-rat-llm/src/fastembed.rs:FastEmbedEmbedder
- Remote embeddings (OpenAI) + model2vec — cq27-dev__rag-rat/crates/rag-rat-llm/src/openai.rs
- Semantic search tool — cq27-dev__rag-rat/crates/rag-rat-mcp/src/tools/catalog.rs:semantic_search
### Keyword/Lexical Search
- Lexical search — cq27-dev__rag-rat/crates/rag-rat-core/src/search/lexical/mod.rs:search
### Hybrid & Ranked Retrieval
- Reciprocal-rank-fusion hybrid — cq27-dev__rag-rat/crates/rag-rat-core/src/search/hybrid.rs:reciprocal_rank_fusion
### Code Navigation (LSP/refs/defs)
- Callers / callees / symbol lookup — cq27-dev__rag-rat/crates/rag-rat-mcp/src/tools/catalog.rs:find_callers
### Code Analysis (static/dataflow/taint)
- Clone detection (coupling/refactor ROI) — cq27-dev__rag-rat/crates/rag-rat-query/src/coupling.rs
- Impact-surface analysis — cq27-dev__rag-rat/crates/rag-rat-mcp/src/tools/catalog.rs:impact_surface
- Load-bearing symbols via PageRank — cq27-dev__rag-rat/crates/rag-rat-query/src/pagerank.rs
- FFI surface extraction — cq27-dev__rag-rat/crates/rag-rat-mcp/src/tools/catalog.rs:ffi_surface
### Memory Model & Types
- Source-anchored repo memories (create/rebind/update/edges) — cq27-dev__rag-rat/crates/rag-rat-mcp/src/tools/catalog.rs:memory_create
- Memory write module — cq27-dev__rag-rat/crates/rag-rat-core/src/memory_write
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Dream-mode maintenance worklist (gap/stale findings) — cq27-dev__rag-rat/crates/rag-rat-dream/src/compact.rs
- Mark obsolete / validate memories — cq27-dev__rag-rat/crates/rag-rat-mcp/src/tools/catalog.rs:memory_mark_obsolete
### Temporal Reasoning
- Commit/git-history/blame search — cq27-dev__rag-rat/crates/rag-rat-mcp/src/tools/catalog.rs:commit_search
- Tracker papertrail (issue/commit linkage) — cq27-dev__rag-rat/crates/rag-rat-mcp/src/tools/catalog.rs:papertrail_for_chunk
### Multi-tenant / Scoping / Namespaces
- Sync/fleet enrollment + auth — cq27-dev__rag-rat/crates/rag-rat-sync/src/auth.rs
- Op-log identity/account/project scoping — cq27-dev__rag-rat/crates/rag-rat-oplog/src/identity.rs
### Persistence & Storage Backends
- SQLite (rusqlite) index storage — cq27-dev__rag-rat/crates/rag-rat-db/src/storage.rs:IndexConnection
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server, 47 tools — cq27-dev__rag-rat/crates/rag-rat-mcp/src/tools/catalog.rs:TOOL_NAMES
- Authenticated editor Lens HTTP API — cq27-dev__rag-rat/crates/rag-rat-mcp/src/lens_server.rs:serve_standalone
- P2P sync protocol — cq27-dev__rag-rat/crates/rag-rat-sync/src/wire.rs
- CLI — cq27-dev__rag-rat/crates/rag-rat-cli/src/cli.rs:Command
### LLM & Agent Integration
- LLM chat/providers — cq27-dev__rag-rat/crates/rag-rat-llm/src/chat.rs
- Dream-mode LLM verdict prompts — cq27-dev__rag-rat/crates/rag-rat-dream/src/prompts
### IDE / Editor / Chat / CI Integration
- Claude Code / git hooks + editor plugins (VSCode, appliance) — cq27-dev__rag-rat/crates/rag-rat-cli/src/cli.rs:Hooks
### Security & Access Control
- Sync auth + op-log audit trail — cq27-dev__rag-rat/crates/rag-rat-sync/src/auth.rs
- Lens origin allow-list — cq27-dev__rag-rat/crates/rag-rat-cli/src/cli.rs:--allow-origin
### Observability & Evaluation
- Dream findings + eval suite + embedding benchmark — cq27-dev__rag-rat/crates/rag-rat-core/src/eval.rs
### Extensibility (plugins/providers/custom)
- Oracle backends (rust-analyzer / SCIP / custom) — cq27-dev__rag-rat/crates/rag-rat-oracle/src/backend
### Ops & Deployment
- Container / distribution tooling — cq27-dev__rag-rat/glama.Dockerfile

================================================================
## REPO #9 — emulo (remote: https://github.com/ohad6k/emulo)
lang: Python
### Ingestion & Indexing
- Ingest local AI coding session logs — emulo.py:discover_files
### Parsing & Chunking
- Mine session logs into normalized blocks — emulo.py:mine_files
### Memory Model & Types
- Profile domain model (work/design/write/video) — emulo.py:VALID_DOMAINS
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Review, approve, activate and roll back profile generations — emulo_autopilot/cli.py:review
- Migrate/cutover/rollback profile versions — emulo.py:plugin_main
### Persistence & Storage Backends
- Local filesystem profile/run storage in scoped private dirs — emulo.py:resolve_emulo_home
- SQLite-backed autopilot store — emulo_autopilot/store.py:AutopilotStore
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server exposing the load_emulo_profile tool — emulo.py:load_emulo_profile
- CLI mine/verify/plugin/mcp subcommands — emulo.py:main
- emulo-autopilot CLI — emulo_autopilot/cli.py:build_parser
### LLM & Agent Integration
- Load the mined user profile into the agent at task start via MCP — emulo.py:load_emulo_profile
### IDE / Editor / Chat / CI Integration
- Install mined profile into agent/IDE memory files — emulo.py:install_profile
### Web UI & Visualization
- Render a profile "card" — emulo.py:show_card
### Security & Access Control
- Redact secrets/PII from session text before mining — emulo.py:redact
- Encrypted cross-device continuity recovery kit — emulo_autopilot/cli.py:continuity-init
### Observability & Evaluation
- Verify profile quotes against source sessions — emulo.py:verify_profile
- Usage/coach report of session patterns — emulo.py:usage_report
### Extensibility (plugins/providers/custom)
- Plugin protocol + starter candidate profiles — emulo.py:plugin_main
### Ops & Deployment
- Container image — emulo/Dockerfile
- MCP server packaging manifest — emulo/server.json

================================================================
## REPO #10 — getzep__graphiti (remote: https://github.com/getzep/graphiti)
lang: Python
### Ingestion & Indexing
- Ingest episodes into the knowledge graph — getzep__graphiti/mcp_server/src/graphiti_mcp_server.py:add_memory
### Symbol/Entity Extraction
- Extract entities + relationship facts from episodes via LLM — getzep__graphiti/graphiti_core/graphiti.py:add_episode
### Graph Construction
- Detect and build community summaries over entities — getzep__graphiti/graphiti_core/graphiti.py:build_communities
- Summarize an ordered saga of related episodes — getzep__graphiti/graphiti_core/graphiti.py:summarize_saga
- Add an explicit fact triplet directly — getzep__graphiti/mcp_server/src/graphiti_mcp_server.py:add_triplet
### Embeddings & Vector Search
- Pluggable embedder providers + vector search — getzep__graphiti/graphiti_core/embedder/
### Hybrid & Ranked Retrieval
- Hybrid RRF/MMR/cross-encoder/node-distance search recipes — getzep__graphiti/graphiti_core/search/search_config_recipes.py:COMBINED_HYBRID_SEARCH_RRF
- Rerankers (bge/gemini/openai cross-encoders) — getzep__graphiti/graphiti_core/cross_encoder/
### Memory Model & Types
- Node/edge types: EntityNode, EpisodicNode, CommunityNode, SagaNode, EntityEdge — getzep__graphiti/graphiti_core/nodes.py:EntityNode
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Remove episode with cascading entity/edge cleanup — getzep__graphiti/graphiti_core/graphiti.py:remove_episode
### Temporal Reasoning
- Bi-temporal model: reference_time + valid_at/invalid_at — getzep__graphiti/mcp_server/src/graphiti_mcp_server.py:search_memory_facts
### Multi-tenant / Scoping / Namespaces
- Group-id scoping across all memory operations — getzep__graphiti/mcp_server/src/graphiti_mcp_server.py:add_memory
### Persistence & Storage Backends
- Graph DB drivers: Neo4j, FalkorDB, Kuzu, Neptune — getzep__graphiti/graphiti_core/driver/neo4j_driver.py
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server (13 tools) — getzep__graphiti/mcp_server/src/graphiti_mcp_server.py:add_memory
### LLM & Agent Integration
- Pluggable LLM providers for extraction — getzep__graphiti/graphiti_core/llm_client/
### Observability & Evaluation
- Telemetry/tracing of operations — getzep__graphiti/graphiti_core/telemetry/
### Extensibility (plugins/providers/custom)
- Custom entity/edge type config + schema migrations — getzep__graphiti/mcp_server/src/config/schema.py:EntityTypeConfig
### Ops & Deployment
- Docker/compose images — getzep__graphiti/mcp_server/Dockerfile

================================================================
## REPO #11 — graph-memory-starter (remote: https://github.com/Glitch-Cat-Club/graph-memory-starter)
lang: Python
### Ingestion & Indexing
- Load extraction/*.json into SQLite graph.db — graph-memory-starter/src/build_graph.py:main
- Chunk corpus and build FTS5 + vector index — graph-memory-starter/rag/build_index.py:main
### Parsing & Chunking
- Section/chunk document splitter — graph-memory-starter/rag/build_index.py:chunk_file
### Graph Construction
- Build entity/relation/alias graph — graph-memory-starter/src/build_graph.py:main
### Embeddings & Vector Search
- Sentence embedding + cosine vector search — graph-memory-starter/rag/build_index.py
### Keyword/Lexical Search
- SQLite FTS5 BM25 keyword search — graph-memory-starter/rag/search.py:keyword_leg
### Hybrid & Ranked Retrieval
- Reciprocal rank fusion of keyword + meaning legs — graph-memory-starter/rag/search.py:fuse
### Memory Model & Types
- Graph schema (entities/relations/aliases) — graph-memory-starter/src/schema.sql
### Persistence & Storage Backends
- SQLite storage (graph.db, rag.db, FTS5) — graph-memory-starter/src/build_graph.py
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- CLI recall/search/index tools — graph-memory-starter/src/recall.py:recall
### LLM & Agent Integration
- Distil notes into Q&A via Claude subprocess — graph-memory-starter/rag/distil.py:distil_one
### IDE / Editor / Chat / CI Integration
- Claude Code UserPromptSubmit hook injecting recalled memory — graph-memory-starter/src/recall_hook.py
### Extensibility (plugins/providers/custom)
- Custom knowledge-graph source via extraction/*.json — graph-memory-starter/extraction/*.json

================================================================
## REPO #12 — github__codeql (remote: https://github.com/github/codeql)
lang: QL (CodeQL query language) / Starlark / Python
### Code Navigation (LSP/refs/defs)
- IDE local-definitions / local-references / AST queries — github__codeql/ql/ql/src/ide-contextual-queries/localDefinitions.ql
### Code Analysis (static/dataflow/taint)
- Taint-tracking dataflow library consumed by security queries — github__codeql/python/ql/src/Security/CVE-2018-1281/BindToAllInterfaces.ql
- Per-language CWE security query packs — github__codeql/python/ql/src/Security/CWE-089/SqlInjection.ql
- Code quality / metrics / dead-code / complexity query categories — github__codeql/java/ql/src/Metrics
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- Query-pack & extension system — github__codeql/codeql-workspace.yml
### IDE / Editor / Chat / CI Integration
- Per-language code-scanning query suites for CI — github__codeql/java/ql/src/codeql-suites/java-code-scanning.qls
### Ops & Deployment
- Language database extractors — github__codeql/unified/codeql-extractor.yml

================================================================
## REPO #13 — hindsight (remote: https://github.com/vectorize-io/hindsight)
lang: TypeScript (control-plane/Next.js, SDKs), Rust (CLI), Python (MCP server, embeddings)
### Ingestion & Indexing
- Store/retain a memory unit — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_register_retain
- Bulk/async retain of files — hindsight/hindsight-cli/src/main.rs:MemoryCommands::RetainFiles
- Upload/ingest documents — hindsight/hindsight-control-plane/src/app/api/documents/route.ts:POST
### Parsing & Chunking
- Parse documents (markitdown / LlamaParse / Iris) — hindsight/hindsight-api-slim/hindsight_api/engine/parsers/markitdown.py
- Chunk content during retain — hindsight/hindsight-cli/src/main.rs:BankCommands::SetConfig
### Symbol/Entity Extraction
- Extract entities and the entity graph — hindsight/hindsight-control-plane/src/app/api/entities/route.ts:POST
### Graph Construction
- Build/query entity relationship graph — hindsight/hindsight-control-plane/src/app/api/entities/graph/route.ts:GET
### Embeddings & Vector Search
- Generate embeddings (local ST / ONNX / remote TEI) — hindsight/hindsight-api-slim/hindsight_api/engine/embeddings.py:Embeddings
- Semantic/vector recall of memories — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_register_recall
### Keyword/Lexical Search
- Hybrid full-text + vector knowledge-base search — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_register_search_knowledge_base
### Hybrid & Ranked Retrieval
- Rerank results with MLX/Jina reranker — hindsight/hindsight-api-slim/hindsight_api/engine/jina_mlx_reranker.py:MLXReranker.rerank
### Memory Model & Types
- Fact types world/experience/observation — hindsight/hindsight-api-slim/hindsight_api/api/http.py
- Mental models (curated summaries) — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_register_create_mental_model
- Directives (injected behavioral rules) — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_register_create_directive
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Consolidate raw facts into observations — hindsight/hindsight-api-slim/hindsight_api/engine/consolidation/consolidator.py:run_consolidation_job
- Trigger consolidation — hindsight/hindsight-cli/src/main.rs:BankCommands::Consolidate
- Invalidate/forget a memory — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_register_invalidate_memory
- Clear memories (bulk forgetting) — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_register_clear_memories
### Temporal Reasoning
- Time-windowed recall — hindsight/hindsight-cli/src/main.rs:MemoryCommands::Recall
### Multi-tenant / Scoping / Namespaces
- Multi-tenant memory banks — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_register_create_bank
- Tag-scoped recall — hindsight/hindsight-cli/src/main.rs:MemoryCommands::Recall
### Persistence & Storage Backends
- PostgreSQL storage backend — hindsight/hindsight-api-slim/hindsight_api/engine/db/postgresql.py
- Oracle storage backend — hindsight/hindsight-api-slim/hindsight_api/engine/db/oracle.py
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server (FastMCP, Streamable HTTP) — hindsight/hindsight-api-slim/hindsight_api/api/mcp.py:create_mcp_server
- REST control-plane API — hindsight/hindsight-control-plane/src/app/api/recall/route.ts:POST
- Rust CLI — hindsight/hindsight-cli/src/main.rs:Cli
- Multi-language SDK clients — hindsight/hindsight-clients/typescript/src/index.ts
- Webhooks (HMAC-signed) — hindsight/hindsight-control-plane/src/app/api/banks/[bankId]/webhooks/route.ts:POST
### LLM & Agent Integration
- Reflect/reason with bank identity — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_register_reflect
- 50+ agent-framework integrations — hindsight/hindsight-integrations/
- Pluggable LLM providers — hindsight/hindsight-api-slim/hindsight_api/engine/providers/
### IDE / Editor / Chat / CI Integration
- Coding-agent integrations (Claude Code, Cursor, Codex, GitHub Copilot, etc.) — hindsight/hindsight-integrations/coding-agents
### Web UI & Visualization
- Web control-plane UI — hindsight/hindsight-control-plane/
- Interactive TUI explorer — hindsight/hindsight-cli/src/main.rs:Commands::Explore
### Security & Access Control
- API auth (login/logout) — hindsight/hindsight-control-plane/src/app/api/auth/login/route.ts:POST
- Bank-scoped MCP tool filtering — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_apply_bank_tool_filtering
- Audit logging of operations — hindsight/hindsight-api-slim/hindsight_api/mcp_tools.py:_apply_audit_logging
### Observability & Evaluation
- Prometheus metrics — hindsight/hindsight-cli/src/main.rs:Commands::Metrics
### Extensibility (plugins/providers/custom)
- Custom MCP tool extensions — hindsight/hindsight-api-slim/hindsight_api/extensions/mcp.py:McpToolExtension.register_tools
### Ops & Deployment
- Health/version endpoints — hindsight/hindsight-control-plane/src/app/api/health/route.ts:GET
- Helm/K8s deployment — hindsight/helm/
- Docker deployment — hindsight/docker/

================================================================
## REPO #14 — honcho (remote: https://github.com/plastic-labs/honcho)
lang: Python (FastAPI backend), TypeScript (MCP server), Python/TypeScript SDKs, Python CLI (Typer)
### Ingestion & Indexing
- Ingest session messages — honcho/mcp/src/tools/sessions.ts:add_messages_to_session
- Create session/peer/workspace containers — honcho/mcp/src/tools/workspace.ts:create_workspace
### Embeddings & Vector Search
- Generate text embeddings — honcho/src/embedding_client.py:EmbeddingClient.embed
### Keyword/Lexical Search
- Search across workspaces/sessions/peers — honcho/src/routers/workspaces.py:search_workspace
### Memory Model & Types
- Peer memory representations / peer cards — honcho/src/routers/peers.py:get_peer_card
- Workspaces + scopes (namespacing) — honcho/src/routers/workspaces.py:get_or_create_workspace
- Conclusions (derived facts) — honcho/src/routers/conclusions.py:create_conclusions
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Schedule/run "dream" consolidation — honcho/src/dreamer/dream_scheduler.py:DreamScheduler.schedule_dream
- Background representation derivation — honcho/src/deriver/deriver.py:process_representation_tasks_batch
### Multi-tenant / Scoping / Namespaces
- Workspace isolation (tenant header) — honcho/src/routers/workspaces.py:get_or_create_workspace
- Scopes grouping sessions/peers — honcho/src/routers/scopes.py:get_or_create_scope
### Persistence & Storage Backends
- Database persistence (Postgres) — honcho/src/db.py
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server — honcho/mcp/src/server.ts:register
- REST API (FastAPI routers, 54 endpoints) — honcho/src/routers/sessions.py
- Python/TypeScript HTTP SDK — honcho/sdks/python/src/honcho/http/client.py:HonchoHTTPClient
- Typer CLI — honcho-cli/src/honcho_cli/main.py:app
### LLM & Agent Integration
- Peer dialectic chat — honcho/src/routers/peers.py:chat
- Multi-provider LLM backends — honcho/src/llm/backends/anthropic.py
### Security & Access Control
- Admin-key gated operations — honcho/mcp/src/tools/workspace.ts:withAdminKeyHint
### Observability & Evaluation
- Dream/queue status — honcho/src/routers/workspaces.py:get_queue_status
### Extensibility (plugins/providers/custom)
- Pluggable LLM backends — honcho/src/llm/backends/
### Ops & Deployment
- Docker deployment — honcho/Dockerfile

================================================================
## REPO #15 — intuit__infigraph (remote: https://github.com/intuit/infigraph)
lang: Rust (workspace: cli, mcp, core, languages, docs, confluence, grammar-plugin, pipeline-plugin, lsp-to-scip, tree-sitter-vb6)
### Ingestion & Indexing
- Build code knowledge graph (index) — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("index_project")
- Import SCIP index — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("scip_import")
- Index package manifests/dependencies — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("index_manifests")
- Index documents — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("index_docs")
- Index Confluence pages — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("index_confluence")
### Parsing & Chunking
- Parse 60+ languages via tree-sitter — intuit__infigraph/crates/infigraph-languages/
- Document parsing/chunking — intuit__infigraph/crates/infigraph-docs/
### Symbol/Entity Extraction
- List symbols in a file — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("get_symbols_in_file")
- Detect HTTP routes/endpoints — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("detect_routes")
- Public API surface — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("get_api_surface")
- Find all references — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("find_all_references")
### Graph Construction
- Cypher query over code graph — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("query_graph")
- Export graph (cypher/graphml/json) — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("export_graph")
### Embeddings & Vector Search
- Unified hybrid (BM25+vector) search — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("search")
- Semantic-weighted search — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("semantic_search")
### Keyword/Lexical Search
- Regex code search — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("search_code")
- BM25 symbol search — intuit__infigraph/crates/infigraph-cli/src/main.rs:Commands::Search
### Hybrid & Ranked Retrieval
- Multi-strategy merged search — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("search")
- Hierarchical LM2 context assembly — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("memory_context")
### Code Navigation (LSP/refs/defs)
- Trace callers/callees — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("trace_callers")
- Symbol context (callers+callees+scope) — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("symbol_context")
### Code Analysis (static/dataflow/taint)
- Intra/inter-procedural taint analysis — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("detect_taint_flows")
- Security vulnerability scan — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("detect_security_issues")
- Dead-code detection — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("detect_dead_code")
- Cyclomatic complexity — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("get_complexity")
- Near-duplicate clone detection — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("detect_clones")
- Semantic diff — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("semantic_diff")
### Memory Model & Types
- Save/recall session memory (LM2) — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("save_session")
- Semantic session search — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("search_sessions")
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Consolidate sessions — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("consolidate_memory")
- Purge old sessions (forgetting) — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("purge_sessions")
- Auto-reindex file watcher — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("watch_project")
### Multi-tenant / Scoping / Namespaces
- Multi-repo groups (org-scoped) — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("group_create")
- Project registry/namespacing — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("list_projects")
### Persistence & Storage Backends
- Kuzu / Cozo graph database backends — intuit__infigraph/crates/infigraph-cli/src/bin/cozo_vs_kuzu.rs
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server (dispatch registry) — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:dispatch_tool
- CLI — intuit__infigraph/crates/infigraph-cli/src/main.rs:Commands
### LLM & Agent Integration
- Agent memory-context assembly — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("memory_context")
### IDE / Editor / Chat / CI Integration
- Install/uninstall MCP config for coding agents — intuit__infigraph/crates/infigraph-cli/src/main.rs:Commands::Install
### Web UI & Visualization
- Interactive graph visualization (vis.js) — intuit__infigraph/crates/infigraph-mcp/src/lib.rs:tool_def("visualize")
### Extensibility (plugins/providers/custom)
- Grammar plugins — intuit__infigraph/crates/infigraph-grammar-plugin/
- Pipeline plugins — intuit__infigraph/crates/infigraph-pipeline-plugin/
### Ops & Deployment
- Auto-reindex watcher — intuit__infigraph/crates/infigraph-cli/src/main.rs:Commands::Watch
- Self-update — intuit__infigraph/crates/infigraph-cli/src/main.rs:Commands::Update

================================================================
## REPO #16 — luuuc__sense (remote: https://github.com/luuuc/sense)
lang: Go (cmd/sense, internal packages), tree-sitter, ONNX Runtime, SQLite, mcp-go
### Ingestion & Indexing
- Scan & index a codebase — cmd/sense/main.go:runScan
- Incremental scan/walk — internal/scan/collector.go:collector.Symbol
### Parsing & Chunking
- Multi-language symbol extraction — internal/extract/extractor.go
### Symbol/Entity Extraction
- Emit symbols & edges — internal/scan/collector.go:collector.Symbol
### Graph Construction
- Build call/dependency graph — internal/blast/engine.go:Compute
### Embeddings & Vector Search
- Generate embeddings via ONNX (bundled model) — internal/embed/bundle.go:NewBundledEmbedder
- Semantic/keyword code search — internal/mcpserver/search.go:handleSearch
### Keyword/Lexical Search
- Keyword/semantic search CLI — cmd/sense/main.go
### Code Navigation (LSP/refs/defs)
- Dead-code detection — internal/dead/dead.go:FindDead
- Call-graph traversal (callers/callees) — internal/blast/engine.go:Compute
### Code Analysis (static/dataflow/taint)
- Dead-code analysis — internal/dead/dead.go:FindDead
- Blast-radius / change impact — internal/blast/engine.go:Compute
- Convention detection — internal/conventions/
### Persistence & Storage Backends
- SQLite storage — go.mod (modernc.org/sqlite)
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server — internal/mcpserver/builder.go:AddTool
- CLI — cmd/sense/main.go:run
### LLM & Agent Integration
- MCP server for agents — internal/mcpserver/builder.go:AddTool
### IDE / Editor / Chat / CI Integration
- Editor hook integration — internal/hook/
### Observability & Evaluation
- Status/health — internal/mcpserver/status.go:handleStatus
### Extensibility (plugins/providers/custom)
- Pluggable language extractors — internal/extract/
### Ops & Deployment
- Doctor/diagnostics — cmd/sense/main.go
- Self-update — cmd/sense/main.go

================================================================
## REPO #17 — mengram (remote: unknown) [dir: mengram]
lang: Python
### Ingestion & Indexing
- Import past conversations/notes from Claude Code, ChatGPT, Obsidian, text files — mengram/mengram/importer.py:import_claude_code
- Store a conversation or free text into the vault — mengram/mengram/engine/brain.py:remember
- Re-index the whole vault into vectors + graph — mengram/mengram/engine/brain.py:_reindex_vault
### Parsing & Chunking
- Parse markdown frontmatter, wikilinks, tags, sections and chunk — mengram/mengram/engine/parser/markdown_parser.py:parse_frontmatter
### Symbol/Entity Extraction
- Extract structured facts/entities/relations from a conversation via LLM — mengram/mengram/engine/extractor/conversation_extractor.py:ConversationExtractor.extract
### Graph Construction
- Add entities/relations and traverse the knowledge graph — mengram/mengram/engine/graph/knowledge_graph.py:KnowledgeGraph.add_entity
### Embeddings & Vector Search
- Generate and compare embeddings — mengram/mengram/engine/vector/embedder.py:Embedder.embed
- Store and search chunks by vector — mengram/mengram/engine/vector/vector_store.py:VectorStore.add_chunk
### Hybrid & Ranked Retrieval
- Combine graph traversal + vector search for recall — mengram/mengram/engine/retrieval/hybrid_search.py:HybridRetrieval.query
### Memory Model & Types
- Core memory brain orchestrating vault/graph/vector — mengram/mengram/engine/brain.py:MengramBrain
- Typed graph primitives — mengram/mengram/engine/graph/knowledge_graph.py:Entity
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Evolve procedures on failure and mine new procedures — mengram/mengram/cloud/evolution.py:EvolutionEngine.evolve_on_failure
- Record procedure feedback — mengram/mengram/engine/vault_manager/vault_manager.py:VaultManager.procedure_feedback
### Temporal Reasoning
- Store and query episodic memory with timestamps — mengram/mengram/engine/vault_manager/vault_manager.py:get_episodes
### Multi-tenant / Scoping / Namespaces
- Sub-user scoping of cloud requests — mengram/mengram/cloud/sub_user.py:SubUserScoped
### Persistence & Storage Backends
- Markdown/Obsidian-vault file storage — mengram/mengram/engine/vault_manager/vault_manager.py:VaultManager
- SQLite-backed vector store — mengram/mengram/engine/vector/vector_store.py:VectorStore
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server with tools — mengram/mengram/api/mcp_server.py:create_mcp_server
- REST API routes — mengram/mengram/api/rest_server.py:@app.post("/api/remember")
- CLI subcommands — mengram/mengram/cli.py:main
### LLM & Agent Integration
- OpenAI-compatible memory middleware — mengram/mengram/mengram_middleware.py:AutoMemory.chat
- LangChain chat history + retriever + chain — mengram/mengram/integrations/langchain.py:MengramChatMessageHistory
- CrewAI memory tools — mengram/mengram/integrations/crewai.py:create_mengram_tools
### IDE / Editor / Chat / CI Integration
- Claude Code auto-save hook management — mengram/mengram/cli.py:sub.add_parser("hook")
- Generate CLAUDE.md / .cursorrules from memory — mengram/mengram/cli.py:sub.add_parser("rules")
### Web UI & Visualization
- Local web UI (chat + knowledge graph) — mengram/mengram/cli.py:sub.add_parser("web")
### Security & Access Control
- Redact secrets from ingested text — mengram/mengram/engine/extractor/redact.py:redact_secrets
### Observability & Evaluation
- Extraction eval harness — mengram/mengram/evals/run_extraction_evals.py
- LOCOMO long-term-memory benchmark — mengram/mengram/benchmarks/locomo_bench.py
### Extensibility (plugins/providers/custom)
- CrewAI + LangChain integrations — mengram/mengram/integrations/crewai.py
### Ops & Deployment
- Self-host Docker image + compose — mengram/mengram/Dockerfile.selfhost

================================================================
## REPO #18 — mnemon (remote: unknown) [dir: mnemon]
lang: Go
### Ingestion & Indexing
- Import a memory draft file — mnemon/cmd/memory/import.go:Use:"import [file]"
- Store a new insight — mnemon/cmd/memory/remember.go:Use:"remember [content]"
### Symbol/Entity Extraction
- Extract/resolve entities from insight text — mnemon/internal/memory/graph/entity.go:ExtractEntities
### Graph Construction
- Breadth-first graph traversal — mnemon/internal/memory/graph/bfs.go:BFS
- Auto-create causal edges and neighborhood queries — mnemon/internal/memory/graph/causal.go:CreateCausalEdges
### Embeddings & Vector Search
- Embedding client supporting Ollama + OpenAI protocols — mnemon/internal/memory/embed/ollama.go:Client.Embed
- Cosine similarity over serialized vectors — mnemon/internal/memory/embed/vector.go:CosineSimilarity
### Keyword/Lexical Search
- Token/Jaccard/CJK-content scoring search — mnemon/internal/memory/search/keyword.go:KeywordSearch
### Hybrid & Ranked Retrieval
- Intent-aware recall blending vector + keyword + graph — mnemon/internal/memory/search/recall.go:IntentAwareRecall
- Beam search from an anchor + causal topological ordering — mnemon/internal/memory/search/recall.go:beamSearchFromAnchor
### Memory Model & Types
- Core insight/edge data model with importance + categories — mnemon/internal/memory/model/node.go:Insight
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Soft-delete an insight — mnemon/cmd/memory/forget.go:Use:"forget [id]"
- Review retention and suggest cleanup (GC) — mnemon/cmd/memory/gc.go:Use:"gc"
### Temporal Reasoning
- Temporal traversal mode + causal ordering of recall — mnemon/internal/memory/graph/engine.go:TemporalMode
### Multi-tenant / Scoping / Namespaces
- Multiple named memory stores — mnemon/cmd/memory/store.go:Use:"store"
- Agency authority admission + multi-agent peer federation — mnemon/internal/agency/authority/admission.go:Admit
### Persistence & Storage Backends
- SQLite-backed store — mnemon/internal/memory/store/db.go:DB.Open
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- Cobra CLI: remember, recall, search, forget, link, related, embed, import, gc, log, show, status, store, brief, receipt, viz — mnemon/cmd/memory/root.go
- Agency daemon serving + peer exchange (unix socket) — mnemon/cmd/agency/serve_command.go:Use:"serve"
### LLM & Agent Integration
- Durable agent work / projections / receipts for peers — mnemon/internal/agency/agent_view.go
### IDE / Editor / Chat / CI Integration
- One-shot setup deploying mnemon into many LLM-CLI environments — mnemon/cmd/memory/setup.go:Use:"setup"
### Web UI & Visualization
- Export knowledge graph for visualization — mnemon/cmd/memory/viz.go:Use:"viz"
### Security & Access Control
- Admission authority verifies artifacts before commit — mnemon/internal/agency/authority/admission.go:verifyAdmissionArtifacts
- Export privacy-safe operation receipt — mnemon/cmd/memory/receipt.go:Use:"receipt"
### Observability & Evaluation
- Show recent operations log — mnemon/cmd/memory/log.go:Use:"log"
### Extensibility (plugins/providers/custom)
- Pluggable embedding protocol (Ollama/OpenAI auto-detect) — mnemon/internal/memory/embed/ollama.go:Protocol
### Ops & Deployment
- Docker + compose + Make + goreleaser — mnemon/Dockerfile

================================================================
## REPO #19 — mnemosyne-oss (remote: https://github.com/mnemosyne-oss/mnemosyne) [dir: mnemosyne-oss__mnemosyne]
lang: Python
### Ingestion & Indexing
- Remember / batch / import via MCP — mnemosyne-oss__mnemosyne/mnemosyne/mcp_tools.py:_TOOL_HANDLERS
- Importers from other memory systems — mnemosyne-oss__mnemosyne/mnemosyne/core/importers/mem0.py:Mem0Importer
### Symbol/Entity Extraction
- Regex entity extraction + similarity matching — mnemosyne-oss__mnemosyne/mnemosyne/core/entities.py:extract_entities_regex
### Graph Construction
- Triple store (add/end/query) — mnemosyne-oss__mnemosyne/mnemosyne/core/triples.py:TripleStore
- Episodic graph of gists/facts/edges — mnemosyne-oss__mnemosyne/mnemosyne/core/episodic_graph.py:EpisodicGraph
### Embeddings & Vector Search
- Embedding provider abstraction — mnemosyne-oss__mnemosyne/mnemosyne/core/embeddings.py
- Vector recall inside BeamMemory — mnemosyne-oss__mnemosyne/mnemosyne/core/beam.py:BeamMemory
### Hybrid & Ranked Retrieval
- Maximal Marginal Relevance reranking — mnemosyne-oss__mnemosyne/mnemosyne/core/mmr.py:mmr_rerank
- Beam (working) memory recall — mnemosyne-oss__mnemosyne/mnemosyne/core/beam.py:BeamMemory
- Polyphonic (multi-voice) recall engine — mnemosyne-oss__mnemosyne/mnemosyne/core/polyphonic_recall.py:PolyphonicRecallEngine
### Memory Model & Types
- Public memory API class — mnemosyne-oss__mnemosyne/mnemosyne/core/memory.py:Mnemosyne
- Typed memory — mnemosyne-oss__mnemosyne/mnemosyne/core/typed_memory.py
- Scoped memory banks — mnemosyne-oss__mnemosyne/mnemosyne/core/banks.py:BankManager
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Weibull decay/boost of memory strength — mnemosyne-oss__mnemosyne/mnemosyne/core/weibull.py:weibull_decay_factor
- Hygiene audit + clean — mnemosyne-oss__mnemosyne/mnemosyne/core/hygiene.py:AuditReport
- Forget / sleep / canonical forget — mnemosyne-oss__mnemosyne/mnemosyne/mcp_tools.py:_TOOL_HANDLERS
- Veracity consolidation of facts — mnemosyne-oss__mnemosyne/mnemosyne/core/veracity_consolidation.py:VeracityConsolidator
### Temporal Reasoning
- Natural-language date / temporal expression parsing — mnemosyne-oss__mnemosyne/mnemosyne/core/temporal_parser.py:parse_nl_date
### Multi-tenant / Scoping / Namespaces
- Bank manager (isolated memory stores) — mnemosyne-oss__mnemosyne/mnemosyne/core/banks.py:BankManager
- Shared vs canonical stores — mnemosyne-oss__mnemosyne/mnemosyne/mcp_tools.py:_TOOL_HANDLERS
### Persistence & Storage Backends
- SQLite BeamMemory store — mnemosyne-oss__mnemosyne/mnemosyne/core/beam.py:_get_connection
- SQLite triple store — mnemosyne-oss__mnemosyne/mnemosyne/core/triples.py:_get_conn
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server with stdio / SSE / Streamable-HTTP transports — mnemosyne-oss__mnemosyne/mnemosyne/mcp_server.py:_build_mcp_server
- 29 MCP tools dispatched via `_TOOL_HANDLERS` — mnemosyne-oss__mnemosyne/mnemosyne/mcp_tools.py:_TOOL_HANDLERS
- CLI subcommands — mnemosyne-oss__mnemosyne/mnemosyne/cli.py
### LLM & Agent Integration
- LLM provider presets/resolution — mnemosyne-oss__mnemosyne/mnemosyne/core/llm_providers.py:get_provider_preset
- Hermes agent integration — mnemosyne-oss__mnemosyne/mnemosyne/hermes_memory_provider/persona_adapter.py
### Security & Access Control
- MCP bearer-token + Host/Origin policy on non-loopback — mnemosyne-oss__mnemosyne/mnemosyne/mcp_server.py:_resolve_http_auth
- Encrypted sync transport — mnemosyne-oss__mnemosyne/mnemosyne/core/sync.py:SyncEncryption
### Observability & Evaluation
- Recall diagnostics — mnemosyne-oss__mnemosyne/mnemosyne/core/recall_diagnostics.py
- Self-diagnose tool — mnemosyne-oss__mnemosyne/mnemosyne/mcp_tools.py:_handle_diagnose
### Extensibility (plugins/providers/custom)
- Plugin system (logging/metrics/filter/compression) — mnemosyne-oss__mnemosyne/mnemosyne/core/plugins.py:MnemosynePlugin
- Pluggable importer providers — mnemosyne-oss__mnemosyne/mnemosyne/core/importers/base.py:BaseImporter
### Ops & Deployment
- Docker + compose + deploy scripts + migrations — mnemosyne-oss__mnemosyne/mnemosyne/Dockerfile

================================================================
## REPO #20 — serena (remote: https://github.com/oraios/serena) [dir: oraios__serena]
lang: Python
### Symbol/Entity Extraction
- Retrieve symbols via LSP — oraios__serena/src/serena/symbol.py:LanguageServerSymbolRetriever
### Graph Construction
- Reference graph between symbols — oraios__serena/src/serena/symbol.py:ReferenceInLanguageServerSymbol
### Keyword/Lexical Search
- Regex/grep search across project — oraios__serena/src/serena/tools/file_tools.py:SearchForPatternTool
### Code Navigation (LSP/refs/defs)
- Find symbol / references / implementations / declaration — oraios__serena/src/serena/tools/symbol_tools.py:FindSymbolTool
- Rename + symbol-body edits — oraios__serena/src/serena/tools/symbol_tools.py:RenameSymbolTool
- JetBrains IDE navigation — oraios__serena/src/serena/tools/jetbrains_tools.py:JetBrainsFindSymbolTool
### Code Analysis (static/dataflow/taint)
- LSP diagnostics for file/symbol — oraios__serena/src/serena/tools/symbol_tools.py:GetDiagnosticsForFileTool
### Memory Model & Types
- Project-scoped memory manager — oraios__serena/src/serena/memories/memory_manager.py:MemoryManager
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Memory CRUD (write/read/list/delete/rename/edit) — oraios__serena/src/serena/tools/memory_tools.py:WriteMemoryTool
### Multi-tenant / Scoping / Namespaces
- Activate/remove project + current config — oraios__serena/src/serena/tools/config_tools.py:ActivateProjectTool
### Persistence & Storage Backends
- On-disk project memory files — oraios__serena/src/serena/memories/memory_manager.py:MemoryManager
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- FastMCP server with tool registry — oraios__serena/src/serena/mcp.py:SerenaFastMCPTool
- HTTP project query server — oraios__serena/src/serena/project_server.py:ProjectServer
- CLI to start MCP server — oraios__serena/src/serena/cli.py:start-mcp-server
### LLM & Agent Integration
- Agent orchestration + dashboard manager — oraios__serena/src/serena/agent.py:SerenaAgent
### IDE / Editor / Chat / CI Integration
- JetBrains IDE tools — oraios__serena/src/serena/tools/jetbrains_tools.py
### Web UI & Visualization
- Web dashboard server — oraios__serena/src/serena/dashboard.py:Dashboard
### Security & Access Control
- Tool capability/permission markers gating availability — oraios__serena/src/serena/tools/tools_base.py:ToolMarkerCanEdit
### Observability & Evaluation
- Token counting + tool usage stats — oraios__serena/src/serena/analytics.py:TokenCountEstimator
### Extensibility (plugins/providers/custom)
- Custom tool registration — oraios__serena/src/serena/tools/tools_base.py:ToolRegistry
- Multi-language LSP backends — oraios__serena/src/serena/solidlsp/language_servers/
### Ops & Deployment
- Docker + compose + devcontainer — oraios__serena/Dockerfile

================================================================
## REPO #21 — potpie-ai__potpie (remote: https://github.com/potpie-ai/potpie)
lang: Python (Typer CLI + FastAPI context-engine service + Rust/native sandbox)
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- Run bounded context resolution for a task — potpie/cli/commands/query.py:resolve
- Expose context resolve/search/record/status over HTTP — potpie/context-engine/src/potpie_context_engine/adapters/inbound/http/api/v1/context/router.py
- Receive GitHub webhook events — potpie/context-engine/src/potpie_context_engine/adapters/inbound/http/webhooks/integrations/github.py:github_router
- Manage the local daemon lifecycle — potpie/cli/commands/daemon.py
### Ingestion & Indexing
- Ingest a code repository as a "pot" source — potpie/cli/commands/pots.py:source_app
- Pull external source/ledger data into a pot — potpie/cli/commands/ledger.py:pull
- Submit a raw episodic event for ingestion — potpie/context-engine/src/potpie_context_engine/application/use_cases/submit_raw_episode.py
- Connect GitHub/Notion sources — potpie/context-engine/src/potpie_context_engine/adapters/outbound/connectors/github/connector.py
### Parsing & Chunking
- Parse repositories into a structured context graph — potpie/context-engine/src/potpie_context_engine/adapters/outbound/graph/context_graph_service.py
### Graph Construction
- Store the context graph in FalkorDB — potpie/context-engine/src/potpie_context_engine/adapters/outbound/graph/backends/falkordb_backend.py:FalkorDBGraphBackend
- Store the context graph in Neo4j — potpie/context-engine/src/potpie_context_engine/adapters/outbound/graph/backends/neo4j_backend.py:Neo4jGraphBackend
### Embeddings & Vector Search
- Produce local, dependency-free embeddings — potpie/context-engine/src/potpie_context_engine/adapters/outbound/intelligence/local_embedder.py:build_embedder
### Keyword/Lexical Search
- Lexical (labeled) graph/claim lookup fallback — potpie/context-engine/src/potpie_context_engine/adapters/outbound/graph/backends/in_memory_backend.py:search
### Hybrid & Ranked Retrieval
- Resolve an agent envelope ranking incl. context items by score — potpie/context-engine/src/potpie_context_engine/application/services/graph_service.py:resolve
### Memory Model & Types
- Typed request/record model — potpie/context-engine/src/potpie_context_engine/requests.py
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Flush windowed ingestion batches — potpie/context-engine/src/potpie_context_engine/application/use_cases/flush_windowed_batches.py
- Reap stale batches — potpie/context-engine/src/potpie_context_engine/application/use_cases/reap_stale_batches.py
- Record durable context — potpie/context-engine/src/potpie_context_engine/application/use_cases/record_durable_context.py
### Temporal Reasoning
- Read the activity timeline of a pot — potpie/context-engine/src/potpie_context_engine/application/readers/timeline_reader.py
### Multi-tenant / Scoping / Namespaces
- Scope memories to a pot/namespace (pot_id) — potpie/context-engine/src/potpie_context_engine/application/services/graph_service.py:resolve
- Manage multiple pots (create/use/linked/default) — potpie/cli/commands/pots.py:pot_app
### Persistence & Storage Backends
- Graph store backends: FalkorDB, Neo4j, embedded, in-memory — potpie/context-engine/src/potpie_context_engine/adapters/outbound/graph/backends/
- Postgres-backed event ledger — potpie/context-engine/src/potpie_context_engine/adapters/outbound/postgres/ledger.py
### LLM & Agent Integration
- Reconcile ingested episodes via a Pydantic-AI deep agent — potpie/context-engine/src/potpie_context_engine/adapters/outbound/reconciliation/pydantic_deep_agent.py
- GitHub read-agent tools with per-pot tenant isolation — potpie/context-engine/src/potpie_context_engine/adapters/outbound/connectors/github/agent_tools.py:github_get_pull_request
### IDE / Editor / Chat / CI Integration
- Claude-plugin skill + nudge hook templates — potpie/cli/templates/claude_plugin/skills/potpie-source-ingestion/SKILL.md
### Web UI & Visualization
- Daemon HTTP UI router — potpie/daemon/http/ui/router.py
### Security & Access Control
- Interactive login/logout for integrations (OAuth) — potpie/cli/commands/auth.py:login
- Per-repo allowlist enforcement on GitHub tools — potpie/context-engine/src/potpie_context_engine/adapters/outbound/connectors/github/agent_tools.py:_repo_allowed
### Observability & Evaluation
- OpenTelemetry tracing/metrics — potpie/context-engine/src/potpie_context_engine/adapters/outbound/observability/otel.py
- Graph quality reports — potpie/cli/commands/graph.py:quality_app
### Extensibility (plugins/providers/custom)
- Pluggable source connectors via registry — potpie/context-engine/src/potpie_context_engine/application/services/source_connector_registry.py
- Installable skills — potpie/cli/commands/skills.py:skills_app
### Ops & Deployment
- Daemon start/stop/status/restart/logs — potpie/cli/commands/daemon.py
- Cloud push/pull of context — potpie/cli/commands/cloud.py:cloud_app

================================================================
## REPO #22 — qualixar__superlocalmemory (remote: https://github.com/qualixar/superlocalmemory)
lang: Python (npm wrapper + Python MCP server + CLI `slm`)
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- Expose ~95 MCP tools over stdio — src/superlocalmemory/mcp/server.py
- `slm` CLI surface — src/superlocalmemory/cli/commands.py
### Ingestion & Indexing
- Ingest documents/files into memory — src/superlocalmemory/cli/commands.py:cmd_ingest
- Build the memory/association graph — src/superlocalmemory/mcp/tools_core.py:build_graph
### Parsing & Chunking
- Extract atomic facts from text — src/superlocalmemory/encoding/fact_extractor.py
### Symbol/Entity Extraction
- Mine behavioral assertions from tool usage — src/superlocalmemory/learning/assertion_miner.py
### Graph Construction
- Build/update a code knowledge graph — src/superlocalmemory/mcp/tools_code_graph.py:build_code_graph
### Embeddings & Vector Search
- Sentence-transformers embeddings — src/superlocalmemory/core/embeddings.py:embed
- Semantic code search — src/superlocalmemory/mcp/tools_code_graph.py:semantic_search_code
### Keyword/Lexical Search
- Lexical memory search — src/superlocalmemory/mcp/tools_core.py:search
### Hybrid & Ranked Retrieval
- Recall with ranked facts — src/superlocalmemory/mcp/tools_core.py:recall
### Code Navigation (LSP/refs/defs)
- Query the code graph — src/superlocalmemory/mcp/tools_code_graph.py:query_graph
### Code Analysis (static/dataflow/taint)
- Blast-radius analysis of changed files — src/superlocalmemory/mcp/tools_code_graph.py:get_blast_radius
- Detect changes / refactor preview — src/superlocalmemory/mcp/tools_code_graph.py:detect_changes
- Community/architecture overview — src/superlocalmemory/mcp/tools_code_graph.py:list_communities
### Memory Model & Types
- Remember/recall/update/delete memories — src/superlocalmemory/mcp/tools_core.py:remember
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Consolidate cognitive memory — src/superlocalmemory/mcp/tools_v33.py:consolidate_cognitive
- Forget memories — src/superlocalmemory/mcp/tools_v33.py:forget
- Set retention policy / compact — src/superlocalmemory/mcp/tools_v28.py:set_retention_policy
### Temporal Reasoning
- Lifecycle status & retention stats over time — src/superlocalmemory/mcp/tools_v28.py:get_lifecycle_status
### Multi-tenant / Scoping / Namespaces
- Profile-switching (namespaced memory) — src/superlocalmemory/mcp/tools_core.py:switch_profile
### Persistence & Storage Backends
- SQLite-backed memory/learning stores — src/superlocalmemory/access/rbac.py
### LLM & Agent Integration
- Record agent experiences / cognitive turns — src/superlocalmemory/mcp/tools_brain.py:record_agent_experience
- Evolve skills from experience — src/superlocalmemory/mcp/tools_evolution.py:evolve_skill
### IDE / Editor / Chat / CI Integration
- Editor integrations: Claude Code, Cursor, Windsurf, Copilot, Codex, Antigravity plugins — plugin/
### Web UI & Visualization
- Memory-graph/architecture visualization via code-graph tools — src/superlocalmemory/mcp/tools_code_graph.py:get_architecture_overview
### Security & Access Control
- RBAC over memory operations — src/superlocalmemory/access/rbac.py
- ABAC policies — src/superlocalmemory/compliance/abac.py
- Audit logging — src/superlocalmemory/compliance/audit.py
- Operation admission gating — src/superlocalmemory/core/admission.py
### Observability & Evaluation
- GDPR data export/erasure — src/superlocalmemory/compliance/gdpr.py
- Consistency/sheaf checks — src/superlocalmemory/mcp/tools_v3.py:consistency_check
### Extensibility (plugins/providers/custom)
- Evolution/budget/model-selection for skills — src/superlocalmemory/evolution/budget.py
### Ops & Deployment
- DB migration — src/superlocalmemory/cli/db_migrate.py
- Local peer mesh (send/inbox/lock/state) — src/superlocalmemory/mcp/tools_mesh.py:mesh_send

================================================================
## REPO #23 — supermemoryai__supermemory (remote: https://github.com/supermemoryai/supermemory)
lang: TypeScript (Hono MCP server + web app) plus Python/TS SDK packages
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server over streamable HTTP — apps/mcp/src/server/server.ts:createSupermemoryServer
- Register 15 MCP tools — apps/mcp/src/server/tools/*.ts
### Ingestion & Indexing
- Save a memory (or forget) to the active space — apps/mcp/src/server/tools/add-memory.ts:register
- Upload a file + prepare upload session — apps/mcp/src/server/tools/upload-file.ts:register
### Graph Construction
- Query the user's memory graph — apps/mcp/src/server/tools/memory-graph.ts:register
### Embeddings & Vector Search
- Server-side memory search — apps/mcp/src/server/tools/search-memory.ts:register
### Keyword/Lexical Search
- List memories by container tag — apps/mcp/src/server/tools/list-memories.ts:register
### Multi-tenant / Scoping / Namespaces
- List available spaces (container tags) — apps/mcp/src/server/tools/list-container-tags.ts:register
- Select active space — apps/mcp/src/server/tools/select-space.ts:register
### Memory Model & Types
- Save-memory tool (content + metadata) — apps/mcp/src/server/tools/save-memory.ts:register
### Web UI & Visualization
- Memory-graph playground web app — apps/memory-graph-playground
### LLM & Agent Integration
- Official SDKs for agent frameworks — packages/ai-sdk
### IDE / Editor / Chat / CI Integration
- Browser & Raycast extensions — apps/browser-extension
### Observability & Evaluation
- Posthog analytics on tool usage — apps/mcp/src/server/analytics.ts

================================================================
## REPO #24 — volcengine__OpenViking (remote: https://github.com/volcengine/OpenViking)
lang: Python (FastAPI server + agent `vikingbot`) + Rust (ov_cli, ragfs FUSE) + TypeScript web studio
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- MCP server (streamable HTTP) with 15 tools — openviking/server/mcp_endpoint.py
- REST API via FastAPI routers — openviking/server/app.py
- Rust CLI `ov` — crates/ov_cli/src/commands/*
### Ingestion & Indexing
- Backfill/ingest configured sources — openviking/ingest/orchestrator.py:IngestOrchestrator.backfill
- Reindex content — openviking/server/routers/content.py
- Add a resource to the memory store — openviking/server/mcp_endpoint.py:add_resource
### Parsing & Chunking
- Parse via internal parser registry OR third-party UnderstandingAPI — openviking/parse/parser_router.py:ParserRouter.parse
- Registry of format parsers — openviking/parse/registry.py:ParserRegistry._register
### Symbol/Entity Extraction
- Parser-based structured node extraction — openviking/parse/base.py:ResourceNode
### Graph Construction
- VikingDB vector+graph index management — openviking/storage/vikingdb_manager.py:VikingDBManager
### Embeddings & Vector Search
- Multiple embedder providers — openviking/models/embedder/*
- VikingDB vector adapter factory — openviking/storage/vectordb_adapters/factory.py
### Hybrid & Ranked Retrieval
- Hierarchical retriever (vector + lexical tiers) — openviking/retrieve/hierarchical_retriever.py:HierarchicalRetriever
- Intent analysis for query planning — openviking/retrieve/intent_analyzer.py:IntentAnalyzer
- Rerank providers (Volcengine/Cohere) — openviking/models/rerank/volcengine_rerank.py
### Keyword/Lexical Search
- Server-side grep/glob over indexed content — openviking/server/routers/search.py
- VikingDB full-text grep — openviking/storage/collection_schemas.py
### Code Navigation (LSP/refs/defs)
- Filesystem ls/tree/stat over viking:// URIs — openviking/server/routers/filesystem.py
### Memory Model & Types
- Store messages as memories — openviking/server/mcp_endpoint.py:remember
### Memory Lifecycle (consolidation/decay/forgetting/promotion)
- Hotness/decay scoring — openviking/retrieve/memory_lifecycle.py:hotness_score
- Forget URI(s) — openviking/server/mcp_endpoint.py:forget
### Temporal Reasoning
- Session archives & context history — openviking/server/routers/sessions.py
### Multi-tenant / Scoping / Namespaces
- Account/user/group management + RBAC — openviking/server/routers/admin.py
- ACL grant/revoke — openviking/server/routers/acl.py
### Persistence & Storage Backends
- VikingDB (Volcengine managed vector DB) primary backend — openviking/storage/vikingdb_manager.py
- Local persist/volatile vector engines — openviking/storage/vectordb/engine/_python_api.py
### LLM & Agent Integration
- `vikingbot` agent with built-in tools — bot/vikingbot/agent/tools/factory.py
- Compile agent tasks — bot/vikingbot/agent/tools/compile.py:CompileScopedTool
### IDE / Editor / Chat / CI Integration
- MCP connector for Claude Code / any MCP client — openviking/connector
### Web UI & Visualization
- Web studio — openviking/web_studio
### Security & Access Control
- Auth plugins: API key, LDAP, OIDC, dev, trusted — openviking/server/auth/plugins/*
- Privacy config versions/activation — openviking/server/routers/privacy_configs.py
### Observability & Evaluation
- Prometheus metrics — openviking/server/routers/metrics.py
- RAGAS-based eval + recorder — openviking/eval/ragas/*
### Extensibility (plugins/providers/custom)
- Pluggable embedder/rerank providers — openviking/models/embedder/*
- Skills management API — openviking/server/routers/skills.py
### Ops & Deployment
- Deploy assets resolve/preflight — openviking/server/routers/openviking_assets.py
- Rust CLI ops: snapshot, pack, watch, crypto, task — crates/ov_cli/src/commands/*

================================================================
## REPO #25 — joernio__joern (remote: https://github.com/joernio/joern)
lang: Scala (primary), Java (frontends/cpg schema)
### Ingestion & Indexing
- Build a Code Property Graph (CPG) from source via the `joern-parse` CLI — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernParse.scala:JoernParse.main
- Scan source, generate the CPG and run bundled queries via the `joern-scan` CLI — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernScan.scala:JoernScan.main
- Import an existing CPG into the interactive workspace — joernio__joern/console/src/main/scala/io/joern/console/Console.scala:importCpg
- Dispatch parsing to 15+ pluggable language frontends — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernParse.scala:generateCpg
### Parsing & Chunking
- Parse source into a layered Code Property Graph using language frontends — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernParse.scala:generateCpg
- Apply the default overlays (base / type / call-graph) — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/DefaultOverlays.scala:DefaultOverlays.create
### Symbol/Entity Extraction
- Query extracted code entities via semantic-CPG traversal API — joernio__joern/semanticcpg/src/main/scala/io/shiftleft/semanticcpg/language/NodeSteps.scala:NodeSteps
### Graph Construction
- Compose the layered CPG (AST / CFG / DDG / CDG) — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/DefaultOverlays.scala:DefaultOverlays.create
- Add the data-dependence graph overlay — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernSlice.scala:JoernSlice.checkAndApplyOverlays
### Embeddings & Vector Search
- Generate sparse, feature-hashed vector embeddings of CPG nodes — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernVectors.scala:JoernVectors.main
### Code Navigation (LSP/refs/defs)
- Traverse the AST of a node — joernio__joern/semanticcpg/src/main/scala/io/shiftleft/semanticcpg/language/types/expressions/generalizations/AstNodeTraversal.scala:AstNodeTraversal.ast
- Resolve references to a declaration — joernio__joern/semanticcpg/src/main/scala/io/shiftleft/semanticcpg/language/types/expressions/IdentifierTraversal.scala:IdentifierTraversal.refsTo
- Find callers / callees of a method — joernio__joern/semanticcpg/src/main/scala/io/shiftleft/semanticcpg/language/callgraphextension/MethodTraversal.scala:MethodTraversal.callIn
### Code Analysis (static/dataflow/taint)
- Compute data-flow / taint flows between sources and sinks — joernio__joern/dataflowengineoss/src/main/scala/io/joern/dataflowengineoss/language/ExtendedCfgNode.scala:ExtendedCfgNode.reachableByFlows
- Run source-to-sink data-flow queries from the CLI — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernFlow.scala:JoernFlow.main
- Slice the CPG by data-flow or by usages — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernSlice.scala:JoernSlice.main
- Run bundled vulnerability / pattern queries — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernScan.scala:JoernScan.Scan.create
### Persistence & Storage Backends
- Load/persist the CPG as a flatgraph binary (`cpg.bin`) — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/CpgBasedTool.scala:CpgBasedTool.loadFromFile
- Export the CPG to Neo4j CSV, GraphML, GraphSON or DOT — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernExport.scala:JoernExport.exportCpg
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- CLI tools: `joern-parse`, `joern-scan`, `joern-slice`, `joern-flow`, `joern-export`, `joern-vectors` — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/{JoernParse,JoernScan,JoernSlice,JoernFlow,JoernExport,JoernVectors}.sca
- Interactive CPGQL REPL — joernio__joern/console/src/main/scala/io/joern/console/BridgeBase.scala:BridgeBase.parseConfig
- HTTP CPGQL server mode — joernio__joern/console/src/main/scala/io/joern/console/BridgeBase.scala:BridgeBase.startHttpServer
### Web UI & Visualization
- Export graph representations for external visualization (DOT/GraphML/GraphSON) — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernExport.scala:JoernExport.exportCpg
### Security & Access Control
- Protect the CPGQL HTTP server with basic-auth — joernio__joern/console/src/main/scala/io/joern/console/BridgeBase.scala:BridgeBase.Config.serverAuthUsername
### Extensibility (plugins/providers/custom)
- Install / remove / list / run plugin layer-creators — joernio__joern/console/src/main/scala/io/joern/console/BridgeBase.scala:BridgeBase.Config.addPlugin
- Download and apply custom query bundles — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernScan.scala:JoernScan.downloadAndInstallQueryDatabase
- Register custom language frontends — joernio__joern/joern-cli/src/main/scala/io/joern/joerncli/JoernParse.scala:JoernParse.generateCpg
### Ops & Deployment
- Build and run via container image — joernio__joern/Dockerfile

================================================================
## REPO #26 — oracle__opengrok (remote: https://github.com/oracle/opengrok)
lang: Java (primary), jflex lexers, some shell/Python
### Ingestion & Indexing
- Build/update a Lucene inverted source index from a source root via the `Indexer` CLI — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/index/Indexer.java:Indexer
- Index VCS repository history — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/history/GitRepository.java:GitRepository
- Extract ctags symbols during indexing — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/analysis/Ctags.java:Ctags
### Parsing & Chunking
- Tokenize/parse 50+ languages with pluggable jflex analyzers — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/analysis/AnalyzerGuru.java:AnalyzerGuru
### Symbol/Entity Extraction
- Extract symbol definitions/references via ctags and analyzers — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/analysis/Ctags.java:Ctags
- Fetch symbol definitions for a file via REST — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/FileController.java:FileController.getDefinitions
### Keyword/Lexical Search
- Full-text / lexical search over indexed code via REST — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/SearchController.java:SearchController.search
- Build fielded Lucene queries — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/search/QueryBuilder.java:QueryBuilder
### Hybrid & Ranked Retrieval
- Ranked Lucene retrieval with unified result highlighting — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/search/SearchEngine.java:SearchEngine
### Code Navigation (LSP/refs/defs)
- Retrieve definitions/references for a file — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/FileController.java:FileController.getDefinitions
- Serve raw/source file content for navigation — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/FileController.java:FileController.getContentPlain
- Browse version history of a file/path — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/HistoryController.java:HistoryController.get
### Temporal Reasoning
- Browse VCS version history and attach user annotations — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/HistoryController.java:HistoryController.get
### Multi-tenant / Scoping / Namespaces
- Organize code into Projects and manage them via REST — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/ProjectsController.java:ProjectsController.addProject
- Group repositories and match projects to groups via REST — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/GroupsController.java:GroupsController.listGroups
### Persistence & Storage Backends
- Store the index in a Lucene data root — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/index/IndexDatabase.java:IndexDatabase
- Cache VCS history on disk — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/history/FileHistoryCache.java:FileHistoryCache
### APIs & Protocols (MCP/REST/CLI/LSP/gRPC)
- JAX-RS REST API under `/api/v1` — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/RestApp.java:RestApp
- CLI indexer — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/index/Indexer.java:Indexer
### Web UI & Visualization
- Web application serving file content, search and cross-references — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/GetFile.java:GetFile
- Autocomplete/suggest backend for the search UI — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/controller/SuggesterController.java:SuggesterController.getSuggestions
### Security & Access Control
- Path-based authorization filter (JAX-RS) — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/filter/PathAuthorizationFilter.java:PathAuthorizationFilter
- Authentication-token enforcement — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/api/v1/filter/IncomingFilter.java:IncomingFilter
### Observability & Evaluation
- Expose Prometheus metrics via servlet — oracle__opengrok/opengrok-web/src/main/java/org/opengrok/web/servlet/MetricsServlet.java:MetricsServlet
### Extensibility (plugins/providers/custom)
- Pluggable authorization plugins — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/authorization/AuthorizationFramework.java:AuthorizationFramework
- Pluggable VCS repository types — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/history/RepositoryFactory.java:RepositoryFactory
- Pluggable language analyzers — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/analysis/AnalyzerGuru.java:AnalyzerGuru
### Ops & Deployment
- Watch configuration for hot reload — oracle__opengrok/opengrok-indexer/src/main/java/org/opengrok/indexer/configuration/WatchDogService.java:WatchDogService
- Container / distribution packaging — oracle__opengrok/Dockerfile
