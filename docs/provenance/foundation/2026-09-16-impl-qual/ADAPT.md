# ADAPT lane report — impl qualification

Lane scope: evidence-bound mining, reviewed Taste/Insight artifacts,
recurrence and lineage, proposals, outcome interpretation, and the
`adapt_behavioral_review` learner lane for
`engine/crates/membrane-adapt/**`,
`engine/crates/membrane-runtime/src/adapt*.rs`,
`engine/crates/membrane-runtime/src/background_review*.rs`, and the
Adapt-owned MCP/CLI projections. Coordinator owns git state, canon
registers, and Rust builds. No cargo/rightkit, installs, activations,
host config, git mutations, or canon-register edits were performed.
Adjacent crates (`cortex-core`, `cortex-store`, `membrane-protocol`,
`membrane-runtime/src/pull`, `membrane-runtime/src/bin/membrane-daemon.rs`)
were read-only; defects there are reported as PATCH REQUESTS.

Boundary held: Adapt emits proposals and never writes durable truth;
accepted Taste/Insight cross the daemon-owned Cortex admission boundary;
Adapt is not a Pull provider and not a federation candidate source;
missing evidence/providers/joins remain typed unavailable states.

## ATOM TABLE

Statuses: IMPLEMENTED+VERIFIABLE = mechanism confirmed by inspection and
covered by an in-tree executable test row; IMPLEMENTED-UNVERIFIED =
mechanism present but the atom's full released-boundary claim is not
verified by this lane; PARTIAL = committed end state materially open;
GAP-REMAINS = required mechanism absent.

