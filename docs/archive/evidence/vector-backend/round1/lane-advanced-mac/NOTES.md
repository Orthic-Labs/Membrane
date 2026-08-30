# Advanced SIMD lane — macOS arm64, 2026-08-01

Same Round 1 fixtures/config, 100 queries plus 20 warmups per cell. New arms run beside
the frozen B2/B3 receipts; no existing result was modified.

| Cell | B2 p95 ms | B3 p95 ms | B3 parallel p95 ms | B4 f16 p95 ms |
|---|---:|---:|---:|---:|
| N12 × 100% | 7.30 | 30.91 | 5.14 | 15.80 |
| N12 × 10% | 4.10 | 1.54 | 0.70 | 1.83 |
| 100K × 100% | 14.30 | 43.69 | 17.43 | 44.43 |
| 100K × 10% | 12.17 | 5.12 | 2.11 | 5.19 |

B3 parallel uses Rayon with 8 threads. Candidate IDs exactly equal scalar B3 for every
query in every cell. B4 f16 used scalar convert-on-load: minimum overlap with arm A was
127/128, mean 127.94 or better, and every target was present. It loses both B2 and B3
parallel in every measured cell, so it is rejected for adoption.

Raw per-query timings and candidate IDs are the four adjacent JSON bundles.
