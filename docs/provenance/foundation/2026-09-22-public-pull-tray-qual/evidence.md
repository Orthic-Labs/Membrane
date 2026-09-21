# Installed public Pull, recovery & tray proof — 2026-09-22

## Boundary

Internal unsigned Windows qualification passed. Fresh Codex invoked public Pull,
received Ledger evidence while Blueprint was stale, then recovered an exact
source span through public Pull. This is installed-path proof, not RELEASED
qualification or whole-product lifecycle closure.

Machine-readable assertions & identities: [proof.json](proof.json).
Actual installed desktop capture: [tray.png](tray.png).

- Clean runtime source: `736aa5fcd517f90a545b9cd5f05228c2a5de1fea`.
- Preceding public resolver, freshness & tray repairs: `71ded6d52b29037856b06af07f00525b786eac85`.
- Version: `0.1.24`.
- Generation: `sha256:caa07153256b01fb6812d57574b03e8a03ed12e293e6042c9863be74fce663ac`.
- Installer SHA-256: `ec13f795aad4e8cdc679d6f53d4ff2312a4bde9efe7a0b28437d06e2fdba5cdc`.
- Installed engine SHA-256: `820b90774f1d855118c3779a03f464c619bed132393f0969cc4a6b1464473ff8`.
- Installer-owned stable `current` resolves to `versions\0.1.24`.

## Production repairs

- Stale Blueprint evidence remains quarantined while independently identified
  Ledger/Cortex candidates retain their own eligibility checks.
- Ledger document & skill reads previously shared mutable SQLite TEMP query
  scopes. A budget-aware read-operation lock now protects scope, erasure view &
  cancellation handler for each read. Maintenance still uses its separate writer.
- Public `pull(operation: "source_read")` projects the owner-issued resolver;
  caller, ticket, source/span hashes, revision & generation stay intact.
- Empty native Pull responses retain rejection receipts. Activity records count
  actual delivered blocks; planned-but-undelivered blocks are not admissions.
- Windows caller-root comparison accepts equivalent slash/device-prefix forms
  while retaining distinct-root rejection.
- Tray separates engine status, last Pull outcome, state age & snapshot update.
  It uses the existing transparent icon, removes the redundant status square,
  labels budget drops explicitly & describes shared engine ownership correctly.

## Installed results

`pnpm run release:local:win:unsigned` passed through native Windows desktop.
Build/install/qualification/restore elapsed 26m32s. Installer receipt covers
startup, health, MCP, host cutover, tray/popup, renderer, Hub-owned & Hub-off
Blueprint, file-change refresh, repair/upgrade, state continuity, Adapt probes,
workspace migration, doctor, uninstall & residue. Downgrade was not applicable.

| Lifecycle case | Observed result |
|---|---|
| Graceful final owner | `graceful_replace_signal` → `final_holder_release`; 319 ms drain |
| Forced owner loss | `final_holder_expired`; 20,957 ms including lease expiry |

An earlier candidate's forced-loss qualification timed out while this agent's
own tool probes renewed a harness lease. Final run used passive log monitoring
through lifecycle checks; no timeout, ownership rule or acceptance gate was weakened.

Fresh Codex task `01a0c559-d8b4-7883-b41d-a4ad0ee7b1fa`, model `gpt-5.6-luna`:

1. Public Pull returned 12 Ledger blocks with typed `blueprint_stale` degradation.
   Complete serialized MCP result measured 23,925 tokens under declared 24,000.
2. Public Pull source recovery succeeded using returned resolver arguments.
   Result measured 627 tokens under the same budget. Recovered 149 bytes from
   `docs/architecture/execution-lifecycle-boundary.md`; source & span hashes plus
   byte equality against `[13778,13927)` were independently asserted.
3. Host capacity remained explicitly unobserved; no whole-transcript-fit claim.

An earlier test agent added an unauthorized outer `tool` argument to source
recovery; runtime rejected it with `context_envelope_invalid`. The successful
run passed only the resolver's returned `arguments` plus explicit budget fields.
Membrane hooks completed; a separate Arcane Stop completion hook reported failure.
This receipt claims observed Membrane calls, not an all-plugin completion result.

