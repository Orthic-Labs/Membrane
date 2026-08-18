# Membrane best-of-market execution plan

> **Superseded by:** [`../MEMBRANE-IMPLEMENTATION-GUIDE.md`](../MEMBRANE-IMPLEMENTATION-GUIDE.md) as implementation authority (2026-08-18). Historical content retained.

Date: 2026-08-12

Status: derived execution plan; implementation stopped; no completion claim

Authority: [`../../sol.md`](../../sol.md). This plan cannot change its invariants, requirements, dispositions, or gates.

Repository: canonical `/Volumes/D/claude/membrane` only; `membrane2` is forbidden.

## Outcome

Deliver every `MB-R001..MB-R036` requirement through consumer-compatible packages that are independently releasable only when their partial-state contract is green. Final system provides deterministic, authority-first, local context; durable typed memory; explainable hybrid recall; reversible consolidation; temporal truth; governed artifact references; typed context editing; robust backup/recovery; real whole-task evaluation; & absolute user process control through Hub.

## Requirement closure map

| Book requirements | Accountable acceptance owner | Predecessors | Partial-state invariant | Not green until |
|---|---:|---|---|---|
| MB-R001–R004 | Membrane Hub owner / P1 | 0 | Existing installed ownership remains until shared protocol & joint child proof pass | Lifecycle + portability cells; joint Hub/Cortex native receipt |
| MB-R005–R008 | Crypt memory owner / P5 | 0 → 2 | Existing memory schema/API remains authoritative; additions are versioned & disabled until migrated | Equivalence + recovery cells; P2/P5 migration receipt |
| MB-R009–R012 | Membrane policy owner / P5 | 0 → 2 | Current ACL path remains fail-closed; no new cache/derived projection admitted early | Scope + revocation + publication cells; P2/P5 race receipt |
| MB-R013–R016 | Membrane planner owner / P6 | 0 → 2 → 3 | Current packet order stays authoritative; new lanes stay additive/disabled | Equivalence + scope + task-quality cells; P6 receipt |
| MB-R017–R020 | Membrane planner owner / P6 | 0 → 2 → 3 | Existing packet/edit contract stays readable; optimization cannot alter outcomes | Equivalence + token + warm-context cells; P6 receipt |
| MB-R021–R024 | Crypt memory owner / P5 | 0 → 2 | Background output remains invisible/non-authoritative until full identity validates | Scope + recovery + equivalence cells; P5 receipt |
| MB-R025–R028 | Membrane artifact owner / P8 | 0 → 2 → 4 → 7 | Stored formats remain readable; ArtifactRef stays disabled until policy/recovery works | Revocation + storage + recovery cells; P4/P7/P8 receipts |
| MB-R029–R032 | Crypt storage owner / P9 | 0 → 3 → 7 | Current DB identity remains authoritative; maintenance never changes sole copy | Resources + storage + recovery + observability cells; P7/P9 receipts |
| MB-R033–R036 | Membrane release owner / P10 | 0 then all owners | No performance/market/delivery claim from partial packages | Evidence + comparative + compatibility + native delivery cells; final release receipt |

Named owner produces with package's checked-in command into `artifacts/receipts/MB-R###-R###.json`; independent Oracle verifies source/book/protocol/artifact digests & acceptance cells before green. Only named owner may propose green; validator + verifier establish it. A package may commit before downstream completion only when its partial-state invariant, compatibility/migration gate, rollback, & current acceptance cells pass; otherwise it is an integration checkpoint, not independently shippable.

## Final-absorption package map

| Packages | Mandatory book closure rows |
|---|---|
| 0, 2 | MB-F01–F03, F05–F06, F24–F25 |
| 3, 6 | MB-F04, F07–F12, F20–F23 |
| 5 | MB-F13–F17, F26 |
| 7 | MB-F06–F07, F18 |
| 9–10 | MB-F18–F26 |

No package may mark a broad `MB-A`, atomic aspect, or `MB-R` row green while any mapped `MB-F` receipt remains absent.

## Frozen starting evidence

