# Pull — Evidence Acquisition, Selection & Admission

**Canonical name:** Pull  
**Parent system:** Membrane  
**Implementation namespace:** `engine/crates/membrane-runtime/src/pull/`

Pull answers: **which current, eligible evidence is sufficient for this task,
& how much headroom remains for it?** It owns bounded provider acquisition,
authority/freshness checks, fusion, dedupe, headroom/attention-budget
selection, packet publication, & typed receipts. Cortex/Persist remains the
durable-knowledge owner; Blueprint remains repository-truth owner.

Pull selects evidence & its available headroom. Push receives that selection
& performs faithful representation transforms; Push does not rank candidates or
set the evidence headroom.

## Public surface

- `membrane cli pull plan-context` — deterministic admission for a candidate set.
- `membrane cli pull federate` — provider fan-out, admission, & packet publication.
- `membrane cli pull memory-candidates` — Cortex candidate projection consumed by
  federation; it does not write durable knowledge.

The runtime crate exposes these paths only under `membrane_runtime::pull`.
There are no root-level planner, federation, or admission compatibility paths.

## Discoverability spine

| Evidence | Canonical location |
|---|---|
| candidate-set schema | `schemas/context-candidate-set.v1.schema.json` |
| fixtures | `tests/fixtures/pull/` |
| evals | `docs/evaluation/pull.md` |
| metrics | `docs/evidence/qualification/pull-metrics.json` |
| competitor inputs | `docs/research/competitors/pull-matrix.md` |

Each entry must record sufficiency, eligibility, headroom, omission,
degradation, & publication receipts without storing source payloads.
