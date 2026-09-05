# Pull boundary completion: supplemental capability comparison

**Date:** 2026-09-05
**Compared Membrane baseline:** `b6c335c2643d165a4370f13e6369805fe3675e01`
**Baseline note:** Last shared `main` before the Pull completion change; implementation evidence below is supplied by this change.
**Authority:** user-directed implementation of PUL-041 and PUL-042, 2026-09-05.
**Scope:** the two newly explicit Pull boundary behaviors. This is a static implementation-gap comparison, not release qualification.

Existing Pull atom comparisons remain pinned to their historical receipts.

| Atom | Scope | Competitive disposition | Best mechanism | Current evidence | Donor evidence | Gap / action |
|---|---|---|---|---|---|---|
| PUL-041 | COMMITTED | CURRENT_INCOMPLETE | Native registry-derived multi-target Pull fan-out under one deterministic aggregate token ceiling | `engine/crates/membrane-runtime/src/authorization.rs` derives only the caller and independently authorized child repositories; `engine/crates/membrane-runtime/src/mcp_executor.rs` allocates the aggregate budget, invokes native Pull per target, preserves repository/scope identity, target omissions, and final host-capacity fitting. | No donor superiority established; this composes Membrane's existing authorization, Pull, and Push owners. | Source implementation exists but installed-host and requirement-coverage qualification remain open; keep PARTIAL until the real workspace host path is witnessed. |
| PUL-042 | COMMITTED | CURRENT_INCOMPLETE | Server-intersected consumer resolver capability before planner admission | `engine/crates/membrane-runtime/src/pull/federation.rs` intersects caller capability with runtime-owned resolver surfaces; `engine/crates/cortex-core/src/planner.rs` rejects resolver-only evidence before ranking when no callable resolver is negotiated and retains inline faithful fallback when present. | No donor establishes Membrane's owner authorization and resolver revalidation guarantees. | Generic owner coverage is still narrower than the eventual resolver universe; keep PARTIAL while qualifying `membrane_source_read` and future owner resolvers end to end. |

PUL-034 is not required for either behavior. Ledger and Cortex remain independent provider evidence lanes; semantic document-derived synthesis stays exploratory.
