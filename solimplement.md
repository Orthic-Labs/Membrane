---
cortex:
  document_id: cortex-sol-implementation-plan
  type: plan
  status: accepted
  effective_from: 2026-08-14
  scope:
    deployable_units: [cortex]
    branches: [canonical]
  canonical_for: [implementation-sequencing, implementation-status, delivery-gates, retirement-of-predecessor-plans]
---

# Cortex implementation plan

## Live status — 2026-08-15

- W0/F-CX & all six Phase A source owners are complete.
- Integrated working-tree candidate has 19 staged paths at baseline `cdfd7de41dc17494cd8ba6975508b3f44defcb13`.
- `pnpm test`, `pnpm test:all` (912/912), security (27/27), hardening (20/20), qualification, AX (15 pass, 3 pending skips), & full doctor pass.
- Mac arm64 unsigned archive exists at `release/candidates/darwin-arm64/cortex-darwin-arm64.tar.gz`, SHA-256 `1579c85bc3c4ad292cbad1557c1b253e9027934fd7105ca259d7fa9a757416d8`; RightKit sealing/signing & Windows artifact remain.
- W7/W8 are **not complete**: independent certification, native artifacts, Cortex commit/push, Orthic adoption, installed Mac/Windows proof, parent pin, publication, & remote verification remain.

Current receipts: workspace `tasks/evidence/orthic-suite/integration/cortex/**`. This status annotates execution only; requirements, gates, & ownership below remain authoritative. Historical receipts retain original authority hashes.

Authority: [`sol.md`](sol.md) owns product target, performance target, competitor absorption, & research disposition. Workspace [`SEAM-CONTRACT.md`](../docs/plans/orthic/SEAM-CONTRACT.md) owns every Cortex ↔ Membrane ↔ Orthic boundary. This file owns Cortex implementation order, child-side conformance, open-work classification, evidence gates, & delivery closure. Code, `cortex doctor --full --json`, qualification receipts, native-host receipts, Git state, & remote state own current operational truth.

No other Cortex implementation plan is current. Predecessor plans are archived under `../docs/archive/cortex-pre-solimplement-2026-08-14/`; retained evidence remains evidence, not competing authority.

## 1. Outcome

Deliver one local-only Cortex system that:

- accounts for 100% of tracked, non-ignored files through U0–U5;
- publishes qualified capability vectors, provider identity, provenance, generation, confidence, omissions, & fallback reasons;
- builds one atomic `BuildSnapshotV1` through one scan, registry, store, publication path, coordinator, & doctor;
- serves deterministic exact search plus optional local semantic candidates without granting semantic authority;
- remains read-only through MCP, Hub-owned through lifecycle, zero-egress, recoverable, portable, & versioned;
- meets every CX-R001–CX-R036 gate plus every applicable CX-F01–CX-F165 disposition in `sol.md`;
- earns any “best” claim only when correctness/performance plus AX both pass, frozen same-corpus comparison proves all-axis noninferiority, & one material axis wins. Backend speed cannot substitute for AX.

## 2. Authority & evidence order

Use this order when text conflicts:

1. `sol.md` frozen invariants, requirements, feature dispositions, thresholds, & rejections.
2. Current production code plus generated schemas/manifests.
3. Current qualification, test, doctor, benchmark, Mac, & Windows receipts.
4. This plan's sequencing/status classification.
5. Retained baselines, bakeoffs, audits, reviews, & historical plans.

“Implemented” means production registry → qualification → build → store → query → doctor is wired where applicable. Module presence, test fixture presence, old receipt, or plan prose alone means `landed_unqualified`, not complete.

### Full-scope dispatch rule

Every W0–W8 obligation remains binding. Workspace [`tasks/plan.md`](../tasks/plan.md) compiles them into file-exclusive dispatches for minimum elapsed time; it changes no requirement, threshold, evidence gate, experiment disposition, comparison, publication, or delivery state. W0 truth plus W6 gate decisions freeze before source effects. Any W6 outcome admitted into production is implemented by that destination file's sole owner during its one implementation pass. W1–W5 may execute concurrently against frozen interfaces. W7 producer code may be prepared in that source wave, but W7 evaluation, claims, artifact production, & publication start only after integrated behavior receipts. W8 remains integration-owner closure.

