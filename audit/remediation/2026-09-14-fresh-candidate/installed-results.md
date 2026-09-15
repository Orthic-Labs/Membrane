# Fresh installed candidate results

Release generation: sha256:041bb8df2612c05f0df12bbf2281ea6622a87e115e099e5001ed5d16dc1c0539.
Source drift: zero frozen input changes. Packaged diagnostic markers present. Installer exit 0; installed payload hashes matched release manifest; installed client build-info matched generation. No additional build, code edit, commit or push in this qualification turn.

## Startup — FAIL
Original five-client cold start exceeded 20-second initialize timeout. Diagnostic rerun with 120-second RPC window collected all outcomes: four clients initialized in 17.2–17.8 seconds; one never initialized within 120 seconds, stderr empty. Recovery stages were not reached. Single cold client also timed out in both 20-second & 120-second diagnostic attempts. Therefore development 5/5 claim does not establish installed behavior. Engine acquired transient holders & later expired/drained while client remained uninitialized; activation/ownership handoff still needs tracing.

## Hub lifetime — partial pass; clean shutdown FAIL
Tray acquired ownership, held one engine, recovered after forced engine termination; harness survived tray loss with unchanged engine generation. On final release, engine exited but emitted resident_blueprint_supervisor_stop_timeout, timeoutMs 5000, stage build:\\?\D:\Claude\coderight. Reproduced after Hub-held performance run too. Helper finalDrain.complete means process absence, NOT clean teardown.

## Pull — concurrency FAIL; diagnostic propagation FAIL
Cold startup prevented initial runs. Hub-held run executed same MCP H8 requests. Single Pull succeeded in 7387 ms; concurrency 5 & 20 each had one success, with application errors, deadlines & explicit admission rejection among remaining requests. Full response bodies saved in performance-hub.json. Push samples: 1/1, 4/5, 16/20 success (20-way had four explicit admission rejections).

No gatewayStageTimingsMs appears in returned MCP envelopes. mcp_executor.rs rebuilds result/receipt, selectively copies federationMetrics at lines 1780/1812, but omits new timing field. Thus binary marker presence did not establish end-to-end diagnostic delivery. Gateway timings still cannot identify internal Pull bottleneck from this test.

## Evidence
installer-hash.json, release.json, diagnostics.json, source-drift.json, payload-verification.json, installed-build-info.json, install-exit.txt; harness-lifetime.json, harness-extended.json; tray-lifetime.json; performance.json, performance-extended.json, performance-hub.json; corresponding logs; runtime-events.log; final-processes.json.

Next source work: trace installed client activation completion/handoff; propagate timing evidence through MCP result & error boundary; trace cancellation within CodeRight initial build. Do not label any of these resolved from compilation. No new build authorized by this test result.
