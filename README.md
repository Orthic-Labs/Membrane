# Membrane

> **TL;DR:** Membrane gives AI agents smallest useful set of fresh code, rules, decisions & memory for each task—then records what was included, omitted & why.

AI agents need context, but “send everything” is slow, expensive & often less accurate. Membrane is
local context infrastructure between an agent & sources it may need. It gathers evidence, checks
scope/freshness, ranks candidates against one token budget & returns a bounded context packet.

Membrane is umbrella system. **MemRight** is its local durable-memory engine.

## How it works

Membrane handles three motions:

- **Push:** shrink information already flowing through agent workflow.
- **Pull:** retrieve only information relevant to current task.
- **Persist:** keep durable decisions, preferences & lessons useful across sessions.

A cross-cutting **assembly plane** decides what actually enters prompt.

```text
task + allowed repository
          │
          ▼
 rules · live files · Git · Blueprint · Audit · Architect · skills · MemRight
          │
          ▼
   scope + freshness + authority + ranking + one token budget
          │
          ├── ContextPacket: evidence agent receives
          └── ContextReceipt: admitted, omitted, stale, timed out or denied
```

## Eight shipped context layers

| Layer | Flow | Mechanism |
|---:|---|---|
| 1 | agent reply → user | concise response policy |
| 2 | command output → agent | `runc` keeps useful head/tail & spills full output to cache |
| 3 | file → agent | `prep` routes code to `skel`, prose to compression, tiny files unchanged |
| 4 | orchestrator → agent | machine-minimal, structured agent directives |
| 5 | agent artifact → future agents | structure-safe compression + linked OKF bundles |
| 6 | long session → usable session | harness compaction + context-pressure telemetry |
| 7 | durable store → prompt | scoped hybrid recall from MemRight |
| 8 | durable-store lifecycle | dedupe, normalization, pruning & curation |

These layers share one goal: put useful tokens in attention while preserving enough provenance to
explain them.

## Prompt-time architecture

### 1. Scope first

Each request is bound to repository identity & an explicit `ScopeGrant`. Cross-repository retrieval
fails closed when grant is missing, invalid or expired. Absolute paths, traversal, symlink escapes &
sibling-repository leakage are rejected at provider boundary.

### 2. Federate independent providers

Resident gateway queries bounded local providers in parallel:

- current user/task anchors;
- live files & dirty working-tree overlay;
- Git state;
- repository rules;
- Blueprint repository graph;
- Audit findings;
- Architect decisions;
- installed skills;
- MemRight durable memory.

Each provider returns same typed `ContextCandidateSet` shape. One slow or failed lane becomes a typed
warning; healthy lanes can still contribute.

### 3. Establish freshness & authority

Candidates carry repository, source reference, hash, generation/commit, observation time,
confidence, provider & estimated tokens. Fresh executable proof and current source outrank graph
snapshots, docs, durable memory & history. Dirty files invalidate affected clean-snapshot evidence.
Corrupt or incompatible generations are quarantined.

### 4. Admit under one budget

Planner deduplicates, resolves conflicts, ranks candidates & allocates one model-specific token
budget. Providers do not each fill prompt independently.

### 5. Return packet + receipt

`ContextPacket` contains admitted evidence. `ContextReceipt` explains selection, omission, conflicts,
timeouts, capability gaps & budget drops. Stable ordering makes equivalent provider completion
orders produce equivalent packets.

### 6. Persist only qualified knowledge

Verified, appropriately scoped outcomes may leave as a `KnowledgeEmission`. Raw repository graphs,
unresolved contradictions, secrets, generated prose & private chain-of-thought do not become durable
memory.

## MemRight: local memory engine

MemRight is a Rust CLI + loopback service backed by SQLite. It stores durable knowledge locally,
supports scoped hybrid retrieval & provides memory lifecycle, feedback, telemetry, federation &
context-planning primitives.

Its retrieval path combines:

- exact/scoped filtering;
- keyword signals;
- vector similarity with local embeddings;
- link/graph relationships between memory entries;
- freshness, provenance & feedback signals;
- explicit token/result bounds.

