# Resident request & shutdown flow closure — 2026-09-14

Source implementation & validation complete for gaps found in follow-up trace. Working tree remains uncommitted; no installer build, install, installed-product launch, performance qualification or native-only seal performed. Three-build installer ceiling preserved. Lead traced & integrated code; Luna supplied bounded HTTP, hook, catalog & supervisor changes that lead corrected before testing.

## Production path map

| Entry | Execution / control | Terminal effect / teardown | Evidence |
| --- | --- | --- | --- |
| Direct HTTP MCP Pull & membrane_context | mcp_http::handle_mcp_request → bounded blocking lane → inherited request control → RuntimeMcpExecutor → native_route_response_with_control | Child cancellation follows engine drain; permits stay with actual worker; absolute deadline passes through workspace fanout | MCP 23 tests, executor 13, provider 3 |
| Stdio-only harness | membrane-client::run_stdio_mcp forwards lines to /mcp | Same engine path above; transport process owns no provider runtime | Source trace: membrane/src/bin/membrane-client.rs:248 |
| Legacy HTTP /federate | serve::dispatch → route_with_context_ingest_lease → inherited control → common native wrapper | Reuses current engine Handle; cancellation & deadline survive route adapter | Actual route test submits cancelled & expired controls; refuses before owner binding |
| HTTP /cli → pull federate | controlled_blocking → inherited control → run_cli_captured → run_federate → run_federate_value | Common bridge inherits Handle, deadline & cancellation; response wait ends on cancellation/deadline | Captured CLI Pull test verifies cancelled request never reaches owner binding; authenticated CLI route test passes |
| HTTP /hook → recall | controlled_blocking → hook::invoke captures Handle & child control → module worker enters Handle/control → hook_mode_federate_with_observation → run_federate_value | Module deadline clamped to request deadline; child cancelled at scope exit; later cancelled modules skipped | Actual hook invocation test observes inherited control & Handle; cancelled hook Pull rejects before owner binding |
| Configured-cap Pull | run_federate_value merges inherited control & runtime | Checks cancellation before owner binding, after providers & before response | Federation tests 16 |
| H8 Pull publication | native federation → selection → final fence → controlled catalog transaction → advisory packet cache | Checks cancellation after providers, before selection, before publication & response; mutex/SQLite waits bounded; transaction rollback if cancellation observed before commit | Catalog tests cover cancellation while mutex held, expired request no-write, live request persistence |
| Hub/harness final-owner loss | LifecycleControl::request_drain → descendant cancellation → HTTP drain → Tokio shutdown → telemetry stop → Blueprint supervisor/repo drain | Installed process has 15-second complete teardown watchdog; timeout logs & exits process. OS singleton lock retained until process exit, including detached blocking tasks | Service 35 tests, including spawned test process exercising timeout exit, lock reacquisition after exit & shared resident store access |

## Additional ownership correction

serve::run installs its Cortex store once. open_installed_store & open_installed_lexical_store clone this owner when present. CLI/hook Pull therefore reuse resident store instead of reopening Cortex. Hook ambient bindings retain their lexical/persisted-freshness mode. Standalone compatibility retains fallback store/runtime behavior; production forwarding routes use resident engine.

## Scope of cancellation & shutdown claims

Cancellation is cooperative. Synchronous external calls cannot be forcibly aborted as Rust threads. Their capacity remains charged until execution ends. Publication checks cancellation at transaction boundary; an already committed publication is not retroactively undone. Complete installed-engine teardown has an OS process-exit backstop, covering telemetry joins, watcher joins, store waits & other stuck workers while keeping singleton ownership intact. Development/embedded compatibility entrypoints do not receive installed-process termination policy.

## Verification on final source

Managed RightKit from engine/, --locked --target x86_64-pc-windows-msvc throughout:

- cargo check -p membrane-runtime -p membrane --all-targets: passed.
- Runtime --lib filters: federation 16; real /federate route 1; MCP HTTP 23; hooks 3; controlled catalog publication 3; service 35 (includes live diagnostics service); MCP executor 13; native federation 3; federation sources 2. Total 99 passed, zero failed.
- pnpm test: 32 + 60 passed; one existing skip; legal inventory verified.
- Runtime-language manifest regenerated: 1097 files, zero production interpreter rows, zero errors.
- Invocation graph regenerated: 1101 nodes, 790 edges, zero errors.
- git diff --check passed. Compiler warnings remain; no warning-free claim.

Logs retained under [2026-09-14-flow-closure](2026-09-14-flow-closure/). No installed qualification inferred from these source tests.
