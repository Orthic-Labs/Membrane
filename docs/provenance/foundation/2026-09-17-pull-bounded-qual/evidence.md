# Installed bounded-pull + capability qualification — 2026-09-17

## Scope

Direct installed-surface qualification of the `0.1.24` unsigned build produced by
`pnpm run release:local:win:unsigned` (receipt `membrane.windows-installed-qualification.v1`,
installer sha256 `6dffefa5d0f3123b89b1c075797c8d9389d9cd1df03f7e4e5d88f4c4f8226856`,
lane evidence `C:\Users\adrds\AppData\Local\Temp\membrane-local-windows-20260917T025452Z`).
This build contains the bounded-response empty-packet fix in
`engine/crates/membrane-runtime/src/pull/federation.rs`.

Probe: `installed-capability-qual.mjs` drives the installed payload at
`C:\Users\adrds\AppData\Local\Orthic Labs\Membrane\current` via
`membrane-client.exe hook` and `membrane-client.exe stdio-mcp`. Output:
`probe-output.log` — 10/10 checks pass.

## Defect repaired and verified

Bounded-response Pull (public `pull` without a request-time H8) previously
returned HTTP-200 with an empty packet and no `status`, so the executor
deserialized a missing `packetReduction` and failed closed with
`context_selection_invalid`. PUL-033 requires the typed
`insufficient_confidence` envelope. `run_federate_value` now mirrors the
resident host-fit empty-packet path: `status: "insufficient_confidence"` plus
`emptyEvidenceSummary` carrying candidate count and the typed omission reasons
(id:reason(detail@stage), truncated at 16 with an `elided` count — the wire
omission shape loses detail_id/stage).

Installed verification: pre-fix build returned `context_selection_invalid`;
post-fix build returns `kind: success`, `status: insufficient_confidence`,
`insufficientConfidence` extension with `searched` lanes and
`suggestedAction`, and — once evidence exists — a real packet.

## Checks (all PASS)

- `sessionstart_hook_transport` — installed `membrane-client.exe hook`
  forwards HookHost JSON to the resident `/hook` route and returns structured
  `hookSpecificOutput` for `SessionStart`.
- `sessionstart_orientation_insertion` — structured insertion envelope
  delivered (`hookSpecificOutput.additionalContext` empty on this run; real
  orientation *content* insertion remains open evidence).
- `mcp_initialize_and_registry` — authenticated initialize + `tools/list`
  returns exactly `pull,push`.
- `push_admission` — `push` admitted under caller `scopeId: membrane`;
  `contentHash` equals the submitted body's sha256; `authority: A2`,
  `managedBy: cortex`, `recordPresent: true`.
- `push_admission_fs_scope` — `push` under the registry-bound
  `scopeDescriptor {kind: filesystem, path: membrane}` admitted; memory lands
  in scope `D--Claude-membrane` (path-derived from `caller.root`).
- `byte_exact_recall` — `membrane_memory_read` with `expectedContentHash`
  returns the body; recomputed sha256 matches the pushed bytes exactly
  (UTF-8 multi-byte + tabs + trailing spaces preserved).
- `memory_recall_finds_push` — `membrane_memory` `recall` (recipe
  `cortex.hybrid@1`) returns the pushed item with `completeness.state: exact`.
- `pull_no_selection_invalid` — pull never emits `context_selection_invalid`.
- `pull_typed_envelope` — `kind: success` with `status: ok` (packet) or
  `insufficient_confidence` (typed abstention).
- `pull_semantic_retrieval` — pull's cortex lane returns the filesystem-scoped
  pushed memory as a packet block (`provider: cortex`, `sourceKind: memory`,
  `resolver: membrane_memory_read:…`, `sourceHash` = body sha256).

## Scope-semantics note (verified, not a defect)

- `push` write scope = `caller.scopeDescriptor.kind == "filesystem"` →
  `path_to_scope(caller.root)`; otherwise `caller.scopeId` verbatim.
- `membrane_memory recall` searches `caller.scopeId` verbatim.
- Pull's cortex lane resolves `ScopeDescriptorV1::filesystem(repository_root)`
  → `D--Claude-membrane` chain. A memory pushed under bare scopeId `membrane`
  is therefore recallable via `membrane_memory` but invisible to pull — typed
  insufficiency, fail-closed, no sibling-scope leak.
- Authorization binds caller identity `(repositoryId, scopeId)` plus an exact
  scope descriptor from the installation registry; a mismatched descriptor is
  `caller_scope_binding_denied`.

## Provider topology note (verified)

Public `pull` bounded-response runs `NativeFederation::hook` — Cortex + Skills
lanes only; the remaining seven lanes are recorded as typed omissions
(`provider_disabled`, `stage: configuration`). Full multi-lane federation runs
under host-fit mode with a validated request-time H8.
