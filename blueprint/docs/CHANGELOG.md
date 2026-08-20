# Changelog

## Unreleased

- Blueprint is the canonical CLI. README, SKILL, IMPLEMENTATION-STATUS, and package description use
  Blueprint naming; prose brands nodes/edges/flows as Neurons/Synapses/Circuits. No unshipped
  executable alias is claimed.

- P1 orientation admission library: `lib/admission.mjs` (`orient`/`expand`/`status`/`revoke`),
  host-owned `lib/receipt-store.mjs`, Forge-consumable `lib/orientation-evidence.mjs` (no hooks /
  shell classifier / MCP).
- Standalone package surface: `@orthic-labs/blueprint@0.2.0` with `bin`, `files`, `engines`,
  `exports`; workspace contract tests moved to `tests/workspace/`.
- Reconciled the portable manifest producer and bootstrap consumer around one nested generation contract.
- Corrected every portable graph artifact reference to the sole SQLite store at `.agent/graph/graph.db`.
- Replaced largest-file Phase-2 anchors with deterministic claim-relevance and cross-file graph-connectivity ranking.
- Added build-to-bootstrap and graph-ranked anchor regressions.
- Declared exact hashing, Tree-sitter runtime, and grammar dependencies so a clean checkout can run Blueprint and its tests.
- Enforced LF text checkout so frozen evidence hashes remain identical across Windows and macOS.