No production/test/config path may appear in two dispatch ownership sets. Generated outputs belong to generator owner. A new or omitted path must be assigned by integration owner before edit. Repair returns to same owner; no cleanup dispatch reopens another owner's file.

## 3. Inputs absorbed

| Input | Use | Lifecycle |
|---|---|---|
| `sol.md` | Frozen contract; CX-I01–I24, CX-R001–R036, CX-F01–F165 | Active authority |
| Current `src/`, `bin/`, `test/`, `evals/`, `release/`, `package.json` | Shipped baseline & executable surface | Active truth |
| `deepseek.md` | AX sequencing & competitor-mechanism proposal | Superseded by this file; retained in place |
| `docs/plans/cortex/**` | Productization, architecture, packaging, roadmap, dispatch, audit, contracts, ratings | Archived |
| Cortex/Blueprint plans outside that pack | Historical sequencing, language coverage, incremental build, Explorer | Archived when fully superseded |
| Workspace vector-backend bakeoff run dated 2026-07-27 | Vector-backend bakeoff trail | Preserved active |
| `docs/baselines/**`, `docs/evidence/**`, `docs/reviews/covenant/**` | Baselines, receipts, independent review trail | Preserved active |
| Workspace [`SEAM-CONTRACT.md`](../docs/plans/orthic/SEAM-CONTRACT.md) | Normative cross-product Hub/Membrane/Cortex boundary | Active authority |
| Prior Cortex seam executable contract | Requirements mapped into W0/W2/W3/W7; evidence & rationale only | Historical |
| Rules, gotchas, release/signing docs, incident records | Workspace execution constraints | Preserved active |

Inventory basis: 997 files under workspace `docs/`; 212 mention Cortex; direct Cortex plan pack held 15 files plus predecessor Cortex/Blueprint plans. Retirement is topic-scoped: unrelated product docs remain unchanged.

## 4. Conflict resolutions

These decisions correct stale implementation prose without changing `sol.md`:

