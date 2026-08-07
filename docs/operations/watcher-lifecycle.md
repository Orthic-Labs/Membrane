# Watcher lifecycle

**MBR-201** places the cortex watcher under the per-user Membrane supervisor's
coordination. The watcher remains a separate sidecar (`cortex-watch.mjs`); the
supervisor does not embed it. This document records how duplicate watchers are
prevented across supervisor restarts.

## Why one watcher

The cortex watcher owns one FSEvents / inotify / ReadDirectoryChangesW subscription per
enrolled repository. Two watchers would each open their own file descriptor per root
and each maintain their own debounce queue. The result is doubled CPU and double event
volume, with no semantic benefit: every consumer downstream wants the same
single-pass event stream.

## State diagram

```
            ┌────────────────────────┐
            │      WatcherAction     │
            └────────────────────────┘
              │      │      │
              ▼      ▼      ▼
            Adopt  Spawn   Unavailable
              │    Fresh     │
              ▼      │       ▼
        [record    [cortex-watch.mjs
        never      write pidfile then
        spawn]     adopt on next decide]
```

## Decision rule

The decision is a pure function of three facts:

1. **Is the watcher script available?** If `watcherPolicy.script` resolves to a regular
   file on disk, the answer is `yes`. If the path is `None` or missing, the answer is
   `no` and the supervisor reports `Unavailable`.
2. **What is the recorded watcher PID?** The watcher is required to publish its PID at
   `<watcherPolicy.pidFile>` (canonical: `$HOME/.cortex/watchman.pid`). A missing,
   unreadable, or non-numeric value is `None`.
3. **Is the recorded PID alive?** Probed via `kill(pid, 0)` on Unix. On Windows, the
   probe is conservative and reports `alive` — that case surfaces a `pid-lock-held`
   style error to the user rather than silently reclaiming the watcher.

The mapping:

| Script available | Recorded PID | PID alive | Action |
|---:|---:|---:|---|
| no | (n/a) | (n/a) | `Unavailable` |
| yes | missing/unparseable | (n/a) | `SpawnFresh` |
| yes | present | dead | `SpawnFresh` |
| yes | present | alive | `Adopt { pid }` |

## Two-decision invariant

After the supervisor's outer loop spawns a watcher, the NEXT decision cycle must
observe the recorded PID as alive. The pure function `two_decisions_agree` enforces
this in tests; a `SpawnFresh` followed by another `SpawnFresh` without an intervening
`Adopt` is treated as a hard failure.

In practice this invariant is enforced by the OS service model: only one supervisor
holds the supervisor lock, so only one `SpawnFresh` can race at a time. The other
supervisor sees the recorded PID and adopts.

## Crash-only restart

The watcher is a crash-only process: it never tries to clean itself up. When the
supervisor restarts after a crash, it reads the watcher pidfile as step one. If the
watcher survived (the likely case — the watcher usually outlives a resident crash),
the supervisor adopts it.

If the watcher dies at the same time as the supervisor, the next supervisor's
`SpawnFresh` action reaps the stale pidfile and starts fresh. The cortex-watch.mjs
script is required to clobber any stale pidfile on its own startup.

## What the supervisor does NOT do

The supervisor does not:

- Embed the watcher. Embedding would violate MBR-105's "each tool has one authority"
  rule.
- Restart the watcher on crash. The cortex watcher has its own supervision story;
  double-supervising it would re-introduce the duplicate-spawn problem.
- Detect missed events. If a watcher dies and the supervisor is not yet back up, the
  cortex scanner's daily analysis will surface what was missed.

## Test coverage

The `engine/crates/membrane-supervisor/src/watcher.rs::tests` module asserts the
decision matrix exhaustively. The acceptance test for MBR-201 lives there:

```
fn adopt_dedupes_two_simultaneous_supervisors
```

Two supervisors running concurrently on the same user must both `Adopt`; neither may
`SpawnFresh`. The supervisor's PID lock prevents the second supervisor from doing
useful work — but its decision function MUST observe the live PID and adopt anyway,
because that is what the live supervisor also sees on its next cycle.

## See also

- `docs/operations/supervisor.md` — the supervisor's overall lifecycle.
- `engine/crates/membrane-runtime/src/serve.rs` (lines around 4350–4510) — the
  resident's existing watcher-action logic, which the supervisor now governs.
