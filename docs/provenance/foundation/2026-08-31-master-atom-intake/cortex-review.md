# Cortex archive reconciliation

## Scope & result

- Review target: `a9a4afb3eeaf4ee00869e8c303c50f810632f273`.
- Archive inputs: `cortex_master_atom_list.md` SHA-256 `d43402759b9e91366f988e5a4645ad8df1bc5cc76b69f34089e95735bd97dbdd`; `cortex_foundation_stage3_matrix.md` SHA-256 `7753ce99c688d829b0731d561f0f23a6f568fdd04d171c1524b4405fcb9b4f9f`.
- Current authorities: `docs/canon/cortex.md` SHA-256 `e62aa5bc93e6b42a03dc6425da2051e5d0bd77238a39822736b199164f2bf333`; `docs/architecture/cross-subsystem-evidence.md` SHA-256 `3f289d668776583352157a7d81f2f6e6eff4fd2e27791d4e59aeeb3f53dfef8b`; operative Rust source under `engine/crates/cortex-*`, `engine/crates/membrane-runtime`, `engine/crates/membrane-federation`, & `engine/crates/membrane-core`.
- Archive inventory: 60 rows = 36 committed CTX atoms + 1 exploratory CTX atom + 23 donor-disposition rows. Stage-3 matrix covers only 36 committed rows.
- Disposition: EXISTING 37; NEW 1; REGISTER 8; DUPLICATE 0; OBSOLETE 0; EXCLUDED 14; UNRESOLVED 0. Total 60.
- Current Cortex closure is unchanged: 0/36 committed rows meet required `RELEASED` boundary. All 36 have pending/stale qualification; `CTX-033` remains exploratory & non-counted.
- No current ID is renamed or replaced. `D-001` remains donor identity until canon assigns a stable ID.

## Full migration map

### Archive CTX rows

Each archive CTX behavior is already represented by same current stable ID. `EXISTING` means identity/behavior mapping only; it does not promote implementation, verification, qualification, delivery, or closure.

