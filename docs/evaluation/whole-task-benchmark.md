# MBR-804 whole-task benchmark

This contract measures end-to-end task success, stale/unauthorized/missed-authoritative context, latency, token use, cache behavior, cost, and control outcomes. A receipt must name exact release (40-char commit, generation, client, service), corpus (version, case count, SHA-256), every model, and macOS/Windows hardware.

The bakeoff configuration, input corpus, and generated receipt are immutable and identified by SHA-256. Both host receipts must be current, measured, tied to same release generation, and carry their own hashes. Failures remain an explicit array; absence of failures is not evidence of success.

`verify-whole-task-benchmark.mjs` fails closed for missing, stale, unauthorized, mismatched, estimated, or incomplete evidence. `status: source-ready` describes this protocol only and can never produce PASS. Each metric carries `measured: true`; estimated values are not accepted. Publication requires explicit `disposition` (`not-published`, `internal-only`, or `published`) and boolean approval.

Run synthetic checks with `node --test tests/benchmarks/whole-task.test.mjs`. No benchmark is run by this source contract; no result is claimed here.
