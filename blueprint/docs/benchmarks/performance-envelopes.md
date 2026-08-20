# Performance envelopes

Every repository class that Cortex claims to serve has a machine-readable
budget table in `evals/performance-envelopes.json`. The table is the single
source of truth for what "acceptable performance" means per class; the test in
`tests/performance-envelopes.test.mjs` enforces it with a **4× CI slack
multiplier**, so a real regression fails CI without a noisy runner flaking the
gate.

## Envelopes measured

| Envelope | Meaning |
|---|---|
| `coldBuildMs` | `cortex build` on a fresh checkout to a sealed generation |
| `incrementalUpdateMs` | one-file delta ingest and journal drain |
| `noopBarrierMs` | `syncToCurrentSource` on a store with no pending events |
| `searchImpactMs` | `cortex orient --json` freshness barrier + canned query |
| `mcpResponseMs` | equivalent read path as served to MCP/UI (status) |
| `rssMb` | peak process RSS during the measured paths |
| `dbSizeMb` | on-disk `graph.db` size for the seeded fixture |
| `incrementalWriteBytes` | store growth from one delta |

## Repository classes

- **small** — ≤ 2k lines, single module, < 50 files (fixture: `mixed-doc`).
- **medium** — 2k-40k lines, multi-module, dozens of files
  (fixture: `typescript-commerce`).
- **large** — > 40k lines or > 500 files. There is no large fixture in CI;
  large-class budgets are validated by the D52 soak fleet under
  `docs/benchmarks/soak.md`.

## Waivers

A budget may be waived by appending to `waivers` in
`evals/performance-envelopes.json`. Each waiver requires:

```json
{
  "id": "perf-2026-08-foo",
  "envelope": "coldBuildMs",
  "repoClass": "medium",
  "rationale": "Benchmarked on a 40-core runner; next optimization tracked in #1234",
  "expiresAt": "2026-09-30T00:00:00.000Z"
}
```

`expiresAt` must be at least 24 hours in the future, or the gate fails. There
are no permanent waivers — expiry forces re-measurement.

## CI

`qualification.yml` runs the envelope suite on every pull request touching
`graph/**`, `tests/**`, `evals/**`, or `package.json`.