- Canonical doctor reported `broken` on 2026-08-12: stale Cortex/provider identity, Merkle mismatches, missing understanding/verdict artifacts, & 302 missing references.
- Four Crypt database/WAL files contain an uncommitted stopped-task overlay. It is not accepted baseline or release evidence.
- Crypt schema, scoped memory, temporal facts, lifecycle events, feedback, supersession, expiry, quarantine/restore, hybrid vector/lexical retrieval, vector index, query-embedding LRU, worker admission, resident federation worker, streaming `run_capped`, identifier prefixes, & document hash/parser-version skipping exist in source but lack current clean installed qualification.
- Doc Spine production recall scans projections; FTS exists in benchmark/test surfaces, not production query path. Doc packet admission remains shadow-only.
- LoCoMo, LongMemEval, BEAM, & whole-task commit-reveal harnesses are source-ready only; no accepted real result exists.
- Full-provider warm profiling is absent; a fixture that merges empty provider sets is not performance evidence.
- Backup/restore/export/wipe, artifact registry, multimodal extraction, one unified policy plane, & final Hub lifecycle are incomplete.
- Historical DB measurements remain useful leads: Mac Crypt held material free pages; duplicate catalogs/orphan sidecars needed ownership proof. No live data mutation is authorized by this plan.

## Delivery law

1. Freeze packets, decisions, & real baselines before optimization.
2. Land one package at a time with focused tests, measurement, rollback, compatibility proof, commit, & remote receipt only when its partial-state invariant permits release.
3. Same canonical inputs must preserve candidates/order, grants, authority, freshness, omissions, deadlines, cancellation, token/byte caps, exit/output behavior, & receipts unless `sol.md` requires a new behavior.
4. Never relax fixtures, thresholds, timeouts, expected omissions, or sample counts to pass.
5. Source-ready, unit-tested, or schema-present does not mean installed, qualified, measured, or complete.
6. Hot-path concurrency/caches/workers remain unchanged until full-provider profiling identifies a bottleneck & equivalent bounded variant wins.

## Work package 0 — truth freeze & plan sync

Requirements: MB-R033–R036.

- Inventory dirty overlay without modifying it; reproduce clean committed baseline separately.
- Generate a machine-readable requirement/competitor/status map from `sol.md`; CI rejects unmapped `MB-A`, `MB-R`, or `MB-I` IDs.
- Freeze packet/order/omission/freshness/grant/cancellation/timeout/context-edit/output receipts.
- Freeze full-provider warm benchmark, DB identity, lifecycle process census, retrieval corpus, benchmark dataset identity, & whole-task holdout protocol.
- Freeze `benchmark-protocol.v1.json`, `membrane-competitors.v1.json`, atomic aspect closure, public-surface inventory, supported-version window, & exact canonical `sol.md`/plan digests. Every later receipt names those digests.
- Freeze package capacity manifest: owned file paths, prerequisite artifacts, measured baseline paths/lines, projected changed-line ceiling, native-host effort, acceptance cells, & aggregate remaining work.

Exit: failing current behavior is recorded honestly; later packages cannot edit expected outputs or gates.

Package 0 contributes immutable Membrane Hub source/ref/tree/commit/artifact/protocol/book/receipt inputs to parent workspace integration owner. That owner is sole serialized writer of root `artifacts/releases/context-stack-release.v1.json`; manifest version + prior digest use compare-and-swap, & any divergent/stale child tuple is rejected. Manifest also seals Cortex tuple, supported lifecycle ranges, native install source, joint test commands, & receipt digests. Exact sequence: pre-install commits exist → native artifacts build from those commits → parent owner seals one tuple → joint installed tests run against sealed artifacts → nested refs push unchanged → parent pins those exact commits → remote refs/pins verify. Any child/source/artifact/receipt mismatch or post-test change invalidates joint proof.

## Work package 1 — Hub-only lifecycle closure

Requirements: MB-R001–R004.

- Make Hub sole OS-started/user-controlled process. “Start at login” registers Hub only.
- Hub directly owns Membrane/Crypt resident + Cortex watcher/service in one process group on macOS & Job Object on Windows.
- Publish shared `hub-child-lifecycle.v1`: protocol version, executable/artifact hash, lease, inherited liveness handle, readiness, drain, exit taxonomy, restart/backoff, update, & process-tree identity.
- Atomically acquire one host/install-scoped Hub owner lease with monotonic fencing token before child start. Children reject stale/lower/foreign fences; lease-store ambiguity fails closed; update handoff advances fence without overlap.
- Issue short-lived Hub lease + inherited liveness pipe; children exit on pipe closure, lease expiry, parent death, update, or quit.
- Reject an existing listener lacking current Hub lease as `rogue_process`; never adopt silently.
- Add bounded restart/backoff, crash-loop state, health/readiness, update-safe drain, full child-tree kill/wait.
- Remove standalone persistence/registration/public supervisor paths; foreground CLI/MCP one-shots remain nonpersistent.

Exit: Hub protocol commit/release & compatibility proof exists before Cortex removes any prior path; installed joint Mac + Windows tests name matching Hub/Cortex protocol + artifact hashes & cover concurrent Hub start, stale owner, lease-store loss/corruption, update handoff, login launch, user off/on, tray quit, parent kill, child crash, rogue port, uninstall, crash loop, one fenced owner, & zero surviving owned descendants.

