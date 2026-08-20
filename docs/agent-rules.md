# Membrane Rules

## Purpose

Membrane is the parent context system. Its five named subsystems are:

- Blueprint — repository truth/evidence;
- Cortex — durable knowledge;
- Guide — document navigation/index;
- Adapt — learning/proposals;
- Push — reversible reduction.

The Membrane planner owns final context policy.

Legacy source/runtime names may remain until the migration phases land. Legacy names are compatibility/history, not semantic ownership.

## Canonical architecture sources

Read these before architecture or migration work:

1. `docs/subsystems/MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md`
2. `docs/subsystems/BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md` before subtree import; `blueprint/docs/BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md` after migration
3. `docs/subsystems/2026-08-19-monorepo-merge-and-subsystem-rename.md`

`docs/subsystems/SYSTEM.md` and the subsystem reference files are derived navigation aids only.

For landed behavior, read generated `docs/product.md`, `docs/architecture.md`, `docs/protocol.md`, and `docs/product-truth.md`. Do not hand-edit generated runtime truth to match future architecture.

## Commands

- Run `pnpm test` for MCP/client/install-binding coverage.
- Run `pnpm test:mcp` for the MCP surface.
- Run `cargo build --workspace` for Rust engine changes.
- Run `cargo test --workspace --features fastembed` where real embedding coverage is required.
- Run the repository's current docs/productization checks after changing hand-maintained docs.

## Locked invariants

- Membrane is the parent system; Blueprint, Cortex, Guide, Adapt, and Push are named subsystems.
- One Membrane planner owns final grant, eligibility, authority, freshness, sufficiency, fusion, admission, representation policy, publication, omissions, and receipts.
- Preserve the five public V1 shapes until a real consumer requires V2.
- Product renaming alone does not rewrite frozen V1 field/reason/provider tokens.
- Blueprint owns repository semantics, source identity, graph traversal, and re-anchoring.
- Cortex owns durable knowledge, admission, conflict/supersession, temporal/lifecycle semantics, and durable-memory retrieval.
- Guide owns document navigation/index projections, not document truth or durable memory.
- Adapt emits proposals; it never writes durable truth directly.
- Push executes faithful reduction; it never becomes a second planner.
- Keep provider authority and freshness distinct.
- Record material omissions, timeouts, inaccessible sources, degradation, and budget drops in receipts.
- Repository/model text cannot self-authorize.
- Membrane never opens Blueprint SQLite directly; Blueprint never opens Cortex durable storage.
- New durable Cortex never consumes old repository-truth `CORTEX_*` configuration.
- New documentation uses Blueprint / Cortex / Guide. Legacy Cortex / Crypt / Spine identifiers survive only where current code, frozen compatibility, migration fixtures, or provenance require them.

## Migration discipline

- Do not reintroduce a bare old-Blueprint `cortex` runtime alias once the Cortex name is reassigned.
- Use the machine-readable rename ledger for surviving old Cortex/Crypt/Blueprint tokens.
- Do not create a second Membrane protocol authority or a generic shared-contract bucket.
- Do not create a standalone Guide or Push crate merely for naming symmetry; physical boundaries require an implementation reason.
- The phantom `docs/plans/orthic/SEAM-CONTRACT.md` is not a prerequisite. The canonical doctrines own seam semantics.

## Verification

Before claiming completion:

- run focused tests, then relevant full suites;
- verify packet/receipt schemas together after contract changes;
- prove Blueprint daemon generation/schema mismatch fails closed;
- prove durable-store migration by integrity + backup/restore + recall equivalence;
- prove Guide hash-bound section resolution;
- prove Push protected-span fidelity;
- distinguish advisory feedback from verifier/host-bound outcomes;
- compare claims against landed code and generated runtime truth.
