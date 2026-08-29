# Dispatch prompt — Membrane §13.4 in-repo schedulable gaps

Source of truth: `docs/pending/MEMBRANE-PENDING-IMPLEMENTATION.md` at revision
`41036d730764a1032ab96c5424730b5290621196`. Canon under `docs/subsystems/` overrides this prompt on
any conflict. Workers are zero-context: everything needed is stated here or in the referenced
read paths.

## Scope

In scope (one wave, six parallel lanes): §15 native-path authorization; §16.1 approved-proposal
consumer; §16.3 write-time duplicate/conflict detection; §16.4 hard erase + backup/restore over the
existing quarantine; §17.1 typed abstention; §17.2 publication fence; §13.1 first-party
sufficiency caller; §2.5–§2.7 `InterventionAttributionV1`; §18 Ledger section-identity
unification; §19 installed-runtime guarantees.

Out of scope, excluded deliberately: host-side capabilities (H7/H9/H10 producers, request-time H8
host refresh, H4/H6 caller transport), the background semantic provider server, all qualification
freezes (§8.2, §10.2 — integrator-owned measurement after merge), §4.2 (spec-only by its own
disposition).

## Hard worker constraints (every lane)

Edit only your exact allowlist. You may read any listed read path. Never run cargo, tests, builds,
generators, installs, commits, pushes, merges, or expensive checks — record intended checks for the
integration owner instead. If your task requires touching a file outside your allowlist, stop and
return `TRUE_BLOCKER` with the path and reason. If an owned path is already dirty, stop. Match
surrounding code style; comments only for constraints code cannot show. Typed failure everywhere:
never silence, never a zeroed default, never downgrade-and-continue.

## Frozen cross-lane interfaces

1. `admit_approved_proposal`: lane-cortex-durable-lifecycle defines on the store:
   `pub fn admit_approved_proposal(&self, proposal_id: &str, payload_json: &str) -> Result<ApprovedProposalAdmissionV1, StoreError>`
   returning a typed outcome (`admitted { memory_id } | duplicate { existing_id } | conflict { existing_id }`).
   lane-native-authorization calls exactly this signature from the executor review path.
2. `sufficiencyContract` passthrough: lane-native-authorization forwards the executor's
   `arguments["sufficiencyContract"]` JSON value verbatim into the federate call it already makes;
   lane-pull-fence-abstention owns schema acceptance (`tools.rs`) and consumption
   (`pull/federation.rs`). Neither lane edits the other's files.
3. Ledger public signatures: lane-ledger-identity must not change the public signatures of
   `read_section` / `read_section_with_cursor`; callers (`cli.rs`, `checkpoint.rs`,
   `mcp_executor.rs`) are out of its allowlist.

## Lane instructions

### lane-native-authorization (§15, §16.1 executor side)

Port, do not reimplement from scratch: `mcp/authorization.mjs` (`intersectAuthority`,
`authorizeTarget`) is the existing source implementation; port its behavior into a new Rust module
`engine/crates/membrane-runtime/src/authorization.rs` with the same gate order:
installation grant → repository scope chain → caller/target binding → authority level (monotone
minimum) → cross-root denial → validity interval/revocation. Wire it in
`engine/crates/membrane-runtime/src/mcp_executor.rs` so every repository-scoped read or write on
the native path (stdio → `RuntimeMcpExecutor::execute`) runs the gate before retrieval scoring and
before admission. Failure is a typed `authorization_denied` naming the failed gate — never silent
scope widening, never downgrade-and-continue. Bearer token authenticates the channel only; a
self-declared `repositoryId`/`scopeId` is a claim verified against the installation registry
(see `mcp/installation-binding.mjs` for registry semantics). Register the module in `lib.rs`.
Then §16.1 executor side: implement the `pending → approved | rejected` review transition on the
`membrane_knowledge_proposal` table (it currently only inserts `pending`; the CHECK constraint
already permits the values) and, on approval, call frozen interface 1. Add the `sufficiencyContract`
verbatim passthrough (frozen interface 2). Tests in
`engine/crates/membrane-runtime/tests/native_authorization.rs`: unauthorized/mismatched repository
identity denied with the failed gate named; the test fails if the executor stops calling the shared
module; approved proposal reaches admission or `approved` is unreachable — no third outcome.

