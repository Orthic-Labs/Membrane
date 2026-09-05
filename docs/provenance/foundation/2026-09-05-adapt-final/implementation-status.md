# Adapt reconciled implementation — status and remaining work

Branch: `codex/adapt-main-reconciled-20260905`.
PR: `#17` (`feat(adapt): reconcile governed runtime with Pull and Ledger`).
Reconciled base: `5e95595f1ffcfc992620b63838a2d0d766a6e9ba` (current `main` containing the Ledger and Pull merges at reconciliation time).
Audit/canon source baseline: `75c257ad711d19ffce69258d132a45dbffa9b4ac`.

## Implemented in this branch

The native CLI forwards canonical Adapt operations to the active resident instead of opening a caller-selected database. Explicit offline mining and supplied-data analysis stay offline. Operator and observation HTTP routes require bearer authentication and matching installation, store, release and resident-session bindings. The optional MCP Adapt inspection tool is read-only and scope-bound; it cannot approve proposals.

Federation consumes the existing Taste selector before planning and reduction. Pull remains the final packet owner: admission, fusion, budgeting, placement, suppression and publication stay in the current Pull path. Final-packet checks bind complete preference representations and current lifecycle/version state. Packet emission, host acknowledgement, loaded identities and evaluator outcomes are separate receipts. Missing acknowledgement is not inferred as exposure or benefit. Cross-repository candidates are removed before forming inspection/selection receipts.

The reconciliation preserves the current Ledger daemon/service owner and default MCP discovery contract. It does not add a second Ledger provider or let Adapt bypass Ledger/Pull authorization, materialization or publication boundaries.

One structured required-verification detector consumes sequenced host observations, persists coverage and cursor state, and handles restart/latest-window replay without duplicating its output. A failed required verification followed by a completion claim is distinct from honestly reporting failure; a passing retry clears the failure. These APIs consume host-provided mechanical facts; they do not implement a CodeRight producer.

Candidate comparison interprets bounded, version-bound case-level development and frozen-test outcomes. It preserves the baseline on no improvement, cancellation, budget exhaustion, insufficient coverage or regression. Guard-stage evaluation distinguishes shadow/advisory/scoped-blocking evidence eligibility from host authorization. Neither API executes a model, mutates the instruction target or grants blocking permission.

The branch also contains the following governed domain mechanisms:

- **ADP-073 candidate target/version exclusion:** `ProposalPlanStore` derives a stable semantic-target identity from the sealed target surface. At most one non-expired Proposed/Approved plan may occupy a target/version slot. An exact semantic/risk retry converges on the existing plan without another store mutation; a different variant for the same target/version returns a typed conflict. Different target versions and distinct semantic targets remain independent. The existing optimistic store revision guard still handles stale concurrent writers.
- **ADP-072 clarification state machine (partial):** a bounded clarification record binds lineage, semantic target, target version and evidence digest; stale target/evidence, expiry and cancellation are terminal; one answer can produce a same-lineage resume binding. Persistence/restart and stale-writer behavior are implemented. The domain type requires an authenticated-human receipt identity/digest but deliberately does not pretend that a serialized `source=local_operator` proves human identity. A resident/host transport must still independently authenticate the human/adjudicator receipt before ADP-072 is complete.

## Reconciliation validation completed

The reconciled source commit passed the focused repository gates after the Ledger/Pull merge conflict resolution:

- `cargo test --manifest-path engine/Cargo.toml --locked -p membrane-adapt --tests` — PASS, including clarification, governed decisions, proposal target exclusion, held-out/benchmark, remediation and Taste suites.
- `cargo test --manifest-path engine/Cargo.toml --locked -p membrane-mcp --test discovery_roundtrip` — PASS, 6/6.
- `cargo test --manifest-path engine/Cargo.toml --locked -p membrane-runtime --test adapt_resident --test adapt_proposal_service` — PASS, 8/8.
- `cargo check --manifest-path engine/Cargo.toml --locked -p membrane-runtime -p membrane --all-targets` — PASS.
- `git diff --check` — PASS.
- `node scripts/ci/check-atomic-canons.mjs` — PASS after rebinding the normalized comparison receipt and regenerating the derived canon indexes.

The comparison-receipt repair and generated-index refresh are documentation-only changes on top of the source-tested tree. These checks establish repository/source consistency; they do not establish packaged host or release qualification.

## Not complete; do not claim end-to-end product qualification

The live CodeRight producer/evaluator deployment, complete human review UI, signed-key administration, every remaining efficiency detector, and full trusted evaluator-receipt resolution remain open. ADP-072 still needs a resident transport that independently authenticates the human/adjudicator receipt before accepting an answer.

ADP-038 and ADP-040 now have meaningful runtime substrate (sequenced detector coverage and evaluator joins), but their atomic requirements remain intentionally incomplete: the coverage contract still needs all declared terminal states/version/honesty semantics and the outcome contract still needs the complete exact episode/exposure/effectiveness join.

Full longitudinal recurrence aggregation, old-window replay across arbitrarily advanced cursors, comprehensive persisted multiwriter qualification, and deployed host-effect rollback require further work. Windows and macOS packaged tray/daemon/client qualification is not established by Linux/unit or copied-binary checks. `qualified` remains false and unavailable producer/outcome fields remain explicit. No historical evidence receipt is repurposed as a new pass.

## Completion order after merge

1. Bind the real CodeRight producer to observations, packet acknowledgements, loaded representations and qualified evaluator receipts.
2. Finish authenticated clarification transport and the operator review queue using existing owners.
3. Finish detector-coverage and exact outcome-join semantics, then the remaining structured efficiency detectors whose required host facts are available.
4. Run the full reviewed issue-to-case-to-experiment-to-authorized-mitigation loop, including rejected candidates, no-improvement outcomes and rollback.
5. Qualify actual packaged clients on Windows and macOS and the relevant real held-out detector cohorts before claiming production effectiveness.

The final improvement plan and accepted canon remain the design basis. Do not reopen architecture or create parallel Adapt stores/lifecycles unless a concrete acceptance failure requires it.
