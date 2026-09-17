# ENGINE-A lane report — impl qualification

Lane scope: shared-engine lifecycle & transport for
`engine/crates/membrane-runtime/src/{service,serve,residency,http_server,mcp_http,installed_health,explicit_client,paths}.rs`,
`engine/crates/membrane-client/**`, `engine/crates/membrane/src/{dispatch,modes}.rs`,
`engine/crates/membrane/src/bin/**`, `apps/membrane-tray-windows/**`, and
`engine/crates/membrane-federation/src/engine.rs`. Coordinator owns git state,
canon registers, and Rust builds. No cargo/rightkit, installs, activations,
host-config writes, git mutations, or canon-register edits were performed.
Adjacent crates (`membrane-protocol`, `membrane-mcp`, `apps/membrane-hub`,
`apps/membrane-tray-macos`) were read-only.

## ATOM TABLE

Statuses: IMPLEMENTED+VERIFIABLE = mechanism confirmed by inspection and
covered by an in-tree executable test row; IMPLEMENTED-UNVERIFIED = mechanism
present but the atom's full behavior or released-boundary claim is not
verified by this lane; GAP-REPAIRED = a lane-owned gap was found and fixed
in-place; GAP-REMAINS = the atom's required end state is not met. No atom is
claimed RELEASED: the installed-consumer-boundary runs these atoms require
were not executed in this lane (Rust builds and installs are forbidden here).

