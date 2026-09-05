# Adapt — Membrane-only Codex handoff

Date: 2026-09-05
Repository: `Orthic-Labs/Membrane`
Merged source branch: `work/adapt-membrane-completion-20260905` (deleted after merge)
PR: `#18` — merged
Merge revision on `main`: `6be4981dd8964fd5d3372a5de6b9b41bea496902`
Pre-merge reconciliation: `bcb2851044594e3c0ce8236290b0a7d9e60ec61a`
Original Adapt completion base: `f612cdee804922cf59cd5b288624674492252c0a` (PR #17 merge)

## Scope and instruction

This handoff is **Membrane-only**.

Do not edit `bogusyogi/coderight`, do not add CodeRight-specific transport code, and do not widen Membrane host contracts merely to make an Adapt atom appear complete. Any work whose missing fact is owned by CodeRight, a real host, an installed client, or empirical production qualification stays pending and must be recorded as such.

The immediate purpose of PR #18 is to land the remaining Membrane-owned Adapt source work that can be implemented truthfully with the contracts Membrane already owns.

Do not reopen Adapt architecture unless a concrete acceptance failure proves the current canon internally inconsistent. The governing semantic authority remains:

- `docs/architecture/subsystems/adapt.md`
- `docs/canon/adapt.md`
- `docs/agent-rules.md`

The source-level completion receipt for this branch is:

- `docs/provenance/foundation/2026-09-05-adapt-final/membrane-completion-20260905.md`

## Current repository state

PR #18 is merged on `main` at `6be4981dd8964fd5d3372a5de6b9b41bea496902`. The source branch was deleted after merge. The post-merge finalization reconciles `docs/canon/adapt.md` to that merged source, regenerates derived canon truth, and removes the superseded Adapt working branches.

The Membrane-owned source mechanisms in this handoff are therefore landed. Remaining items described below are cross-owner/host-observability or empirical/release qualification boundaries; do not recreate the completion branch to address them.

## What PR #17 already established

PR #17 established the architecture-critical Adapt integration slice:

- resident daemon ownership rather than caller-selected local database ownership;
- Taste delivery through the existing Pull packet owner instead of a parallel context allocator;
- structured host observation ingress and persisted cursor/coverage state;
- read-only MCP inspection;
- daemon-backed Adapt status projection;
- bounded candidate comparison;
- guard-rollout eligibility separated from host authorization;
- ADP-072 clarification domain state machine;
- ADP-073 proposal target/version exclusion;
- reconciliation with Ledger and Pull ownership.

That merge was important but was not the complete committed Adapt implementation.

## What this completion branch adds

### 1. Taste evidence authority repair — ADP-005 seam

`engine/crates/membrane-adapt/src/taste.rs`

Eligible external-user preference/correction evidence now preserves the transcript parser's `UserAuthoritative` evidence class. Mandatory review does not downgrade user-authoritative evidence to behavioral evidence.

This is intentionally narrow. Agent, tool, repository, diagnostic, or contextual evidence still cannot manufacture Taste authority.

### 2. FailureEpisode applicability — ADP-018 seam

`engine/crates/membrane-adapt/src/insights/mod.rs`

`FailureEpisodeV1` now carries deterministic applicability known at episode emission time. A single exact host/client can bind the client dimension. Mixed-host evidence is not silently generalized. Blank applicability dimensions are rejected.

Broader issue applicability remains independently governed by recurrence/issue formation.

### 3. Versioned known-family detector contract — ADP-017 seam

`engine/crates/membrane-adapt/src/detector_contract.rs`
`engine/crates/membrane-adapt/src/cli_api.rs`

The 32 current native Insights detector families now have a frozen V1 catalog containing:

- family identity;
- detector version;
- input contract;
- evidence policy;
- family-specific hard-negative boundary.

The production native mining path executes through this catalog. A detector emitting an unregistered or mismatched family fails closed rather than silently changing historical semantics.

This is source-level contract closure. It is **not** real held-out precision/recall qualification.

### 4. Exact procedural-effectiveness version separation — ADP-036 seam

`engine/crates/membrane-runtime/src/adapt_effectiveness.rs`
`engine/crates/membrane-runtime/src/lib.rs`

Effectiveness projection now separates evidence by exact procedural asset content digest. H6 outcomes that ambiguously join to multiple content versions are excluded rather than duplicated or assigned by traversal order.

Final effectiveness remains unavailable when the exact host-loaded representation digest is absent. Do not weaken this rule.

### 5. Detector coverage and efficiency detector family — ADP-038 and ADP-043..064

`engine/crates/membrane-runtime/src/adapt_efficiency.rs`
`engine/crates/membrane-runtime/src/adapt_observations.rs`
`engine/crates/membrane-runtime/src/lib.rs`

The branch adds a versioned execution-efficiency detector catalog covering all committed ADP-043 through ADP-064 detector/reporting identities.

Every detector result binds:

- atom ID;
- detector ID and detector version;
- input schema/version and observation digest;
- required host facts;
- terminal coverage state;
- missing fields;
- findings;
- qualified metrics where available;
- honesty limit.

The important invariant is **typed unavailability**. If the existing H4 contract cannot express a required fact, the detector returns `unavailable`. Missing telemetry must never be converted into `ran` with no finding, inferred evidence, or a fabricated metric.

Where H4 already provides the exact prerequisite facts, the detector can run deterministically and may return either findings or an explicit no-finding result.

`adapt.detector-coverage.v2` persists detector-family/version/input identity and per-detector coverage along the resident observation path.

Do not widen H4 solely to turn `unavailable` into `ran`. A host-contract extension belongs with the owner of the mechanically knowable fact.

### 6. ADP-072 and ADP-073 implementation truth

`engine/crates/membrane-adapt/src/clarification.rs`
`engine/crates/membrane-adapt/src/proposal_state.rs`

The old canon prose saying these have no production mechanism is stale.

ADP-072 already has a persisted, bounded, nonmutating clarification state machine. It binds lineage, semantic target, target version, evidence digest, one human-answer receipt identity/digest, stale/expiry/cancel terminal states, and a same-lineage resume binding.

The remaining gap is **independent authentication of the claimed human/adjudicator receipt at the real operator/host transport**. The pure domain type cannot prove that a serialized `source=local_operator` came from a human. Do not fake this by trusting a caller-supplied enum.

ADP-073 already has semantic-target plus target-version exclusion in `ProposalPlanStore`. One live Proposed/Approved slot exists per target/version. Exact semantic/risk replay converges on the existing plan without another mutation. A competing variant returns a typed target conflict. Persisted revision checks reject stale writers.

## Important remaining Membrane semantics

The following should remain partial unless a concrete existing owner can close them without inventing a parallel subsystem:

### ADP-041 persisted multiwriter convergence

`engine/crates/membrane-adapt/src/multiwriter.rs` already provides deterministic order-independent merge semantics and preserves equal-precedence conflicts.

The remaining atom is not another merge algorithm. The canon requires the behavior to be proven through the **persisted production writer path** under competing daemon requests, retry/crash conditions, and retained conflicts.

Do not create an Adapt database or second durable writer to close this. Cortex/admission remains the persistence owner. If the current production path already exposes the necessary seam after other subsystem merges, wire and test it there. Otherwise leave ADP-041 partial with the exact residual.

### ADP-072 authenticated human transport

Only close if the existing Membrane operator/auth surface can independently authenticate the human/adjudicator receipt before `ClarificationStore::answer` is accepted. Do not build a new identity system for Adapt.

### ADP-038/040 exact outcome chain

Coverage is now materially stronger, but full closure requires an exact join from detector coverage through the real execution episode/exposure to evaluator identity/outcome/effectiveness. Where H4/H6/H9/H10 facts are absent, keep the join unavailable.

### ADP-036 final loaded representation

Content-version separation is implemented. A measured-effectiveness claim still requires the exact representation the host actually loaded. Membrane must not infer this from what it emitted.

## Explicitly out of scope for this Membrane handoff

Leave these for later integration/qualification. Do not implement them in this PR:

- CodeRight observation producer changes;
- CodeRight acknowledgement/loaded-representation producer changes;
- CodeRight evaluator execution or result storage changes;
- CodeRight experiment execution;
- CodeRight consumption of learned behavior;
- any CodeRight repository edits;
- installed Windows/macOS product qualification;
- real held-out production detector cohorts;
- production causal/effectiveness claims;
- host authorization and execution of blocking guard stages;
- exploratory ADP-065..ADP-071, which remain canonically HOLD.

## Canon reconciliation rule

`docs/canon/adapt.md` has descriptive implementation rows that predate the final source changes. In particular, ADP-072/073 are known stale, and the efficiency family still reflects pre-branch `MISSING` state.

Do not mechanically mark every new detector `DELIVERED` just because a detector identity exists. Re-derive each implementation state from the final merged source:

- `DELIVERED` only where Membrane owns all implementation semantics and the production path reaches them;
- `PARTIAL` where the Membrane mechanism exists but a required host-owned fact/transport/production join is absent;
- `MISSING` only where no mechanism exists;
- qualification remains independent from implementation.

Do not convert real-host qualification into implementation status.

After PR #18 is merged, refresh descriptive implementation/register rows against the actual merge SHA and run the repository's canon generator/integrity checks. Evidence must reference the final merged revision, not the pre-merge branch receipt.

## Post-merge continuation checklist

1. Treat `6be4981dd8964fd5d3372a5de6b9b41bea496902` and later `main` as the Adapt source baseline; do not recreate PR #18 or its source branch.
2. Preserve Cortex durable-admission ownership and Pull final packet ownership.
3. Keep host-unobservable efficiency facts typed unavailable until the owning host contract supplies them.
4. Keep ADP-041 partial until persisted production multiwriter convergence is proven through the Cortex-owned writer path.
5. Keep ADP-072 partial until the real operator/adjudicator transport independently authenticates the human receipt.
6. Keep exact outcome/effectiveness claims partial until the execution-episode and loaded-representation joins exist.
7. Keep release/installed-platform and held-out detector qualification independent from source implementation state.

## Post-merge completion criterion for the Membrane side

For purposes of subsystem implementation, Membrane Adapt should be considered source-complete when:

- all Membrane-owned committed mechanisms reachable with current Membrane contracts are implemented;
- host-unobservable efficiency facts return typed unavailable coverage;
- all remaining residuals are explicitly cross-owner, installed-platform, or empirical qualification work;
- the atomic canon accurately distinguishes delivered/partial/missing implementation from pending qualification;
- full repository CI is green on the final merged source.

Do **not** call Adapt release-qualified merely because this criterion is met.

## Do-not-regress invariants

- Adapt is governed behavioral learning, not generic memory.
- Taste authority comes only from permitted user-backed evidence.
- Insights are diagnostic and cannot manufacture user preference authority.
- Cortex remains the one durable admission/storage owner.
- Pull remains the final packet allocation/publication owner.
- Adapt does not create a second experiment engine.
- Missing host evidence remains missing/typed unavailable.
- Selection, emission, loading, evaluation, and effectiveness are separate facts.
- A model proposal cannot self-approve or self-authenticate a human reviewer.
- Guard eligibility is not host blocking authority.
- No private reasoning collection is introduced.

## Files added/changed by this completion phase

Primary new/modified implementation surfaces relative to the PR #17 base:

- `engine/crates/membrane-adapt/src/taste.rs`
- `engine/crates/membrane-adapt/src/insights/mod.rs`
- `engine/crates/membrane-adapt/src/detector_contract.rs`
- `engine/crates/membrane-adapt/src/cli_api.rs`
- `engine/crates/membrane-adapt/src/lib.rs`
- `engine/crates/membrane-runtime/src/adapt_effectiveness.rs`
- `engine/crates/membrane-runtime/src/adapt_efficiency.rs`
- `engine/crates/membrane-runtime/src/adapt_observations.rs`
- `engine/crates/membrane-runtime/src/lib.rs`
- `docs/provenance/foundation/2026-09-05-adapt-final/membrane-completion-20260905.md`
- this handoff document.

Current-main Push hardening files are present through the explicit reconciliation merge and are not Adapt-owned changes.

## Final instruction to Codex

PR #18 is already merged. Do not reopen or redesign the Membrane Adapt completion slice. Continue only the explicitly residual cross-owner/host/qualification work from current `main`, preserving typed unavailability and existing subsystem ownership boundaries.
