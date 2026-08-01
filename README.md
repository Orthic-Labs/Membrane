<img src=".github/banner.svg" alt="Membrane — The smallest useful context, with a receipt." width="100%">

**Membrane gives an AI agent the smallest useful set of current code, rules, decisions, and memory for each task — plus a receipt showing what entered context, what didn't, and why.**

![license](https://img.shields.io/badge/license-source--available-df6428?style=flat-square&labelColor=111318)
![local-first](https://img.shields.io/badge/local--first-df6428?style=flat-square&labelColor=111318)
![MCP](https://img.shields.io/badge/MCP-df6428?style=flat-square&labelColor=111318)

## What it is

When an agent gets a whole repository, a stale plan, and every old lesson, attention fills before
useful evidence arrives. Membrane sits between an agent and the sources it might need: code, rules,
decisions, and durable memory. It gathers evidence, checks scope and freshness, ranks candidates
against one token budget, and returns a bounded context packet you can inspect.

Membrane is the umbrella system. **Crypt** is its local durable-memory engine; `memright*` remains a
literal compatibility binary family. Membrane is the public name — RightContext remains an
internal/legacy alias on headers and telemetry tokens.

## How it works

Membrane handles three motions:

- **Push** — shrink information already flowing through the agent workflow.
- **Pull** — retrieve only what's relevant to the current task.
- **Persist** — keep durable decisions, preferences, and lessons useful across sessions.

A cross-cutting assembly plane decides what actually enters the prompt:

```text
task + allowed repository
          │
          ▼
 rules · live files · Git · Cortex · Audit · Architect · skills · Crypt
          │
          ▼
   scope + freshness + authority + ranking + one token budget
          │
          ├── ContextPacket: evidence the agent receives
          └── ContextReceipt: admitted, omitted, stale, timed out or denied
```

Prompt time runs six steps: scope the request to a repository and a `ScopeGrant`; federate
independent providers (user/task anchors, live files, Git, rules, Cortex, Audit, Architect, skills,
Crypt) in parallel, each returning the same typed `ContextCandidateSet`; establish freshness and
authority — fresh executable proof and current source outrank graph snapshots, docs, memory, and
history, and dirty files invalidate affected clean-snapshot evidence; admit under one token budget
by deduplicating, resolving conflicts, ranking, and allocating, since providers never fill the
prompt independently; return a `ContextPacket` plus a `ContextReceipt` that explains every
selection, omission, conflict, timeout, and budget drop; and persist only qualified knowledge as a
`KnowledgeEmission` — raw graphs, unresolved contradictions, secrets, and private chain-of-thought
never become durable memory.

## Eight shipped context layers

| Layer | Flow | Mechanism |
|---:|---|---|
| 1 | agent reply → user | concise response policy |
| 2 | command output → agent | `runc` keeps the useful head/tail, spills full output to cache |
| 3 | file → agent | `prep` routes code to `skel`, prose to compression, tiny files unchanged |
| 4 | orchestrator → agent | machine-minimal, structured agent directives |
| 5 | agent artifact → future agents | structure-safe compression plus linked OKF bundles |
| 6 | long session → usable session | harness compaction and context-pressure telemetry |
| 7 | durable store → prompt | scoped hybrid recall from Crypt |
| 8 | durable-store lifecycle | dedupe, normalization, pruning, curation |

## Crypt: local memory engine

Crypt is a Rust CLI and loopback service backed by SQLite. It stores durable knowledge locally and
supports scoped hybrid retrieval: exact/scoped filtering, keyword signals, vector similarity with
local embeddings, link/graph relationships between entries, freshness/provenance/feedback signals,
and explicit token/result bounds.

SQLite is the source of truth. Multi-machine sync uses immutable events through Git; each
installation rebuilds or imports its own database. Context telemetry carries installation, client,
session, turn, and trace identity, never prompt content.

## Typed contracts

| Contract | Purpose |
|---|---|
| `ScopeGrant` | exact repository/scope a client may access |
| `ContextCandidateSet` | provider-neutral batch of candidate evidence |
| `ContextPacket` | bounded context admitted for the current task |
| `ContextReceipt` | why each source/item was selected, omitted, or rejected |
| `KnowledgeEmission` | verified output eligible for durable storage |

Provider database formats, parser details, and local absolute paths never leak into client
adapters, so Claude, Codex, and other clients share one policy while providers evolve independently.

## Interfaces

Crypt exposes local CLI and loopback HTTP surfaces for memory CRUD/recall, federation, freshness,
context planning, feedback, telemetry, and curation. Workspace shims:

```sh
memright recall ...
memright federate ...
memright plan-context ...
memright curate ...

runc ...
skel ...
compress ...
```

Prompt hooks connect supported agent clients. Engine state, memory mirror, receipts, and telemetry
stay local or repository-controlled.

## What makes it different

- **One context economy** — compression, retrieval, curation, and assembly share budgets/telemetry.
- **Federation without flattening** — code graph, rules, live files, findings, decisions, and
  memories keep their own type, authority, and freshness.
- **Receipts for absence** — the system records what it skipped, timed out on, couldn't access, or
  dropped.
- **Freshness over similarity** — stale but semantically similar context can't silently beat current
  code.
- **Root confinement** — access stays repository-bound even when the service can see a wider
  workspace.
- **Local-first data plane** — SQLite stores, local embeddings, a local loopback service, and Git
  event sync keep provider credentials and content outside a hosted context vendor.
- **Replaceable producers** — Cortex, memory, rules, or future providers can change without changing
  the client packet contract.

## Trust, privacy and failure model

- Repository text, retrieved docs, and memories are data, never instructions.
- Secret scanning and redaction run before any eligible durable output.
- Telemetry is content-free: hashes, sizes, timings, outcomes, and identities, not prompts or
  compacted summaries.
- Provider failure is lane-local where safe; stale, corrupt, scope-invalid, or generation-mismatched
  evidence fails closed.
- Current source wins over conflicting memory; conflicts queue for curation.
- Full files remain required for verify/edit work — lossy skeletons or compression are orientation
  tools, not edit evidence.

## Current scope

Live: Rust Crypt, SQLite memory, scoped hybrid recall, cross-machine event replication, compaction
tools, resident provider federation, privacy/scope enforcement, freshness, typed
candidates/packets/receipts, telemetry.

Not yet shipped:

- provider coverage varies by repository and installed tools;
- model-provider conversation history is still compacted by each host, not Membrane;
- raw provider scores are lane-local — reserved memory/skill lanes are the cross-provider policy;
- some unified-planner wiring remains host-specific;
- interactive repository visualization belongs to Cortex, not Membrane;
- structured cognition/reasoning layers are design targets, not shipped context layers.

## Read next

- [`docs/MEMBRANE-STATE.md`](docs/MEMBRANE-STATE.md) — current live state and backlog
- [`docs/UNIFIED-CONTEXT-SYSTEM-ARCHITECTURE.md`](docs/UNIFIED-CONTEXT-SYSTEM-ARCHITECTURE.md) — boundaries and contracts
- [`engine/README.md`](engine/README.md) — Crypt engine (`memright*` compatibility crates/binaries)

<!-- blueprint:docs:start -->
## Repository truth docs
- [Product overview](docs/product.md) — what this is and does (generated, code-grounded)
- [Architecture](docs/architecture.md) — components, flows, interfaces (generated, code-grounded)
<!-- blueprint:docs:end -->

## Repository posture

This checkout is an internal mirror / workspace-coupled control plane for Adrian's studio
machines, not a standalone public product. Runtime wiring (hooks, Crypt loopback, federation
providers, install binding) depends on the parent workspace. Membrane is the public name;
RightContext remains an internal/legacy alias on headers and telemetry tokens.

## License

Source-available proprietary software for internal use and evaluation; redistribution,
repackaging, and competing use are prohibited. See [LICENSE](LICENSE).

---

<sub><b><a href="https://orthic-labs.github.io">Orthic Labs</a></b> — local-first infrastructure for AI-assisted development.<br>
<a href="https://github.com/Orthic-Labs/Membrane">Membrane</a> · <a href="https://github.com/Orthic-Labs/Cortex">Cortex</a> · <a href="https://github.com/Orthic-Labs/Sentinel">Sentinel</a> · <a href="https://github.com/Orthic-Labs/Roundtable">Roundtable</a> · <a href="https://github.com/Orthic-Labs/Morph">Morph</a> · <a href="https://github.com/Orthic-Labs/CutRight">CutRight</a> · <a href="https://github.com/Orthic-Labs/claudecodeX">claudecodeX</a></sub>