| Archive atom | Archive behavior | Disposition | Current evidence / qualification effect |
|---|---|---|---|
| CTX-001 | Canonical local durable authority | EXISTING (`CTX-001`) | Current canon line 17; `CTX-I001` remains PARTIAL; RELEASED qualification pending. |
| CTX-002 | Governed pre-admission gate | EXISTING (`CTX-002`) | Current canon line 18; `CTX-I002` remains PARTIAL; no full ordered gate proof. |
| CTX-003 | Atomic governed batch write | EXISTING (`CTX-003`) | Current canon line 19; `CTX-I003` source `store.rs:6647-6709,6967-7199`, consumer `serve.rs:3297-3401`; qualification pending. |
| CTX-004 | Canonical knowledge record envelope | EXISTING (`CTX-004`) | Current canon line 20; shape convergence remains partial. |
| CTX-005 | Exact semantic duplicate no-op | EXISTING (`CTX-005`) | Current canon line 21; `CTX-I005` source `store.rs:352-380,7421-7576`, consumer `serve.rs:3366-3401`; qualification pending. |
| CTX-006 | Transactional near-duplicate handling | EXISTING (`CTX-006`) | Current canon line 22; `CTX-I006` source `store.rs:7461-7576,7690-7820`, consumer `serve.rs:3366-3401`; qualification pending. |
| CTX-007 | Conflict quarantine & restoration | EXISTING (`CTX-007`) | Current canon line 23; `CTX-I007` source `store.rs:7577-7648,2781-2983`, consumer `serve.rs:4590-4604`; qualification pending. |
| CTX-008 | Non-destructive supersession | EXISTING (`CTX-008`) | Current canon line 24; `cortex-store/src/temporal.rs:442-608,665-707`, consumer `mcp_executor.rs:791-837`; qualification pending. |
| CTX-009 | Bitemporal point-in-time recall | EXISTING (`CTX-009`) | Current canon line 25; `temporal.rs:608-651,711-837`, consumer `mcp_executor.rs:791-837`; qualification pending. |
| CTX-010 | Deterministic archive-first lifecycle | EXISTING (`CTX-010`) | Current canon line 26; PARTIAL; Ledger/Blueprint/version/outcome triggers remain incomplete. |
| CTX-011 | Scoped lexical durable recall | EXISTING (`CTX-011`) | Current canon line 27; `cortex-store/src/fts5.rs:43-190,229-247`, consumers `store.rs:4422-4715` & `memory_provider.rs:471-545`; qualification pending. |
| CTX-012 | Local vector durable recall | EXISTING (`CTX-012`) | Current canon line 28; operative indexed/fallback path at `store.rs:74-80,1967,4515-4536`; qualification explicitly STALE. |
| CTX-013 | Provider-local lexical/vector fusion | EXISTING (`CTX-013`) | Current canon line 29; PARTIAL. Current RRF mechanism is proven below under D-007; RELEASED qualification remains pending. |
| CTX-014 | Bounded recall preview + resolver | EXISTING (`CTX-014`) | Current canon line 30; `store.rs:8622-8669`, consumers `serve.rs:3935-3958,4249-4327`; qualification pending. |
| CTX-015 | Receipt-bound usefulness feedback | EXISTING (`CTX-015`) | Current canon line 31; PARTIAL; verified producer coverage incomplete. |
| CTX-016 | Bounded unknown outcome closure | EXISTING (`CTX-016`) | Current canon line 32; `store.rs:3488-3605`, consumers `serve.rs:4717` & `cli.rs:4443`; qualification pending. |
| CTX-017 | Bounded evidence relationship graph | EXISTING (`CTX-017`) | Current canon line 33; PARTIAL; bounded inspection consumer exists through `/graph`. |
| CTX-018 | Machine-local A0 checkpoints | EXISTING (`CTX-018`) | Current canon line 34; `checkpoint.rs:192-270`, MCP/CLI consumers; qualification pending. |
| CTX-019 | Checkpoint promotion as proposal | EXISTING (`CTX-019`) | Current canon line 35; `cli.rs:4016-4027` prints proposal but does not submit it; PARTIAL. |
| CTX-020 | Pending KnowledgeEmission intake | EXISTING (`CTX-020`) | Current canon line 36; `mcp_executor.rs:917-1065`, live MCP dispatch consumer; qualification pending. |
| CTX-021 | Single-decision proposal review | EXISTING (`CTX-021`) | Current canon line 37; `mcp_executor.rs:1065-1269`, live MCP dispatch consumer; no version fence, per D-005. |
| CTX-022 | Deterministic reversible Dream Stage 0 | EXISTING (`CTX-022`) | Current canon line 38; `cortex-core/src/dream.rs:76-304`, `store.rs:2486-2770`, CLI/service consumers; qualification pending. |
| CTX-023 | Proposal-first semantic Dream Stage 1 | EXISTING (`CTX-023`) | Current canon line 39; PARTIAL; semantic provider, foreground signal, & sink remain unproven. |
| CTX-024 | Gap-gated episodic proposal extraction | EXISTING (`CTX-024`) | Current canon line 40; PARTIAL; Cortex-specific proposal sink remains unproven. |
| CTX-025 | Complete hard erase | EXISTING (`CTX-025`) | Current canon line 41; `store.rs:7822-7914`; no production consumer; PARTIAL. |
| CTX-026 | Digest-sealed backup | EXISTING (`CTX-026`) | Current canon line 42; `MemoryStore::backup_cortex` at `store.rs:7920-7990`; no authorized production surface; PARTIAL. |
| CTX-027 | Canonical scoped audit export | EXISTING (`CTX-027`) | Current canon line 43; `store.rs:8416-8495`, CLI consumer `4861-4869`; qualification pending. |
| CTX-028 | Deterministic review queue export | EXISTING (`CTX-028`) | Current canon line 44; `store.rs:8335-8414`, CLI consumer `4871-4881`; qualification pending. |
| CTX-029 | Rebuildable retrieval projections | EXISTING (`CTX-029`) | Current canon line 45; `MemoryStore::reindex` at `store.rs:8271-8331`, CLI consumer `4849-4858`; PARTIAL. |
| CTX-030 | Read-only explain & bounded browse | EXISTING (`CTX-030`) | Current canon line 46; fragmented CLI/Hub/service surfaces; no complete canonical inspection surface. |
| CTX-031 | Controlled causal-learning experiment | EXISTING (`CTX-031`) | Current canon line 47; `store.rs:5088-5199,5202-5385`, CLI consumer `4455-4473`; controlled joins incomplete. |
| CTX-032 | Purpose-separated event/telemetry log | EXISTING (`CTX-032`) | Current canon line 48; `cortex-store/src/absorbed_records.rs:290-642` & `context_telemetry.rs:2954-3197`, daemon/service consumers; qualification pending. |
| CTX-033 | Ledger-fenced document-derived semantic admission/revalidation | EXISTING (`CTX-033`) | Current canon line 49 preserves exact behavior as EXPLORATORY/MISSING/HOLD; remains non-counted & Ledger-fenced. |
| CTX-034 | Governed portable skill index | EXISTING (`CTX-034`) | Current canon line 50; `store.rs:3722-3864`, Pull/CLI consumers; bounded body resolver E2E remains partial. |
| CTX-035 | Admission-time utility eligibility | EXISTING (`CTX-035`) | Current canon line 51; MISSING. `store.rs:7393-7690` has novelty/conflict gates but no independent utility gate. |
| CTX-036 | Verified transactional restore | EXISTING (`CTX-036`) | Current canon line 52; `MemoryStore::restore_cortex` at `store.rs:7997-8039`; digest/schema/transaction exist, but no production surface or frozen recall-equivalence proof. |
| CTX-037 | Explicit durable import | EXISTING (`CTX-037`) | Current canon line 53; `cli.rs:1412-1503`, CLI consumer `4883-4886`; qualification pending. |

