# Blueprint archive atom reconciliation

## Scope & frozen inputs

- Requested: reconcile all 60 archive atoms against current Blueprint canon, architecture, & operative source.
- Evaluated: 60/60 archive atoms at Membrane `a9a4afb3eeaf4ee00869e8c303c50f810632f273`.
- Archive target baseline: Blueprint `29adfc8e2fe5a2d43ed25634a91ebec3bb4070d3`; donor revisions remain archive-supplied claims.
- Current canon baseline: 68 COMMITTED capabilities, 1 EXPLORATORY capability (`BPT-048`), 0 canon rows renamed.
- Source surfaces inspected: `blueprint/**`, relevant `engine/**` ownership consumers, `docs/canon/blueprint.md`, & `docs/architecture/subsystems/blueprint.md`.
- No tests, builds, generators, repository edits, commits, or pushes were run.

Input fingerprints:

| Input | SHA-256 |
|---|---|
| `BLUEPRINT-MASTER-ATOM-LIST.md` | `376CAFBF5FC400F32A169F1A85A72A1DE03FC95DD15E079A682E88C5C9CC131C` |
| `master_atom_list.json` | `2BBFBA0BCBC168223AE8BF3501D85A373627769080B713AD73D752CC8167ED5A` |
| `docs/canon/blueprint.md` | `CE65779475DFCA8BFF04214850CE072E0B56FF91091FA0ABC1218E35B7BF74DF` |
| `docs/architecture/subsystems/blueprint.md` | `202E69B8BDFD34602C68A0E0B1F22996A557C8D1D527EDFCEF3A845EBBB6EA76` |

## Reconciled counts

| Disposition | Count | Counted capability effect |
|---|---:|---|
| EXISTING | 37 | Maps to preserved current stable IDs |
| NEW | 1 | One distinct observable candidate; no ID assigned |
| REGISTER | 14 | Implementation/reference records only |
| DUPLICATE | 2 | Archive bundles overlap multiple existing capabilities |
| OBSOLETE | 0 | — |
| EXCLUDED | 6 | Current Blueprint boundary rejects ownership |
| UNRESOLVED | 0 | — |
| **Total** | **60** | Exact archive denominator |

Current canon closure is `0/68` COMMITTED capabilities because every current Blueprint qualification row remains PENDING; archive intake does not change lifecycle state.

## Full migration map

Each archive atom receives exactly one disposition. EXISTING rows preserve one current stable capability ID. REGISTER rows may target several stable capabilities because they describe shared mechanisms, not new product behavior.

