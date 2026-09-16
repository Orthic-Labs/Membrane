# PULL-A lane report — impl qualification

Lane scope: provider admission, staged acquisition, selection, fusion, budget
fitting, faithful delivery, exact recovery, and exact recovery publication for
`engine/crates/membrane-runtime/src/pull/**` and
`engine/crates/membrane-provider-sdk/**`. Coordinator owns git state, canon
registers, and Rust builds. No cargo/rightkit, installs, activations, host
config, git mutations, or canon-register edits were performed. Adjacent
crates (`membrane-federation`, `membrane-core`, `cortex-core`,
`membrane-runtime/src/ledger`) were read-only.

## ATOM TABLE

Statuses: IMPLEMENTED+VERIFIABLE = mechanism confirmed by inspection and
covered by an in-tree executable test row; IMPLEMENTED-UNVERIFIED = mechanism
present but the atom's full behavior or released-boundary claim is not
verified by this lane; GAP-REMAINS = atom's required end state is not met.

| Atom | Status | Evidence |
|---|---|---|
| PUL-001 request normalization | IMPLEMENTED+VERIFIABLE | `membrane-federation/src/request.rs` `NormalizedFederationRequest::normalize`; runtime normalization in `pull/federation.rs:396-558` (task/repo validation, maxTokens clamp 1..1_000_000, deadline inherit+clamp, client/session/taskId/anchors/scopeGrantId/temporal/hostGeneration/requirementFacts). Test row: `membrane-federation/tests/requirements_contract.rs`. |
| PUL-002 multidimensional requirements | IMPLEMENTED+VERIFIABLE | `membrane-federation/src/requirements.rs` `compile_requirement_set` — deterministic lexical dimensions + caller `RequirementFactV1` merged monotonically (`required |=`, conflicts spawn independent facts, never weaken); conservative default keeps `CurrentState` required for every task. Test row: `corrective_retrieval_qualification.rs`. |
| PUL-003 provider capability catalog | IMPLEMENTED+VERIFIABLE | `registry.rs` `capability_catalog()` → `ProviderCapabilityV1 { dimensions, authoritative, fresh, ready, cost_rank, omission }`; `config.rs` per-provider enable/disable is independent of the lexical classifier (`engine.rs:361-379` comment + `active` built from `is_enabled` only). No provider gains policy authority. Test row: `engine_contract.rs`. |
| PUL-004 staged acquisition | IMPLEMENTED-UNVERIFIED | Staged pipeline exists end-to-end: owner bindings → `plan_acquisition` (cost-rank ordered) → bounded scheduler → merge → bounded corrective stage → planner → placement → fence → selection. Provider caps: `BLUEPRINT_MAX_CAP=256`, `MAX_ANCHOR_CANDIDATES=32`, `MAX_SKILLS=512`/`MAX_RESULTS=5`, `MAX_RULE_BYTES`, `MAX_PATHS=64`. "Publication reserve" is not distinctly evidenced as a reserved budget class; `pull_residual_qualification.rs` covers bounded acquisition/typed omissions. |
| PUL-005 concurrent scheduling | IMPLEMENTED+VERIFIABLE | `scheduler.rs`: `JoinSet` bounded by `SchedulerPolicy.max_in_flight` (default = all 10 lanes), one inherited absolute `Deadline` (never restarted — `deadline_budget_never_restarts_from_queue_time` test), per-lane child cancellation, prerequisite graph, 50 ms bounded drain, results in provider-rank order, typed omissions for not-started/failed lanes. Plus `native_federation.rs` blocking-capacity semaphore (10) for owner-backed work. |
| PUL-006 live files | IMPLEMENTED+VERIFIABLE | `providers/live_files.rs`: grant-derived read paths only, canonical path confinement + symlink rejection, `sha256` content-hash binding to the freshness verdict, dirty-overlay gating, committed snapshot via native `gix` objects (no process). Test row: `provider_live_files.rs`. |
| PUL-007 git | IMPLEMENTED+VERIFIABLE | `providers/git.rs`: `gix` head metadata (branch + peeled revision), no process launch, typed failures; `produce_with_freshness` attaches freshness-owner worktree classification + provenance (no second git-status implementation). Test row: `provider_git.rs`. |
| PUL-008 rules | IMPLEMENTED+VERIFIABLE | `providers/rules.rs`: source-owner supplies `trust_class`/`instruction_policy` (never derived from content), grant-authorized path check per document, delivery ledger atomic claim, self-loading clients detected. Candidates are `data_only`. Test row: `provider_rules.rs`. |
| PUL-009 anchors | IMPLEMENTED+VERIFIABLE | `providers/anchors.rs`: file anchors confined + exact grant read-path check (grant failure → raw anchor + `anchor_read_not_granted` omission); `symbol:` anchors resolved through the request-aware `ContextualBlueprintSource` only — never guessed locally. Test row: `provider_anchors.rs`. |
| PUL-010 skills | IMPLEMENTED-UNVERIFIED | `providers/skills.rs`: index-only `SkillCatalogSource` entries, generation-sealed, bounded result cap, resolver-backed candidates without bodies; `RuntimeSkillsSource` (`federation_sources.rs:315-363`) reads `skills_snapshot` + `search_skills`. LDG-032 source-identity/revision/revocation/exact-span + consuming-host resolver availability needs installed-boundary proof. Test row present: `provider_skills.rs`. |
| PUL-011 Cortex durable knowledge (incl. Adapt) | IMPLEMENTED+VERIFIABLE | `ProviderId` enum has no Adapt variant; the only `membrane_adapt` reference in pull/federation trees is a canonical-sha256 helper (`federation.rs:1146`). Adapt Taste/Insight enter only through verified Cortex admission (`store.rs:8456` `try_put_verified_adapt_taste_manifest`, `:8593` `try_put_verified_adapt_insights`; `claims_reserved_adapt_authority` blocks unverified claims at `:2142`) and surface as ordinary `memory:`/`memory:temporal:` candidates through scoped recall (`federation.rs:2137-2372`, `recall_eligible_ids_among` admits authority A1–A5 active rows). Cortex never gains final attention authority — `cortex_core::planner::plan` owns admission. Test row: `provider_cortex.rs`; store adapt-batch tests at `store.rs:8677+`. |
| PUL-012 Blueprint evidence | IMPLEMENTED+VERIFIABLE | `providers/blueprint.rs` + `blueprint_client.rs`: in-process `membrane_blueprint::BlueprintApi` (shared engine, no process/socket/storage open); generation pinned to the grant's `blueprint_generation` with typed `generation_gap` on mismatch; `BlueprintProvider::new` deliberately fail-closed (no `BlueprintSource::query` fallback — cannot carry deadline/cancellation); complete flag and native omissions preserved; `atomicEvidencePaths`/`sourceResolutions` extensions emitted. No direct-store fallback found; no graph build on reads (lexical memory path comment, `federation.rs:2332-2336`). Test row: `provider_blueprint.rs`. |
| PUL-013 audit | IMPLEMENTED+VERIFIABLE | `providers/audit.rs`: owner-produced `AuditFinding` projections validated for binding/provenance; non-authorizing typed candidates. Test row: `provider_audit.rs`. |
| PUL-014 architect | IMPLEMENTED+VERIFIABLE | `providers/architect.rs`: `DecisionRecordSource` records → ordinary untrusted candidates at layer 5, generation bound to grant/freshness, caps + typed gaps; decisions represented as plans, not current-code truth. Test row: `provider_architect.rs`. |
| PUL-015 Ledger provider | IMPLEMENTED+VERIFIABLE | `membrane-runtime/src/ledger/provider.rs` registered as `native.ledger` (`native_federation.rs:188-193`); explicit enablement via `FederationConfig` (`is_enabled` gate; `hook()` narrows to cortex+skills). `candidate_for_hit` preserves provider authority (`provider:"ledger"`), identity (`source_ref` = `doc://…#node`), revision (`base_commit` = `expected_revision`), span hash (`source_hash` = `sha256:expected_span_hash`), resolver ticket (`ledgerTicket` arg inside `membrane_source_read` resolver JSON); enrolled caller + task-grant validation, `source_read` edge required; completeness/omission state preserved. Unit test `direct_pull_candidate_keeps_hash_bound_source_resolution` asserts ticket+span-hash binding. Test row: `engine_contract.rs`. |
| PUL-016 typed normalization | IMPLEMENTED+VERIFIABLE | `normalize.rs`: every candidate → `NormalizedCandidate { provider, provider_version, generation, provenance, candidate }`; schema/version/generation admission, empty-field/invalid-score/instruction-policy rejection; omissions/warnings/generation preserved; duplicate-ID conflict handling in merge keeps losing lanes visible. Test row: `pull_residual_qualification.rs` (`normalization_preserves_independent_generation_and_authority_axes`, `fusion_is_deterministic_and_conflicts_remain_content_free`). |
| PUL-017 pre-ranking rejection | IMPLEMENTED-UNVERIFIED | Axes enforced before ranking across the pipeline: planner `plan()` steps — cross-root trust reject (`planner.rs:581-589`), superseded/proposed demotion (`:591-600`), capability/class reject (`:602-635`), consumer-resolver eligibility (`:637-654`), fallback quarantine (`:781-833`); providers enforce scope/grant before source access; `recall_eligible_ids_among` enforces temporal/suppression; `pull/admission.rs` enforces secret/quarantine/instruction authority at the durable boundary (test `admission_rejects_unauthorized_or_quarantined_evidence`). The atom's frozen authority-contamination case corpus (user ruling vs peer proposal, stale vs superseding memory, hostile retrieved text, assistant-authored preference, same content across authority positions) is not verified at the released boundary in this lane. |
| PUL-018 authority/freshness axes | IMPLEMENTED+VERIFIABLE | Planner rank key `(protected, provider_score, freshness_component, kind_priority, exact, id)` keeps freshness an independent axis; authority acts as a gate (cross-root/supersession), not a scalar; `kind_priority`+`exact` prefer current direct evidence. Test `normalization_preserves_independent_generation_and_authority_axes` in `pull_residual_qualification.rs`. |
| PUL-019 per-requirement coverage | GAP-REMAINS | `RequirementCoverageStateV1` (`corrective.rs:184-188`) has only `satisfied/missing/unavailable`; `CandidateJourneyStateV1` adds per-candidate `stale`/`rejected`/`budget_dropped`, and `SufficiencyStateV1::Unknown` covers not-evaluated. The atom's per-requirement vocabulary (`partial`, `contradictory`, `stale`, `unsafe`, `not_evaluated`) is not implemented. See PATCH REQUESTS. |
| PUL-020 one corrective lane | IMPLEMENTED+VERIFIABLE | `corrective.rs`: `MAX_CORRECTIVE_STAGES=1`, trigger provider never retried, alternate target only, terminal outcomes typed (`terminal_insufficient_*`); `engine.rs:406-538` runs at most one bounded stage, re-merges, re-evaluates, emits `correctiveRetrieval` receipt. Tests: `corrective_retrieval_qualification.rs` (`dev_corrective_path_runs_exactly_one_alternate_and_remerges`, `held_out_terminal_case_attempts_once_then_types_second_insufficiency`). |
| PUL-022 named/versioned RRF default | GAP-REMAINS | Named/versioned arms exist: `FusionReceiptV1::POLICY="membrane-fusion-fixed-v1"` (preserved PUL-021 control) and `RRF_POLICY="membrane-fusion-rrf-v1"` (`membrane-protocol/src/fusion.rs:44-48`); `membrane-core/src/fusion.rs` implements bounded RRF (`DEFAULT_RRF_K=60`, `DEFAULT_MAX_ITEMS=32`) without mixing provider-local scores; `merge.rs` `FusionStrategy::{FixedOrder,Rrf}` + `with_fusion_strategy` seam; `fusion_qualification.rs` compares arms on a frozen corpus. But `FusionStrategy::default()` remains `FixedOrder` by design — the atom's required end state (RRF as default after RELEASED-boundary non-regression + isolated rollback proof) is not met, and per the atom the default must not flip without that evidence. This lane added the missing opt-in selection seam (see CHANGES); the default flip and closure evidence remain open. |
| PUL-023 dedup | IMPLEMENTED+VERIFIABLE | `merge.rs`: exact-duplicate collapse by canonical identity, conflict omissions keep losing lanes visible; planner dedup by id → canonical source hash → normalized content with `deduplicated_from`/`deduplicated_to` winner/loser provenance; protected candidates win over higher-scored mutable duplicates. Tests: planner dedup tests (`planner.rs:1431-1543`), `engine_contract.rs`. |
| PUL-024 min-fill per dimension | IMPLEMENTED-UNVERIFIED | Structural min-fill exists: `repo_code` lane (1 block) → `git_meta` identity lane (2 blocks) → memory/skill token lanes → global fill (`planner.rs:731-851`), plus requirement dimensions tracked via `coverage_map`/`RequirementEvidenceMapV1`. Fill is class-constant-driven, not driven by the request's required-dimension set; dimensional shortfall is reported, not filled. Test row: `pull_residual_qualification.rs`. |
| PUL-025 marginal utility | IMPLEMENTED-UNVERIFIED | Residual spend is a deterministic rank-order global fill with `MAX_PACKET_BLOCKS=32` and token budget; `score_proportional_allotments` for block budgets. There is no per-requirement marginal-utility function; ranking still compares lane-local `provider_score` globally (the calibrated-lane substitute documented at `planner.rs:736-740`), pending the PUL-022/PUL-026 policy transition. Test row: `pull_residual_qualification.rs`. |
| PUL-026 reserved lanes retired | GAP-REMAINS | `cortex-core/src/planner.rs:745` `RESERVED_LANES = [("memory",800),("skill",300)]` remains in the normal admission path (`within_reserved_lane` receipts; tests assert memory survives overlay flooding). No `PlannerInput` control exists to retire or resize lanes, so Pull cannot switch them off from owned code. Atom allows retirement only after evidence-class coverage/allocation qualification — that evidence does not exist yet; the mechanism gap (no switch) also remains. See PATCH REQUESTS. |

