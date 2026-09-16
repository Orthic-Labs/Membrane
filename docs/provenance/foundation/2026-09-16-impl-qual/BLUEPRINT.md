# BLUEPRINT lane report — impl qualification

Lane scope: `engine/crates/membrane-blueprint/**`,
`engine/crates/membrane-grammars-vendored/**`, the Blueprint-owned runtime
files (`blueprint_explore.rs`, `blueprint_one_shot.rs`,
`blueprint_provider_qualification.rs`, `blueprint_security_qualification.rs`,
`sources_explorer.rs`, `sources_producer.rs`,
`providers/blueprint_findings.rs`, `providers/rust_analyzer_provider.rs`,
`providers/typescript_provider.rs`), and
`engine/crates/membrane-federation/src/blueprint_client.rs`. Coordinator owns
git state, canon registers, and Rust builds. No cargo/rightkit, installs,
activations, host-config writes, git mutations, or canon-register edits were
performed. `membrane-runtime/src/freshness.rs`, `service.rs`, `cli.rs` and all
other runtime files were read-only.

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
| BPT-001 canonical root identity & confinement | IMPLEMENTED+VERIFIABLE | `lib_application_root_registry.rs`, `lib_path_confinement.rs`, `security.rs::canonical_root`/`is_confined_path`; `confined_root` (`engine.rs:158-169`) canonicalizes request root and compares against canonical request scope before any operation, rejecting traversal/escape with typed `root_escape`. Production consumers: `cli.rs`, `lib_cli_mcp.rs`, `service.rs`. |
| BPT-002 HEAD/index/worktree observation | IMPLEMENTED+VERIFIABLE | `git_source_observation.rs` — bounded `git rev-parse HEAD` + `git status --porcelain=v1 -z --untracked-files=all`, 5 s timeout, output-capped, xxh3-128 status digest; `freshness_observation.rs` — stricter bounded overlay enumeration (64 files / 64 MiB / 8 MiB output / 2 s timeout) with before/after stability re-check and `rev-list --count` commit distance. |
| BPT-003 typed terminal ingestion disposition | IMPLEMENTED+VERIFIABLE | `index.rs`/`static_provider.rs` classify every considered source to a typed disposition (indexed/skipped-oversize/skipped-binary/unsupported) with no silent disappearance; dispositions surface in generation file reports and truncation reasons. |
| BPT-004 deterministic lexical baseline | IMPLEMENTED+VERIFIABLE | `static_provider.rs` emits lexical symbols/occurrences/edges deterministically as the baseline/fallback provider; covered by parity tests (`parity_static_provider*.rs`). |
| BPT-005 pinned verified Tree-sitter grammars | IMPLEMENTED+VERIFIABLE | `providers/` tree-sitter lane + `membrane-grammars-vendored` pinned grammar set; explicit language-capability reporting (`lib_cli_languages.rs`, `lib_languages_custom_config.rs`); facts carry provider identity/version. |
| BPT-006 SCIP ingestion | IMPLEMENTED+VERIFIABLE | SCIP provider under `providers/` ingests definitions/references/relations/roles/ranges/diagnostics with integrity and position checks; registered through `providers/mod.rs`. |
| BPT-007 JS/Python module binding resolution | IMPLEMENTED+VERIFIABLE | `module_resolution.rs` (734 lines) resolves JS and Python module/import bindings deterministically; feeds edge construction in `graph.rs` and findings BP001–BP003 in `findings.rs`. |
| BPT-008 HTTP-domain facts | IMPLEMENTED+VERIFIABLE | `providers/` HTTP provider emits route/endpoint facts under declared capability; gated by provider registry admission. |
| BPT-009 SQL schema facts | IMPLEMENTED+VERIFIABLE | `providers/` SQL provider emits schema facts under declared capability; gated by provider registry admission. |
| BPT-010 capability/permission provider system | IMPLEMENTED+VERIFIABLE | `providers/mod.rs` is the single registry: capability/permission declarations, identity/checksum/licence validation, per-provider admission before any fact emission; `contract_registry.rs` binds provider contracts. |
| BPT-011 rules/documents as evidence-bound claims | IMPLEMENTED+VERIFIABLE | `lib_rules_*` (parser/baseline/evaluate/exceptions) and `lib_comment_claims.rs` admit rules/documents only as declared claims bound to evidence, never as observed code facts. |
| BPT-012 trusted providers read-only/bounded | IMPLEMENTED+VERIFIABLE | Provider execution is repository-read-only, network-free, process-bounded and cancellable through `RequestContext`/`CancellationToken`; crash-isolated by bounded dispatch. |
| BPT-013 stable identities & rename reconciliation | IMPLEMENTED+VERIFIABLE | `identity.rs` assigns repo/file/entity/occurrence/claim/evidence/generation identities; `delta_store.rs` reconciles renames/moves through content-hash matching on delete+create pairs. |
| BPT-014 evidence binding | IMPLEMENTED+VERIFIABLE | `evidence_authority.rs` + node/edge `evidence` rows bind source address, span/hash, provider/version, generation, truth class and confidence; `confidence_tiers.rs` owns the tier taxonomy. |
| BPT-015 staged atomic generation publication | IMPLEMENTED+VERIFIABLE | `store.rs::save_generation` replaces graph rows and writes the envelope in one SQLite transaction; publish happens only after full validation; last-known-good is preserved because failed validation never reaches `save_generation`. `delta_store::apply_file_deltas` wraps incremental updates in the same transactional publication (graph rows + watch state + journal ack + generation identity). |
| BPT-016 single writer lease / migration recovery | IMPLEMENTED+VERIFIABLE | `store.rs::open_store` takes an exact pre-migration backup before upgrading (`migration_backup_path`), WAL + 30 s busy timeout; `repair_interrupted_migration` restores the from-version backup and re-runs migrations; `open_store_read_only` never migrates. Single-flight is enforced by the generation-pin + transaction boundary. |
| BPT-017 conservative re-anchoring | IMPLEMENTED+VERIFIABLE | `reanchor.rs` re-anchors by exact entity/text/fingerprint/unique normalized text only; otherwise reports stale/ambiguous rather than guessing. |
| BPT-018 cross-file identity resolution | IMPLEMENTED+VERIFIABLE | `module_resolution.rs` + `graph.rs::resolve_file_facts_edges` resolve exact-first; same-tier ambiguity stops resolution; unsupported module semantics produce typed omissions. |
| BPT-019 honest freshness, never old-fresh | GAP-REPAIRED | Was defective: `sealed_freshness_receipt` fabricated `current` from the sealed basis, so an edit after the last refresh reported `fresh`/`Clean` and the live-observation machinery (`observe_current_vcs_state`, `changed_paths_for_freshness`, stale-suppression inputs) was dead code. Now `generation_freshness_receipt` (`engine.rs:1172`) runs the bounded live `git rev-parse`/`git status` probe on every status/query/read receipt, enumerates changed paths lazily only on drift, and `apply_freshness_suppression` (`engine.rs:1218`) injects `staleSourcePaths`/`staleWholeGeneration` into the query request so stale rows are suppressed (`query.rs:464-479`). Test rows: `native_engine.rs::status_observes_live_source_without_full_construction_receipt` (fresh→stale over a real git repo), `status_without_vcs_reports_unavailable_instead_of_old_fresh`. |
| BPT-020 dependency-DAG invalidation | IMPLEMENTED+VERIFIABLE | `dependency_dag.rs` records source/provider/config/schema dependencies; `ProjectionCache` fingerprints declared parent dimensions only and invalidates selectively; covered by `parity_delta_store.rs`/projection tests. |
| BPT-021 incremental maintenance | IMPLEMENTED+VERIFIABLE | `engine.rs::incremental_refresh_with_repair` — explicit watcher event paths skip the repo scan; empty event sets run scan-and-diff (`source_path_delta`); stable reads + content-hash confirm changes; affected-reference closure repairs incoming/outgoing dependents (`unresolved_reference_dependents`); `incremental_noop` on no effective change; unchanged files stay content-addressed/reused; full construction only through `verified_construction_reason`. `watch.rs` owns native watcher + scan reconciliation + event-gap barriers. Test rows: `native_engine.rs::incremental_repair_updates_incoming_reference_edges_and_reuses_unchanged_files`, `empty_refresh_discovers_source_changes_instead_of_reporting_fresh`, `parity_delta_store.rs`. |
| BPT-023 recall seed resolution | IMPLEMENTED+VERIFIABLE | `recall_circuit.rs::resolve_seeds_native` — valid ID, source/path/anchor, qualified symbol, exact term, bounded fuzzy; `query.rs::recall_op` reports unresolved/ambiguous seeds with typed omissions. |
| BPT-024 named bounded recall policies | IMPLEMENTED+VERIFIABLE | `recall_circuit.rs::select_traversal_policy_native` + `policy_kinds` — dependency/impact/callgraph/test/config/architecture families with per-policy hop/node/edge caps; unknown explicit family falls back with a typed omission rather than silent reinterpretation (`query.rs:416-441`). |
| BPT-025 ordered evidence paths | IMPLEMENTED+VERIFIABLE | `execute_recall_circuit_with_totals` returns complete ordered paths with path ID, node/edge evidence, completeness flags and omissions (`query.rs:455-479`). |
| BPT-026 non-compensatory ranking | IMPLEMENTED+VERIFIABLE | Recall ranking gates on admissibility/evidence/seed/coverage/truth/analysis/confidence — non-compensatory ordering in `recall_circuit.rs`; `evidence_authority.rs` orders freshness before authority. |
| BPT-027 traversal bounds & closed cursor failure | IMPLEMENTED+VERIFIABLE | `Limits` enforce max seeds/paths/nodes/edges during traversal (`query.rs:443-451`); stale generation/digest cursors fail closed via `ensure_generation` + `assert_generation_coherence`. |
| BPT-028 bounded text/type/path search | IMPLEMENTED+VERIFIABLE | `query.rs` search op + `bm25_index.rs` over the persisted generation; bounded by `Limits`. |
| BPT-029 anchor resolution | IMPLEMENTED+VERIFIABLE | `query.rs` resolve op maps one user/source anchor to canonical node(s) with ambiguity reporting. |
| BPT-030 bounded neighborhood expand | IMPLEMENTED+VERIFIABLE | `query.rs` expand op — bounded typed neighborhood around resolved nodes via `Limits`. |
| BPT-031 bounded relationship path | IMPLEMENTED+VERIFIABLE | `query.rs` path op — bounded path between two resolved anchors. |
| BPT-032 bounded impact | IMPLEMENTED+VERIFIABLE | `query.rs` impact op + `change_impact.rs` — upstream/downstream impact from diff/file/line/stack/test/treeish seeds without fabricating coverage. |
| BPT-033 liveness LIVE/UNREACHED/UNKNOWN | IMPLEMENTED+VERIFIABLE | `liveness.rs` (276 lines) reports only the three states with evidence; zero inbound edges never proves dead — `parity_*` tests cover the UNKNOWN floor. |
| BPT-034 test recommendation | IMPLEMENTED+VERIFIABLE | `test_recommendation.rs` recommends tests with evidence/reason, uncovered impact and omissions; no unproved claims. |
| BPT-035 change-risk decomposition | IMPLEMENTED+VERIFIABLE | `change_impact.rs`/`analytics.rs` decompose risk into inspectable factors; co-change kept at lower authority via `evidence_authority.rs`. |
| BPT-036 named snapshots | IMPLEMENTED+VERIFIABLE | `lib_application_snapshots.rs` (622 lines) create/list/get over persisted generations. |
| BPT-037 changes since snapshot/treeish | IMPLEMENTED+VERIFIABLE | `changes_since_reference` (`lib_application_snapshots.rs`) serves snapshot/generation/treeish diffs; detached historical worktrees are constructed outside the live store and never overwrite current state (`engine.rs:103-129`). |
| BPT-038 claim grounding | IMPLEMENTED+VERIFIABLE | `lib_comment_claims.rs` binds claims to facts; `doc_truth.rs`/`conformance_verifier.rs` expose direct/indirect/unsupported/contradicted/ambiguous/stale grounding. |
| BPT-039 declared-vs-determined comparison | IMPLEMENTED+VERIFIABLE | `doc_truth.rs`/`conventions.rs` compare declared intent vs deterministic evidence preserving both sides, mismatch, citation and generation. |
| BPT-040 architecture synthesis | IMPLEMENTED+VERIFIABLE | `architecture_views.rs`/`architecture_model.rs` synthesize evidence-backed components/flows as disposable cited projections over the generation. |
| BPT-041 orientation verdicts | IMPLEMENTED+VERIFIABLE | `orientation_projection.rs`/`lib_orientation_evidence.rs` return allow/continue/block/noop with scope, generation, freshness, evidence and omissions. |
| BPT-042 single resident engine across adapters | IMPLEMENTED+VERIFIABLE | `native_blueprint_operation()` (`engine.rs:28`) is the one shared operation: CLI one-shot (`blueprint_one_shot.rs` `OneShotExecutor::new(native_blueprint_operation())`), resident service (`service.rs`), and federation (`membrane-federation::blueprint_client.rs` `from_operation` — injected operation, no sockets/process spawn) all dispatch through it. |
| BPT-043 holder-gated watcher lifecycle | IMPLEMENTED+VERIFIABLE | `service.rs::acquire_holder` starts the resident service on the first `Hub`/`CodeRight` holder; `release_holder` drains only when every holder count reaches zero; watcher/reconciler runs only inside the resident service. Explicit user build/refresh dispatches `Operation::Build/Refresh` independently of holders. |
| BPT-044 canonical envelope + typed errors | IMPLEMENTED+VERIFIABLE | `api.rs` `BlueprintResponse`/`BlueprintError` taxonomy (`code`/`message`, typed `generation_mismatch`, `root_escape`, `blueprint_store_*`); stable across CLI/service/federation adapters. |
| BPT-046 diagnose local state | IMPLEMENTED+VERIFIABLE | `lib_operations_doctor.rs` (293 lines) — bounded local diagnosis operation. |
| BPT-047 federation slices | IMPLEMENTED+VERIFIABLE | `lib_application_federate.rs` (262 lines) — explicit repositories federate as independent generation/evidence/omission slices without merging. |
| BPT-049 findings baselines | IMPLEMENTED+VERIFIABLE | `findings.rs` + `lib_rules_baseline.rs` capture/list/compare baselines with deterministic delta identity (fingerprint = rule+path+name+spec hash). |
| BPT-050 bounded SARIF export | IMPLEMENTED+VERIFIABLE | `export.rs` emits bounded SARIF from detected findings; no remediation authority granted. |
| BPT-051 finding explanation | IMPLEMENTED+VERIFIABLE | `findings.rs` evidence rows bind source path/span/content-hash plus rule reasoning per finding. |
| BPT-052 finding evidence pack | IMPLEMENTED+VERIFIABLE | Source-bound evidence pack path through governed host (`lib_operations_support_bundle.rs`/`export.rs`). |
| BPT-053 framework facts | IMPLEMENTED+VERIFIABLE | `framework_intelligence.rs` (612 lines) — gated event/database/deployment facts into generation evidence. |
| BPT-054 Terraform facts | IMPLEMENTED+VERIFIABLE | `providers/iac_terraform.rs` — Terraform facts into generation-bound evidence under provider capability. |
| BPT-055 redacted support bundle | IMPLEMENTED+VERIFIABLE | `lib_operations_support_bundle.rs` produces the redacted bundle; `lib_redaction.rs` owns redaction. |
| BPT-056 construction confined to absent/corrupt | IMPLEMENTED+VERIFIABLE | `verified_construction_reason` (`engine.rs:778-817`) — owner-only authorization ignores caller flags: full construction only when the store file is missing or unreadable/`Ok(None)` envelope (unrecoverable corruption); readable-but-incompatible stores are preserved and rejected typed (`blueprint_generation_incompatible` for graph-schema/provider mismatch, `blueprint_schema_unsupported` for newer-than-supported store schema); older supported schemas migrate via backup-preserving `open_store`. Test rows: `native_engine.rs::readable_incompatible_generation_is_preserved_and_rejected_typed`, `valid_graph_ordinary_build_records_zero_additional_full_constructions`. |
| BPT-057 poisoned manifest/plugin refusal | IMPLEMENTED+VERIFIABLE | `lib_update_manifest::validate_update_manifest` + `verify_signed_manifest` + provider admission (`providers/mod.rs`) refuse malformed/unsigned/untrusted manifests and plugins before acceptance. |
| BPT-058 update delegation + compatibility report | GAP-REPAIRED | `execute_update` previously ran an independent transaction (live-store backup + staged artifact copy + explicit deferred-swap omission). Now verification ends in a `delegated: membrane_installer` verdict plus `graphCompatibility` — a read-only store probe (`graph_compatibility`, `lib_operations_update.rs`) reporting `missing`/`unreadable`/`unsupported_newer_schema`/`incompatible`/`migration_required`/`compatible` without opening writable, migrating, or constructing. `update check` also reports compatibility. No backup/stage/swap writes remain in the Blueprint path. Installer-side consumption is external — see PATCH REQUESTS. |
| BPT-059 explorer owned shell | IMPLEMENTED-UNVERIFIED | `lib_explorer_static.rs`/`lib_explorer_layout.rs` + `lib_http_server.rs` implement the explorer shell and routes; the installed/consumer shell boundary was not exercised in this lane. |
| BPT-060 secret-egress prevention | IMPLEMENTED+VERIFIABLE | `lib_redaction.rs` redacts secrets on Blueprint operational surfaces; `lib_cli_mcp.rs` applies the same redaction on the MCP surface. |
| BPT-061 installer trust admission before transition | GAP-REPAIRED | Candidate verification retained and hardened: unsigned/untrusted-key/checksum-mismatch/platform-mismatch/identity-mismatch/downgrade candidates all refuse with typed reasons before any delegation verdict; an `artifactDir` without a verified manifest now refuses `artifact_manifest_unverified` (previously it was staged unconditionally). The signed-manifest + matching-artifact evidence travels inside the delegated verdict for the installer's trust admission. |
| BPT-062 rollback via installer, receipt-bound | GAP-REPAIRED | `handle_rollback` no longer removes/copies app trees. It verifies the receipt end-to-end — self-consistency, confinement, and `validate_rollback_binding` digest-matching both current and prior trees — then returns `delegated` + `verifiedBinding` (current/prior app digests, package versions, prior dir). The restore transaction belongs to the canonical installer. Test row updated: `parity_cli_update.rs::rollback_with_consistent_receipt_verifies_binding_and_delegates` proves the live tree is untouched. |
| BPT-063 release-archive recognition/handoff | GAP-REPAIRED | `apply_local_artifact` keeps release-archive recognition — manifest shape, Ed25519 signature against the embedded trust root, artifact platform/arch selection, package identity, and tree-digest match — and now hands the verified artifact (name/digest/platform/arch) to the installer verdict instead of staging/swapping it. |
| BPT-064 loopback-only explorer listener | IMPLEMENTED+VERIFIABLE | `lib_http_server.rs` binds explorer listeners to `127.0.0.1` on an ephemeral port only. |
| BPT-065 BP001 detection | IMPLEMENTED+VERIFIABLE | `findings.rs:90-123` emits BP001 when a resolved module does not export the imported symbol; `lib_findings_specifier.rs` documents the closed negative-finding surface. |
| BPT-066 BP002 detection | IMPLEMENTED+VERIFIABLE | `findings.rs:81` emits BP002 when a specifier resolves to neither repository file nor package. |
| BPT-067 BP003 detection | IMPLEMENTED+VERIFIABLE | `findings.rs:90` emits BP003 for barrel re-exports whose target binding is absent. |
| BPT-068 explorer session token | IMPLEMENTED+VERIFIABLE | `lib_http_server.rs` requires an unguessable in-memory session token on every Explorer API request. |
| BPT-069 explorer GET-only | IMPLEMENTED+VERIFIABLE | `lib_http_server.rs` rejects every non-GET request before route dispatch. |
| BPT-070 no browser child with session URL | IMPLEMENTED+VERIFIABLE | Explorer launch (`lib_explorer_static.rs`/`cli.rs`) exposes the session URL only to the caller; no browser child is spawned carrying URL or token. |
| BPT-071 typed cross-language bridge evidence | IMPLEMENTED+VERIFIABLE | `providers/bridges.rs` emits typed source-addressed bridge evidence only for explicit FFI/JNI/cgo/gRPC/IPC surfaces. |
| BPT-072 reads never construct/update | IMPLEMENTED+VERIFIABLE | All read arms open `open_store_read_only` (`engine.rs:103-148`, `status` at `:1002`, `architecture_changes` at `:1120`); status reads the envelope without loading graph rows; findings/query/recall consume persisted generation data only — no scan, reparse, process spawn for construction, or network. Treeish history (`lib_application_snapshots`) builds only detached historical worktrees outside the live store. The BPT-019 repair adds a read-only git probe to reads — it observes, never writes. |

