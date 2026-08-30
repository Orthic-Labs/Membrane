# Pull archive intake reconciliation

Target: Membrane `a9a4afb3eeaf4ee00869e8c303c50f810632f273`
Archive target: `29adfc8e2fe5a2d43ed25634a91ebec3bb4070d3`
Mode: independent `RECONCILE`; archive claims treated as untrusted; repository unchanged.

## Outcome

- Archive atom rows: **41 requested / 41 evaluated / 1 unresolved / 0 excluded**.
- Migration: **36 EXISTING + 3 NEW + 1 REGISTER + 1 UNRESOLVED = 41**. No `DUPLICATE`, `OBSOLETE`, or `EXCLUDED` rows.
- Current Pull canon stays **35 committed + 1 exploratory**. Required boundary is `RELEASED`; qualification is pending for every row, so **0/35 committed capabilities are closed**.
- Canon promotion is blocked: `PUL-038` fails atomicity; all donor reuse/license dispositions are absent; `PUL-037`, `PUL-039`, & `PUL-040` need owner acceptance plus frozen qualification.

Frozen donor references from archive receipt: PackMind `62cb56781002e3df5b8c9c8973fc133993ad37a4`; Trellis `fe843fc18c5ba6ee6cd067111f4c868b6b80f154`; LlamaIndex `f87a57bb2b95a7ca9923b4e5029cb6d7ea6e28fe`; Haystack `e318778c9bf60a1963e3b5f451359655dd696c30`; ContextCore `4eb3381c3d000f55b78f62c6a6f0eb9cb3374d49`. Archive marked each shared/library-applicable, but no local manifest or license ledger independently proves that claim.

## Full migration map

Each archive atom appears exactly once. Stable current IDs are unchanged.

| Archive ID | Disposition | Target / reason |
|---|---|---|
| PUL-001 | EXISTING | PUL-001 |
| PUL-002 | EXISTING | PUL-002 |
| PUL-003 | EXISTING | PUL-003 |
| PUL-004 | EXISTING | PUL-004 |
| PUL-005 | EXISTING | PUL-005 |
| PUL-006 | EXISTING | PUL-006 |
| PUL-007 | EXISTING | PUL-007 |
| PUL-008 | EXISTING | PUL-008 |
| PUL-009 | EXISTING | PUL-009 |
| PUL-010 | EXISTING | PUL-010 |
| PUL-011 | EXISTING | PUL-011 |
| PUL-012 | EXISTING | PUL-012 |
| PUL-013 | EXISTING | PUL-013 |
| PUL-014 | EXISTING | PUL-014 |
| PUL-015 | EXISTING | PUL-015 |
| PUL-016 | EXISTING | PUL-016 |
| PUL-017 | EXISTING | PUL-017 |
| PUL-018 | EXISTING | PUL-018 |
| PUL-019 | EXISTING | PUL-019 |
| PUL-020 | EXISTING | PUL-020 |
| PUL-021 | EXISTING | PUL-021 |
| PUL-022 | EXISTING | PUL-022 |
| PUL-023 | EXISTING | PUL-023 |
| PUL-024 | EXISTING | PUL-024 |
| PUL-025 | EXISTING | PUL-025 |
| PUL-026 | EXISTING | PUL-026 |
| PUL-027 | EXISTING | PUL-027 |
| PUL-028 | EXISTING | PUL-028 |
| PUL-029 | EXISTING | PUL-029 |
| PUL-030 | EXISTING | PUL-030 |
| PUL-031 | EXISTING | PUL-031 |
| PUL-032 | EXISTING | PUL-032 |
| PUL-033 | EXISTING | PUL-033 |
| PUL-034 | EXISTING | PUL-034, still `EXPLORATORY`; archive proposal does not promote it |
| PUL-035 | EXISTING | PUL-035 |
| PUL-036 | EXISTING | PUL-036 |
| PUL-037 | NEW | Distinct cross-turn selection behavior: suppress unchanged prior delivery, with changed-content/refresh recovery. Current rule lane changes representation to reference; it does not suppress generic evidence. Preserve proposed ID if accepted. |
| PUL-038 | UNRESOLVED | One row combines adjacency gap fill with child→parent replacement. Those have separate state, owners, winners, & failure semantics; constituents map to PUL-004/PUL-020 acquisition plus PUL-023/PUL-027 representation, using Ledger/Blueprint/Cortex-owned relationships. Split decision required; do not allocate ID yet. |
| PUL-039 | NEW | Distinct cache-objective layout behavior. Current code diagnoses finalized order but does not choose or preserve a reusable prefix. Preserve proposed ID if accepted. |
| PUL-040 | NEW | Distinct position-aware layout policy already required by architecture but absent from Pull ledger. PSH-015 only executes/preserves planner-selected order. Preserve proposed ID if accepted. |
| PUL-041 | REGISTER | Register Trellis MMR as an implementation/evaluation arm for PUL-025. Novelty is already an explicit marginal-utility input in `docs/architecture/membrane.md:661-675`; no independent user outcome is proven. |