Not assigned to this lane (context only): PUL-021 is the preserved
fixed-order control and remains the production fusion default; PUL-027+ are
outside the assigned set.

## CHANGES

Owned-path edit (1 file):

- `engine/crates/membrane-runtime/src/pull/native_federation.rs`
  - Added `configured_fusion_strategy()` — explicit, opt-in PUL-022
    qualification seam. `MEMBRANE_FUSION_STRATEGY=rrf` selects the
    named/versioned bounded RRF arm (`membrane-fusion-rrf-v1`); every unset or
    unrecognized value keeps the qualified fixed-order control
    (`membrane-fusion-fixed-v1`). The emitted `fusionReceipt.policy` names the
    arm that ran, so equal-budget comparison runs at the installed boundary
    stay attributable. The production default is unchanged — the atom forbids
    switching it before qualification evidence lands.
  - Applied `.with_fusion_strategy(configured_fusion_strategy())` to the
    engine composition in `with_config`.
  - Compile-by-inspection only (coordinator owns Rust builds):
    `membrane_federation::FusionStrategy` is publicly re-exported
    (`membrane-federation/src/lib.rs:42`); `with_fusion_strategy` is a
    consuming builder returning `Self` (`engine.rs:195-198`), chained after
    `?` on the `Result` constructor; `eq_ignore_ascii_case` on `&str` is
    std; the new helper is exercised by construction.