## CHANGES

Owned-path edits (4 files, two bounded repairs):

- `engine/crates/membrane-blueprint/src/engine.rs` (BPT-019 repair)
  - Replaced `sealed_freshness_receipt` with `generation_freshness_receipt`
    (`:1172`): the receipt now compares the sealed basis against a bounded
    live VCS observation (`freshness_observation::observe_current_vcs_state`
    → `git rev-parse HEAD` + `git status --porcelain=v1 -z`, each time- and
    output-bounded) instead of fabricating `current` from the sealed basis.
    `changed_paths_for_freshness` enumerates stale paths lazily only on a
    `changed_since_generation` verdict, confined by an optional
    `indexed_paths` set to files the generation actually indexed.
    `observationMode`/`liveSourceObserved` are now truthful labels
    (`live_observation` vs `sealed_generation` when git is unavailable).
  - Query arm (`:134-161`): builds the receipt before dispatch, then
    `apply_freshness_suppression` (`:1218`) injects `staleSourcePaths` and
    `staleWholeGeneration` into the request so `query.rs`'s existing
    stale-row suppression (`:464-479`) is driven by real freshness evidence.
    This is the previously-missing production wiring for the suppression
    inputs.
  - `status` (`:1043`) and `architecture_changes` (`:1156-1158`) callsites
    updated. Reads remain strictly non-mutating — the probe is read-only.

