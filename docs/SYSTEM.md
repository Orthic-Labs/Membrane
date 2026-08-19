# Membrane — System Map

**Status:** canonical system map · one page · the only place the subsystem list lives
**Date:** 2026-08-19
**Rule:** a subsystem owns a store, a process, or a public contract. Anything else is a module of one. New subsystems require an edit to this file.

Detailed doctrine per subsystem lives in `docs/subsystems/<name>.md`. Those docs share one shape: purpose · owns / does not own · public contract · invariants · Definition of Done. This file never duplicates them.

---

## 1. One sentence

> Membrane decides what deserves the agent's limited attention now, in what form, under whose authority, and records exactly why. Everything else either produces evidence for that decision or learns from its outcome.

---

## 2. The map

```
                         ┌──────────────────────────────────┐
                         │             HOSTS                │
                         │ MCP · hooks · Claude/Codex       │
                         │ supervisor · updater · Hub       │
                         └───────────────┬──────────────────┘
                                         │ ScopeGrant + task
                                         ▼
                         ┌──────────────────────────────────┐
                         │            PLANNER               │
                         │ requirements · acquisition       │
                         │ eligibility · sufficiency        │
                         │ fusion · admission · receipt     │
                         └──┬──────┬──────┬──────┬──────┬───┘
                            │      │      │      │      │
              ┌─────────────┘      │      │      │      └─────────────┐
              ▼                    ▼      ▼      ▼                    ▼
        ┌───────────┐       ┌────────┐ ┌──────┐ ┌─────────┐   ┌────────────┐
        │ BLUEPRINT │       │ CORTEX │ │SPINE │ │PROVIDERS│   │    PUSH    │
        │ repo truth│       │durable │ │ md   │ │git/live │   │ reduction  │
        │           │       │knowl.  │ │index │ │rules    │   │ artifacts  │
        │ own db    │       │ own db │ │own db│ │skills   │   │ TokenBal.  │
        │ own daemon│       │        │ │      │ │audit    │   │            │
        └───────────┘       └───▲────┘ └──────┘ │architect│   └────────────┘
                                │               │anchors  │
                                │ proposals     └─────────┘
                          ┌─────┴─────┐
                          │   ADAPT   │
                          │ Taste     │  ← transcripts, Cortex observable events
                          │ Insights  │
                          └───────────┘
```

---

## 3. Subsystems

| Subsystem | The one question it answers | Owns | Does not own | Contract consumed by planner | Status |
|---|---|---|---|---|---|
| **Planner** | What deserves attention now? | final policy: grant → eligibility → authority → freshness → sufficiency → fusion → admission → representation → publication → receipt | any evidence store; any parser | — (it *is* the consumer) | live; converging to one path |
| **Blueprint** (ex-Cortex) | What is true in this repository? | repo SQLite (`.agent/graph/graph.db`), resident daemon, watcher, generations, code+doc-claim graph, RecallCircuit, declared-vs-done, impact/tests/history, admission decisions | prompt budget, memory, host enforcement | `blueprint-protocol` (RecallCircuit, resolution/freshness states, truth findings) via daemon IPC; CLI fallback | live; canonical doc adopted; RecallCircuit/daemon `recall` pending |
| **Cortex** (ex-Crypt) | What do we durably know? | memory SQLite; record model; admission-before-write; conflict/supersession; temporal facts; lifecycle; Dream; observable-event telemetry | context policy; repo facts; compression | typed candidates via `providers/cortex` adapter; `membrane_temporal_fact`, `membrane_knowledge_propose` | live; **no subsystem doc yet** |
| **Spine** | Where in the markdown is it? | section-anchor index: `DocArtifactV1` registry, `Lexical/WholeDocument/Section` projections, `recall → doc_id + source_ref + anchor_id + expected_hash` | doc *truth* (Blueprint), doc *memory* (Cortex) | doc-candidate provider | built, **shadow-only** (never admitted); storage currently inside Cortex's `MemDb` |
| **Adapt** | What should we have learned? | transcript mining; Taste (preferences → proposals); Insights (19 failure detectors → `FailureCardV1`) | any store; any direct write | proposals into Cortex admission only | Taste ships; Insights built, **report-only**, not wired |
| **Push** | How do we shrink what's flowing without losing it? | one transform contract; `runc/skel/compress`; content-addressed raw artifacts; query-critical verifier; `TokenBalanceV1` | ranking; what is delivered | transform contract at MCP result egress, source reads, host post-tool hook | live primitives; **misplaced inside the `crypt` crate**; adoption 1-in-7 |
| **Providers** | Current worktree / rules / skills / audit / architect / anchors | thin adapters, typed candidates | final budget, authority, ranking | `membrane-provider-sdk` conformance | live |
| **Hosts** | How does a client reach Membrane? | MCP server (10 tools), hooks, Claude/Codex adapters, supervisor, updater, Hub handoff, install/doctor | policy | `membrane-protocol` five shapes | live |

