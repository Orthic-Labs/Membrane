# RightContext ADR index

This is the compact decision log for the governing RightContext and MemRight context-system ADRs.
The linked ADR owns its decision, rationale, and supersession. [RightContext state](RIGHTCONTEXT-STATE.md)
owns current deployment truth; [Context Engineering](../../tools/lib/CONTEXT-ENGINEERING.md) owns the
three-family/eight-layer engine policy; the [unified architecture](UNIFIED-CONTEXT-SYSTEM-ARCHITECTURE.md)
owns the durable RightContext product boundary. Execution plans, review artifacts, and documents
without an ADR decision are intentionally excluded.

| Decision | Status | Successor | Canonical owner | Related / dependencies |
|---|---|---|---|---|
| [MemRight context-engineering hardening pass](plans/2026-07-05-memright-context-engineering-next.md) | Implemented | None | [Context Engineering](../../tools/lib/CONTEXT-ENGINEERING.md) | The ADR is the frozen rationale and evidence; its gated fused-ranking experiment was measured and reverted. |
| [Admission budget lanes and memory DB-provenance seal](plans/2026-07-15-rightcontext-admission-lanes-memory-seal.md) | Implemented | None | [Admission-lanes ADR](plans/2026-07-15-rightcontext-admission-lanes-memory-seal.md) | — |
| [Verified per-candidate feedback rail](plans/2026-07-15-rightcontext-feedback-rail.md) | Implemented | None | [Feedback-rail ADR](plans/2026-07-15-rightcontext-feedback-rail.md) | The accepted [IR-11 activation contract](plans/2026-07-17-rightcontext-independent-review-addendum.md) remains pending and does not supersede this decision. |
| [One-hop wikilink recall](plans/2026-07-15-rightcontext-link-graph-recall.md) | Implemented | None | [Link-graph ADR](plans/2026-07-15-rightcontext-link-graph-recall.md) | — |
| [Trust-gated memory-content delivery](plans/2026-07-15-rightcontext-memory-delivery.md) | Implemented | None | [Memory-delivery ADR](plans/2026-07-15-rightcontext-memory-delivery.md) | The [harness-protocol ADR](plans/2026-07-16-rightcontext-harness-protocol-adr.md) governs transport evolution. |
| [Warm-service inversion and three thin doors](plans/2026-07-16-rightcontext-harness-protocol-adr.md) | Proposed | None | [Harness-protocol ADR](plans/2026-07-16-rightcontext-harness-protocol-adr.md) | Proof requirements live in the [gate plan](plans/2026-07-16-rightcontext-gates-execution.md). |
| [Event-log compaction: checkpoint and acknowledgement](plans/2026-07-10-memright-compaction-checkpoint-ack.md) | Proposed | None | [Checkpoint-and-ack ADR](plans/2026-07-10-memright-compaction-checkpoint-ack.md) | Companion decisions: [crash-safe promotion](plans/2026-07-10-memright-compaction-crash-safe-epoch.md) and [replica bootstrap](plans/2026-07-10-memright-compaction-replica-bootstrap.md). |
| [Event-log compaction: crash-safe epoch promotion](plans/2026-07-10-memright-compaction-crash-safe-epoch.md) | Proposed | None | [Crash-safe epoch ADR](plans/2026-07-10-memright-compaction-crash-safe-epoch.md) | Depends on the [checkpoint-and-ack ADR](plans/2026-07-10-memright-compaction-checkpoint-ack.md). |
| [Event-log compaction: replica bootstrap](plans/2026-07-10-memright-compaction-replica-bootstrap.md) | Proposed | None | [Replica-bootstrap ADR](plans/2026-07-10-memright-compaction-replica-bootstrap.md) | Depends on the [checkpoint-and-ack ADR](plans/2026-07-10-memright-compaction-checkpoint-ack.md). |

Update this index only when a listed decision is accepted, implemented, superseded, or replaced.
Do not copy live measurements or review prose into it.