| Atom | Status | Evidence |
|---|---|---|
| ADP-001 transcript normalization | IMPLEMENTED-UNVERIFIED | Canon row DELIVERED; normalization machinery in `membrane-adapt/src/transcript.rs` (role/origin/span/digest/provenance, typed omissions). Not re-audited this lane. |
| ADP-002 external transcript formats | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; `transcript.rs` format adapters plus CodeRight native structured-event projection seam. Not re-audited. |
| ADP-003 Taste candidate binding | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; candidates bind transcript/session/event/span/parser/source digest + act + scope in `membrane-adapt` taste pipeline. Not re-audited. |
| ADP-004 local review / signed adjudication | IMPLEMENTED-UNVERIFIED | Canon FOCUSED_PASS; `cortex-core/src/review.rs` + Adapt gate paths require verified review before acceptance. |
| ADP-005 evidence classification | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; user-authoritative/behavioral/diagnostic/context-only classes enforced before proposal. |
| ADP-006 model proposes wording only | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; deterministic code owns authority/scope/lifecycle/effect/receipts. |
| ADP-007 user evidence only | IMPLEMENTED-UNVERIFIED | Canon FOCUSED_PASS; non-user evidence cannot manufacture Taste authority. |
| ADP-008 class/category distinctness | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; standing/scoped/operational/behavioral-decision classes closed; episodic/unclassified rejected. |
| ADP-009 scope narrowing fail-closed | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; unknown/malformed narrowing across the scope vocabulary fails closed. |
| ADP-010 deterministic precedence | IMPLEMENTED-UNVERIFIED | Canon FOCUSED_PASS; lower authority cannot repeal higher; same-tier conflicts surfaced. |
| ADP-011 counterfactual preservation | PARTIAL | Canon row PARTIAL/CURRENT_INCOMPLETE; rejected-alternative preservation beside correction-derived Taste remains open work. |
| ADP-012 receipted Taste lifecycle | PARTIAL | Canon PARTIAL; lifecycle states exist; full receipted transition coverage incomplete. |
| ADP-013 always-on core compile | IMPLEMENTED-UNVERIFIED | Canon DELIVERED/FOCUSED_PASS; only tiny active root-scoped standing preferences compile into bounded always-on core. |
| ADP-014 applicability before search | IMPLEMENTED-UNVERIFIED | Canon FOCUSED_PASS; structured applicability filter excludes inactive/conflicting/nonmatching before semantic search. |
| ADP-015 applicability/delivery receipts | PARTIAL | Canon PARTIAL; receipts emitted but bounded outcome feedback tied to exact execution identity is incomplete. |
| ADP-016 governed preference edit | PARTIAL | Canon PARTIAL; inspect/edit/narrow/deactivate/supersede/delete/export/import surfaces exist; governed-review completeness open. |
| ADP-017 deterministic failure families | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; failure-family detectors with hard negatives + versioned detector contract. |
| ADP-018 evidence-bound FailureEpisode | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; episodes carry detector, spans, applicability, severity, outcome, honesty limit. |
| ADP-019 recurrence-gated issue formation | PARTIAL | Canon PARTIAL; durable issues form from deterministic recurrence; one-off/lifecycle enforcement partially open. This lane's learner (`learner.rs`) emits recurrence-bound proposals with `MIN_RECURRENCE=2` gating, keeping learner output consistent with this rule. |
| ADP-021 honesty limits only | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; behavioral facts + honesty limits, no root-cause-from-recurrence. Enforced structurally in new `LearnerFindingClass::honesty_limit` (`learner.rs:113-128`) and `HonestyLimitMismatch` validation. |
| ADP-022 proposal kind/effect/target independence | PARTIAL | Canon PARTIAL/STALE; explicit compatibility policy separate from issue/Taste authority partially open. |
| ADP-023 sealed intervention attribution | PARTIAL | Canon PARTIAL; preventability/alternatives/ownership/support/eligibility sealing partially open. |
| ADP-024 evaluator applicability denominator | PARTIAL | Canon PARTIAL; applicable/not-applicable/insufficient-evidence classification exists; denominator exclusion completeness open. |
| ADP-025 mitigation tracking | PARTIAL | Canon PARTIAL; baseline/version/exposure/recurrence/regression/dismissal tracking partially open. |
| ADP-026 sealed meaning/applicability | IMPLEMENTED-UNVERIFIED | Canon DELIVERED/FOCUSED_PASS; `insights/sealed_issue.rs` immutable canonical payload; lifecycle mutates only through receipts. |
| ADP-027 stable semantic IDs + atomic batch | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; meaning+scope-derived IDs, atomic/idempotent batch apply with source/installation receipt. |
| ADP-028 duplicate/cross-semantic rejection | IMPLEMENTED-UNVERIFIED | Canon FOCUSED_PASS; exact-duplicate and cross-semantic grouping rejected; conflicts preserved; abstain path exists. |
| ADP-029 one daemon-owned Cortex boundary | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; consumer rows ADP-I029 COMPLETE (`gates.rs:31-96`, `sealed_issue.rs:329-348`; runtime `store.rs:6712-6778,6789-6889`; `cli.rs:3183-3250`). Verified-admission puts at `store.rs:8456`/`8593`; unverified adapt claims blocked (`claims_reserved_adapt_authority`, `store.rs:2142`). Dry-run validates without write. This lane did not find a second admission owner. Daemon-side Adapt learner drain arm remains a patch request (PATCH REQUESTS item 2) so learner proposals reach the same boundary rather than an unlabeled arm. |
| ADP-030 typed CodeRight observations | PARTIAL | Typed `ExecutionObservationKindV1` covers route/model/tool/write/verification/approval/retry/scope/subagent/artifact/completion/retrieval (`membrane-protocol/src/host_observation.rs`). Two stale names remain — `PushReduction`/`PushRestore` carry Pull reduction/restore facts under retired Push naming (PATCH REQUESTS item 4). Caller transport incomplete per canon note ADP-I030. |
| ADP-031 versioned evaluator outcomes | IMPLEMENTED-UNVERIFIED | `adapt_observations.rs` validates outcome schema, exact execution receipt/case/dataset identities, valid hashes, matching coverage receipt/scope/session/task, coverage state `ran`. Real H7/H9/H10 producers absent per canon ADP-I031 — consumer side is honest and fails closed. |
| ADP-032 context-cost attribution | IMPLEMENTED+VERIFIABLE | Canon DELIVERED/FOCUSED_PASS; ADP-I032 COMPLETE — `membrane-adapt/src/context_cost.rs:156-379,587-820`; measured/inferred/unattributed preserved; cli_api `32-36`; `cli.rs:3356`. |
| ADP-033 regression-case proposal | IMPLEMENTED-UNVERIFIED | `membrane-adapt/src/remediation.rs` `regression_case_proposal` (lines ~1103): converts a reviewed sealed issue into a minimal privacy-safe regression-case proposal addressed to the evaluator surface — gated on `Confirmed`/`Reopened` lifecycle (`IssueNotConfirmed` refusal), caller-authored summary bounded to 2,048 chars (`InvalidCaseSummary` refusal), case text carries issue id + payload sha256 + evidence digests only (no episode bodies/transcript text), sealed with `requires_human_review` effect boundary via `SealedRemediationProposalV1::seal`; the external evaluator owner adopts/runs — Adapt never executes. New unit tests: `regression_case_proposal_requires_confirmed_issue`, `regression_case_proposal_is_privacy_safe_and_deterministic`. |
| ADP-034 separate Taste/Insight queues | IMPLEMENTED-UNVERIFIED | `adapt_service.rs` `inspect_issues` now projects scope, schema/issue identity, family, description, honesty limit, lifecycle, recurrence signature/count/first/last, post-mitigation recurrence, applicability, authority/influence class, confidence/evidence quality, candidate mechanisms, mitigation links, evidence refs/digests, validator receipt, available lifecycle actions, receipt count/recent receipts, update time, payload digest. New `inspect_proposals` projects `membrane_knowledge_proposal` + `cortex_proposal_admission_v1` (review/admission state, timestamps, reviewer, attempts, last error, emission digest, truncated text) and returns typed `proposal_queue_schema_absent` when the queue schema is absent instead of an empty-success. MCP `adapt` "proposals" routing still returns the old null+reason shape until PATCH REQUESTS item 5 lands. Read-only: inspection creates no exposure receipt and mutates nothing (qual rows in `adapt_admin_qualification.rs` assert this). |
| ADP-035 learner inside admitted job | IMPLEMENTED-UNVERIFIED | New `membrane-adapt/src/learner.rs`: deterministic `run_adapt_behavioral_review` over a bounded validated window (≤512 events), emits ≤32 `AdaptLearnerProposalV1` records (contract `adapt.learner-proposal.v1`), content-derived ids (idempotent retries), proposal-only boundary marker, typed `Unavailable{EmptyWindow|InvalidInput}` vs `NoFindings` vs `Proposals`, self-validation before emission, no model/usage claim, cursor untouched. New `AdaptBehavioralReviewProvider` + `AdaptFirstPartySemanticReviewProvider` dispatcher in `adapt_service.rs` route Adapt jobs to the learner and Cortex jobs to the deterministic analyzer — neither impersonates the other; unsupported kinds return typed blocked. Wiring gaps in non-owned files: executor arm + JSONL sink kind (PATCH 1), Cortex drain arm (PATCH 2), daemon provider registration (PATCH 3). Cursor still advances only after successful proposal submission in the executor. |
| ADP-036 exact-effectiveness joins | IMPLEMENTED-UNVERIFIED | `adapt_effectiveness.rs` requires exact asset-content + loaded-representation digests; absent exact host-loaded-representation acknowledgement keeps effectiveness `null`/unavailable; no cross-version co-aggregation. Final host ack remains absent per ADP-I036 — honestly unavailable, not fabricated. |
| ADP-038 detector coverage receipts | IMPLEMENTED+VERIFIABLE | Canon DELIVERED; `adapt.detector-coverage.v2` persists family/catalog/input identity, input digest, typed `Ran|Skipped|Unavailable|Failed`, missing fields, findings, honesty limit (`adapt_observations.rs`, `adapt_efficiency.rs`). |
| ADP-040 coverage→episode/outcome join | IMPLEMENTED-UNVERIFIED | `adapt_observations.rs` joins coverage to evaluator, dataset/case, exact task/session; `h4_to_h6_exact_execution_episode_binding` and `exact_loaded_exposure_binding` are explicit typed fields and effectiveness stays null until they exist. |
| ADP-042 read-only lineage + absent-host gaps | IMPLEMENTED+VERIFIABLE | Canon DELIVERED/FOCUSED_PASS; ADP-I042 COMPLETE — `membrane-adapt/src/lineage.rs:275,308-312,413-422,445,501-512` → `cli_api.rs:165-177` → `cli.rs:2998`; typed absent-host gaps. |
| ADP-053 no-progress model loop | IMPLEMENTED-UNVERIFIED | Canon DELIVERED; detector in `adapt_efficiency.rs` catalog with typed coverage state; missing required facts produce `Unavailable`/`Skipped`, never "no finding". Not re-executed this lane (no cargo). |
| ADP-054 duplicate tool work | IMPLEMENTED-UNVERIFIED | Same as ADP-053 — detector present in catalog. |
| ADP-057 retry-loop cost | IMPLEMENTED-UNVERIFIED | Same — detector present. |
| ADP-058 verification churn | IMPLEMENTED-UNVERIFIED | Same — detector present. |
| ADP-059 replan churn | IMPLEMENTED-UNVERIFIED | Same — detector present. |
| ADP-062 stranded worker work | IMPLEMENTED-UNVERIFIED | Same — detector present. |
| ADP-072 bounded clarification | IMPLEMENTED+VERIFIABLE | `membrane-adapt/src/clarification.rs`: one evidence-bound clarification question when safe proposal formation is impossible; nonmutating clarification state persisted; test row `membrane-adapt/tests/clarification_state.rs`. |
| ADP-073 one apply-eligible per target+version | IMPLEMENTED+VERIFIABLE | `membrane-adapt/src/proposal_state.rs` `ProposalStore` enforces single apply-eligible pending per `semantic_target_sha256 + target_version` with typed `ApplyEligibleConflict`; exact replay converges idempotently. Test rows: `tests/proposal_state_machine.rs`, `tests/proposal_target_exclusion.rs`. |
| ADP-074 negotiated read-only inspection | IMPLEMENTED-UNVERIFIED | Scope-bound read-only inspection in `adapt_service.rs` (`inspect_issues`, `inspect_proposals`, status); scope mismatch rejected, limits enforced, no control actions or exposure receipts created — asserted in `adapt_admin_qualification.rs` qual rows. Negotiated capability exposure and the MCP proposals arm remain per PATCH REQUESTS item 5. |
| ADP-075 honest daemon-backed status | IMPLEMENTED-UNVERIFIED | `adapt_service::status` kept contract `adapt.live-status.v1` (qual + `adp-windows.mjs` depend on it) and now reports producer/consumer bindings: evidence-stream availability vs empty workload, detector coverage ran/skipped/unavailable/failed counts, outcome-join availability, review-queue depth (pending/blocked/failed/admitted), learner lane (configured provider kind, last run, cursor vs durable reviewed seq), admission delivery, and effectiveness join state. Blocked/failed/unavailable are distinct fields; configuration alone no longer reads as ready. Qual logic in `adapt_admin_qualification.rs` checks the additive honest-status evidence. |