| Topic | Binding implementation decision |
|---|---|
| Vector database | No mandatory vector DB, ANN authority, hosted embedding, second truth store, or repository/model egress. Optional local semantic lane remains eligible only through CX-A23/CX-F75/CX-F156 gates. Preserve bakeoff evidence. |
| Direct-page SQLite writer | CX-F158 is `GATE`, disabled by default. Prototype outside production path; land only after byte-equivalence, integrity, crash-safety, cross-platform, maintainability, & material-win proof. SQL path remains fallback. |
| Fused decompression + Aho-Corasick | CX-F159 is `GATE`, disabled by default. Land only after deterministic-result, UTF-8/binary, cancellation, RSS, exact-latency, & update-cost proof. |
| Compression portfolio | Do not substitute deflate/brotli by assertion. CX-F160 preserves LZ4 fast-path plus LZ4 HC/Zstd size-path as independently optional codecs; each needs dependency/security, deterministic format, corruption, migration, compatibility, & benchmark admission. No codec becomes authority. |
| Quantized vectors/IDF | Model-free TF-IDF/RRF under CX-F33 is distinct from CBM's int8 IDF-enriched token/vector blobs. CBM representation remains derived-material research under CX-F156/CX-F161—not a BM25/exact schema under CX-F126—and cannot affect exact truth or citations. |
| Dependencies | Current shipped dependencies are baseline. Additions require qualification, license/security review, lockfile identity, offline operation, & measurable value; “no dependencies” is not a truthful current-state claim. |
| Atomic publication | Use proven platform primitives behind one publication contract. Do not claim handle-bound exclusivity from generic Node rename behavior; qualify POSIX & Windows crash/concurrency semantics directly. |
| SQL/parser pools | Consolidate resource-handle lifecycle. Cognee-style pool modes inform CX-F118/CX-F162; they do not import Cognee runtime or create multiple stores. CodeCompress remains negative evidence for pool claims. |
| Roam semantic compression | Rejected by CX-F164. No freeform LLM summary, semantic cache authority, or remote compression enters Cortex. |
| Synced/shared store paths | CX-F165 adapts Roam's provider warning but not its safety claim. Central detector combines provider markers with platform mount/share probes, reports uncertainty, & routes all mutable state to explicit owner-only local storage or refuses typed. DELETE journal, EXCLUSIVE locking, force flags, & acknowledgement never authorize cloud/shared SQLite. |
| AX | Existing `evals/ax` runner plus scenarios are landed scaffolding, not a passed release gate until conformance, behavioral repetitions, claim-fidelity, & state-verification receipts exist. |
| Agent result contract | Every read result separates invocation state, domain outcome, & warranted claim boundary. `claimBoundary` retains `status`, `cleanClaimAllowed`, `safeClaims`, `prohibitedClaims`, & `gaps`; exit 0, empty results, or completed invocation cannot imply clean outcome. |
| Admission vocabulary | Cortex emits `OrientationDecisionV1` plus evidence readiness; it never authorizes tool use or admits context. Reserve context admission/fusion for Membrane & tool-effect policy for rhook. Treat current `lib/admission.mjs` naming as migration debt, not authority. |
| Shared contracts | Orthic owns released manifest/lifecycle/snapshot contracts. Membrane owns released provider-candidate contract. Cortex consumes exact version + digest; optional sibling-source schema fallback is forbidden. Parent gitlink records delivery history only. |
| Store roots | Cortex alone owns graph SQLite/WAL/cache/temp/backup roots. Membrane & Orthic consume published APIs and never open or co-locate mutable Cortex state. |
| Release artifact | Cortex produces signed runtime/add-on artifacts. Orthic adopts exact digest & builds one suite installer; Cortex never builds a competing app installer. |
| Public read surface | Preserve exactly eight versioned read intents: `orient`, `context`, `search`, `impact`, `verify`, `truth`, `proof`, & `status`; advanced reads require discover/expand. MCP has no mutation, preview/commit, or mutating idempotency contract. Internal jobs retain CX-F130 idempotency/recovery. |
| Schedule & thresholds | Dependency edges serialize; disjoint evidence producers may run in parallel. Report 100-file update scaling & old-generation residue, but do not invent a `10×` release threshold or freeze dependency count beyond `sol.md` admission gates. |
| Deferred surfaces | Node SEA, public Rust crate/core rewrite, hosted/team runtime, generic writable MCP, source codemods, & third-party plugin marketplace remain outside current delivery unless their existing product-authority gate is explicitly reopened. |

## 5. Current baseline

Baseline revision observed during plan creation: `c1a08ba` plus pre-existing dirty documentation changes. Re-freeze revision & tree state in W0 before implementation.

