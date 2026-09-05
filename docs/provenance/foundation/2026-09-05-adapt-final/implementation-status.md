# Adapt implementation branch — status and remaining work

Branch: `codex/adapt-governed-runtime-integrated-20260905`.
Base: `b6c335c2643d165a4370f13e6369805fe3675e01` (exact current-main integration baseline before the pre-compilation closure pass).
Audit/canon source baseline: `75c257ad711d19ffce69258d132a45dbffa9b4ac`.

## Implemented in this branch

The native CLI forwards canonical Adapt operations to the active resident instead of opening a caller-selected database. Explicit offline mining and supplied-data analysis stay offline. Operator and observation HTTP routes require bearer authentication and matching installation, store, release and resident-session bindings. The optional MCP Adapt inspection tool is read-only and scope-bound; it cannot approve proposals.

Federation consumes the existing Taste selector before planning and reduction. Final-packet checks bind complete preference representations and current lifecycle/version state. Packet emission, host acknowledgement, loaded identities and evaluator outcomes are separate receipts. Missing acknowledgement is not inferred as exposure or benefit. Cross-repository candidates are removed before forming inspection/selection receipts.

One structured required-verification detector consumes sequenced host observations, persists coverage and cursor state, and handles restart/latest-window replay without duplicating its output. A failed required verification followed by a completion claim is distinct from honestly reporting failure; a passing retry clears the failure. These APIs consume host-provided mechanical facts; they do not implement a CodeRight producer.

Candidate comparison interprets bounded, version-bound case-level development and frozen-test outcomes. It preserves the baseline on no improvement, cancellation, budget exhaustion, insufficient coverage or regression. Guard-stage evaluation distinguishes shadow/advisory/scoped-blocking evidence eligibility from host authorization. Neither API executes a model, mutates the instruction target or grants blocking permission.

The pre-compilation closure pass adds two more domain mechanisms:

- **ADP-073 candidate target/version exclusion:** `ProposalPlanStore` derives a stable semantic-target identity from the sealed target surface. At most one non-expired Proposed/Approved plan may occupy a target/version slot. An exact semantic/risk retry converges on the existing plan without another store mutation; a different variant for the same target/version returns a typed conflict. Different target versions and distinct semantic targets remain independent. The existing optimistic store revision guard still handles stale concurrent writers.
- **ADP-072 clarification state machine (partial):** a bounded clarification record binds lineage, semantic target, target version and evidence digest; stale target/evidence, expiry and cancellation are terminal; one answer can produce a same-lineage resume binding. The pure domain type requires an authenticated-human receipt identity/digest but deliberately does not pretend that a serialized `source=local_operator` proves human identity. Runtime transport verification/persistence remains required before this atom can be called complete.

These two additions have source tests staged with them. Their canon verification/release status must not be upgraded until compilation and the relevant tests have passed on the final revision.

## Not complete; do not claim end-to-end product qualification

The live CodeRight producer/evaluator deployment, complete human review UI, signed-key administration, every remaining efficiency detector, and full trusted evaluator-receipt resolution remain open. ADP-072 still needs a resident transport that independently authenticates the human/adjudicator receipt before accepting an answer. Full longitudinal recurrence aggregation, old-window replay across arbitrarily advanced cursors, comprehensive persisted multiwriter tests, and deployed host-effect rollback require further qualification.

ADP-038 and ADP-040 now have meaningful runtime substrate (sequenced detector coverage and evaluator joins), but their atomic requirements are intentionally not relabeled complete in this pre-compilation pass: the coverage contract still needs all declared terminal states/version/honesty semantics and the outcome contract still needs the complete exact episode/exposure/effectiveness join.

Windows and macOS packaged tray/daemon/client qualification is not established by Linux unit or copied-binary tests. `qualified` remains false and unavailable producer/outcome fields remain explicit. No historical evidence receipt is repurposed as a new pass.

## Validation policy

The previous exact-main v5 integration passed atomic canon, Adapt/MCP domain tests, resident integration, copied CLI tests and all-target compilation for commit `9ca43b76a5417aa113e3ace740e410be928549c0`. The pre-compilation closure changes described above are newer source and therefore require a fresh validation receipt before their implementation status can be promoted.

A checked-in test is not proof that a later revision passed. No release deployment or main-branch merge is performed by this status document.

## Completion order

1. Compile/test the staged ADP-072/073 closure changes and repair any defects before changing canon status.
2. Bind a real host producer to observations, packet acknowledgements, loaded representations and qualified evaluator receipts.
3. Finish authenticated clarification transport and the operator review queue using existing owners.
4. Finish detector-coverage and exact outcome-join semantics, then the remaining structured efficiency detectors whose required host facts are available.
5. Run the full reviewed issue-to-case-to-experiment-to-authorized-mitigation loop, including rejected candidates and rollback.
6. Qualify actual packaged clients on Windows and macOS and the relevant real held-out detector cohorts before claiming production effectiveness.

The earlier final improvement plan and full accepted canon remain the design basis. This is a source implementation status, not a completion or production-quality receipt.
