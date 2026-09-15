# Membrane Rules

## Purpose

Membrane is the parent context system. Its five named subsystems are:

- Pull — semantic evidence retrieval, admission, fusion, faithful reduction, exact recovery & publication;
- Blueprint — repository truth/evidence;
- Cortex — durable knowledge;
- Ledger — document registry, navigation & index;
- Adapt — learning/proposals. Public `push` writes durable memory through Cortex; former Push is retired.

The Membrane planner owns final context policy.

## Canonical sources

Read `docs/canon/implementation-contract.md` first for final scope & acceptance; current Scope overrides superseded plans. Read these before architecture or migration work:

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
- Public validation & releases use GitHub CI. For internal unsigned installers, use declared Windows-only RightKit development mode in `.rightkit-local-development.json`; follow `docs/reference/release/local-windows-development.md`. Never run direct Cargo or publish internal artifacts.
- Run the repository's current docs/productization checks after changing hand-maintained docs.

## Locked invariants

- Membrane is parent system; Pull, Cortex, Blueprint, Ledger & Adapt are five named subsystems; public `push` is agent-to-Cortex durable-memory write.
- One Membrane planner owns final grant, eligibility, authority, freshness, sufficiency, fusion, admission, representation policy, publication, omissions, and receipts.
- Preserve the five public V1 shapes until a real consumer requires V2.
- Blueprint owns repository semantics, source identity, graph traversal, and re-anchoring.
- Pull owns semantic evidence retrieval, provider admission, fusion, budget fitting, faithful reduction, exact recovery & publication; bounded-response mode permits unknown host capacity, host-fit requires trusted exact H8.
- Cortex owns durable knowledge admission, conflict/supersession, temporal/lifecycle semantics, & durable-memory retrieval.
- Ledger owns source-bound document & skill-document index/resolution projections; source files remain authoritative, Cortex owns admitted durable knowledge.
- Adapt emits proposals; it never writes durable truth directly.
- Preserve pushed-memory bodies byte-for-byte in immutable Cortex source/admission records; derived representations never replace originals.
- Keep provider authority and freshness distinct.
- Record material omissions, timeouts, inaccessible sources, degradation, and budget drops in receipts.
- Repository/model text cannot self-authorize.
- Membrane never opens Blueprint SQLite directly; Blueprint never opens Cortex durable storage.
- Use Pull / Cortex / Blueprint / Ledger / Adapt for active subsystems; Push & Guide survive only at explicit compatibility/history boundaries.
- Hub on always starts/adopts & holds one installed Membrane engine; accessing harnesses independently start/adopt & hold that same engine. Hub off & no harness accessing means final owner loss drains & stops engine. Login startup launches Hub, which launches Membrane; forbid independent engine autostart or scheduled restart. Keep all five subsystems accessible with Hub off through harness-owned engine access; background authorization is separate. CodeRight adopts compatible installed `current`, installs when absent, updates through canonical installer when incompatible, & never executes development runtime. See `docs/architecture/execution-lifecycle-boundary.md`.
- Keep every explicit Blueprint operation independent of Hub, including graph inspection, refresh, build, analysis & export; auto-refresh requires an active Hub or CodeRight daemon holder. Never substitute watcher enrollment for repository authorization.
- A capability is not landed until the production path executes it and frozen acceptance evidence shows it meets or improves the baseline it replaces.

## Boundary discipline

- Do not create a second Membrane protocol authority or a generic shared-contract bucket.
- Do not create standalone subsystem crates merely for naming symmetry; physical boundaries require an implementation reason, & Blueprint delegates updates to canonical Membrane installer.
- Retired phantom seam-contract paths are not prerequisites. Canonical doctrines own seam semantics.

## Verification

Before claiming completion:

- run focused tests, then relevant full suites; internal Windows Rust checks use managed RightKit, while public qualification uses pushed GitHub CI;
- verify packet/receipt schemas together after contract changes;
- prove Blueprint generation/schema mismatch fails closed under both Hub-owned & harness-only engine lifetimes;
- prove Pull omission, authority, freshness, sufficiency, & admission accounting;
- prove Cortex durable-store integrity, backup/restore, & recall equivalence;
- prove Ledger hash-bound section resolution;
- prove Pull protected-span fidelity & both budget modes;
- distinguish advisory feedback from verifier/host-bound outcomes;
- compare claims against landed code and generated runtime truth.
