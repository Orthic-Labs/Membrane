# Final Absorption — Taking Cortex to Best-in-Class

**Evidence base.** Four competitor matrices read in full on 2026-08-12: `k3.md`, `ds.md`, `m3.md` (this checkout) and `sol.md` (the `cortex` checkout) — together covering all 30 repos under `vendor-research/`, function by function. Cortex's baseline is taken from its canonical sources: `README.md`, `docs/roadmap.md`, `docs/reference/deferred-surfaces.md`, `docs/reference/mcp.md`, and `release/compatibility.template.json`.

**The one-sentence strategy.** Of 31 products studied, Cortex is the only one whose primary job is *truth about the repository* — an evidence-backed map of what docs claim and code does, with contradictions surfaced and verdicts sealed to fingerprints. Everyone else sells **retrieval** (find code fast) or **judgment** (lint, audit, review). Absorption means wiring the field's best retrieval and judgment features into Cortex's evidence substrate, so every one of them ships with provenance, freshness, and confidence tiers no competitor can match.

---

## 1. Where Cortex stands today

| Surface | State |
|---|---|
| Store | `node:sqlite`, zero-server, WAL, transactional by generation |
| Phases | Phase 1 deterministic map (docs, claims, symbols, edges); Phase 2 claim verification with fingerprint-sealed verdicts |
| Honesty | Contradictions surfaced; `supersedes` chains as provenance; staleness + missing references reported; explicit ladders (`COMPILER > AST > LEXICAL`; `EXACT_RESOLUTION > … > UNRESOLVED`) |
| Languages | 36 across Tier A (12, AST-backed; JS/TS + Python compiler-backed via committed SCIP), Tier B (12), Tier C (12, lexical fallback for **any** language) |
| Query | `search`, `neighbors`, `path`, `impact`, `architecture`, `doc-truth`, `mermaid`; `doctor --full --json` |
| MCP | Frozen at 6 tools (`orient`, `search`, `expand`, `impact`, `doc_truth`, `status`), 8 resources, 6 prompts; read-only; effect profiles; typed errors with remediation |
| Freshness | Content-fingerprint invalidation, snapshot-backed proofs, merkle reconciliation, resident watchman |
| Federation | Self-hosted only; cross-repo slices scoped by `repoId`; barrier fan-out with independent receipts |
| Guards | Task-scoped path grants with TTL; hostile-repo suite; plugin trust boundary |
| Release | Signing, SBOM, OIDC trusted publishing, qualification gates, compatibility template |
| Implemented, unpublished | Explorer, tray, desktop onboarding |
| Declared limits | **No embeddings / semantic vector search**; parser depth varies; dynamic runtime registration can stay unresolved; SCIP opt-in |

---

## 2. Guardrails absorption may not touch

These come from Cortex's own doctrine and are treated as hard constraints throughout:

1. **Do-not-absorb list** (`docs/roadmap.md` § "What does NOT change in 1.x"): never turn Cortex into general user memory or final cross-layer context admission (Membrane/Crypt boundary); never rewrite the core in Rust or ship a placeholder crate.
2. **Deferred surfaces with decision records** (`docs/reference/deferred-surfaces.md`): Node SEA, hosted remote/team mode, third-party plugin marketplace — each requires a new decision record to reverse.
3. **Frozen public surfaces**: existing CLI names stay stable; the MCP surface changes only by typed proposal (D07), never silent widening; language Tier A/B/C labels are the public claim, not raw grammar counts.
4. **Local-only**: repository content never leaves the machine; MCP tools stay read-only.

These are not obstacles to absorption — they are the fence around the moat. Every feature below is specified to land inside them.

## 3. The five absorption tests

Every candidate feature is triaged against five tests:

1. **Evidence test** — can it carry path, span, content hash, provider, generation, and confidence like every other Cortex artifact?
2. **Freshness test** — does it invalidate by content fingerprint (only the evidence it touches), never by wall clock?
3. **Ladder test** — does it slot into the existing precision/confidence ladders instead of inventing adjectives?
4. **Local test** — zero-server, `node:sqlite`-compatible, no managed service, no network egress?
5. **Boundary test** — does it stay repository-scoped (not user memory, not context admission, not code authorship)?

