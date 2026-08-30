# Membrane — pending specifications

Specifications whose production path Membrane must still execute.

Per `docs/agent-rules.md`, a capability is not landed until the production path executes it and
frozen acceptance evidence shows it meets or improves the baseline it replaces. Work stays here
until then.

Nothing here is canon. Where a pending document and a canonical doctrine disagree, the doctrine
wins and the pending document is corrected.

## Host neutrality

Required Membrane specifications define Membrane's work and Membrane's side of the seam as a
**host-neutral contract**. They do not name a host implementation or host repository path. Host requirements appear
as numbered host capabilities (`H1`–`H11`) that any harness may satisfy.

A first-party host implements those capabilities under its own specification in its own repository.
That document depends on these. These never depend on it.

CodeRight's target shape is split by ownership in
`docs/plans/pending/CODERIGHT-MEMBRANE-CONTEXT-INTEGRATION.md` and
`docs/plans/pending/CODERIGHT-EVIDENCE-PRODUCTION-FOR-MEMBRANE.md` in the CodeRight repository.

## Index

| Document | Kind | Contents |
|---|---|---|
| [MEMBRANE-PENDING-IMPLEMENTATION.md](MEMBRANE-PENDING-IMPLEMENTATION.md) | required | Production-path audit (§0) plus non-experimental target contracts: Adapt `intervention_target`, `InterventionAttributionV1` mutation-eligibility gate, asset activation evidence, asset effectiveness, daemon background review, Cortex Stage 1 Dream, Learning Lineage, insights projection, Pull fusion/corrective retrieval, Push wiring/qualification, `PacketReductionPlanV1`, and host seam `H1`–`H10`. |
| [ADAPT-HARNESS-EFFICIENCY-INSIGHTS.md](ADAPT-HARNESS-EFFICIENCY-INSIGHTS.md) | required extension | Closes dispatch-graph, per-assignment budget, progress attribution, detector coverage, duplicate execution, role leakage, context amplification, retry/tool/subagent waste & CodeRight H4 evidence gaps exposed by the 2026-08-29 dispatch incident. |
| [semantic-blueprint-review-pack-v2/README.md](semantic-blueprint-review-pack-v2/README.md) | Fable review pack | Reconciled Sol proposal for source-bound Ledger → Cortex semantic compilation, Pull fusion, Adapt pipeline-efficiency learning, optional local semantic-worker experiment & Blueprint architecture-view hardening. All eight source documents remain pending; no canon or landed-state claim. |
| [MEMBRANE-SEMANTIC-ADVISOR-EXPERIMENTAL.md](MEMBRANE-SEMANTIC-ADVISOR-EXPERIMENTAL.md) | experimental, optional | Bounded LLM semantic assistance: three phases, corrective-first default posture, recorded-nondeterminism replay, wire contracts, challenge/resume, host capability `H11`. Deletable without affecting the required document. |

## Start with the production-path audit

`MEMBRANE-PENDING-IMPLEMENTATION.md` §0 separates implementation presence, production
reachability, and production-bound qualification evidence. It never infers absence from an
identifier search or promotes doctrine prose into landed-state evidence.

Current traced findings include reachable RRF, query-aware Push and sealed-remediation paths whose
promotion/effect claims still require frozen qualification. Fixed provider/security ordering remains
the active fusion control. Background semantic inputs, host observation transport and installed
native qualification remain real wiring or execution gaps, not greenfield replacements.

§0.2 names what has **not** been audited. Absence from the table means unverified, not absent.

## Superseded

These are consumed by the two documents above and have been removed:

- `MEMBRANE-INTERVENTION-OUTPUT-AND-HARNESS-EVOLUTION.md`
- `MEMBRANE-LLM-CONTEXT-PLANE-IMPLEMENTATION.md`
- the pointer stub for the host evidence-production specification

## Canon and pending-work boundary

Canonical doctrines in `docs/subsystems/` own contracts. These pending documents own unlanded
implementation work:

- `MEMBRANE-CROSS-SUBSYSTEM-IMPROVEMENTS-AND-EVIDENCE-GATES.md` retains invariants, ownership, and
  evaluation gates; rollout work belongs here.
- `CODERIGHT-MEMBRANE-OBSERVABILITY-LEARNING-AND-EVAL-INTEGRATION.md` retains cross-product
  contracts. Host-owned implementation planning belongs in the host repository.

Both stay in `docs/subsystems/` because `docs/agent-rules.md` cites them as canonical sources.
