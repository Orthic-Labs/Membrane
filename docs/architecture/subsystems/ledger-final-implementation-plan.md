# Ledger final implementation plan

Status: source implementation landed on `ledger-end-to-end`; final release qualification and installed-host verification remain deliberately pending.

## Final shape

Ledger is the daemon-owned, source-bound document-navigation subsystem. It registers eligible sources, publishes rebuildable structural/search/link projections, resolves exact captured evidence, and offers bounded navigation to agents. Pull remains the only cross-provider admission/fusion authority; Blueprint remains repository/code truth; Cortex remains durable semantic memory; Push remains representation reduction.

The implemented path is:

`harness / MCP / stateless CLI → tray-owned daemon → Ledger owner → scoped exact/FTS/literal/graph retrieval → native Ledger provider → Pull admission → exact source resolver → delivered evidence receipt`.

## Landed repairs

- distinct identity for copied equal-content sources and correct tombstone resurrection;
- source-owner-aware reconciliation so Markdown scans do not retire imported conversions;
- complete internal projection beyond 256 headings and one shared Comrak AST for section/block/link projections;
- stable node identity independent of unrelated global order;
- exact registered node and imported-snapshot resolution with raw/projection/span verification and UTF-8-safe continuation;
- repository/grant eligibility before ranking, exact-first retrieval, source-verified literal matching and bounded structural/link alternatives;
- repository-local Git-ignore semantics, policy revalidation, deadline/cancellation and byte/item bounds;
- one daemon Ledger owner, stateless operational CLI, MCP discovery/reader, and native federation provider;
- durable erasure exclusions outside the rebuildable index;
- LDG-029 backlinks/link health, LDG-030 literal span matching and LDG-031 named structural drift manifests.

## Deliberately gated

Automatic provider delivery remains disabled until a release-specific end-to-end qualification receipt exists. The historical FTS benchmark qualifies the earlier retrieval experiment, not the new owner/provider/resolver/host composition. `LDG-023` and `CTX-033` remain exploratory. Public non-Markdown format claims remain subject to LDG-028 per-format qualification. No Tantivy migration, source-mutating hidden IDs, second resident process, or second planner is introduced.

## Validation boundary

During concurrent subsystem integration the branch must pass Ledger wiring checks and `cargo check --manifest-path engine/Cargo.toml --workspace --tests --locked`. `tests/ledger-wiring-contracts.test.mjs` freezes native/JavaScript MCP discoverability, daemon ownership, provider registration, Push coexistence and the qualification fence. Rust test binaries, application binaries, packaging, installation, activation and release qualification are intentionally not run under the current no-build constraint.

Before declaring every committed Ledger atom `RELEASED`, run the frozen installed-host qualification: exact release → normal harness request → Ledger provider → Pull decision/receipt → exact source read → evidence delivered, plus the per-capability negative, stale, revocation, concurrency and format suites recorded in the Ledger canon.