### Donor-derived rows

| Archive atom | Contract / mechanism | Disposition | Destination & reason |
|---|---|---|---|
| D-001 | Epistemic completeness envelope | NEW | Distinct consumer-visible Cortex behavior. Current provider status is partial, but bounded recall/inspection outputs do not consistently declare `exact` vs `lower_bound` with complete machine-readable causes. No stable ID assigned. |
| D-002 | Capability-specific readiness/degradation | EXCLUDED | Cross-subsystem owner is Pull/Hub/Membrane: `PUL-003`, `MEM-016`, `MEM-025`, `MEM-029`; Cortex supplies health facts only. |
| D-003 | Incremental projection invalidation locality | REGISTER | Qualification/performance criterion for `CTX-029`, not independent behavior. Current `MemoryStore::reindex` scans every canonical row, skips valid `(content_hash, embed_model)`, then updates invalid rows. |
| D-004 | Canonical truth vs disposable projections | REGISTER | Architecture/implementation reference for `CTX-001` + `CTX-029`; no separate consumer-visible behavior. |
| D-005 | Version-bound proposal commit | REGISTER | Acceptance strengthening for `CTX-021` + `CTX-023`. Current proposal table/review state machine binds repository/scope/content digest, but persists no expected canonical version/generation fence. |
| D-006 | Episode provenance separated from resolved fact | REGISTER | Data-model/qualification reference for `CTX-004`, `CTX-009`, `CTX-024`, & `CTX-032`; no new atom. |
| D-007 | Rank fusion for heterogeneous retrieval scores | REGISTER | Implementation evidence for `CTX-013`. Current `MemoryRetriever::retrieve_hybrid_with_lexical_hits` already performs lexical/vector reciprocal-rank fusion with `RRF_K=60`; donor recommendation must not create another atom. |
| D-008 | Two-stage bounded context delivery | REGISTER | Existing `CTX-014` implementation/qualification reference; preview then resolver is already canonical behavior. |
| D-009 | Quality-finding taxonomy | REGISTER | Diagnostic taxonomy/qualification reference for existing `CTX-022`; findings must remain proposal/review signals. |
| D-010 | Generation/digest-bound restore | REGISTER | Integrity/qualification reference for `CTX-026` + `CTX-036`. Current backup validates schema + payload digest transactionally, but lacks production surface, generation/embedder refusal, projection rebuild, & frozen recall-equivalence evidence. |
| D-011 | Task-conditioned structural ranking | EXCLUDED | Pull owns task-conditioned selection/fusion; Blueprint supplies repository structure. |
| D-012 | Adaptive token-budget elasticity | EXCLUDED | Membrane planner/Pull owns final budget, sufficiency, admission, & omissions. |
| D-013 | Structure-preserving line-of-interest rendering | EXCLUDED | Pull/Blueprint source rendering concern, not durable knowledge. |
| D-014 | Bounded query pagination/max-token contract | EXCLUDED | Pull/Membrane response-bound contract; provider-local limits remain inputs to final planner. |
| D-015 | Precision-tier code indexing | EXCLUDED | Blueprint owns repository parsing, semantic precision, graph evidence, & degradation. |
| D-016 | Incremental repository watcher | EXCLUDED | Blueprint owns repository freshness & graph generation. |
| D-017 | Execution/process-flow query | EXCLUDED | Blueprint owns code execution structure. |
| D-018 | Code impact/blast-radius analysis | EXCLUDED | Blueprint owns repository traversal/impact evidence. |
| D-019 | Derived route/tool/API maps | EXCLUDED | Blueprint owns derived repository views. |
| D-020 | Call/import/inheritance resolution | EXCLUDED | Blueprint owns language-aware code edges, confidence, & unresolved fallbacks. |
| D-021 | Multi-language parser/provider plugin surface | EXCLUDED | Blueprint owns parser/indexer surfaces. |
| D-022 | Source path sandbox & secret sanitation | EXCLUDED | Blueprint/Pull provider boundary owns source traversal & sanitation; `CTX-002` may consume classification but must not traverse repository source. |
| D-023 | Warm persistent code-index restore | EXCLUDED | Blueprint accelerator concern; it cannot become Cortex durable truth. |

