# Integrated installed runtime qualification — 2026-09-20

## Scope

Fresh Windows installed-boundary evidence for MEM-010, MEM-012, MEM-055, MEM-056, MEM-068, and CTX-043 after repairing the qualification runner's graceful tray shutdown path. This is an internal unsigned candidate, not RELEASED-boundary closure.

- Source revision: `b6650edb506d26d93a82617711319b32cb847873`
- Installer: `Membrane_Hub_0.1.24_x64-setup.exe`
- Installer SHA-256: `ade8fc0900102c38e23d0c44984b8cbc779c9f0d5edcbafc5093dddeea357bf7`
- Installed release generation: `sha256:217b2e4c1145e3d5ddcc8155328ff692d9d427ca57ba0452b979476c05f5ef23`
- Installed root: `C:\Users\adrds\AppData\Local\Orthic Labs\Membrane\current`
- Qualification profile/verdict: `internal-unsigned` / `unsigned-functional`
- Native receipt: `membrane.windows-installed-qualification.v1`, generated `2026-09-19T22:34:17.7527120Z`
- Native evidence path: `C:\Users\adrds\AppData\Local\Temp\membrane-local-windows-20260919T221029Z\membrane-local-windows-20260919T221029Z.tmp-89512912905b418585387772bb22b10f`

## Repair & lifecycle evidence

`Stop-QualificationHub` had called `CloseMainWindow()` on the tray popover. Production intentionally handles that event by hiding the popover and keeping the tray resident, so the supposed graceful lane waited, force-killed the tray, and depended on lease expiry. The qualification runner now invokes the production `--replace` instance signal, which makes the primary tray begin installed-origin detachment, exit its event loop, drop `InstalledHubLease`, and release its holder.

The fresh installed run recorded:

| Label | Tray exit mode | Drain reason | Elapsed | Result |
|---|---|---|---:|---|
| `graceful-holder-release` | `graceful_replace_signal` | `final_holder_release` | 352 ms | PASS |
| `lease-expiry-recovery` | `forced_kill` | `final_holder_expired` | 27,167 ms | PASS |

Both LC-01 observations are `observed: true`. The graceful lane fails closed if it falls back to `forced_kill_after_replace_signal`; that fallback did not occur. The installed qualification also passed install, startup, Hub health, native cutover, same-version repair, state continuity, uninstall/residue, native-only process tree, Adapt, Blueprint Hub-owned and Hub-off operations, MCP registry, and watcher refresh. Downgrade was not applicable because no older installer was supplied.

## Authentication & CLI dogfood

The installed `membrane-client.exe cli health --timeout-seconds 10` path returned exit 0 with `ok:true`, `serviceId:membrane-hub`, `nativeOnly:true`, a non-empty installation identity, and the exact release generation above. This proves the thin installed CLI resolved canonical authentication without exposing token bytes.

With the tray holding the engine, an otherwise valid MCP `initialize` POST to `http://127.0.0.1:47851/mcp` without authorization returned HTTP 401. The engine was then stopped through the same `--replace` signal; no manual cache or token edits were used.

The native qualification's authenticated stdio MCP lane exposed exactly public `pull` and `push`, returned structured envelopes for every configured compatibility operation, and passed native-host cutover. The focused MCP regression was 94/94 green.

## Durable-memory dogfood

A real installed `membrane-client.exe stdio-mcp` session exercised public `push` with the enrolled `D:\Claude\membrane` binding (`repositoryId=membrane`, `scopeId=membrane`, `taskGrantLevel=write-proposed`). The exact submitted UTF-8 body was:

```text
membrane-dogfood-auth-20260920: byte-exact push/pull qualification.
```

The request replay returned `kind:success`, `status:replayed`, authority `A2`, immutable-source provenance `cortex_agent_memory_source_v1`, `rawBodyBytes:67`, memory ID `membrane/agent_push_643ec14c8f45a96eeeb16b081e76ca3a`, and content hash `sha256:15506402a2b82ae686ac24c40c0320681b405174c16fcf5e2eb83c4d15f32568`. An authenticated `membrane_memory_read` through the same installed stdio bridge, bound to that ID and expected content hash, returned `kind:success`; the returned payload contained the exact submitted body and the same content hash.

Negative scope evidence remained typed: the first call deliberately used an unregistered scope ID and returned `repository_scope_chain_denied` without writing. Current-candidate wrong-repository, non-memory destination, read-only write, idempotency-conflict, and independent raw SQLite source-row checks were not rerun in this lane; older evidence cannot replace those fresh legs at the RELEASED boundary.

## Pull observability

Public `pull` remained honest but did not deliver the dogfood marker in this lane:

- 8,192 response tokens: `request_time_selection_refused` because the protected packet required 8,244 tokens.
- 12,000 response tokens: `context_delivery_capacity_exceeded`.
- 8,300 response tokens: success envelope with `status:insufficient_confidence` and no packet.

These typed outcomes are not a successful pushed-memory-to-Pull recall claim. Exact Cortex source readback passed independently as recorded above.

## Regression evidence

- `node --test scripts/qualification/install-release.test.mjs`: 13 pass, 0 fail.
- `node --test scripts/qualification/cases/mem-lifecycle-windows.test.mjs`: 23 pass, 0 fail.
- `pnpm test`: 32 pass, 1 intentional slow unmocked skip; restored suite 64 pass; legal verification clean.
- `pnpm test:mcp`: 94 pass, 0 fail; one pre-existing unused-function warning.
- `pnpm run release:local:win:unsigned`: exit 0; installed qualification PASS under `unsigned-functional` profile.