| Archive atom | Disposition | Current target / owner | Reconciliation |
|---|---|---|---|
| AT-001 Repository source discovery | EXISTING (`BPT-002`) | Blueprint | Current source observation already covers deterministic discovery accounting; `BPT-001` supplies root confinement. |
| AT-002 Incremental source-change detection | EXISTING (`BPT-002`) | Blueprint | HEAD/index/worktree, dirty overlay, & source hashes establish changed state; `BPT-021` governs incremental/full equivalence. |
| AT-003 Multi-language syntax parsing | EXISTING (`BPT-005`) | Blueprint | Pinned Tree-sitter grammar loading & explicit language capability are current contract. |
| AT-004 Symbol definition extraction | EXISTING (`BPT-004`) | Blueprint | Deterministic lexical symbols/occurrences/edges already cover baseline definition extraction. |
| AT-005 Reference extraction | REGISTER | `BPT-004`, `BPT-018` | Reference occurrences & unresolved records are extraction/resolution mechanisms, not independently closing user behavior. |
| AT-006 Import & module dependency extraction | EXISTING (`BPT-007`) | Blueprint | Current capability owns deterministic JavaScript/Python module/import binding. |
| AT-007 Call relationship extraction | REGISTER | `BPT-004`, `BPT-005`, `BPT-018` | CALLS production is provider/resolver implementation supporting Recall, impact, & flow consumers. |
| AT-008 Type & inheritance relationship extraction | REGISTER | `BPT-005`, `BPT-006`, `BPT-018` | Structural type edges are provider evidence, with SCIP overlay & shared resolution. |
| AT-009 Field & read-write relationship extraction | REGISTER | `BPT-005`, `BPT-018` | ACCESS/read-write edges are provider depth, not a separate product closure unit. |
| AT-010 Test relationship evidence | REGISTER | `BPT-034` | TESTS edges are evidence mechanism for test recommendation/exposure behavior. |
| AT-011 Framework & configuration semantic extraction | REGISTER | `BPT-008`, `BPT-009`, `BPT-053`, `BPT-054` | Archive row bundles HTTP, SQL, framework/event/database/deployment, & Terraform provider families already split by current canon. |
| AT-012 Receiver & type inference | REGISTER | `BPT-018` | Language-specific inference is shared exact-first resolution implementation. |
| AT-013 Compiler or indexer semantic overlay | EXISTING (`BPT-006`) | Blueprint | SCIP ingestion is current optional compiler-grade overlay contract. |
| AT-014 Evidence tier & provenance classification | EXISTING (`BPT-014`) | Blueprint | Current stable atom owns source/provider/version/generation/truth/confidence/freshness binding. |
| AT-015 Ambiguity & unresolved preservation | EXISTING (`BPT-018`) | Blueprint | Same-tier ambiguity stops & unsupported semantics remain typed. |
| AT-016 Persistent local graph store | EXISTING (`BPT-015`) | Blueprint | Immutable staged SQLite generations are current local graph authority. |
| AT-017 Deterministic generation identity | EXISTING (`BPT-013`) | Blueprint | Stable generation identity is included in current identity contract. |
| AT-018 Freshness & graph recovery | DUPLICATE | `BPT-019` + `BPT-056` | Archive row bundles two independently closing current behaviors: freshness honesty & corruption recovery. |
| AT-019 Exact symbol & path resolution | EXISTING (`BPT-023`) | Blueprint | Recall seed resolution is exact-first, bounded, ambiguity-preserving, & abstaining. |
| AT-020 Keyword & full-text search | EXISTING (`BPT-028`) | Blueprint | Current graph search owns bounded text/type/path query. |
| AT-021 Semantic vector search | EXCLUDED | Blueprint boundary | Canonical architecture §2.2(3), §12.6, §27, & §28.11 exclude vector retrieval from Blueprint. |
| AT-022 Hybrid retrieval fusion | EXCLUDED | Blueprint boundary | Canonical architecture §2.2(31) excludes semantic/hybrid additive ranking. |
| AT-023 Graph neighborhood expansion | EXISTING (`BPT-030`) | Blueprint | Bounded typed neighborhood is current stable behavior. |
| AT-024 Task-conditioned graph relevance | EXISTING (`BPT-024`) | Blueprint | Named task-shaped Recall traversal policies already own dependency/impact/callgraph/test/config/architecture/exploration modes. |
| AT-025 Token-budget context packing | REGISTER | `BPT-027`; final policy belongs Membrane planner | `buildNeighborhood` remains a live compatibility adapter, but final prompt/token admission is explicitly outside Blueprint; only bounded query traversal belongs here. |
| AT-026 Omission & completeness receipts | EXISTING (`BPT-025`) | Blueprint | Complete evidence paths carry completeness & omissions as current atomic Recall unit. |
| AT-027 Symbol 360-degree context | REGISTER | `BPT-025`, `BPT-029`, `BPT-030` | One-envelope UX composes resolve/show, bounded neighborhood, & path receipts; no distinct semantic owner. |
| AT-028 Repository architectural map | EXISTING (`BPT-040`) | Blueprint | Evidence-backed components, flows, & architecture views already occupy current stable scope. |
| AT-029 Strongly connected component & cycle analysis | REGISTER | `BPT-040` | Tarjan SCC is an architecture-view mechanism, not separate user behavior. |
| AT-030 Layer & topological architecture inference | REGISTER | `BPT-040`, `BPT-048` | Layer assignment is a derived-view mechanism; architecture-view hardening remains exploratory in `BPT-048`. |
| AT-031 Functional community & component discovery | EXISTING (`BPT-040`) | Blueprint | Observable component discovery is already in `BPT-040`; Leiden/community algorithms are implementation candidates, not new scope. |
| AT-032 Centrality, hub, & coreness analysis | REGISTER | `BPT-035`, `BPT-040` | Named metrics may inform risk/architecture views but cannot become production Recall authority. |
| AT-033 Dead & liveness candidate detection | EXISTING (`BPT-033`) | Blueprint | Current contract already limits output to LIVE/UNREACHED/UNKNOWN with evidence. |
| AT-034 Code complexity metrics | REGISTER | `BPT-035` | Complexity is one inspectable change-risk factor, not a standalone product capability. |
| AT-035 Entry-point discovery & scoring | REGISTER | `BPT-040` | Entry identification is flow-synthesis implementation; scoring must remain evidence-bound. |
| AT-036 Execution-flow & process discovery | EXISTING (`BPT-040`) | Blueprint | Current source now produces bounded flow inventories consumed through CLI & application service. |
| AT-037 Control, dataflow, & taint analysis | EXCLUDED | Blueprint boundary | Canonical architecture §2.2(11–12) excludes full program-analysis & general statement-level taint/dataflow platforms. |
| AT-038 Architecture fitness & boundary validation | NEW | Blueprint diagnostics; host/Adapt enforce | Read-only generation-bound architecture constraint findings are independently observable & not covered by current BP001–BP003 findings or synthesis atoms. No stable ID assigned. |
| AT-039 Reverse dependency & impact traversal | EXISTING (`BPT-032`) | Blueprint | Current stable atom owns bounded upstream/downstream impact. |
| AT-040 Diff-to-affected-process mapping | EXISTING (`BPT-032`) | Blueprint | Diff seeds & semantic impact already fall inside `BPT-032`; process/flow projection is missing implementation depth, not new scope. |
| AT-041 Hotspot & churn risk | EXISTING (`BPT-035`) | Blueprint | Current risk contract names inspectable factors & lower-authority co-change. |
| AT-042 Git co-change evidence | EXISTING (`BPT-035`) | Blueprint | Co-change is already an explicit lower-authority factor, not structural truth. |
| AT-043 Test-gap & exposure risk | EXISTING (`BPT-034`) | Blueprint | Recommended-test output already includes uncovered impact, coverage, reasons, & omissions. |
| AT-044 Pre-edit graph impact gate | EXISTING (`BPT-041`) | Blueprint decides; host enforces | Current orientation returns allow/continue/block/noop plus freshness/evidence/receipt/next action; enforcement remains outside Blueprint. |
| AT-045 Hash-verified graph-aware edits | EXCLUDED | Adapt/editor/execution host | Blueprint architecture §3.2 excludes edit execution, code rewriting, & host policy enforcement. |
| AT-046 Post-edit diff-impact verification | EXISTING (`BPT-032`) | Blueprint | Recomputed diff impact uses same existing impact contract; commit blocking remains host policy. |
| AT-047 Document discovery & indexing | EXISTING (`BPT-011`) | Blueprint evidence intake | Current contract ingests repository documents as evidence-bound declarations/claims, not document-navigation truth. |
| AT-048 Hierarchical document chunking | EXCLUDED | Ledger | Archive defines chunks as retrieval/navigation units; Ledger owns document navigation/index projections. Blueprint may consume source-bound claims only. |
| AT-049 Claim & reference extraction from documentation | EXISTING (`BPT-011`) | Blueprint | Rules/documents enter as claims with evidence, never observed code facts or authority. |
| AT-050 Document-to-code entity linking | EXISTING (`BPT-038`) | Blueprint | Current fact-to-claim binding owns grounded/unsupported/ambiguous/stale joins. |
| AT-051 Bidirectional document-code navigation | EXCLUDED | Ledger | Ledger owns document navigation/index projections; Blueprint owns claim/code evidence links & truth state. |
| AT-052 Documentation truth & drift verification | EXISTING (`BPT-039`) | Blueprint | Declared intent vs deterministic evidence, mismatch, citation, generation, confidence, & invalidation are current stable scope. |
| AT-053 State-bound verification receipts | EXISTING (`BPT-039`) | Blueprint | Current truth-verification behavior already binds generation, confidence, citation, & invalidation. |
| AT-054 Code-change to documentation invalidation | EXISTING (`BPT-020`) | Blueprint | Explicit dependency DAG invalidates derived artifacts from changed source/provider/config/schema/generation parents. |
| AT-055 Resident watcher & continuous refresh | EXISTING (`BPT-043`) | Blueprint under tray daemon | Current watcher/reconciler is tray-daemon-owned, stops with daemon, & types loss. |
| AT-056 Agent-facing graph query protocol | EXISTING (`BPT-042`) | Blueprint | Current stable atom owns application semantics across daemon IPC, one-shot, CLI, SDK, & MCP adapters. |
| AT-057 Readiness, freshness, & status surface | EXISTING (`BPT-046`) | Blueprint | Doctor/status diagnostics expose local graph/provider/freshness blind spots. |
| AT-058 Guided workflow & suggested-next actions | EXISTING (`BPT-041`) | Blueprint | Orientation already returns typed next action from evidence/freshness/omissions. |
| AT-059 Multi-repository federation & contracts | EXISTING (`BPT-047`) | Blueprint | Current stable atom preserves independent repo generation/evidence/omission slices. |
| AT-060 Task-scoped plan, grant, & seal lifecycle | DUPLICATE | `BPT-041`, `BPT-039`, `BPT-040`; final grants/plans belong Membrane/host | Archive bundles orientation, Phase-2 plan/seal, & access-grant enforcement that close under different owners. |

