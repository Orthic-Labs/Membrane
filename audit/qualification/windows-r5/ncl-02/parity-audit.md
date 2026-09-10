# Blueprint legacy-JS vs native-Rust parity audit (AUDIT1, read-only)

Full detail: `parity-audit.json` (52 rows: 16 `service.mjs` public operations expanded into their
sub-behaviors, CLI/launcher surface, and the legacy graph algorithm modules named in the audit brief).

## Counts by classification

| Classification | Count |
|---|---|
| EXECUTED-NATIVELY | 21 |
| PORTED-NOT-WIRED | 18 |
| MISSING | 13 |
| **Total rows** | **52** |

## Method

Traced the shipped production entrypoint for every `service.mjs` operation and CLI verb:
`engine/crates/membrane-runtime/src/mcp_executor.rs:execute_blueprint` (the MCP `membrane_blueprint`
tool handler) and `engine/crates/membrane-runtime/src/blueprint_one_shot.rs:run_cli` both call
`membrane_blueprint::native_blueprint_operation()` -> `engine.rs::NativeBlueprintOperation::execute`,
which is the single dispatch point. Anything not reached from that `match request.method` block
(or from `query.rs::execute_query`'s inner match for Search/Resolve/Recall/Expand/Impact/Path/
Architecture) is not production-executed, regardless of whether a same-named `.rs` module exists
and is declared `pub mod` in `lib.rs`.

## Real gaps blocking deletion of `blueprint/` (PORTED-NOT-WIRED or MISSING **with** a shipped caller)

These are legacy behaviors that something outside the dead legacy CLI verbs still reaches (i.e. not
dead-in-legacy) and that native does not yet serve from its production dispatch path:

### PORTED-NOT-WIRED (native module exists, compiled, `pub mod`-exported, but no production caller)
- **`architecture({view:'liveness'})`** — `liveness.rs` exists, unreferenced by `engine.rs`/`query.rs`.
- **`architecture({view:'contracts'})`** — `contract_registry.rs` exists, unreferenced.
- **`architecture({view:'orientation'})`** — `lib_orientation_evidence.rs` exists, unreferenced.
- **`architecture({view:'projection'})`** — `architecture_model.rs` exists, unreferenced.
- **`architecture({view:'changes'})`** — the standalone `Operation::Changes` handler exists, but
  native `Operation::Architecture` has no `view` switch, so this specific composed view is unreached.
- **`federate`** — `lib_application_federate.rs::execute_federate` is fully implemented and wired
  into `engine.rs`'s `Operation::Federate` arm, but `mcp_executor.rs::execute_blueprint`'s operation
  match has **no `"federate"` case** (falls through to `blueprint_envelope_invalid`), and no shipped
  CLI verb in `blueprint_one_shot.rs`'s verb table calls `membrane_blueprint::cli::federate`. Dead
  end on both shipped entrypoints — see dead-in-legacy note below.
- **CLI `init` / `update` / `doctor` / `service` (repair)** — `Operation::Init/Update/Doctor/Repair`
  are declared in `model.rs`'s `Operation` enum, and their corresponding `lib_init_*`/`lib_update_*`/
  `lib_operations_doctor.rs`/`lib_operations_repair.rs` modules are compiled, but `engine.rs`'s
  dispatch `match` has **no arm** for any of these four — they fall into the catch-all
  `unsupported_operation` error. Shipped caller: `blueprint/package.json#bin.blueprint` (`blueprint
  init|update|doctor|service`), and `docs/agent-rules.md` in `blueprint/` directs agents to run
  `blueprint doctor --full --json`.
- **`blueprint-watch` resident loop** — `watch.rs` is compiled and exported from `lib.rs`, but this
  pass did not confirm a resident-host (membrane-hub / membrane-tray-windows) production caller in
  the time available. Needs a dedicated follow-up trace before treating native watch as proven.
- **`dependency_dag.rs` / `store_delta.rs`→`delta_store.rs` / `static_provider.rs` /
  `framework_intelligence.rs` / `architecture_model.rs` / `git_source_observation.rs`** — all
  compiled `pub mod`s with zero callers found in `engine.rs`, `query.rs`, `service.rs`, `cli.rs`, or
  `watch.rs`. `static_provider.rs` in particular backs legacy `status()`/`search()`/
  `architecture({view:'flows'})`; native re-implemented those two operations independently in
  `engine.rs`/`query.rs` rather than porting this module, so the module itself is orphaned even
  though its *responsibilities* are otherwise covered.

