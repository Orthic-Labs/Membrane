# Parity & Phase 2 inventory

Status: complete (read-only inventory). Lane: `parity-phase2-inventory`. Baseline: `2e8e57bbb1e368332b37466080f1e925f2f4726e` (packet source revision and current `HEAD`).

## Receipt / scope

Receipt verified with:

```powershell
$head=(git rev-parse HEAD).Trim(); $packet=(Get-FileHash audit/remediation/blueprint-native-reconciliation.dispatch.json -Algorithm SHA256).Hash.ToLower(); $head; $packet
```

Observed `HEAD=2e8e57bbb1e368332b37466080f1e925f2f4726e`; packet digest `8ab6486fa2c3086e9ac9f49d69aa876af9710d2d0f4021b657b24a81f9f990ca`, matching `dispatch.receipt.json`. Authority hashes for `AGENTS.md` and `blueprint/AGENTS.md` also match receipt. No tests, builds, generators, installs, Cargo commands, graph rebuilds, or watcher activation were run.

## Test classification A–E

Classification follows closure brief §17 (`D:/Downloads/MEMBRANE-BLUEPRINT-NATIVE-RUST-CLOSURE.md:650-678`). Paths below are source-grounded representatives; adjacent tests with same contract inherit classification.

| Class | Current JS/Python tests & evidence | Disposition / later Rust gate |
|---|---|---|
| A — contract / behavior | `blueprint/tests/graph-query.test.mjs`, `graph-cli.test.mjs`, `graph-substrate.test.mjs`, `doc-truth-projection.test.mjs:15-44`, `superseded-docs.test.mjs:18-269`, `publication-boundary.test.mjs:19-75`, `relationship-parity.test.mjs:20-57`, `store-sqlite.test.mjs` | Port to `engine/crates/membrane-blueprint/tests/` (query, doc truth, publication, relationship, storage modules) and retain language-neutral fixtures. |
| B — differential migration | `blueprint/tests/adapter-parity.test.mjs:1-24,136-180` (same logical requests across five adapters), `build-recovery-equivalence.test.mjs:10-17,42-100`, `retrieval-equivalence.test.mjs`, `resolver-ghost-edge-equivalence.test.mjs`, `incremental-equivalence-config-provider.test.mjs` | Keep until native outputs match frozen envelopes; then delete JS harnesses. Rust differential harness: `engine/crates/membrane-blueprint/tests/parity.rs`; fixtures under `engine/crates/membrane-blueprint/tests/fixtures/`. |
| C — retired implementation internals | Node launcher/process-specific assertions in `blueprint/tests/cli-entrypoints.test.mjs`, `blueprint/tests/cli-contract.test.mjs`, `blueprint/tests/mcp-server.test.mjs`, `blueprint/tests/ipc-protocol.test.mjs`, `blueprint/tests/daemon-recovery.test.mjs` where assertion targets JS module/process wiring rather than observable protocol | Delete only after native CLI/MCP/service gates pass. Preserve observable cases in `engine/crates/membrane-blueprint/tests/contract.rs` or `engine/crates/membrane-mcp/tests/`. |
| D — end-to-end product | `blueprint/tests/clean-host-smoke.test.mjs`, `installer.test.mjs`, `service-install.test.mjs`, `standalone-completion.test.mjs`, `runtime-bundle.test.mjs` | Rewrite against installed native binaries in `engine/crates/membrane-blueprint/tests/installed.rs` plus repository qualification fixture; no Node/Python backend dependency. |
| E — fixture generator | `blueprint/fixtures/stores/build-stores.mjs:1-13,53-89`; `blueprint/evals/equivalence/*.json`; `blueprint/evals/fixture-repos/*` | Replace executable store generation with checked-in language-neutral DB/JSON fixtures or a native fixture builder. Keep `recovery-corpus-v1`, `ghost-edge-corpus-v1`, and retrieval corpus as parity inputs. Proposed `engine/crates/membrane-blueprint/tests/fixtures/` and `engine/crates/membrane-blueprint/tests/fixture_builder.rs`. |

## Parity fixture ledger

* `blueprint/evals/fixture-repos/typescript-commerce` is the adapter-parity repository (`adapter-parity.test.mjs:43-47,59-64`); compare normalized status/search envelopes while stripping only named transport volatility (`:96-119`).
* `blueprint/evals/equivalence/recovery-corpus-v1.json:1-18` fixes cancellation, failed publication rollback, and poisoned snapshot recovery codes.
* `blueprint/evals/equivalence/ghost-edge-corpus-v1.json:1-78` fixes add/delete/move/rename and ambiguity transitions, including unresolved edges and deterministic ordering.
* `blueprint/evals/equivalence/retrieval-corpus-v1.json:1-73` fixes generation-pinned lexical ranking, cap, and `generation_not_found` omission.
* `blueprint/fixtures/stores/build-stores.mjs:1-13,53-83` materializes real capped-schema stores through `saveGeneration`, not hand-rolled tables. Current source creates v16/v15 fixtures (`:61-70`); Phase2 report must add v20 compatibility fixtures later without changing this lane.
* Semantic/doc truth fixtures: `blueprint/tests/doc-truth-projection.test.mjs:15-44`, `doc-code-join-index.test.mjs:5-29`, `superseded-docs.test.mjs:18-269`.

## Phase 2 semantics to preserve

