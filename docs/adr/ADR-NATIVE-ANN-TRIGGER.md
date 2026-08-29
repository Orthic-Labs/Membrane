---
blueprint:
  document_id: adr-native-ann-trigger-v1
  type: decision
  status: accepted
  effective_from: 2026-08-23
  canonical_for: [vector-retrieval, ann-adoption, vector-index]
  scope: { deployable_units: [engine], branches: [main] }
---

# ADR: Native ANN trigger

- **Status:** Accepted — retain exact resident VectorIndex
- **Date:** 2026-08-23
- **Decision ID:** `ADR-NATIVE-ANN-TRIGGER-v1`
- **Scope:** Cortex/Membrane vector retrieval

## Decision

Membrane keeps `cortex_core::VectorIndex` as its correctness oracle and
production vector path. No approximate-nearest-neighbour (ANN) index or ANN
dependency is adopted by this decision.

The checked-in runner at `engine/benches/vector_scale.rs` measures the oracle
across record count, embedding dimension/quantization, scope selectivity,
cold/warm query state, update/delete cost, recall@k, and host SIMD dispatch. A
future candidate must report ordered-ID recall against the exact oracle using
the same corpus and scope filters.

## Evidence

The resident f32 implementation and host dispatch are covered by the current
`cortex-core` VectorIndex lifecycle, dimension, scope, parity, and dispatch
tests. Existing cross-host bakeoff evidence is retained in
`docs/benchmarks/vector-backend/2026-08-02-rust-vector-optimization-bakeoff-2-report.md`.
This ADR adds the reproducible trigger runner; its integrator result is not
present yet, so no new timing or recall claim is made here.

## Revisit trigger

The default remains **no ANN** until an integrator records full-run results in
the v1 manifest. Reconsideration requires a reproducible, checked-in result
that breaches every populated policy threshold for the same corpus, platform,
and scope filter while preserving the recorded recall floor. A prose claim,
an uncommitted local timing, or a result without hardware/OS/SIMD capture is
not a trigger.

Threshold values are deliberately `null` in `benchmarks/vector-scale.v1.json`.
Only measured integrator results may populate `p95WarmQueryNs`,
`minimumRecallAtK`, `p95UpdateNs`, `p95DeleteNs`, and `residentBytes`.

## Risks

- Exact scans may stop meeting latency or memory objectives as corpus size
  grows.
- Synthetic fixtures may not represent production embedding distributions.
- Host SIMD paths can change the crossover point between runs or platforms.
- ANN recall, update, and deletion behavior could be unsuitable even when
  query latency improves.

## Rollback

If a future ANN experiment fails recall, lifecycle, scope, or resource gates,
discard candidate output and keep `VectorIndex` as the serving path. Any ANN
adoption requires a separate implementation and acceptance decision; this ADR
does not authorize changing the current index or its dependencies.
