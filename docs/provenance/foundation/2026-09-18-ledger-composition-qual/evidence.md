# Installed Ledger composition qualification — 2026-09-18

## Scope

Installed-boundary qualification of the Ledger FTS/activation/publication/
lifecycle atoms plus the LDG-032 skill-document Pull handoff, on the
`0.1.24` unsigned Windows build produced by
`pnpm run release:local:win:unsigned` (receipt
`membrane.windows-installed-qualification.v1`, installer sha256
`5976cbb405859b82f72ecc0eba7cd2393a5be6553b9bc296edbacbbb957991cc`, lane
evidence `C:\Users\adrds\AppData\Local\Temp\membrane-local-windows-20260918T122103Z`,
installed `unsigned-functional` pass). Material revision:
`782633a60c39e33b56db1c105e2a3838d12f8439` (skill-lane merge fix `ca698a7f`
plus resolver serialization fix `782633a6`). Resident engine release
generation `sha256:41b4c611…`, loopback `127.0.0.1:47851`.

This lane supersedes the LDG-032 note in
`docs/provenance/foundation/2026-09-18-runtime-qual/evidence.md`, which
recorded the skill provider as admitted with typed omissions but no
installed skill-hit delivery. The merge collision behind that gap is now
fixed and delivered at the installed boundary.

## Runtime evidence

### Native per-row installed qualification

Each atom ran `membrane.exe cli qualification ledger <id>` on the installed
binary, bound by `installedIdentity` to `sourceRevision 782633a6`,
`releaseGeneration sha256:41b4c611`, exe sha256 `1640a2b2…`,
`verified: true`, `runtimeOrigin: installed` — all `status: passed`:

- **LDG-011**: `authority: source`, `readback: true` — FTS hit bound to
  `ledger.doc:f2ed276b…` with source-authority readback.
- **LDG-012**: `refusal: ledger_fts_requires_qualification`,
  `delivery: blocked`, `modeUnchanged: true` — caller promotion refused;
  delivery blocked without the trusted receipt.
- **LDG-017**: `coherentGeneration: true`, 4 readers each observed one
  generation tuple (generations 2–5) — concurrent publication coherence,
  no mixed generation.
- **LDG-018**: three sync reports — initial 3 docs, delete →
  `tombstoned: 1`/`deleted: 1`, identical-byte recreate → restored to 3
  registered; `boundedUnchanged: true`.
- **LDG-019**: `fenceDenied: true`, `rebuiltVisibleHits: 1` — rebuild
  cannot resurrect an erased path; erasure durable across rebuild.
- **LDG-032**: `catalogEntries: 1` (`skillId: deploy`), `hitBound: true`,
  `grantCoveringDocument: true`, `grantNarrowedEmpty: true`,
  `partialGrantRefused: true`, `driftRefused: true`,
  `erasureRefused: true`, negative `no Cortex/store skill body is
  consulted or emitted`.

### Ledger FTS activation on the installed boundary

The resident owner reconciled activation at open to
`mode: ledger_fts` carrying pinned receipt `a796a687…` (corpus
`ledger-eval-v1`) — the `5de1c689` pin is live in the installed build.
Boundary legs against the enrolled `membrane-finish-smoke` root:

- `activate mode=legacy_scan` → executed, status `legacy_scan`
  (caller-driven rollback to the qualified-baseline lane works).
- `activate mode=ledger_fts` → typed refusal
  `ledger_owner_qualification_required` — promotion is receipt-bound and
  never caller-forgeable.
- Owner re-open (tray-daemon) → reconciled back to `ledger_fts` via the
  pinned receipt — activation follows the build's trusted qualification
  receipt, not the last caller-set row.

### Composition evaluation (supports LDG-011/LDG-012/LDG-017)

`ledger_service_composition_eval` run12, corpus `ledger-eval-v1`
(`be0421e5…`), all 8 hard gates green — FTS MRR 0.607 vs legacy-scan
0.051, recall@5 74.5% vs 8.5%, max latency 1922 ms,
`crossValidationMatch`, `erasureBinds`, `grantNarrowsOnly`,
`readCompletesUnderMaintenance`, `ticketsIssued`, `sourceBoundHits`.
Receipt `a796a687…` pinned in `TRUSTED_LEDGER_FTS_RECEIPTS` +
`QUALIFIED_FTS_ACTIVATION` (`5de1c689`).

### Read-under-maintenance isolation

`recall` through the installed boundary against a **held foreign writer
transaction** on the installed index: 1.86 s, 3 hits, `lane: ledger_fts`.
Published-index reads bypass the operation mutex on the dedicated read
connection; `status` latency is identical held/unheld (baseline
owner-state reads, not contention).

### LDG-032 — skill-document delivery through production Pull