## Highest-value material candidates

### 1. NEW — AT-038 architecture fitness & boundary validation

- Distinct behavior: caller receives generation-bound architecture constraint violations/fitness results with exact graph evidence, omissions, & stable finding identity.
- Current source: `blueprint/src/lib/findings/registry.mjs#FINDING_RULES` registers only BP001–BP003 import findings; `blueprint/src/lib/findings/detect.mjs#detectFindings` emits those rules; live path is `blueprint/src/lib/findings/service.mjs#createFindingsService` → `blueprint/src/service/server.mjs#createDaemonServer` → `blueprint/src/service/client.mjs#findingsGet`. No architecture-fitness rule or boundary-policy surface exists.
- Archive donor: CALM `crates/calm-server/src/tools/orient.rs#fitness_report` at `ffafc36ad1580cc94bf7a8a3267c6f6aa209f070`; GitNexus `gitnexus/src/mcp/tools.ts#GITNEXUS_TOOLS` at `72edf400871c1589ceb975ad868909389249606a`.
- Owner/boundary: Blueprint owns read-only diagnostics & evidence receipts. Host/Adapt/CI owns enforcement or mutation refusal.
- Acceptance/qualification: frozen positive/negative architecture corpus; deterministic stable rule/finding IDs; generation/evidence hashes; explicit unsupported/partial omissions; repeatable daemon/CLI/MCP parity; stale generation fail-closed; no edit authority; RELEASED qualification.
- Reuse/license: archive omitted Stage 4 & supplied no license evidence. Donor behavior is reference-only until exact SPDX/license obligations are verified; greenfield behavioral reimplementation is safest current action.

