# Cortex best-of-market execution plan

Date: 2026-08-12

Status: derived execution plan; implementation stopped; no completion claim

Authority: [`../../sol.md`](../../sol.md). This plan cannot change its invariants, requirements, dispositions, or gates.

Repository: canonical `/Volumes/D/claude/cortex` only; `cortex2` is forbidden.

## Outcome

Deliver every `CX-R001..CX-R036` requirement through consumer-compatible packages that are independently releasable only when their partial-state contract is green. Final product maps every file, reports capability depth honestly, resolves globally without ghost edges, searches exactly at repository scale, ingests precise compiler/LSP facts where available, falls back structurally everywhere, optionally ranks semantic relevance without deciding Membrane packet authority, remains local-first, & runs only as a Hub-owned child.

## Requirement closure map

| Book requirements | Accountable acceptance owner | Predecessors | Partial-state invariant | Not green until |
|---|---:|---|---|---|
| CX-R001–R004 | Cortex integration owner / P2 | 0 → 1 | Existing query/doctor schemas stay readable while U0–U5 fields are additive | Coverage + truthfulness cells; P1 disposition + P2 lattice receipts |
| CX-R005–R008 | Cortex integration owner / P3 | 0 → 2 | Old registry path remains authoritative until one pipeline has consumer parity | Truthfulness + compatibility cells; P3 end-to-end receipt |
| CX-R009–R012 | Cortex graph owner / P4 | 0 → 3 | Last valid generation remains queryable; no new snapshot schema adopted early | Correctness + snapshot + recovery cells; P4 churn/equivalence receipt |
| CX-R013–R016 | Membrane Hub owner + Cortex service owner / P5 | 0; Membrane P1 protocol release | Existing lifecycle remains installed until Hub protocol compatibility is proven | Lifecycle + portability cells; joint installed receipt |
| CX-R017–R020 | Cortex storage/query owner / P10 | 0 → 4 → 6 | Exact API & stored schema stay compatible; migration is rebuildable/backout-safe | Query + storage + recovery cells; P6/P10 receipts |
| CX-R021–R024 | Cortex provider owner / P8 | 0 → 2 → 3 → 7 | New providers are disabled/unadvertised until positive production qualification | Coverage + truthfulness + compatibility cells; P7/P8 receipts |
| CX-R025–R028 | Cortex retrieval owner / P9 | 0 → 3 → 8 | Exact/structural order remains default; semantic/policy additions stay additive or disabled | Retrieval + correctness + privacy cells; P8/P9 receipts |
| CX-R029–R032 | Cortex integration owner / P10 | 0 → 4 → 6 | No export/schema becomes default before recovery & old-reader behavior pass | Compatibility + recovery + delivery cells; P10 receipt |
| CX-R033–R036 | Cortex release owner / P10 | 0 then all owners | No performance/market/delivery claim from partial packages | Evidence + comparative + portability + delivery cells; final release receipt |

Named owner produces with package's checked-in command into `artifacts/receipts/CX-R###-R###.json`; independent Oracle verifies source/book/protocol/artifact digests & acceptance cells before green. Only named owner may propose green; validator + verifier establish it. A package may commit before downstream completion only when its partial-state invariant, compatibility/migration gate, rollback, & current acceptance cells pass; otherwise it is an integration checkpoint, not independently shippable.

## Final-absorption package map

| Packages | Mandatory book closure rows |
|---|---|
| 0, 2 | CX-F02, F05–F06, F20–F22 |
| 1–3, 7–8 | CX-F15–F18, F27 |
| 4–6 | CX-F08, F10–F11, F19, F24 |
| 5, 10 | CX-F17, F25 |
| 8–9 | CX-F01, F03–F04, F07, F09, F12–F14, F21–F23, F26 |

No package may mark a broad `CX-A`, `CX-H`, or `CX-R` row green while any mapped `CX-F` receipt remains absent.

## Frozen starting evidence