## Material existing rows: current path & donor registration

`Q` below means current `PUL-Q<atom>` remains `PENDING` at `RELEASED`. `R0` means archive Stage 4 was omitted: donor license/SPDX, obligations, & permitted reuse action are unknown; donor code is reference-only until disposition exists. Donor paths are frozen archive evidence, not independently manifest-validated source.

| Atom | Current operative path, symbol, live consumer, residual | Donor evidence / disposition | Acceptance / reuse |
|---|---|---|---|
| PUL-001 | `engine/crates/membrane-federation/src/request.rs::normalize_request`; `FederationEngine::federate` calls `NormalizedFederationRequest::normalize`; `engine/crates/membrane-runtime/src/pull/native_federation.rs::NativeFederation::federate` consumes engine. Requirement derivation remains partial. | PackMind `Command::Pack`, `PackRequest`; Trellis `get_context`. Register deterministic request-shape references only. | Q / R0 |
| PUL-002 | `engine/crates/membrane-federation/src/corrective.rs::{validated_contract_for_request,evaluate_sufficiency}`; consumed by `FederationEngine::federate`. Full monotonic requirement model remains partial. | PackMind `Profile`,`build_pack`; LlamaIndex `RouterRetriever`. Register profile/selector references; model router stays non-authoritative. | Q / R0 |
| PUL-004 | `engine/crates/membrane-federation/src/engine.rs::FederationEngine::federate` builds active providers/stages; `schedule_providers` executes them; native runtime consumer above. Capability/requirement staging is incomplete. | PackMind `gather`; Trellis `PackBuilder.build`; LlamaIndex `RecursiveRetriever`. Register bounded-stage/composition references. | Q / R0 |
| PUL-005 | `engine/crates/membrane-federation/src/scheduler.rs::schedule_providers`; called at `engine.rs:266` & corrective lane; native runtime consumer above. | LlamaIndex `QueryFusionRetriever`/`RouterRetriever` async fan-out. Register shape only; donor does not prove Membrane deadline/authority semantics. | Q / R0 |
| PUL-007 | `engine/crates/membrane-federation/src/providers/git.rs::{produce,GitProvider::provide}`; registered as `native.git` in `engine/crates/membrane-runtime/src/pull/native_federation.rs::NativeFederation::new`; consumed by native federation. | PackMind `Command::{Status,Pack}`, `build_pack`. Register dirty-state reference. | Q / R0 |
| PUL-009 | `engine/crates/membrane-federation/src/providers/anchors.rs::{AnchorsProvider::provide,resolve}`; registered as `native.anchors` in `engine/crates/membrane-runtime/src/pull/native_federation.rs::NativeFederation::new`; consumed by native federation. | PackMind `gather`,`select`. Register protected-anchor reference. | Q / R0 |
| PUL-016 | `engine/crates/membrane-federation/src/normalize.rs::{normalize_candidate,normalize_provider_output}`; `merge_outputs_with_strategy` consumes normalized lanes inside `FederationEngine::federate`. | PackMind `build_pack`; Trellis `PackBuilder.build`; LlamaIndex `BaseRetriever.retrieve`; Haystack `DocumentJoiner.run`. Register common-unit references. | Q / R0 |
| PUL-017 | Operative gates are distributed across providers plus `engine/crates/cortex-core/src/planner.rs::plan`; `engine/crates/membrane-runtime/src/pull/admission.rs::admit` has no non-test live consumer. Canon's cited implementation is therefore not production proof. | Trellis `PackBuilder.build`; LlamaIndex `KeywordNodePostprocessor`; ContextCore `ContextCache.load_documents` had no proven selector consumer. Reference only. | Q / R0 |
| PUL-018 | `engine/crates/membrane-core/src/fusion.rs::{trust_rank,fusion_order,fuse}` preserves authority/freshness ordering when RRF path is selected; `engine/crates/cortex-core/src/planner.rs::{plan,freshness_component}` is live via `engine/crates/membrane-runtime/src/pull/federation.rs::envelope_from_ccs`. Policy is not fully unified. | PackMind `build_pack`; Trellis `_apply_recency_decay`,`_apply_importance`. Register temporal ranking references beneath hard policy. | Q / R0 |
| PUL-020 | `engine/crates/membrane-federation/src/corrective.rs::{corrective_plan,corrective_trigger,append_output}`; called by `FederationEngine::federate`, which remerges once. | LlamaIndex `TransformRetriever`,`QueryFusionRetriever`. Register deterministic transform seam; LLM expansion remains optional/ineligible as default. | Q / R0 |
| PUL-021 | `engine/crates/membrane-federation/src/merge.rs::{FusionStrategy::FixedOrder,fuse_fixed_normalized}`; `FederationEngine` defaults to fixed order & native composition does not override it. | Trellis `RRFReranker`; LlamaIndex `QueryFusionRetriever`; Haystack `DocumentJoiner`. Register alternatives, not replacement proof. | Q / R0 |
| PUL-022 | `engine/crates/membrane-federation/src/merge.rs::{FusionStrategy::Rrf,fuse_rrf_normalized}` calls `engine/crates/membrane-core/src/fusion.rs::fuse`; however only tests/configurable constructor select RRF. Native production composition never calls `with_fusion_strategy`, so archive wording “current RRF baseline” overstates live state. | Trellis `RRFReranker`; LlamaIndex `QueryFusionRetriever`; Haystack `DocumentJoiner`. Evaluation/reference only until live cutover & frozen comparison. | Q / R0 |
| PUL-023 | `engine/crates/cortex-core/src/planner.rs::plan` performs ID/source-hash/normalized-content collapse with winner/loser receipts; live via `engine/crates/membrane-runtime/src/pull/federation.rs::envelope_from_ccs`. `engine/crates/membrane-core/src/fusion.rs::fuse` also collapses families on selectable RRF path. | PackMind `select`; Trellis `_semantic_dedup_tracked`; LlamaIndex `QueryFusionRetriever`; Haystack `DocumentJoiner`. Register layered-collapse references; parent concentration is qualification, not atom. | Q / R0 |
| PUL-024 | `engine/crates/cortex-core/src/planner.rs::plan` uses protected candidates & fixed source-kind lanes; live via `engine/crates/membrane-runtime/src/pull/federation.rs::envelope_from_ccs`. Canonical evidence-dimension floor is not implemented end-to-end. | PackMind `select`; Trellis `build_sectioned`. Register section-budget reference; do not copy UI/tool sections as requirements. | Q / R0 |
| PUL-025 | `engine/crates/cortex-core/src/planner.rs::{plan,score_proportional_allotments}`; live via `engine/crates/membrane-runtime/src/pull/federation.rs::envelope_from_ccs`. Whole-task marginal-utility qualification remains absent. | PackMind `select`; plus PUL-041 Trellis `MMRReranker.rerank` as disabled evaluation arm. | Q / R0 |
| PUL-027 | `engine/crates/membrane-runtime/src/push/selection.rs::{build_packet_reduction_plan_with_policy,select_packet_for_h8_with_policy}`; consumed by `engine/crates/membrane-runtime/src/pull/federation.rs::native_route_response`. Ladder remains partial. | PackMind `select`; Trellis `apply_disclosure`. Register representation/timing references. | Q / R0 |
| PUL-028 | `engine/crates/membrane-core/src/{budget.rs::CrossProviderBudget,reconcile.rs::reconcile}`. `reconcile` has no production call outside its module/tests, so archive/canon “every packet” claim is unproven. | PackMind `build_pack`; Trellis `PackBuilder.build`,`apply_disclosure`,`get_context`. Register trace/recharge references only. | Q / R0 |
| PUL-031 | `engine/crates/membrane-federation/src/merge.rs::MergeResult::response` emits fusion receipt/omissions; `engine/crates/cortex-core/src/planner.rs::{ContextReceiptV2,plan}` emits candidate decisions; `engine/crates/membrane-runtime/src/pull/federation.rs::envelope_from_ccs` returns both. Full canon receipt fields remain partial. | PackMind `build_pack`,`Command::Why`; Trellis `PackBuilder.build`,`get_context`; ContextCore `ContextSelector.select` lacks proven live consumer. | Q / R0 |
| PUL-032 | `engine/crates/cortex-core/src/planner.rs::ContextReceiptV2` records planned delivery; `engine/crates/cortex-store/src/context_telemetry.rs` stores typed events, but `evaluate_delivery_outcome` has test-only callers & canon records missing H7/H9/H10 producers. | Trellis `build_learning_observations_from_event_log`,`record_feedback` proves an attributable donor path per archive. Register attribution reference; Adapt owns learning pressure. | Q / R0 |
| PUL-033 | `engine/crates/membrane-federation/src/engine.rs::insufficient_confidence_from_merge`; called by `FederationEngine::federate`, emitting typed error with searched-lane counts. | Trellis `PackAssemblyError`,`StrategyFailure`,`get_context`. Register degraded-vs-fatal failure reference. | Q / R0 |

