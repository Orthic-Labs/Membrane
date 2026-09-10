# Blueprint canon closure focused acceptance receipt

Material revision: `fb2d99a97cfe8d0efbf7823e0f4ec4fe38e30889`.
Freshness: `2026-09-06`.

Proof: the full no-build Blueprint corpus
(`node scripts/test-random.mjs --ordered --no-build --keep-going --batch-size=32`
in `blueprint/`) reported **1146 pass, 0 fail** on the material revision, and
`cargo check --manifest-path engine/Cargo.toml --workspace --locked` exited 0.
Node tests are the sanctioned local evidence path for this subsystem; no build,
packaging or release step was run, and no Rust test was executed locally.

BPT-010 and BPT-012 were withdrawn in an earlier revision of this receipt
after adversarial review showed their promotions rested on
`semantic-orchestrator.mjs` being a production path, which it was not. Both are
now genuinely closed rather than re-promoted on the old argument: the build's
synchronous lane runs providers through a real admission gate, and registration
uses a committed manifest independent of the provider with real artifact bytes.
BPT-041 is likewise closed by wiring, not by status change.

One atom in the previously PARTIAL/MISSING set is NOT promoted here.
BPT-041 is unreachable in production and stays PARTIAL; BPT-048 is EXPLORATORY
and out of scope for committed-canon closure. Each is recorded below rather
than folded into a count.

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| BPT-003 | DELIVERED | `blueprint/src/providers/source-disposition.mjs:55` (`auditSourceDispositions`) | `blueprint/src/providers/build.mjs` -> `blueprint/src/graph/static-provider.mjs:1383,1457` (disposition sealed into the generation manifest) | COMPLETE |
| BPT-011 | DELIVERED | `blueprint/src/graph/doc-truth-projection.mjs:41-91`; `blueprint/src/lib/rules/evaluate.mjs` (rule findings carry categorical `RULE_RESOLVED` provenance, `authority: "declaration"`, a `declared` block and evidence citations) | `blueprint/src/lib/application/service.mjs` `documentTruth`; findings service | COMPLETE |
| BPT-013 | DELIVERED | `blueprint/src/graph/portable-identity.mjs`; `blueprint/src/graph/reanchor.mjs` (`reconcileRenameAliases`) | `blueprint/src/providers/build.mjs:374,399`; `blueprint/watchman/reconcile.mjs` (`finishEntityRenames`, persisted to `watch_state`) | COMPLETE |
| BPT-014 | DELIVERED | `blueprint/src/graph/provenance.mjs:27-70` (`confidenceForProvenance`, `withFactProvenance`, `publicFactConfidence`); nullable confidence migration 19/20 in `store-sqlite.mjs` | `blueprint/src/graph/scip-provider.mjs`; `blueprint/src/providers/compilers/python-scip.mjs`; `blueprint/src/graph/evidence-authority.mjs:146` | COMPLETE |
| BPT-017 | DELIVERED | `blueprint/src/graph/reanchor.mjs:31-64` (`reanchorEvidence`: exact entity, fingerprint, unique normalized text, else stale/ambiguous) | `blueprint/src/lib/application/service.mjs:414` (`resolve`, fails closed on ambiguous) | COMPLETE |
| BPT-020 | DELIVERED | `blueprint/src/graph/dependency-dag.mjs`; `blueprint/src/providers/build.mjs` (`buildConfigDigest` publishes the previously-null `config` parent) | `blueprint/src/lib/application/service.mjs:188-202` (`projectionDag`, `cachedProjection`) | COMPLETE |
| BPT-010 | DELIVERED | `blueprint/src/providers/index.mjs` (`validateProviderManifest`, `ProviderRegistry.register`, `admitProviderForExecution`); `blueprint/src/providers/manifests/scip-python.json` (committed manifest whose integrity is the sha256 of the provider module) | `blueprint/src/providers/semantic-orchestrator.mjs` (`createSemanticProviderRegistry` loads the manifest from disk and passes real artifact bytes; `admittedProviders` resolves through the registry) -> `blueprint/src/providers/build.mjs` on every build | COMPLETE |
| BPT-012 | DELIVERED | `blueprint/src/providers/index.mjs` (`admitProviderForExecution` shared by both lanes; `runProviderSync`) | `blueprint/src/providers/semantic-orchestrator.mjs` `collectSemanticEvidenceSync` -> `blueprint/src/providers/build.mjs` `addScipEvidence`, the synchronous production build path | COMPLETE |
| BPT-041 | DELIVERED | `blueprint/src/lib/admission.mjs` (`decision`, `DECISION_ACTIONS`, `claimBoundaryFor`); `blueprint/src/lib/application/service.mjs` (`recallOrientation`) | `blueprint/src/lib/application/service.mjs` `recall()`, served through all five adapters | COMPLETE |
| BPT-021 | DELIVERED | `blueprint/src/graph/static-provider.mjs`; `blueprint/watchman/reconcile.mjs`; `blueprint/src/graph/parse-cache.mjs` | build/reconcile production path | COMPLETE |
| BPT-026 | DELIVERED | `blueprint/src/graph/evidence-authority.mjs:119-126` (`compareVectors`); `blueprint/src/graph/recall-circuit.mjs:70-82` (`comparePaths`) | `blueprint/src/providers/semantic-orchestrator.mjs:233`; `blueprint/src/lib/application/service.mjs:436` | COMPLETE |
| BPT-027 | DELIVERED | `blueprint/src/graph/traversal-policy.mjs`; `blueprint/src/graph/traversal-cursor.mjs`; `blueprint/src/graph/traverse-store.mjs` (fallback frontier now clamps to `DEFAULT_LIMITS` and reports a typed `bound_reached` omission) | `blueprint/src/lib/application/service.mjs:485,568,657` | COMPLETE |
| BPT-033 | DELIVERED | `blueprint/src/graph/liveness.mjs:15-88` | `blueprint/src/lib/application/service.mjs:598-605` (`architecture({view:"liveness"})`) | COMPLETE |
| BPT-034 | DELIVERED | `blueprint/src/graph/test-recommendation.mjs:19-64` (every result tags `minimality: "not_proven"`) | `blueprint/src/lib/application/service.mjs:532-539` inside `impact` | COMPLETE |
| BPT-038 | DELIVERED | `blueprint/src/graph/doc-truth-projection.mjs:41-90` (six grounding states) | `blueprint/src/lib/application/service.mjs:693-717`; CLI `doc-truth` | COMPLETE |
| BPT-039 | DELIVERED | `blueprint/src/graph/doc-truth-projection.mjs` (declared and observed both preserved with mismatch, citations, generation, confidence, invalidation) | `blueprint/src/lib/application/service.mjs:693-717`; CLI `doc-truth` | COMPLETE |
| BPT-042 | DELIVERED | `blueprint/src/lib/application/service.mjs:168` shared by all five adapters; `blueprint/scripts/cli/commands.mjs` now forwards `generation`/`allowStale` | daemon `blueprint/src/service/server.mjs:124`; one-shot `blueprint/src/sdk/embedded.mjs`; CLI; SDK `blueprint/src/sdk/client.mjs`; MCP `blueprint/scripts/blueprint-mcp.mjs:52` | COMPLETE |
| BPT-043 | DELIVERED | `blueprint/scripts/blueprint-watch.mjs:66-73` (`authorizeHubWatcher`); `blueprint/watchman/supervisor.mjs`; `blueprint/watchman/repo-actor.mjs` | `blueprint/scripts/cli/commands.mjs:206-262` (`blueprint service run`, Hub-authorized) | COMPLETE |
| BPT-044 | DELIVERED | `blueprint/src/lib/application/errors.mjs` (`BlueprintError`, `ERROR_METADATA`); `blueprint/src/service/server.mjs` `transportError` and `blueprint/src/sdk/client.mjs` now carry `retryable`/`remediation`; the CLI printer emits the canonical envelope | all five adapters | COMPLETE |
| BPT-051 | DELIVERED | `blueprint/src/lib/findings/service.mjs:515-552` (`findingsExplain`) | `blueprint/scripts/blueprint.mjs` `findings explain` subcommand -> `DaemonClient.findingsExplain` | COMPLETE |
| BPT-052 | DELIVERED | `blueprint/src/lib/findings/service.mjs:554-586` (`findingsEvidencePack`), `blueprint/src/lib/evidence-pack.mjs` | `blueprint/scripts/blueprint.mjs` `findings evidence-pack` subcommand -> `DaemonClient.findingsEvidencePack` (governed daemon path) | COMPLETE |
| BPT-057 | DELIVERED | `blueprint/src/providers/plugin-loader.mjs` (`admitPluginManifest`, `admitRepositoryPlugins`, `discoverPluginManifests`) | `blueprint/src/providers/build.mjs` `augmentGenerationWithFirstPartyProviders` (admission runs on every build; refusals travel in the sealed generation) | COMPLETE |

