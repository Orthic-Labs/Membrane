# Pull runtime & cancellation source fix — 2026-09-14

Status: implemented in working tree; source checks passed. Not committed, packaged, installed, or installed-qualified. Installer budget remains 3/3 used; this work consumed zero installer builds. Earlier unrelated working-tree changes preserved.

## Changes reviewed by lead after Luna implementation

- Resident HTTP Pull borrows engine Tokio Handle, including workspace workers & configured-cap routes. Standalone compatibility retains scoped runtime with bounded shutdown.
- Engine drain cancels descendant request tokens. Losing Hub while another holder remains does not cancel engine work.
- HTTP Pull admission capped at 8 of 16 MCP requests; protocol capacity remains available under Pull saturation. Permits remain held until actual blocking dispatch finishes.
- Queued dispatch checks cancellation & absolute deadline before executing. Provider work, freshness & grant work share 10 global blocking permits; queue waiting consumes deadline. Running synchronous work requires cooperative cancellation; timeout does not forcibly terminate threads.
- Federation cancellation propagates through child token & drop guard; workspace deadlines never reset ingress deadline.
- Fixed integration fixtures exposed by all-target checking: Harness match arms & shadowed authorization request helper.

Source: engine/crates/membrane-runtime/src/{service.rs,mcp_http.rs,mcp_executor.rs,pull/native_federation.rs,pull/federation.rs,pull/federation_sources.rs}; fixtures under tests/{residency_holders.rs,native_authorization.rs}.

## Verification

All Rust commands used managed RightKit from engine/, with --locked --target x86_64-pc-windows-msvc.

`rightkit cargo check -p membrane-runtime -p membrane --all-targets --locked --target x86_64-pc-windows-msvc`: PASS. Existing compiler/linker warnings remain.

| Test filter / target | Passed |
| --- | ---: |
| pull::native_federation::tests | 3 |
| mcp_http::tests | 22 |
| service::tests (also matches live diagnostics service) | 31 |
| mcp_executor:: | 13 |
| pull::federation_sources::tests | 2 |
| pull::federation::tests | 15 |
| runtime_shutdown_is_bounded_when_blocking_work_ignores_cancellation | 1 |
| integration native_authorization | 17 |
| integration residency_holders | 10 |
| Total | 114 |

Regression evidence includes five concurrent calls retaining same runtime identity, expired queued provider never executing, cancellation reclaiming provider permit, saturated Pull returning 429 while ping succeeds, queued expired dispatch returning its permit, sibling holder continuity & final drain cancellation.

Runtime language manifest: 1097 files, zero production interpreter rows, zero errors. Invocation graph: 1101 nodes, 790 edges, zero errors. Both checks pass after regeneration. No native-only seal issued. git diff --check passes.

Logs are retained in sibling directory 2026-09-14-pull-cancellation/. Final compiler log is .tmp-pull-fix-check-final.log; corrected executor filter log is .tmp-pull-fix-mcp-executor-final.log (earlier executor filter selected zero tests, excluded from total).

Installed lifetime, real Push under Pull saturation, installed Pull performance & final artifact qualification remain separate from these source results. No product process was launched for this change.