| Surface | Current classification | Required closure |
|---|---|---|
| CLI bins: `cortex`, `cortex-watch`, `cortex-mcp`, `cortex-install`, `orthic-cortex` alias | Landed | Golden compatibility matrix plus supported-version receipts |
| Canonical SQLite graph, deterministic Phase 1, generated docs | Landed | Fresh graph from clean canonical source; schema/store/generation receipt |
| Indexed global resolution, equivalent-build singleflight, resident routing | Landed | Frozen concurrency, ghost-edge, cancellation, no-op, & native-host qualification |
| Local Explorer | Landed/unpublished | Preserve Cortex-owned loopback product surface; qualify auth, generation-awareness, & Hub link-out |
| Tray & desktop onboarding | Retired from Cortex | Orthic owns sole tray/app/onboarding; enforce zero shipping Cortex tray/app paths |
| Store migration/repair, performance envelopes, security, fault injection | Previously qualified | Re-run against frozen final revision; reject stale receipts |
| Candidate/SBOM/checksum/provenance release contracts | Landed | Signed native artifacts, clean-host install, publish, rollback, remote verification |
| `cortex mcp serve`, six tools, ≥8 resources, six prompts, output schemas, structured content, annotations, claim boundary | Landed | Fresh-init handshake, schema/deep-equality goldens, AX conformance, & behavioral claim-fidelity receipts |
| AX 12-scenario harness | Landed scaffolding | `pass^1`, `pass^3`, `pass^5`, routing matrix, no-tool accuracy, recovery, overclaim rate |
| Universal disposition/capability lattice/provider qualification | Partial/unproven as one chain | Generated ledger, 100% accounting, doctor reconciliation, per-cell receipt identity |
| Python SCIP/resolver, Terraform, SQL, framework, & hybrid-ranking modules | Landed/unqualified | Production registry→build→store→query→doctor qualification; do not rebuild already-landed adapters |
| Optional semantic lane | Not authority; qualification incomplete | Local-only frozen relevance/latency/RSS/determinism/fallback proof or remain off |
| Direct-page writer, fused scan, codec portfolio, int8 vectors | Research-gated | Separate experiments; default-off unless named CX gate passes |
| Synced/shared canonical-state detection | Absent/unproven | CX-F165 centralized detector, typed init/doctor/status result, explicit local relocation or refusal, & native fixtures |
| Ed25519 update trust, package-manager manifests, candidate/SBOM/checksum/provenance contracts | Landed | Shipped-root rotation/negative proof, signed native artifacts, clean-host install, channel publication receipts |
| 1.0/public distribution | Unproven | Mac + Windows signed artifacts, final qualification, owner-selected npm/MCP Registry/Homebrew/Scoop/WinGet publication receipts |

## 6. Bounded execution program

Aggregate owner-effort ceiling: **1,740 minutes** from frozen W0 start, derived from file-ownership mapping of W0–W8. Maximum-parallel critical-path target is **660 minutes excluding authenticated native-host waits**. Work runs concurrently where paths & evidence producers are disjoint. Total change ceiling remains **120 files / 9,000 changed lines** across production, tests, schemas, fixtures, scripts, & active docs. Exceeding either ceiling requires re-slicing unfinished work; no requirement silently drops.

Input size used for bound: 677-line frozen book, 245-line absorbed draft, roughly 11,000 lines of direct predecessor plans/contracts, 212 Cortex-referencing docs, current source/tests/evals/release manifests. Expected implementation rate is 8–14 verified changed lines/minute; native packaging, benchmarks, & test runtime consume remaining elapsed time.

| Wave | Effort window (not dispatch order) | File / line ceiling | Output |
|---|---:|---:|---|
| W0 — freeze & reconcile | 0–60 | 8 / 500 | Clean baseline tuple, manifest, receipt map, open-cell ledger |
| W1 — coverage & provider truth | 45–210 | 24 / 2,000 | U0–U5 disposition, capability lattice, provider qualification, doctor reconciliation |
| W2 — snapshot, store, concurrency, lifecycle | 90–300 | 22 / 1,800 | One snapshot/publication path, exact no-op/delta semantics, Hub fence proof |
| W3 — exact retrieval, federation, truncation | 180–420 | 24 / 2,000 | Deterministic exact API, cross-repo generation set, device-specific render budgets |
| W4 — language depth, policy, AX | 240–540 | 28 / 2,200 | Precise/fallback providers, policy facts, typed MCP/CLI contract, behavioral AX receipts |
| W5 — security, recovery, compatibility | 420–660 | 20 / 1,500 | Permission, redaction, tamper, export, backup, migration, corrupt-store recovery |
| W6 — gated performance experiments | 480–720 | 20 / 1,400 | Independent accept/reject receipts; no default-on experimental path without pass |
| W7 — native release & comparison | 600–900 | 20 / 1,400 | Mac/Windows signed candidates, clean-host proof, frozen comparison report |
| W8 — closure | 900–960 | 8 / 400 | Final doctor/tests, clean Git proof, nested commit/push, parent pin/push, remote verification |

Named waves are obligation namespaces, not dispatch order. Their minute spans remain effort-allocation evidence only. Setup/reproduce is 20%, implementation 45%, focused tests 20%, evidence/status 15%. File-exclusive dispatch ownership plus frozen interfaces determine parallel schedule in `tasks/plan.md`.

