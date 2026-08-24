# Membrane transcript normalization

**Status:** retired Python differential namespace, not an independent product or Membrane subsystem

**Native owner:** `engine/crates/membrane-transcript` under [`../migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md`](../migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md) §5

Canonical multi-host transcript normalization lives in native
`membrane-transcript`. It emits deterministic `TranscriptEventV1` events,
source byte spans, parser receipts & typed failures.

Missing or inaccessible transcripts raise typed failures; callers must not
turn omission into an empty-success result.

Python code here is release-excluded differential evidence only. `mcp/host/continuity.mjs` is a separate host checkpoint adapter governed by N6 MCP cutover, not this package.
