# Adapt/Cortex committed residual dispatch

## Objective

Close only current committed Adapt/Cortex source residuals that are not covered by `native-closure-remaining`, hook, `adapt-lead` ADP-011, or active Pull acknowledgement reservations. Inspect current implementation first; preserve working behavior; add no second Adapt database or Cortex admission authority.

## Authority

- `AGENTS.md`
- `docs/canon/adapt.md`
- `docs/canon/cortex.md`
- `docs/architecture/subsystems/adapt.md`
- `docs/architecture/membrane.md`
- `docs/pending/MEMBRANE-UNIFIED-IMPLEMENTATION-PLAN.md`, section G
- `audit/remediation/native-closure-wave1.dispatch.json`
- `audit/remediation/native-closure-remaining.dispatch.json`

## Current ownership

`task 01a07e9f-8f38-7463-b35e-f6b6013bb23e` is sole integration, check, commit, & push owner. Active/frozen owners retain `taste.rs`, `proposal.rs`, `manifest.rs`, `delivery.rs`, `taste_contracts.rs`, `adapt_service.rs`, `store.rs`, `adapt_proposal_service.rs`, `adapt_observations.rs`, `serve.rs`, `catalog.rs`, Pull files/tests, native closure, hooks, Blueprint, Hub/package, Cargo locks/manifests, qualification scripts, canon, pending index, & generated artifacts. Workers must not edit those paths.

## Worker boundary

Workers are Luna edit-only. Each inspects declared reads, edits exact allowlist only, runs no Cargo, tests, builds, generators, installs, commits, pushes, merges, or expensive checks, & returns content-addressed handoff. Integrator alone reconciles paths, runs every named check, updates canon/provenance only after proof, commits, & pushes. Integrator never repairs lane-owned files; return defects to owner.

## Required behavior

- ADP-041: converge competing writer requests through existing durable Cortex proposal/admission boundary; preserve conflicts; exact retries, response loss, restart, & crash recovery remain idempotent. Adapt owns no durable truth store.
- ADP-019/022/023/024/025: preserve recurrence lifecycle, explicit proposal kind/effect/target tuple with compatibility + legacy-derived provenance, sealed current-digest attribution, alternatives, exact outcome applicability, & exposure-aware recurrence. Wrong-owner, already-correct, redundant, stale, unsupported, & insufficient evidence remain typed non-eligible states.
- Never fabricate host facts. ADP-036/038/040/043-064/072/074-077 remain assigned to existing integration/qualification owner because required runtime files are reserved. CodeRight-owned H4/H6/H9/H10 producer gaps stay typed unavailable.
- CTX-018..022 implementation is already delivered; CTX-Q018..Q022 remain existing integration-owner released-bound qualification, not worker source scope.
- ADP-065..071 are exploratory/HOLD & excluded.

## Completion

Every worker returns `STATUS`, `SUMMARY`, `ACCEPTANCE`, `ARTIFACTS`, `CHANGES`, `COMMANDS`, `RECOVERY`, `DEVIATIONS`, `BLOCKER`, `NEXT`, `baselineRevision`, & `patchDigest`. `TRUE_BLOCKER` requires exhausted recovery, completed independent work, raw evidence, one missing input, & exact resume command.
