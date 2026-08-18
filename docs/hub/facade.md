# Membrane Hub facade (CU-20)

Read-only facade producers (seam-rescoped, BOUNDED). Extends `hub_readonly_db.rs` and surfaces
`hub.capabilities` / `snapshot` / `deliveries` / `providers` / `repositories` / `adapters` / `memory` / `sentinel`
plus `hub.delivery.get/trace` (backed by CU-11's live `delivery_trace_view.rs` wiring, not a stub).

## Sections

- `capabilities`, `snapshot`, `deliveries`, `providers`, `repositories`, `adapters`, `memory`, `sentinel` — read from existing producers (`sources_producer.rs`, `agent_adapter_producer.rs`, `memory_sentinel_producer.rs`, `delivery_trace_view.rs`). Fixture asserts producer functions are called, not reimplemented.
- `devices`/`alerts` — remain `not_instrumented` (deferred per CU-25, O-4). Regression guard: grep for `not_instrumented` must match these sections only.

## Transport

No HTTP transport here — that is CU-H03's scope. This crate produces data structures a transport layer calls into. The facade's public function signatures match what CU-H03's HTTP handlers call (checked via `cargo check -p membrane-runtime`).

## Generation

Snapshot schema and status-dimension enum unchanged from product-shape CU-P13. No mutation, no HTTP transport.
