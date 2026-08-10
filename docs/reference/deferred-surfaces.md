# Deferred surfaces — explicit deferral record (CU-25)

Per consolidated contract D-C09 and dead-surface disposition §2, the following surfaces are **explicitly deferred**, not silently dropped. They are recorded here so a future contract can promote them without rediscovery.

## S-4: `fleet.rs` + `apps/membrane-hub/src/fleet.mjs`

Both halves' only consumer was `apps/membrane-hub`, which CU-H01 removes. `fleet.rs` stays orphaned-but-compiling in the membrane engine. Whether "fleet" (installation/replication projection) becomes an `orthic-hub` tab is a product decision for `Orthic-Labs/orthic-hub`'s own contract, not membrane's to make unilaterally post-migration. No `fleet` logic is wired in CU-20; the facade's `devices`/`alerts` sections remain `not_instrumented`. Revisit when `orthic-hub` defines its fleet tab contract.

## S-7: `mcp_http.rs`

Already honestly disclosed as intentionally out of scope: `evidence/productization/MBR-306/summary.json` states CLI wiring is intentionally out of scope. No new deferral decision is needed; this entry records the existing disclosure for completeness so the "0 DELETED" count in the consolidated disposition remains auditable. No `mcp_http.rs` wiring is added in any CU; the HTTP transport remains outside membrane's loopback scope.

## S-8: `notifications.rs` (MBR-711) + `devices`/`alerts`

`notifications.rs` would back `devices`/`alerts` hub sections, which stay `not_instrumented` per seam §6 and D-P04 — no backing concept exists in membrane's data model. Wiring the alert tracker without a product decision on what `alerts` means would reproduce the false-clean failure documented in state-of-truth §1. Deferred until open question O-4 ("`devices`/`alerts` decision") gets a product decision in `orthic-hub`. Recorded here, not dropped; CU-20's facade asserts `not_instrumented` for these sections as a regression guard.

## Summary

- **Wired in this contract:** S-1 (CU-11), S-2+S-6 (CU-17), S-5 (CU-18), S-3 (CU-19)
- **Deferred with reason here:** S-4, S-7, S-8
- **No silent deletions:** every S-1..S-8 has exactly one disposition row in §2 and, for deferred items, one paragraph here.
