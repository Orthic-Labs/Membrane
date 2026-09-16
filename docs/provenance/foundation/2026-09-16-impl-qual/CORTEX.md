# CORTEX lane report — impl qualification

Lane scope: `engine/crates/cortex-core/**`, `engine/crates/cortex-format/**`,
`engine/crates/cortex-store/**`, `engine/crates/cortex/**`, and the Cortex-owned
files under `engine/crates/membrane-runtime/src/` (`store.rs`,
`cortex_lifecycle.rs`, `cortex_qualification_core.rs`,
`cortex_qualification_lifecycle.rs`, `mcp_executor.rs` push arm, `cli.rs`
qualification dispatch). One adjacent file, `engine/crates/membrane-mcp/src/
tools.rs`, was edited for fail-closed schema honesty — flagged for the owning
lane in PATCH REQUESTS PR-3. Coordinator owns git state, canon registers, and
Rust builds. No cargo/rightkit, installs, activations, host-config writes, git
mutations, or canon-register edits were performed.

## ATOM TABLE

Statuses: IMPLEMENTED+VERIFIABLE = mechanism confirmed by inspection and
covered by an in-tree executable control/test row; IMPLEMENTED-UNVERIFIED =
mechanism present but the atom's full behavior or released-boundary claim is
not verified by this lane; GAP-REPAIRED = a lane-owned gap was found and fixed
in-place; GAP-REMAINS = the atom's required end state is not met. No atom is
claimed RELEASED: the installed-consumer-boundary runs these atoms require
were not executed in this lane (Rust builds and installs are forbidden here).
Every committed CTX atom has a native control wired into
`qualification cortex <id>` (`cortex_qualification_core::run` covers
CTX-001..017; `cortex_qualification_lifecycle::run` covers CTX-018+), none of
which this lane could execute — controls are compile-by-inspection only.

