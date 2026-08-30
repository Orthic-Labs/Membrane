# Dispatch prompt — Membrane pending implementation, code-executable gaps

Source of truth: `docs/pending/MEMBRANE-PENDING-IMPLEMENTATION.md` (§4.2, §13.1, §13.2,
§13.3, §13.4, §15, §16.2, §17.2). Canon in `docs/subsystems/` overrides it on conflict.

## Objective

Close the §13.4 gaps that are executable in code today, without host instrumentation that
does not exist (H7/H9/H10 outcome, deployment and delivery-acknowledgement observations
remain out of scope, as do all qualification-corpus freezes, which require measurement runs
rather than implementation).

## Verified starting state (read from source, not from the ledger)

The pending ledger is stale in one place: `membrane_context` already declares
`sufficiencyContract` (`engine/crates/membrane-mcp/src/tools.rs:40`) and
`engine/crates/membrane-runtime/src/pull/federation.rs:190,381` transports it verbatim.
The remaining §13.1 gap is therefore only the first-party planner *caller* that authors a
contract, not the schema or transport.

`PublicationFenceV1` is validated but caller-supplied on both the engine
(`membrane-federation/src/engine.rs:606`) and runtime
(`membrane-runtime/src/pull/federation.rs:501`) seams. No grant owner performs the
post-fusion second observation; `serve.rs:4936` sets `scope_grant_present: true` with no
fence producer. That is the real §17.2 gap.

`ReviewInputSelectionV1` (§4.2) and `TemporalValidityV1` (§16.2) have no implementation of
any kind in `engine/crates`. Those two are the only green-field items in this packet.

The §13.2 background semantic seam is **already substantially implemented** and must not be
rebuilt. `membrane-protocol/src/background_review.rs:623,633,714` defines
`BackgroundSemanticReviewRequestV1`, `BackgroundSemanticReviewResultV1` and the
`foreground_memory_state` tri-state. `membrane-runtime/src/background_review.rs` implements
`AuthenticatedLoopbackSemanticReviewProvider`, loopback-only endpoint parsing, the
scheduler, bounded framing and cursor-advance-only-after-sink-success.
`membrane-runtime/src/bin/membrane-daemon.rs:207-214` already wires
`ForegroundMemoryStateV1::{Unavailable, AvailableNoEmission, AvailableEmission}` into
`cortex_core::review`. A second implementation of that seam is exactly the "no second model
stack" violation the same section forbids.

## Binding constraints for every lane

- Edit only the exact files in your allowlist. Read anything.
- Do not run cargo, tests, builds, generators, installs, commits, pushes or merges.
  Record the checks you intend; the integration owner runs them.
- Preserve the five public V1 shapes. A new payload field increments its schema version and
  changes the sealed digest basis where one exists.
- Absent evidence is typed `unavailable` with a reason; never `0`, never blank.
- Proposals never write durable truth. Only Cortex admission does.
- No model call may decide what a model call will read (§4.2 selection is mechanical).
- Membrane never opens Blueprint SQLite directly; Blueprint never opens Cortex storage.
- Do not hand-edit generated runtime truth (`docs/product.md`, `docs/architecture.md`,
  `docs/protocol.md`, `docs/product-truth.md`).
