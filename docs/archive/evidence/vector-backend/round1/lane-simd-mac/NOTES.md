# Rust vector optimization matrix — macOS arm64, unified rerun

Date: 2026-08-02. Host: Apple M1, 8 cores, 8 GB RAM, macOS 26.5.2.
Toolchain: Rust/Cargo 1.96.0. Runs used release mode, locked dependencies &
serialized processes.

Canonical crossover config: `config/crossover-v1.json`, SHA-256
`0abb7ef1ef52617c9b839a0f49b9a58b0ec74c3ef548d81d21a8e4bb25831205`.
`round1-v1.json` remained read-only at SHA-256
`ad34773d194597444f07b2a12b0ac40c647476d1483d4122a9f571ad1def6bdf`.

## What changed

Windows `f20f5ab` made B3-family semantics exact through a 32× integer
prefilter (4,096 rows at topK 128) followed by exact f32 reranking. Mac reran
all 12 crossover cells, all 6 Round-1 cells & smoke from that same source.
This invalidated Mac's earlier `parallel-B3` default recommendation.

19 bundles contain 12,705 measurements. Every target is present. B3,
parallel-B3 & B3-SIMD have exact ordered A parity. B2, parallel-B2 &
B2-gather meet overlap ≥127/128; observed full-cell minimum is 128/128.

## Crossover p95 — milliseconds

| Rows | Eligible | A | B2 | parallel-B2 | B2-gather | B3 | parallel-B3 | B3-SIMD | Winner |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 30,549 | 100% | 15.602 | 4.376 | **3.982** | 10.305 | 5.889 | 6.330 | 5.285 | parallel-B2 |
| 30,549 | 50% | 7.921 | 3.330 | **3.007** | 5.649 | 4.469 | 4.738 | 4.542 | parallel-B2 |
| 30,549 | 25% | 4.072 | 3.080 | **2.745** | 3.322 | 3.853 | 3.986 | 3.719 | parallel-B2 |
| 30,549 | 10% | 1.735 | 3.173 | 2.783 | **1.311** | 1.655 | 1.758 | 1.659 | B2-gather |
| 30,549 | 5% | 0.928 | 3.400 | 2.941 | **0.648** | 0.910 | 1.005 | 0.873 | B2-gather |
| 30,549 | 1% | 0.269 | 2.994 | 2.782 | **0.182** | 0.252 | 0.373 | 0.295 | B2-gather |
| 100,000 | 100% | 52.224 | 11.771 | 10.803 | 34.587 | **9.037** | 10.754 | 9.977 | B3 |
| 100,000 | 50% | 26.529 | 10.736 | 9.513 | 19.802 | 9.117 | **7.801** | 8.543 | parallel-B3 |
| 100,000 | 25% | 13.618 | 9.923 | 8.613 | 11.224 | 6.920 | **5.879** | 6.730 | parallel-B3 |
| 100,000 | 10% | 6.247 | 10.918 | 9.790 | **5.104** | 5.385 | 6.013 | 5.342 | B2-gather |
| 100,000 | 5% | 3.290 | 12.707 | 9.570 | **2.942** | 3.311 | 3.104 | 3.434 | B2-gather |
| 100,000 | 1% | 1.158 | 11.672 | 13.625 | **0.818** | 0.831 | 0.949 | 0.836 | B2-gather |

N0 (2,361 rows): B2 wins both Round-1 spots—0.157 ms at 100% versus A
1.189 ms, & 0.071 ms at 10% versus A 0.133 ms. Parallel pool overhead makes
parallel-B2 slower at this scale.

## Dispatch recommendation

Measured maximum-performance Mac policy:

- `<4,096 rows`: scalar B2.
- `4,096–99,999 rows`: parallel-B2 at ≥25% estimated eligibility; B2-gather below 25%.
- `≥100,000 rows`: B3 exact-rerank at 100%, parallel-B3 at 25–50%, B2-gather at ≤10%.

Recommended v1 is smaller: one resident contiguous f32 projection, scalar B2
below 4,096, parallel-B2/gather above it, with a 25% Mac switch. At 100K this
still reduces A by 36.8–79.3% for ≥25% cells while avoiding a second 73 MiB i8
projection. Add B3 resident indexing only after production telemetry shows
100K-scale corpora where its extra 1.7–2.7 ms improvement matters.

Windows remains: B2-gather below 4,096 rows; at larger scales use parallel-B2
at ≥50% estimated eligibility & B2-gather below 50%. Both share one f32 projection.

## Validation & provenance

- Current runner: `2a62a87d14e5f4d536c58b438db63f3d073797a655905589fd142ccef36df396`.
- Timed runner source: pushed `f20f5ab` source `9bc90947…`.
- Post-run source changes only narrow Mac scalar fallback compilation & correct
  two Mac backend labels; tests plus smoke prove output shape/parity unchanged.
- `cargo fmt --check`, Clippy `-D warnings`, 4 Rust tests & 7 Python harness
  tests passed on current source.
- Mac JSON backend labels were corrected from AVX2 wording to Accelerate wording;
  timing, candidates, fixtures & parity fields were unchanged.

## Disposition

- Adopt f32 B2, parallel-B2 & B2-gather.
- Keep A as reference, unsupported-feature fallback & kill switch.
- Defer B3/parallel-B3 production index until 100K demand is observed.
- Reject B3-SIMD from production: exact parity passed, performance did not win.

## NOT-MEASURED

- **sgemm batching:** online fixture is single-query; batching changes latency semantics.
- **GPU/ANE:** transfer, launch & hardware paths require a separate lane; no measurement was run.
- **i8-GEMM dependency:** dependency/layout cost was not justified after native integer SIMD failed to win.

Bundle hashes & fixture bindings are in `receipt.json`.
