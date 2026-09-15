# Candidate4 installed tests — 2026-09-14

Result: NOT qualified. No new build, install, source change, commit or push performed during testing.

Artifact release generation: sha256:ac44820f5b90dc16b573cba5fccca5e27525ae8c98f451261b98b030f2af40c1.

## Passed

- Single established harness: Pull & Blueprint status succeeded.
- Push preserved exact UTF-8 bytes, trailing spaces & content hash; Cortex readback matched submitted body.
- SessionStart & SessionEnd hooks returned ok.
- Tray acquired Hub ownership, held one engine across 35-second dwell & recovered after forced engine termination.
- With both owners active, killing tray preserved harness engine generation through Hub lease expiry.
- Harness-only final release logged clean engine_stopped: 217 ms after functional run; 182 ms after performance run.

## Failed

1. Five simultaneous cold-start stdio clients: one exited during initialize with engine_unavailable / harness ownership acquisition failed. Test stopped & cleaned its clients; one engine was observed. Remaining four initialization outcomes were not fully collected. Client hides activation stderr & acquisition cause (membrane-client.rs:367–398), so exact underlying startup failure remains unidentified.
2. Final release after Hub/watcher activity: process exited, but engine log reported resident Blueprint supervisor drain timeout; singleton retained. Tray helper's finalDrain.complete proves process absence only; it does NOT prove clean teardown. Timeout originates service.rs:500.
3. Concurrent Pull did not consistently complete within requested 15-second deadline. See samples below.

## Bounded HTTP samples

One batch per concurrency, same repository scope, established stdio holder. These are diagnostic samples, not statistically qualified p95/p99 or an idle-versus-indexing benchmark.

| Operation | Concurrency | Success | Deadline HTTP 503 | Other application errors | Explicit HTTP 429 |
|---|---:|---:|---:|---:|---:|
| Pull | 1 | 1 | 0 | 0 | 0 |
| Pull | 5 | 1 | 2 | 2 | 0 |
| Pull | 20 | 1 | 5 | 2 | 12 |
| Push | 1 | 1 | 0 | 0 | 0 |
| Push | 5 | 5 | 0 | 0 | 0 |
| Push | 20 | 16 | 0 | 0 | 4 |

429 responses correctly expose bounded admission (pull_lane_saturated / mcp_lane_saturated); they are not crashes. Single Pull took 9570 ms, single Push 560 ms. Application-error bodies were not retained by this helper, so their specific cause is unclassified.

## Evidence & boundaries

- harness-lifetime.json/log
- tray-lifetime.json/log (read with shutdown-log correction above)
- functional.json/log
- performance.json/log
- lifetime-events.log
- final-processes.json

Not established: full Blueprint parity, every hook, CodeRight-specific lease integration, graceful tray UI Quit, harness crash-recovery path (initialization failed before that stage), or control-route responsiveness during saturated Pull. No full end-to-end seal issued.

Next fixes: preserve underlying activation/acquisition error & reproduce simultaneous startup; trace Blueprint supervisor cancellation during Hub indexing; trace admitted Pull provider/queue time against deadline. All test-owned processes exited; final global Membrane process snapshot empty.