## Work package 2 — one policy & protocol plane

Requirements: MB-R005–R020.

- Make one canonical Rust planner own eligibility, authority/freshness ordering, fusion, admission, global budget, rendering, & typed omissions.
- Python/providers return typed candidates only; remove duplicate policy decisions or mark unreachable scaffolds.
- Generate language bindings/schemas from one protocol source; cross-language goldens must be byte-equivalent.
- Every candidate carries source, scope, ACL, generation, authority, freshness, sensitivity, cost, resolver, provider/version, & evidence digest.
- Provider contracts including Cortex stop at typed facts/candidates + relevance components. Only planner resolves current grant/policy & owns packet eligibility, authorization, authority/freshness ordering, cross-source fusion, budgets, omissions, & rendering.
- Bind cache/derived identity to source generation, policy/grant/ACL/scope/sensitivity versions; ordered invalidation fail-closes admission during revocation.
- Bind candidate read through final publication to one policy epoch; fence/revalidate immediately before any output bytes, retry once or emit typed `policy_changed` abstention on change.

Exit: all provider omissions reach final receipt; no policy decision changes with adapter/runtime path; revoke-during-packet across CLI/MCP/HTTP/SDK/Hub emits zero obsolete-epoch bytes.

## Work package 3 — hot-path isolation & measured concurrency

Requirements: MB-R013–R020, MB-R029–R032.

- Trace representative warm requests across every provider, gateway scheduling, subprocess/RPC wait, CPU, serialization, merge, DB work, RSS, cancellation, & deadline.
- Prompt path may read immutable ready snapshots, query local indexes, run bounded resident RPC, fuse/admit/render, & enqueue content-free telemetry.
- Move recursive walks, document parsing, corpus embedding, schema migration, backup, checkpoint, compaction, LLM extraction, remote network, & blocking telemetry off prompt path.
- Reuse existing resident worker, LRU, worker admission, & `run_capped`; add no second cache/pool.
- Change concurrency only when sequential/current control loses on p95 without timeout, RSS, determinism, or result-variance regression.

Exit: syscall/process trace shows zero per-prompt child creation, corpus scan/mutation, maintenance, or unapproved network; full-provider warm receipt meets gate.

## Work package 4 — indexed document & artifact retrieval

Requirements: MB-R025–R028. Absorbs MB-A03, MB-A07, MB-A11, MB-A14–A16.

- Keep existing content-hash + parser-version no-op sync; prove deletion, parser upgrade, interruption, duplicate path, & source churn.
- Replace production O(N) projection recall with measured FTS5/BM25 or chosen compact exact index while preserving path, anchor, Unicode, short-substring, source-hash, & deterministic fallback behavior.
- Wire document candidates through final admission; retire shadow-only ambiguity.
- Add `ArtifactRefV1`: `art:<sha256>`, MIME, bytes, origin/source/derived hashes, scope, stored ACL evidence, current authoritative policy resolver + version, timestamps, extractor/version, derived refs, integrity, availability, sensitivity.
- Keep original bytes at source or Hub-owned object store; memory stores governed metadata/projections/citations/resolver handles.
- Re-resolve current policy on read/citation/derived lookup/export/restore/share; deny stale/unavailable policy & cascade revocation through lexical/vector/graph/working/derived/artifact/export/restore projections.

Exit: production trace proves indexed path; legacy corpus results remain exact; 12k-document corpus avoids O(N) scan; unsupported/missing/denied/corrupt artifacts return typed states.

## Work package 5 — complete memory model

Requirements: MB-R005–R012, MB-R021–R024. Absorbs MB-A01–A04, MB-A08–A09, MB-A14.

- Formalize Working, Episodic, Semantic, Procedural, Entity Summary, Evolving Belief, & Artifact Reference records with schema migrations/backouts.
- Separate retain, recall, reflect, consolidate, correct, supersede, expire, forget, quarantine, restore, export, migrate, & audit contracts.
- Unify temporal validity/supersession & bounded graph/entity traversal across memory families.
- Add pinned, scoped, size-bounded working blocks; private/team/global visibility stays ACL-governed.
- Reflect/consolidation creates reversible derived records with complete evidence/model/prompt/corpus identity; never silently promotes truth.

Exit: provenance round-trip, contradiction, expiry, supersession, ACL, derived-belief, quarantine/restore, migration/backout, & no-unreceipted-promotion suites pass.

## Work package 6 — explainable retrieval & context editing