Final capture shows 24 admissions, matching two successful 12-block context
requests in the canonical runtime catalog. Source recovery does not add a context
admission. Counters summarize final admission receipts; upstream provider
omissions remain separately recorded in Pull packets. Engine readback was healthy,
watcher coverage complete & catalog healthy. Daily analysis remained unavailable.
The initial screenshot helper incorrectly treated `--activate` as a popup-open
command; final capture used the existing qualification tray-click callback.

## Verification

- Focused managed Rust checks: 7/7, including concurrent read scope isolation,
  maintenance independence, qualified activation binding, Windows path identity,
  non-delivery receipts & real admission counters.
- `pnpm test`: 32 passes, 1 intentional skip; restored suite 68 passes.
- `pnpm test:mcp`: 95 passes.
- Earlier focused public resolver tests covered exact bytes, scope/hash denial,
  tiny response budgets & invalid H8; this installed run adds the actual Codex path.

## Atom disposition

| Atoms | Evidence added | Still required |
|---|---|---|
| MEM-016 | Truthful installed tray/status & identity readback | RELEASED boundary & remaining promised-host proof |
| MEM-055, MEM-056 | Shared installed engine, graceful drain & forced lease expiry | RELEASED boundary & broader host ownership matrix |
| PUL-027, PUL-031 | Useful bounded delivery during stale Blueprint; measured wire result & admission accounting | Full acceptance matrix at RELEASED boundary |
| PUL-042 | Fresh Codex discovers/calls public exact Ledger resolver | Resolver negotiation/revocation/restart negative matrix at required boundary |
| PUL-050 | Actual-host bounded-response with explicit unknown host capacity | Complete host-fit & supported-transport qualification |
| LDG-022 | Installed native Ledger provider → Pull → exact public recovery | Full installed harness-only source path & RELEASED qualification |

Scope remains required or excluded. Counts remain 299 committed, 54 excluded &
0 lifecycle-closed. These results advance required qualification; they neither
remove remaining work nor reclassify it as deferred.

## Canon receipt rows

