# Hub

Membrane Hub is the read-only facade over runtime state. Start with the overview, then
go to the resource doc you need.

- [overview.md](overview.md) — resources, schema, and staleness/error semantics
- [facade.md](facade.md) — the read-only facade crate (`hub_readonly_db.rs`), sections, and transport boundary
- [actions.md](actions.md) — post-v1 action request builders (restart, reconcile, token rotation, etc.)
- [agents-adapters.md](agents-adapters.md) — `agent-adapters.v1` client/device projection
- [delivery-trace.md](delivery-trace.md) — `delivery-trace.v1` per-trace projection
- [memory-sentinel.md](memory-sentinel.md) — Cortex memory-lifecycle read model
- [notifications.md](notifications.md) — sparse notification state (MBR-711)
- [sources-explorer.md](sources-explorer.md) — `sources-explorer.v1` repository/provider projection

See [../hub-handoff.md](../hub-handoff.md) for the lifecycle-conformance handoff contract.