Requirements: MB-R013–R020. Absorbs MB-A01–A06, MB-A09, MB-A12–A13.

- Eligibility first: grant, ACL, scope, quarantine, temporal validity, supersession, generation, source availability.
- Run exact anchor, lexical/BM25, vector, entity/graph, temporal, & active-overlay lanes.
- Fuse deterministically with visible lane ranks plus authority, freshness, importance, recency, frequency, outcome/feedback, & diversity; stable tie by canonical ID.
- Keep learned reranker shadow-only until holdout proof; base order always receipt-visible.
- Add JIT memory/artifact references & context edit contract: durable reference before clearing, placeholder, source set, byte/token delta, recovery pointer, typed failure.
- Compression may not remove protected facts/citations/authority or change task outcome beyond equivalence gate.

Exit: held-out task quality improves or stays within noninferiority gate while reducing tokens; zero scope/ACL leaks; deterministic fallback/abstention works without vectors/graph/provider.

## Work package 7 — storage identity, durability, & recovery

Requirements: MB-R025–R032.

- One resolver owns Crypt/catalog/outbox paths across Rust, Python, installer, Hub, health, & operations; absolute paths only.
- Persist installation/store identity, schema, effective journal mode, main/WAL sizes, owner/lease, & duplicate candidates in runtime receipt.
- Writer/maintenance owner alone checkpoints; readers never migrate/checkpoint. Report WAL frames, busy reader, duration, & starvation.
- Add read-only inventory that never creates DBs; classify sidecars/duplicates by liveness & provenance.
- Add live consistent backup, restore drill, deterministic vault export/import, wipe policy, integrity checks, migration preflight/backout, & recoverable quarantine.
- Compact durable Crypt only by backup → new-file compaction → logical equivalence → atomic adopt → health → rollback proof, per host.

Exit: crash-at-every-boundary suite, active-backup test, clean-machine restore, old-schema backout, key-set/count/event continuity, recall equivalence, & no-data-loss receipts pass.

## Work package 8 — multimodal context

Requirements: MB-R025–R028. Absorbs MB-A07, MB-A19.

- Stage local extractors: PDF/text first; image metadata/OCR second; audio transcript third; video metadata/keyframe references last.
- Derived text/summary remains hash-addressed, cited, scoped, ACL-filtered, versioned, rebuildable, & independently expirable.
- Original binary never enters prompt without explicit grant & budget.

Exit: MIME/hash/ACL/sensitivity/size/deadline/missing-source/extractor-unavailable matrix passes; no binary or derived text crosses scope.

## Work package 9 — real evaluation & operability

Requirements: MB-R029–R036. Absorbs MB-A20–A23.

- Run real LoCoMo, LongMemEval, BEAM, & commit-reveal whole-task corpora against exact release on native Mac + Windows.
- Compare eligible local/self-hosted competitors through same datasets, execution adapter, model, hardware class, budgets, scoring, & raw receipt schema.
- Freeze named eligibility/config/exclusion manifest from `sol.md`; hosted-only, archived, missing, or incompatible entries remain explicit & cannot be cherry-picked away.
- Measure task success, unauthorized/stale context, missed authority, precision/recall/MRR/nDCG, contradiction/temporal accuracy, tokens, cached tokens, latency, CPU/RSS, DB/index growth, durability, restoration, & cost.
- Publish support matrix only from current installed receipts; disable unqualified integration claims.
- Produce content-free stage metrics, typed doctor, diagnostic bundle, backup age, lifecycle, resource, omission, & benchmark dashboards.
- Validate public CLI/MCP/HTTP/SDK/Hub UI/protocol/schema/backup/export compatibility across supported version window.

Exit: every `MB-A` row has `ALREADY+proof`, `ADOPT+receipt`, `GATE+decision`, or enforced `REJECT`; every MB-R requirement is green.

## Work package 10 — native delivery

Requirements: MB-R033–R036.

- Run full source, protocol, lifecycle, migration, fault, security, equivalence, benchmark, & native-host gates on clean tree.
- Build/sign each native binary on its host only; publish exact patch through existing RightKit/release path.
- Commit/push Membrane; pin/push parent gitlink; verify remote SHAs; install through Hub; rerun installed lifecycle/doctor/benchmark/restore smoke.
- Preserve integration order in final receipts: Hub protocol commit/release → Cortex compatible child adoption → installed joint native proof → nested pushes → parent pins.

Exit: exact installed source/release generation/host/artifact hashes match; no dirty overlay contributes to claim.

## Frozen acceptance matrix

