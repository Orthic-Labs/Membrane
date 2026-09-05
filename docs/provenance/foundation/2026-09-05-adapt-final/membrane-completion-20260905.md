# Membrane-side Adapt completion receipt — 2026-09-05

Branch: `work/adapt-membrane-completion-20260905`

Original branch base: `f612cdee804922cf59cd5b288624674492252c0a` (PR #17 merge).
Current `main` reconciled into branch: `4a51a89383d259109d213579c9befbd77b08298f`.
Reconciliation merge: `bcb2851044594e3c0ce8236290b0a7d9e60ec61a`.

This receipt records source-level completion work only. It does not claim CodeRight deployment, packaged-client qualification, or RELEASED qualification. The branch was developed in isolation while other subsystem work landed, then explicitly reconciled with the actual current `main` through PR #19 before PR #18 was prepared for review.

## Implemented or repaired on this branch

### Taste authority

`engine/crates/membrane-adapt/src/taste.rs` now preserves the transcript parser's `UserAuthoritative` evidence class for eligible external-user preference/correction evidence. Mandatory review does not downgrade that evidence to behavioral evidence. This repairs the known ADP-005 authority-laundering defect without allowing agent/tool/repository evidence to manufacture Taste authority.

### FailureEpisode applicability

`engine/crates/membrane-adapt/src/insights/mod.rs` now carries deterministic episode applicability. A single exact host/client may bind the client dimension; mixed hosts are not generalized. Blank dimensions are refused. This closes the previously absent episode-level applicability mechanism while leaving broader recurrence/issue applicability independently governed.

### Versioned Insights detector contract

`engine/crates/membrane-adapt/src/detector_contract.rs` freezes the 32 current native Insights family IDs, detector version, input contract, evidence policy and family-specific hard-negative boundary. The production mining path in `cli_api.rs` runs through the catalog and fails closed if a detector emits an unregistered or mismatched family.

This is source-level closure for the missing versioned-family contract in ADP-017. It is not held-out production precision/recall qualification.

### Exact procedural-effectiveness version separation

`engine/crates/membrane-runtime/src/adapt_effectiveness.rs` projects H4/H6 effectiveness by exact asset-content digest, excludes ambiguous evaluator outcomes instead of assigning or duplicating them across versions, and keeps final effectiveness unavailable when the exact host-loaded representation digest is absent.

This repairs the cross-version co-aggregation defect in ADP-036. Full ADP-036 qualification still requires the final H9/H10 loaded-representation join from a real host.

### Detector coverage and execution-efficiency catalog

`engine/crates/membrane-runtime/src/adapt_efficiency.rs` defines a versioned ADP-043..ADP-064 execution-efficiency detector catalog over the existing typed H4 observation contract. Each detector reports its atom/detector identity, detector version, input schema version and digest, terminal coverage state, required host facts, missing fields, findings, qualified metrics and honesty limit.

The contract is deliberately evidence-conservative: when H4 does not carry a prerequisite fact, the detector returns `unavailable`; it never converts absent telemetry into a clean run or inferred finding. Where H4 contains sufficient exact facts, the detector may return `ran` with either findings or an explicit no-finding result.

`engine/crates/membrane-runtime/src/adapt_observations.rs` persists detector-family/version/input identity and per-detector coverage alongside the existing required-verification detector state. The coverage contract is `adapt.detector-coverage.v2`. Coverage and outcome joins keep missing exact host episode/exposure facts explicit rather than manufacturing them.

This provides the Membrane-side implementation substrate for ADP-038 and ADP-043..ADP-064. It does not make host-unobservable detectors qualified, and it does not close the missing CodeRight producer facts.

### Existing ADP-072/073 mechanisms re-confirmed

The reconciled branch already contains `clarification.rs`, which persists bounded nonmutating clarification state, binds one authenticated-human receipt identity/digest, refuses stale target/evidence, and emits only a same-lineage resume binding. The remaining ADP-072 gap is independent resident/host authentication of the claimed human/adjudicator receipt.

The reconciled branch also already contains target/version exclusion in `ProposalPlanStore`: one non-expired Proposed/Approved plan may occupy a semantic-target/version slot; exact semantic/risk replay converges without another store mutation; a different variant returns a typed target conflict; optimistic store revision checks preserve stale-writer refusal. Canon rows that describe ADP-073 as having no mechanism are therefore descriptive-status debt, not the current source state.

## Qualification evidence

Temporary branch-only workflow `adapt-completion` run `33970460410` completed successfully on Ubuntu 24.04 against commit `3c6b5405b3dc44306d8cc7c390ffefa9cb45de20`.

Passed gates:

- `cargo check --manifest-path engine/Cargo.toml -p membrane-adapt -p membrane-runtime --locked`
- `cargo test --manifest-path engine/Cargo.toml -p membrane-adapt --lib --locked`
- `cargo test --manifest-path engine/Cargo.toml -p membrane-runtime --lib --no-default-features --locked adapt -- --test-threads=1`
- repository canon-integrity scripts when present

Those checks establish focused source behavior before the final `main` reconciliation. They are supporting evidence only. The repository's normal PR CI on the final reconciled PR #18 head is the authoritative merge gate.

The temporary branch workflow was removed after qualification so the product tree does not retain a subsystem-specific CI path.

## Current-main reconciliation

PR #19 merged current `main` into this branch. The incoming delta was the Push hardening merge. No Adapt-owned source conflict occurred; current Push/Ledger/Pull ownership was preserved rather than manually reimplemented in Adapt.

The final PR must be judged against the post-reconciliation branch head and normal repository CI. If `main` advances again before merge, repeat this reconciliation rule against the new head instead of relying on this receipt.

## Deliberately unresolved

The following remain outside truthful Membrane-only closure:

- real CodeRight H4/H6/H9/H10 producers and exact execution/exposure bindings;
- real host-loaded representation acknowledgement needed for procedural effectiveness;
- exact coverage-to-execution-episode/evaluator/outcome/effectiveness closure where the current host schema lacks the bridge;
- independently authenticated human/adjudicator transport for ADP-072;
- real held-out detector precision/recall and packaged Windows/macOS release qualification;
- host authorization/execution of guard rollout stages;
- any exploratory ADP-065..ADP-071 detector work, which remains HOLD by canon.

Missing host facts remain typed `unavailable`; no source-level pass is promoted to RELEASED qualification.

## Merge/reconciliation rule

PR #18 may merge only from a branch that contains the then-current `main`, is mergeable, and passes normal repository CI. After merge, descriptive canon implementation rows must be re-derived against the actual merge revision; stale pre-merge implementation text is not evidence. Do not reuse this receipt as proof of a later revision if `main` changes again before merge.