- Canonical doctor reported `broken` on 2026-08-12: stale graph/provider identity, Merkle mismatches, missing understanding/verdict artifacts.
- Working tree contains an uncommitted language-coverage overlay plus unrelated generated-doc drift. Neither is accepted baseline.
- Measured 550-file cold build: `23.4 s`, `899 MB RSS`; lexical resolution `21.2 s`, Tree-sitter `2.2 s`.
- Measured one-file delta: `77.9 ms`; no-op barrier: `2.7 ms`.
- Exact in-process queries are already below `5 ms`; external startup dominates CLI latency.
- `symbol_terms` + secondary index consumed roughly half measured DB allocation; free pages were negligible.
- Existing `delta-store` owns Merkle/file/artifact state, `MAX_HOPS=2`, & `MAX_DEPENDENT_FILES=500`.
- Indexed global re-resolution already exists in committed source & remains a soundness choice; bounded resolved-edge closure caching is excluded.
- Semantic provider is disabled & ranking/compiler/framework/IaC/schema modules are not wired through production end to end.

All measurements remain observations until benchmark manifests bind command, fixture, commit/tree, host, toolchain, samples, raw output, & peak RSS.

## Delivery law

1. Freeze output equivalence before optimization.
2. Land one work package at a time; each package has its own tests, measurements, rollback, compatibility proof, commit, & remote receipt only when its partial-state invariant permits release.
3. Reject any change that alters ordered exact results, graph facts, omissions, provider/generation identity, or ghost-edge outcomes unless `sol.md` explicitly requires that new behavior.
4. Never relax fixtures, thresholds, ordering, expected omissions, timeouts, or sample counts to make a change pass.
5. A tested module is not a product capability until registry → qualification → build → store → query → doctor is complete.
6. Re-measure after each package. Stop adding machinery once all frozen gates pass.

## Work package 0 — establish admissible baseline

Requirements: CX-R033–R036.

- Inventory every dirty path & classify it as prior implementation overlay or unrelated user drift without modifying either.
- Reproduce canonical `main` in an isolated temporary clone only for comparison; do not switch this checkout or create a worktree.
- Freeze graph fact/order corpus, ghost-edge add/delete/move/rename/ambiguity fixtures, retrieval corpus, no-op byte identity, interruption recovery, & lifecycle process census.
- Freeze cold/no-op/delta/100-file/5,000-file benchmark manifests on Mac & Windows.
- Freeze `benchmark-protocol.v1.json`, `cortex-competitors.v1.json`, atomic `CX-H` closure, public-surface inventory, supported-version window, & exact canonical `sol.md`/plan digests. Every later receipt names those digests.
- Freeze package capacity manifest: owned file paths, prerequisite artifacts, measured baseline paths/lines, projected changed-line ceiling, native-host effort, acceptance cells, & aggregate remaining work.
- Record current provider registry, extension/file/byte counts, capability vectors, fallback reasons, & doctor output.

Exit: a failing baseline is accepted as truth; test harness is green against frozen expected behavior & cannot be edited by later packages.

Package 0 contributes immutable Cortex source/ref/tree/commit/artifact/protocol/book/receipt inputs to parent workspace integration owner. That owner is sole serialized writer of root `artifacts/releases/context-stack-release.v1.json`; manifest version + prior digest use compare-and-swap, & any divergent/stale child tuple is rejected. Manifest also seals Membrane Hub tuple, supported lifecycle ranges, native install source, joint test commands, & receipt digests. Exact sequence: pre-install commits exist → native artifacts build from those commits → parent owner seals one tuple → joint installed tests run against sealed artifacts → nested refs push unchanged → parent pins those exact commits → remote refs/pins verify. Any child/source/artifact/receipt mismatch or post-test change invalidates joint proof.

## Work package 1 — universal file disposition

Requirements: CX-R001–R004. Absorbs CX-A01–A03.

- Make discovery emit one U0–U5 disposition for every tracked, non-ignored file.
- Add U1 opaque artifact facts: type/MIME, byte size, content hash, Git metadata, generated/binary classification, references.
- Add U2 lexical/document/config facts for all text not handled by richer providers.
- Add U3 generic structural provider with explicit parse confidence & error coverage.
- Publish unexplained file/byte remainder; acceptance requires zero.