### 2. `BPT-040` — community/component & process-flow implementation depth

- AT-031 does not add scope: component synthesis already belongs `BPT-040`. `blueprint/src/graph/architecture-model.mjs#buildArchitectureModel` performs layer/degree grouping but has no non-test live consumer at current commit, so it cannot prove current component discovery.
- AT-036 old absence is invalidated. Current `blueprint/src/graph/static-provider.mjs#graphFlowInventory` is consumed by `blueprint/src/lib/application/service.mjs#architectureFlowPage` & `blueprint/scripts/blueprint.mjs#build`/graph flow commands.
- Archive donors: GitNexus `gitnexus/src/core/ingestion/community-processor.ts#processCommunities`; CodeGraph `crates/codegraph-graph/src/lib.rs#communities`; GitNexus `gitnexus/src/core/ingestion/process-processor.ts#processProcesses`; CodeGraph `crates/codegraph-graph/src/lib.rs#flows` at archive-pinned revisions.
- Owner/boundary: Blueprint Phase-2 derived understanding. Community/centrality scores cannot enter production Recall evidence-priority ranking.
- Acceptance/qualification: `BPT-Q040`; require deterministic membership/flow identity, ordered evidence paths, every truncation cause, generation binding, component/flow goldens, no semantic naming before evidence membership, & released consumer parity.
- Reuse/license: unknown; reference-only pending Stage 4.