## Proposed rows & strongest current evidence

| ID | Current source / live consumer | Donor evidence | Owner & boundary | Required acceptance / qualification | Reuse |
|---|---|---|---|---|---|
| PUL-037 NEW | Related, narrower behavior exists: `engine/crates/membrane-federation/src/providers/rules.rs::{DeliveryLedger,RulesProvider::produce}` keys repository/client/session/candidate/hash; `engine/crates/membrane-runtime/src/pull/federation_sources.rs::RuntimeDeliveryLedger::claim` emits first `Inline`, later `Reference`; wired by `engine/crates/membrane-runtime/src/pull/native_federation.rs::NativeFederation::new`. It is process-local, rules-only, unbounded by time/event horizon, & never suppresses candidate membership. Scoped search found no generic Pull served-set/suppression path. | Trellis `pack_builder.py::{_recently_served,_is_suppressed}` consumed by MCP `get_context` per archive. Frozen source was not locally manifest-verifiable. | Pull owns cross-turn admission/omission. Provider may supply identity; Cortex telemetry may retain observations but cannot decide attention. | Preserve changed hash, explicit refresh, unknown prior hash, expiry, compaction recovery; typed suppression omission; restart/session isolation; no required-coverage loss; released whole-task non-regression. | R0; no direct/translated port allowed yet. |
| PUL-038 UNRESOLVED | `engine/crates/membrane-runtime/src/store.rs::MemoryStore::recall_scored_detailed_timed_at` performs bounded one-hop relationship augmentation, consumed by `engine/crates/membrane-runtime/src/pull/federation.rs::memory_candidates_payload`; Ledger LDG-014 owns document parent/child/sibling/ancestry; Blueprint BPT-032 owns bounded repository adjacency. No generic Pull parent-promotion consumer found. | LlamaIndex `AutoMergingRetriever` contains distinct `_fill_in_nodes` & `_get_parents_and_merge`; Trellis `measure_parent_concentration` is non-mutating qualification. | Relationship truth stays Ledger/Blueprint/Cortex. Pull may choose acquisition or representation only. | Architect must split or explicitly prove shared state/failure contract, then assign separate acceptance for gap fill vs faithful parent replacement. | R0. |
| PUL-039 NEW | `engine/crates/membrane-runtime/src/cache_prefix.rs::diagnose_cache_prefix` computes content-free digest/order; `engine/crates/membrane-runtime/src/pull/federation.rs::envelope_from_ccs` emits it. Consumer always passes `previous=None`; function does not choose ordering, partition invariant/task blocks, or preserve prefix. Diagnostic should become qualification register for proposed atom. | PackMind `plan.rs::{stable_order,build_pack}` with `report.rs::cache_report` qualification per archive. | Pull planner chooses cache-objective layout; Push executes unchanged. Cache diagnostic remains observability/qualification. | Stable policy/version; previous-packet comparison; prefix reuse metric; required-evidence placement non-regression; interaction matrix with PUL-040; rollback/disable; released host/model qualification. | R0; no direct/translated port allowed yet. |
| PUL-040 NEW | Scoped source search found no production semantic placement/reorder policy. `docs/architecture/membrane.md:1270-1308` already requires deterministic position-aware layout; `docs/canon/push.md::PSH-015` only preserves/executes chosen order. | LlamaIndex `LongContextReorder`; Haystack `LostInTheMiddleRanker.run` & `MetaFieldGroupingRanker.run`. Public source/docs confirm request-final reordering, but exact frozen-commit manifest proof remains archive-only. | Pull owns versioned placement policy; Push/renderer faithfully executes; provider identity cannot become layout ontology. | Stable semantic classes; membership/authority invariant; atomic-group & cache-prefix protection; model-specific whole-task non-regression; flag/rollback; released host/model qualification. | R0; no direct/translated port allowed yet. |
| PUL-041 REGISTER | `docs/architecture/membrane.md:661-675` already makes novelty one PUL-025 marginal-utility input. Current planner has no MMR pass. | Trellis `rerankers/mmr.py::MMRReranker.rerank`; archive itself found no live consumer & named no winner. | Pull/PUL-025 implementation arm only. | Frozen evaluation must prove task-value gain, required coverage safety, deterministic tie-breaks, & score/penalty receipt before enablement. | R0. |