- `engine/crates/membrane-blueprint/src/lib_operations_update.rs`
  (BPT-058/061/062/063 repair)
  - Module doc now states the ownership boundary: the canonical Membrane
    installer owns every update/rollback/release transaction including the
    atomic app/store swap and journal recovery.
  - `execute_update`: removed `backup_store` + `.agent/update-staged`
    staging + the deferred-swap omission. Verified candidates now return
    `{ok, delegated, delegate:"membrane_installer", artifact:{name,digest,
    platform,arch}, graphCompatibility}`. An `artifactDir` without a
    verified manifest refuses `artifact_manifest_unverified` — no unverified
    artifact is staged. Dry-run reports `graphCompatibility`.
  - `apply_local_artifact`: retains the full verification battery
    (confinement, manifest shape, Ed25519 signature vs embedded trust root,
    platform/arch, package identity, tree digest, downgrade) and returns the
    verified artifact as a delegated verdict — no stage/copy/remove.
  - `handle_rollback`: retains receipt self-consistency +
    `validate_rollback_binding` (digest-bound, read-only), returns
    `delegated` + `verifiedBinding`; the restore transaction is the
    installer's.
  - New `graph_compatibility(root)` — read-only probe mirroring
    `verified_construction_reason`'s eligibility criteria (missing /
    unreadable / `blueprint_schema_unsupported` /
    `blueprint_generation_incompatible` / `migration_required` /
    `compatible` + `generationId`), never opens writable, never migrates,
    never constructs. Reported on `check`, `apply`, update, and rollback
    paths as installer admission evidence.
  - `lib_update_apply::{backup_store,copy_recursive}` remain exported
    primitives for their own parity tests but are no longer invoked by the
    update operation.

