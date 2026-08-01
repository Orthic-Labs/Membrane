# MemRight vector-backend bake-off — Round 1 cross-host report (v1)

**Date:** 2026-08-01 · **Status:** Round 1 complete on both hosts; Round 2 (tuned shortlist) pending.
**Benchmark commit:** `2f7e4c4f70d31462815155c3cd84717809db1e8a` · input digest `574943145f3502ca…` · config SHA `ad34773d19459744…` (identical on both hosts; per-(cell,runner) fixture SHA-256 verified equal across hosts, 24/24).
**Hosts:** macOS arm64 (Apple Silicon, Darwin 25.5.0, rustc 1.96.0) · Windows amd64 (Bogus-Dell, Windows 10, rustc 1.96.1).
**Raw data:** `round1/mac/`, `round1/windows/`, `round1/lane-simd-mac/` beside this file (per-query timings + candidate IDs + receipts). Harness: `engine/vector-bakeoff/` at the commit above.

## What was measured

Six cells — corpus sizes N0 = 2,361 rows (production scale), N12 = 30,549 (12-month forecast), 100K (stress), each at 100% and 10% filter selectivity — × 8 arms, 768-dim vectors, topK 128, 100 queries/cell, 20 warmups, release builds. Backends executed candidate selection only; ranking authority stayed with exact cosine in every arm. Versions: sqlite-vec 0.1.9 / 0.1.10-alpha.4, LanceDB 0.33.0.

| Arm | Backend |
|---|---|
| A | current Rust (f32 resident, full sort) — control |
| B | "optimized" Rust as-run (DEFECTIVE, see Corrections) |
| C1 | sqlite-vec 0.1.9 vec0, metadata-filtered |
| C2 | sqlite-vec 0.1.9 vec0, scope-partitioned |
| D | LanceDB 0.33.0 exact flat |
| E | LanceDB IVF_HNSW_FLAT (defaults) |
| F | LanceDB IVF_PQ + scalar indexes, prefilter (defaults) |
| G | sqlite-vec 0.1.10-alpha DiskANN — PRE-RELEASE, research-only |

ANN arms (E, F, G) ran library-default/heuristic configurations (sqrt(N) IVF partitions; HNSW m=20, efConstruction=300; nprobes=min(partitions,64), refine 4). **A calibration-tuned pass is owed before any published comparison is final (§12.5.1 of the spec); these numbers prune the field, they do not represent each engine's best.**

## Latency

### macOS arm64

| Cell | A | B | C1 | C2 | D | E | F | G |
|---|---|---|---|---|---|---|---|---|
| **p50 (ms)** | | | | | | | | |
| N0 (2,361) × 100% | 1.08 | 1.51 | 2.87 | 21.84 | 4.22 | 5.76 | 7.98 | 7.13 |
| N0 (2,361) × 10% | 0.11 | 0.16 | 1.86 | 2.44 | 3.96 | 2.51 | 4.64 | 6.69 |
| N12 (30,549) × 100% | 15.11 | 22.14 | 32.90 | 96.60 | 27.87 | 18.78 | 20.14 | 8.28 |
| N12 (30,549) × 10% | 1.55 | 2.28 | 19.70 | 10.01 | 21.81 | 8.68 | 11.49 | 7.90 |
| 100K × 100% | 50.42 | 69.17 | 109.12 | 214.31 | 97.78 | 50.01 | 361.30 | 215.60 |
| 100K × 10% | 5.96 | 8.03 | 60.89 | 20.71 | 71.78 | 16.11 | 50.81 | 28.42 |
| **p95 (ms)** | | | | | | | | |
| N0 (2,361) × 100% | 1.15 | 1.85 | 3.24 | 23.80 | 6.56 | 6.03 | 9.02 | 7.52 |
| N0 (2,361) × 10% | 0.12 | 0.20 | 2.36 | 2.93 | 4.37 | 2.94 | 5.05 | 7.09 |
| N12 (30,549) × 100% | 15.28 | 24.08 | 33.53 | 99.55 | 30.20 | 21.74 | 21.58 | 9.10 |
| N12 (30,549) × 10% | 1.96 | 2.69 | 20.45 | 10.57 | 22.85 | 9.45 | 12.09 | 8.29 |
| 100K × 100% | 53.15 | 100.55 | 129.67 | 247.37 | 276.42 | 67.88 | 490.88 | 246.80 |
| 100K × 10% | 6.86 | 8.51 | 93.26 | 21.04 | 93.47 | 19.50 | 174.84 | 57.52 |

### Windows amd64

| Cell | A | B | C1 | C2 | D | E | F | G |
|---|---|---|---|---|---|---|---|---|
| **p50 (ms)** | | | | | | | | |
| N0 (2,361) × 100% | 0.71 | 5.97 | 12.80 | 112.46 | 12.08 | 20.00 | 23.61 | 11.09 |
| N0 (2,361) × 10% | 0.07 | 0.43 | 5.77 | 7.18 | 7.68 | 5.01 | 7.72 | 10.88 |
| N12 (30,549) × 100% | 11.19 | 55.09 | 73.43 | 314.60 | 100.06 | 34.51 | 55.02 | 14.85 |
| N12 (30,549) × 10% | 1.18 | 5.59 | 76.08 | 42.71 | 72.54 | 35.25 | 27.95 | 12.90 |
| 100K × 100% | 63.98 | 356.43 | 337.78 | 544.35 | 402.44 | 64.17 | 41.41 | 23.38 |
| 100K × 10% | 6.09 | 21.74 | 273.03 | 72.13 | 227.75 | 35.29 | 39.29 | 26.31 |
| **p95 (ms)** | | | | | | | | |
| N0 (2,361) × 100% | 0.78 | 6.77 | 14.07 | 118.55 | 13.68 | 24.85 | 27.01 | 11.79 |
| N0 (2,361) × 10% | 0.09 | 0.47 | 6.36 | 7.81 | 8.39 | 5.51 | 8.36 | 11.54 |
| N12 (30,549) × 100% | 12.11 | 62.58 | 81.68 | 427.56 | 109.09 | 38.68 | 64.31 | 18.77 |
| N12 (30,549) × 10% | 1.42 | 6.00 | 90.64 | 50.55 | 78.68 | 45.93 | 33.11 | 15.22 |
| 100K × 100% | 77.04 | 1147.07 | 389.24 | 626.60 | 545.10 | 72.39 | 48.05 | 26.45 |
| 100K × 10% | 6.82 | 23.68 | 313.81 | 82.12 | 250.13 | 40.47 | 48.07 | 42.21 |

