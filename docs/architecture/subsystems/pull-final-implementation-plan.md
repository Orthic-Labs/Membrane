# Pull final implementation plan

Status: implementation landed on `work/pull-end-to-end-20260905`; this document records the intended final shape and the remaining release-qualification boundary.

## Final shape

Pull is Membrane's single evidence-retrieval, admission, fusion, and publication subsystem. It accepts a task plus distinct request/task/session identity, repository/worktree authority, explicit token/deadline limits, anchors, sufficiency requirements, and an authentic host remaining-context observation. Providers supply evidence; they never gain policy authority. Push remains the representation-reduction owner and does not become a second planner.

The production path is native and tray-daemon bound:

`host -> membrane_context / resident federate -> request normalization -> authorized provider acquisition -> eligibility -> deterministic fusion -> bounded correction -> coverage -> packet planning -> Push selection -> final publication reconciliation -> one selected representation + receipts`

## Landed repairs

1. Preserve task text, task ID, session ID, request ID, scope, generation, deadline, anchors, and explicit token budget as distinct request fields. H8 validates against identities, not task prose or repository scope aliases.
2. Keep the native MCP schema and executor behavior aligned. `budgetTokens` is explicit; legacy `budget` remains compatibility input. Workspace scope resolves the caller plus explicitly granted child repositories from the installation registry (or a caller-selected authorized subset), applies the same read-only fan-out authorization gate per target, and assembles attributed target packets under one aggregate token ceiling.
3. Preserve the selected Push representation as the sole model-facing evidence body. Do not duplicate selected block bodies outside Push accounting; retain selection/fusion/corrective/publication/source-resolution evidence for the host.
4. Preserve Blueprint evidence-path provenance through the provider boundary instead of flattening path identity away.
5. Carry source generation into the graph/vector resolution gate so exact current evidence can survive publication and stale/mismatched evidence fails typed.
6. Register Ledger as a real Pull provider and include it in expected-lane accounting while keeping Ledger responsible for document materialization/navigation semantics.
7. Distinguish successful federation telemetry from unavailable/error states.
8. Implement bounded same-session suppression only when the consumer explicitly confirms prior delivered evidence remains available. Protected evidence is never suppressed; content change, expiry, unknown host state, or explicit refresh restores eligibility.
9. Compare against the actual prior selected packet for reusable-prefix diagnostics and keep final placement separate from membership/authority decisions.
10. Preserve semantic placement as downstream ordering only; it cannot authorize rejected evidence, change trust/data-only status, or break atomic groups.
11. Enforce consumer-aware resolver selection. The server intersects the host-declared resolver set with runtime-owned resolver surfaces; resolver-only evidence with no callable negotiated resolver is rejected before ranking with a typed omission, while evidence carrying faithful inline text falls back to rendered delivery.

## Canon mapping

The implementation strengthens existing Pull atoms rather than creating parallel planner semantics:

- request/host contract: PUL-001, PUL-004, PUL-005;
- Blueprint/Ledger/provider completeness: PUL-012, PUL-015, PUL-016;
- eligibility, coverage, correction, fusion: PUL-017 through PUL-025;
- representation and accounting: PUL-027 through PUL-033;
- source/publication reconciliation: PUL-035, PUL-036;
- session suppression: PUL-037;
- byte-stable previous-packet reuse: PUL-039;
- semantic placement: PUL-040.

Two independent boundary capabilities are now explicit canon targets:

- **PUL-041** — native workspace evidence assembly across independently authorized repository targets under one aggregate attention ceiling, preserving target identity and typed omissions;
- **PUL-042** — consumer-aware resolver selection, where resolver-only evidence is eligible only after host capability is intersected with a runtime-owned callable resolver surface.

Both remain lifecycle/qualification-open until installed-host witnesses close their acceptance boundaries. **PUL-034 remains exploratory and is not required by either capability**: ordinary Ledger and Cortex provider evidence continues to fuse without introducing semantic document-derived synthesis.

## Non-negotiable invariants

- Eligibility runs before ranking; no scalar score compensates for weak authority.
- Authority and freshness remain independent axes.
- Repository/model text cannot self-authorize.
- Corrective retrieval is bounded to one alternate lane.
- A provider hit rejected later cannot keep a delivered requirement marked satisfied.
- Final publication revalidates bound authority/source identity before bytes leave Pull.
- Empty/insufficient evidence is not fabricated into a successful answer.
- Delivery state records only evidence actually selected for emission.
- Suppression and prefix reuse never bypass current authorization or freshness.

## Verification boundary

This repository is public and declares Rust compilation as GitHub-Actions-only. The branch uses an isolated `cargo check --manifest-path engine/Cargo.toml --workspace --locked` lane so unrelated subsystem work is not treated as Pull qualification. Compile success establishes source coherence only.

Before marking every affected atom `RELEASED`, separately qualify an installed tray-owned daemon with a real CodeRight/native-MCP request: distinct identities, authentic H8, Ledger/Blueprint evidence, typed insufficiency, resolver follow-up where selected, suppression invalidation, selected-representation hash conservation, and final host attachment exactly once. Do not promote canon lifecycle states from compilation alone.
