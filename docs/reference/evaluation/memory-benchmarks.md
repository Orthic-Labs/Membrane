# Memory benchmark contract (MBR-805)

This source-ready contract evaluates `cortex-memory` only. Adapters accept LoCoMo, LongMemEval, & BEAM-style payloads while preserving original payload under `raw` & its `role` (`input`, `result`, or other benchmark-defined role).

Every result must identify corpus (`id`, `version`), model, hardware, & release. Missing identity is an open result. Synthetic fixtures may exercise adapters; they are not measured benchmark evidence.

Metrics remain separate: `retrieval` (e.g. recall/ranking), `admission` (e.g. accepted/rejected decisions), & `product` (e.g. latency, reliability, usability). No aggregate is implied. Values marked `estimated: true` are rejected as measured evidence.

`componentUnderTest` is exactly `cortex-memory`. Membrane, managed-service, unrelated-system, or marketing score attribution is rejected. This contract does not claim Membrane performance, product superiority, or corpus/model/hardware/release results without a measured receipt.

Run a fixture verifier with `node scripts/qualification/verify-memory-benchmark.mjs path/to/result.json`; exit 0 means schema checks passed, not that a real benchmark was run.
