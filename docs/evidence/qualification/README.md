# Qualification evidence

Store real MBR-801 installed-path conformance receipts under `mbr801/<platform>/receipt.json`
(schema `orthic.mbr801-installed-receipt.v1`), produced by `node scripts/qualification/run.mjs`
and validated by `scripts/qualification/verify-mbr801-evidence.mjs`. `tools/productization/generate-support-matrix.mjs`
(MBR-808) derives the published support-tier matrix (`docs/support-matrix.md`,
`docs/support-matrix.json`, the README support-tier block, and `server.json`'s
per-target `platformReceipt` fields) from exactly these receipts, cross-checked
against the current commit and release generation. A receipt names one client
(whichever installed host ran the ten-scenario harness); it never qualifies a
different client or a stale commit/release generation. Missing, stale,
malformed, or incomplete receipts render `unavailable`, not `unsupported` and
not `qualified`.