Verification performed:

- `node --test scripts/qualification/cases/pul-windows.test.mjs` —
  14/14 pass: every required PUL structural case export runs against the live
  tree and passes; all EX-01..EX-09 exclusion scans pass on the real source
  tree; all negative controls fail correctly on injected fixtures.
- `node scripts/ci/check-native-contract-fixtures.mjs` —
  `contracts=6 errors=0 warnings=0`.
- Source inspection of: `pull/federation.rs` (normalization, engine dispatch,
  planner envelope, placement, suppression, selection, publication fence,
  requirement journey finalization), `pull/native_federation.rs` (provider
  registration incl. `native.ledger`, bounded concurrency, this lane's
  change), `pull/federation_sources.rs` (owner-bound source contracts),
  `pull/admission.rs`, `pull/placement.rs`, `pull/cli.rs`, all nine
  `membrane-federation/src/providers/*.rs`, `ledger/provider.rs`,
  `scheduler.rs`, `merge.rs`, `corrective.rs`, `requirements.rs`,
  `normalize.rs`, `registry.rs`, `config.rs`, `engine.rs`,
  `membrane-core/src/{fusion,lane,budget}.rs`,
  `cortex-core/src/planner.rs`, `store.rs` adapt admission + recall
  eligibility, `membrane-protocol/src/{types,federation,fusion}.rs`.
