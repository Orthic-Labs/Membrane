# Cortex remaining work focused acceptance receipt (tranche D: repair pass close-out)

Material runtime revision: `5be1f9fd443c22e69128988601963cee090823f9`.
Freshness: `2026-09-06`.

Managed proof: GitHub Actions pull-request run `34063008859` completed with
conclusion success on the material revision. `run-ci.sh` is `set -euo pipefail`
and runs `cargo check --manifest-path engine/Cargo.toml --workspace
--all-targets --locked` then `cargo test --manifest-path engine/Cargo.toml
--workspace --locked --no-fail-fast` before every later step, so a successful
run means the whole Rust workspace suite reported ok with 0 failed, the canon
checker passed, and the sealed runtime-language manifest and invocation graph
were current. The focused tests below are part of that green run. No Rust test
was executed locally; this receipt cites managed CI only.

This tranche supersedes the pre-repair evidence for the atoms whose behavior
moved underneath them. The immediately preceding run `34062409101` on revision
`fc790ac747b4c2c323f2acf09f943832e1cd2f44` failed only at
`check-runtime-language-manifest` (two STALE_DIGEST rows) — that is, it passed
the entire Rust phase — and the digest refresh is the sole difference between
that revision and this one.

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| CTX-001 | DELIVERED | `engine/crates/cortex-store/src/memdb.rs:2039-2050` (`MemDb::open`: sole SQLite open authority, WAL/busy-timeout/synchronous pragmas, `migrate`, legacy identity backfills, event-ledger extraction); migration ladder `memdb.rs:564-1343` | `engine/crates/membrane-runtime/src/store.rs:2307` (`MemoryStore::try_open`, the only other caller); tray-owned `serve.rs:5599`, `bin/membrane-daemon.rs:174` |  COMPLETE |
| CTX-009 | DELIVERED | `engine/crates/cortex-store/src/temporal.rs:479-600` (`record_validity_observed`); `engine/crates/membrane-runtime/src/cortex_lifecycle.rs:603-662` (`admit_temporal` passes the proposer's observation instant) | `engine/crates/membrane-runtime/src/mcp_executor.rs:1391`; `engine/crates/membrane-runtime/src/store.rs:1641,1703` (`TemporalFactStore::query`) |  COMPLETE |
| CTX-011 | DELIVERED | `engine/crates/cortex-store/src/fts5.rs:154-200` (`search` with the full eight-column bm25 weight vector) | `engine/crates/membrane-runtime/src/store.rs` (`fts5_lexical_hits`, recall lexical lane); `engine/crates/membrane-runtime/src/memory_provider.rs:471-545` |  COMPLETE |
| CTX-012 | DELIVERED | `engine/crates/cortex-core/src/vector_index.rs:41-80` (host-policy kernel dispatch) wired production-default via `engine/crates/membrane-runtime/src/store.rs:127-139,2323-2327,5127-5183` (`retrieve_hybrid_indexed`, legacy `retrieve_hybrid` as exact fallback) | Pull Cortex provider via `engine/crates/membrane-runtime/src/pull/federation_sources.rs` | COMPLETE |
| CTX-015 | DELIVERED | `engine/crates/membrane-runtime/src/store.rs` (`resolve_cited_verdict`, called from `record_feedback_observed`); `engine/crates/cortex-core/src/effectiveness.rs` (`EffectivenessGate`) | `engine/crates/membrane-runtime/src/mcp_executor.rs:1431-1456` |  COMPLETE |
| CTX-025 | DELIVERED | `engine/crates/membrane-runtime/src/store.rs` (`hard_erase`: transactional erase across memories, quarantine, links, tombstone and the FTS projection) | `engine/crates/membrane-runtime/src/cli.rs:961,5008` |  COMPLETE |
| CTX-026 | DELIVERED | `engine/crates/membrane-runtime/src/store.rs` (digest-sealed backup with length-prefixed field encoding) | `engine/crates/membrane-runtime/src/cli.rs:963,5013` |  COMPLETE |
| CTX-029 | DELIVERED | `engine/crates/membrane-runtime/src/store.rs` (`rebuild_fts5_from_canonical`); `engine/crates/cortex-store/src/fts5.rs:100-112` | `engine/crates/membrane-runtime/src/cli.rs:959` |  COMPLETE |

## Focused verification

| Capability targets | Focused command | Direct test evidence | Result | Run identity/time |
|---|---|---|---|---|
| CTX-011 | `cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast` | `store::tests::fts5_bm25_ranks_keyword_column_above_body_only_match` proves a document matching only the 2.0-weighted `keywords` column outranks a shorter document matching only the 1.0-weighted `content` column — impossible under the previous two-weight call, which bound both weights to UNINDEXED columns and left content and keywords tied at the default 1.0. | FOCUSED_PASS — 0 failures. | GitHub Actions managed CI run `34063008859`; revision `5be1f9fd443c22e69128988601963cee090823f9`; conclusion success; 2026-09-06. |
| CTX-025 | `cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast` | `store::tests::hard_erase_fails_when_fts_projection_delete_cannot_succeed` drops the FTS5 content shadow table so `cortex_fts5` remains listed in `sqlite_master` (the existence probe still fires and the delete is genuinely attempted) while the delete fails, and proves `hard_erase` returns `Err` and the canonical row survives rollback. | FOCUSED_PASS — 0 failures. | GitHub Actions managed CI run `34063008859`; revision `5be1f9fd443c22e69128988601963cee090823f9`; conclusion success; 2026-09-06. |
| CTX-009 | `cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast` | `ctx009_temporal_round_trip_keeps_observed_valid_recorded_expiry_distinct` proves recorded time is the admission instant — distinct from both the caller's observation instant and the validity start — while the fact-shaped read still returns the caller's observation, so all four dimensions survive admission. | FOCUSED_PASS — 0 failures. | GitHub Actions managed CI run `34063008859`; revision `5be1f9fd443c22e69128988601963cee090823f9`; conclusion success; 2026-09-06. |
| CTX-015 | `cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast` | `mem_024_cited_verdict_resolution::unresolvable_verdict_ref_is_rejected_and_never_ranks`, `resolvable_verdict_ref_ranks`, `advisory_never_ranks_even_with_a_verdict_ref_shaped_string` and `a_verdict_cannot_be_replayed_across_different_candidates` prove a cited verdict ranks only when it resolves to a durable verdict event bound to this trace and candidate, that resolution failure is rejected rather than silently demoted, and that one verdict cannot manufacture signal for a second candidate sharing its content hash. | FOCUSED_PASS — 0 failures. | GitHub Actions managed CI run `34063008859`; revision `5be1f9fd443c22e69128988601963cee090823f9`; conclusion success; 2026-09-06. |
| CTX-012 | `cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast` | `ctx012_hybrid_recall_identical_eligibility_across_vector_dispatch_settings` proves recall eligibility is identical under both host-policy vector dispatch settings, so the indexed kernel path and the exact legacy fallback agree and neither introduces a remote correctness dependency. This tranche re-pins the claim to the current head: the tranche-B receipt was issued against `42b40541ba4fc0703cd41b27cb44dd1c3173d7eb`, and the shared acceptance file has changed since, which is what made CTX-Q012 STALE. | FOCUSED_PASS — 0 failures. | GitHub Actions managed CI run `34063008859`; revision `5be1f9fd443c22e69128988601963cee090823f9`; conclusion success; 2026-09-06. |
| CTX-026 | `cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast` | `store::tests::backup_restore_round_trip_preserves_quarantine_governance_metadata` proves `quarantined_at` and `reason` — both TEXT NOT NULL with no default, and load-bearing because `restore_quarantined` branches on the `admission_conflict:` prefix — round-trip exactly, so a backup holding a quarantined row can actually be restored. `store::tests::backup_restore_round_trip_preserves_suppression_and_drops_nothing` proves the sealed dump drops no row. | FOCUSED_PASS — 0 failures. | GitHub Actions managed CI run `34063008859`; revision `5be1f9fd443c22e69128988601963cee090823f9`; conclusion success; 2026-09-06. |
| CTX-029 | `cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast` | `store::tests::erase_reindex_restore_cycle_keeps_erased_payload_absent` proves a hard-erased payload does not reappear after the vector and FTS projections are rebuilt from canonical content and after a restore, so projection rebuild cannot resurrect erased material. | FOCUSED_PASS — 0 failures. | GitHub Actions managed CI run `34063008859`; revision `5be1f9fd443c22e69128988601963cee090823f9`; conclusion success; 2026-09-06. |
| CTX-001 | `cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast` | `memdb::tests::v10_upgrade_to_latest_and_backout_preserve_all_recall_payloads`, `startup_wal_reports_file_backed_main_and_event_ledger` and `startup_wal_reports_in_memory_as_healthy_without_paths` prove one open authority with WAL integrity and a lossless migration/backout ladder through schema v26. | FOCUSED_PASS — 0 failures. | GitHub Actions managed CI run `34063008859`; revision `5be1f9fd443c22e69128988601963cee090823f9`; conclusion success; 2026-09-06. |

## Atoms that remain truthfully PARTIAL

These are recorded rather than promoted. Each names the specific missing thing.

| Capability | Why it is still partial | Owner of the residual |
|---|---|---|
| CTX-004 | Schema v26 binds `sensitivity` and `derivation` as durable columns, backfills every pre-existing row to the explicit `unavailable_legacy` marker, and `resolve_memory` reports both verbatim. No production write path classifies either field yet, so rows admitted today land on the marker by column default. | Cortex; in-repo. |
| CTX-010 | Time, version-change and outcome-change triggers all enqueue review without rewriting authority. Ledger- and Blueprint-originated triggers remain absent, and there is no Cortex-side intake seam — no table and no ingestion call — through which another subsystem could hand Cortex such a signal. | Membrane; in-repo. Not external merely because another subsystem originates the event. |
| CTX-017 | `linked_neighbors` and `relationship_graph` are converged onto the validated canonical relation view, and a dangling wikilink is diagnostic but non-traversable. Only `supersedes` has a durable persistence path; `supports`, `contradicts` and `derived_from` have no column or ingest syntax, so their traversal is convergence-ready but unexercised. | Cortex; in-repo. |
| CTX-023 | The background semantic provider is a real loopback client requiring `MEMBRANE_BACKGROUND_SEMANTIC_PROVIDER_ENDPOINT`/`_TOKEN`; with neither set the daemon tick is a typed no-op. No first-party semantic model server exists in this repository to listen on that endpoint. The drain cursor defect is fixed; the sink remains a file-mediated CLI drain rather than in-process admission. | External producer/deployment. |
| CTX-024 | Same absent producer as CTX-023; the authoritative foreground signal is not proven. | External producer/deployment. |
| CTX-031 | H7/H9/H10 trusted controlled joins require live host telemetry (harness-evolution outcome, already-loaded-context identity, packet-delivery acknowledgment) that no mechanism in this repository can supply without fabricating host facts. | External host. |