Exit: 100% file disposition fixture across source, docs, config, schema, lockfiles, generated files, media, archives, models, binaries, unknown extensions, Unicode, case collisions, & platform path forms.

## Work package 2 — capability lattice & qualification

Requirements: CX-R003–R008. Absorbs CX-A04, CX-A05, CX-A09.

- Replace language-count claims with generated capability matrix.
- Qualify discovery, syntax, symbols, imports, definitions, references, calls, types, implementations, frameworks, schemas/IaC, dataflow/security, tests, & refactors independently.
- Bind every cell to provider/version, fixture hash, platform, result, fallback, & last successful receipt.
- Doctor reports `qualified`, `fallback`, `unsupported`, or `failed`, never inferred marketing support.

Exit: checked-in fixtures cover every advertised extension & every level-2/3 capability on Mac + Windows.

## Work package 3 — one provider pipeline

Requirements: CX-R005–R008, CX-R021–R024.

- Create one production registry consumed by qualification, build, store, query, doctor, SDK, & MCP.
- Wire existing Python SCIP, Python module resolver, framework, Terraform, SQL, hybrid-ranker, & provider-interface islands or mark them absent.
- Enforce provider permissions, protocol range, input digest, output schema, time/memory/result limits, cancellation, & typed failure.
- Store provider facts through one versioned language-neutral fact schema.

Exit: deleting any registry entry removes corresponding product capability & turns its qualification/doctor row explicit; no test-only capability remains advertised.

## Work package 4 — qualify resolution & remove remaining rebuild work

Requirements: CX-R009–R012.

- Qualify existing shared `filesByPath`, `symbolsByName`, imports-by-file, package/module, schema/config, & provider indexes against frozen graph/ghost-edge fixtures.
- Profile current clean source after indexed resolution; treat old `21.2 s` resolver evidence as historical, not current bottleneck proof.
- Implement true unchanged-source fast path, remove duplicate generation serialization/hash, & reuse scanned bytes/hashes across providers.
- Fix generic tier-2 call extraction or mark each affected capability `UNSUPPORTED`; zero emitted facts never qualify a positive capability.
- Preserve global add/delete/move/rename/ambiguity semantics exactly.
- Implement & qualify `BuildSnapshotV1`; stage facts/indexes only under one scan/provider/schema/resolver/source identity, compare-and-swap adoption, & reject/restart any mismatch.
- Add source/provider/resolver churn during build plus cancellation/crash fixtures proving no mixed snapshot becomes queryable.

Exit: frozen graph is semantically identical; ghost-edge suite passes; no-op avoids rebuild/publication; 550-file cold build meets `<5 s` & `<300 MB RSS` or remaining profile identifies next measured bottleneck without changing resolution semantics.

## Work package 5 — qualify daemon build & close Hub lifecycle

Requirements: CX-R013–R016.

- Retain existing cancellable build singleflight keyed by canonical repo, output, source fingerprint, provider set, & schema; remove any bypass path or mismatched identity semantics found by conformance tests.
- Join equivalent callers; isolate waiter cancellation; replace incompatible stale work through existing cancellation path.
- Route exact reads through resident service.
- Consume shared `hub-child-lifecycle.v1`: protocol version, executable/artifact hash, host/install + Hub instance/process identity, monotonic fencing token, lease, inherited liveness handle, readiness, drain, exit taxonomy, restart/backoff, update, & process-tree identity.
- Reject stale/lower/foreign Hub fences even with a live old handle; higher-fence handoff drains prior owner before readiness; ambiguous/lost owner proof exits boundedly.
- Remove independent startup/persistence/self-restart paths only after Membrane Hub publishes compatible protocol proof. Hub owns children, readiness, drain, kill, & optional system startup.

Exit: concurrent-build fixture executes one build; interrupted publication preserves prior generation; concurrent/stale Hub & lease-store-loss fixtures prove one fenced owner; Hub-off process census is zero on Mac + Windows.

## Work package 6 — exact search & compact storage

Requirements: CX-R017–R020. Absorbs CX-A06–A08, CX-A13.