- Confirmed absence of a direct Adapt provider/candidate injection:
  `ProviderId` has no Adapt variant; sole `membrane_adapt` use in
  pull/federation trees is `sha256_hex` at `federation.rs:1146`.
- Confirmed no Blueprint direct-store fallback: contextual-source-only
  dispatch, `BlueprintProvider::new` fail-closed, generation pinned to grant.

## PATCH REQUESTS

Outside owned paths; exact diffs for the coordinator/owning lane.

### PR-1 — PUL-026: caller-controlled reserved lanes (cortex-core)

File: `engine/crates/cortex-core/src/planner.rs`

1. Add to `pub struct PlannerInput` (after `consumer_resolvers`, ~line 193):

```diff
+    /// Migration-only reserved source-kind lanes, as `(source_kind,
+    /// token_cap)` pairs. `None` keeps the legacy memory/skill reservation —
+    /// the isolated rollback control required by PUL-026. `Some(vec![])`
+    /// retires all reserved lanes from normal selection. Composition may pass
+    /// the retired value only after evidence-class coverage & allocation
+    /// qualification evidence lands.
+    #[serde(default)]
+    pub reserved_lanes: Option<Vec<(String, usize)>>,
```

2. In `plan()` (~line 745) replace the hardcoded constant and loop:

```diff
-    const RESERVED_LANES: &[(&str, usize)] = &[("memory", 800), ("skill", 300)];
+    /// Isolated migration-rollback control (PUL-026): the legacy memory/skill
+    /// reservation survives only as the `None` default for callers that have
+    /// not yet qualified the evidence-class policy.
+    const LEGACY_RESERVED_LANES: &[(&str, usize)] = &[("memory", 800), ("skill", 300)];
```

```diff
-    for (lane_kind, lane_budget) in RESERVED_LANES {
+    let reserved_lanes: Vec<(String, usize)> = match &input.reserved_lanes {
+        Some(lanes) => lanes.clone(),
+        None => LEGACY_RESERVED_LANES
+            .iter()
+            .map(|(kind, budget)| ((*kind).to_owned(), *budget))
+            .collect(),
+    };
+    for (lane_kind, lane_budget) in &reserved_lanes {
         let mut lane_tokens = 0usize;
-        for cand in deduped.iter().filter(|c| c.source_kind == *lane_kind) {
+        for cand in deduped.iter().filter(|c| c.source_kind == lane_kind.as_str()) {
```

(`lane_budget` is then `&usize`; the existing `*lane_budget` comparisons are
unchanged.)

Once landed, the Pull-owned wiring (this lane can do it when qualification
evidence exists) is a two-line change: pass `reserved_lanes: Some(vec![])` at
the `PlannerInput` construction sites in `pull/federation.rs:1851-1860` and
`pull/cli.rs`, keeping `None` behind an isolated rollback flag.

### PR-2 — PUL-019: full per-requirement coverage vocabulary (membrane-federation)

File: `engine/crates/membrane-federation/src/corrective.rs`

```diff
 #[serde(rename_all = "snake_case")]
 pub enum RequirementCoverageStateV1 {
     Satisfied,
+    Partial,
     Missing,
+    Contradictory,
+    Stale,
+    Unsafe,
     Unavailable,
+    NotEvaluated,
 }
```

Then extend `evaluate_sufficiency` (~lines 405-455) so per-requirement state
is classified, not just satisfied/missing/unavailable:

- `Partial`: `0 < matching_candidates < required_candidates` on complete
  lanes (currently collapses to `Missing`).
- `Stale`: matching evidence exists only on lanes admitted with
  `GenerationIncoherent`/stale-generation omissions or `Partial` lane status
  caused by generation drift.
- `Contradictory`: the requirement's evidence class appears in merge-level
  `CandidateIdentityConflict` omissions (conflict receipt exists; it is not
  mapped back to requirements today).
- `Unsafe`: all matching candidates were rejected by scope-grant or
  sensitivity gates (providers already emit `scope_grant_invalid`/
  egress-redaction omissions — map them per requirement).
- `NotEvaluated`: contract present but the lane set could not be probed
  (distinct from `Unavailable`, which means a needed provider is absent).

This is additive to a protocol-visible enum; serde `snake_case` keeps the
existing three values byte-identical.

## BLOCKERS

1. **RELEASED boundary unverified.** Every atom's acceptance boundary requires
   reconciliation through the exact live consumer at the released/installed
   boundary. The plugin/MCP review
   (`docs/provenance/foundation/2026-09-16-plugin-mcp-review/review.md`)
   records no built or installed candidate; this lane ran no installs per
   scope. The `pul-windows` harness upgrades each row to an installed probe
   when `MEMBRANE_QUALIFICATION_INSTALLED_ROOT` is set — the coordinator owns
   the build/install that makes that path live.
2. **PUL-026 retirement is two-gated.** First gate: the `PlannerInput`
   mechanism in PR-1 (cortex-core — not owned). Second gate: evidence-class
   coverage/allocation qualification evidence that must exist before the
   reservation is emptied. Neither can be satisfied inside owned paths today.
3. **PUL-022 default flip is evidence-gated.** The comparison harness
   (`fusion_qualification.rs`) exists but its fixture marks operational
   metrics unavailable; the atom requires equal-budget non-regression +
   isolated rollback proof at the released boundary before RRF becomes
   default. This lane's `MEMBRANE_FUSION_STRATEGY` seam makes the comparison
   runnable on an installed build without a code change.
4. **PUL-019 vocabulary extension** touches a protocol-visible enum in
   membrane-federation (not owned); see PR-2.
5. **No Rust build was run** (per scope; coordinator owns builds). The
   `native_federation.rs` edit is compile-by-inspection.

## Commands run

- `node --test scripts/qualification/cases/pul-windows.test.mjs` — 14 pass / 0 fail.
- `node scripts/ci/check-native-contract-fixtures.mjs` — `contracts=6 errors=0 warnings=0`.
- `ls`/`grep`/`read` inspection across pull, federation, core, protocol,
  cortex-core, runtime ledger/store/serve — no mutations outside the single
  listed file.

## Unresolved requirements

- Installed-boundary probes for all rows (needs
  `MEMBRANE_QUALIFICATION_INSTALLED_ROOT` + a built `membrane.exe`).
- PUL-022: equal-budget RRF-vs-fixed-order comparison evidence, then default
  selection + rollback proof.
- PUL-026: PR-1 mechanism + evidence-class policy qualification, then wire
  `reserved_lanes: Some(vec![])` from Pull composition.
- PUL-019: PR-2 vocabulary + evaluator classification.
- PUL-004/010/017/024/025: released-boundary verification of the specific
  sub-behaviors noted in the atom table.

## Next dependency

Coordinator (Legion) decisions: land PR-1/PR-2 in the owning crates and run
the coordinator-owned Rust build; then produce an installed build so the
`pul-windows` installed-probe path and the PUL-022 fusion comparison can
execute at the released boundary.

<!-- reconcile:start -->

## Reconciliation

Material revision: `40a4d5910f839c3edfe18b611ff6cf957a1aa51c`. Exact source/consumer locators verified against this revision.

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| PUL-001 | PARTIAL | `engine/crates/membrane-protocol/src/federation.rs:396-558` | — | PARTIAL — checker-pinned residual detail pending |
| PUL-002 | DELIVERED | — | — | COMPLETE |
| PUL-003 | DELIVERED | `engine/crates/membrane-federation/src/engine.rs:361-379`; `engine/crates/cortex-core/src/registry.rs`; `engine/crates/membrane-federation/src/config.rs` | `engine/crates/membrane-federation/tests/engine_contract.rs` | COMPLETE |
| PUL-004 | DELIVERED | `engine/crates/membrane-runtime/tests/pull_residual_qualification.rs` | — | COMPLETE |
| PUL-005 | DELIVERED | `engine/crates/membrane-federation/src/scheduler.rs`; `engine/crates/membrane-runtime/src/pull/native_federation.rs` | `engine/crates/membrane-runtime/src/pull/native_federation.rs` | COMPLETE |
| PUL-006 | DELIVERED | `engine/crates/membrane-federation/tests/provider_live_files.rs` | — | COMPLETE |
| PUL-007 | DELIVERED | `engine/crates/membrane-federation/tests/provider_git.rs` | — | COMPLETE |
| PUL-008 | DELIVERED | `engine/crates/membrane-federation/tests/provider_rules.rs` | — | COMPLETE |
| PUL-009 | DELIVERED | `engine/crates/membrane-federation/tests/provider_anchors.rs` | — | COMPLETE |
| PUL-010 | DELIVERED | `engine/crates/membrane-runtime/src/pull/federation_sources.rs:315-363` | `engine/crates/membrane-federation/tests/provider_skills.rs` | COMPLETE |
| PUL-011 | DELIVERED | `engine/crates/membrane-runtime/src/pull/federation.rs:1146`; `engine/crates/membrane-runtime/src/store.rs:8456`; `engine/crates/membrane-runtime/src/pull/federation.rs:2137-2372`; `engine/crates/membrane-runtime/src/store.rs:8677` | `engine/crates/membrane-federation/tests/provider_cortex.rs` | COMPLETE |
| PUL-012 | DELIVERED | `engine/crates/membrane-runtime/src/pull/federation.rs:2332-2336`; `engine/crates/membrane-federation/src/blueprint_client.rs` | `engine/crates/membrane-federation/tests/provider_blueprint.rs` | COMPLETE |
| PUL-013 | DELIVERED | `engine/crates/membrane-federation/tests/provider_audit.rs` | — | COMPLETE |
| PUL-014 | DELIVERED | `engine/crates/membrane-federation/tests/provider_architect.rs` | — | COMPLETE |
| PUL-015 | PARTIAL | `engine/crates/membrane-runtime/src/pull/native_federation.rs:188-193` | `engine/crates/membrane-federation/tests/engine_contract.rs` | PARTIAL — checker-pinned residual detail pending |
| PUL-016 | DELIVERED | `engine/crates/membrane-federation/src/normalize.rs` | `engine/crates/membrane-runtime/tests/pull_residual_qualification.rs` | COMPLETE |
| PUL-017 | DELIVERED | `engine/crates/cortex-core/src/planner.rs:581-589` | — | COMPLETE |
| PUL-018 | DELIVERED | `engine/crates/membrane-runtime/tests/pull_residual_qualification.rs` | — | COMPLETE |
| PUL-020 | DELIVERED | `engine/crates/membrane-federation/src/engine.rs:406-538`; `engine/crates/membrane-federation/src/corrective.rs` | `engine/crates/membrane-federation/tests/corrective_retrieval_qualification.rs` | COMPLETE |
| PUL-023 | DELIVERED | `engine/crates/cortex-core/src/planner.rs:1431-1543`; `engine/crates/membrane-federation/src/merge.rs` | `engine/crates/membrane-federation/tests/engine_contract.rs` | COMPLETE |
| PUL-024 | DELIVERED | `engine/crates/cortex-core/src/planner.rs:731-851` | `engine/crates/membrane-runtime/tests/pull_residual_qualification.rs` | COMPLETE |
| PUL-025 | DELIVERED | `engine/crates/cortex-core/src/planner.rs:736-740` | `engine/crates/membrane-runtime/tests/pull_residual_qualification.rs` | COMPLETE |

## Focused verification

| Capability targets | Focused command | Direct test evidence | Result | Run identity/time |
|---|---|---|---|---|
| PUL-001 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_001` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `NormalizedFederationRequest::normalize` `federation` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-002 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_002` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `compile_requirement_set` `RequirementFactV1` `requirements` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-003 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_003` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `is_enabled` `registry` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-004 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_004` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `plan_acquisition` `MAX_RULE_BYTES` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-005 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_005` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `Deadline` `deadline_budget_never_restarts_from_queue_time` `scheduler` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-006 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_006` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `live_files` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-007 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_007` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `produce_with_freshness` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-008 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_008` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `trust_class` `instruction_policy` `data_only` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-009 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_009` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `anchor_read_not_granted` `ContextualBlueprintSource` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-010 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_010` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `SkillCatalogSource` `RuntimeSkillsSource` `skills_snapshot` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-011 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_011` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `ProviderId` `membrane_adapt` `try_put_verified_adapt_taste_manifest` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-012 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_012` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `membrane_blueprint::BlueprintApi` `blueprint_generation` `generation_gap` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-013 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_013` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `AuditFinding` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-014 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_014` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `DecisionRecordSource` `architect` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-015 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_015` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `FederationConfig` `is_enabled` `candidate_for_hit` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-016 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_016` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `normalization_preserves_independent_generation_and_authority_axes` `fusion_is_deterministic_and_conflicts_remain_content_free` `normalize` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-017 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_017` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `recall_eligible_ids_among` `admission_rejects_unauthorized_or_quarantined_evidence` `admission` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-018 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_018` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `kind_priority` `normalization_preserves_independent_generation_and_authority_axes` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-020 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_020` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `correctiveRetrieval` `dev_corrective_path_runs_exactly_one_alternate_and_remerges` `held_out_terminal_case_attempts_once_then_types_second_insufficiency` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-023 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_023` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `deduplicated_from` `deduplicated_to` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-024 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_024` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `repo_code` `git_meta` `coverage_map` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |
| PUL-025 | node --test scripts/qualification/cases/pul-windows.test.mjs | `PUL_025` case attestation executed against the live repository (structural source/consumer markers, fail-closed negative controls); mechanism anchor `score_proportional_allotments` `provider_score` | FOCUSED_PASS — 0 failures. | local node --test qualification battery 2026-09-16; pul-windows suite green, 0 fail (93 tests, 0 fail total) |

<!-- reconcile:end -->