### 3. `BPT-032` — diff seed to affected process projection

- Current `blueprint/src/graph/traverse-store.mjs#indexedImpact` accepts one resolved `nodeId` & reverse-neighborhood depth. Live consumer `blueprint/src/lib/application/service.mjs#impact` resolves one anchor then calls `boundedImpact`. Diff/line/treeish seed families & affected-flow projection remain absent, matching current canon's PARTIAL state.
- Archive donors: GitNexus `gitnexus/src/mcp/tools.ts#GITNEXUS_TOOLS` (`detect_changes`) & CALM `crates/calm-server/src/tools/change.rs#diff_impact`.
- Disposition remains EXISTING because `BPT-032` already names diff/file/line/stack/test/treeish impact seeds. Implement within that stable ID.
- Acceptance/qualification: `BPT-Q032`; map changed lines → exact symbols → reverse impact → `BPT-040` flows/components; preserve confidence, source lines, omitted seed families, generation freshness, & no adjacency-as-impact overclaim.
- Reuse/license: unknown; reference-only pending Stage 4.

### 4. `BPT-014` — empirical evidence-tier calibration

- Current `blueprint/src/graph/confidence-tiers.mjs#tierConfidence` maps named tiers to fixed values. `blueprint/src/graph/static-provider.mjs#edge` stamps them; `buildGraphGeneration` is live through `blueprint/scripts/blueprint.mjs#build`.
- Archive donor: CodeGraph `crates/codegraph-cli/src/audit.rs#run` at `7bac941899221377251440eac9f2fc8afd164f38` claims compiler-oracle precision measurement.
- Difference is implementation/qualification under existing `BPT-014`, not NEW. Measure per-language/resolver precision & coverage against compiler-grade evidence without making measurements new truth authority.
- Acceptance/qualification: `BPT-Q014`; frozen per-language goldens, generation-bound calibration receipt, resolver/version identity, false-positive/abstention accounting, & unchanged explicit unresolved semantics.
- Reuse/license: unknown; reference-only pending Stage 4.

### 5. Boundary cleanup — AT-025, AT-021/022, & AT-060

- `blueprint/src/graph/neighborhood.mjs#buildNeighborhood` remains live through `blueprint/scripts/blueprint.mjs#runNeighborhood`, including `budgetTokens`. Treat it as `BPT-027` compatibility implementation only; final prompt/token policy remains Membrane planner ownership.
- `blueprint/src/graph/store-sqlite.mjs#upsertVectors/#searchSimilar` remain source residue with no non-test live consumer. `blueprint/src/providers/ranking/semantic.mjs` & `hybrid.mjs` are gone. Vector/hybrid Blueprint atoms stay EXCLUDED.
- `blueprint/src/lib/receipt-store.mjs#issueScopeGrant/#checkScopeGrant` remain live through `blueprint/scripts/blueprint.mjs#runGrantCommand`; `blueprint/src/lib/incremental-phase2.mjs#sealPhase2Artifacts` remains live through `runPhase2Command`. These do not justify archive AT-060's bundled capability: access enforcement, agent plan, Phase-2 seal, & orientation have separate owners/closure.

## Cross-subsystem ownership issues