## 7. Work packages

### W0 — freeze & reconcile

1. Record canonical revision, dirty-tree classification, Node/pnpm/platform versions, source/tree digest, ignored-path policy, corpus manifests, current Hub state, & exact released Orthic/Membrane contract versions + digests.
2. Run focused tests, full qualification entrypoint, `cortex doctor --full --json`, graph manifest/schema/languages, MCP surface inventory, AX conformance, & current benchmark harness.
3. Generate one row per CX-R & CX-F with state: `qualified`, `landed_unqualified`, `absent`, `gated_off`, `rejected`, or `not_applicable`; attach producer command plus artifact path.
4. Freeze comparison corpus, holdouts, capacity manifest, competitor manifest, expected behavior, Mac/Windows host identity, & receipt schema.

Exit: no row relies on prose alone; every open row maps to W1–W7.

### W1 — coverage, registry, facts, provenance

Scope: CX-I01, I03, I05, I09, I19; CX-R001–R008; CX-F114–F121, F152.

1. Make canonical scan emit exactly one disposition for every discovered file: U0 exclusion or U1–U5 coverage.
2. Generate capability cells from production registry plus qualification receipts; expose provider/version/config/binary digests, byte/file coverage, parse success, error nodes, fallback reason, receipt time, & receipt digest.
3. Consolidate provider registry, fact schema, provenance schema, qualification runner, & doctor reconciliation.
4. Fail closed on registry/receipt mismatch, missing capability evidence, duplicate authority, or unexplained file/byte remainder.
5. Preserve early shallow facts while deeper providers run; never promote shallow inventory into completeness proof.

Exit: `U1..U5 + U0 = 100%`, unexplained files/bytes `0`, ledger/runtime mismatch `0`, unsupported dimensions typed.

### W2 — snapshot, store, concurrency, Hub lifecycle

Scope: CX-I02, I07, I09–I16; CX-R009–R016; CX-F118–F123, F133–F135, F145–F148, F162–F163, F165.

1. Complete `BuildSnapshotV1` identity: root, source/tree, file manifest, ignore/config, registry/providers, schema, resolver universe, index plan, & generation. Grammar identity stays inside provider digest, dirty state inside source/tree + file-manifest digest, & edge-bridge identity inside resolver/index-plan digest; create no duplicate shadow manifest keys. Hub protocol, lease, endpoint, instance, & fence never enter build identity or force graph rebuild.
2. Before opening mutable state, canonicalize SQLite/WAL/temp/spill/cache/blob/backup/sidecar paths; classify local, synced/shared, or unknown through one provider registry plus platform mount/share probes. Relocate known-unsafe state only to explicit owner-only local storage bound to repository identity, otherwise refuse typed; never treat journal/locking fallback or acknowledgement as support.
3. Enforce one scan/hash, SQLite authority, publication compare-and-swap, resource-handle lifecycle, build singleflight, waiter isolation, cancellation, & resident routing path.
4. Prove global resolution over complete staged universe, ghost-edge equivalence, source/provider churn restart, real no-op reuse, one-file delta, & interrupted-publication preservation.
5. Implement Cortex child side of pinned `orthic.lifecycle.v1`: inherited authenticated channel, hello/ready endpoint registration, installation/instance/artifact/fence identity, drain/stop/update handoff, parent-loss handling, & stale-fence rejection. Orthic alone owns lease issuance, supervision, restart/backoff, compatibility evaluation, & child-tree cleanup.
6. Keep pool modes explicit (`null`, bounded, timeout/recycle/preflight equivalents); do not infer pool support from libraries without production use.

Exit: one queryable generation, one current graph owner, no mixed rows, no duplicate builds, lifecycle changes do not rebuild graph, lost Hub ownership drains/exits, no orphan process, & no canonical mutable state on known synced/shared storage.

### W3 — exact retrieval, federation, truncation