## Highest-value material candidates & differences

### D-001 — NEW: epistemic completeness envelope

- Donor evidence: archive `cortex_master_atom_list.md:672,702-714` & stage-3 matrix CTX-030 row cite GitNexus `gitnexus/src/mcp/tools.ts` `context`/impact envelope. Donor source is not in allowed current workspace roots, so symbol/caller evidence is archive-supplied, not independently reopened.
- Current source gap: `engine/crates/membrane-runtime/src/pull/federation.rs:812-910`, symbol `memory_candidates_payload_for_descriptor`, probes N+1 & emits `ceiling_truncated` omissions. `engine/crates/membrane-runtime/src/pull/federation_sources.rs:214-264`, symbol `RuntimeMemorySource::candidates`, consumes candidates but discards payload omissions & sets `SourceResponse.complete=true`. `engine/crates/membrane-federation/src/providers/cortex.rs:46-63,167-232`, symbols `CortexProvider::provide_source`/`build_output`, can publish Complete/Partial with warnings/omissions, but receives false completeness from adapter. Live route is `engine/crates/membrane-runtime/src/pull/native_federation.rs:112-125,145-188` → `engine/crates/membrane-runtime/src/pull/federation.rs:81-103,216-233` → Pull planner/publication.
- Inspection gap: `engine/crates/membrane-runtime/src/store.rs:4172-4267`, symbols `relationship_graph`/`relationship_graph_json`, is bounded but has no completeness field/cause; live consumers are `serve.rs:3050-3054` (`GET /graph`) & `cli.rs:4838-4846`. `cli.rs:1393` `explain_memory`, consumed at `cli.rs:4097`, likewise has no envelope.
- Owner/boundary: Cortex owns truthful lane-local coverage facts for its recall/list/explain/relationship outputs. Pull retains final cross-provider sufficiency, fusion, admission, publication, & omission receipts (`cross-subsystem-evidence.md:218-229`). Cortex must not claim final context completeness.
- Acceptance qualification: frozen corpus exercises empty exact result, over-cap recall, lifecycle/scope/feedback drops, stale/unavailable projection, unresolved relation, & storage/provider failure. Every public Cortex recall/list/explain/graph result emits stable `completeness: exact|lower_bound`, considered/returned/dropped counts where knowable, & exhaustive machine cause codes. Adapter must preserve source omissions. CLI, service, & native Pull lane must agree byte-for-byte on semantic status. Qualification must execute production consumers at RELEASED boundary.
- Reuse/license: archive reports GitNexus under PolyForm Noncommercial 1.0.0 (`LICENSE`). Use as reference/clean-room behavioral reimplementation for commercial work unless separate rights exist. No donor code should be ported from this evidence.

### D-002 — material boundary correction, not Cortex NEW

