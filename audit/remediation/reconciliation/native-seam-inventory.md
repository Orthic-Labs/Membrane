# Native seam inventory — Blueprint port

## status

`complete` (read-only inventory; no product source changed)

## summary

Current production path still treats Blueprint as an interpreter-backed child. `membrane-blueprint::Supervisor` resolves `runtime/blueprint/lib/node[.exe]`, launches `blueprint.mjs service run`, passes a named-pipe endpoint plus launch-token environment, then owns child drain. Explicit operations use a second Node child (`blueprint-one-shot.mjs`); CLI uses `blueprint.mjs`. Native Rust already owns typed request/response validation, MCP framing, federation provider adaptation, Hub runtime, & tray lifecycle, but no native Blueprint graph/store/service owner exists in the inspected crates.

## current seams, contracts, consumers

| seam | current owner & exact evidence | contract/state | consumers & tests | exact later Rust proposal |
|---|---|---|---|---|
| Resident launch | `engine/crates/membrane-blueprint/src/lib.rs:104-136` requires Node, `blueprint.mjs`, `blueprint-watch.mjs`; `:257-310` `Supervisor::start` enrolls workspace, computes endpoint/token, launches `node ... service run`; `:369-375` stops child; `:496-503` kills tree. | `RuntimeLayout`, `ServiceStatus`, `LifecycleState`, `BLUEPRINT_DAEMON_ENDPOINT`, launch-token/parent-PID env; named pipe `\\.\pipe\membrane-blueprint-{sha16}` on Windows (`:153-163`). | Tray starts it only for installed runtime at `apps/membrane-tray-windows/src/main.rs:149-174`; lifecycle tests in `engine/crates/membrane-blueprint/src/lib.rs:526-602`. | Replace wrapper internals with native Blueprint owner in `engine/crates/membrane-blueprint/src/lib.rs` plus new `engine/crates/membrane-blueprint/src/service.rs`; keep endpoint/typed status only if process isolation remains justified. Remove Node layout fields/launch env after native service is proven. |
| Explicit CLI | `engine/crates/membrane-runtime/src/blueprint_one_shot.rs:106-135` launches packaged Node + `blueprint.mjs`; bounded transport `:139-202` launches Node + `blueprint-one-shot.mjs`, JSONL stdin/stdout, cancellation/deadline, 64KiB request/response caps. | `BlueprintWireRequest/Response`, `BlueprintBounds`, `BlueprintClientError`; request has protocol/request/repo/generation/method/deadline/input (`engine/crates/membrane-federation/src/blueprint_client.rs:24-55,126-183`). | `membrane-runtime/src/mcp_executor.rs:647-730` uses `ExplicitBlueprintTransport` for `build` or fallback when resident endpoint unavailable. Explicit transport tests `blueprint_one_shot.rs:40-69`. | Port CLI handlers into `engine/crates/membrane-blueprint/src/cli.rs` (or canonical `engine/crates/membrane/src/cli.rs` dispatch) & make `engine/crates/membrane-runtime/src/blueprint_one_shot.rs` call native bounded owner directly. Preserve one-shot/no-residency, deadline, cancellation, caps, generation checks.
| MCP | Native framing is already `engine/crates/membrane-mcp/src/jsonrpc.rs:4-12,105-120`; one process-wide typed owner is `engine/crates/membrane-mcp/src/tools.rs:333-344`. Blueprint is tool `membrane_blueprint` (`tools.rs:11,132,318-321`). | Operation envelope/schema/error versions are resolved by `tools.rs`; runtime maps Blueprint failures at `engine/crates/membrane-runtime/src/mcp_executor.rs:167-188`. `execute_blueprint` validates caller repository/method/generation/budget (`:647-730`). | `HubTransportExecutor` & `ExplicitOperationExecutor` both route Blueprint (`mcp_executor.rs:599-615`); MCP tests `engine/crates/membrane-mcp/tests/discovery_roundtrip.rs`, `prompts.rs`, `resources.rs`; runtime contract tests under `engine/crates/membrane-runtime/tests/` include `native_authorization.rs`, `request_time_h8.rs`. | Keep `membrane-mcp` framing/tool registry unchanged; replace `execute_blueprint` backend in `engine/crates/membrane-runtime/src/mcp_executor.rs` with native Blueprint service/client. Add native MCP operation tests beside existing discovery/authorization tests; do not add legacy Node bootstrap/fallback.
| Federation/provider | Consumer seam explicitly forbids process/storage ownership (`engine/crates/membrane-federation/src/blueprint_client.rs:1-7`). `BlueprintTransport` exchanges typed request, bounds, deadline, cancellation (`:158-166`); client caches by repository/worktree/query/symbol/anchors (`:59-95,200-224`). | `BlueprintWireRequest/Response`, protocol v1, typed errors unavailable/timeout/cancelled/malformed/oversized/stale/remote (`:126-195`); response parser enforces request/protocol/generation/candidate/path bounds (`:658-744`). | `engine/crates/membrane-federation/src/providers/blueprint.rs:1-15,239-258` consumes `ContextualBlueprintSource`; federation tests `tests/provider_blueprint.rs`, `tests/provider_anchors.rs`, `tests/fence_abstention.rs`, `tests/engine_contract.rs`. | Preserve `BlueprintClient`, `BlueprintSource`, `ContextualBlueprintSource`, error codes & receipts. Add native in-process transport/owner in `engine/crates/membrane-blueprint` or `membrane-runtime`; change only construction/wiring in `engine/crates/membrane-federation/src/blueprint_client.rs` / runtime provider setup. Delete Unix/named-pipe transport only after a measured isolation decision; otherwise retain as native-to-native IPC, never Node-specific.
| Hub holder/runtime | Hub `Supervisor` owns one in-process runtime thread (`apps/membrane-hub/src-tauri/src/supervisor.rs:51-104`), calls `run_hub_runtime`; `stop` drains it (`:155-160`). `engine/crates/membrane-runtime/src/service.rs:13-23,38-56` carries ephemeral lifecycle capability, admission, shutdown, readiness & failure. | Hub runtime is lifecycle authority; Blueprint currently is an independent tray-launched child, not part of `run_hub_runtime`. | Hub production source deliberately has no runtime spawn (`apps/membrane-hub/src-tauri/src/main.rs:166-194,281-285`); Hub supervisor tests `:241-306`; tray daemon supervisor launches native daemon `apps/membrane-tray-windows/src/supervisor.rs:621-704`. | Add Blueprint native owner to shared controller/runtime lifecycle (`engine/crates/membrane-runtime/src/service.rs` or a new `blueprint_service.rs`), with Hub/CodeRight holder reference counting, readiness, drain & identity binding. Remove tray-only sidecar launch from `apps/membrane-tray-windows/src/main.rs:149-174`; update Hub supervisor only after shared-holder contract is implemented. Explicit one-shot remains independent of holder.
| Native CLI mode | Product dispatcher has `Cli` & `StdioMcp`, with no resident mode (`engine/crates/membrane/src/dispatch.rs:41-56`); wrapper delegates CLI to runtime (`engine/crates/membrane/src/cli.rs:1-18`). | Generated operation registry is compile-bound (`dispatch.rs:11-39`); `stdio-mcp` is the supported MCP client command. | `engine/crates/membrane/tests/dispatch.rs`, `identifier_contracts.rs`, `activation_hub_off.rs`; MCP clients bind `membrane.exe stdio-mcp`. | Register Blueprint operations in existing native CLI/runtime operation registry, not a second executable or Node wrapper. Update `engine/crates/membrane-runtime/src/cli.rs`, `engine/crates/membrane/src/dispatch.rs`, generated registry source only through its prescribed generator lane (outside this inventory).