## Non-atom registrations from archive

| Archive mechanism | Disposition |
|---|---|
| Trellis `replay_pack_value` | REGISTER qualification for PUL-017/PUL-027 policy replay; not runtime evidence. |
| PackMind `cache_report` | REGISTER qualification for proposed PUL-039. |
| Trellis `analyze_pack_sections` | REGISTER qualification for PUL-024. |
| Trellis `measure_parent_concentration` | REGISTER qualification/negative evidence for PUL-023 & unresolved PUL-038. |
| Trellis PackBuilder evaluator hook | REGISTER assembly qualification; no selection ownership. |

## Cross-subsystem ownership

- Pull owns bounded acquisition, final eligibility, sufficiency, cross-provider fusion/dedupe/diversity, attention admission, publication, omissions, & layout-policy choice (`docs/architecture/membrane.md:213-240`; `cross-subsystem-evidence.md:214-235`).
- Push owns faithful execution only; PSH-015 preserves planner order unless Pull selects an explicit versioned policy.
- Ledger owns registered-document hierarchy/navigation (LDG-003/LDG-014), Blueprint owns repository semantics/adjacency (BPT-032), & Cortex owns durable-memory retrieval/relationships. PUL-038 cannot absorb those truth authorities.
- Adapt may learn outcome-backed pressure from PUL-032; it cannot mutate Pull truth or authority.