Root cause of the prior lane's "emitted but never delivered" gap:
`candidate_for_skill` emitted `id: skills:{skill_id}` for every node hit
of one document — multi-hit docs produced several candidates sharing one
identity, which merge excluded as `candidate_identity_conflict`. Fix
`ca698a7f`: per-span identity `skills:{skill_id}:{node_id}` +
`kind_priority` ranks `skill` above `doc` so same-source dedup selects
the skill-typed candidate (planner's reserved `skill` lane becomes
reachable).

`/federate` on the installed build (task
"audit google ads budget pacing dayparting bid caps for an
under-delivering campaign", repo `membrane-finish-smoke`, 2.2 s):

- Packet carries **2 skill-typed blocks**:
  `skills:ads-pacing:ledger.node:aeec4607…` and
  `skills:ads-pacing:ledger.node:c949d53d…` — `provider: skills`,
  `deliveryStage: planned`, `dropReason: none`, snapshot `ledger:353`,
  `sourceRef: doc://…/tools/skills/ads-pacing/SKILL.md#ledger.node:*`,
  `overlayDigest` = doc content hash `633f2280…`, `layer: 7`,
  `instructionPolicy: data_only`.
- `providerQuotas: {blueprint:1, cortex:0, ledger:2, skills:2}`;
  `providerDiagnostics.skills: status complete, candidateCount 2,
  1736 ms, fallback none`; `ambiguityDisposition.action: allow`.
- Ledger lane's same-node doc-typed candidates deduplicated into the
  skill winners — honest `duplicate_source_hash` accounting, not
  identity conflict.

### Emitted resolver — verbatim replay

Delivered block resolvers emit `tool: membrane_source_read` with the
full source-bound handle (caller identity, `sourceRef` doc:// ref,
anchor/doc/node ids, `expectedContentHash`, `expectedSpanHash`,
`expectedRevision`, `ledgerGeneration`, `ledgerTicket`, `maxBytes`).

`782633a6` adds `skip_serializing_if` to optional `ResolveRequest`
fields — the installed build's emitted resolver contains **no null
fields** (previously `continuationCursor: null` failed MCP schema
validation on verbatim replay).

Replaying the emitted arguments **byte-for-byte** through `stdio-mcp`
`tools/call` `membrane_source_read`: `result.kind: success`, `ok: true`,
`contentSha256 633f2280…` == `expectedContentHash`, `ledgerGeneration
353`, span hash `5cbdcfcf…` == emitted `expectedSpanHash`,
`resolverVersion: ledger.exact-node.v1`, bounded section with neighbor
anchors — an exact, ticketed, grant-checked source read.

### Case-module repair

`nativeLedgerQualification` in `ldg-windows.mjs` (and the equivalent
`ctx-windows.mjs` arm) invoked `<cli> qualification …` without the `cli`
verb prefix every other call site uses; the installed engine only
reaches `qualification` through `cli` forwarding. Fixed to
`['cli', 'qualification', …]` — the native per-row probes above are the
repaired path.

### Skill adapter boundary ops (context)

`membrane_ledger` ops through `tools/call` on the resident engine:
`skillCatalog` → `[ads-pacing]` (gen 345); `skillDocuments` → 2 ticketed
`ledger_fts` hits in 1.94 s; `sync` `registered: 3` at gen 345→353.
Disposable fixture root `D:\Claude\.tmp\membrane-finish-smoke` (enrolled
`membrane-finish-smoke` scope) holds the
`tools/skills/ads-pacing/SKILL.md` portable-layout document — the only
enrolled root carrying the required layout.

## Reconciliation

Material revision: `782633a60c39e33b56db1c105e2a3838d12f8439`. Exact
source/consumer locators verified against this revision.

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| LDG-011 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/query.rs:188-197,375-393`; `engine/crates/membrane-runtime/src/ledger/index.rs` | `engine/crates/membrane-runtime/src/ledger/provider.rs:239-` ; `engine/crates/membrane-runtime/src/ledger/qualification_lifecycle.rs` | COMPLETE |
| LDG-012 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/service.rs:159-171,582-593`; `engine/crates/membrane-runtime/src/ledger/qualification.rs:43-60` | `engine/crates/membrane-runtime/src/cli.rs:5106-5128`; `engine/crates/membrane-runtime/src/ledger/qualification_lifecycle.rs` | COMPLETE |
| LDG-017 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/doc_spine.rs`; `engine/crates/membrane-runtime/src/ledger/resolve.rs:194-` | `engine/crates/membrane-runtime/src/ledger/provider.rs`; `engine/crates/membrane-runtime/src/ledger/qualification_lifecycle.rs` | COMPLETE |
| LDG-018 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/doc_spine.rs:985-993,1163-1183`; `engine/crates/membrane-runtime/src/ledger/reconcile.rs:6-42` | `engine/crates/membrane-runtime/src/ledger/qualification_lifecycle.rs`; `engine/crates/membrane-runtime/src/cli.rs` | COMPLETE |
| LDG-019 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/service.rs:831-863`; `engine/crates/membrane-runtime/src/ledger/erasure.rs` | `engine/crates/membrane-runtime/src/ledger/doc_spine.rs:989-993`; `engine/crates/membrane-runtime/src/ledger/qualification_lifecycle.rs` | COMPLETE |
| LDG-032 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/skill_documents.rs:215-290`; `engine/crates/membrane-runtime/src/ledger/provider.rs:212-238`; `engine/crates/membrane-runtime/src/ledger/resolve.rs:40-49` | `engine/crates/membrane-runtime/src/pull/native_federation.rs:180-184`; `engine/crates/cortex-core/src/planner.rs:757,1119-1127` | COMPLETE |
| PUL-010 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/provider.rs:212-238`; `engine/crates/membrane-runtime/src/pull/native_federation.rs:180-184` | `engine/crates/cortex-core/src/planner.rs:757`; `engine/crates/membrane-mcp/src/tools.rs` | COMPLETE |

## Focused verification

| Capability targets | Focused command | Direct test evidence | Result | Run identity/time |
|---|---|---|---|---|
| LDG-011 | node --test scripts/qualification/cases/ldg-windows.test.mjs | `LDG-011` native probe `authority:source`+`readback:true` on installed 782633a6; mechanism anchors `ledger_fts` `bm25` FTS5 weighted query | FOCUSED_PASS — 0 failures on LDG-011-bearing rows (unrelated BM12 enrolled-root gate failed) | local node --test battery 2026-09-18 at 782633a6; ldg-windows 13 pass, 1 fail (BM12 enrolled-root gate), 1 skip |
| LDG-012 | node --test scripts/qualification/cases/ldg-windows.test.mjs | `LDG-012` native probe `ledger_fts_requires_qualification`+`modeUnchanged`; boundary legs `qualified_fts_activation` reconcile + `activate` refusal | FOCUSED_PASS — 0 failures on LDG-012-bearing rows (unrelated BM12 enrolled-root gate failed) | local node --test battery 2026-09-18 at 782633a6; ldg-windows 13 pass, 1 fail (BM12 enrolled-root gate), 1 skip |
| LDG-017 | node --test scripts/qualification/cases/ldg-windows.test.mjs | `LDG-017` native probe `coherentGeneration` across 4 readers (generations 2–5); mechanism anchors `index_generation` publication tuple | FOCUSED_PASS — 0 failures on LDG-017-bearing rows (unrelated BM12 enrolled-root gate failed) | local node --test battery 2026-09-18 at 782633a6; ldg-windows 13 pass, 1 fail (BM12 enrolled-root gate), 1 skip |
| LDG-018 | node --test scripts/qualification/cases/ldg-windows.test.mjs | `LDG-018` native probe delete→`tombstoned`→identical-byte restore; mechanism anchors `markdown_absences` `lifecycle_state` | FOCUSED_PASS — 0 failures on LDG-018-bearing rows (unrelated BM12 enrolled-root gate failed) | local node --test battery 2026-09-18 at 782633a6; ldg-windows 13 pass, 1 fail (BM12 enrolled-root gate), 1 skip |
| LDG-019 | node --test scripts/qualification/cases/ldg-windows.test.mjs | `LDG-019` native probe `fenceDenied` no-resurrection across rebuild; anchors `erasure_fence_removes_skill_from_catalog_and_search` `record` | FOCUSED_PASS — 0 failures on LDG-019-bearing rows (unrelated BM12 enrolled-root gate failed) | local node --test battery 2026-09-18 at 782633a6; ldg-windows 13 pass, 1 fail (BM12 enrolled-root gate), 1 skip |
| LDG-032 | node --test scripts/qualification/cases/ldg-windows.test.mjs | `LDG-032` native probe (grant/drift/erasure refusals + bound hit); federate delivered 2 `skills:` blocks; anchors `erasure_fence_removes_skill_from_catalog_and_search` `skill_id_from_path` | FOCUSED_PASS — 0 failures on LDG-032-bearing rows (unrelated BM12 enrolled-root gate failed) | local node --test battery 2026-09-18 at 782633a6; ldg-windows 13 pass, 1 fail (BM12 enrolled-root gate), 1 skip |
| PUL-010 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_010` case attestation; production `LedgerSkillProvider` delivered 2 skill-typed blocks + verbatim `membrane_source_read` replay `ok:true`; anchors `candidate_for_skill` `kind_priority` | FOCUSED_PASS — 0 failures | local node --test battery 2026-09-18 at 782633a6; pul-windows suite green, 0 fail (14 tests, 0 fail) |
