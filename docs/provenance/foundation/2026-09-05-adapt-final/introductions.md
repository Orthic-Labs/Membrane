# Adapt final consolidation — capability introduction register

**Date:** 2026-09-05
**Adoption status:** reconciled repository implementation; runtime and release qualification remain independent.

Append the following rows to `## New capability register` in `docs/provenance/migrations/2026-08-30-atomic-canons/preservation-map.md`. Do not add them to the historical 728-row preservation union or the frozen split register. The bundle's integration helper performs this exact additive merge after validating current source identities.

| Introduced ID | Origin | Observable behavior | Authority/evidence |
|---|---|---|---|
| ADP-074 | 2026-09-05 Adapt final audit consolidation | Expose negotiated, scope-bound, read-only agent inspection of Adapt preferences, applicability decisions, Insights, and proposal state without approval or exposure side effects. | User-requested final plan and revised canon with atoms, 2026-09-05; docs/architecture/subsystems/adapt.md §12.5 |
| ADP-075 | 2026-09-05 Adapt final audit consolidation | Report daemon-backed Adapt pipeline readiness and progress from actual producer/consumer bindings, distinguishing empty workload, unavailable evidence, blocked work, and missing outcome joins. | User-requested final plan and revised canon with atoms, 2026-09-05; docs/architecture/subsystems/adapt.md §12.6 |
| ADP-076 | 2026-09-05 Adapt final audit consolidation | Issue a bounded, version-bound behavioral candidate-comparison decision from host-run baseline/variant outcomes, allowing no improvement without granting review, admission, or activation authority. | User-requested final plan and revised canon with atoms, 2026-09-05; docs/architecture/subsystems/adapt.md §11.5 |
| ADP-077 | 2026-09-05 Adapt final audit consolidation | Determine evidence-bound eligibility for each learned-guard rollout-stage transition without granting the host's separate blocking or scope-expansion authority. | User-requested final plan and revised canon with atoms, 2026-09-05; docs/architecture/subsystems/adapt.md §6.10 |

## Non-duplication and delivery status

Only these four IDs are added. The existing 71 capabilities, including ADP-072 and ADP-073 and the seven exploratory rows, remain present. Existing qualification boundaries are made explicit; the capability/implementation evidence states are preserved. All new rows are MISSING/PENDING/PENDING/LOCAL. COMMITTED denotes an adopted product commitment, not code delivered; the prepared ledger records the proposed commitments for adoption.

Do not reuse ADP-037 or ADP-039: they are preserved aliases. Recheck ID allocation and canonical-file identities before merging into another branch.
