# Soak

The 1.0 soak exercises a deterministic multi-repo fleet with continuous
create/modify/rename/delete/build-output/branch/worktree churn and every
required fault class. It is the only test that proves a real release can run
a fleet in the wild for days without corruption, false-current responses, or
starvation.

## What runs

`scripts/run-soak.mjs` builds a fleet of N repos, seeds each with a
deterministic generation, then applies the full fault sequence from
`scripts/fault-inject.mjs`:

| Fault class | Source path |
|---|---|
| `watcher_overflow` | actor handles a synthetic overflow event |
| `process_kill` | actor stopped, driver reopens |
| `sqlite_lock_contention` | actor.handleFailure with "database is locked" |
| `disk_full` | actor.handleFailure with ENOSPC |
| `malformed_event` | adapter-rejected event, recorded only |
| `oversized_file` | dropped by descriptor budget, no journal write |
| `directory_event` | ignored by source-file filter |
| `slow_repo` | fleet-level backoff, no actor change |
| `callback_failure` | actor.handleFailure with watch_callback_error |

The full set is fixed at 9 classes; `faultCoverageReport()` makes any missing
class a release blocker. Same seed produces a byte-for-byte reproducible
summary.

## Invariants asserted

The contract test `tests/soak-contract.test.mjs` is the gate. Every run must
prove:

- **Zero graph corruption.** Every repo's store opens after the run and
  reports the expected row counts.
- **No false-current response.** A repo that received a `watcher_overflow`
  is flagged `eventGap: true`; the store is still readable, not torn.
- **Independent repo degradation.** A fault on repo N never changes the
  actor state of repo M. Verified by per-repo failure counters.
- **Bounded work.** Total duration is bounded by event count; the driver
  records `durationMs` in the report.
- **Typed findings.** A thrown `applyFault` for an unrecognised kind is the
  only way a "crash" finding is produced — and it is a code defect, not a
  runtime degradation. The runbook's rule is: a failed invariant blocks
  release; do not reclassify it as flaky.

## CI

A short CI soak runs on every pull request touching `watchman/`, `service/`,
`graph/`, `tests/`, `evals/`, `package.json`, or `pnpm-lock.yaml`:

```sh
node --test tests/soak-contract.test.mjs tests/daemon-recovery.test.mjs tests/freshness-regressions.test.mjs
```

A longer scheduled/manual soak uses:

```sh
node scripts/run-soak.mjs --seed 1 --duration-events 5000 --report evals/soak/long-soak.json
```

The longer run exercises the same invariants with 10× the event count; a
failing long-soak is a release-blocker but a passing short-soak is sufficient
to merge.