Not assigned/excluded: ADP-020, ADP-037, ADP-039, ADP-041, ADP-043–052,
ADP-055–056, ADP-060–061, ADP-063–071, ADP-076+ are EXCLUDED in canon —
not implementation work.

## CHANGES

Owned-path edits (5 files):

- `engine/crates/membrane-adapt/src/learner.rs` (new, ~700 lines)
  - Deterministic first-party `adapt_behavioral_review` learner semantics
    (ADP-035): `LearnerEventV1`/`AdaptLearnerInputV1` bounded window
    (≤512 events, seq strictly > cursor, non-empty ids/hashes, object
    payloads), closed `LearnerFindingClass` set
    (`repeated_detector_episodes`, `repeated_detector_coverage_gap`,
    `repeated_identical_emission`), `AdaptLearnerProposalV1` with
    content-derived `proposal_id` (scope+session+class+summary+evidence+
    seq-range+count → `alp_<sha256>`), `boundary:"proposal_only"`,
    per-class honesty limits enforced by `validate()`
    (`HonestyLimitMismatch`, `IdentityMismatch` reject mutation/forgery),
    ≤32 proposals, ≤64 evidence refs, self-validation before emission
    (`retain(validate().is_ok())`), typed `Unavailable{EmptyWindow|
    InvalidInput}` distinct from `NoFindings`. No store, no sink, no
    cursor ownership, no model/usage claim. Unit tests cover recurrence
    gating, identity determinism across job ids, forged-identity and
    boundary-mutation rejection, empty/invalid windows, no-findings.
  - Fixed the repeated-emission test fixture: `content_hash` now derives
    from `sha256_canonical(payload)` so genuinely identical payloads
    collide (previously derived from seq, making the detector unreachable).