## native target contract

Preserve protocol v1 wire shape, request identity, repository scope binding, expected generation, candidate/path/response caps, absolute ingress deadline, cancellation, typed omissions/errors, & no storage access by federation/MCP. Native Blueprint owns parsing, graph semantics, SQLite generations/WAL/transactional publication, freshness/watch reconciliation, symbol resolution, Phase 2 operations, & service response shape. No consumer may open Blueprint SQLite or reimplement graph semantics.

## baseline & acceptance

- Baseline revision: `2e8e57bbb1e368332b37466080f1e925f2f4726e`.
- Current baseline is interpreter-backed: Node launch exists in `membrane-blueprint` & `membrane-runtime`; tray invokes resident launch; MCP/federation consume typed compatibility seam.
- Closure acceptance from `D:/Downloads/MEMBRANE-BLUEPRINT-NATIVE-RUST-CLOSURE.md:5-18,75-108,469-520,781-837,1046-1052`: native Rust Blueprint; no separately spawned Blueprint Node service; no bundled Node executable; native CLI/MCP/federation; Hub/CodeRight lifecycle; no installed Node/Python backend child.
- This lane ran no Cargo/tests/builds/generators/installs/watchers, per packet policy. Integration owner must run parity, no-interpreter installed qualification, process-tree, MCP, federation, lifecycle & package/SBOM gates.

## artifacts & changes

- Produced: `audit/remediation/reconciliation/native-seam-inventory.md`.
- Changed path: only that file, created once via `apply_patch`.
- No product files, generated files, tests, commits, pushes, merges, installs, or runtime state changed.

## commands

- Receipt validation: SHA-256 packet/artifact comparison via PowerShell `Get-FileHash`; all referenced hashes matched.
- Read-only inventory: `Get-Content`, `rg`, `git rev-parse HEAD`.

## recovery / deviations / blocker

- Recovery: none required.
- Deviations: none; a dedicated repository validator was not present in `tools`; supplied receipt hashes were validated directly.
- Blocker: none for this read-only lane.

## next

Integration owner should consume exact file proposals above, then freeze native Blueprint core/service/CLI/MCP/federation/lifecycle packets before source edits. Keep current Node paths as deletion targets, not compatibility fallbacks.

## citations

- `D:/Downloads/MEMBRANE-BLUEPRINT-NATIVE-RUST-CLOSURE.md:5-18,75-108,469-520,781-837,1046-1052`
- `engine/Cargo.toml:1-20`
- `engine/crates/membrane-blueprint/src/lib.rs:1-13,104-163,251-375,496-503,526-602`
- `engine/crates/membrane-runtime/src/blueprint_one_shot.rs:1-6,71-202`
- `engine/crates/membrane-runtime/src/mcp_executor.rs:167-188,336-354,599-730`
- `engine/crates/membrane-federation/src/blueprint_client.rs:1-7,24-55,59-95,126-195,212-224,313-389,455-590,658-744,790-924`
- `engine/crates/membrane-federation/src/providers/blueprint.rs:1-15,22-56,239-278`
- `engine/crates/membrane-mcp/src/jsonrpc.rs:4-12,105-120`
- `engine/crates/membrane-mcp/src/tools.rs:11,132,318-344`
- `engine/crates/membrane/src/dispatch.rs:11-56,181-198,209-228`
- `engine/crates/membrane/src/cli.rs:1-18`
- `apps/membrane-tray-windows/src/main.rs:139-174,908-914`
- `apps/membrane-tray-windows/src/supervisor.rs:621-704`
- `apps/membrane-hub/src-tauri/src/supervisor.rs:51-104,155-160`
- `apps/membrane-hub/src-tauri/src/main.rs:166-194,281-285`

