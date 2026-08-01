# Rust vector optimization lane — Windows x86-64, 2026-08-02

Plan: `plans/sol/2026-08-01-rust-vector-optimization-lane.md`.
Runner: `engine/vector-bakeoff/simd/`, Rust 1.96.1, release build.
Host: Intel Core i9-13900H, 14 cores/20 logical processors, 64 GiB RAM,
Windows 11 Pro build 26200. Rayon used 20 threads. Runtime kernels were AVX2/FMA
for f32 & AVX-VNNI `vpdpbusd` for exact i8 dot products.

## Arms & gates

| Arm | Implementation | Parity gate |
|---|---|---|
| A | current dequantized f32 scope scan + full sort | reference |
| B2 | AVX2/FMA full matrix, filter after, bounded top-N | A overlap >= topK-1 + target present |
| B3 | scalar exact-i8 prefilter, 32x exact rerank | exact ordered IDs vs A |
| parallel-B3 | Rayon scalar-i8 scan, 32x exact rerank | exact ordered IDs vs B3/A |
| parallel-B2 | Rayon chunks over AVX2/FMA full matrix | A overlap >= topK-1 + target present |
| B3-SIMD | AVX2 `vpmaddubsw`/`vpmaddwd`, AVX-VNNI where detected, 32x exact rerank | exact i32 dot + exact ordered IDs vs B3/A |
| B2-gather | gather eligible rows, then AVX2/FMA + bounded top-N | A overlap >= topK-1 + target present |

Direct integer ranking was rejected during preflight: N12x100 query 1 returned
127/128 A candidates. All B3 variants therefore share a 32x integer prefilter followed
by exact dequantized reranking. Publication bundles passed exact ordered parity.

All 18 full cells + smoke passed. Every target was present. Every f32 arm recorded
min/mean overlap 128/128 in every full cell. Six Round 1 fixture SHA-256 values match
Mac lane 6/6. Crossover config SHA-256 is
`0abb7ef1ef52617c9b839a0f49b9a58b0ec74c3ef548d81d21a8e4bb25831205`;
Mac had not pushed crossover receipts when this lane fetched, so this committed config
is canonical input for cross-host receipt matching.

## Round 1 matrix — p95 ms

100 queries/cell, 20 warmups, topK 128.

| Cell | A | B2 | B3 | parallel-B3 | parallel-B2 | B3-SIMD | B2-gather |
|---|---:|---:|---:|---:|---:|---:|---:|
| N0 x 100% | 0.79 | 0.34 | 1.28 | 1.19 | 0.24 | 1.24 | **0.21** |
| N0 x 10% | 0.15 | 0.30 | 0.17 | 0.36 | 0.19 | 0.12 | **0.03** |
| N12 x 100% | 17.67 | 8.68 | 8.41 | 6.85 | **3.02** | 7.14 | 4.65 |
| N12 x 10% | 1.31 | 9.35 | 1.70 | 2.42 | 2.33 | 2.17 | **1.00** |
| 100K x 100% | 50.76 | 23.54 | 19.59 | 12.93 | **9.50** | 13.26 | 18.47 |
| 100K x 10% | 5.21 | 16.61 | 6.88 | 4.57 | 6.66 | 5.23 | **2.19** |

No N0 arm regressed more than 1 ms. Selected dispatch improves A at both N0 cells.
At N12x100, parallel-B2 improves p95 82.9% vs A, clearing 25% material-value gate.

## Crossover sweep — p95 ms

| Corpus | Selectivity | A | B2 | B3 | parallel-B3 | parallel-B2 | B3-SIMD | B2-gather |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| N12 | 100% | 12.342 | 7.397 | 9.439 | 7.851 | **3.489** | 8.963 | 5.924 |
| N12 | 50% | 9.590 | 11.930 | 5.626 | 4.680 | **2.916** | 5.091 | 3.008 |
| N12 | 25% | 4.050 | 5.992 | 4.981 | 5.077 | 2.388 | 3.994 | **1.413** |
| N12 | 10% | 1.379 | 5.664 | 3.436 | 2.164 | 2.304 | 1.725 | **0.516** |
| N12 | 5% | 1.102 | 7.624 | 1.891 | 1.502 | 2.341 | 2.377 | **0.591** |
| N12 | 1% | 0.278 | 11.977 | 0.558 | 0.918 | 2.548 | 0.462 | **0.222** |
| 100K | 100% | 40.360 | 28.766 | 21.852 | 15.728 | **10.532** | 18.599 | 17.015 |
| 100K | 50% | 28.327 | 28.175 | 15.279 | 9.676 | **8.942** | 12.075 | 15.436 |
| 100K | 25% | 14.674 | 26.264 | 10.960 | 7.562 | 7.597 | 7.060 | **6.675** |
| 100K | 10% | 5.649 | 21.023 | 7.517 | 6.110 | 6.909 | 6.010 | **2.836** |
| 100K | 5% | 3.795 | 20.420 | 4.284 | 3.987 | 6.680 | 5.429 | **1.433** |
| 100K | 1% | 1.459 | 26.973 | 1.283 | 1.168 | 6.398 | 1.122 | **0.467** |

## Windows dispatch recommendation

Use `parallel-B2` when estimated eligible fraction is >=50%; use `B2-gather` below
50%. Both corpus sizes switch winner between measured 25% & 50% cells. This rule selects
the measured winner in every crossover cell; both N0 selected choices improve A. B3
variants remain fallback evidence, not dispatch winners on this host.

Raw per-query bundles are under `<cell>/simd.json`; immutable hashes & fixture receipts
are in `receipt.json`.