- `engine/crates/membrane-adapt/src/lib.rs`
  - Registered `pub mod learner;`.
- `engine/crates/membrane-adapt/src/remediation.rs` (ADP-033)
  - `regression_case_proposal(issue, case_summary, at)` (~line 1103):
    converts a reviewed sealed issue (`Confirmed`/`Reopened` only, else
    `IssueNotConfirmed`) into a minimal privacy-safe regression-case
    proposal for the evaluator surface — bounded caller-authored summary
    (≤2,048 chars), case text carries issue identity + payload sha256 +
    ≤16 evidence digests, sealed via `SealedRemediationProposalV1::seal`
    under `requires_human_review`/`adapt-remediation-v1`/
    `adapt-redaction-v1`. External evaluator owner adopts/runs; Adapt
    never executes or writes durably. New error variants
    `IssueNotConfirmed`, `InvalidCaseSummary`. Unit tests
    `regression_case_proposal_requires_confirmed_issue` and
    `regression_case_proposal_is_privacy_safe_and_deterministic` cover
    gating, determinism, privacy, and summary bounds (fixture mirrors
    `sealed_issue.rs`'s `sealed()` builder + receipted
    Observed→Recurring→Confirmed transitions).
- `engine/crates/membrane-runtime/src/adapt_service.rs`
  - `AdaptBehavioralReviewProvider` — maps the protocol request window
    to `AdaptLearnerInputV1`, runs the learner, serializes proposals to
    `curation_proposals` values; result carries learner id/version + job
    id as provider identity, provenance digest bound to the exact
    request, no model/usage fields; `NoFindings` → empty proposals + no
    cursor; `Unavailable` → typed blocked reason; `Proposals` →
    `consumed_through_seq` as next cursor (advanced downstream only
    after sink submission).
  - `AdaptFirstPartySemanticReviewProvider` — dispatches
    `AdaptBehavioralReview` to the learner provider and Cortex kinds to
    `DeterministicFirstPartySemanticReviewProvider`; non-Adapt kinds
    sent to the learner provider return typed `Blocked{InvalidJob}`.
  - `inspect_proposals` — bounded projection over
    `membrane_knowledge_proposal` JOIN `cortex_proposal_admission_v1`:
    proposal id, review state, admission state, created/updated/reviewed
    timestamps, reviewer, attempts, last error, emission digest,
    truncated text; returns `available:false` +
    `proposal_queue_schema_absent` when the schema is absent (ADP-034/074).
  - `inspect_issues` enriched (ADP-034): scope, schema/issue identity,
    family/description, honesty limit, lifecycle, recurrence
    signature/count/first/last, post-mitigation recurrence,
    applicability, authority/influence class, confidence/evidence
    quality, candidate mechanisms, mitigation links, evidence
    refs/digests, validator receipt, available lifecycle actions,
    receipt count + recent receipts, update time, payload digest.
  - `status` enriched (ADP-075) on the unchanged `adapt.live-status.v1`
    contract: evidence-stream availability vs empty workload, coverage
    state counts, join availability, review-queue depth, learner lane
    (provider kind, last run, cursor vs reviewed seq), admission
    delivery, effectiveness join state.
