# Membrane — System Map

**Status:** derived system map · non-normative  
**Date:** 2026-08-20  
**Parent system:** Membrane
**Authority:** this file summarizes canonical Membrane doctrine & Blueprint SSOT. If it conflicts with either authority, that authority wins.

## 1. System rule

> **Membrane is the parent context system. Pull, Push, Cortex, Blueprint, Ledger, and Adapt are its six named subsystems.**

Planner, provider adapters, host adapters, MCP, supervisor/updater, and Hub integration are Membrane core/modules/surfaces. They are not peer semantic subsystems.

A subsystem may retain its own package, protocol, or store. Resident Membrane
lifecycle belongs only to Hub. Blueprint may use an explicit bounded one-shot
while Hub is inactive, but it does not retain independent residency.

## 2. One question per subsystem

| Subsystem | The question it answers | Owns | Does not own |
|---|---|---|---|
| **Pull** | What current evidence is sufficient for this task? | bounded acquisition, eligibility, fusion, attention admission, packet publication, receipts | durable knowledge, repository truth, reduction mechanics |
| **Blueprint** | What is true in this repository? | repository observation, evidence graph, generations, source identity, RecallCircuit, truth/drift/change intelligence, own SQLite/service/watcher | final context policy, durable knowledge, host enforcement |
| **Cortex** | What do we durably know? | governed durable knowledge, admission-before-write, conflict/supersession, temporal/lifecycle semantics, memory retrieval, own SQLite | repository truth, document index, final attention policy, reduction |
| **Ledger** | Where in the documents is the relevant material? | document/section index, stable anchors, hash-bound references, document navigation, rebuildable index store | source-document authority, document truth, durable knowledge, final admission |
| **Adapt** | What should we have learned? | transcript/event mining, Taste/Insights-style learning, evidence-backed proposals | any canonical truth store, direct durable writes, final context policy |
| **Push** | How can flowing context be reduced faithfully? | reversible transform mechanics, content-addressed artifacts, protected-span verification, token/byte accounting, host telemetry | ranking, final admission, durable knowledge, Cortex storage |

## 3. Membrane core

Membrane core owns the governed context decision:

```text
task + ScopeGrant + state + deadline + attention budget
    ↓
evidence requirements
    ↓
bounded acquisition from Blueprint / Cortex / Ledger / providers
    ↓
hard eligibility + authority + freshness
    ↓
sufficiency
    ↓
fusion + attention admission
    ↓
Push-selected faithful representation
    ↓
publication + ContextPacket + ContextReceipt
    ↓
outcome signals
```

Adapt consumes experience and emits proposals into Cortex admission; it does not bypass the planner or write durable truth directly.

## 4. Store ownership

| Store | Owner | Nature |
|---|---|---|
| `.agent/graph/graph.db` | Blueprint | derived repository evidence; rebuildable |
| `cortex-engine.db` | Cortex | authored durable knowledge; irreplaceable |
| Ledger index store | Ledger | derived document/section projection; rebuildable |
| content-addressed raw reduction artifacts | Push | recoverability substrate; reproducible by re-capture where source remains available |

No subsystem opens another subsystem's store as an implementation shortcut.

## 5. Boundary direction

```text
hosts / MCP / Hub integration
            ↓
      Membrane core planner
       ↙      ↓       ↘
Pull planner  Blueprint  Cortex  Ledger
                    ↘      ↓
                    Push executes selected representation

Adapt ── proposals ──→ Cortex admission
```

Rules:

- `engine/**` and `mcp/**` do not import `blueprint/src/**`.
- Membrane consumes Blueprint through Blueprint-owned public schemas/service/client surfaces.
- Blueprint does not open Cortex storage.
- Ledger indexes source documents but does not become their authority.
- Cortex does not absorb Ledger's document-index semantics.
- Push executes reduction; it does not become a second ranking/admission owner.
- Adapt writes proposals only through Cortex admission.

## 6. Physical placement is separate from semantic hierarchy

- Blueprint may be independently packaged, directly usable, and published while
  remaining a Membrane subsystem. Its continuous watcher/query role is hosted
  under Hub; Hub-off direct access is bounded one-shot only.
- Cortex is a Membrane-owned durable subsystem.
- Ledger may remain implemented inside Membrane runtime code while still having distinct semantic ownership.
- Adapt lives at `adapt/` within Membrane while retaining its semantic boundary.
- Push may remain implemented within Membrane runtime modules; a separate crate is not required merely to justify subsystem status.

## 7. Normative authorities

Six canonical authorities govern Membrane:

1. `MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md`
2. `BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md`
3. `ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md`
4. `LEDGER-MARKDOWN-INDEXING-AND-DOCUMENT-NAVIGATION-CANON.md`
5. `MEMBRANE-CROSS-SUBSYSTEM-IMPROVEMENTS-AND-EVIDENCE-GATES.md`
6. `CODERIGHT-MEMBRANE-OBSERVABILITY-LEARNING-AND-EVAL-INTEGRATION.md`

This map and the subsystem reference files are derived navigation aids, not parallel authorities.
