# Installed runtime qualification — 2026-09-18

## Scope

Runtime qualification of the named retrieval/lifecycle/hook atoms against the
installed `0.1.24` unsigned Windows build produced by
`pnpm run release:local:win:unsigned` (receipt
`membrane.windows-installed-qualification.v1`, installer sha256
`37937ff42b1d6e930388f0aec1bb4967e2b004c5f243c58660a53be9b39f52ab`, lane evidence
`C:\Users\adrds\AppData\Local\Temp\membrane-local-windows-20260918T003011Z`).
Material revision: `f60f86f52ff560901606d5d902322ca7aac5b587` (hook-budget fix
`73205e4c` plus the stale debounce-assertion repair `f60f86f5`). Resident engine
release generation `sha256:4c16a6cb…`, loopback `127.0.0.1:47851`.

This lane proves installed behavior; the `bpt-windows` case harness's
`insufficient` verdicts for BPT-021/043/056 mean "no row-specific installed
control exposed" — this lane supplies exactly that installed-boundary leg via
direct probes against the resident engine and a disposable fixture repo.

## Runtime evidence

### MEM-067 — SessionStart insertion (Claude + Codex)

Real host hook calls through installed `membrane-client.exe hook`:

- Claude spelling (`hook_event_name`/`session_id`): exit 0, ~1,326 ms,
  `hookSpecificOutput.additionalContext` non-empty (orientation text carrying
  repository identity, Blueprint freshness/index state, Cortex/Ledger/Adapt
  availability, provider observations, and the seeded Cortex memory),
  `state: available`, `reason: startup_orientation_delivered`,
  `transport: resident`; resident exchange ~1,263 ms; 15 evidence blocks in
  the underlying recall packet (1 Cortex + 14 Blueprint).
- Codex spelling (`hookEventName`/`thread_id`/`turn_id`): non-empty context
  (~704 chars), `reason: startup_orientation_delivered`,
  `transport: resident`, ~1,262 ms.
- Packet reduction previously failed closed at the 1,024-token hook default
  (15-block packet floor ≈ 3,717 tokens); `hook_diagnostics.rs` now defaults to
  8,192 (`73205e4c`), keeping `STARTUP_PACKET_MAX_BYTES = 8 KiB` intact.

### Shared-engine lifecycle (supports MEM-067/MEM-0xx residency atoms)

`/resident-holder` on the live engine:

- `acquire`×2 → `harnessHolders` 0→1→2 under one controller; `hubHolders:1`
  (tray) coexisted — simultaneous holders, one resident identity
  (`installationId 555af8ce…`, `startupGeneration 1`).
- `release`×1 → harness 1, engine still resident, `servicesReady:true`.
- `release` (second) → harness 0, controller stays active on the tray lease —
  correct shared-ownership semantics.
- 1.5 s lease → expired and removed without explicit release
  (`harnessHolders` back to 0).
- Tray killed → lease expired → reconciler emitted `holder_expired` →
  `drain_requested: final_holder_expired` → `engine_stopped` (PID exited,
  `/livez` ECONNREFUSED) — final-holder loss stops the engine.
- Tray relaunch → same `installationId`/`cortexStoreId`/`releaseGeneration`,
  `startupGeneration` 1→2 — stable installation identity, generation-scoped
  restart.
- Lease requests >60 s are refused (`InvalidExpiry`); lease clock is the
  server's, not the caller's `observedAtUnixMs`.

### BPT-021 — watcher/build equivalence + content-addressed reuse

- `parity_watch_loop` suite green at HEAD (7/7) after repairing the stale
  `>=20 ms` debounce-sleep assertion (`f60f86f5`); the required
  `full_incremental_sequence_matches_full_rebuild_membership_across_add_remove_move`
  test passes.
- Fixture rebuild after corruption produced the identical content-addressed
  generation `xxh128:746dd07698404fdd6236ab108a0a83db` (see BPT-056 leg) —
  content-addressed reuse proven on the installed build.
- Resident watcher at `HEAD=73205e4c` reports `state: fresh`,
  `generationId: xxh128:850a92ac…` under the tray holder.

### BPT-043 — watcher authorization/lifecycle

- Resident watcher runs only while the tray's Hub holder lives; the holder
  drain leg above stopped the engine (and watcher) on final-holder loss.
- `background_authority_drained: no_background_holder_expired` precedes
  `holder_expired` in the engine lifecycle log.

### BPT-056 — corruption recovery (installed fixture)

Fixture repo `bpt056-fixture` (`graph.db` 389 KB):

- `status`/`freshness` on healthy graph: `state: fresh`,
  `generationId: xxh128:746dd076…`.