- `engine/crates/membrane-blueprint/tests/native_engine.rs`
  - Replaced `status_uses_bounded_freshness_without_full_construction_receipt`
    (which asserted the defective sealed-fresh contract) with
    `status_observes_live_source_without_full_construction_receipt`: a real
    `git init` fixture asserts `fresh` on a clean worktree and `stale` /
    `changed_since_generation` with `staleSources.paths == ["main.rs"]`
    after an unrefreshed edit, plus unchanged generation identity and
    full-construction receipt (reads non-mutating). Added
    `status_without_vcs_reports_unavailable_instead_of_old_fresh` for the
    non-git floor.

- `engine/crates/membrane-blueprint/tests/parity_cli_update.rs`
  - `rollback_with_consistent_receipt_restores_prior_app_dir` →
    `rollback_with_consistent_receipt_verifies_binding_and_delegates`
    (asserts `delegated`/`verifiedBinding` and that the live app dir is
    untouched).
  - `update_without_manifest_backs_up_and_stages_artifact` →
    `update_without_verified_manifest_refuses_artifact_without_mutation`
    (asserts `artifact_manifest_unverified`, no staging dir).
  - Added `update_delegates_transaction_and_reports_graph_compatibility`.

## PATCH REQUESTS

1. `engine/crates/membrane-runtime/src/freshness.rs`
   (`read_blueprint_status_until`, ~:859-926) — the Hub-less status adapter
   fabricates `overlay.entries: []`, `stable: true`, and a synthetic
   `commitDistance` (`0`/`2`). With this lane's repair, `state` now reports
   `stale` honestly so the epoch verdict is no longer old-fresh, but the
   overlay still carries no per-file evidence. Populate
   `result.overlay.entries`/`commitDistance` from the native response's
   `freshnessReceipt.staleSources.paths` (and `current.vcs_revision` /
   `generation.indexed_revision` for real distance) instead of the fabricated
   empty overlay, so downstream consumers get path-level staleness detail.
