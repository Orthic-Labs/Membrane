# Deferred surfaces — explicit deferral record (CU-25)

Per consolidated contract D-C09 and dead-surface disposition §2, the following surfaces are **explicitly deferred**, not silently dropped. They are recorded here so a future contract can promote them without rediscovery.

## S-4: `fleet.rs` + `apps/membrane-hub/src/fleet.mjs`

Both halves' only consumer was `apps/membrane-hub`, which CU-H01 removes. `fleet.rs` stays orphaned-but-compiling in the membrane engine. Whether "fleet" (installation/replication projection) becomes an `orthic-hub` tab is a product decision for `Orthic-Labs/orthic-hub`'s own contract, not membrane's to make unilaterally post-migration. No `fleet` logic is wired in CU-20; the facade's `devices`/`alerts` sections remain `not_instrumented`. Revisit when `orthic-hub` defines its fleet tab contract.

## S-7: `mcp_http.rs`

Already honestly disclosed as intentionally out of scope: `evidence/productization/MBR-306/summary.json` states CLI wiring is intentionally out of scope. No new deferral decision is needed; this entry records the existing disclosure for completeness so the "0 DELETED" count in the consolidated disposition remains auditable. No `mcp_http.rs` wiring is added in any CU; the HTTP transport remains outside membrane's loopback scope.

## S-8: `notifications.rs` (MBR-711) + `devices`/`alerts`

`notifications.rs` would back `devices`/`alerts` hub sections, which stay `not_instrumented` per seam §6 and D-P04 — no backing concept exists in membrane's data model. Wiring the alert tracker without a product decision on what `alerts` means would reproduce the false-clean failure documented in state-of-truth §1. Deferred until open question O-4 ("`devices`/`alerts` decision") gets a product decision in `orthic-hub`. Recorded here, not dropped; CU-20's facade asserts `not_instrumented` for these sections as a regression guard.

## S-9: cross-provider fusion/reconciliation (`federation.rs::registered_providers`, `serve.rs::cross_provider_reconciled_context`)

Both functions existed only to make a grep-based dead-surface gate pass: `registered_providers()` forced a monomorphized reference to `memory_provider::produce_candidate_set` without ever being called by production code, and `cross_provider_reconciled_context()` forced references to `membrane_core::fusion::fuse`/`reconcile::reconcile`/`budget::CrossProviderBudget` the same way. Neither had a real caller anywhere in the crate. `produce_candidate_set`, `fusion::fuse`, and `reconcile::reconcile` themselves are untouched and keep their own `#[test]` coverage — only the decoy callers were deleted (EC-2026-08-11-membrane-false-clean-repair-contract.md, D-1). Wiring real multi-provider fusion/reconciliation into the resident-serve HTTP handlers requires deciding what "multi-provider" means on the current single-`MemoryStore` `/memory-candidates` hot path — there is no second provider to fuse against yet. That is a product/architecture decision for a future contract, not a mechanical repair.

## S-10: opt-in doc-candidate admission (`doc_candidate_provider.rs::maybe_admit_doc_candidates`, `::plan_with_doc_shadow`)

Both functions were unreachable in both directions: `plan_with_doc_shadow` (the only caller of `maybe_admit_doc_candidates`) itself had no production caller, and its only prior caller was a test (`engine/crates/crypt/tests/doc_candidate_provider.rs`) exercising the dead function directly rather than any real request path — that test was removed alongside the functions. The shadow-selection seam (`DocCandidateProvider::select_shadow`, `RegisteredDocCandidateProvider`, `is_doc_provider_enabled()`) remains; only the planner-admission wrapper and opt-in admission function were removed (D-2). Deciding what task classes get doc candidates admitted to the planner, and at what trust tier, is a product decision for a future contract — not something this repair invents.

## Summary

- **Wired in this contract:** S-1 (CU-11), S-2+S-6 (CU-17), S-5 (CU-18), S-3 (CU-19)
- **Deferred with reason here:** S-4, S-7, S-8, S-9, S-10
- **No silent deletions:** every S-1..S-10 has exactly one disposition row in §2 and, for deferred items, one paragraph here.