Verdicts: **ABSORB** (passes all five), **ADAPT** (take the idea, rebuild it to pass), **DEFER** (sound but not now — waits on a later SDK-era surface or decision record), **REJECT** (fails a test that cannot be engineered around).


---

## 4. Triage of the field

All 30 competitors, grouped by capability cluster. Sources noted per row.

### A. Retrieval & context assembly — **ABSORB** (the last mile Cortex is missing)

| Feature | Best-in-field source | Verdict |
|---|---|---|
| Token-budgeted context assembly in one call (`assemble_context` collapses 5–10 tool round-trips into one) | code-compress (k3 §12, ds CLI row) | **ABSORB** — Cortex has `orient` candidate sets and bounded `expand`; add explicit token budgeting with presets |
| File skeleton / module-API digest views (names, signatures, digest, expanded) | code-compress `outline`/`get-module-api`; ast-grep `outline` views (ds CLI rows) | **ABSORB** — pure derivation over existing symbol nodes |
| Ranked repo map (PageRank over symbol references for auto-context) | aider `repomap.py` (k3, sol evidence map) | **ADAPT** — rank candidates by graph centrality + claim-verification status, a ranking no one else can offer |
| Shallow/deep index tiers for instant-first-answer | johnhuang code-index-mcp (k3 legend, §13) | **ABSORB** — formalize as read paths over the existing lexical/AST tiers |
| Hot-path / topic-outline navigation | code-compress `get-hot-path`, `topic-outline` (ds CLI row) | **ABSORB** as graph derivations |

### B. Semantic & vector search — **ADAPT** (Cortex's single biggest declared gap)

| Feature | Best-in-field source | Verdict |
|---|---|---|
| Hybrid semantic + lexical code search | claude-context (Milvus, hybrid mode, 5 embedding providers incl. Ollama — m3 §11, ds §1); context8 (Qdrant); cognee (LanceDB) | **ADAPT** — build local-only: the store already ships an optional `vectors` table with cosine `searchSimilar` (off by default — `store-sqlite.mjs`); what's missing is AST-boundary chunking + local embedding generation that populates it, with local/BYO-key providers. Vector hits enter as **retrieval hints tiered below LEXICAL**, never as verdict evidence |
| Structure-aware chunking on AST boundaries | treesitter-chunker (36+ built-in, 100+ downloadable grammars, parser pool, parallel chunking — k3 §13, sol) | **ABSORB** — Cortex already holds spans; chunking is a derivation, and it is the prerequisite for embeddings done right |
| Merkle snapshot discipline for incremental re-embedding | claude-context (`~/.context` Merkle snapshots, abort-aware indexing, poisoned-entry recovery — m3 §11) | **ABSORB the discipline** — Cortex's fingerprint invalidation already exceeds it; extend the same receipts to embedding generations |

### C. Evidence, verification & provenance — **ABSORB** (deepens the moat)