### lane-cortex-durable-lifecycle (§16.3, §16.4, §16.1 admission side)

All in `engine/crates/membrane-runtime/src/store.rs`. (a) Write-time dedup: the durable write path
(`persist_entry_with_record_lifecycle_on`, id-keyed `ON CONFLICT(id)`) admits near-identical
content under different ids silently. Add a deterministic admission pre-filter — exact
normalization, a cheap specificity gate, then a near-duplicate similarity threshold
(MinHash/Jaccard class) — yielding `duplicate` or `conflict` dispositions per Cortex canon
vocabulary. No model call on the write path; ambiguity surfaces as `conflict`, never a second
silent record. (b) Erasure: reversible quarantine already exists (`memory_quarantine`,
`restore_quarantined`) — keep it first. Add governed hard erase as a distinct operation that
provably clears payload from every Cortex-owned projection path (memories row, FTS index,
embeddings, quarantine copy). Add backup (dump) and restore with a
dump → wipe → restore → recall-equivalence proof path. (c) Define frozen interface 1
(`admit_approved_proposal`) routing through Cortex admission including the new dedup pre-filter.
Tests in `engine/crates/membrane-runtime/tests/cortex_lifecycle_gaps.rs`: near-duplicate write
under a different id yields `duplicate`/`conflict`, never a second silent record; quarantined rows
restore transactionally; hard erase leaves no payload in any projection; backup → wipe → restore
proves recall equivalence.

### lane-pull-fence-abstention (§17.1, §17.2, §13.1)

(a) §17.1: add `InsufficientConfidenceV1` to `engine/crates/membrane-protocol/src/federation.rs`
(fields: `status: insufficient_confidence`, per-lane `searched` counts,
`reason: no_authorized_candidate_above_threshold | no_candidates | evidence_floor`, optional
`suggested_action`), emitted instead of below-floor hits when nothing clears the admission floor.
Add `InsufficientConfidenceV1` to the explicit `pub use federation::{...}` re-export list in
`engine/crates/membrane-protocol/src/lib.rs` so it is reachable as
`membrane_protocol::InsufficientConfidenceV1`, matching the existing pattern.
A string-only `insufficient_confidence` already exists on the Cortex/Taste recall path in
`store.rs` (out of your allowlist): do not touch it; keep the new shape explicitly distinct and
versioned. (b) §17.2: in `engine/crates/membrane-federation/src/engine.rs` (scope binding happens
once, early, in `bind_scope`) and `engine/crates/membrane-runtime/src/pull/publication.rs`,
re-validate grant identity, policy epoch and revocation after fusion, immediately before packet
emission; a change publishes typed `policy_changed` insufficiency and the stale-authorized packet
is never emitted. (c) §13.1: extend the `membrane_context` tool schema in
`engine/crates/membrane-mcp/src/tools.rs` with optional `sufficiencyContract` (schema currently has
`additionalProperties: false` and would reject it) and consume it in
`engine/crates/membrane-runtime/src/pull/federation.rs`, which already forwards
`sufficiencyContract` from resident JSON bodies — unify so the MCP caller path reaches the same
`SufficiencyContractV1` evaluation in `corrective.rs`. One alternate-provider-lane corrective
action only; never repeat against the trigger provider. Tests in
`engine/crates/membrane-federation/tests/fence_abstention.rs`: no-answer query publishes typed
`insufficient_confidence`, never below-floor hits; grant/policy-epoch change between admission and
emission publishes `policy_changed`.

### lane-adapt-attribution (§2.5–§2.7)