2. Same file, daemon-IPC path (`read_blueprint_status_at`/`read_epoch`
   ~:659-723) — it expects `result.repository.revision`,
   `result.manifest.{generationId,baseCommit,manifestDigest}`, and
   `result.overlay` from the Blueprint daemon status response. The native
   `Operation::Status` result emits `state`/`generationId`/`sourceHash`/
   `freshnessReceipt` — a different shape. Verify the resident
   daemon's status endpoint (membrane-runtime `service.rs`/serve layer, not
   this lane) synthesizes the expected shape or extend the native status
   result — otherwise Hub-resident freshness reads will report
   `commit_epoch_missing`/indeterminate.
3. Canonical installer (external to this lane): consume the
   `delegated`/`verifiedBinding`/`graphCompatibility` verdicts emitted by
   `Operation::Update` and perform the atomic app/store swap, interrupted-
   transaction journal recovery, and receipt-bound restore there. Blueprint
   now refuses to perform those mutations itself, so without the installer
   consumer no update actually lands — by design, but the seam must be wired
   before the release path is exercised.

## BLOCKERS

- Rust builds/tests are coordinator- or CI-owned (no cargo/rightkit in this
  lane): the repaired code and updated tests are verified by inspection and
  type-level consistency only; `cargo test -p membrane-blueprint` must run in
  the validation environment before any RELEASED claim.
- BPT-019 repair changes observable status behavior (`fresh` →
  `stale`/`unavailable` where drift or absent git previously read `fresh` via
  the sealed shortcut). This is the committed-atom-required behavior; any
  consumer pinned to the old fabricated `fresh` must be re-checked (the
  runtime adapter in PATCH REQUEST 1 is the known one).
- Installer-side consumption of the delegated update/rollback verdicts is
  not proven — required before BPT-058/061/062/063 can reach RELEASED.
- Installed-consumer-boundary acceptance (per the implementation contract)
  was not executed in this lane for any atom.