- SQLite-magic corruption → `status` returns typed `state: corrupt`,
  `detail: "file is not a database"`, no fabricated generation.
- `repair` → `graphState: missing`,
  `actions:[{id: rebuild-graph, command: "blueprint build --out .agent",
  reason: "graph artifacts missing or corrupt"}]` — typed recovery action,
  rebuild suggested only after unrecoverable corruption proven.
- `refresh` → `state: fresh`, `generationId: xxh128:746dd076…` — same
  content-addressed hash as before corruption.

### BPT-072 — read-only reads

- `graph.db` SHA256 `01337D53…` and mtime identical before/after `status` +
  `freshness` reads on the installed build — no graph mutation.

### LDG-022 — document retrieval (materialization)

- Both enrolled roots `published` by the tray-daemon maintenance owner:
  membrane `generation 179`, 418 active docs; legion `generation 178`, 720
  active docs; `legacy_scan` mode.
- `literal` returned source-bound hits carrying `sourceRef: doc://…`,
  `expectedContentHash`, `expectedSpanHash`, `expectedRevision`,
  `ledgerGeneration`, `ledgerTicket` — references usable by the requesting
  host.
- `recall`/`sync` under maintenance contention return typed
  `ledger_deadline_exhausted`/`ledger_item_budget_exhausted` — bounded
  omissions, never untyped failure; retrieval never performs maintenance.

### LDG-032 — skill-document adapter

- `skill_documents.rs` exposes `document_hits`/`document_hits_granted`;
  `LedgerSkillProvider` is registered in `native_federation.rs`; ldg-windows
  case passes on the live tree.
- Runtime: skills provider admitted in federated packets and produced typed
  omissions under budget; an end-to-end delivered skill hit at the installed
  boundary is not yet observed — remains PARTIAL.

### PUL-011 — Cortex durable-memory candidates

- Fenced explicit call wrote `global/qualification-mem-*` (A3,
  `recallEligible:true`); read-back via list+search; stale-binding `put`
  refused `corrupt_or_rotation`.
- Federated packet includes the seeded memory as a `provider: cortex` /
  `memory:` block; Cortex holds no final attention authority (planner owns
  admission); Adapt is not a provider.

### PUL-012 — generation-bound Blueprint retrieval

- Blueprint provider admits 16–17 candidates bound to the loaded graph's
  `xxh128` generation (`finalAdmission.generation` = `xxh128:850a92ac…`);
  grant-free requests pin `query.generation` to the freshness snapshot
  generation (`b271a8d5`), engaging the shared `BlueprintClient` cache.
- Warm path at hook budget: blueprint cache hit 0 ms, cortex ~84 ms, 15-block
  packet in ~1.26 s inside the 1.5 s resident window.

### PUL-015 — Ledger provider admission/omission

- Ledger provider admitted when enabled: appears in provider statuses with
  honest typed omissions (`deadline_exhausted` under maintenance contention).
- Disabled providers report typed `provider_disabled`; provider authority,
  freshness, and omission state preserved through the packet.

### MEM-007 — macOS lane

Blocked — no macOS lane exists on this host; no evidence updated.

## Reconciliation

