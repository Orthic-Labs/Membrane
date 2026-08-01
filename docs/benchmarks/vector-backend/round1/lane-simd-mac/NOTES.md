# SIMD/BLAS follow-on lane — macOS arm64, 2026-08-01

Plan: `plans/sol/2026-07-27-vector-backend-bakeoff.md` §13.2 (private workspace).
Runner: `engine/vector-bakeoff/simd/` (release build, rustc per Round 1 receipt host).
Fixtures: identical generator/config (`config/round1-v1.json`, seed 20260801) — fixture
SHA-256 per cell matches the Round 1 Mac receipt.

## Why this lane exists

Round 1's arm B ("optimized Rust") re-quantized every row per query
(`common/src/lib.rs:384` `quantized_candidates`) instead of holding quantized vectors
resident as §4 specified. Its Round 1 numbers measure a pessimized arm. Round 1 result
bytes stay frozen; this lane carries the corrected arms. The final report must state the
correction wherever Round 1 arm-B numbers appear.

## Arms (all exact, no ANN)

| Arm | Backend | Parity vs in-process arm A |
|---|---|---|
| A | control arm A, byte-identical implementation | reference |
| B2 | Accelerate `cblas_sgemv`, contiguous dequantized f32 matrix, scores all rows, filter after, bounded top-N | candidate sets identical, minOverlap 128/128 every query, every cell (gate: overlap ≥ topK−1 + target present) |
| B3 | quantized-RESIDENT (pre-quantized once at build) filtered scan, bounded top-N | exact candidate-ID equality, gate-enforced |

## Result summary (p95 ms, 100 queries/cell, 20 warmups, topK 128)

| Cell | A | B2 sgemv | B3 resident-quant |
|---|---|---|---|
| n0-s100 (2,361 rows) | 3.28 | 1.24 | 2.08 |
| n0-s10 | 0.21 | 0.21 | 0.14 |
| n12-s100 (30,549) | 18.04 | 4.49 | 14.64 |
| n12-s10 | 2.63 | 3.42 | 1.86 |
| 100k-s100 | 58.92 | 45.43 | 50.97 |
| 100k-s10 | 6.87 | 11.77 | 6.97 |

Target recall 100% in every arm/cell. Raw per-query data: `<cell>/simd.json`.

## Reading

- B2 clears the §9 material-value gate (≥25% p95 at forecast scale): −75% at N12×100%.
  No Round 1 DB arm cleared it (same cell: lance E 21.7 ms, DiskANN alpha 9.1 ms).
- B2 loses at 10% selectivity by design (computes all scores, filters after); B3 wins
  there. Adoption shape: selectivity-dispatched hybrid B2/B3 — same store, no new engine.
- macOS-only (Accelerate). Windows needs a BLAS/SIMD equivalent lane before the
  cross-host table can include these arms.
