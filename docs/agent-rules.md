# Membrane Rules

## Purpose
Membrane assembles minimal current context packets and receipts from typed local providers.
Crypt is its durable-memory subsystem.

## Canonical sources
- Read `README.md` for product contracts and measured behavior.
- Read `docs/architecture.md` for components, flows, and provider boundaries.
- Read `docs/design/MEMBRANE-STATE.md` for live rollout state.

## Commands
- Run `pnpm test` for MCP, client, and install-binding coverage.
- Run `pnpm test:mcp` for the MCP surface.
- Run `cargo build --workspace` for Crypt engine changes.
- Run `cargo test --workspace --features fastembed` for real embedding coverage.

## Locked invariants
- Preserve typed `ScopeGrant`, candidate, packet, receipt, and knowledge-emission contracts.
- Keep provider authority and freshness distinct instead of flattening sources.
- Record omissions, timeouts, inaccessible sources, and budget drops in receipts.
- Keep data local, loopback-bound, and repository-confined.
- Let fresh code evidence outrank stale documents and memory.
- Preserve current Crypt compatibility shims and RightContext telemetry aliases.
- Report degraded provider state instead of silently claiming full context.

## Verification
- Run focused provider or admission tests before the full suite.
- Check packet and receipt schemas together after contract changes.
- Measure warm federation behavior when modifying gateway concurrency or budgets.

Before sealing any contract touching hub, watcher lifecycle, the cortex↔membrane API, or peer-service discovery, read `docs/plans/orthic/SEAM-CONTRACT.md` and declare it a dependency.