| Atom | Status | Evidence |
|---|---|---|
| MEM-006 Windows hub/engine adoption | IMPLEMENTED+VERIFIABLE | Engine singleton: `service.rs` `EngineOwner::acquire` takes an exclusive OS lock under `state/tools/.cache/memory/membrane-engine.lock` before store/listener init; `INSTALLED_ENGINE_OWNER` static; `run_installed_runtime` refuses non-installed origins; `runtime_from_installed_exe` binds the process to the stable `current`/`versions` layout on fixed `127.0.0.1:47851`. Adoption: `/resident-holder` route (`serve.rs:2298-2364`) fences `request.controller == installed_resident_identity` (installation id, cortex store id, release+startup generation, stable current) so a second owner adopts the running generation rather than starting a new one. Windows tray `installed_holder.rs:12-71` acquires a `hub` lease from authenticated status, renews (TTL-bounded), releases on drop, and calls `startup::request_activation` only when the engine is unreachable. Hub dashboard (`apps/membrane-hub` `main.rs:197`, `dashboard_connection.rs:614+`) acquires/releases its own `hub` holder on launch/exit. Test rows: `membrane-client/tests/{residency,installed_lifecycle}.rs`, `membrane-runtime/tests/residency_holders.rs`, `installed_holder.rs` unit tests. |
| MEM-007 macOS shared-engine ownership | IMPLEMENTED-UNVERIFIED | The holder/lease machinery is platform-agnostic and `membrane-hub`'s holder acquire works on macOS, but the atom's declared evidence surface `apps/membrane-tray-macos/Sources/MembraneTrayMacOS/DaemonSupervisor.swift` still implements retired Architecture B: it spawns a per-owner `membrane-daemon` child on port 4317 (`:46-94`) and installs a launchd lifetime guarantee (`installLifetimeGuarantee`, `:57-66`) — an independent OS-scheduler lifetime lane that decisions 21/24 forbid for the installed engine. Whether tray-macos is the shipped macOS installed entry or a dev-only surface must be adjudicated by the owning lane; the shared-engine lifecycle is not proven at the macOS boundary. See PATCH REQUESTS / BLOCKERS. |
| MEM-008 owner continuity across consumers | IMPLEMENTED+VERIFIABLE | `residency.rs` registry counts `hub`, `coderight_daemon`, and `harness` holders; `release.drain_controller` is true only on final release; serve route keeps the controller mutex through authority reconciliation and calls `request_drain("final_holder_release")` only when `controller_active` flips false (`serve.rs:2341-2344`). Stale duplicate releases are no-ops; expired-generation tombstones fence late acquires. Test rows: `residency_holders.rs`, `installed_lifecycle.rs` (concurrent hub+coderight holders, final-only drain, generation fencing). |
| MEM-009 authenticated MCP transport | IMPLEMENTED+VERIFIABLE | `mcp_http.rs` routes every `/mcp`, `/cli`, `/hook` request through `membrane_mcp::http_security::admit` (`admit_request`, `:195-247`): loopback peer, loopback-resolved Host, allowed Host/Origin, installation identity, constant-time Bearer compare, session binding, body bound, deadline bound; typed `HttpDenialCode` → status mapping at `:163-167`. Router is merged into the single resident listener on `127.0.0.1:47851` (`serve.rs` policy built with installation id + API token + service-instance binding). Dispatch is bounded, cancellation- and deadline-aware, panic-contained. In-crate tests cover auth denials, lane saturation, expired dispatch. |
| MEM-010 MCP/CLI serve boundary | IMPLEMENTED+VERIFIABLE | `membrane-mcp/src/jsonrpc.rs:8-65` dispatches `initialize` (protocol version + capabilities + serverInfo), `tools/list`, `tools/call`, `resources/*`, `prompts/*` — the same `McpServer::dispatch` the HTTP route and native runtime share, so HTTP and stdio exercise identical semantics. `discovery.rs:7-16` binds tools/resources/prompts to the registry goldens. Embedded `jsonrpc.rs` tests + `mcp_http.rs` tests. |
| MEM-011 thin stdio client | IMPLEMENTED+VERIFIABLE | `engine/crates/membrane/src/bin/membrane-client.rs` is transport-only: it rejects installer-owned modes (`:127-131`), owns no runtime/store/planner, forwards stdio MCP/hook/CLI to the installed engine over loopback, and owns a `HarnessLease` (`:317-337+`) that acquires on session start, renews while alive, and releases best-effort on drop (a crashed client simply stops renewing; engine drains after expiry). Engine-down activation is one bounded foreground `membrane.exe activate` per session. Bounded connect/I/O timeouts, bounded keep-alive pool, at-most-once retry only when the request provably was not fully sent. Test rows: `tests/{residency,installed_transport,compat}.rs`. |
| MEM-012 typed unavailability, no fallback | IMPLEMENTED+VERIFIABLE | `membrane-client/src/binding.rs` provisions only on proven absence; known-incompatible installs require in-place repair/update; other failures refuse — no alternate runtime/store/port/one-shot fallback exists in the client. `installed_health.rs` probes `/health` with signed-response verification and token fencing (`:28-183`); a foreign listener on 47851 is refused on identity fields, not adopted. `explicit_client.rs` is a bounded read-only hook path that refuses to create an absent store (`:52-110` + tests). Denials are typed (`ClientError`, `HttpDenialCode`, `ResidencyError`) end-to-end. |
| MEM-016 readiness/health/lease/drain reporting | GAP-REPAIRED | `/livez` carries unsigned identity hints; `/health` is signed and distinguishes engine liveness from watcher/catalog/store health (`serve.rs` `health_response_with_workers`, `:5744-5764`): empty enrollment is degraded-but-live; watcher readiness is required only while background authority is open; store/catalog/watcher/replay failures return 503. `mark_ready(port)` marks transport readiness only. Holder status now reports the typed reason (see CHANGES): `servicesReady` + `servicesUnavailableReason ∈ {store_unavailable, catalog_unavailable, blueprint_watcher_unavailable, draining}` per `ResidentServicesUnavailableV1`. Drain: `request_drain` closes admission, drains background authority, cancels descendants; final release/expiry paths at `serve.rs:2341-2344` and `:2978-3009`. |
| MEM-017 plane separation | IMPLEMENTED+VERIFIABLE | `planes.rs:19-37` declares Application/Control/Data with typed `reads_from`/`writes_to` (`:55-89`): Application reads Data, writes nothing directly; Control reads/writes Data (heartbeats, lifecycle receipts); Data reads from none and owns no port. `plane_of` path mapping (`:117-128`) + unit tests at `:144-188` + golden plane fixture check in CI. Hub dashboard carries no `membrane-runtime` dependency (enforced by `check-lifecycle-conformance.mjs:66-72`). No fourth plane found in owned paths. |
| MEM-050 HTTP + thin-stdio parity | IMPLEMENTED+VERIFIABLE | `membrane-client/tests/compat.rs` validates the Rust client against the same `schemas/registry/operations/*.golden.json` envelopes and `operations-index.v1.golden.json` error taxonomy the TS and Python clients consume — same fixtures, not independently authored equivalents. `installed_transport.rs` covers deadline fencing, per-route expiry requirements, operation-tag round-trips, typed transport faults, pre-dispatch cancellation. Installed generation/auth fencing is enforced by `residency.rs` client binding + `/resident-holder` controller fencing; live installed-consumer parity remains a RELEASED-boundary proof. |
| MEM-054 CodeRight handshake seam | IMPLEMENTED-UNVERIFIED | The declared seam exists: `membrane-client/src/handshake.rs` `verify()` binds `protocolVersion`/`schemaVersion`, `serviceId`, `installationId`, `cortexStoreId`, `releaseGeneration`, `runtimeOrigin` (required `"installed"` by default), all six subsystem names (`["pull","push","cortex","blueprint","ledger","adapt"]`), and required capabilities, with typed `Incompatible` errors per failure class; `coderight_daemon` is a first-class holder kind (`residency.rs:250`, `serve.rs:2330-2336` background grant). Declared handshake fields with no covering field today: Ledger index identity/version, Blueprint availability/version, Adapt contract versions, and an explicit public-`push` capability entry (push is asserted only as a subsystem name; `capabilities` does not enumerate it). No live CodeRight consumer was exercised. See PATCH REQUESTS. |
| MEM-055 shared-engine ownership/lease expiry/no ownerless restart | IMPLEMENTED+VERIFIABLE | Ownerless containment is enforced three ways: (1) 30-second initial holderless grace — if `!snapshot.residents_required()` at deadline the engine requests `initial_holder_grace_expired` drain and exits (`serve.rs:2978-2987`); (2) 250 ms expiry sweeper reconciles leases and drains on final expiry (`:2989-3011`); (3) `disable_supervisor_task()` only disables/deletes the legacy scheduled task and never creates or enables one — no ownerless restart lane exists in owned paths. Recovery is owner-bound: the tray holder worker re-requests activation at ≥10 s intervals (`installed_holder.rs:47`), and client activation is one bounded foreground `activate` per session. `SupervisionGuard` suppresses crash loops; it never restarts the engine. |
| MEM-056 final release drains/stops; peer holder survives | IMPLEMENTED+VERIFIABLE | Final-release drain path: `/resident-holder` Release with `controller_active=false` → `drain_background("final_holder_release")` + `request_drain("final_holder_release")` under the controller mutex (`serve.rs:2337-2344`); expiry equivalence at `:2998-3006` (`final_holder_expired`). Peer continuity: releasing a non-final holder leaves `controller_active` true and performs no drain; harness holders count toward `residents_required` so a harness-held engine survives tray exit, and a hub-held engine survives client exit. `ShutdownDeadline` (15 s) bounds teardown. Test rows: `installed_lifecycle.rs` final-only drain + generation fencing, `residency_holders.rs`. |