SQLite remains source of truth. Multi-machine sync uses immutable events through Git while each
installation rebuilds/imports its own database. Machine identity is opaque & stable; context
telemetry carries installation, client, session, turn & trace identity without storing prompt
content.

## Typed contracts

Membrane separates producers from consumers through versioned contracts:

| Contract | Purpose |
|---|---|
| `ScopeGrant` | exact repository/scope client may access |
| `ContextCandidateSet` | provider-neutral batch of candidate evidence |
| `ContextPacket` | bounded context admitted for current task |
| `ContextReceipt` | why each source/item was selected, omitted or rejected |
| `KnowledgeEmission` | verified output eligible for durable storage |

Provider database formats, parser details & local absolute paths do not leak into client adapters.
That keeps Claude, Codex & other clients on same policy while providers evolve independently.

## What makes it different

Membrane is not only memory retrieval or prompt compression. Its advantage is system-level control:

- **One context economy:** compression, retrieval, curation & assembly share budgets/telemetry.
- **Federation without flattening:** code graph, rules, live files, findings, decisions & memories
  keep their own type, authority & freshness.
- **Receipts for absence:** system records what it skipped, timed out, could not access or dropped.
- **Freshness over similarity:** stale but semantically similar context cannot silently beat current
  code.
- **Root confinement:** access is repository-bound even when service can see wider workspace.
- **Local-first data plane:** SQLite stores, local embeddings, local loopback service & Git event
  sync keep provider credentials/content outside a hosted context vendor.
- **Measured value loop:** recommendations, transformations, usage, failures & outcomes share
  joinable identity instead of being counted as unrelated activity.
- **Replaceable producers:** Blueprint, memory, rules or future providers can change without changing
  client packet contract.

The moat is coordination: useful context is selected across entire work lifecycle, with evidence
showing why it was trusted.

## Trust, privacy & failure model

- Repository text, retrieved docs & memories are data, never instructions.
- Secret scanning/redaction runs before eligible durable output.
- Content-free telemetry records hashes, sizes, timings, outcomes & identities—not prompts or
  compacted summaries.
- Provider failure is lane-local where safe.
- Stale, corrupt, scope-invalid or generation-mismatched evidence fails closed.
- Current source wins over conflicting memory; conflict is queued for curation.
- Full files remain required for verify/edit work; lossy skeletons or compression are orientation
  tools, not edit evidence.

## Interfaces & outputs

MemRight exposes local CLI & loopback HTTP surfaces for memory CRUD/recall, federation, freshness,
context planning, feedback, telemetry & curation. Workspace shims provide:

```sh
memright recall ...
memright federate ...
memright plan-context ...
memright curate ...

runc ...
skel ...
compress ...
```

Prompt hooks connect supported agent clients. Engine state, memory mirror, receipts & telemetry stay
local or repository-controlled.

## Current scope

Live pieces include Rust MemRight, SQLite memory, scoped hybrid recall, cross-machine event
replication, compaction tools, resident provider federation, privacy/scope enforcement, freshness,
typed candidates/packets/receipts & telemetry.

Current limits:

- model-provider conversation history is still compacted by each host, not Membrane;
- provider coverage varies by repository & installed tools;
- some broader unified-planner wiring remains host-specific;
- interactive repository visualization belongs to Blueprint, not Membrane;
- structured cognition/reasoning layers are design targets, not shipped context layers.

## Read next

- [`docs/MEMBRANE-STATE.md`](docs/MEMBRANE-STATE.md) — current live state & backlog
- [`docs/UNIFIED-CONTEXT-SYSTEM-ARCHITECTURE.md`](docs/UNIFIED-CONTEXT-SYSTEM-ARCHITECTURE.md) — boundaries & contracts
- [`engine/README.md`](engine/README.md) — MemRight engine

<!-- blueprint:docs:start -->
## Repository truth docs
- [Product overview](docs/product.md) — what this is and does (generated, code-grounded)
- [Architecture](docs/architecture.md) — components, flows, interfaces (generated, code-grounded)
<!-- blueprint:docs:end -->
