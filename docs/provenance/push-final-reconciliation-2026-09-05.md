# Push final canon reconciliation — 2026-09-05

## Purpose

Record the post-implementation reconciliation between the Push architecture, the pre-existing 24-atom canon, and the user-approved final Push design.

## Result

The final Push capability boundary is **26 committed atoms**. The prior 24 atoms remain valid. Two capabilities are additive rather than replacements:

- **PSH-025 — consumer-qualified offload admission.** Push may choose an offloaded/reduced representation only when the actual consumer path has demonstrated that it can resolve the retained original under the same authority/scope contract. A merely stored original, a caller boolean, or a nominal resolver feature is insufficient proof.
- **PSH-026 — declared-lease retention under bounded storage.** Retained originals have explicit bounded lease semantics. Reads do not renew leases; renewal is explicit and compare-and-swap guarded; storage quotas remain authoritative and expiry/invalidation are distinct terminal states.

These capabilities are not duplicates of PSH-002/005/011/012/023/024. Those atoms cover restoration, raw-first externalization, capacity selection/refusal, expiry, and exact selectors. PSH-025 governs whether a consumer is qualified to receive a recoverable offload at all. PSH-026 governs lifecycle retention guarantees after publication.

## Documentation correction

`docs/architecture/subsystems/push.md` previously said "29 release gates". That count was stale. It is corrected to **26** and the architecture now names PSH-025 and PSH-026 explicitly.

## Verification scope

This reconciliation is documentation/governance work. It does not convert compile success into host qualification and does not claim full CI. The implementation remains subject to the release/qualification states recorded by the canon and focused evidence.