### MISSING (no native equivalent found at all)
- **`resolve()`'s `reanchorEvidence` fallback** (`blueprint/src/graph/reanchor.mjs`) — used when a
  direct node-id lookup fails and `previousEvidence` is supplied.
- **`impact()`'s `decomposeChangeRisk`** (`graph/analytics/change-impact.mjs`, `analytics/index.mjs`)
  — native `Operation::Impact` returns `ImpactFrontierClass` edge classification only, no risk score.
- **`impact()`'s `recommendTestsForImpact`** (`graph/test-recommendation.mjs`) — no
  `testRecommendations` field in the native impact response.
- **`architecture({view:'flows'})`** (`static-provider.mjs` `graphFlowInventory` /
  `architectureFlowPage` cursor pagination) — no native flows view at all (distinct from the
  PORTED-NOT-WIRED liveness/contracts/orientation/projection views above, which at least have a
  native module; flows has none).
- **`architecture({view:'processes'})`** (`graph/process-projection.mjs`) and
  **`architecture({view:'signatures'})`** (`graph/signature-projection.mjs`) — no native module.
- **`graph/providers/*`** (compilers, frameworks, iac, manifests, plugin-loader, ranking,
  `semantic-orchestrator.mjs` cross-check-with-live-verifier) — no corresponding native module tree;
  native graph construction (`graph.rs`, `ast_walker.rs`, `module_resolution.rs`) is an independent
  reimplementation, not a port, and does not offer live-verifier cross-check at all.
- **CLI `docs` / `explore` / `uninstall` / `languages` / `rules` / `mcp` (sub-verb)** — no
  `Operation` variant or native module traced to these; only spot-checked given the audit time
  budget, flag for a dedicated follow-up pass before deleting `blueprint/scripts/cli/commands.mjs`.
- **`graph/resolution/` directory and `providers/modules/python-resolver.mjs`** — enumerated by
  directory listing only; no native-equivalent trace completed in this pass.

## Dead-in-legacy (no shipped caller anywhere — excluded from native porting scope)
- **`federate` op's own shipped surface**: legacy `service.mjs federate()` is only reachable through
  `blueprint/src/sdk/embedded.mjs`, which itself has no confirmed shipped caller outside
  `blueprint/` (checked `apps/`, top-level `scripts/`, `mcp/`, `.github/`, `docs/product`). The
  legacy JS MCP server (`blueprint-mcp.mjs`) does not register a federate tool either. So while the
  *native* federate implementation is unreachable (a real wiring gap, listed above), the *legacy*
  federate path is close to unreachable too — low regression risk, but not zero, since
  `blueprint/package.json#bin` still exposes it indirectly via any script that imports
  `embedded.mjs` directly.
- **`blueprint-install.mjs`** — no shipped caller found outside `blueprint/`; per this repo's locked
  invariants, CodeRight/Hub use a canonical external installer for the native Membrane controller,
  not this legacy script. Safe to drop without a native port.
- **`release/launchers/*`** (`blueprint`, `blueprint-mcp`, `blueprint.cmd`, `blueprint-mcp.cmd`) —
  confirmed thin `exec` wrappers around `scripts/blueprint.mjs` / `scripts/blueprint-mcp.mjs`; add no
  surface beyond what's already classified above.

## Caveats / follow-up needed before deleting `blueprint/`
- CLI admin verbs (`init`, `update`, `doctor`, `service`/repair, `docs`, `explore`, `uninstall`,
  `languages`, `rules`, `mcp` sub-verb) were traced at the dispatch-arm level (confirmed missing from
  `engine.rs`'s `Operation` match) but not chased module-by-module to the same depth as the query
  operations, given the audit time budget. These are real, confirmed gaps (Init/Update/Doctor/Repair
  literally hit `unsupported_operation` today) but their scope needs a dedicated pass.
- `blueprint-watch.mjs` vs native `watch.rs` residency wiring was not fully traced into
  membrane-hub/membrane-tray-windows.
