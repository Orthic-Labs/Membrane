# Push final implementation plan

Status: implementation landed; this document records the intended final shape and remaining qualification boundary.

## Final shape

Push is the single representation-preparation boundary for already-authorized evidence. It must not become a planner, source-authority layer, second document store, or arbitrary command-execution service. Pull/Membrane owns admission and final policy; Ledger owns governed source/document semantics; Blueprint owns symbol resolution.

The implementation converges on one shared preparation owner and one scoped recovery store. Native MCP, resident HTTP, CLI, and Membrane-owned tool-result middleware delegate to those owners rather than implementing independent compression/recovery semantics.

## Required properties

1. Commit and read-back verify the exact original before any lossy representation is advertised as recoverable.
2. Bind recovery capability to repository/root/scope authority. Never accept caller assertions as proof of resolver availability.
3. Admit offload only for a consumer that has demonstrated the scoped resolver path (**PSH-025**).
4. Preserve exact bounded recovery, including whole object, byte range, line range, and unambiguous structured selectors.
5. Keep storage lifecycle explicit: bounded quotas, declared TTL/lease, no read-renewal, explicit CAS renewal, distinct invalidation and expiry (**PSH-026**).
6. Validate reduced output independently against the immutable original and protected obligations.
7. Refuse uncertain transforms rather than silently cascading into another lossy fallback.
8. Materialize packet candidates before measurement and never relabel unknown token/provider accounting as measured fact.
9. Measure the actual owned final envelope where the integration permits it; do not claim arbitrary host interception.
10. Keep command capture direct-argv and bounded by default; shell mode is explicit compatibility behavior, not a resident remote-execution API.

## Canon reconciliation

The authoritative Push canon is 26 committed atoms: PSH-001 through PSH-026. The original 24 remain; PSH-025 and PSH-026 are additive because consumer qualification and retention-lifecycle guarantees are independent acceptance boundaries.

The prior architecture reference to 29 release gates was stale and is corrected to 26.

## Validation boundary

For the current integration phase, compile-level/focused evidence is sufficient to establish that the merged Rust graph is coherent. Do not run or interpret full repository CI as a Push gate while unrelated subsystems are concurrently changing unless a later release-qualification pass explicitly asks for it.

Before claiming RELEASED for every Push atom, separately qualify installed-host behavior, cross-platform containment, long-running retention maintenance, learned/query-aware quality, provider-usage joins, and any host-specific interception claim. Compilation alone does not close those gates.