---

## 4. Stores — one owner each, enforced

| Store | Owner | Durability | Rebuildable? | Who may open it |
|---|---|---|---|---|
| `.agent/graph/graph.db` (per repo) | Blueprint | derived from repo | yes, from repo | `blueprint/**` only |
| Cortex memory db | Cortex | **authored, irreplaceable** | no | `engine/crates/cortex-store/**` only |
| Spine index db | Spine | derived from markdown files | yes, from files | Spine modules only |
| Push artifact store | Push | content-addressed raw payloads | yes (re-capture) | Push modules only |

**Decision:** Spine gets its own SQLite file. It is a regenerable projection; Cortex is irreplaceable truth. Different backup, erasure, and rebuild semantics — they must not share a file. (Today Spine tables live in Cortex's `MemDb`; this is a migration item.)

---

## 5. Dependency direction (DAG, CI-enforced)

```
hosts      → planner
planner    → blueprint-protocol · cortex · spine · push · providers
adapt      → cortex (proposals through admission only)
push       → nothing below it
blueprint  → nothing in this tree
```

- `engine/**`, `mcp/**` MUST NOT import `blueprint/src/**`; they consume `packages/blueprint-protocol` + the Blueprint service, as an external consumer would.
- `blueprint/**` MUST NOT import `engine/**`, `mcp/**`, `membrane-protocol`.
- `blueprint-protocol` is **generated** from Blueprint's schemas/types; CI fails on `generate && git diff --exit-code`.
- Blueprint is the only subsystem published standalone (`@orthic-labs/blueprint`).

---

## 6. Target layout

```
membrane/
  docs/SYSTEM.md                ← this file
  docs/subsystems/{planner,blueprint,cortex,spine,adapt,push,providers,hosts}.md
  blueprint/                    ← ex-Cortex repo (subtree), own package.json
  engine/crates/
    cortex/ cortex-core/ cortex-store/ cortex-format/      ← ex-crypt-*
    membrane-core/ membrane-runtime/ membrane-protocol/ membrane-provider-sdk/ …
    push/                       ← extracted from crypt crate
    spine/                      ← extracted from membrane-runtime/doc_*.rs
  engine/federation/providers/  ← blueprint.py (ex-cortex.py), cortex.py (ex-crypt.py), git, live, rules, skills, audit, architect, anchors
  adapt/                        ← ex-Adapt repo (subtree), Python
  mcp/  hooks/                  ← hosts
  packages/membrane-protocol/  packages/blueprint-protocol/
  tests/integration/            ← real Blueprint daemon + real Membrane stack
  docs/design/                  ← archived rationale (read-only)
```

---

## 7. Open wiring — the gaps that make the system feel unmanaged

| Gap | Owner | Closes when |
|---|---|---|
| Spine is shadow-only | Spine + Planner | doc candidates admitted under the evidence-class coverage floor; frozen fixtures prove non-regression |
| Insights is report-only | Adapt → Cortex | `FailureCardV1` → gotcha proposal → Cortex admission; gotchas surface when planned action matches trigger |
| Push lives in the memory crate; 1-in-7 adoption | Push | extracted crate; wired at MCP egress + source reads; adoption metric on receipts |
| Blueprint consumed via per-query Node spawn | Blueprint + Providers | daemon `recall` + persistent client in `providers/blueprint.py` |
| Cortex has no doctrine | Cortex | `docs/subsystems/cortex.md` adopted |
| Spine tables inside Cortex db | Spine | own file + migration |
| No outcome ledger | Planner | content-free candidate journey ledger joins delivered → used/ignored/contradicted |

---

## 8. Named and not built (so they stop resurfacing)

- **Cognition family** (layers 9–11: decompose / thought-graph / claim-verify) — named 2026-07-18, unbuilt, not a subsystem.
- Relation graph in Cortex beyond depth-1 bounded expansion.
- Second re-anchor ladder in Membrane (Blueprint owns it).
- Vector lane in Blueprint.
- Shared "contracts" bucket.
- Remote/hosted store of any kind.

---

## 9. Historical names

| Historical | Current | Note |
|---|---|---|
| RightContext / Unified Context Engine | Membrane | product |
| Cortex | Blueprint | repo truth engine (rename pending, see `docs/plans/2026-08-19-monorepo-merge-and-subsystem-rename.md`) |
| MemRight → Crypt | Cortex | durable knowledge (rename pending; `crypt*` remains the compat facade) |
| Markdown Doc Spine / RMS D1–D4 | Spine | section index |
| four families / eight layers (`tools/lib/CONTEXT-ENGINEERING.md`) | Push · Planner · Cortex · (Cognition) | Compaction=Push, Retrieval=Planner pull, Curation=Cortex lifecycle, Cognition=unbuilt |
