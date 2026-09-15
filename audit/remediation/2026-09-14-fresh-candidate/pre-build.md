# Fresh candidate pre-build evidence

Authorized: one fresh local unsigned RightKit build. Preserve dirty source; no commit/push.

Pipeline traced: release-build-windows.mjs → engine release binaries → tray release → Hub/NSIS → staged release identity & payload. Use build-only command, not automatic install/qualification wrapper.

Diagnostic coverage in source: client activate_installed returns Result with captured stderr/status; acquisition retry observes startup-lock/health timeout. Runtime supervisor logs stage on timeout; Blueprint checks cancellation before generation write & docs. Both configured-cap & H8 native Pull paths attach stage timings. These are diagnostic changes, not proof shutdown/Pull defects are fixed.

Risks: startup retry uses per-exchange timeouts & may exceed its advertised total budget; installed tests must measure this. In-flight git/store/docs phases remain uninterruptible. HTTP outer deadlines may precede inner timing response. Do not claim full qualification from compilation or binary strings.

Prior focused results reported by implementing agent: Blueprint 196, Pull 114, service 43, client boundary 2 passed; not independently rerun in this build task. Unsigned pipeline compiles release binaries; separate --validate mode is not automatically invoked. No cargo/rustc/makensis process observed before launch. Frozen input hashes recorded for post-build drift verification.

Failure policy: preserve failing stage/log; no automatic rebuild. Verify packaged markers, release identity, source drift & installer hash before installation.