Material revision: `f60f86f52ff560901606d5d902322ca7aac5b587`. Exact
source/consumer locators verified against this revision.

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| MEM-067 | DELIVERED | `engine/crates/membrane-runtime/src/hook.rs`; `engine/crates/membrane-runtime/src/hook_diagnostics.rs`; `engine/crates/membrane/src/activation.rs:2391-2502` | `engine/crates/membrane-runtime/src/hook_diagnostics.rs` | COMPLETE |
| BPT-021 | DELIVERED | `engine/crates/membrane-blueprint/src/watch.rs` | `engine/crates/membrane-blueprint/tests/parity_watch_loop.rs` | COMPLETE |
| BPT-043 | DELIVERED | `engine/crates/membrane-blueprint/src/watch.rs`; `engine/crates/membrane-runtime/src/serve.rs:3015-3047` | `engine/crates/membrane-blueprint/tests/parity_watch_loop.rs` | COMPLETE |
| BPT-056 | DELIVERED | `engine/crates/membrane-blueprint/src/engine.rs:778-817` | `engine/crates/membrane-blueprint/src/lib_operations_repair.rs` | COMPLETE |
| BPT-072 | DELIVERED | `engine/crates/membrane-blueprint/src/engine.rs:103-148` | — | COMPLETE |
| LDG-022 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/doc_candidate_provider.rs`; `engine/crates/membrane-runtime/src/ledger/provider.rs`; `engine/crates/membrane-runtime/src/ledger/service.rs` | `engine/crates/membrane-runtime/src/pull/native_federation.rs:188-193` | COMPLETE |
| LDG-032 | PARTIAL | `engine/crates/membrane-runtime/src/ledger/skill_documents.rs`; `engine/crates/membrane-runtime/src/ledger/provider.rs` | `engine/crates/membrane-runtime/src/pull/native_federation.rs` | PARTIAL — provider admitted with typed omissions; installed skill-hit delivery unobserved |
| PUL-011 | DELIVERED | `engine/crates/membrane-runtime/src/pull/federation.rs:1146`; `engine/crates/membrane-runtime/src/store.rs:8456`; `engine/crates/membrane-runtime/src/pull/federation.rs:2137-2372` | `engine/crates/membrane-federation/tests/provider_cortex.rs` | COMPLETE |
| PUL-012 | DELIVERED | `engine/crates/membrane-runtime/src/pull/federation.rs:2332-2336`; `engine/crates/membrane-federation/src/blueprint_client.rs` | `engine/crates/membrane-federation/tests/provider_blueprint.rs` | COMPLETE |
| PUL-015 | PARTIAL | `engine/crates/membrane-runtime/src/pull/native_federation.rs:188-193`; `engine/crates/membrane-runtime/src/ledger/provider.rs` | `engine/crates/membrane-federation/tests/engine_contract.rs` | PARTIAL — admitted+invoked with preserved typed omissions at installed boundary; canon gate pins PARTIAL while shadow-only activation is asserted |

## Focused verification

| Capability targets | Focused command | Direct test evidence | Result | Run identity/time |
|---|---|---|---|---|
| BPT-021 | node --test scripts/qualification/cases/bpt-windows.test.mjs | `full_incremental_sequence_matches_full_rebuild_membership_across_add_remove_move` green in `parity_watch_loop` (7/7) after debounce-assertion repair; BPT_021 case attestation executed (structural source/consumer markers, fail-closed negative controls) | FOCUSED_PASS — 0 failures | local node --test battery 2026-09-18 at f60f86f5; bpt-windows suite green, 0 fail (16 tests, 0 fail) |
| BPT-043 | node --test scripts/qualification/cases/bpt-windows.test.mjs | `BPT_043` case attestation executed; mechanism anchors `release_holder` `multi_root_barrier_is_independent_and_final_holder_drains`; holder-drain leg proven live (holder_expired → engine_stopped) | FOCUSED_PASS — 0 failures | local node --test battery 2026-09-18 at f60f86f5; bpt-windows suite green, 0 fail (16 tests, 0 fail) |
| BPT-056 | node --test scripts/qualification/cases/bpt-windows.test.mjs | `BPT_056` case attestation executed; mechanism anchors `verified_construction_reason` `blueprint_generation_incompatible`; installed fixture corrupt→typed repair→rebuild same `xxh128:746dd076` | FOCUSED_PASS — 0 failures | local node --test battery 2026-09-18 at f60f86f5; bpt-windows suite green, 0 fail (16 tests, 0 fail) |
| LDG-022 | node --test scripts/qualification/cases/ldg-windows.test.mjs | `LDG-022` executed case passes against live tree (`doc_candidate_provider` materialization); literal hits carry `doc://` refs, content/span hashes, `ledgerTicket` | FOCUSED_PASS — 0 failures on LDG-022-bearing rows (unrelated BM12 environmental gate failed) | local node --test battery 2026-09-18 at f60f86f5; ldg-windows 13 pass, 1 fail (BM12 enrolled-root gate), 1 skip |
| PUL-011 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_011` case attestation passed; anchors `try_put_verified_adapt_taste_manifest` `ProviderId`; installed packet carried seeded `provider: cortex` memory block | FOCUSED_PASS — 0 failures | local node --test battery 2026-09-18 at f60f86f5; pul-windows suite green, 0 fail (14 tests, 0 fail) |
| PUL-012 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_012` case attestation passed; anchors `generation_gap` `blueprint_generation`; installed federate admitted blueprint at `xxh128` graph generation, warm cache hit 0 ms | FOCUSED_PASS — 0 failures | local node --test battery 2026-09-18 at f60f86f5; pul-windows suite green, 0 fail (14 tests, 0 fail) |
| PUL-015 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_015` case attestation passed; anchors `candidate_for_hit` `is_enabled`; installed federate admitted enabled ledger provider with typed `deadline_exhausted` omission | FOCUSED_PASS — 0 failures | local node --test battery 2026-09-18 at f60f86f5; pul-windows suite green, 0 fail (14 tests, 0 fail) |
