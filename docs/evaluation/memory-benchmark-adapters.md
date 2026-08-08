# Memory benchmark adapter runner (MBR-805)

This document covers the run harness that sits on top of the MBR-805 memory
benchmark contract described in `docs/evaluation/memory-benchmarks.md`. Read
that file first for the result contract (`componentUnderTest`, metric
groups, identity, and the estimated/attribution rejections). This file
covers how a LoCoMo/LongMemEval/BEAM-style run is actually invoked.

## What the runner is, and is not

`benchmarks/memory/runner.mjs` (`runMemoryBenchmark`, `runAllMemoryBenchmarks`)
and its CLI entry point `scripts/qualification/memory/run-memory-benchmarks.mjs`
wire together three things:

1. A local dataset path (`datasetRoot`) supplied by the caller. The runner
   only ever calls `existsSync` on it — it never fetches, clones, or
   downloads a dataset over the network.
2. A caller-supplied `execute({ benchmark, datasetRoot })` function that
   performs the actual benchmark run against `crypt-memory` and returns a
   raw result payload. The runner does not implement this function and does
   not build or invoke the Crypt engine itself; wiring a real executor is a
   book-gate-time integration step, consistent with the no-CI execution
   rules (`docs/plans/.../MEMBRANE-BOOK-MODE-EXECUTION-RULES.md`), which
   keep heavy/engine commands out of task-time changes.
3. The existing MBR-805 verifier (`scripts/qualification/verify-memory-benchmark.mjs`),
   which every executor result is passed through before it can be reported
   as `ran`. This is the same check that rejects `componentUnderTest`
   values other than `crypt-memory` and any `membraneScore`/`managedScore`/
   `attribution`/`marketingClaim` field, so a real run cannot conflate a
   third-party benchmark result with a Membrane or managed-service claim.

## Result states

Every call to `runMemoryBenchmark` returns exactly one of:

- `status: "degraded"` — a precondition is missing: no dataset root was
  configured (`dataset-root-not-configured`), the configured path does not
  exist on disk (`dataset-not-found`), or no executor was supplied
  (`executor-not-configured`). No metrics are present on a degraded result.
- `status: "invalid"` — an executor ran but its payload failed the MBR-805
  verifier (missing identity, an estimated value presented as measured, or
  Membrane/managed attribution). The `reason` field carries the verifier's
  rejection message.
- `status: "ran"` — the payload passed verification. The result carries the
  verified `identity`, `metrics` (grouped into `retrieval`, `admission`, and
  `product`, per the contract), and `componentUnderTest: "crypt-memory"`.

Degrading one benchmark never blocks the others: `runAllMemoryBenchmarks`
(and the CLI's default of running every declared benchmark) reports one
result per benchmark independently.

## No fabricated numbers

The runner never invents metrics on its own. Absent a dataset and an
executor it reports `degraded`; it does not compute or estimate a
placeholder score. The only synthetic payloads in this task are inside the
test files (`benchmarks/memory/runner.test.mjs`,
`scripts/qualification/memory/run-memory-benchmarks.test.mjs`), where they
are passed as an in-test fake `execute` function and use a `corpus.id` of
`"synthetic"` plus a `raw.note` explicitly stating they are fixtures, not
measured benchmark evidence — consistent with the fixture labelling already
established by `benchmarks/memory/memory-benchmark.test.mjs`.

## CLI usage

```bash
node scripts/qualification/memory/run-memory-benchmarks.mjs \
  --benchmark LoCoMo --dataset-root /path/to/local/dataset
```

Omitting `--benchmark` runs every declared benchmark (`LoCoMo`,
`LongMemEval`, `BEAM`). Omitting `--dataset-root` — or pointing it at a path
that does not exist — reports an explicit `degraded` result per benchmark;
it exits `0` because an honestly reported degrade is not a script failure.
The CLI does not supply an `execute` function, so on its own it will always
report `degraded`; a `status: "ran"` result requires a real executor wired
in by the caller at gate time.