- Prove supported consumers for FTS & `symbol_terms`; select one exact authority.
- Benchmark compact FTS/terms against positional trigram prototype on exact, prefix, substring, regex, Boolean, symbol, update, size, & memory corpora.
- Normalize generation/symbol/token identities to integer rows where measured beneficial.
- Prepare/batch affected rows through one SQLite writer; never rewrite whole generation for ordinary delta.
- Migrate by typed rebuild-required transition with N-2/N-1 fixtures, rollback, atomic adoption, & recovery.

Exit: exact API meets p95 `<5 ms`; no-op `<1 s`; delta p95 `<100 ms`; 100-file update improves `>=10×`; storage/update cost beats frozen baseline without relevance loss.

## Work package 7 — precise semantic providers

Requirements: CX-R021–R024. Absorbs CX-A04, CX-A05, CX-A16, CX-A24.

- Define compiler/LSP/SCIP adapter ABI, sandbox, permissions, version/probe, source digest, incremental update, & stale behavior.
- Prioritize TS/JS, Python, Rust, Go, Java/Kotlin, C/C++, C#, Ruby, & Swift based on repository byte coverage, not marketing order.
- Import portable SCIP/LSIF with provenance; support local LSP adapters where compiler indexers are absent.
- Preserve U4/U3/U2 fallback for every provider failure.

Exit: each precise capability has compiler/LSP-derived fixture proof; unsupported semantic dimensions remain explicit while file coverage stays complete.

## Work package 8 — grammars, frameworks, schemas, IaC, policies

Requirements: CX-R021–R028. Absorbs CX-A02, CX-A09, CX-A15, CX-A19.

- Add signed local custom-grammar registration & conformance.
- Wire framework routes/events/ORM/deploy/test facts, SQL/schema relations, Terraform/IaC resources, package/build/workflow ownership.
- Add dependency policies, cycles, forbidden edges, trust boundaries, change impact, dataflow/security adapter results.
- Every report cites facts, provider, generation, uncertainty, omissions, & source ranges.

Exit: representative polyglot apps produce qualified cross-language/framework/data relationships & deterministic policy results.

## Work package 9 — explainable hybrid retrieval

Requirements: CX-R025–R028. Absorbs CX-A10–A14, CX-A17–A18, CX-A21.

- Keep exact/structural/graph lanes authoritative.
- Add optional local semantic candidates behind provider availability & explicit enablement.
- Rank with visible exact, lexical, semantic, graph centrality, change relevance, ownership/history, diversity, & source freshness evidence. Cortex never assigns packet authority or authorization.
- Add typed abstention & deterministic exact fallback.
- Never upload source/embeddings by default; any egress requires explicit provider permission & receipt.

Exit: held-out retrieval/whole-task results improve without exact-order, privacy, latency, RSS, determinism, or fallback regression. Otherwise semantic remains disabled.

## Work package 10 — cross-repository operation, recovery, & release proof

Requirements: CX-R017–R020, CX-R029–R036.

- Add branch/repository identity, ownership/history overlays, cross-repo symbol references, bounded pack export, schema compatibility, backup/restore, diagnostics, & repair.
- Exercise corrupt DB, stale provider, missing grammar/compiler, blocked WAL reader, interrupted build, source churn, daemon death, Hub loss, & downgrade.
- Produce Mac + Windows qualification, performance, lifecycle, & recovery receipts.
- Run public CLI/MCP/daemon/SDK/schema/artifact compatibility matrix & migrations across supported version window.
- Run frozen same-corpus competitor adapters under shared statistical evaluator; retain target wording unless noninferiority + dominance passes.
- Release sequence is strict: Membrane Hub protocol commit/release + compatibility receipt → Cortex child adoption → installed joint Mac/Windows process-census/fault receipt with matching protocol/artifact hashes → Cortex commit/push → parent pin/push.
- Commit/push Cortex; then pin/push parent gitlink; verify remote SHAs.

Exit: every `CX-A` ledger row has `ALREADY+proof`, `ADOPT+receipt`, `GATE+decision`, or enforced `REJECT`; every CX-R requirement is green.

## Frozen acceptance matrix

