<img src=".github/banner.svg" alt="Membrane — The smallest useful context, with a receipt." width="100%">

**Give an agent the whole repository, a stale plan, and every old lesson, and its attention fills before useful evidence arrives. Membrane sits between the agent and its sources, and returns the smallest useful set of current code, rules, decisions, and memory for each task — plus a receipt showing what entered context, what didn't, and why.**

![license](https://img.shields.io/badge/license-source--available-df6428?style=flat-square&labelColor=111318)
![local-first](https://img.shields.io/badge/data%20plane-local--first-df6428?style=flat-square&labelColor=111318)
![MCP](https://img.shields.io/badge/surface-MCP%20·%20hooks%20·%20CLI-df6428?style=flat-square&labelColor=111318)

## Six axes

| Motion | What it does |
|---|---|
| **Push** | Shrinks information already flowing through the agent workflow — command output, file reads, prose |
| **Pull** | Retrieves only what is relevant to the current task, from every source that might hold it |
| **Cortex / Persist** | Keeps governed durable decisions, preferences, and lessons useful across sessions and machines |
| **Blueprint** | Owns repository truth, evidence generations, and drift/change observation |
| **Ledger** | Navigates indexed document sections with hash-bound references |
| **Adapt** | Mines experience into governed proposals; never writes durable truth directly |

All six share one context economy while retaining separate ownership, tests,
metrics, and improvement paths.

## How a packet is assembled

```mermaid
flowchart LR
    T[task + repository] --> SG[ScopeGrant]
    SG --> F[federation fan-out<br/>9 parallel providers:<br/>Blueprint · rules · live files · Git<br/>audit · architect · skills · memory · anchors]
    F --> FA[freshness / authority<br/>fresh proof outranks snapshots,<br/>docs, memory, history]
    FA --> AP[admission planner<br/>reserved lanes, then global fill<br/>under one token budget]
    AP --> CP[ContextPacket<br/>what the agent gets]
    AP --> CR[ContextReceipt<br/>what it didn't get, and why]
    CP --> KE[KnowledgeEmission<br/>qualified output persisted<br/>as durable memory]
```

Every source keeps its own type, authority, and freshness — the code graph is not flattened into the same blob as a six-week-old decision note. Conflicts are resolved by rank, not by whichever chunk embedded closest.

## What makes it different

- **Receipts for absence.** The receipt records what was skipped, timed out, inaccessible, or dropped for budget — not just what was returned. "Why didn't the agent know X" becomes a lookup instead of an argument.
- **Freshness beats similarity.** A stale but semantically-similar candidate cannot silently outrank current code.
- **Root confinement.** Access stays repository-bound even though the service can see a wider workspace.
- **Local-first data plane.** SQLite stores, local embeddings, a loopback service, Git-based event sync. No hosted context vendor sees credentials or content.
- **Replaceable producers.** Blueprint, memory, rules, or a future provider can change without changing the client packet contract.

The contract is five typed shapes — `ScopeGrant`, `ContextCandidateSet`, `ContextPacket`, `ContextReceipt`, `KnowledgeEmission` — so provider database formats, parsers, and local paths never leak into client adapters. Claude, Codex, and any MCP client share one policy.

## Measured, not vibes

| Figure | Value |
|---|---|
| Warm `/federate` latency (resident gateway, 20 runs) | **p50 81.8 ms · p95 108.8 ms** (was 434–506 ms + ~150 ms spawn per request) |
| Admission budget | 4,096 tokens, with reserved lanes: memory 800 · skills 300, then global fill |
| Packet size cap | 30,000 code points, independent 10,000-char rendered-door cap |
| Federation deadlines | Claude 7 s · Codex 6.25 s inside a 9 s internal deadline |

## Inside

- **Pull** — bounded acquisition, eligibility, fusion, admission, and packet publication.
- **Push** — faithful reduction for command output, file reads, source/provider payloads, and delegated evidence.
- **Cortex / Persist** — the durable-memory engine: a durable library and CLI projection owned by Membrane, backed by SQLite with a quantized vector store and hybrid retriever. Membrane Hub owns the single resident service.
- **Blueprint** — repository truth and evidence graph.
- **Ledger** — hash-bound document navigation.
- **Adapt** — governed experience-to-knowledge proposals.
- **MCP server** — seventeen tools over stdio (`membrane_context`, `membrane_source_read`, `membrane_blueprint`, `membrane_knowledge_propose`, `membrane_checkpoint_save`, `membrane_checkpoint_load`, `membrane_working_context`, `membrane_temporal_fact`, `membrane_scratchpad`, `membrane_feedback`, `membrane_diagnostic_workspace`, `membrane_diagnostic_mutation`, `membrane_diagnostic_snapshot`, `membrane_diagnostic_fence`, `membrane_diagnostic_capabilities`, `membrane_diagnostic_baseline`, `membrane_diagnostic_provider`), serving both the 2025-03-26 and 2026-07-28 MCP discovery eras. The generated source of truth for this surface is [docs/reference/product-truth.md](docs/reference/product-truth.md).
- **Federation gateway** — a supervised resident worker behind `POST /federate` that fans out to providers in parallel.
- **Prompt hooks** — per-host recall planners (Claude and Codex) that route candidates through admission on every prompt.
- **Membrane Hub** — sole runtime, desktop build, release, installer, updater, service-supervision, icon, & legal-asset authority.

## Running it

```sh
pnpm install        # Node >= 20, pnpm 11
pnpm test           # MCP server + client + install-binding suites

rightkit cargo build --workspace                 # Membrane engine
rightkit cargo test --workspace --features fastembed # Cortex/Persist embeddings
```

Day-to-day surfaces are the installed Membrane CLI: `membrane cli pull plan-context`,
`membrane cli pull federate`, `membrane cli push runc`, `membrane cli push skel`,
`membrane cli push compress`, `membrane cli push restore`, and Cortex durable-memory
commands such as `membrane cli recall` and `membrane cli curate`.

## Recent

- **Vector backend bake-off (2026-08)** — reproducible Rust benchmark across Mac/Windows SIMD lanes; decision: keep vectors in Cortex, move to resident in-process f32 dispatch.
- **Resident federation gateway (2026-07)** — per-request spawns replaced by a supervised resident worker; warm-path latency dropped ~5×.
- **MCP dual-era stdio (2026-07)** — exact `@modelcontextprotocol/server@2.0.0`, enforced I/O schemas, structured tool results, W3C trace propagation through `/federate`, caller authorization bound to exact repo/root/scope.
- **Honesty pass (2026-08)** — reserved lanes documented as the explicit cross-provider score policy; write paths now refuse hand-typed scopes that would fork the corpus.

## Repository posture

This checkout is an internal mirror of a workspace-coupled control plane for the author's studio machines — not a standalone public product. Runtime wiring (hooks, Membrane resident service, federation providers, install binding) depends on the parent workspace. Conversation-history compaction still belongs to each host, and the structured cognition layers (`plan` / `think` / `verify`) are design targets, not shipped code.

---

<sub><b><a href="https://orthic-labs.github.io">Orthic Labs</a></b> — local-first infrastructure for AI-assisted development.<br>
<a href="https://github.com/Orthic-Labs/Membrane">Membrane</a> · <a href="https://github.com/Orthic-Labs/Membrane/tree/main/blueprint">Blueprint</a> · <a href="https://github.com/Orthic-Labs/Membrane/tree/main/adapt">Adapt</a> · <a href="https://github.com/Orthic-Labs/Forge">Forge</a> · <a href="https://github.com/Orthic-Labs/Roundtable">Roundtable</a> · <a href="https://github.com/Orthic-Labs/CutRight">CutRight</a> · <a href="https://github.com/Orthic-Labs/claudecodeX">claudecodeX</a></sub>

<!-- blueprint:docs:start -->
## Repository truth docs
- [Product overview](docs/product/README.md) — what this is and does (generated, code-grounded)
- [Architecture](docs/architecture/membrane.md) — components, flows, interfaces (generated, code-grounded)
<!-- blueprint:docs:end -->

<!-- support-matrix:start -->
## Support tier matrix

Generated from current MBR-801 installed-path conformance receipts — 0 of 10
platform/client pairs currently qualified. Full table, tiers, and reasons:
[docs/product/support/matrix.md](docs/product/support/matrix.md) (also machine-readable at `docs/product/support/matrix.json`).
This block plus JSON/MD matrix are written by
`node scripts/tools/productization/generate-support-matrix.mjs` from same receipts;
none are hand-maintained.
<!-- support-matrix:end -->
