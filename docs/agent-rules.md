# Membrane Rules

## Purpose

Membrane is the parent context system. Its six named subsystems are:

- Pull — semantic evidence retrieval, admission, fusion, & publication;
- Blueprint — repository truth/evidence;
- Cortex — durable knowledge;
- Guide — document navigation/index;
- Adapt — learning/proposals;
- Push — reversible reduction.

The Membrane planner owns final context policy.

## Canonical sources

Read these before architecture or migration work:

1. `docs/subsystems/MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md`
2. `docs/subsystems/BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md`

`docs/subsystems/SYSTEM.md` and the subsystem reference files are derived navigation aids only.

For landed behavior, read generated `docs/product.md`, `docs/architecture.md`, `docs/protocol.md`, and `docs/product-truth.md`. Do not hand-edit generated runtime truth to match future architecture.

## Commands

- Run `pnpm test` for MCP/client/install-binding coverage.
- Run `pnpm test:mcp` for the MCP surface.
- Run Rust checks through workspace RightKit shim.
- Run the repository's current docs/productization checks after changing hand-maintained docs.

## Locked invariants

- Membrane is parent system; Pull, Push, Cortex, Blueprint, Guide, & Adapt are named subsystems.
- One Membrane planner owns final grant, eligibility, authority, freshness, sufficiency, fusion, admission, representation policy, publication, omissions, and receipts.
- Preserve the five public V1 shapes until a real consumer requires V2.
- Blueprint owns repository semantics, source identity, graph traversal, and re-anchoring.
- Pull owns semantic evidence retrieval, provider admission, fusion, & publication.
- Cortex owns durable knowledge admission, conflict/supersession, temporal/lifecycle semantics, & durable-memory retrieval.
- Guide owns document navigation/index projections, not document truth or durable memory.
- Adapt emits proposals; it never writes durable truth directly.
- Push executes faithful reduction; it never becomes a second planner.
- Keep provider authority and freshness distinct.
- Record material omissions, timeouts, inaccessible sources, degradation, and budget drops in receipts.
- Repository/model text cannot self-authorize.
- Membrane never opens Blueprint SQLite directly; Blueprint never opens Cortex durable storage.
- New documentation uses Pull / Push / Cortex / Blueprint / Guide / Adapt.

## Boundary discipline

- Do not create a second Membrane protocol authority or a generic shared-contract bucket.
- Do not create a standalone Pull, Push, or Guide crate merely for naming symmetry; physical boundaries require an implementation reason.
- Retired phantom seam-contract paths are not prerequisites. Canonical doctrines own seam semantics.

## Verification

Before claiming completion:

- run focused tests, then relevant full suites;
- verify packet/receipt schemas together after contract changes;
- prove Blueprint daemon generation/schema mismatch fails closed;
- prove Pull omission, authority, freshness, sufficiency, & admission accounting;
- prove Cortex durable-store integrity, backup/restore, & recall equivalence;
- prove Guide hash-bound section resolution;
- prove Push protected-span fidelity;
- distinguish advisory feedback from verifier/host-bound outcomes;
- compare claims against landed code and generated runtime truth.