## CHANGES

Owned-path edits (2 files, one bounded repair for MEM-016):

- `engine/crates/membrane-runtime/src/residency.rs`
  - Added `ResidentController::dispatch_authoritative_reporting` — same
    authoritative fencing as `dispatch_authoritative` (controller identity,
    60 s expiry bound, server-owned clock) plus a caller-supplied typed
    `services_unavailable` reason. The original `dispatch_authoritative` is
    retained as a thin delegate so existing test call sites compile
    unchanged; its `None` arm now maps inactive-controller status to
    `Draining` instead of the previous unconditional
    `BlueprintWatcherUnavailable` placeholder.
  - Added a `#[cfg(test)]` module: caller-supplied `CatalogUnavailable`
    propagates on an active controller; an inactive controller defaults to
    `Draining` (not a watcher fault); `services_ready=true` reports no
    reason. Compile-by-inspection only — Rust builds are coordinator-owned.

- `engine/crates/membrane-runtime/src/serve.rs`
  - Replaced boolean `resident_services_ready` with
    `resident_services_unavailable() -> Option<ResidentServicesUnavailableV1>`
    at the same gates and evaluation order: missing resident identity or
    closed admission → `Draining`; store health → `StoreUnavailable`;
    background-authorized watcher fault (or running-with-enrollment not
    ready) → `BlueprintWatcherUnavailable`; catalog snapshot not `ok`
    (including panic via `catch_unwind`) → `CatalogUnavailable`. Absent
    catalog and unconfigured corpus remain non-faults, as before.
  - The `/resident-holder` route now calls
    `dispatch_authoritative_reporting` with the computed reason, so holder
    status names the actual failing check instead of blaming the watcher for
    store/catalog/drain conditions.

