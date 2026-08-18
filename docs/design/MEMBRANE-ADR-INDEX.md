# Membrane ADR index

> **Historical / Superseded** — the original RightContext-era ADR index (2026-07-era) listed nine decisions under `docs/plans/2026-07-*`; those files were deleted during the public reorganization and are not recoverable from git history. This index has been rewritten to describe only the ADR/plan documents that currently exist under `docs/plans/`.

The linked plan owns its decision, rationale, and supersession. [Membrane state](MEMBRANE-STATE.md) owns
current deployment truth. Execution plans, review artifacts, and documents without a decision are
intentionally excluded.

| Decision | Status | Canonical owner | Related / dependencies |
|---|---|---|---|
| [Vector backend bake-off harness](../plans/2026-08-01-vector-backend-bakeoff-harness.md) | Implemented (source accepted; release binding open) | [Vector-backend bake-off plan](../plans/2026-08-01-vector-backend-bakeoff-harness.md) | See [Membrane state](MEMBRANE-STATE.md) for release-binding status. |
| [F10 — tamper-evident canonical context-event history](../plans/2026-08-08-membrane-f10-event-integrity.md) | Source implemented; release/installed acceptance separate | [F10 event-integrity plan](../plans/2026-08-08-membrane-f10-event-integrity.md) | — |
| [Membrane best-of-market execution plan (Crypt DB hygiene and performance)](../plans/2026-08-12-membrane-crypt-database-hygiene-and-performance.md) | Superseded | [`MEMBRANE-IMPLEMENTATION-GUIDE.md`](../MEMBRANE-IMPLEMENTATION-GUIDE.md) | Implementation stopped on this plan; the implementation guide is now the authority. |
| [Planned retrieval and circuit admission](../plans/2026-08-17-contextplan-recallcircuit.md) | Proposed | [Recall-circuit plan](../plans/2026-08-17-contextplan-recallcircuit.md) | Pinned to base `e640aaa7` / tree `bca3d94b`; distinct from [`MEMBRANE-IMPLEMENTATION-GUIDE.md`](../MEMBRANE-IMPLEMENTATION-GUIDE.md), which is the current implementation authority. |

Update this index only when a listed decision is accepted, implemented, superseded, or replaced.
Do not copy live measurements or review prose into it.