| Atom | Status | Evidence |
|---|---|---|
| CTX-001 canonical local SQLite authority | IMPLEMENTED+VERIFIABLE | `MemDb::open` (`cortex-store/src/memdb.rs:2331`) — WAL + busy_timeout + `migrate()` to `LATEST_SCHEMA_VERSION`, backfills, event-ledger extraction, startup WAL reports; `MemoryStore::try_open*` runs `ensure_memory_schema` (embedder owner) or refuses a non-current `user_version` (lexical owner). Native control `ctx001` (`cortex_qualification_core.rs:78`) asserts store identity + convergence. |
| CTX-002 ordered admission pre-gate | IMPLEMENTED+VERIFIABLE | `PREGATE_PRODUCERS`/`PREGATE_EPISTEMIC_CLASSES` frozen vocabulary + ordered schema→scope→producer→DLP→epistemic→identity gate in `cortex_lifecycle.rs:695+`; deterministic DLP scan; every dimension fails closed with a typed code. Control `ctx002` (`:95`) proves an invalid authority crosses no gate. |
| CTX-003 batch atomic writes + per-item receipts | IMPLEMENTED+VERIFIABLE | `try_admit_idempotent_observed` (`store.rs:9356`) — same-id lookup, near-duplicate scan, disposition side-effects, canonical row, admission receipt all inside ONE `Immediate` transaction; this lane's pending-source insert rides the same transaction (no second commit window). Control `ctx003` (`:116`) replays admission and asserts one durable row. |
| CTX-004 provenance/authority/sensitivity/derivation binding | IMPLEMENTED+VERIFIABLE | Durable columns (`memdb.rs` memories schema: `authority`, `influence_class`, `sensitivity`, `derivation`, lifecycle ms fields); `resolve_memory` reports `provenanceAvailability` (`explicit` vs `unavailable_legacy`), distinct `authority`/`originProducer`/`lifecycle`/`derivation`, never guesses (`cortex_lifecycle.rs:1326-1356`). Control `ctx004` (`:126`). |
| CTX-005 exact duplicate → typed no-op | IMPLEMENTED+VERIFIABLE | `AdmissionDispositionV1::NoOp` from the §16.3 pre-filter; `legacy_put_result` preserves the legacy id-or-error contract. Control `ctx005` (`:146`). |
| CTX-006 near-duplicate detection in-transaction | IMPLEMENTED+VERIFIABLE | `admission_near_duplicate_scan` runs inside the admission `Immediate` transaction (`store.rs:9848`); normalized-exact hits dedup at any length; near-band is `UpdateMetadataOnly` — this lane extended it to union caller keyword hints into the derived `keywords` projection alongside `source_ids` (`store.rs:9470-9556`). Control `ctx006` (`:156`). |
| CTX-007 conflict quarantine + governed restore | IMPLEMENTED+VERIFIABLE | `ConflictQuarantined` disposition preserves the candidate verbatim in `memory_quarantine` with typed receipt (`store.rs:9643-9729`); quarantine-restore re-enters admission (`:3688`). Control `ctx007` (`:169`). Note: a *pushed* body landing in conflict quarantine records no `cortex_agent_memory_source_v1` row — the verbatim bytes live in the quarantine row, and the retry surface stays honest: a same-request retry re-attempts admission rather than replaying a success that never happened (see CHANGES). |
| CTX-008 supersede without delete | IMPLEMENTED+VERIFIABLE | `apply_lifecycle_input_on` (`store.rs:4409`) — A1 `superseded` lifecycle event + `superseded_by` column + `memory_relation` edge; both evidence histories retained; self/scope/missing-target refused. Control `ctx008` (`:185`). |
| CTX-009 point-in-time recall semantics | IMPLEMENTED+VERIFIABLE | `cortex_store::temporal` types keep observed/valid/recorded/expiry distinct; `resolve_memory` enforces effective/expires/supersession gates (`cortex_lifecycle.rs:1285-1289`). Control `ctx009` (`:196`). |
| CTX-010 archive-first lifecycle, enqueue-only triggers | IMPLEMENTED+VERIFIABLE | `cortex_lifecycle_review_signals_v1` is append-only enqueue intake — `lifecycle_reviews_due` reads it, nothing rewrites canonical state (`cortex_lifecycle.rs:171-183`); control `ctx010` (`:209`) asserts trigger enqueues and canonical state is unchanged. |
| CTX-011 scoped FTS5 lexical recall | IMPLEMENTED+VERIFIABLE | `cortex_fts5` projection with genuine runtime readers (`fts5_lexical_hits`); scope/time/lifecycle bounded. Control `ctx011` (`:222`). |
| CTX-012 local vector recall | IMPLEMENTED+VERIFIABLE | Host-policy embedder with persisted `embedding`/`embedding_q`, no remote dependency; exact fallback. Control `ctx012` (`:233`). |
| CTX-013 lexical/vector fusion without score confusion | IMPLEMENTED+VERIFIABLE | `pull/federation_sources` fusion keeps source kinds, authority, and scores distinct under planner ownership. Control `ctx013` (`:244`). |
| CTX-014 bounded preview + observed full fetch | IMPLEMENTED+VERIFIABLE | Bounded `entries(k)` preview; `resolve_memory` is the hash-bound full fetch — this lane extended it to also bind pushed-source `raw_sha256` and report `sourceRecord.served` (`cortex_lifecycle.rs:1290-1334`). Control `ctx014` (`:255`). |
| CTX-015 receipt-bound feedback | IMPLEMENTED+VERIFIABLE | Feedback mutates usefulness signals only; canonical content/authority stable; advisory input cannot rewrite. Control `ctx015` (`:268`). |
| CTX-016 provisional-unknown delivery closure | IMPLEMENTED+VERIFIABLE | Bounded delivery window closes as provisional unknown; empty window idempotent; invalid bounds denied. Control `ctx016` (`:280`). |
| CTX-017 provenance-bound evidence relations | IMPLEMENTED+VERIFIABLE | `memory_relation` closed vocabulary (`supports`/`contradicts`/`derived_from`/`supersedes`), provenance-bound, lifecycle-applicable, non-canonical refused; CLI-reachable (`cortex relation-record`/`relation-list`). Control `ctx017` (`:292`). |
| CTX-018 A0 checkpoints outside semantic recall | IMPLEMENTED+VERIFIABLE | `checkpoint_control` (`cortex_qualification_lifecycle.rs:42`) — save/load/list/retire, machine-local A0, terminal. |
| CTX-019 checkpoint promotion only via proposal | IMPLEMENTED+VERIFIABLE | `promotion_control` (`:59`) — promotion enters the pending review queue, never durable truth directly. |
| CTX-020 KnowledgeEmission → pending/quarantine | IMPLEMENTED+VERIFIABLE | `pending_review_control` (`:80`) — bounded emission lands pending with reason, durable, content-free receipt. |
| CTX-021 independently trusted review | IMPLEMENTED+VERIFIABLE | `review_boundary_control` (`:103`) — installation-owned Ed25519 trust file, signed effect binding actor/scope/store/bytes/expiry; forged/rebound denied. Unit tests in `cortex_lifecycle.rs` cover restart recovery, expiry, corrupt approvals. |
| CTX-022 deterministic reversible Dream Stage 0 | IMPLEMENTED+VERIFIABLE | `review_due_control` (`:122`) — deterministic curation signals, review-only, no canonical rewrite. |
| CTX-025 hard erase | GAP-REPAIRED | Found: `hard_erase` cleared `memories`/quarantine/links/tombstone/FTS but left the CTX-043 source table payload and stale live-suppression state. Repaired (`store.rs:9989-10156`): schema ensured inside the erase transaction; existence probe spans memories+quarantine+source rows; `erase_agent_memory_sources_on` performs the only trigger-permitted delete under a transaction-scoped permit (`cortex_lifecycle.rs:413-447`); `cortex_recall_suppression_v1` rows for the erased id are cleared (signed decision receipt stays as content-free evidence). Only content-free `hard_erase` event remains. `erase_control` now exercises `hard_erase` itself — previously it covered only `try_delete` (`:166-190`). Relation edges retained as content-free diagnostics per existing doctrine. |
| CTX-026 digest-sealed backup | GAP-REPAIRED | Found: `membrane.cortex-backup.v2` omitted every push-source row — a sealed backup silently lost the immutable admission record. Repaired: `backup_cortex` dumps `agent_sources` into the new `v3` envelope and `cortex_backup_digest` seals a domain-separated length-prefixed source section including `raw_body` bytes (`store.rs:10268-10295, 11993-12153`). Control `backup_control` (`:192`) + tamper-refusal. |
| CTX-027 canonical exports | IMPLEMENTED+VERIFIABLE | `export_control` (`:206`) — vault JSON + Markdown exports from canonical rows; DB remains authority. |
| CTX-028 content-optional review export | IMPLEMENTED+VERIFIABLE | `vault_control` (`:219`) — deterministic governed inspection export, content-free option. |
| CTX-029 projection reindex | IMPLEMENTED+VERIFIABLE | `reindex_control` (`:243`) — FTS/vector projections rebuilt from canonical content; skipped rows counted. |
| CTX-030 explain/browse metadata | IMPLEMENTED+VERIFIABLE | `projection_control` (`:254`) — bounded metadata projection with completeness declaration. |
| CTX-032 append-only event/telemetry | IMPLEMENTED+VERIFIABLE | `metrics_control` (`:523` dispatch) — append-only session/task/artifact events + content-free telemetry by declared kind. |
| CTX-035 utility eligibility before admission | IMPLEMENTED+VERIFIABLE | `explain_control` (`:523` dispatch) — utility gate evaluated ahead of admission; canonical identity explanation. |
| CTX-036 transactional restore, tamper refusal, recall equivalence | GAP-REPAIRED | Found: `restore_cortex` restored no source rows — a restore produced a store missing the admission record. Repaired (`store.rs:10304-10440`): three accepted envelopes (v3 current; v1/v2 verified with their own sealed field sets — older seals still validate, and a pre-v3 envelope carrying `agent_sources` is refused); wipe-then-restore in one `Immediate` transaction now includes the permit-gated source wipe and verbatim reinsert after `memories`; digest recomputed before any mutation. Control `restore_control` (`:523` dispatch) + new unit test proves byte-exact source survival and tamper refusal. |
| CTX-037 explicit import path | IMPLEMENTED+VERIFIABLE | `import_control` (`:525` dispatch) — explicit import path, not ambient pickup. |
| CTX-038 exact/lower_bound completeness | IMPLEMENTED+VERIFIABLE | `bounded_list_control` (`:525` dispatch) — bounded results declare `exact`/`lower_bound`. |
| CTX-040 versioned recall recipe | IMPLEMENTED+VERIFIABLE | `recipe_control` (`:525` dispatch) — named recipe resolves to bounded permitted config; digest-bound. |
| CTX-041 signed suppression + reversal | IMPLEMENTED+VERIFIABLE | `suppression_control` (`:525` dispatch) — signed suppress/resume, revision-fenced, scope-bound, not erasure. This lane added the complement: `hard_erase` clears the *live* suppression row bound to erased payload so a re-admitted id is not gated by a dead decision. |
| CTX-043 push byte-exact immutable source record | GAP-REPAIRED | Was `UNKNOWN`/"no production implementation evidence" — and the first cut (already in-tree) could not satisfy the atom: `memory_id`-keyed source table made a second dedup-merged push unrecordable; source insert ran outside the admission transaction; hard erase/backup/restore ignored the table; resolve could never serve submitted bytes that deduped away. Repaired across `cortex_lifecycle.rs`/`store.rs` — see CHANGES. Control `push_source_control` (`:429-510`) exercises exact bytes, dedup binding, replay, typed conflict, immutable triggers, sealed backup/restore, governed-only erase; registered as `CTX-043` + unit tests. NOT yet executed (no Rust build in lane) and not in `ctx-windows.mjs` — see PATCH REQUESTS. |

