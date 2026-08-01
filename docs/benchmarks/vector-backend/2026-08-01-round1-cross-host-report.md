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

The Round 1 arm B implementation re-quantizes every row on every query (`engine/vector-bakeoff/common/src/lib.rs`, `quantized_candidates`) instead of holding quantized vectors resident as specified. Its numbers above therefore measure a pessimized arm and must not be read as "optimized Rust" (Windows 100K×100%: 1,147 ms p95 is this defect, not quantization cost). Round 1 result bytes remain frozen. Corrected follow-on lanes have now run on both hosts with parity gates.

- **B2:** full-matrix scores + bounded top-N; Mac uses Accelerate `cblas_sgemv`, Windows uses runtime-gated AVX2/FMA intrinsics.
- **B2-gather:** filter-first gather + subset Accelerate/AVX2 scoring.
- **parallel-B2 / parallel-B3:** Rayon scoring/selection rungs.
- **B3:** resident i8 prefilter + exact f32 rerank; ordered candidate parity versus A.
- **B3-SIMD:** runtime-detected integer SIMD with scalar fallback + exact rerank; ordered candidate parity versus A.

### Corrected Round-1 lane winners — p95 ms

Both hosts ran all seven arms from unified source. Each lane publishes 19
bundles: smoke, six Round-1 cells & 12 crossover cells.

| Cell | Mac winner | Mac p95 | Mac A | Windows winner | Windows p95 | Windows A |
|---|---|---:|---:|---|---:|---:|
| N0 × 100% | B2 | 0.157 | 1.189 | B2-gather | 0.210 | 0.792 |
| N0 × 10% | B2 | 0.071 | 0.133 | B2-gather | 0.032 | 0.148 |
| N12 × 100% | parallel-B2 | 4.097 | 15.349 | parallel-B2 | 3.017 | 17.670 |
| N12 × 10% | B2-gather | 1.160 | 1.678 | B2-gather | 1.005 | 1.308 |
| 100K × 100% | B3-SIMD | 9.655 | 50.082 | parallel-B2 | 9.502 | 50.761 |
| 100K × 10% | parallel-B3 | 4.253 | 5.689 | B2-gather | 2.192 | 5.210 |

### Unified crossover winners — p95 ms

| Scale | Eligible | Mac winner | Mac p95 | Windows winner | Windows p95 |
|---|---:|---|---:|---|---:|
| N12 | 100% | parallel-B2 | 3.982 | parallel-B2 | 3.489 |
| N12 | 50% | parallel-B2 | 3.007 | parallel-B2 | 2.916 |
| N12 | 25% | parallel-B2 | 2.745 | B2-gather | 1.413 |
| N12 | 10% | B2-gather | 1.311 | B2-gather | 0.516 |
| N12 | 5% | B2-gather | 0.648 | B2-gather | 0.591 |
| N12 | 1% | B2-gather | 0.182 | B2-gather | 0.222 |
| 100K | 100% | B3 | 9.037 | parallel-B2 | 10.532 |
| 100K | 50% | parallel-B3 | 7.801 | parallel-B2 | 8.942 |
| 100K | 25% | parallel-B3 | 5.879 | B2-gather | 6.675 |
| 100K | 10% | B2-gather | 5.104 | B2-gather | 2.836 |
| 100K | 5% | B2-gather | 2.942 | B2-gather | 1.433 |
| 100K | 1% | B2-gather | 0.818 | B2-gather | 0.467 |

Both lanes passed target, overlap & exact-order gates. Windows passed 7 Rust
tests, 7 Python tests & Clippy. Current unified Mac source passed 4 Rust tests,
7 Python tests & Clippy. Fixture hashes match 12/12 crossover & 6/6 Round-1.

Recommended v1 uses one resident f32 projection: Mac scalar B2 below 4,096,
parallel-B2 at ≥25% eligibility & B2-gather below; Windows parallel-B2 at
≥50% & B2-gather below. Mac B3 paths remain measured 100K upgrades, deferred
until production corpus scale justifies a second resident i8 projection.

## Reading (our constraints; measurements above stand on their own)

1. Round 1 external/database arms did not clear the pre-registered ≥25% N12 material-value gate. Corrected same-store arms now clear it on both hosts: Mac parallel-B2 is 74.5% faster than A at N12×100%; Windows parallel-B2 is 71.7% faster.
2. Mac f32 dispatch crosses between 10% & 25%; Windows crosses between 25% & 50%. Exact-rerank B3 becomes a Mac winner only at 100K ≥25%.
3. External-backend anomalies remain Round 2 work: Windows arm-E's recall miss and arm F's 100K×100% cross-host inversion (48 ms Windows vs 491 ms Mac).

**Disposition (ours):** adopt host-specific same-store f32 dispatch; retain current storage/ranking authority. `DEFER WITH MEASURED TRIGGER` remains the external-vector-backend disposition — trigger = corpus approaching 100K rows with vector-stage p95 outside budget after same-store dispatch.

## Limitations

- Production corpus is small (2,361 rows); synthetic corpora above it are seeded-generated (single shape, production-like scope distribution), not organic.
- ANN arms untuned (defaults); exact arms are the shipping implementations. Round 2 tunes on a calibration split with a locked holdout.
- Two hosts, one hardware class each; single embedding shape (768-dim); one engine's scope-filtered retrieval pattern — this is not a general-purpose ANN benchmark and not a substitute for ann-benchmarks.
- Cells not listed were not run; nothing is interpolated.