- Donor evidence: archive `cortex_master_atom_list.md:673,714` & stage-3 CTX-001/CTX-030 rows cite Potpie `BackendCapabilities` + `DataPlaneStatus`, principally `potpie/context-engine/src/potpie_context_engine/core/ports/graph/backend.py` & `graph_service.py`. Donor symbols/callers are archive-supplied.
- Current owner evidence: `docs/canon/pull.md:19` (`PUL-003`) owns provider capability/readiness/omissions; `docs/canon/membrane.md:32,41,45` assign daemon readiness, Hub capability inventory, & memory health to `MEM-016`, `MEM-025`, `MEM-029`; `cross-subsystem-evidence.md:522-535` requires capability truth to converge in Hub.
- Current source: `MemoryStore::health_json`/`detailed_health_json` at `store.rs:6342-6395` expose Cortex-owned DB/embedder/write facts; service consumes detailed health at `serve.rs:4885`; `hub.rs:77-89` `provider_readiness_hub_read` turns readiness into Hub Available/Degraded/Unavailable. This is implementation under existing cross-subsystem atoms, not distinct Cortex ownership.
- Acceptance destination: qualify `PUL-003` + `MEM-016/025/029` using live Hub/provider path; require canonical store, lexical/vector projections, fallback, process/storage identity, & qualification state without inferring liveness from missing data.
- Reuse/license: archive reports Potpie Apache-2.0 (`LICENSE`), permitting reviewed reuse with notices; no direct port is required for ownership correction.

### D-003, D-005, D-007, D-010 — material REGISTER items

| Row | Current path + symbol + live consumer | Acceptance effect | Donor/reuse |
|---|---|---|---|
| D-003 | `engine/crates/membrane-runtime/src/store.rs:8271-8331` `MemoryStore::reindex`; live CLI consumer `engine/crates/membrane-runtime/src/cli.rs:4849-4858`. | Extend `CTX-029` qualification with one-row mutation proving unaffected vector/FTS rows remain byte-identical; full rebuild stays recovery. Current row scan + selective update is not proof of incremental FTS invalidation locality. | Archive cites GitNexus generation caches (`gitnexus/src/storage/parsedfile-store.ts`) + CodeGraphContext watcher (`src/codegraphcontext/core/watcher.py`). GitNexus PolyForm NC requires clean-room behavior; CodeGraphContext MIT permits reviewed reuse with notice. |
| D-005 | `engine/crates/membrane-runtime/src/mcp_executor.rs:985-1057,1061-1201` proposal table + review transition; live consumer is `membrane_knowledge_propose` MCP dispatch in same file. | `CTX-021/023`: proposal records expected source/canonical generation/version; approval fails typed-stale or refreshes before commit; one-decision & rollback-on-admission-failure remain. | Archive cites Potpie `potpie/context-engine/src/potpie_context_engine/core/workbench_service.py`, Apache-2.0; reviewed adaptation permitted with notices. |
| D-007 | `engine/crates/cortex-core/src/retriever.rs:17-18,33-105,136-156` `RRF_K`, `retrieve_hybrid_with_lexical_hits`, `retrieve_hybrid`; live recall calls at `engine/crates/membrane-runtime/src/store.rs:4518-4558`, native consumer `engine/crates/membrane-runtime/src/pull/federation_sources.rs:214-264`. | Repair `CTX-I013` evidence to exact current RRF symbols; qualify lexical-only, vector-only, conflicting-rank, fallback, & deterministic tie cases. Do not add atom. | Archive cites GitNexus BM25/vector fusion at `gitnexus/src/mcp/tools.ts`, PolyForm NC. Current implementation is independently present; no donor port needed. |
| D-010 | `engine/crates/membrane-runtime/src/store.rs:7920-7990` `MemoryStore::backup_cortex`; `7997-8039` `MemoryStore::restore_cortex`; current production consumer: none. | `CTX-026/036`: authorized surface, generation/embedder fence, atomic publication, projection rebuild, tamper/incomplete refusal, pre/post recall equivalence, & prior-authority recovery. | Archive cites GitNexus `gitnexus/src/storage/v8-sidecar.ts` / `parsedfile-store.ts`, PolyForm NC; clean-room behavioral reference only. |

## Cross-subsystem ownership

