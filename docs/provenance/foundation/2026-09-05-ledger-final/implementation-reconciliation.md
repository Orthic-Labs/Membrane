# Ledger implementation reconciliation — 2026-09-05

**Branch:** `ledger-end-to-end`
**Source revision:** `62243a9c099b53e5ef1694739aec8b9ca277b055`
**Managed source/type-check run:** `33952026427`
**Audit baseline:** `75c257ad711d19ffce69258d132a45dbffa9b4ac`

This record reconciles the final Ledger design/canon package against the implementation source added after the original audit. It is intentionally narrower than a release receipt.

## Evidence actually obtained

- The branch source contains the daemon-owned Ledger service, stateless operational CLI adapter, MCP document surface and native federation Ledger provider.
- Exact scope-grant read ranges are persisted through the runtime catalog and supplied to native Ledger retrieval; a partial grant is never interpreted as unrestricted access.
- Source identity/lifecycle repairs cover equal-content copies, tombstone resurrection, imported-conversion isolation, durable exclusions and root-local generations.
- Internal projection is independent of outline presentation pagination; section/block/link projection shares one Comrak AST; stable node identity excludes unrelated global order.
- Exact registered-node/imported-snapshot resolution carries expected raw hash, revision, projection/span evidence, generation, bounded UTF-8-safe continuation and source-bound tickets.
- Source-scoped exact, FTS, literal and bounded graph-alternative lanes share current eligibility, work budgets and policy revalidation.
- Read-only related-node, backlink/link-health and named-manifest structural-drift operations are present through the daemon owner.
- The managed branch lane passed atomic-canon/source checks and `cargo check --manifest-path engine/Cargo.toml --workspace --tests --locked` on the named source revision. This compiles workspace and test sources; it does not execute Rust tests.

## Deliberately not promoted

- `ledger::qualification::QUALIFIED_DELIVERIES` remains empty. Automatic native Ledger delivery is therefore not release-qualified; the provider-enable flag remains off by default.
- No Rust test binary, application binary, installer, package, tray runtime or CodeRight session was executed in this implementation pass because the active constraint prohibits builds/runtime execution here.
- Public non-Markdown ingest is not advertised through the generic Ledger MCP/CLI surface. Format support remains subject to LDG-028 per-format semantic/integrity/installed round-trip qualification.
- LDG-023 and CTX-033 remain exploratory/HOLD. No change feed or automatic Cortex truth mutation was promoted.

## Contracts still partial after source implementation

1. **LDG-004/024 — move/alias history:** copy identity and stable node identity are repaired, but a qualified rename/move evidence protocol and complete alias-history transition remain partial.
2. **LDG-017/018 — publication/update concurrency:** generations are transactional and root-local, but sync still performs substantial source read/parse work under the Ledger write transaction. Shorter preparation/publication transactions, interruption and cross-repository latency need executed evidence.
3. **LDG-022 — automatic delivery:** the native provider and resolver path are source-wired, but installed host delivery is intentionally gated until release-specific qualification.
4. **LDG-028 — converted formats:** internal deterministic conversion/snapshot resolution exists, but advertised formats need separate format qualification.
5. **LDG-029/030/031:** source implementations are present, but verification, qualification and release delivery remain pending.

The correct atomic representation is therefore source implementation `PARTIAL` where the broadened acceptance boundary is not yet proven, not `DELIVERED` merely because code exists. Historical narrower successes remain historical evidence.
