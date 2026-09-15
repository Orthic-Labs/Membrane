# Installed startup hang diagnosis

Confirmed against installed generation 041bb8df2612c05f0df12bbf2281ea6622a87e115e099e5001ed5d16dc1c0539. No source edits or builds.

Five-client trace: slots 0/2/3/4 initialized at 18.6–19.2 seconds. Slot 1 (PID 251864) never replied. Its activation child PID 252240 launched resident PID 228312; activation itself was absent at 20-second snapshot. Thus failing client was engine launcher, not one of competing activation clients.

Direct activation with stderr capture: PID 244272 exited code 0 at 9158ms, but no stream close occurred by 25018ms. Closing test-side stream ended capture. Independent causal test: activation PID 252608 exited code 0 at 8305ms; resident child PID 231488 survived. Killing that specific child at ~10.3 seconds caused stderr close at 11804ms. This establishes resident lifetime retains activation capture pipe, despite explicit stdio redirection in launch_engine_detached.

Client activate_installed uses stderr(Stdio::piped()) then Command::output(), which waits for captured output EOF as well as process exit. Successful activation does not therefore guarantee return to acquire_lease/MCP initialization. The newly added error capture introduced a dependency on descendant pipe lifetime. Longer lease/initialize timeouts cannot fix this.

Earlier experiment killing engine underneath client did not visibly unblock within its 8-second window; no broader conclusion drawn from that negative observation. Direct exit/close causal experiment isolates pipe lifetime independently of subsequent client retry behavior.

Fix boundary: ensure daemon cannot inherit activation capture handles; decouple bounded child-process completion from unbounded pipe EOF. Capture activation diagnostics in a bounded file or independently bounded reader while waiting on activation process under one absolute deadline. Preserve stderr/status without waiting for daemon exit. Regression acceptance: activation exits & capture completes while resident stays alive; all five installed clients initialize, including launcher; no leaked owners after final close.

Evidence: startup-trace.json, activation-output-test.json, activation-pipe-owner.json, input-test.json, launcher-pipe-test.json. Cold activation also executes binding reconciliation while holding 15-second activation lock; full delay attribution beyond identified pipe hang requires timings, not inference from overall duration.