`COMPLETE` below describes each existing implementation disposition; qualification
remains PENDING under the boundary & residual matrix above.

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| MEM-016 | DELIVERED | `engine/crates/membrane-runtime/src/pull_activity.rs:1` | `apps/membrane-tray-windows/src/main.rs` | COMPLETE |
| MEM-055 | DELIVERED | `engine/crates/membrane-runtime/src/residency.rs:25` | `scripts/qualification/install-release.ps1:1609` | COMPLETE |
| MEM-056 | DELIVERED | `engine/crates/membrane-runtime/src/residency.rs:63` | `scripts/qualification/install-release.ps1:1609` | COMPLETE |
| PUL-027 | DELIVERED | `engine/crates/cortex-core/src/planner.rs:529` | `engine/crates/membrane-runtime/src/pull/federation.rs:470` | COMPLETE |
| PUL-031 | DELIVERED | `engine/crates/membrane-runtime/src/mcp_executor.rs:239` | `engine/crates/membrane-runtime/src/pull_activity.rs:1` | COMPLETE |
| PUL-042 | DELIVERED | `engine/crates/membrane-runtime/src/mcp_executor.rs:260` | `engine/crates/membrane-mcp/src/tools.rs` | COMPLETE |
| PUL-050 | DELIVERED | `engine/crates/membrane-runtime/src/mcp_executor.rs:145` | `engine/crates/membrane-runtime/src/push/egress.rs` | COMPLETE |
| LDG-022 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/service.rs:259` | `engine/crates/membrane-runtime/src/ledger/provider.rs:93` | COMPLETE |

## Focused verification

Prior residency tests below are retained proof for unchanged ownership code;
this receipt adds current-candidate installed lifecycle evidence independently.

| Capability targets | Focused command | Direct test evidence | Result | Run identity/time |
|---|---|---|---|---|
| MEM-016 | `rightkit cargo test --manifest-path engine/Cargo.toml --locked --target x86_64-pc-windows-msvc -p membrane-runtime --lib -- ledger::service::tests authorization::tests::windows_root_matching pull_activity::tests mcp_executor::hub_transport_tests::native_non_delivery --test-threads=1` | `native_pull_activity_updates_real_admission_snapshot` | FOCUSED_PASS — 7 passed, 0 failed | RightKit `99ec795f-8c22-4bc6-a360-88cd620ff99c`, 2026-09-21 UTC; 0 failed. |
| PUL-031 | `rightkit cargo test --manifest-path engine/Cargo.toml --locked --target x86_64-pc-windows-msvc -p membrane-runtime --lib -- ledger::service::tests authorization::tests::windows_root_matching pull_activity::tests mcp_executor::hub_transport_tests::native_non_delivery --test-threads=1` | `native_non_delivery_preserves_rejection_accounting_within_wire_budget` | FOCUSED_PASS — 7 passed, 0 failed | RightKit `99ec795f-8c22-4bc6-a360-88cd620ff99c`, 2026-09-21 UTC; 0 failed. |
| LDG-022 | `rightkit cargo test --manifest-path engine/Cargo.toml --locked --target x86_64-pc-windows-msvc -p membrane-runtime --lib -- ledger::service::tests authorization::tests::windows_root_matching pull_activity::tests mcp_executor::hub_transport_tests::native_non_delivery --test-threads=1` | `concurrent_scoped_reads_serialize_temp_scope_installation` | FOCUSED_PASS — 7 passed, 0 failed | RightKit `99ec795f-8c22-4bc6-a360-88cd620ff99c`, 2026-09-21 UTC; 0 failed. |
| MEM-055 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc --locked --no-fail-fast -p membrane-runtime --test residency_holders` | `authenticated_renewal_and_expiry_preserve_final_drain_rule` and `either_holder_starts_one_controller_and_both_deduplicate`. | FOCUSED_PASS — 10 passed, 0 failed | RightKit `f43d331b-3d95-4432-a44d-c662062be50d`, 2026-09-20; 0 failed. |
| MEM-056 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc --locked --no-fail-fast -p membrane-runtime --test residency_holders` | `authoritative_final_release_reports_inactive_while_peer_release_stays_ready` and `peer_held_controller_is_never_drained_by_non_final_holder_release`. | FOCUSED_PASS — 10 passed, 0 failed | RightKit `f43d331b-3d95-4432-a44d-c662062be50d`, 2026-09-20; 0 failed. |
| PUL-027 | `rightkit cargo test --manifest-path engine/Cargo.toml --locked --target x86_64-pc-windows-msvc -p cortex-core --lib stale_blueprint` | `stale_blueprint_quarantine_preserves_independent_ledger_and_cortex`; `stale_blueprint_requirement_remains_unsatisfied_when_no_independent_evidence_exists` | FOCUSED_PASS — 2 passed, 0 failed | RightKit `ed6f19d6-2c0a-4767-933e-a5ccd73e287d`, 2026-09-21 UTC; 0 failed. |
| PUL-042 | `rightkit cargo test --manifest-path engine/Cargo.toml --locked --target x86_64-pc-windows-msvc -p membrane-runtime --lib public_pull_ -- --test-threads=1` | `public_pull_source_read_returns_exact_known_section`; `public_pull_source_read_rejects_stale_hash_and_scope_escape` | FOCUSED_PASS — 4 passed, 0 failed | RightKit `c203c853-9d11-4a09-8695-14302cac8493`, 2026-09-21 UTC; 0 failed. |
| PUL-050 | `rightkit cargo test --manifest-path engine/Cargo.toml --locked --target x86_64-pc-windows-msvc -p membrane-runtime --lib public_pull_ -- --test-threads=1` | `public_pull_source_read_refuses_budget_and_invalid_h8_coverage`; `public_pull_source_read_rejects_stale_hash_and_scope_escape` | FOCUSED_PASS — 4 passed, 0 failed | RightKit `c203c853-9d11-4a09-8695-14302cac8493`, 2026-09-21 UTC; 0 failed. |