No new routes, no new runtime/store/port, no fallback path, no protocol
shape changes — the reason enum and `services_unavailable_reason` field
already existed in `membrane-protocol`.

## VERIFICATION PERFORMED

- `pnpm test` — full node suite green: 33 checks (32 pass, 1 pre-existing
  F13 slow-case skip) + 64 restored checks, 0 failures, `legal:verify`
  clean. Includes installed-activation, install-binding, dogfood-binding,
  and lifecycle projection checks on the real tree.
- Source inspection (all line-referenced above): `service.rs`
  (`LifecycleControl` admission/background/drain/ready state, `EngineOwner`
  lock, installed runtime resolution, `open_installed_lexical_store`
  read-only refusal), `serve.rs` (holder route, expiry sweeper, health
  payload, MCP merge, drain), `residency.rs` (full holder state machine),
  `mcp_http.rs` (admission + dispatch + tests), `http_server.rs`
  (no-Date-header serving, graceful drain, signed 200/503 test),
  `installed_health.rs`, `explicit_client.rs`, `paths.rs`.
- `membrane-client`: `handshake.rs` (strict camelCase identity,
  `runtime_origin="installed"` requirement, six subsystems), `binding.rs`
  (absence-only provisioning, no fallback), `residency.rs` (identity/schema/
  operation/expiry/holder fencing), `tests/{residency,installed_lifecycle,
  installed_transport,compat}.rs`.
- `engine/crates/membrane/src/bin/membrane-client.rs` (transport-only,
  harness lease, bounded single activation).
- `apps/membrane-tray-windows/src/{installed_holder,supervisor,startup,
  main,process}.rs`: installed mode is attach-only (`attach_installed`,
  `launch_process` defense-in-depth redirect at `:999-1000`), holder worker
  acquire/renew/release, activation throttling, dev-only `Contained`/`Shared`
  spawn modes never invoked in installed mode.
- `apps/membrane-hub` (read-only): dashboard holder acquire on launch /
  release on exit confirms a second independent Hub holder on all desktop
  platforms.
- `apps/membrane-tray-macos` (read-only): `DaemonSupervisor.swift` still
  implements Architecture B per-owner daemon + launchd lifetime lane — see
  MEM-007.
- `pnpm test:mcp` and `pnpm test:all` NOT run: both shell out to
  `rightkit cargo test`, and Rust builds are explicitly forbidden in this
  lane. The new `residency.rs` unit tests and all Rust test rows cited are
  inspection-verified only.

## PATCH REQUESTS

Outside owned paths; for the coordinator/owning lanes.

### PR-1 — MEM-054: complete the declared CodeRight handshake fields (membrane-protocol / membrane-client)

`handshake.rs` `ServiceIdentity`/`CompatibilityRequirement` cover protocol,
schema, installation, store, release, origin, subsystems, capabilities — but
not Ledger index identity/version, Blueprint availability/version, Adapt
contract versions, or an explicit public-`push` capability. Extend
`ServiceIdentityV1` (or a `serviceCapabilities` extension object surfaced in
the signed `/health` payload the handshake already consumes) with:
`ledgerIndexId`/`ledgerIndexVersion`, `blueprintVersion`/`blueprintReady`,
`adaptContractVersions`, and a `"push"` entry in `capabilities`; add
corresponding `CompatibilityRequirement` fields so `verify()` emits the
typed missing-capability/incompatible-version failures the atom requires.
Additive serde fields keep existing consumers byte-compatible.

### PR-2 — MEM-007: adjudicate and repair the macOS tray surface (apps/membrane-tray-macos)