New `engine/crates/membrane-adapt/src/attribution.rs`: `InterventionAttributionV1`, sealed with
identity `att_<64hex>` (same sealing pattern as `rem_<64hex>` in `remediation.rs`), with the exact
field set of pending doc §2.5 (attribution_id, source_issue_id, candidate_target,
owning_surface_ref?, current_surface_digest?, instruction_state, counterfactual_preventability,
alternative_causes[], support with per-field coverage markers, activation_evidence_refs[],
evaluator_outcome_refs[] with tri-state applicability, mutation_eligible, ineligibility_reason?,
honesty_limit, attribution_policy_version). `mutation_eligible` derives deterministically from the
five gates in §2.5; it grants no authority and bypasses no existing gate. In `remediation.rs`,
require that a sealed proposal targeting a mutable instruction surface (`skill_or_procedure`,
`system_prompt`, `tool_description`, `documentation_policy`) reference a `mutation_eligible`
attribution before it may be marked consumable for variant generation; additive `guard`/`evaluator`
targets are informed, not blocked. Stale `current_surface_digest` invalidates the attribution.
Absent evidence is `unavailable`, never zero; `insufficient_evidence` evaluator outcomes leave the
applicable denominator. Register the module in `lib.rs`. Unit tests in-module per pending doc §14
Adapt list (already_correct never eligible; unsupported counterfactual never eligible; stale digest
fails adoption; mutable-surface proposal without attribution not consumable).

### lane-ledger-identity (§18)

In `engine/crates/membrane-runtime/src/ledger/outline.rs` and `ledger/index.rs`: production section
reads currently resolve `sec:<slug>:<ordinal>` anchors while the index layer computes structural
span-hash node identity (`stable_node_id`/`span_hash`). Unify: the structural span-hash fingerprint
becomes section identity; slug/ordinal anchors remain resolvable as aliases, not identity. Honor
frozen interface 3 (public signatures unchanged; do not edit callers). Preserve hash-bound section
resolution semantics (canon: Ledger owns navigation/index projections, not document truth). Record
intended check: existing ledger resolution tests must pass unchanged plus new alias-resolution
coverage you add in these two files' `#[cfg(test)]` modules.

### lane-ops-runtime (§19)

(a) macOS: `apps/membrane-tray-macos/Sources/MembraneTrayMacOS/DaemonSupervisor.swift` currently
supervises via `Process()` only. Add OS-enforced lifetime coupling scoped to the tray session
(launchd job / XPC-scoped) matching the Windows job-object guarantee
(`apps/membrane-tray-windows/src/process.rs` uses `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` as the
reference behavior): tray exit must guarantee daemon termination with no orphan path. (b) CI:
`.github/workflows/release-candidate.yml` has an `installed-qualification` job gated on
`workflow_dispatch` + boolean input; make installed-artifact qualification
(`scripts/qualification/install-release.ps1`, read-only reference) a required gate on the tag-push
release trigger — a release candidate without that evidence is not a candidate; keep typed skip
reasons only for genuinely absent prerequisites, and fail (not skip) the release when
prerequisites are absent on a tag build. (c) JS SQLite posture: `mcp/proposal-store.mjs`,
`mcp/working-context.mjs`, `mcp/server.mjs` open shared SQLite via `new DatabaseSync(...)` with no
pragmas; set `PRAGMA journal_mode=WAL` and `PRAGMA busy_timeout` identical to the native runtime's
values on every open so mixed-stack writers share one concurrency posture.

## Integration owner protocol (current orchestrator, sole merger)

Merge lane outputs; reconcile changed paths against the ledger; then run, in order: focused tests
per lane (`cargo test -p membrane-runtime --test native_authorization`, `--test
cortex_lifecycle_gaps`, `-p membrane-federation --test fence_abstention`, `-p membrane-adapt`,
ledger tests, `node --test mcp/`), then `pnpm test` and `pnpm test:mcp`, Rust checks through the
workspace RightKit shim, and the docs/productization checks. Packet/receipt schemas verified
together after the protocol change in lane-pull-fence-abstention. Integrator never repairs
lane-owned files: failures return to the owning lane. Qualification freezes (§8.2/§10.2) remain
integrator-owned follow-up work after this packet lands, per §13.4 dependency constraints.

## Recovery

Max 2 retries per lane. Stop conditions: owned path already dirty; required private input missing;
allowlist insufficient (return `TRUE_BLOCKER` + path). Each lane returns: changedPaths, intended
checks, blockers, baselineRevision (`41036d73…`), patchDigest.
