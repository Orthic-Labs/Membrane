# Decision: Public Rust crate

**Status:** Declined
**Owner:** Orthic Labs maintainers
**Date:** 2026-08-06
**Packet:** D53

## Decision

Blueprint does **not** publish a public Rust crate. The core is shipped as a
Node module (ESM + native `node:sqlite` and WASM grammars). A Rust client
is out of scope for 1.0.

## What was measured

- A Rust crate would give us tighter control over memory layout, faster
  parse for the large-class envelope budgets, and a more natural story
  for shipping CLI-only tools.
- Blueprint's primary public surface is a local CLI/MCP/SDK that runs in
  the user's existing language toolchain. The feature gap is on the
  language-server / query side, not on the implementation side.
- A Rust rewrite of the core would, in the runbook's own do-not-absorb
  list ("Do not rewrite the core in Rust or publish a placeholder
  crate"), be the wrong choice for 1.0.

## Why declined, not deferred

- A placeholder crate invites low-quality bindings that take ownership
  of the contract surface from the Node SDK. The SDK already provides
  the typed client (`BlueprintClient`, `EmbeddedBlueprintClient`).
- A real Rust crate would be a multi-quarter project requiring a
  full ABI-equivalent reproduction of the current evidence engine
  (SQLite store + Merkle ledger + parser walker). That is a separate
  product, not a release surface.

## Reversal conditions

- The Node SDK shows a documented community demand for first-class
  Rust bindings that the SDK cannot serve cleanly via FFI.
- A standalone Rust client (not a rewrite of the core) is proposed with
  a concrete contribution path.