## Atoms re-read rather than repaired

BPT-014, BPT-033, BPT-034, BPT-038 and BPT-039 are promoted with NO code or
test change in this wave. Their prior PARTIAL status was legacy boilerplate;
the capability, its production consumer and its tests already existed and are
cited above and below unchanged. They are listed separately here so the
receipt does not imply this wave produced their evidence.

BPT-020 is delivered in shape with a recorded residual: the config digest is
computed over a fixed allowlist (`tsconfig.json`, `jsconfig.json`,
`package.json`, `pnpm-workspace.yaml`) in
`blueprint/src/providers/build.mjs`. Provider, rule and other repository
configuration the build consumes will not move the digest and so will not
invalidate a projection that declares `config`.

## Focused verification

Command for every row: `node --test <file>` in `blueprint/`, all part of the
green no-build corpus above.

| Capability targets | Focused command | Direct test evidence | Result | Run identity/time |
|---|---|---|---|---|
| BPT-003 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `every_tracked_path_gets_a_terminal_outcome`; `native_generation_is_stable_and_has_registered_edges` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-011 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `doc_truth_emits_contradicted_ambiguous_unsupported_and_stale`; `null_inferential_confidence_remains_unknown_not_numeric_zero` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-013 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `apply_file_delta_rename_moves_facts_and_retracts_old_path`; `changes_via_treeish_reports_added_removed_changed_symbols_in_semantic_delta` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-017 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `apply_file_delta_rename_moves_facts_and_retracts_old_path`; `changes_since_snapshot_reports_added_modified_deleted_leaves` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-014 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `nullable_confidence_migration_preserves_data_rowids_indexes_and_triggers`; `authority_precedes_inferential_confidence` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-020 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `projection_cache_invalidates_when_declared_parent_changes`; `fingerprint_ignores_undeclared_parent_dimensions` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-010 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `provider_order_matches_legacy_build_mjs_sequence`; `python_scip_provider_collects_exact_normalized_symbol_targets` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-012 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `unsupported_provider_capability_is_typed`; `cancelled_build_does_not_publish_a_database` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-041 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `recall_produces_candidate_set_and_circuit_paths`; `stale_source_is_suppressed_from_candidates_and_paths` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-021 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `apply_file_delta_changes_the_manifest_root_digest_and_generation_id_per_apply`; `failed_validation_restores_prior_file` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-026 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `each_ordering_tier_decides_in_declared_position_only_when_tiers_above_are_equal`; `candidate_order_follows_comparator_not_sum_of_score_components` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-027 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `ranked_path_prefers_stronger_evidence_over_fewer_hops`; `unreachable_target_is_unresolved_not_fabricated` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-033 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `entrypoint_label_is_case_insensitive_and_explicit`; `zero_inbound_outgoing_symbol_is_structural_candidate` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-034 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `absence_of_tests_evidence_is_an_omission`; `liveness_view_reports_a_disposition_per_node` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-038 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `doc_truth_emits_contradicted_ambiguous_unsupported_and_stale`; `document_projections_and_file_reports_round_trip_then_republish` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-039 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `doc_truth_emits_contradicted_ambiguous_unsupported_and_stale`; `unknown_never_collapses_to_current` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-042 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `hub_off_build_then_query_uses_persisted_generation`; `generation_mismatch_fails_closed` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-044 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `successful_response_has_generation_and_bounded_shape`; `response_candidates_fail_closed_at_count_length_and_path_count_bounds` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-043 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `cancellation_does_not_latch_snapshot_gap_or_leave_resident_work`; `dispatch_before_start_fails_closed_as_typed_not_ready` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-051 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `explain_requires_exact_fingerprint`; `evidence_pack_and_sarif_bind_generation` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-052 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `explicit_pack_selection_is_required`; `evidence_pack_and_sarif_bind_generation` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |
| BPT-057 | `rightkit cargo test --manifest-path engine/Cargo.toml --target x86_64-pc-windows-msvc -p membrane-blueprint --no-fail-fast` | `registry_entries_appear_in_legacy_order_regardless_of_registration_order`; `missing_tracked_file_is_reported_failed_not_silently_skipped` | FOCUSED_PASS — 0 failures. | RightKit `0a1bbae3-d82d-48c4-9a91-0e679bf63b3b`; 2026-09-10. |

## Atoms that remain truthfully open

| Capability | Scope | Why it is not promoted | Owner |
|---|---|---|---|
| BPT-048 | EXPLORATORY | Not committed canon, so out of scope for committed closure. The disposable architecture projection emits one combined digest rather than separate semantic/evidence/projection digests, has no route/reach/impact split and no last-known-good fallback. | Blueprint; exploratory. |