Scope: CX-I08, I14, I17–I18, I22; CX-R017–R020; CX-F124, F126–F132, F136–F144, F149–F154.

1. Consolidate one deterministic exact authority: terms/FTS/BM25, symbol bypass, regex/Boolean constraints, stable tie-breaks, scope filters, pagination, & provenance.
2. Publish explicit ranking components; relevance never changes truth, authority, capability, or citation identity.
3. Build cross-repo query over one frozen generation per repository. Reject mixed/stale generation sets or label them unusable. Emit candidates conforming to exact released `membrane.context-candidate-set.v1` schema + digest; Membrane alone performs cross-source admission/fusion. Remove Cortex's divergent local candidate schema as authority after golden migration.
4. Separate canonical retrieval from presentation truncation. Canonical result set carries stable IDs, total counts, continuation, omitted ranges/reasons, token/byte accounting, generation, & digest.
5. Apply client-specific render budgets at final surface: CLI terminal, MCP host, Explorer/desktop, & same-machine export. Device B can resume Device A through opaque continuation plus generation/digest; never transmit hidden state or repository content across devices.
6. Invalid/expired/mismatched continuation returns typed recovery; it never silently restarts or splices generations.

Exit: exact p95 `<5 ms`; hard budgets never exceeded; multi-device continuation is deterministic, provenance-complete, & zero-egress.

### W4 — language depth, hybrid policy, AX

Scope: CX-R021–R028; CX-F05, F07, F09, F37–F39, F44, F71, F97, F115, F155–F157.

1. Qualify compiler/LSP/SCIP adapters, native syntax, custom grammars, generic structural fallback, lexical/document/config facts, frameworks, schemas/IaC, dataflow/security, tests, & refactor capability cells. Start with already-landed Python SCIP ingestion/resolver fixtures; add no second Python path unless qualification proves a named gap.
2. Keep fallback ladder explicit per file & per query; provider failure cannot become empty success.
3. Add graph centrality, change/history, policy, & optional semantic signals only as visible ranking components under hard budget plus exact fallback.
4. Preserve exactly eight read intents: `orient [task]`, `context <task>`, `search <query>`, `impact [diff/node/path]`, `verify [diff/packet]`, `truth [scope]`, `proof <task-or-packet>`, & `status`; advanced reads require versioned discover/expand.
5. Every operation has `additionalProperties: false` typed input, enum/server validation, `outputSchema`, schema-valid `structuredContent`, equivalent redacted text rendering, read-only/idempotent/open-world annotations, stable errors, remediation, & opaque authority-bearing handles/cursors.
6. Every result separates `invocation = accepted|working|completed|failed|cancelled`, `outcome = pass|policy_fail|partial|incomplete|not_applicable|unproven`, & exact claim boundary fields: `status`, `cleanClaimAllowed`, `safeClaims`, `prohibitedClaims`, `gaps`.
7. Qualify fresh `init` → `cortex mcp serve --root` handshake, all registered resources/prompts, clean close, effects resource, output-schema validation, redaction parity, & compatibility goldens.
8. Run AX conformance plus 12 frozen scenarios and required ambiguous/stale/no-result/conflict/injection/large-corpus cases. Report `pass^1/^3/^5`, routing confusion, no-tool accuracy, first-attempt argument validity, recovery, overclaim, & durable-state verification.
9. Run an outer same-machine, network-disabled multi-agent/multi-model AX matrix and report variance/disagreement. Do not add reviewer orchestration, remote providers, or model authority to Cortex runtime.
10. Keep hosted/team/RBAC runtime outside Cortex. Keep semantic compression rejected. Optional local semantic candidates require frozen admission proof.

Exit: no unsupported market claim, no safety-critical clean false-positive, no freeform model verdict, no semantic authority.

### W5 — security, export, retention, recovery, compatibility

Scope: CX-R029–R032; CX-F125, F145–F151, F165 plus applicable security/recovery rows.