| Axis | Gate |
|---|---|
| Coverage | `U1..U5 + U0 = 100%`; unexplained files/bytes `0` |
| Truthfulness | Capability cells generated from current production receipts; no module/test-only claims |
| Correctness | Ordered exact results, graph facts, ghost edges, omissions, provider/generation identity equivalent |
| Snapshot | One `BuildSnapshotV1`; source/provider/resolver churn exposes `0` mixed-generation rows/results |
| Retrieval | Hit rate `>=86.7%`; MRR `>=0.800`; semantic must improve held-out outcomes to enable |
| Cold build | 550 files `<5 s`, peak RSS `<300 MB`; 5,000 files `<60 s`, peak RSS `<1 GB` |
| Incremental | no-op `<1 s`; one-file delta p95 `<100 ms`; fixed 100-file update `>=10×` baseline improvement |
| Query | resident exact lookup p95 `<5 ms`; deterministic under concurrency |
| Storage | Selected index beats current bytes/update work; no forced target-size claim; old-generation residue bounded |
| Recovery | Previous generation survives interruption/corruption; migrations rebuild/rollback deterministically |
| Lifecycle | exactly one fenced Hub owner; Hub-off process census `0`; Hub quit drains/kills; no independent persistence artifacts |
| Portability | Identical source/provider/schema contract passes native Mac + Windows |
| Compatibility | CLI/MCP/daemon/SDK/schema/artifact supported-version matrix `100%` |
| Comparative | Frozen eligible manifest; all-axis noninferiority + one material dominance or target wording only |
| Evidence | Shared statistical protocol complete; no missing/failed/censored samples; receipt book/tree/artifact digests exact |
| Delivery | Clean full suite, receipts, nested commit/push, parent pin/push, remote SHAs verified |

## Excluded shortcuts

- No fact-level hop-bounded resolved-edge cache.
- No lock file competing with daemon coordination.
- No mandatory ANN, embeddings, hosted backend, or multi-store architecture.
- No parser/compiler rewrite when qualified Tree-sitter/LSP/SCIP provider exists.
- No source mutation/codemod surface in Cortex core.
- No independent launchd/Task Scheduler/login/startup registration.
- No worker pool until current measured bottleneck reaches parallelizable work & RSS gates prove bounded benefit.

## Measured capacity control

Fixed productivity arithmetic is forbidden. Package 0 creates `docs/plans/capacity/cortex-best-market.v1.json` under Cortex integration owner; product authority approves initial ceiling & any revision before implementation. Schema records package, owned files, baseline LOC, projected changed LOC, focused-test active minutes, native-host active minutes, external wait separately, prerequisites, acceptance cells, uncertainty range, contingency `<=10%`, & unmapped-work count `0`. Estimates derive from path-level baseline + one measured representative change per work type, never a global productivity rate; before every package starts, validator rejects missing paths, double-counting, unmapped requirements/work, ceiling increase without new authority revision, or package high estimate exceeding aggregate remaining allocation. Product authority must split/resequence/revise before any over-cap package work begins.

| Package | Prerequisite artifacts | Acceptance owner/cells | Capacity fields frozen at P0 |
|---:|---|---|---|
| 1–3 | baseline, aspect registry, provider/surface inventory | coverage, truthfulness, compatibility | owned paths, baseline lines, projected delta, native-host effort |
| 4–6 | snapshot schema, graph/equivalence corpus, exact-index migration design | correctness, snapshot, incremental, query, storage, recovery | same fields + migration/backout work |
| 7–9 | qualified provider ABI, policy boundary, holdouts/comparator adapters | truthfulness, retrieval, privacy, comparative | same fields + provider/toolchain/runtime cost |
| 10 | all acceptance-owner receipts, Hub protocol release | recovery, lifecycle, portability, delivery | exact remaining paths, native release/install effort, parent pin work |

After every package, receipt records actual changed files/lines, active engineering time, native-host wait separately, completed acceptance cells, & aggregate remaining capacity. No “all requirements green” or final-package start is allowed when mapped remaining work exceeds frozen remaining ceiling; product authority must split/resequence scope without weakening this book.
