# Donor-better implementation receipt

Implementation revision: `2708e44435825af7a4211e3bf7b38a22503b7a85`.
Freshness: `2026-08-31`.

All 28 donor-better atoms now have production-path implementations. Competitive promotion remains separate: focused verification, exact-SHA CI, & qualification still control closure.

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| ADP-002 | DELIVERED | `engine/crates/membrane-transcript/src/adapters.rs`; `engine/crates/membrane-transcript/src/source.rs` | `engine/crates/membrane-transcript/src/parser.rs`; Adapt intake | COMPLETE |
| ADP-006 | DELIVERED | `engine/crates/membrane-adapt/src/proposal_state.rs` | `engine/crates/membrane-adapt/src/lib.rs`; sealed proposal lifecycle | COMPLETE |
| BPT-006 | DELIVERED | `blueprint/src/graph/scip-provider.mjs`; `blueprint/src/providers/build.mjs` | `blueprint/src/graph/static-provider.mjs`; full & incremental build | COMPLETE |
| BPT-007 | DELIVERED | `blueprint/src/providers/modules/javascript.mjs`; `blueprint/src/providers/modules/python-resolver.mjs` | `blueprint/src/providers/build.mjs`; graph generation | COMPLETE |
| BPT-008 | DELIVERED | `blueprint/src/providers/frameworks/http/index.mjs` | `blueprint/src/providers/build.mjs`; graph generation | COMPLETE |
| BPT-009 | DELIVERED | `blueprint/src/providers/schemas/sql.mjs` | `blueprint/src/providers/build.mjs`; graph generation | COMPLETE |
| BPT-018 | DELIVERED | `blueprint/src/providers/modules/javascript.mjs`; `blueprint/src/providers/modules/python-resolver.mjs` | `blueprint/src/providers/build.mjs`; graph generation | COMPLETE |
| BPT-019 | DELIVERED | `blueprint/src/graph/freshness-receipt.mjs` | `blueprint/src/lib/application/service.mjs`; query response boundary | COMPLETE |
| BPT-021 | DELIVERED | `blueprint/watchman/reconcile.mjs`; `blueprint/src/providers/build.mjs` | full/incremental convergence oracle | COMPLETE |
| BPT-023 | DELIVERED | `blueprint/src/graph/seed-resolver.mjs` | `blueprint/src/graph/recall-circuit.mjs`; application service | COMPLETE |
| BPT-025 | DELIVERED | `blueprint/src/graph/recall-circuit.mjs` | `blueprint/src/lib/application/service.mjs`; SDK/protocol | COMPLETE |
| BPT-029 | DELIVERED | `blueprint/src/graph/seed-resolver.mjs` | `blueprint/src/lib/application/service.mjs`; SDK/protocol | COMPLETE |
| BPT-032 | DELIVERED | `blueprint/src/graph/analytics/change-impact.mjs` | `blueprint/src/lib/application/service.mjs`; SDK/protocol | COMPLETE |
| BPT-035 | DELIVERED | `blueprint/src/graph/analytics/change-impact.mjs` | `blueprint/src/lib/application/service.mjs`; risk response | COMPLETE |
| BPT-037 | DELIVERED | `blueprint/src/graph/snapshots.mjs`; `blueprint/src/graph/analytics/change-impact.mjs` | `blueprint/src/lib/application/service.mjs`; history response | COMPLETE |
| BPT-040 | DELIVERED | `blueprint/src/graph/architecture-model.mjs` | `blueprint/src/lib/application/service.mjs`; cited disposable views | COMPLETE |
| BPT-047 | DELIVERED | `blueprint/src/lib/federation/index.mjs` | `blueprint/src/lib/application/service.mjs`; repository-isolated routing | COMPLETE |
| BPT-053 | DELIVERED | `blueprint/src/providers/frameworks/index.mjs` | `blueprint/src/providers/build.mjs`; graph generation | COMPLETE |
| BPT-054 | DELIVERED | `blueprint/src/providers/iac/terraform.mjs` | `blueprint/src/providers/build.mjs`; graph generation | COMPLETE |
| BPT-071 | DELIVERED | `blueprint/src/providers/bridges/seams.mjs` | `blueprint/src/providers/build.mjs`; graph generation | COMPLETE |
| CTX-013 | DELIVERED | `engine/crates/membrane-runtime/src/store.rs`; `engine/crates/membrane-core/src/fusion.rs` | `engine/crates/membrane-runtime/src/pull/federation_sources.rs`; Pull provider | COMPLETE |
| CTX-034 | DELIVERED | `engine/crates/membrane-runtime/src/store.rs` | `engine/crates/membrane-runtime/src/pull/federation_sources.rs`; CLI/service | COMPLETE |
| CTX-038 | DELIVERED | `engine/crates/membrane-runtime/src/store.rs`; `engine/crates/membrane-runtime/src/serve.rs` | native Pull, CLI, list, recall, explain, & relationship surfaces | COMPLETE |
| LDG-015 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/link_projection.rs` | `engine/crates/membrane-runtime/src/ledger/index.rs`; document spine | COMPLETE |
| LDG-016 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/link_projection.rs` | `engine/crates/membrane-runtime/src/ledger/doc_spine.rs`; Pull document lane | COMPLETE |
| LDG-027 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/query_alias.rs` | `engine/crates/membrane-runtime/src/ledger/doc_spine.rs`; shadow retrieval lane | COMPLETE |
| LDG-028 | DELIVERED | `engine/crates/membrane-runtime/src/ledger/document_conversion.rs` | Ledger normalization boundary | COMPLETE |
| PUL-022 | DELIVERED | `engine/crates/membrane-federation/src/merge.rs`; `engine/crates/membrane-core/src/fusion.rs` | `engine/crates/membrane-federation/src/engine.rs`; Pull fusion | COMPLETE |

## Local verification

- `pnpm test`: 204 product Node passes, 1 skip; 57 restored passes; legal inventory verified.
- Blueprint changed-path suite: 114 pass, 0 fail.
- Blueprint application/brief/query-runtime regression slice after fixes: 20 pass, 0 fail.
- `git diff --check`: pass.
- Rust compile & focused Rust behavior remain pending exact-revision GitHub CI because managed RightKit runtime activation is unavailable on this host.