1. Enforce canonical-root containment, symlink/junction escape defense, permission model, secret redaction, injection isolation, plugin boundary, tamper evidence, & local-only payload policy.
2. Keep same-machine export/import versioned, provenance-bound, stale-detecting, & zero-egress. Outbound repository-derived bytes remain zero.
3. Implement retention, generation cleanup, backup, restore, repair, corrupt-store quarantine/rebuild, migration forward-read/backout or explicit refusal, & prior-generation survival.
4. Complete CLI/MCP/daemon/SDK/query/store/export/UI compatibility matrix with old/new supported-window fixtures.
5. Gate optional envelope encryption separately; crypto-shred, key rotation, crash recovery, performance, migration, & platform key handling must pass before default-on.
6. Freeze Mac + Windows store-location fixtures for named provider folders, UNC/mapped SMB, NFS/SMB/CIFS/WebDAV, local paths, marker collisions, relocated roots, symlink/junction targets, probe failure, relocation, refusal, restart, & zero partial publication; synced/shared source roots with local state must report watcher/polling degradation.

Exit: hostile-repository suite green; corruption produces typed recovery; supported compatibility cells 100%.

### W6 — optional performance experiments

Scope: CX-A08, A12, A16, A23, A25; CX-F75, F77, F156, F158–F161.

Each experiment has isolated branchless prototype path, frozen corpus, same public results, cold/warm/update/query/RSS/disk measures, 100-file update scaling, old-generation residue, cancellation/corruption fixtures, Mac/Windows results, maintainability note, dependency review, & explicit `adopt`, `remain_off`, or `reject` receipt. The 100-file and residue cells are diagnostic unless `sol.md` supplies a pass threshold.

Order:

1. Direct-page SQLite builder, including bounded streaming append that flushes heavy properties mid-pipeline before remaining tables/indexes finalize.
2. Fused decompression/Aho-Corasick scan, including exact multi-pattern bitmask equivalence; representation stays implementation-selected rather than mandating BigInt.
3. LZ4 fast codec plus LZ4 HC/Zstd size codecs.
4. Compact mmap read-only export/zero-copy spans.
5. Optional local semantic lane against full-precision baseline.
6. CBM-style int8/IDF token-vector representation only after that baseline exists; bind IDF calculation/update, dimensions, serialization, endianness, model/provider, & fallback identity.
7. Worker parallelism only after profile proves bottleneck plus RSS headroom.

Default disposition is `remain_off`. Material win threshold follows named `sol.md` row; otherwise complexity loses.

### W7 — native release & comparative proof

Scope: CX-R033–R036; CX-I12; CX-F151/F165 plus all completion gates.

1. Freeze final source, dependency lock, provider binaries/config, schema, corpus, capacity, expected behavior, competitors, warmup/repetitions, hardware/OS/runtime, & statistical method.
2. Run final tests, qualification, doctor, AX, security, recovery, compatibility, soak/fault, performance, store-location, & comparative suites on Mac & Windows native hosts; CX-F151 integrates applicable F153–F165 runtime interactions while CX-F152/F162/F163 retain separate receipts.
3. Qualify pinned Ed25519 update trust root, key rotation, absent/unknown/corrupt signer rejection, signed-manifest round trip, native-receipt presence/absence behavior, & trust-root preservation during clean-host rehearsal.
4. Produce signed/checksummed Cortex add-on/runtime, SBOM, provenance, update manifest, & exact digest for Orthic adoption. Orthic builds suite installer once; run clean-host install/upgrade/uninstall/rollback through that byte-identical installer, then produce owner-selected npm OIDC, MCP Registry, Homebrew, Scoop, & WinGet publication receipts where applicable.
5. Compare coverage, exactness, freshness, build/update/query latency, CPU/RSS/disk, privacy, explainability, recovery, lifecycle, portability, compatibility, & AX. Claim “best” only if both correctness/performance & AX pass, every eligible axis is noninferior, plus one material axis wins.

Exit: all thresholds in `sol.md` pass or product language remains target-only; Orthic contract released → Cortex conformance → exact add-on digest adopted → suite installer installed on Mac/Windows → quit/off/update yields zero orphan Cortex children.

### W8 — closure