- `engine/crates/membrane-runtime/src/adapt_admin_qualification.rs`
  - Qualification checks the additive honest-status evidence fields
    while retaining `adapt.live-status.v1` (contract string restored
    after an earlier drift — `adp-windows.mjs` requires it).

All edits are compile-by-inspection only: coordinator owns Rust builds;
no cargo/rightkit was invoked per lane rules.

## PATCH REQUESTS (non-owned files)

1. `engine/crates/membrane-runtime/src/background_review.rs` — route
   Adapt learner proposals as their own kind. Add to
   `BackgroundReviewProposalAdmission` (after
   `submit_memory_candidates`, ~line 464):

   ```rust
   /// ADP-035: Adapt learner proposals are a distinct kind — not
   /// semantic-curation records and must not be relabeled as such.
   fn admit_adapt_proposals(
       &self,
       _job: &BackgroundReviewJobV1,
       _proposals: &[membrane_adapt::learner::AdaptLearnerProposalV1],
   ) -> Result<(), BackgroundReviewReasonV1> {
       Err(BackgroundReviewReasonV1::ProposalSinkUnavailable)
   }
   fn submit_adapt_proposals(
       &self, job: &BackgroundReviewJobV1,
       proposals: &[membrane_adapt::learner::AdaptLearnerProposalV1],
   ) -> Result<(), BackgroundReviewReasonV1> {
       self.admit_adapt_proposals(job, proposals)
   }
   ```

   `JsonlBackgroundReviewProposalAdmission` impl:
   `self.append(job, "adapt_proposal", proposals)`. In
   `execute_background_semantic_review` (~line 1710) split the
   `AdaptBehavioralReview` arm from `CortexSemanticDream`: reject
   non-empty `memory_candidates`; deserialize each
   `curation_proposals` value as `AdaptLearnerProposalV1`
   (`InvalidProposal` on failure); run `proposal.validate()`
   (`InvalidProposal` on failure); submit via
   `sink.submit_adapt_proposals` (`ProposalSinkUnavailable`/`Failed`
   preserved); extend refs with `proposal_ref(&p.proposal_id, p)`.
   Empty proposals must not require a sink (completed no-op) and must
   not advance the cursor — current code already gates cursor on
   submission success.