| Archive concern | Blueprint-owned slice | External owner |
|---|---|---|
| AT-025 context packing | Bounded graph traversal/response receipt (`BPT-027`) | Membrane planner/Pull owns final admission, prompt assembly, & token policy |
| AT-044 pre-edit gate | Evidence-backed orientation decision (`BPT-041`) | Host/Adapt/editor owns enforcement |
| AT-045 graph-aware edits | Generation/impact receipt only | Host/Adapt/editor owns hash-verified mutation |
| AT-048/AT-051 documents | Claims, fact links, truth, freshness | Ledger owns document navigation/index projections |
| AT-060 task lifecycle | Blueprint orientation & Phase-2 evidence seal | Membrane planner/Pull/host owns final grant, plan, publication, & enforcement |
| AT-021/AT-022 semantics | No Blueprint lane | Cortex may use embeddings for durable memory; Pull may fuse provider candidates; neither changes Blueprint truth |

No transfer makes donor behavior committed automatically. Owner canon must accept scope separately.

## Evidence defects & invalidated archive claims

1. Archive target evidence is revision-stale: Blueprint was reviewed at `29adfc8...`, not current `a9a4afb3...`. Every archive Blueprint `Observed` cell is unsuitable as current qualification.
2. Archive's 360 matrix cells use generic `live_consumer: "production graph/service path identified in scoped source pass"`; this is not an exact caller/consumer & fails Foundation evidence-tuple requirements.
3. CodeGraph corpus note says GitHub contents came from `main` after resolving a head because connector rejected SHA. Those path reads are not proven commit-bound to `7bac9418...`.
4. Donor rows often cite umbrella symbols such as GitNexus `GITNEXUS_TOOLS`; they do not identify exact dispatch function or downstream consumer.
5. Archive omitted Stage 4 license/reuse. No donor direct-port, translated-port, or dependency action is authorized by archive evidence.
6. Archive AT-036 Blueprint absence is invalidated by current `graphFlowInventory` plus application/CLI consumers.
7. Archive AT-040 absence is narrowed, not fully invalidated: current node-anchor reverse impact exists, while diff seed families/process projection remain missing.
8. Archive AT-021/022 recommendations conflict with current canonical OUT decision. Vector store helpers remain unconsumed residue, not operative behavior.
9. Current canon's implementation receipts mostly cite `f42b6c...` & all qualification rows remain PENDING. Current source inspection proves mechanism/caller presence only, never RELEASED qualification or closure.
10. No archive donor repository was present in requested operative source roots, so donor claims were reconciled as archive evidence only, not independently re-opened.

## Foundation receipt & final accounting

- Product/scope: Blueprint archive intake; 60 supplied atoms.
- Target revision: `a9a4afb3eeaf4ee00869e8c303c50f810632f273`.
- Platform/runtime set: source inspection on Windows; no installed/device/release qualification.
- Corpus: supplied archive matrix for Blueprint, CodeGraph, GitNexus, Aider, CALM, & Lattice at archive-declared revisions.
- Applicability: Blueprint code-intelligence subsystem only; cross-subsystem concerns dispositioned by current Membrane ownership.
- Exclusions: vector retrieval, hybrid ranking, general taint/dataflow, edit execution, hierarchical document retrieval chunking, & bidirectional document navigation.
- Material fingerprint invalidation: any change to archive files, current Blueprint canon/architecture, target commit, owner boundaries, or donor license evidence invalidates affected rows.
- Foundation protocol: current installed Foundation `model.md` + `protocol.md`, RECONCILE mode.
- Structural self-check: 60 unique migration IDs (`AT-001`–`AT-060`); disposition sum 60; 0 missing IDs; 0 unclassified rows.
- Foundation validator: invoked in `final` mode with `--expected-rows 60`; result was structural `FAIL` because validator only accepts Stage-3 comparison header `Scope | Domain | Atom | Current product | ...`, while this requested artifact is a disposition reconciliation. This does not represent a semantic row failure; no Stage-3 table was fabricated to satisfy an inapplicable schema.

```text
Foundation: Blueprint archive intake
Count view: CAPABILITY
Capabilities: current canon 0/68 closed; 68 open; archive contributes 1 NEW candidate
Non-counted archive decisions: 14 REGISTER; 2 DUPLICATE; 6 EXCLUDED
Unclassified: 0
Verdict: PASS

requested / evaluated / unresolved / excluded = 60 / 60 / 0 / 6
```