The closure brief requires document discovery, claims, source references, claim fingerprints, verification planning, sealing, unchanged-verdict reuse, contradiction handling, stale/superseded/historical handling, machine artifacts, and canonical generated outputs (`D:/Downloads/MEMBRANE-BLUEPRINT-NATIVE-RUST-CLOSURE.md:537-559`).

Current implementation evidence:

* `blueprint/src/lib/incremental-phase2.mjs:3-18,54-94` defines versioned dimensions, eligible claims, file-hash/claim fingerprints; `:176-243` plans verdict/dimension reuse vs verification/synthesis; `:249-315` seals only complete evidence, dependencies, and generation-bound metadata.
* `blueprint/tests/incremental-phase2.test.mjs:72-264` proves cold scheduling, unchanged reuse, new-file invalidation, changed-file/dependent-dimension invalidation, missing-evidence fail-closed behavior, changed-verdict invalidation, and human-choice/fingerprint requirements.
* `blueprint/src/lib/phase2-completion.mjs:1-15,79-155` consumes deterministic `doc` pending work, generation-fences completion, reseals reusable artifacts, leaves explicit `phase2-plan.json` when judgment remains, and clears `semantic` only when complete.
* `blueprint/tests/watchman-phase2-completion.test.mjs:27-94` proves automatic document completion does not perform explicit judgment and cannot clear a newer generation's pending mark.
* `blueprint/tests/doc-truth-projection.test.mjs:15-35` requires direct/indirect, contradicted, ambiguous, unsupported, stale outcomes; direct evidence uses nullable confidence while heuristic evidence may retain confidence.
* `blueprint/tests/superseded-docs.test.mjs:18-80,91-153` keeps historical documents mapped while excluding them from live inputs; malformed, external, absolute, and symlink escape markers remain explicit rather than silently retiring current docs.

Phase2 native acceptance must assert current executable/source evidence outranks stale plans/history, one generation fence covers graph + artifacts, unchanged verdicts reuse only with matching fingerprints, missing evidence never yields cache hit, contradictions remain visible, and judgment work never runs implicitly in watcher completion.

## Schema-v20 compatibility & deletion gates

`blueprint/src/graph/store-sqlite.mjs:723-800` defines generation-pinned `documents`, `claims`, `claim_code_edges`, and `document_supersession` tables; body prose remains source-backed while structure/spans are queryable per generation. `:945-952` makes v19 fact confidence and v20 doc/code-join confidence nullable, with `SCHEMA_VERSION` derived from migration count. `blueprint/tests/doc-truth-projection.test.mjs:37-44` is the explicit v20 nullable-confidence gate.

Compatibility fixtures must cover: v20 stores open read/write without column drift; nullable `claim_code_edges.confidence` survives round-trip; all four doc tables retain generation IDs, foreign-key deletion semantics, evidence spans, lifecycle/supersession fields, and deterministic row ordering; older v15/v16 fixtures still migrate with no loss (`store-migrations.test.mjs:1-13,40-181`; builder `build-stores.mjs:1-13`). Migration backup/repair and typed rollback remain gates before deleting JS storage (`store-migrations.test.mjs:85-131,171-181`).

Deletion gates are ordered: freeze parity fixtures; native Rust output equals JS or has reviewed correction; native schema migration/rollback and Phase2 tests pass; installed native CLI/MCP/service tests pass; then remove only implementation-specific JS tests and obsolete executable. Behavioral/fixture/evaluation assets remain unless their Rust replacement is byte/semantic equivalent. This follows brief §16.1–17 (`...CLOSURE.md:609-678`).

## Exact later Rust/test file proposals

1. `engine/crates/membrane-blueprint/src/phase2.rs` — fingerprints, incremental plan, sealing, contradiction/stale/supersession state machine.
2. `engine/crates/membrane-blueprint/src/doc_truth.rs` — discovery, claims, source refs, joins, lifecycle, generated artifacts.
3. `engine/crates/membrane-blueprint/src/store.rs` and `src/migrations.rs` — schema-v20 tables, nullable confidence, generation fencing, backup/rollback.
4. `engine/crates/membrane-blueprint/tests/phase2.rs`, `tests/doc_truth.rs`, `tests/schema_v20.rs`, `tests/migrations.rs` — focused behavior and compatibility gates.
5. `engine/crates/membrane-blueprint/tests/parity.rs` plus `tests/fixtures/{typescript-commerce,recovery-corpus-v1.json,ghost-edge-corpus-v1.json,retrieval-corpus-v1.json}` — differential parity.
6. `engine/crates/membrane-blueprint/tests/installed.rs` and `engine/crates/membrane-mcp/tests/contract.rs` — Class D/native adapter boundaries.

## Required return fields

`status`: complete. `summary`: parity, Phase2, fixture, schema-v20, and deletion-gate inventory produced. `acceptance`: cited A–E classification, parity fixtures, Phase2 semantics, v20 gates, and exact Rust destinations above. `artifacts`: `audit/remediation/reconciliation/parity-phase2-inventory.md`. `changes`: one new report file only. `commands`: read-only `Get-Content`, `rg`, `git rev-parse`, and `Get-FileHash`; no forbidden checks. `recovery`: none. `deviations`: validator executable was not present in workspace; packet/receipt SHA and authority bindings were verified directly with `Get-FileHash`. `blocker`: none. `next`: integration owner reviews citations and freezes later implementation packet. `baselineRevision`: `2e8e57bbb1e368332b37466080f1e925f2f4726e`. `citations`: all paths/line ranges in this report.