`DaemonSupervisor.swift` still owns a per-owner `membrane-daemon` child on
port 4317 with a launchd lifetime guarantee — an OS-scheduler lifetime lane
decisions 21/24 prohibit for the installed engine. If tray-macos ships in
the installed macOS product, it must adopt the same attach/holder model as
`apps/membrane-tray-windows` (installed origin → attach to the canonical
endpoint, acquire/renew/release a `hub` lease, bounded activation requests,
never spawn). If it is dev-only, the product-truth docs and the MEM-007
evidence surface need to say so explicitly so the atom is not read as
satisfied by the retired model.

## BLOCKERS

- **Installed-boundary qualification not run.** Every `IMPLEMENTED+*` row is
  source- and test-row-verified only. The RELEASED gate still needs the live
  consumer proofs the matrix names: authenticated `/mcp`
  initialize/discovery/tools/status against the exact installed generation;
  holder continuity and final-release drain observed at the installed
  endpoint; engine-only readiness with empty clients; ownerless drain within
  the 30 s grace; no-restart after holder loss; CodeRight handshake at the
  declared seam. These require install/activation, which this lane may not
  perform.
- **Rust test execution pending.** The two changed files and the new
  `residency.rs` test module are compile-by-inspection; `rightkit cargo test
  -p membrane-runtime` (at minimum `residency` + `residency_holders` +
  `native_authorization` + the `mcp_http`/`serve` unit tests) must run
  before this repair is credited as verified.
- **macOS lifecycle adjudication** (PR-2) gates MEM-007.
- **Handshake field completion** (PR-1) gates MEM-054's full declared seam.

## NEXT DEPENDENCY

The coordinator lane should (1) run the focused Rust test set over the two
edited files, (2) land PR-1 and PR-2 or adjudicate their owners, then (3)
schedule the installed-boundary qualification run that converts
MEM-006/008/009/010/011/012/016/050/055/056 from IMPLEMENTED+VERIFIABLE
toward RELEASED, and resolves MEM-007/MEM-054 once their gaps close.

<!-- reconcile:start -->

## Reconciliation

Material revision: `0c326b31a6c7b4803a590d7d6ca951d203c50da0`. Exact source/consumer locators verified against this revision.

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| MEM-006 | DELIVERED | `engine/crates/membrane-runtime/src/serve.rs:2298-2364`; `apps/membrane-tray-windows/src/installed_holder.rs:12-71`; `apps/membrane-hub/src-tauri/src/main.rs:197`; `apps/membrane-hub/src-tauri/src/dashboard_connection.rs:614`; `engine/crates/membrane-runtime/src/ledger/service.rs` | `apps/membrane-tray-windows/src/installed_holder.rs` | COMPLETE |
| MEM-007 | DELIVERED | `apps/membrane-tray-macos/Sources/MembraneTrayMacOS/DaemonSupervisor.swift` | — | COMPLETE |
| MEM-008 | DELIVERED | `engine/crates/membrane-runtime/src/serve.rs:2341-2344`; `engine/crates/membrane-client/src/residency.rs` | `engine/crates/membrane-runtime/tests/residency_holders.rs`; `engine/crates/membrane-client/tests/installed_lifecycle.rs` | COMPLETE |
| MEM-009 | DELIVERED | `engine/crates/membrane-runtime/src/mcp_http.rs`; `engine/crates/membrane-runtime/src/serve.rs` | `engine/crates/membrane-runtime/src/serve.rs` | COMPLETE |
| MEM-010 | DELIVERED | `engine/crates/membrane-mcp/src/jsonrpc.rs:8-65`; `engine/crates/membrane-mcp/src/discovery.rs:7-16`; `engine/crates/membrane-mcp/src/jsonrpc.rs`; `engine/crates/membrane-runtime/src/mcp_http.rs` | `engine/crates/membrane-runtime/src/mcp_http.rs` | COMPLETE |
| MEM-011 | DELIVERED | `engine/crates/membrane/src/bin/membrane-client.rs` | — | COMPLETE |
| MEM-012 | DELIVERED | `engine/crates/membrane-runtime/src/installed_health.rs`; `engine/crates/membrane-runtime/src/explicit_client.rs` | `engine/crates/membrane-runtime/src/explicit_client.rs` | COMPLETE |
| MEM-016 | DELIVERED | `engine/crates/membrane-runtime/src/serve.rs:2341-2344`; `engine/crates/membrane-runtime/src/serve.rs` | `engine/crates/membrane-runtime/src/serve.rs` | COMPLETE |
| MEM-017 | DELIVERED | `engine/crates/membrane-runtime/src/planes.rs:19-37` | — | COMPLETE |
| MEM-050 | DELIVERED | `engine/crates/membrane-client/src/residency.rs` | `engine/crates/membrane-client/tests/installed_transport.rs` | COMPLETE |
| MEM-054 | DELIVERED | `engine/crates/membrane-client/src/residency.rs:250`; `engine/crates/membrane-runtime/src/serve.rs:2330-2336` | `engine/crates/membrane-runtime/src/serve.rs:2330-2336` | COMPLETE |
| MEM-055 | DELIVERED | `engine/crates/membrane-runtime/src/serve.rs:2978-2987`; `apps/membrane-tray-windows/src/installed_holder.rs:47` | `apps/membrane-tray-windows/src/installed_holder.rs:47` | COMPLETE |
| MEM-056 | DELIVERED | `engine/crates/membrane-runtime/src/serve.rs:2337-2344` | `engine/crates/membrane-client/tests/installed_lifecycle.rs`; `engine/crates/membrane-runtime/tests/residency_holders.rs` | COMPLETE |

