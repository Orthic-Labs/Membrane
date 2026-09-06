# Cortex governed lifecycle focused acceptance receipt

Material runtime revision: `df4a5665d9d2361d5e85262040ba9539413c50f1`.
Freshness: `2026-09-06`.

Managed proof: GitHub Actions pull-request run `34039549949` compiled and exercised the material revision merged onto `ba7db9a86996c793a90ee60fe049d43c402ebedc`. The Rust workspace and the Cortex acceptance assertions below completed with zero test failures. The workflow later stopped at the sealed runtime-language manifest because the material runtime edit had not yet been regenerated. This receipt therefore records focused verification only; it does not claim release qualification or a fully green workflow at this revision.

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| CTX-035 | DELIVERED | `engine/crates/membrane-runtime/src/store.rs:8931-9064`; `engine/crates/membrane-runtime/src/cortex_lifecycle.rs:401-439,467-527` | `engine/crates/membrane-runtime/src/mcp_executor.rs:1256-1266` | COMPLETE |
| CTX-040 | DELIVERED | `engine/crates/membrane-runtime/src/cortex_lifecycle.rs:636-760` | `engine/crates/membrane-runtime/src/mcp_executor.rs:1483-1515`; `engine/crates/membrane-mcp/src/tools.rs:144-155` | COMPLETE |
| CTX-041 | DELIVERED | `engine/crates/membrane-runtime/src/cortex_lifecycle.rs:119-163,304-321,538-628`; `engine/crates/membrane-runtime/src/store.rs:2062,2113,8747-8763,8781-8830,9282-9302` | `engine/crates/membrane-runtime/src/mcp_executor.rs:1257-1266` | COMPLETE |

## Focused verification

| Capability targets | Focused command | Direct test evidence | Result | Run identity/time |
|---|---|---|---|---|
| CTX-035 | `cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast` | `cortex_lifecycle::tests::cortex_admission_utility_precedes_mutation_and_preserves_independent_gates` proves utility rejection occurs before canonical mutation, protected explicit-user/high-consequence evidence remains eligible, and duplicate handling remains an independent gate. | FOCUSED_PASS — 0 failures. | GitHub Actions managed CI run `34039549949`; head `df4a5665d9d2361d5e85262040ba9539413c50f1`; Rust workspace 0 fail; 2026-09-06T14:33:45Z. |
| CTX-040 | `cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast` | `cortex_lifecycle::tests::cortex_recall_recipe_is_versioned_bounded_deterministic_and_receipt_complete` proves deterministic supported execution, digest changes with configuration, bounded projection, explicit actual lanes, graph traversal disabled, typed unsupported identity, and explicit fallback. | FOCUSED_PASS — 0 failures. | GitHub Actions managed CI run `34039549949`; head `df4a5665d9d2361d5e85262040ba9539413c50f1`; Rust workspace 0 fail; 2026-09-06T14:33:45Z. |
| CTX-041 | `cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast` | `cortex_lifecycle::tests::cortex_suppression_is_reversible_persistent_version_fenced_and_not_erasure`; `cortex_lifecycle::tests::cortex_suppression_resume_rechecks_current_lifecycle_and_erase_state`; `cortex_lifecycle::tests::cortex_suppression_survives_backup_restore_without_erasing_canonical_record` prove restart/reindex/recipe/resolver exclusion, CAS reversal, lifecycle/erase recheck, retained canonical payload, and digest-bound backup/restore persistence. | FOCUSED_PASS — 0 failures. | GitHub Actions managed CI run `34039549949`; head `df4a5665d9d2361d5e85262040ba9539413c50f1`; Rust workspace 0 fail; 2026-09-06T14:33:45Z. |