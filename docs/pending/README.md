# Membrane — pending specifications

Specifications whose production path Membrane must still execute.

Per `docs/agent-rules.md`, a capability is not landed until the production path executes it and
frozen acceptance evidence shows it meets or improves the baseline it replaces. Work stays here
until then.

Nothing here is canon. Where a pending document and a canonical doctrine disagree, the doctrine
wins and the pending document is corrected.

## Host neutrality

Both documents specify Membrane's work and Membrane's side of the seam as a **host-neutral
contract**. Neither names a host implementation or a host repository path. Host requirements appear
as numbered host capabilities (`H1`–`H11`) that any harness may satisfy.

A first-party host implements those capabilities under its own specification in its own repository.
That document depends on these. These never depend on it.

CodeRight's target shape is split by ownership in
`docs/plans/pending/CODERIGHT-MEMBRANE-CONTEXT-INTEGRATION.md` and
`docs/plans/pending/CODERIGHT-EVIDENCE-PRODUCTION-FOR-MEMBRANE.md` in the CodeRight repository.

## Index

| Document | Kind | Contents |
|---|---|---|
| [MEMBRANE-PENDING-IMPLEMENTATION.md](MEMBRANE-PENDING-IMPLEMENTATION.md) | required | Production-path audit (§0) plus non-experimental target contracts: Adapt `intervention_target` and asset effectiveness, daemon background review, Cortex Stage 1 Dream, Learning Lineage, insights projection, Pull fusion/corrective retrieval, Push wiring/qualification, `PacketReductionPlanV1`, and host seam `H1`–`H10`. |
| [MEMBRANE-SEMANTIC-ADVISOR-EXPERIMENTAL.md](MEMBRANE-SEMANTIC-ADVISOR-EXPERIMENTAL.md) | experimental, optional | Bounded LLM semantic assistance: three phases, corrective-first default posture, recorded-nondeterminism replay, wire contracts, challenge/resume, host capability `H11`. Deletable without affecting the required document. |

## Start with the production-path audit

`MEMBRANE-PENDING-IMPLEMENTATION.md` §0 separates implementation presence, production
reachability, and production-bound qualification evidence. It never infers absence from an
identifier search or promotes doctrine prose into landed-state evidence.

Current traced findings include an unwired RRF implementation, unwired query-aware Push, and an
unwired sealed-remediation path. Active provider merge uses fixed provider/security ordering, not
the standalone RRF implementation. These are wiring and qualification problems, not greenfield
implementations.

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