## Focused verification

| Capability targets | Focused command | Direct test evidence | Result | Run identity/time |
|---|---|---|---|---|
| MEM-006 | node --test scripts/qualification/cases/mem-lifecycle-windows.test.mjs | `MEM_006` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `INSTALLED_ENGINE_OWNER` `run_installed_runtime` `runtime_from_installed_exe` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-17; mem-lifecycle-windows suite green, 0 fail (93 tests, 0 fail total) |
| MEM-008 | node --test scripts/qualification/cases/mem-lifecycle-windows.test.mjs | `MEM_008` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `coderight_daemon` `controller_active` `residency` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-17; mem-lifecycle-windows suite green, 0 fail (93 tests, 0 fail total) |
| MEM-009 | node --test scripts/qualification/cases/mem-lifecycle-windows.test.mjs | `MEM_009` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `admit_request` `HttpDenialCode` `mcp_http` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-17; mem-lifecycle-windows suite green, 0 fail (93 tests, 0 fail total) |
| MEM-010 | node --test scripts/qualification/cases/mem-lifecycle-windows.test.mjs | `MEM_010` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `initialize` `McpServer::dispatch` `discovery` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-17; mem-lifecycle-windows suite green, 0 fail (93 tests, 0 fail total) |
| MEM-012 | rightkit cargo test --manifest-path engine/Cargo.toml -p membrane-mcp --locked | `public_calls_fail_with_typed_envelopes` plus host-boundary parity tests prove authenticated streamable-HTTP surface rejects unsafe origin/host/token requests with typed envelopes | FOCUSED_PASS — 0 failures. | local rightkit-managed lane via pnpm test:mcp 2026-09-17; membrane-mcp suite 101 tests, 0 fail |
| MEM-016 | node --test scripts/qualification/cases/mem-lifecycle-windows.test.mjs | `MEM_016` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `health_response_with_workers` `ResidentServicesUnavailableV1` `request_drain` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-17; mem-lifecycle-windows suite green, 0 fail (93 tests, 0 fail total) |
| MEM-054 | node --test scripts/qualification/cases/mem-lifecycle-windows.test.mjs | `MEM_054` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `protocolVersion` `schemaVersion` `serviceId` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-17; mem-lifecycle-windows suite green, 0 fail (93 tests, 0 fail total) |
| MEM-055 | node --test scripts/qualification/cases/mem-lifecycle-windows.test.mjs | `MEM_055` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `initial_holder_grace_expired` `activate` `SupervisionGuard` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-17; mem-lifecycle-windows suite green, 0 fail (93 tests, 0 fail total) |
| MEM-056 | node --test scripts/qualification/cases/mem-lifecycle-windows.test.mjs | `MEM_056` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `final_holder_expired` `controller_active` `residents_required` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-17; mem-lifecycle-windows suite green, 0 fail (93 tests, 0 fail total) |

<!-- reconcile:end -->
