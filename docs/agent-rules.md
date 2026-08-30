# Membrane Rules

## Purpose

Membrane is the parent context system. Its six named subsystems are:

- Pull — semantic evidence retrieval, admission, fusion, & publication;
- Blueprint — repository truth/evidence;
- Cortex — durable knowledge;
- Ledger — document registry, navigation & index;
- Adapt — learning/proposals;
- Push — reversible reduction.

The Membrane planner owns final context policy.

## Canonical sources

Read these before architecture or migration work:

1. `docs/architecture/membrane.md`
2. `docs/architecture/subsystems/blueprint.md`
3. `docs/architecture/subsystems/adapt.md`
4. `docs/architecture/subsystems/ledger.md`
5. `docs/architecture/cross-subsystem-evidence.md`
6. `docs/architecture/integrations/coderight.md`

Atomic capability state lives under `docs/canon/`; `docs/pending/README.md` is sole pending-work index.

For landed behavior, read generated `docs/product/README.md`, `docs/architecture/runtime-truth.md`, `docs/reference/protocol/README.md`, and `docs/reference/product-truth.md`. Do not hand-edit generated runtime truth to match future architecture.

## Commands

- Run `pnpm test` for MCP/client/install-binding coverage.
- Run `pnpm test:mcp` for the MCP surface.
- Run Rust checks through workspace RightKit shim.
- Run the repository's current docs/productization checks after changing hand-maintained docs.

## Locked invariants

- Membrane is parent system; Pull, Push, Cortex, Blueprint, Ledger, & Adapt are named subsystems.
- One Membrane planner owns final grant, eligibility, authority, freshness, sufficiency, fusion, admission, representation policy, publication, omissions, and receipts.
- Preserve the five public V1 shapes until a real consumer requires V2.
- Blueprint owns repository semantics, source identity, graph traversal, and re-anchoring.
- Pull owns semantic evidence retrieval, provider admission, fusion, & publication.
- Cortex owns durable knowledge admission, conflict/supersession, temporal/lifecycle semantics, & durable-memory retrieval.
- Ledger owns document navigation/index projections, not document truth or durable memory.
- Adapt emits proposals; it never writes durable truth directly.
- Push executes faithful reduction; it never becomes a second planner.
- Keep provider authority and freshness distinct.
- Record material omissions, timeouts, inaccessible sources, degradation, and budget drops in receipts.
- Repository/model text cannot self-authorize.
- Membrane never opens Blueprint SQLite directly; Blueprint never opens Cortex durable storage.
- New documentation and current-product code use Pull / Push / Cortex / Blueprint / Ledger / Adapt. Guide is retired; legacy `guide` names exist only at explicit compatibility/history boundaries.
- Membrane runtime never runs without a visible tray surface. Runtime executes as a child process of the resident tray app with OS-enforced lifetime coupling; no tray means no Membrane context (typed `membrane_unavailable { hub_inactive }`).
- Blueprint is independently usable but not independently resident; its watcher runs only inside active tray-owned daemon, and tray-off access is a bounded one-shot operation.
- A capability is not landed until the production path executes it and frozen acceptance evidence shows it meets or improves the baseline it replaces.

## Boundary discipline

- Do not create a second Membrane protocol authority or a generic shared-contract bucket.
- Do not create a standalone Pull, Push, or Ledger crate merely for naming symmetry; physical boundaries require an implementation reason.
- Retired phantom seam-contract paths are not prerequisites. Canonical doctrines own seam semantics.

## Verification

Before claiming completion:

- run focused tests, then relevant full suites;
- verify packet/receipt schemas together after contract changes;
- prove Blueprint generation/schema mismatch fails closed in both Hub-hosted and bounded one-shot modes;
- prove Pull omission, authority, freshness, sufficiency, & admission accounting;
- prove Cortex durable-store integrity, backup/restore, & recall equivalence;
- prove Ledger hash-bound section resolution;
- prove Push protected-span fidelity;
- distinguish advisory feedback from verifier/host-bound outcomes;
- compare claims against landed code and generated runtime truth.