## Target recall

All arms delivered the frozen known-useful target in the top-128 in every cell, with one exception:

- Windows, N12 (30,549) × 100%, arm E: 1/100 queries missing the frozen target (query ids [78])

Same fixture bytes produced 0/100 misses on macOS — a cross-host ANN recall divergence at default settings, to be re-tested under tuned configurations in Round 2.

## Projection build time (seconds, 100%-selectivity cells)

| Cell | A | C1/C2 | D | E | F | G |
|---|---|---|---|---|---|---|
| Mac N0 (2,361) × 100% | 0.1 | 0.9 | 0.0 | 0.1 | 0.6 | 8.8 |
| Mac N12 (30,549) × 100% | 0.7 | 7.7 | 0.2 | 2.4 | 27.1 | 130.9 |
| Mac 100K × 100% | 2.2 | 35.6 | 0.7 | 9.9 | 63.2 | 2331.0 |
| Windows N0 (2,361) × 100% | 0.0 | 1.0 | 0.0 | 0.1 | 0.7 | 15.5 |
| Windows N12 (30,549) × 100% | 0.3 | 15.4 | 0.1 | 2.0 | 15.8 | 198.2 |
| Windows 100K × 100% | 1.0 | 174.0 | 0.5 | 7.5 | 73.1 | 847.9 |

Arm G's build is a DiskANN graph construction on a pre-release build (Mac 100K: 2,331 s; Windows comparable) — labeled research-only everywhere it appears.

## Corrections — arm B as-run is not the specified arm B

The Round 1 arm B implementation re-quantizes every row on every query (`engine/vector-bakeoff/common/src/lib.rs`, `quantized_candidates`) instead of holding quantized vectors resident as specified. Its numbers above therefore measure a pessimized arm and must not be read as "optimized Rust" (Windows 100K×100%: 1,147 ms p95 is this defect, not quantization cost). Round 1 result bytes are frozen; the corrected arms ran as a follow-on lane on identical fixtures (macOS, `lane-simd-mac/`):

- **B2** — Accelerate `cblas_sgemv` over a contiguous dequantized f32 matrix, bounded top-N. Candidate parity with arm A: 128/128 identical on every query in every cell.
- **B3** — quantized-resident (pre-quantized once) filtered scan, bounded top-N. Exact candidate-ID equality with arm A, gate-enforced.

| Cell | A p95 | B2 sgemv p95 | B3 resident-quant p95 |
|---|---|---|---|
| N0 (2,361) × 100% | 3.28 | 1.24 | 2.07 |
| N0 (2,361) × 10% | 0.21 | 0.21 | 0.14 |
| N12 (30,549) × 100% | 18.04 | 4.49 | 14.64 |
| N12 (30,549) × 10% | 2.63 | 3.42 | 1.86 |
| 100K × 100% | 58.92 | 45.43 | 50.97 |
| 100K × 10% | 6.87 | 11.77 | 6.97 |

B2 scores all rows then filters, so it wins at high selectivity (−75% p95 vs A at N12×100%) and loses at 10% selectivity, where B3 wins. A Windows equivalent lane (no Accelerate; BLAS/SIMD alternative) has not run yet and is required before cross-host claims about the corrected arms.

## Reading (our constraints; measurements above stand on their own)

1. At production (N0) and 12-month-forecast (N12) scale, the in-process exact scan (arm A) had the lowest p95 of any adoptable arm on both hosts. No adoptable arm met the pre-registered material-value gate (≥25% p95 improvement at N12); the corrected same-architecture arm B2 did (−75% at N12×100%, macOS).
2. A crossover exists on Windows at 100K×100%: G (26.5 ms), F (48.1 ms) and E (72.4 ms) beat A (77.0 ms). On macOS at the same cell, A still led every adoptable arm. 100K is ~1.7× the 24-month forecast (N24 ≈ 58,700).
3. Anomalies for Round 2: the Windows arm-E recall miss above; arm F's cross-host inversion at 100K×100% (Windows 48 ms vs macOS 491 ms p95, same config).

**Disposition (ours):** `KEEP CURRENT` at present scale, with the optimized same-store arms (B2/B3 hybrid) as the adoption path if forecast-scale p95 needs cutting; `DEFER WITH MEASURED TRIGGER` for external vector backends — trigger = corpus approaching 100K rows at high selectivity on x86-class memory bandwidth, or vector-stage p95 exceeding the prompt-path budget. Readers with different constraints can reach different conclusions from the same data.

## Limitations

- Production corpus is small (2,361 rows); synthetic corpora above it are seeded-generated (single shape, production-like scope distribution), not organic.
- ANN arms untuned (defaults); exact arms are the shipping implementations. Round 2 tunes on a calibration split with a locked holdout.
- Two hosts, one hardware class each; single embedding shape (768-dim); one engine's scope-filtered retrieval pattern — this is not a general-purpose ANN benchmark and not a substitute for ann-benchmarks.
- Cells not listed were not run; nothing is interpolated.