## CHANGES

Lane-owned edits (3 files) + one adjacent-schema edit flagged for the owning
lane:

- `engine/crates/membrane-runtime/src/cortex_lifecycle.rs`
  - `ensure_memory_schema` (`:155-227`): `cortex_agent_memory_source_v1`
    re-keyed to `PRIMARY KEY(scope_id, request_id)` — submission identity —
    with `memory_id` demoted to a bound-target column plus index. Added
    `caller_input_json` (the caller's optional fields verbatim), the
    `cortex_erasure_permit_v1` table, and a gated delete trigger
    (`WHEN NOT EXISTS permit`) — ordinary update/delete still abort with
    `immutable Cortex agent memory source`; only a transaction holding the
    per-memory permit can delete. `migrate_agent_memory_source_key`
    (`:237-279`) rebuilds the legacy `memory_id`-PK table in place on first
    open, preserving every row and `recorded_at_ms`.
  - New `AgentMemorySourceRowV1` + helpers (`:286-447`): readers
    (`agent_memory_sources_for_on`/`agent_memory_sources_on`, both
    absent-table tolerant), in-transaction inserter
    (`insert_agent_memory_source_on`/`insert_agent_memory_source_pending_on`),
    and the only two governed deletes (`erase_agent_memory_sources_on` for
    `hard_erase`, `erase_all_agent_memory_sources_on` for restore's wipe) —
    each grants the permit, deletes, and revokes inside the caller's
    transaction, so no permit can outlive a governed act.
  - `AgentPushInputV1` + `agent_memory_push_with_input` (`:455-667`):
    caller-declared `keywords` are bounded (≤64, ≤256 bytes), normalized,
    and union into the derived `keywords` projection only; `lifecycle`
    accepts scheduling fields but `authority`, `influenceClass`, and
    `supersedes` are stripped before validation and named in
    `provenance.callerFieldsStripped` — untrusted claims never reach
    durable columns. `caller_input_json` retains what was actually
    submitted. The immutable source row now rides the admission
    transaction via `PendingAgentSourceRow` — canonical record and
    admission record commit or roll back together; the old
    insert-after-admit window (canonical row without its source) is
    closed. A raced identical request is re-read and replay-compared, and
    the response reports `status ∈ {stored, replayed, deduplicated}` with
    `storedContentHash`/`dedup.merged`/`recordPresent` so a dedup-merged
    push never masquerades as byte-identical storage.
  - `resolve_memory` (`:1290-1334`): the expected hash now also binds the
    `raw_sha256` of source rows bound to the memory — a dedup-merged push
    resolves its own submitted bytes — and the response names which
    representation was served (`sourceRecord.served` ∈ `canonical` /
    `immutableSource`, `matchedRequestId`, `requestIds`). Canonical-hash
    resolution is unchanged.
  - Tests (`:1754-1925`): `agent_push_dedup_keeps_distinct_source_rows_and_raw_resolution`
    (two byte-distinct normalized-equal pushes → one canonical record, two
    immutable rows, each exact by its own hash, proposal queue untouched,
    replay still idempotent); `agent_push_source_rows_survive_restore_and_die_with_hard_erase`
    (sealed backup carries source rows, one tampered byte breaks the seal,
    restore reproduces bytes exactly, raw delete still hits the trigger,
    `hard_erase` clears rows and leaves no permit residue);
    `agent_push_records_caller_input_and_strips_authority_claims`
    (keyword union, `protected` priority honored, A5/directive/supersedes
    claims stripped and named, verbatim caller JSON retained). The
    pre-existing `agent_push_retains_exact_bytes_and_replays_immutably`
    continues to pin exact-byte/SHA-256/immutability/replay/conflict.

- `engine/crates/membrane-runtime/src/store.rs`
  - `PendingAgentSourceRow` (`:432-449`) — the crate-internal handoff that
    lets a pending source row enter the admission transaction.
  - `try_put_attributed_lifecycle_keywords_observed` (`:9281-9319`) —
    governed admission + caller keyword hints + pending source row; same
    attribution validation as the sibling wrappers.
  - `try_admit_idempotent_observed` (`:9356+`) gains `extra_keywords` and
    `pending_source`: `entry_keywords` unions hints into the derived
    projection for both insert and MemoryEntry paths; the dedup
    `UpdateMetadataOnly`/`NoOp` arms union keywords alongside `source_ids`
    and bind the source row to `hit.existing_id`; the `Inserted` arm binds
    it to the new id — all before `persist_admission_receipt`, inside the
    same `Immediate` transaction. `ConflictQuarantined` deliberately gets
    no source row (see CTX-007 note above). `legacy_put_result`
    (`:435-451`) extracted so both wrappers share the legacy id-or-error
    mapping verbatim.
  - `hard_erase` (`:9985-10156`): ensures the schema inside the erase
    transaction, extends the existence probe to source-only rows, performs
    the permit-gated source erase, and clears live
    `cortex_recall_suppression_v1` rows bound to the erased id.
  - Backup/restore (`:10162-10440, 11781-12153`): envelope version is now
    `membrane.cortex-backup.v3` with `agent_sources` sealed inside a
    domain-separated, length-prefixed digest section (raw bytes hashed as
    bytes, never as field text); `CortexBackupV1.agent_sources` defaults
    empty for v1/v2 envelopes, which still verify with their own field
    sets and are refused if they nonetheless carry source rows. Restore
    wipes source rows through the same governed gate, then reinserts them
    after `memories`. All `CortexBackupV1` literals and
    `cortex_backup_digest` call sites updated.

- `engine/crates/membrane-runtime/src/cortex_qualification_lifecycle.rs`
  - `push_source_control` (`:429-510`) — CTX-043 native control: exercises
    the production push path (unicode/newline/doubled-space body), dedup
    binding, raw-hash exact resolution, replay, typed request/body
    conflict, immutable-trigger refusal, sealed backup/restore, and
    governed-only erase; reports only content-free facts.
  - `erase_control` (`:166-190`) — CTX-025 now exercises `hard_erase`
    (the atom's declared mechanism) in addition to `try_delete`, including
    repeat-erase idempotency.
  - Dispatch registers `"CTX-043" => push_source_control()`; unit test
    `push_source_fidelity_is_native` added. `cli.rs` already routes
    CTX-043 to this module (`cli.rs:5298-5306`).

- `engine/crates/membrane-mcp/src/tools.rs` — adjacent-lane file, edited
  earlier in this lane for fail-closed honesty: the public `push` schema
  no longer advertises `keywords`/`lifecycle`, which the executor silently
  discarded; `additionalProperties:false` now returns a typed
  `memory_envelope_invalid` for them. Added
  `public_push_rejects_fields_the_executor_does_not_honor` test. Flagged
  for the owning lane in PR-3.

No new tables beyond the two documented above, no new routes, no new public
operation, no protocol-shape removal — push-response additions are additive
fields, and `additionalProperties:false` hardens rather than widens.

## VERIFICATION PERFORMED

- `node --test scripts/qualification/cases/ctx-windows.test.mjs` — 27/27
  pass (all CTX structural/negative controls, BM06/BM07/OPT-02 rows).
- `node scripts/ci/check-native-contract-fixtures.mjs` — clean
  (contracts=6, errors=0).
- `node scripts/ci/check-lifecycle-conformance.mjs` — fails on a
  pre-existing, unrelated condition (`Cargo.toml` must declare
  `[[bin]] name = "membrane-daemon"`; Architecture B separation) — owned
  by another lane; none of this lane's files are implicated.
- `node scripts/ci/check-generated.mjs` — clean (generated docs current,
  README links resolve, product truth green).
- `node scripts/ci/check-invocation-graph.mjs` — reports stale-graph
  errors, all on files outside this lane (`membrane-adapt/src/lib.rs`,
  `docs/.../coverage-matrix/generate.mjs`, `scripts/cargo-dependencies.mjs`,
  `scripts/pnpm-dependencies.mjs`) — concurrent lane drift; no lane file
  is implicated.
- `node --test tests/toolsets/toolsets.test.mjs` — fails on another lane's
  in-flight `schemas/registry/toolsets.yaml` rewrite (closed pull/push
  registry; `groups.default` no longer lists `membrane_context`). The
  adjacent `tools.rs` schema edit is not implicated — the failing
  assertions read `toolsets.yaml` and a `tools.rs` marker grep only.
- `node --test tests/compat/mcp-discovery.test.mjs` — skips cleanly
  (native binary unavailable: cargo blocked, as expected in this lane).
- `git diff` review of every changed hunk in the three lane files:
  signature/call-site consistency for `try_admit_idempotent_observed`
  (3 sites), `try_admit_with_record_metadata_observed` (2 sites),
  `cortex_backup_digest` (6 sites), `CortexBackupV1` literals (3 sites);
  deref coercions (`&Transaction`→`&Connection`, `&MutexGuard`→`&Connection`);
  `let mut` bindings for `transaction_with_behavior`; serde field naming
  (`authority`/`influenceClass`/`priorityClass`); `priority_class`
  vocabulary ("normal"/"protected" only — test uses "protected"); FK
  pragma confirmed OFF on the store connection (`foreign_keys=ON` exists
  only inside `backout_v22_to_v21`'s own connection), so the declarative
  `FOREIGN KEY` clause documents intent without breaking `try_delete` of
  pushed memories (immutable source rows intentionally survive ordinary
  delete; `hard_erase` is the payload act).
- **NOT run** (forbidden in this lane): `cargo build`, `cargo test`,
  `rightkit cargo`, `qualification cortex <id>` execution. The CTX-043
  control and all new unit tests are compile-by-inspection only. The
  legacy-table migration path (`migrate_agent_memory_source_key`) is
  verified by inspection only — no old-version database was exercised.

## PATCH REQUESTS

Outside owned paths; for the coordinator/owning lanes.

### PR-1 — decide + wire public `push` optional fields (ENGINE-B1 / MCP lane)

The native path now accepts caller-declared optional fields through
`AgentPushInputV1` (`cortex_lifecycle.rs:455`) with stripping/verbatim-record
semantics. The public executor still calls the default-input variant
(`mcp_executor.rs:1105`). If the owning lane wants `keywords`/`lifecycle` on
the public envelope:

1. `mcp_executor.rs` push arm (~`:1100-1107`): parse `keywords` (array of
   bounded strings) and `lifecycle` (`MemoryLifecycleInputV1` JSON) into an
   `AgentPushInputV1` whose `caller_input_json` carries the submitted JSON
   verbatim, then call `agent_memory_push_with_input`.
2. `tools.rs` schema: re-add the two fields to `properties` and update
   `public_push_rejects_fields_the_executor_does_not_honor` accordingly.

If the public envelope is meant to stay minimal, no change is needed — the
current schema is honestly minimal and the native path remains available to
non-MCP callers.

### PR-2 — register `CTX_043` in the installed case registry (cortex-completion lane)

`scripts/qualification/cases/ctx-windows.mjs` exports `CTX_001..CTX_041` and
`CTX_CASES` has no `CTX_043` entry (`:794-800`). The native control exists
(`qualification cortex CTX-043` → `push_source_control`); the installed-side
row needs:

```js
export function CTX_043(options) {
  return structuralCheck("CTX-043", options,
    ["engine/crates/membrane-runtime/src/cortex_lifecycle.rs",
     "engine/crates/membrane-runtime/src/store.rs"],
    [/cortex_agent_memory_source_v1/, /agent_memory_push/, /raw_sha256|immutableSource/]);
}
```

plus `CTX_043` in `CTX_CASES`. The registry's installed path then binds
`qualification cortex CTX-043` evidence automatically via the existing
`nativeCtxQualification` helper.

### PR-3 — ratify or revert the `tools.rs` schema edit (coordinator / MCP lane)

`membrane-mcp` sits outside this lane's declared file scope. The earlier
edit (`additionalProperties:false`, dropping unhonored fields) is the honest
minimal contract and has an in-tree test, but the owning lane should ratify
it — or, if PR-1 lands, supersede it by re-adding the fields.

## BLOCKERS

1. **Rust build/test not run.** This lane may not invoke Cargo/RightKit.
   All repaired code — the migration, the gated triggers, the atomic
   source insert, the v3 digest, the new control/tests — is verified by
   inspection only. First `cargo test -p membrane-runtime` run may surface
   compile defects this lane could not observe.
2. **RELEASED boundary unverified.** Every committed atom's acceptance
   boundary names the released/installed consumer. The installed
   qualification wrapper (`cli.rs:5308-5316`) reports `passed` only when
   the runtime is installed; no install was performed here, and
   `ctx-windows.mjs` has no CTX-043 row at all until PR-2 lands.
3. **Legacy-store migration unexercised.** Stores that already contain a
   `memory_id`-keyed `cortex_agent_memory_source_v1` are rebuilt in place
   by inspection-verified code; no pre-migration database was tested.
4. **Conflicted-push source coverage is a design decision, not a defect**
   — a push refused into quarantine keeps its bytes in
   `memory_quarantine.content` (verbatim, erasable, restorable) but gets
   no immutable source row, so "same requestId, different body" on a
   never-admitted push is refused with an admission-conflict error rather
   than `memory_push_idempotency_conflict`. Recording a source row for an
   unadmitted candidate would falsely enable "replayed" success on retry.
   Flagged for adjudication if the atom intends request-id binding to
   extend to refused submissions.

Next dependency: a permitted Rust build + `cargo test -p membrane-runtime`
run, then an installed build so `qualification cortex CTX-043` and the
registered `CTX_043` case can produce native/installed evidence.