- Cortex: governed durable knowledge, admission, temporal/lifecycle truth, local retrieval, lane-local coverage facts, backup/restore. It is not independently resident (`CTX-D002`).
- Pull/Membrane planner: bounded acquisition, freshness/authority, hard eligibility, cross-provider RRF/fusion, sufficiency, budget, publication, material omissions, & receipts (`cross-subsystem-evidence.md:218-229`). This excludes D-011–D-014 from Cortex & limits D-001 to lane-local truth.
- Blueprint: repository parsing, source identity, graph generation, freshness, code-edge resolution, traversal, flows, impact, watcher, & code-index accelerators. This excludes D-015–D-023 except D-022 sanitation also touches Pull provider boundaries.
- Ledger: registered document corpus indexing/navigation/exact resolution (`cross-subsystem-evidence.md:196-207`). `CTX-033` may consume exact Ledger-fenced candidates only; it cannot own document truth or auto-retire facts on unavailable evidence.
- Adapt: behavioral interpretation/proposals; Cortex owns durable admission/storage/lifecycle/retrieval (`docs/architecture/subsystems/adapt.md:82-98`). D-005/D-006/D-009 must strengthen proposal/admission mechanics without granting Adapt direct write authority.

## Evidence defects & stale-target audit

1. Archive freezes Membrane/Cortex at `29adfc8e2fe5a2d43ed25634a91ebec3bb4070d3`, not requested `a9a4afb3eeaf4ee00869e8c303c50f810632f273`. Every “current” product assertion is therefore formally stale for current qualification.
2. Read-only diff proves no change from `29adfc8e` to `a9a4afb3` in `docs/canon/cortex.md`, operative `engine/crates/cortex-core`, `cortex-store`, `membrane-runtime/src`, `membrane-federation/src`, or `membrane-core/src`. Result: no archive Cortex source assertion is invalidated by intervening Cortex code; stale target bars qualification, but does not create a contradictory implementation finding. Current commit changes Blueprint/Ledger canon/provenance, so routed ownership must use current canon.
3. Stage-3 matrix has 36 rows only; `CTX-033` & D-001–D-023 are absent. It cannot serve as full 60-row migration accounting.
4. Stage-3 Membrane evidence often ends at `docs/canon/cortex.md#implementation-register` or broad files, not exact symbols + live consumers. D-007 is materially under-specified: current source already implements RRF, while archive presents it as a donor enhancement.
5. Archive donor evidence is one-file or broad-symbol evidence in several rows; exact production callers are often absent. Donor repositories are not among authorized current inputs, so donor claims cannot be independently revalidated here.
6. Archive states donor local-path validation was unresolved because its collection environment lacked network/repositories. Donor evidence remains advisory, not acceptance proof.
7. License states are archive observations only: Potpie/Graphiti/Aider Apache-2.0, CodeGraphContext MIT, GitNexus PolyForm Noncommercial 1.0.0. Before code reuse, reopen exact pinned donor commit + license text. GitNexus remains reference/clean-room-only on supplied evidence.
8. Current canon itself has evidence freshness debt: implementation receipts generally point to `f42b6c9`, all committed qualifications remain PENDING except `CTX-Q012` STALE, & no row is RELEASED-closed.

## Reconciliation & Foundation receipt

| Measure | Count |
|---|---:|
| Requested archive rows | 60 |
| Evaluated exactly once | 60 |
| EXISTING | 37 |
| NEW | 1 |
| REGISTER | 8 |
| DUPLICATE | 0 |
| OBSOLETE | 0 |
| EXCLUDED | 14 |
| UNRESOLVED | 0 |
| Sum of dispositions | 60 |

- Stable committed Cortex capability count remains 36; proposed net-new count is 1; exploratory current count remains 1.
- Archive “18 delivered / 17 partial / 1 missing” is implementation status at donor target, not migration disposition or closure. Current canon still records same 36 capability identities with 0/36 RELEASED closure.
- Structural self-check: all 37 CTX IDs are unique; D-001–D-023 are contiguous & unique; every one of 60 rows appears once in migration tables; excluded count is D-002 + D-011–D-023 = 14.
- Validator command: `py -3.11 D:\Claude\tools\skills\foundation\scripts\validate_atom_report.py <report> --mode final --expected-rows 60`.
- Validator result: `FAIL: missing table header: Scope | Domain | Atom | Current product | Best observed | Recommended implementation | Material gap | Why / tradeoffs | Source evidence | Confidence`. This reconciliation uses mandated disposition-map schema, not Foundation comparator-final schema; exact 60-row coverage was therefore checked structurally as recorded above.
- No tests, builds, generators, commits, or pushes were run.