| Feature | Best-in-field source | Verdict |
|---|---|---|
| Contract-first proof gate (`CONTRACT → EXECUTE → AUDIT → PACK`, archive/close per change) | signum (m3 §0, ds CLI row) | **ADAPT** — grants + receipts are the substrate; add a change-proposal flow where doc-drift repairs ship as receipted proposals. MCP stays read-only; proposals are artifacts, not writes |
| Passive decision graph linking agent prompts to commits, with risk | brain0 (k3 legend, m3 §0) | **ADAPT, scoped** — repository-scoped change-provenance nodes only. Never general session/user memory (do-not-absorb #1) |
| Verification-led change safety (preflight, guard, attest, evidence packs) | roam-code (k3 legend, ds CLI row) | **ABSORB the derivable verbs** (preflight a symbol, attest a diff); reject the 285-command sprawl — fold into existing verbs + flags (CLI names frozen) |
| Diff-aware change detection driving review | code-review-graph `detect-changes`, `watch` (ds CLI row) | **ABSORB** — working-tree diff → impacted symbols, claims, docs, tests. Marries the graph to git, which every serious competitor does |

### D. Analysis verbs as graph derivations — **ABSORB**

| Feature | Best-in-field source | Verdict |
|---|---|---|
| Dead-code / unused-export detection | CodeGraphContext `analyze dead-code`; code-compress `find-unused`; RepoDoctor `deadcode` (ds CLI rows) | **ABSORB** — report through the confidence ladder: in dynamic languages an "unused" finding is `CROSS_FILE_HEURISTIC`, stated as such. Cortex's honesty turns the field's noisiest feature into a trustworthy one |
| Callers / call-chain / dependency-tree navigation | CodeGraphContext `analyze {calls, callers, chain, deps, tree, overrides}` (ds CLI row) | **ABSORB** — implied by `neighbors`/`path` today; expose as named, budgeted presets |
| Complexity metrics & churn hotspots | CodeGraphContext `complexity`; dependency-cruiser metrics reporters (k3 §12) | **ABSORB with a twist** — blend complexity × churn × **claim-staleness** into a "decay hotspot" ranking only Cortex can compute |
| Blast-radius analysis | code-compress `blast-radius`; roam preflight (ds CLI rows) | **ABSORB** — formalizes what `impact` already computes; add diff-aware input |

### E. Language breadth — **ABSORB** (inside the honesty tiers)

| Feature | Best-in-field source | Verdict |
|---|---|---|
| 100+ grammars, downloadable on demand | codebase-memory (**158** tree-sitter languages, hybrid LSP for 12 — k3 §13); treesitter-chunker (100+ downloadable); code-review-graph (~30 incl. ipynb, solidity — k3 §13) | **ABSORB the registry pattern** — on-demand WASM grammar acquisition; every new grammar lands in Tier C (lexical/AST only) until fixtures promote it. Grammar count is the field's #1 marketing stat; Cortex counters with breadth-*with-honesty*: 100+ languages, each labeled with its true depth |
| LSP-backed precision for dynamic languages | codebase-memory hybrid LSP for 12 (k3 §13) | **DEFER** — SCIP already covers the compiler lane; LSP is a later SDK-era surface, not core |

### F. Watch, hooks & host enrollment — **ABSORB** (breadth, not mechanism)

| Feature | Best-in-field source | Verdict |
|---|---|---|
| One-command hook install across agent hosts | roam `hooks claude --write`; Code-Index-MCP `hooks install`; repo-graph `install --agents --git-hook`; code-review-graph `register` + `hooks.json` (ds CLI rows) | **ABSORB** — extend `cortex-install` to the full host matrix (Claude, Codex, Cursor, Gemini, pre-commit). The mechanism exists; coverage is the gap |
| Pre-commit / post-commit freshness hooks | dependency-cruiser, semgrep, opengrep, react-doctor, CodeGraphContext post-commit (m3 §12) | **ABSORB** — trigger generation-scoped invalidation, reusing the existing barrier |

### G. Outputs & interchange — **ABSORB** (cheap, high-visibility)

| Feature | Best-in-field source | Verdict |
|---|---|---|
| SARIF export for CI findings | semgrep/opengrep (text, JSON, SARIF, JUnit, GitLab formats — k3 §12) | **ABSORB** — emit staleness / missing-reference findings as SARIF; every CI system in the field eats SARIF |
| Graph export: GraphML, SVG, Obsidian, Cypher | code-review-graph `visualize` (ds CLI row); dependency-cruiser dot/mermaid/d2/html/json/csv (k3 §12) | **ABSORB** — Cortex has mermaid today; add GraphML/JSON/Obsidian export. This is the honest answer to "replace Neo4j?" without replacing the store |
| Interactive HTML explorer | dependency-cruiser html explorer; codebase-memory graph UI on :9749 (m3 §11, ds CLI) | **ABSORB by publishing** — Cortex's Explorer is *implemented but unpublished*. Wave 0 |
| CI mode with baseline | react-doctor `ci install/config`; dependency-cruiser `depcruise-baseline` (ds CLI rows, m3 §11) | **ABSORB** — `cortex ci`: fail on new stale claims / broken refs / coverage regression vs. baseline |

### H. Health scoring & audit — **ABSORB** (as evidence composite)

| Feature | Best-in-field source | Verdict |
|---|---|---|
| 0–100 health score with per-finding explanations (`why <loc>`) | react-doctor (k3 §12, ds CLI row) | **ABSORB, rebuilt as composite evidence**: score = f(stale claims, missing refs, unresolved edges, coverage), trended across generations, every point clickable to its evidence. `cortex doctor` already computes the inputs |
| Multi-perspective repo audit report | ai-code-audit (LangGraph 5 perspectives — m3 §0); RepoDoctor scan/diet/tour (ds CLI row) | **ADAPT** — a `doctor --report` narrative generated strictly from graph evidence, never LLM-invented prose (the field's audit tools can hallucinate; Cortex's cannot) |

### I. Documentation generation — **ABSORB** (extends Phase 2 outputs)

| Feature | Best-in-field source | Verdict |
|---|---|---|
| Auto-generated wiki with per-page citations | context8 (auto-wiki generator, snippet cookbook — m3 §11); code-review-graph `wiki` (ds CLI row) | **ABSORB** — `docs/product.md` / `docs/architecture.md` already generate with claim citations; extend to full wiki export. No competitor's wiki can cite sealed evidence per page |

### J–P. Rejected clusters — see §6


---

## 5. The absorption roadmap

Sequenced by leverage-per-effort and by dependency (each wave's outputs feed the next). Sizes are directional — **S** (days, derivation over existing graph), **M** (weeks, new graph surface or store), **L** (a quarter, new substrate) — with the cost driver named. Execution packets would carry the bounded minute/file budgets per the workspace's contract format.

### Wave 0 — Ship what is already built (the free wins)

| # | Item | From | Size / driver |
|---|---|---|---|
| 0.1 | **Publish Explorer, tray, desktop onboarding** — answers the field's entire UI row (codebase-memory :9749, dependency-cruiser html, brain0 GUI) in one move | codebase-memory, dependency-cruiser | **S** — release mechanics; code exists and is tested |
| 0.2 | **Graph export formats** (GraphML, JSON, Obsidian, SVG) alongside mermaid | code-review-graph, dependency-cruiser | **S** — serializers over existing queries |

### Wave 1 — The retrieval last mile (derivations, no new substrate)

| # | Item | From | Size / driver |
|---|---|---|---|
| 1.1 | **Token-budgeted assembly**: `budget` + preset depths on candidate-set reads; one call returns neighborhood + claims + receipts within budget | code-compress `assemble_context` | **M** — token accounting + response shaping; governance: typed proposal for MCP input widening (D07) |
| 1.2 | **Outline / module-API digest** surfaces | code-compress, ast-grep | **S** — symbol-node projection |
| 1.3 | **Diff-aware impact**: working-tree diff → impacted symbols, claims, docs; `--diff` on impact/preflight | code-review-graph `detect-changes`, roam preflight | **M** — git integration + claim join |
| 1.4 | **Health score + trend**: composite of staleness, missing refs, unresolved edges, coverage; history across generations; `why` per deduction | react-doctor | **S** — doctor already emits inputs; scoring + persistence of trend |
| 1.5 | **SARIF export + `cortex ci` with baseline** | semgrep, react-doctor, dependency-cruiser | **S** — formatter + diff-against-baseline |
| 1.6 | **Named analysis presets**: callers/chain/tree/blast-radius/dead-code as budgeted presets over `neighbors`/`path`, dead-code reported at its true confidence tier | CodeGraphContext, code-compress | **S** — query presets + ladder labeling |

**Why this wave first:** every item is a read-path derivation over the existing graph — zero risk to the store, immediate parity with the features agents actually call most in competitor products.

### Wave 2 — The semantic layer (the declared gap, closed the Cortex way)

| # | Item | From | Size / driver |
|---|---|---|---|
| 2.1 | **AST-boundary chunking** with span provenance | treesitter-chunker | **M** — chunk derivation + chunk table, generation-transactional like everything else |
| 2.2 | **Local embeddings + hybrid search**: local embedding store beside `graph.db`; providers local-first (Ollama-class) with BYO key; **results enter as retrieval hints ranked below LEXICAL** — they influence candidate ordering, never verdicts | claude-context, context8, cognee | **L** — new store + provider abstraction + freshness receipts for embedding generations; this is the quarter-sized item |
| 2.3 | **Truth-ranked candidates**: centrality × verification status × semantic hint as the candidate ranking | aider repomap, rebuilt | **M** — ranking function over 1.1 + 2.2 |

**The rule that makes this safe:** embeddings reorder retrieval; they never mint evidence. Verdicts stay deterministic and fingerprint-sealed. This is the line between absorbing claude-context's capability and importing its weakness (unverifiable semantic claims).

### Wave 3 — Moat deepening (things no competitor can follow)

| # | Item | From | Size / driver |
|---|---|---|---|
| 3.1 | **Doc-repair proposals with receipts**: for each stale claim, a proposed rewrite citing current code evidence; human/agent approves; proposal is a sealed artifact | signum's proof gate + semgrep's autofix, rebuilt read-only | **M** — proposal generation + artifact schema |
| 3.2 | **Change provenance nodes**: repo-scoped links from accepted changes → diffs → affected claims (prompt-linkage where host provides it) | brain0, scoped | **M** — new node types + ingest hooks; boundary: never leaves repo scope |
| 3.3 | **Decay hotspots**: complexity × churn × claim-staleness ranking | CodeGraphContext + dependency-cruiser metrics | **S** — composite over existing signals |
| 3.4 | **Wiki export with per-page sealed citations** | context8, code-review-graph | **M** — extends existing doc generation |
| 3.5 | **Sealed team artifacts**: generation-sealed, manifest-verified graph export/import for self-hosted sharing — inside the federation decision, no hosted service | codebase-memory `graph.db.zst`, Code-Index-MCP artifact push/pull | **M** — packaging + verification; federation envelope exists |

### Wave 4 — Breadth (after depth is undeniable)

| # | Item | From | Size / driver |
|---|---|---|---|
| 4.1 | **On-demand grammar registry to 100+ languages**, each entering at Tier C with honest labels until fixtures promote it | codebase-memory (158), treesitter-chunker (100+) | **L** — registry + acquisition + fixture pipeline; note: grammar *count* is cheap, fixture-backed *depth* is the cost, and tiers keep the claim honest |
| 4.2 | **Full host-hook matrix** in `cortex-install` (Claude, Codex, Cursor, Gemini, pre-commit/post-commit) | roam, Code-Index-MCP, repo-graph, depcruise | **M** — per-host adapters; mechanism exists |

**Explicitly not scheduled:** LSP server (SDK-era), hosted anything (decision record 003), marketplace (004), SEA (001), Rust crate (002).


---

## 6. What Cortex must refuse to absorb

The matrices make the traps as visible as the treasures. Each rejected cluster fails a named test from §3:

| Cluster | Field leaders | Why rejected |
|---|---|---|
| **J. SAST / taint engine** | semgrep, opengrep | Fails Boundary: a rule engine is a different product. **Middle path — ingest, don't build:** accept SARIF findings as *external claims* with provider provenance, so Cortex maps "what semgrep says" alongside "what docs say," verifiable and freshness-tracked. This absorbs semgrep's value into the evidence model without owning its engine |
| **K. Codemod / rewrite engine** | ast-grep, gritql | Fails Boundary (code authorship) and Read-only: Cortex verifies and proposes; it does not rewrite source. Structural search parity is already delivered by AST + lexical tiers for graph purposes |
| **L. Pluggable graph DBs / Cypher substrate** | CodeGraphContext (FalkorDB/Kùzu/Neo4j), Neo4j exports everywhere | Fails Local: `node:sqlite` zero-server is a deliberate advantage — the matrices show DB provisioning is the #1 install friction across the field. Answer with export *formats* (Wave 0.2), not a new store |
| **M. Hosted RAG platform / cloud vector DB** | context8 (PG+Qdrant+Redis ops), claude-context (Milvus/Zilliz Cloud), cognee SaaS | Fails Local and hits deferred decision 003. Self-hostable-later, never managed-by-default |
| **N. Agent framework / multi-agent orchestration** | PraisonAI (9 packages), ai-code-audit (LangGraph), mentat/aider editors | Fails Boundary: Cortex is infrastructure those agents *consume* (MCP), not an agent itself |
| **O. Parser engine authorship** | oxc, tree-sitter itself | Fails Boundary/economics: tree-sitter WASM strategy already rides the ecosystem the whole field converged on (m3 §12: tree-sitter is the common parser) |
| **P. General memory / cross-layer context admission** | cognee cognitive layer, llama-index memory | Explicit do-not-absorb #1 — that is Membrane/Crypt's layer. Cortex stays the repository evidence map |
| **CLI verb sprawl** | roam-code (285 commands) | Fails the frozen-CLI rule. Capability is absorbed as flags/presets on existing verbs |

**Pattern worth noting:** every rejected cluster is a place where a competitor left repository scope — cloud services, user memory, code authorship, general agents. Cortex's coherence *is* the differentiator; absorption stops exactly at the boundary that makes it trustworthy.

---

## 7. Why absorbed features are worth more inside Cortex

Each absorbed feature compounds with the evidence substrate in ways the originator cannot replicate:

- **Token-budgeted assembly (code-compress)** + freshness receipts = the only context bundle an agent can *trust without re-reading*. code-compress budgets tokens; Cortex budgets tokens *and proves currency*.
- **Semantic search (claude-context)** + confidence ladders = hybrid search where the user sees which hits are EXACT and which are hints. The field blends them silently; Cortex labels them.
- **Health score (react-doctor)** + claim verification = a score whose deductions cite sealed evidence, trended by generation — not a lint count.
- **Dead-code detection (CodeGraphContext)** + UNRESOLVED tier = findings that admit their own uncertainty, eliminating the false-positive tax that makes the feature unusable elsewhere.
- **Wiki generation (context8)** + Phase-2 verdicts = docs where every paragraph carries its evidence and expires by fingerprint, not by calendar.
- **Diff-aware impact (code-review-graph)** + doc-truth = the only tool that answers "what breaks" *and* "which docs this diff just made false" in one receipt.

This is the absorption thesis in one line: **the field built retrieval and judgment on sand; the same features on Cortex's evidence base become the best implementations in the category.**


---

## 8. Risks & invariants to protect while absorbing

| Risk | Guard |
|---|---|
| Semantic layer quietly becomes evidence (the claude-context failure mode) | Vector results are a retrieval tier below LEXICAL by construction; verdicts and `doc-truth` never read them. Test: claim verdicts must stay byte-identical with embeddings disabled |
| Feature sprawl breaks the frozen MCP/CLI surfaces | All new capabilities land as inputs/flags/presets on existing verbs or as resources; any new tool goes through the D07 typed-proposal path |
| Breadth dilutes honesty (grammar-count marketing) | New grammars enter Tier C; tiers remain the public claim; fixture-backed promotion only |
| Provenance features leak into user memory | Change-provenance nodes are repo-scoped and sealed to generations; anything session/user-shaped belongs to Membrane/Crypt — that boundary is the do-not-absorb list |
| Freshness regressions from new stores (chunks, embeddings) | Every new artifact is generation-transactional and fingerprint-invalidated, same contract as the graph; readers only see complete generations |
| Absorbing audit/report features that can hallucinate | All narrative output is generated from graph evidence with citations; no free prose about the repository |

## 9. Competitor-by-competitor absorption map

Nothing read is lost — every one of the 30, with what Cortex takes and what it leaves:

| Competitor | Take | Leave |
|---|---|---|
| code-compress | Token-budgeted assembly, outlines, module API, blast-radius, hot-path, CLI≡MCP parity discipline | .NET stack |
| claude-context | Hybrid semantic search pattern, Merkle sync discipline, provider abstraction (local-first) | Milvus/cloud dependency |
| treesitter-chunker | AST-boundary chunking, downloadable-grammar registry | REST service shape |
| codebase-memory | Grammar breadth (158), sealed team artifact (zstd), graph UI existence proof | Pure-C engine rewrite |
| code-review-graph | Diff-aware detection, wiki, GraphML/Obsidian export, FTS5 validation of the SQLite bet | MCP tool-count sprawl (28 tools) |
| CodeGraphContext | Analysis verbs (callers/chain/dead-code/complexity), hook installer | Pluggable graph DBs, Cypher store |
| Code-Index-MCP (Consiliency) | Artifact push/pull, history ingest, preflight | Qdrant/Redis prod stack |
| johnhuang code-index-mcp | Shallow/deep read tiers | Temp-dir index lifecycle |
| contextplus | Semantic feature clustering, guarded propose/undo pattern (as proposals) | Shadow-git write machinery |
| roam-code | Preflight/guard/attest/evidence-pack verbs (folded into existing surface) | 285-command CLI |
| signum | Contract→audit→pack discipline (as receipted proposals) | Shell-harness delivery |
| brain0 | Prompt→commit provenance, repo-scoped | Passive always-on capture outside repo scope |
| aider | Ranked repo map → truth-ranked candidates | LLM edit application |
| react-doctor | Health score + `why`, CI mode, per-finding explanation links | React-only ruleset |
| dependency-cruiser | Export format breadth, baseline pattern, metrics | JS/TS-only scope |
| semgrep / opengrep | SARIF as interchange; findings ingestible as external claims | The SAST/taint engine itself |
| ast-grep / gritql | Outline views; structural-query inspiration for graph presets | Rewrite/codemod engine |
| context8 | Auto-wiki with citations, snippet cookbook | PG+Qdrant+Redis platform |
| cognee | Ontology grounding as *optional* claim taxonomy input | ECL pipelines, general agent memory |
| llama-index | PromptHelper-style budgeting, citation-carrying synthesizers | 300-integration breadth, RAG framework |
| repo-graph | Zero-copy mmap discipline (perf note for large repos), git-hook installer | rkyv custom format over SQLite portability |
| repo-lens | Stack-inventory report as a doctor section | Clone-to-temp lifecycle |
| RepoDoctor | Health-audit framing, `tour` onboarding idea | Copilot-CLI delegation |
| ai-code-audit | Multi-perspective report structure (evidence-generated) | LangGraph runtime, LLM-prose verdicts |
| oxc | Perf bar (SIMD lexer) as benchmark reference | Building a parser |
| tree-sitter | Incremental reparse discipline (`get_changed_ranges`) already honored via WASM grammars | Parser authorship |
| mentat | Cautionary tale: auto-context without evidence died archived | The product |
| PraisonAI | Nothing — out of category (agent framework) | The framework |

---

## 10. The end state

After these four waves, Cortex holds a position no competitor can contest without rebuilding itself:

1. **Retrieval parity** — token-budgeted, hybrid semantic + structural search with outlines, presets, and blast radius (matches code-compress, claude-context, CodeGraphContext).
2. **Judgment at evidence grade** — health scores, dead-code, audit reports, and CI gates where every finding cites sealed evidence and admits its confidence tier (exceeds react-doctor, semgrep-as-input, ai-code-audit).
3. **The uncontested layer** — doc-truth verification, contradiction surfacing, supersession, fingerprint-sealed freshness, and now change provenance and doc-repair proposals (no competitor has entered this layer at all).
4. **Breadth with honesty** — 100+ languages, each labeled with its true depth; the only grammar-count claim in the field that is verifiable.
5. **Zero operational tax** — still one SQLite file, no server, no cloud, read-only MCP; install friction that killed pluggable-DB and PG-based competitors stays at zero.

The best shape possible is not Cortex-plus-everything. It is the field's retrieval and judgment, rebuilt once on evidence — and everything that would dilute the evidence model left on the table, deliberately and by name.
