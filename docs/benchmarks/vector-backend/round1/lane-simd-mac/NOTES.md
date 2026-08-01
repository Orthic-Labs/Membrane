# Rust vector optimization matrix — macOS arm64, 2026-08-01

Host: Apple M1, Darwin 25.5.0, rustc 1.96.0. Runner:
`engine/vector-bakeoff/simd/`, release + locked dependencies. Crossover config:
`engine/vector-bakeoff/config/crossover-v1.json` (`d234bb6b…`), seed 20260801,
768 dimensions, 100 queries, 20 warmups, topK 128. Original
`round1-v1.json` stayed read-only at SHA-256 `ad34773d…`.

## Arms + parity

| Arm | Implementation | Gate |
|---|---|---|
| A | resident f32 scalar control | reference |
| B2 | Accelerate full-matrix `cblas_sgemv`, filter after scoring | target present + overlap ≥127/128 |
| B2-gather | filter first, gather eligible f32 rows, subset `cblas_sgemv` | target present + overlap ≥127/128 |
| parallel-B2 | B2 scoring + Rayon filter/top-N | target present + overlap ≥127/128 |
| B3 | resident per-vector i8 quantization | candidate IDs exactly equal to A |
| B3-SIMD | runtime-detected AArch64 NEON `sdot`, scalar tail/fallback, bounded exact residual refinement | integer accumulator unit parity + candidate IDs exactly equal to B3 |
| parallel-B3 | resident B3 scan + Rayon | candidate IDs exactly equal to A |

All 13 bundles passed: seven arms, 100 queries/arm/cell, 9,100 measurements,
100% target recall. B3/B3-SIMD/parallel-B3 candidate IDs exactly equal A.
B2/B2-gather/parallel-B2 minimum + mean overlap were 128/128 in every cell.

## Crossover results — p95 ms

| Rows | Selectivity | A | B2 | B2-gather | parallel-B2 | B3 | B3-SIMD | parallel-B3 | Winner |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 30,549 | 100% | 16.726 | 4.770 | 7.339 | 5.480 | 13.291 | 25.847 | **3.965** | parallel-B3 |
| 30,549 | 50% | 8.473 | 4.128 | 5.517 | 4.222 | 7.092 | 13.227 | **2.521** | parallel-B3 |
| 30,549 | 25% | 4.531 | 3.803 | 2.770 | 4.186 | 3.825 | 7.450 | **1.205** | parallel-B3 |
| 30,549 | 10% | 2.035 | 3.652 | 0.984 | 3.781 | 1.626 | 3.149 | **0.565** | parallel-B3 |
| 30,549 | 5% | 1.061 | 3.954 | 0.526 | 3.793 | 0.993 | 1.778 | **0.419** | parallel-B3 |
| 30,549 | 1% | 0.253 | 3.682 | **0.149** | 3.915 | 0.263 | 0.629 | 0.246 | B2-gather |
| 100,000 | 100% | 102.605 | **15.347** | 23.693 | 25.219 | 44.555 | 84.744 | 17.801 | B2 |
| 100,000 | 50% | 29.324 | 15.103 | 15.071 | 14.854 | 31.436 | 50.520 | **7.661** | parallel-B3 |
| 100,000 | 25% | 17.469 | 10.846 | 7.943 | 11.845 | 16.768 | 29.194 | **4.755** | parallel-B3 |
| 100,000 | 10% | 5.840 | 16.911 | 4.981 | 11.981 | 5.970 | 10.582 | **2.616** | parallel-B3 |
| 100,000 | 5% | 3.222 | 10.498 | 2.479 | 14.604 | 4.196 | 10.683 | **1.231** | parallel-B3 |
| 100,000 | 1% | 0.931 | 13.139 | 0.748 | 14.146 | 0.618 | 1.367 | **0.428** | parallel-B3 |

N0 full-selectivity pool-overhead spot (2,361 rows): A 1.372, B2 0.211,
B2-gather 0.504, parallel-B2 0.372, B3 1.113, B3-SIMD 2.268,
parallel-B3 0.471 ms p95. Parallel pool overhead versus B2 was +0.161 ms;
parallel-B3 improved B3 by 0.642 ms. No >1 ms pool regression occurred.

## Dispatch recommendation

Mac: use parallel-B3 by default. Use B2 at 100K only when estimated
selectivity is about 90% or higher (measured crossover bracket: 50–100%; linear
p95-delta estimate: 87.6%). Use B2-gather only for the measured N12/1% corner.
At current N0/full-selectivity, use B2.

Windows: existing AVX2 lane endpoints support B2 at ≥55% estimated selectivity
and B3 below 55% for N12/100K (linear endpoint estimates: 48.9% at N12,
54.4% at 100K). At N0 use B2 for high selectivity + A for low selectivity.
This is the conservative per-host rule until a Windows six-point sweep replaces
endpoint interpolation.

## Disposition

- parallel-B3 is adopted matrix winner in 10/12 crossover cells.
- B2 wins 100K/100% + N0/100%; B2-gather wins N12/1%.
- parallel-B2 never wins; retain as measured rejected rung.
- B3-SIMD preserves exact integer/scalar parity but is slower than scalar B3 in
  every cell; retain as measured rejected rung. M1 selected runtime NEON `sdot`;
  `i8mm` was detected as unavailable.

## NOT-MEASURED

- **sgemm batching:** online fixture is one query at a time; batching would change
  latency semantics + no concurrent-query batch fixture exists.
- **GPU/ANE:** transfer + launch costs and embedding hardware are separate lanes;
  adding them changes candidate-search scope.
- **i8-GEMM dependency:** no new dependency was justified after native `sdot`
  lost to scalar B3; single-query row-dot layout does not map directly to i8 GEMM.

Raw per-query data: `crossover-*/simd.json`. Runner output:
`crossover-harness.log`.