## Implementation reconciliation

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| MEM-010 | DELIVERED | `engine/crates/membrane-runtime/src/mcp_http.rs:126-145,195-251` | `engine/crates/membrane-runtime/src/serve.rs:6742-6746` | COMPLETE |
| MEM-012 | DELIVERED | `engine/crates/membrane-runtime/src/mcp_http.rs:163-251` | `engine/crates/membrane-runtime/src/serve.rs:6742-6746` | COMPLETE |
| MEM-055 | DELIVERED | `engine/crates/membrane-runtime/src/residency.rs:25-69` | `engine/crates/membrane-runtime/src/serve.rs:2298-2360,3034-3043` | COMPLETE |
| MEM-056 | DELIVERED | `engine/crates/membrane-runtime/src/residency.rs:63-129` | `engine/crates/membrane-runtime/src/serve.rs:2345-2359,3034-3043` | COMPLETE |
| MEM-068 | DELIVERED | `engine/crates/membrane-runtime/src/mcp_executor.rs:1244-1264` | `engine/crates/membrane-runtime/src/cortex_lifecycle.rs:473-582,603-665` | COMPLETE |
| CTX-043 | DELIVERED | `engine/crates/membrane-runtime/src/cortex_lifecycle.rs:184-225,473-582` | `engine/crates/membrane-runtime/src/cortex_lifecycle.rs:1266-1312` | COMPLETE |

## Focused verification

| Capability targets | Focused command | Direct test evidence | Result | Run identity/time |
|---|---|---|---|---|
| MEM-010 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc --locked --no-fail-fast -p membrane-runtime resident_routes_require_bearer_admission` | `mcp_http::tests::resident_routes_require_bearer_admission` plus installed authenticated CLI health. | FOCUSED_PASS — 1 passed, 0 failed | RightKit `798cd06e-f124-4686-a282-34ebe173e532`, 2026-09-20; 0 failed. |
| MEM-012 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc --locked --no-fail-fast -p membrane-runtime resident_routes_require_bearer_admission` | `mcp_http::tests::resident_routes_require_bearer_admission` plus installed unauthenticated HTTP 401. | FOCUSED_PASS — 1 passed, 0 failed | RightKit `798cd06e-f124-4686-a282-34ebe173e532`, 2026-09-20; 0 failed. |
| MEM-055 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc --locked --no-fail-fast -p membrane-runtime --test residency_holders` | `authenticated_renewal_and_expiry_preserve_final_drain_rule` and `either_holder_starts_one_controller_and_both_deduplicate`. | FOCUSED_PASS — 10 passed, 0 failed | RightKit `f43d331b-3d95-4432-a44d-c662062be50d`, 2026-09-20; 0 failed. |
| MEM-056 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc --locked --no-fail-fast -p membrane-runtime --test residency_holders` | `authoritative_final_release_reports_inactive_while_peer_release_stays_ready` and `peer_held_controller_is_never_drained_by_non_final_holder_release`. | FOCUSED_PASS — 10 passed, 0 failed | RightKit `f43d331b-3d95-4432-a44d-c662062be50d`, 2026-09-20; 0 failed. |
| MEM-068 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc --locked --no-fail-fast -p membrane-runtime agent_push_retains_exact_bytes_and_replays_immutably` | `cortex_lifecycle::tests::agent_push_retains_exact_bytes_and_replays_immutably` plus installed public push replay. | FOCUSED_PASS — 1 passed, 0 failed | RightKit `461c48ca-cd13-4da6-8ee9-963880c97dd7`, 2026-09-20; 0 failed. |
| CTX-043 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc --locked --no-fail-fast -p membrane-runtime agent_push_retains_exact_bytes_and_replays_immutably` | `cortex_lifecycle::tests::agent_push_retains_exact_bytes_and_replays_immutably` plus installed hash-bound exact body resolution. | FOCUSED_PASS — 1 passed, 0 failed | RightKit `461c48ca-cd13-4da6-8ee9-963880c97dd7`, 2026-09-20; 0 failed. |

## Qualification reconciliation

| Capability | Fresh installed result | Remaining boundary |
|---|---|---|
| MEM-010 | Authenticated installed CLI health and native stdio transport passed. | Public RELEASED qualification remains pending. |
| MEM-012 | Valid unauthenticated MCP request refused with HTTP 401; authenticated MCP lane passed. | Full unsafe origin/host/token matrix at RELEASED boundary remains pending. |
| MEM-055 | Hub holder, lease-expiry recovery, singleton/native cutover, and no ownerless restart were observed by the installed lane. | RELEASED delivery remains pending. |
| MEM-056 | Graceful final holder release drained in 352 ms; forced-loss expiry drained separately; both reasons were typed. | Surviving-peer close remains inherited from prior evidence; fresh RELEASED boundary remains pending. |
| MEM-068 | Authorized public push/replay, typed unenrolled denial, provenance, immutable-source identity, hash, and exact resolver readback passed. | Remaining negative matrix, independent raw-row readback, and RELEASED delivery remain pending. |
| CTX-043 | Exact submitted body and expected hash were returned from installed Cortex-owned memory resolution. | Fresh independent immutable raw source/admission row check and RELEASED delivery remain pending. |

Requested / evaluated / unresolved / excluded: **6 / 6 / 6 / 0**. No capability reaches lifecycle closure because every affected canon requires `RELEASED` delivery and this lane is explicitly internal unsigned.