1. Re-run from clean canonical source: focused tests, full tests, qualification, `cortex doctor --full --json`, graph manifest/schema/languages, AX, release verification, & required native receipts.
2. Verify every CX-R row plus applicable CX-F row has current content-addressed evidence; all `GATE` rows have explicit decision; all `REJECT` rows have boundary tests where needed.
3. Verify graph fresh, file coverage complete, understanding state honest, service state honest, generated docs current, stale/invalid lifecycle references zero.
4. Commit nested Cortex repository, push it, pin exact nested commit in parent, commit parent retirement moves, push parent, & verify both remotes resolve expected objects. Parent pin is workspace history only; runtime compatibility derives from released contract versions/ranges & artifact digests.

Exit: `produced → verified → committed → parent-pinned → pushed` is proven. “Done” is forbidden before final remote verification.

## 8. Requirement coverage

| Contract range | Owning package |
|---|---|
| CX-R001–R008 | W1 |
| CX-R009–R016 | W2 |
| CX-R017–R020 | W3 |
| CX-R021–R028 | W4 |
| CX-R029–R032 | W5 |
| CX-R033–R036 | W7 |
| CX-F01–F113 | W0 classifies; W1–W7 close by owning behavior |
| CX-F114–F121 | W1 |
| CX-F122–F123, F133–F135, F145–F148, F162–F163, F165 | W2/W5 |
| CX-F124, F126–F144, F149–F154 | W3/W5 |
| CX-F125 | W5 |
| CX-F155–F157, F164 | W4 |
| CX-F158–F161 | W6 |
| CX-F151–F152 | W7/W0 |

## 9. Performance & release gates

All are simultaneous:

- feature/evidence hit rate `≥86.7%`; MRR `≥0.800`;
- 550-file cold build `<5 s`, peak RSS `<300 MB`;
- no-op `<1 s`; one-file delta `<100 ms`;
- resident exact query p95 `<5 ms`;
- 5,000-file cold build `<60 s`, peak RSS `<1 GB`;
- deterministic results under concurrency, cancellation, restart, & device-specific truncation;
- Hub disabled means zero Cortex background process;
- one atomic `BuildSnapshotV1`; zero mixed-generation facts;
- `BuildSnapshotV1` is invariant across Hub lease/instance/fence changes;
- pinned `orthic.product-manifest.v2`, `orthic.lifecycle.v1`, & `orthic.snapshot.v2` conformance passes without sibling-source fallback;
- Cortex candidate output validates against pinned Membrane protocol schema/golden;
- zero `_membrane`, hardcoded Crypt paths outside configuration, shipping Cortex tray/app paths, or second watcher owner;
- Mac & Windows native receipts from same frozen source/provider/schema contract;
- 100% public compatibility cells for supported window;
- no critical/high unauthorized effect; no repository/model egress;
- fresh-init MCP handshake exposes exact read-only tool/resource/prompt contracts with schema-valid structured results & claim boundaries;
- outer network-disabled agent/model AX matrix reports repeated-run variance without becoming Cortex authority;
- 100-file update scaling & old-generation residue are reported; only `sol.md` thresholds can make them blocking;
- all 33 reviewed sources retain source-span + snapshot-digest provenance in `sol.md` evidence closure.

## 10. Preserved trail & retired surface

Preserve in active locations:

- vector bakeoff run plus linked receipts;
- baselines, benchmarks, evidence, audit outputs, Covenant records, & incident history;
- normative cross-product seam contract plus historical seam receipts;
- current rules, runbooks, generated docs, release decisions, & compatibility templates;
- `sol.md` plus this file.

Archive only superseded implementation/planning prose. Archive root contains manifest with original path, classification, retirement date, canonical successor, & explicit preserved-evidence list. No evidence file is deleted.

## 11. Change-control rule

Change `sol.md` only for product/performance/research authority decisions. Change this file for sequencing, package ownership, bounds, or gate mechanics. Update code/status from executable evidence, never optimistic prose. New plan files are prohibited; add sections here or attach non-authoritative receipts under evidence/run/review paths.