## Evidence defects & stale claims

1. Receipt fingerprints old target `29adfc8e...`; current target is `a9a4afb3...`. `git diff old..current -- engine docs/canon/pull.md docs/architecture/membrane.md docs/architecture/cross-subsystem-evidence.md` is empty, so reopened Pull operative paths are materially unchanged, but archive receipt/validator PASS is still stale metadata & cannot certify current target.
2. No local corpus manifest existed. Validator checked structure, not donor file existence. `79/125` donor cells were `Unclear`; one was `Not found`. Exact donor commits remain frozen references, not independently revalidated clones.
3. No Stage 4 license ledger exists. Every donor reuse action, obligation, & permitted port mode is unresolved.
4. Archive had no independent second pass. This review rejects its five-proposal count: three survive, one becomes implementation register, one remains non-atomic/unresolved.
5. Archive omitted current rules delivery ledger when proposing PUL-037 & current cache-prefix diagnostic when proposing PUL-039.
6. Archive says current RRF baseline, but production `FederationEngine` defaults to `FixedOrder`; native composition never selects `Rrf`.
7. Canon/archive path for PUL-017 points to `engine/crates/membrane-runtime/src/pull/admission.rs::admit`, which has no live non-test consumer.
8. Canon/archive path for PUL-028 points to `engine/crates/membrane-core/src/reconcile.rs::reconcile`, which has no live production consumer.
9. PUL-032's current delivery/outcome claim remains partial: planned receipts & telemetry storage exist, but delivery-outcome evaluator has no live producer consumer.
10. Donor comparison occasionally converts donor mechanism into design recommendation without proving Membrane applicability, current parity, license, or released acceptance. Recommendations remain references, never closure evidence.

## Highest-value candidates

1. **PUL-040** — architecture-required Pull policy is missing from atom ledger & implementation; clean Pull/Push boundary already exists.
2. **PUL-039** — current live diagnostic provides an immediate qualification seam, while actual cache-stable selection remains missing.
3. **PUL-037** — current rules-only representation history proves identity/receipt plumbing, but generic suppression requires strict recovery & coverage safeguards.
4. **PUL-041 register** — low-cost frozen evaluation arm for PUL-025; no new capability.
5. **PUL-038 unresolved** — potentially useful mechanisms, but unsafe to canonize until split & owner decision.

## Foundation summary

Foundation: Pull archive intake
Count view: CAPABILITY
Capabilities: 0/35 closed; 35 open
Non-counted: 1 group; 36 current implementations; 36 current qualifications; 5 archive mechanism registrations; 1 exploratory capability
Unclassified: 0
Reconciliation verdict: **PASS** (41/41 mapped once)
Canon-mutation verdict: **BLOCK** (PUL-038 atomicity; donor license disposition absent; NEW rows unaccepted/unqualified)