| Axis | Gate |
|---|---|
| Equivalence | Exact candidate/order/packet/omission/receipt fixtures `100%` |
| Scope/security | ACL/scope leak cases `0`; denied source I/O `0`; unapproved egress `0` |
| Revocation | stale cache/vector/graph/working/derived/artifact/citation/export/restored admission after policy change `0` |
| Publication | obsolete-policy-epoch output bytes after revoke-during-packet `0`; deterministic retry/abstention |
| Warm context | full-provider p50 `<=75 ms`, p95 `<=125 ms`, p99 `<=250 ms` |
| Ready no-op | p95 `<=25 ms` |
| Cold/degraded | bounded result or typed failure `<=1.5 s` |
| Local recall | p95 `<=50 ms` current corpus; `<=100 ms` at 100k governed memories |
| Task quality | holdout 95% CI lower bound no worse than baseline by `1 pp`; no authority/safety regression |
| Token economy | `>=20%` reduction versus full-context baseline at noninferior task quality |
| Resources | idle CPU `<1%` per child/10 min; wakeups `<6/min` combined; RSS regression `<=10%` |
| Storage | index/projection growth `<=25%` above authority payload unless measured retrieval win justifies recorded variance |
| Recovery | backup/restore/migration/compaction preserve authority key set, events, lifecycle, recall, & provenance |
| Lifecycle | exactly one fenced Hub owner; Hub-off owned-process count `0`; crash/orphan/split-brain tests leave `0` children; only Hub has OS startup registration |
| Evaluation | real corpus/release/model/hardware/host/raw receipts; synthetic results never accepted |
| Compatibility | CLI/MCP/HTTP/SDK/Hub UI/protocol/schema/artifact/export supported-version matrix `100%` |
| Comparative | frozen eligible manifest; all-axis noninferiority + one material dominance or target wording only |
| Evidence | shared statistical protocol complete; no missing/failed/censored samples; receipt book/tree/artifact digests exact |
| Delivery | clean full gates, signed native artifacts when changed, nested commit/push, parent pin/push, remote SHAs, installed proof |

## Excluded shortcuts

- No prompt-time LLM extraction, summarization, graph construction, migration, backup, or corpus scanning.
- No mandatory hosted memory/vector/graph service or unapproved egress.
- No automatic truth promotion, hidden prompt/procedure rewrite, destructive forgetting, or opaque learned global score.
- No unbounded graph traversal, full-history resend, blind blob injection, or multi-store baseline.
- No second LRU, worker pool, manifest, policy plane, lifecycle supervisor, or database authority where one exists.
- No independent launchd, login item, Task Scheduler entry, daemon persistence, port-owner adoption, or child self-restart.
- No provider/concurrency tuning from empty fixtures or GIL speculation.

## Measured capacity control

Fixed productivity arithmetic is forbidden. Package 0 creates `docs/plans/capacity/membrane-best-market.v1.json` under Membrane integration owner; product authority approves initial ceiling & any revision before implementation. Schema records package, owned files, baseline LOC, projected changed LOC, focused-test active minutes, native-host active minutes, external wait separately, prerequisites, acceptance cells, uncertainty range, contingency `<=10%`, & unmapped-work count `0`. Estimates derive from path-level baseline + one measured representative change per work type, never a global productivity rate; before every package starts, validator rejects missing paths, double-counting, unmapped requirements/work, ceiling increase without new authority revision, or package high estimate exceeding aggregate remaining allocation. Product authority must split/resequence/revise before any over-cap package work begins.

| Package | Prerequisite artifacts | Acceptance owner/cells | Capacity fields frozen at P0 |
|---:|---|---|---|
| 1–2 | baseline, shared lifecycle protocol, aspect/surface/policy inventory | lifecycle, equivalence, scope, compatibility | owned paths, baseline lines, projected delta, native-host effort |
| 3–4 | full-provider trace, provider protocol, document/artifact identity | warm/cold, revocation, retrieval, storage | same fields + profiling/index migration work |
| 5–6 | memory migrations, policy invalidation, holdout corpus | memory, scope, equivalence, task/token quality | same fields + migration/backout/model-runtime cost |
| 7–8 | DB identity, backup/restore design, extractor manifests | durability, recovery, multimodal security | same fields + fault/native-host work |
| 9–10 | all acceptance-owner receipts, comparator adapters, release manifests | evidence, comparative, compatibility, delivery | exact remaining paths, native release/install effort, Cortex joint proof, parent pin work |

After every package, receipt records actual changed files/lines, active engineering time, native-host wait separately, completed acceptance cells, & aggregate remaining capacity. No “all requirements green” or final-package start is allowed when mapped remaining work exceeds frozen remaining ceiling; product authority must split/resequence scope without weakening this book.