2. `engine/crates/membrane-runtime/src/cortex_lifecycle.rs` — add a
   `"adapt_proposal"` arm to `drain_background_proposals` that
   deserializes `AdaptLearnerProposalV1`, calls `.validate()`
   (malformed → typed failure, not requeue), then calls `propose(...)`
   with `{text: summary, kind:"adapt_review", producer:"adapt_native",
   epistemicClass:"inferred", adaptProposalId, findingClass,
   sourceEventIds, sourceContentHashes, jobId}` so learner output enters
   the same daemon-owned admission boundary with pre-gates, pending
   review, and idempotency intact (ADP-029/035).

3. `engine/crates/membrane-runtime/src/bin/membrane-daemon.rs` (~line 75)
   — replace `Box::new(DeterministicFirstPartySemanticReviewProvider::new())`
   with `Box::new(AdaptFirstPartySemanticReviewProvider::new())`
   (import `membrane_runtime::adapt_service::AdaptFirstPartySemanticReviewProvider`,
   drop the now-unused deterministic import, update the provider label to
   e.g. `"first_party_dispatch"`). Without this the daemon never invokes
   the Adapt learner — `AdaptBehavioralReview` jobs would hit the
   deterministic Cortex analyzer, which correctly refuses the kind, so
   jobs would fail closed rather than learn.

4. `engine/crates/membrane-protocol/src/host_observation.rs` — retire
   stale Push naming on `ExecutionObservationKindV1::PushReduction` /
   `PushRestore` (ADP-030). These variants carry Pull delivery/reduction
   facts; the names date to the retired Push subsystem. Bounded fix:
   doc-comment the variants as "Pull reduction/restore facts carried
   under retired-Push compat names" now, and rename with a versioned
   migration when a consumer-facing change is acceptable. Current canon
   wording also requires distinguishing Pull delivery/reduction from
   public `push` Cortex writes — the enum names as-is conflate them.

5. `engine/crates/membrane-runtime/src/mcp_executor.rs` — wire the
   `adapt` "proposals" op to `crate::adapt_service::inspect_proposals`
   (scope, limit) instead of returning `{"pending_proposals":null,
   "reason":"pending_proposal_registry_unavailable"}`. Keep the existing
   `adapt.comparison` page merged alongside so the response still
   carries comparisons. The function already returns typed
   `available:false` + `proposal_queue_schema_absent` when the schema
   is absent, preserving fail-closed behavior.

6. `scripts/qualification/cases/adp-windows.mjs` — two stale texts:
   (a) ADP-030 atom text (~line 560) predates the canon wording that
   requires distinguishing Pull delivery/reduction from public `push`
   durable-memory writes — update to the canon text at
   `docs/canon/adapt.md:58`; (b) the ADP-035 case file list should add
   `engine/crates/membrane-adapt/src/learner.rs` and its gap text should
   reflect that the learner + provider now exist in-tree with the
   executor/drain/daemon wiring tracked as remaining seams.

## VERIFICATION

- `node --test scripts/qualification/cases/adp-windows.test.mjs` —
  7/7 pass (source-bound identity evidence, fail-closed CLI probe,
  canonical marker retention, partial-row gap preservation, all
  negative controls).
- `node scripts/ci/check-native-contract-fixtures.mjs` —
  `contracts=6 errors=0 warnings=0`.
- No cargo/rightkit per lane rules; Rust edits are compile-by-inspection
  with API/path verification (module registration, trait signatures,
  `Option::is_some_and`, `BTreeMap` iteration, serde camelCase
  round-trip between `AdaptLearnerProposalV1` and the proposal-sink
  `Value` path).

## KNOWN GAPS / RISKS

- ADP-035 seam chain is source-complete only in owned files; the
  executor arm, drain arm, and daemon registration are PATCH REQUESTS
  1–3 — until they land the learner is unreachable from the daemon
  (fails closed, not silently misrouted).
- ADP-030 caller transport remains incomplete (canon ADP-I030 note);
  host producers are outside this lane.
- ADP-036 effectiveness stays honestly `null`/unavailable pending exact
  host-loaded-representation acknowledgement (ADP-I036) — intentional.
- ADP-011/012/015/016/019/022/023/024/025 remain canon-PARTIAL; not
  closed by this lane.
- `adapt.live-status.v1` contract preserved for qual compatibility; the
  additive fields are extensions, not a contract bump.
- No PR status claimed; no git mutations performed.
