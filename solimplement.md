# Membrane implementation source of truth

**Authority date:** 2026-08-14
**Source baseline:** `459ff530baae12d9ab3973d61f014da069e9d6b7`
**Capability baseline:** [`sol.md`](sol.md), 39 local repositories, mechanics M01–M62
**Plan state:** READY; implementation has not started under this plan

## 1. Authority

This file is Membrane's only implementation plan, sequence, status register, & completion checklist. It supersedes older MemRight, RightContext, context-stack, three-book, absorption, productization, recovery, database-hygiene, & release plans.

Authority order:

1. [`sol.md`](sol.md) owns compared capabilities, M01–M62 requirements, dispositions, & exclusions.
2. Workspace [`SEAM-CONTRACT.md`](../docs/plans/orthic/SEAM-CONTRACT.md) owns every Cortex ↔ Membrane ↔ Orthic boundary.
3. Current checked-in source owns implemented behavior.
4. Rust protocol types plus generated schemas own Membrane wire contracts; [`docs/protocol/source-of-truth.md`](docs/protocol/source-of-truth.md) defines generation rules.
5. Generated product truth & support matrix own exposed operations and qualified client/platform claims.
6. This file owns Membrane implementation order, child-side conformance, gates, package status, & release exit.
7. Receipts, runs, reviews, baselines, & archived plans preserve evidence or rationale; none can mark current work complete.

When sources conflict, fail status closed, record conflict here, & repair source/generated truth before feature work. `ALREADY` in `sol.md` means source-evidenced, not installed, qualified, or released.

[`deepseek.md`](deepseek.md) plus Muse, Kimi, & independent seam reviews are inputs, not competing plans. Section 3.1 records accepted, corrected, & rejected findings so no later implementation silently revives incompatible ownership or stale status.

## 2. Product outcome & boundary

Ship one local-first context admission plane that:

- pushes bounded, reversible reductions into agent workflows;
- pulls current, authorized, task-shaped context across providers & repositories;
- persists typed memory with provenance, lifecycle, conflict, recovery, & user control;
- returns packet plus receipt for inclusion, omission, freshness, authority, delivery, & outcome;
- runs through Orthic Hub as sole desktop lifecycle owner;
- supports stdio MCP, CLI, loopback service, hooks, & generated thin SDKs from one protocol;
- remains useful offline without hosted database, vector service, model, telemetry, or paid control plane.

Membrane does not become autonomous agent runtime, source-of-truth code graph, general vector database, mutable fleet console, conversation-history compactor, or installer/signing framework. Cortex owns repository truth. Crypt owns durable memory. Orthic owns app/installer assembly. RightKit executes build control, native signing, packaging, update artifacts, & publication.

### 2.1 Full-scope dispatch rule

Every P0–P7 obligation remains binding. Workspace [`tasks/plan.md`](../tasks/plan.md) compiles them into file-exclusive dispatches for minimum elapsed time; it changes no mechanic, threshold, evidence gate, comparison, AX requirement, native qualification, publication, or delivery state. P0 truth plus P6 gate decisions freeze before source effects. Any P6 outcome admitted into production requires this plan's named amendment, then is implemented by that destination file's sole owner during its one implementation pass. P1–P5 may execute concurrently against frozen protocol/internal interfaces. P7 producer code may be prepared in that source wave, but P7 evaluation, claims, artifact production, & publication start only after integrated behavior receipts.

No production/test/config path may appear in two dispatch ownership sets. Generated outputs belong to generator owner. A new or omitted path must be assigned by integration owner before edit. Repair returns to same owner; no cleanup dispatch reopens another owner's file.

## 3. Frozen current truth

| Area | Current source-grounded fact | Plan consequence |
|---|---|---|
| Public surface | 10 MCP tools & 7 adapter definitions are generated from source | Preserve operation registry; claim only generated surface |
| Installed qualification | 0 of 10 Mac/Windows client pairs have current installed receipts | No supported-pair claim until P7 |
| Local database ownership | Crypt uses one shared `rusqlite::Connection` plus one event-ledger connection behind poison-recovering `Arc<Mutex<_>>`; this is not a pool | Keep single owner; add lock/hold/busy/poison metrics; never add SQLite pool by analogy |
| SQLite profile | WAL, `busy_timeout=5000`, `synchronous=NORMAL`, `temp_store=MEMORY`; immediate transactions protect compound mutations | Verify profile after open/migrate/restore; add checkpoint-starvation evidence |
| Sensitive data at rest | M39 requires encryption where threat model calls for it, but this plan previously left key lifecycle implicit | P3 freezes envelope format, OS-keystore key ownership, Membrane-owned key lifecycle, rotation, backup/restore, & crypto-shred behavior before encrypted records ship; Hub may transport opaque handles only |
| Remote database | No shared remote DB is required | M02 stays gated; no Postgres/Redis/vector service in local floor |
| Transport reuse | Clients are component-owned; no global cross-provider pool is required | Declare owner, scope/auth binding, limits, timeout, DNS/TLS, backpressure, shutdown, & saturation per M62 |
| Structural code languages | Native tree-sitter skeletonizer supports Rust, Python, JavaScript, & TypeScript | These four are only structural-language claim |
| Broader code handling | Compression classification recognizes Rust, Python, JS/TS/TSX, Go, Java, C/C++, shell, & Swift; unsupported inputs use typed fallback | Do not call classifier coverage parser support; publish per-transform capability matrix |
| Embeddings | Current source recognizes 768-d EmbeddingGemma, 384-d BGE, & 256-d degraded hash vectors | Do not freeze product to 768 dimensions; bind model, dimension, normalization, & degradation in each record/receipt |
| Repository graph | Cortex is source-graph authority; Membrane consumes generation/hash/path-bound candidates & resolves live source before admission | Never duplicate Cortex DB or treat derived graph as authority |
| Graph/index freshness | Cortex source bytes/graph remain external authority; Membrane-owned projections bind content hash, parser/model/schema version, generation, deletion, & current-source resolution | Add exact unchanged skip for Membrane-owned inputs; consume Cortex freshness through published API; refuse stale/partial references |
| Document recall | `doc_artifacts` exists; current imported-document sync still reads/parses every file; production indexed recall is not proven | Implement no-op imported-document sync & measured production lexical index before scale claim |
| Vector backend | Cross-host bakeoff selected Crypt-owned resident in-process f32 dispatch | Keep result; do not rerun or add external vector DB before 100k governed-memory trigger |
| Runtime | Resident federation, typed omissions, grants, lane semaphores, idempotency, compression, memory lifecycle, team-policy admission, & recovery primitives exist in source | Verify reachability/equivalence; extend existing owners instead of duplicating them |
| AX packaging | `plugin.json`, `mcp.json`, skill, strict input schemas, effects annotations, & typed tool errors exist; `.rightax.json` keeps static/conformance/behavioral gates report-only, `claimBoundaryRequired=false`, & current conformance server exercises one tool | P0 downgrades packaging claims; P1 closes operation/result contracts; P7 makes required gates blocking on all public tools |
| Hook coverage | Current runtime handles SessionStart, PreCompact, PostToolUse Bash observation, & PostToolUse write/edit ingestion; PostToolUseFailure, TaskCompleted, & SessionEnd are not wired | Add bounded, idempotent event coverage before default-on Push or learning claims |
| Desktop delivery | Repository still contains legacy Hub/release machinery, while current product boundary says Orthic owns installer | Supply signed add-on plus contract adapters to Orthic; retire Membrane-owned desktop lane after equivalent proof |

### 3.1 Review reconciliation

| Review finding | Decision | Binding treatment |
|---|---|---|
| Default-on Push, streaming transforms, unified CCR, query-aware reduction, & explicit lifecycle events | ACCEPT | P2 activates only bounded, reversible, cohort-measured paths with fail-open recovery |
| Result envelope separating invocation status, domain outcome, & claim boundary; strict schemas/errors/effects; hard AX gates | ACCEPT | P1 defines contract; P7 blocks release on full-surface conformance & adversarial proof |
| Proposal-only learning, pinned session context, episodic close packet, procedural proposals, contradiction handling, & offline proposal miner | ACCEPT | P5 uses existing memory lanes, quarantine, provenance, expiry, user control, & no automatic workspace mutation |
| Pre-transform secret scan, per-transform savings receipt, evidence-class labels, declared-versus-done drift, blind-spot explanation, cache-break forensics | ACCEPT | P0/P2/P3/P4/P7 own respective proof & observability |
| Kimi source corrections: CBM lexical/hooks/skill, TEN MCP/Promptwares, B0 crypto/checkpoints, LET background memory, PRA queues/training, C8 dependencies, COG RBAC, CC/HDR/RG/ZEP boundaries | ACCEPT_WITH_SCOPE | Correct comparison inventory; absorb only bounded storage, lifecycle, queue, auth, dependency, & proof mechanics. Training/agent-runtime features remain inventory-only |
| Membrane owns standalone app, installer, signing, update, or Hub lifecycle | REJECT | Orthic owns desktop lifecycle; RightKit owns packaging/signing/publication |
| Membrane keeps authoritative graph in its own SQLite | REJECT | Cortex remains graph authority; Membrane stores bounded projections/references only |
| Hub lifecycle can be restated inside Membrane | REJECT | Orthic owns supervisor side & released lifecycle contract; P1 implements only conforming Membrane child behavior |
| Cortex & Membrane may maintain separate candidate schemas | REJECT | Membrane protocol owns canonical candidate-set schema; Cortex conforms by exact released version/digest plus bidirectional goldens |
| Product CI may fall back to sibling Orthic schemas | REJECT | Conformance consumes released content-addressed contract artifact & fails closed when unavailable or mismatched |
| First-party-only/no external dependency as literal rule | CORRECT | Runtime may use audited local libraries; no mandatory hosted service, external data plane, or ungoverned provider |
| SQLite+FTS5 is mandatory implementation | CORRECT | Measured FTS5/BM25 or smaller exact local index may satisfy P3; contract governs behavior, not engine branding |
| EmbeddingGemma 768-d is sole embedding shape | REJECT | Current 768/384/256 shapes remain explicit; release truth names exact model/dimension/degradation |
| Shared tenant/fleet/RBAC should be categorically rejected | REJECT | M38 remains gated, not planned; only explicit multi-tenant product need plus isolation proof can open it |
| Old B1–B8/S1–S8 status list is current | CORRECT | B6 & S1/S5/S7/S8 are stale against source; memory/doc/fleet/code-batch reachability remains P0 work; no old status is inherited |
| One fixed 40% token/5-point quality contract replaces current gate | REJECT | Preserve each preregistered experiment's own baseline; release gate remains >=20% reduction with quality lower bound no worse than 1 point |
| Vendor or review-reported benchmarks become product targets | REJECT | Treat as vendor-reported evidence only until same-corpus local reproduction |

## 4. Status model

Every capability/package uses one state:

- `PROVEN_CURRENT`: current commit + exact command + passing receipt prove behavior.
- `IMPLEMENTED_UNQUALIFIED`: source exists, but installed/current qualification is absent.
- `VERIFY_CURRENT`: historical evidence or stale plan claims behavior; current reachability is unknown.
- `NOT_IMPLEMENTED`: current source inspection proves missing behavior.
- `GATED`: excluded until named trigger & decision receipt exist.
- `REJECTED`: M50 or explicit product boundary forbids behavior.
- `COMPLETE`: package exit, integration, native proof where required, & this register all close.

No completion inherits from old task IDs, commits, screenshots, tests, or installed generations. Each receipt binds source commit, dirty-tree digest, protocol version, config digest, dataset/corpus digest, platform, command, result, & timestamp.

## 5. Capacity, clock, & ownership

**Aggregate owner-effort ceiling: 3,120 minutes from minute 0; maximum-parallel critical-path target: 1,800 minutes excluding authenticated native-host waits.** One Membrane integration owner serializes repository HEAD/index/commits. Independent file owners, fixtures, benchmarks, native hosts, & assurance lanes overlap against frozen interfaces.

Inputs used for ceiling: 39-repository capability pass; 62 mechanics; 966 workspace docs; 254 Rust-crate files/92,502 relevant lines; 117 Hub files/9,832 lines; 76 MCP files/8,653 lines; existing test, benchmark, protocol, release, & evidence harnesses.

Change ceiling: at most 160 changed files & 19,200 total changed lines, including generated outputs; within that, at most 120 hand-authored files & 7,200 hand-authored changed lines. Capacity ratio is 2.31 hand-authored changed lines per elapsed minute (`7,200 / 3,120`), used only as stop-loss, never as productivity evidence. Generated schemas, locks, fixtures, & raw receipts must remain mechanically derived. Any ceiling breach stops implementation for scope reduction or explicit plan amendment.

| Package | Effort-accounting span | Named work, summing to package span | Hand-authored ceiling | Primary mechanics | Status |
|---|---:|---|---:|---|---|
| P0 Truth & proof freeze | 0–240 | current reachability 90; golden baselines 90; requirement ledger 60 | 16 files / 700 lines | M41, M43, M49, M61 | READY_IN_FREEZE |
| P1 Authority, protocol, lifecycle | 240–960 | contracts 160; Hub/lifecycle 220; approvals/extensions/tool policy 220; compatibility 120 | 28 / 1,900 | M11, M19, M20, M23, M24, M31, M34, M37, M39, M42, M44, M45, M46, M47, M50, M52, M53, M57, M59, M60, M62 | READY_AFTER_FREEZE |
| P2 Bounded I/O & resources | 960–1,440 | streaming output 180; resource registry 180; load/fault proof 120 | 18 / 1,200 | M18, M21, M22, M25, M26 | READY_AFTER_FREEZE |
| P3 Storage, cache, freshness | 960–1,800 | storage telemetry 180; integrity/recovery 180; graph/document freshness 240; cache safety 120; faults 120 | 30 / 2,000 | M01, M03, M04, M05, M06, M07, M08, M09, M10, M12, M16, M27, M28, M29, M30, M35, M40, M48 | READY_AFTER_FREEZE |
| P4 Retrieval, graph explanation, interchange | 1,800–2,280 | indexed retrieval 180; graph/explanation UI 120; interchange 90; qualification 90 | 16 / 800 | M17, M55, M56 | READY_AFTER_FREEZE |
| P5 Memory & team-policy transport | 2,280–2,640 | memory closure 180; policy transport 120; adversarial proof 60 | 8 / 400 | M14, M32, M51 | READY_AFTER_FREEZE |
| P6 Gated decisions | 2,640–2,820 | DB/transport trigger 30; low-level optimization 30; user sync 30; tenant mode 30; multimodal 30; scheduling/webhook 30 | 2 / 80 | M02, M13, M15, M33, M36, M38, M54, M58 | READY_IN_FREEZE |
| P7 Evaluation, Orthic delivery, release truth | 2,820–3,120 | comparative evaluation 120; native qualification 120; generated truth 60 | 2 / 120 | all mechanics & release gates | BLOCKED_BY_REQUIRED_DISPATCH_RECEIPTS |

P0–P7 are obligation namespaces, not dispatch order. Their elapsed spans remain effort-allocation evidence. P1–P5 file owners implement concurrently from one sealed interface packet; vertical fixtures replace code-output waits. P7 prepares clean hosts/corpora concurrently but release claims wait for every required dispatch receipt.

## 6. P0 — truth & proof freeze

### Work

1. Run current doctor, generated-doc checks, protocol/schema checks, Node/Python suites, RightKit-managed Rust workspace tests, release-identity checks, & source reachability sweep from clean committed state.
2. Classify every M01–M62 row with status model above. Record owner path, test producer, receipt path, dependency, & public claim.
3. Classify every public module/tool/adapter/hook as release-path reachable, test-only, support-only, uncalled, duplicate, or dead. Public means invoked from a non-test entrypoint under supported configuration; export or passing unit test alone is insufficient.
4. Freeze canonical fixtures for packet order, authority, freshness, omissions, delivery bytes, pagination, structured output, approval transitions, retries, cancellation, command output, migration, backup, lifecycle, graph freshness, support pairs, & benchmark provenance.
5. Freeze real provider corpus plus warm/cold profile. Empty-provider or synthetic-only results cannot prove runtime performance.
6. Record DB paths/identity, journal state, WAL frames, lock wait, process census, listeners, release generation, installed artifacts, adapter configs, & duplicate/stranded files using read-only inventory.
7. Label every number or claim `measured`, `calculated`, `estimated`, `counterfactual`, or `vendor-reported`; bind measured/calculated claims to reproducible input & receipt.
8. Preserve dirty overlays outside baseline; no current claim may consume uncommitted bytes.

### Exit

- machine-readable 62-row closure ledger has no missing mechanic;
- every `ALREADY` claim is either `PROVEN_CURRENT` or downgraded;
- exact failing baseline is retained, not relaxed;
- current generated support matrix still says unavailable unless matching installed receipts exist;
- later receipts name frozen plan, `sol.md`, protocol, corpus, & fixture digests.

## 7. P1 — authority, protocol, lifecycle

### Work

1. Make one Rust planner own eligibility, grant/policy, authority/freshness ordering, fusion, budget, rendering, omissions, publication epoch, & exact delivery serialization. Python/Node/providers return typed candidates only.
2. Generate schemas/bindings/docs from Rust types. Require byte-equivalent Rust/TypeScript/Python goldens, explicit version negotiation, unknown-field policy, & future-version refusal.
3. Conform Membrane child to pinned `orthic.lifecycle.v1`: inherited authenticated channel, hello/ready endpoint registration, installation/instance/artifact/fence identity, drain/stop/update handoff, parent-loss handling, & stale-fence rejection. Orthic alone owns lease issuance, supervision, restart/backoff, compatibility evaluation, & child-tree cleanup.
4. Keep foreground CLI/MCP one-shots nonpersistent. Reject rogue listener, foreign install, stale fence, incompatible protocol, wrong data root, or post-update old child. Remove `SpawnFresh`/Cortex pidfile ownership, direct Cortex `graph.db` access, & shipping `apps/membrane-hub` only after published Cortex API plus Orthic migration prove equivalent behavior.
5. Apply one monotonic deadline + cancellation token across HTTP/MCP/provider/DB/subprocess layers. Require keyed idempotency for retried external mutation.
6. Add suspended approval state bound to exact operation/version, canonical arguments, subject, resource/scope, effects, risk, policy epoch, approver, expiry, & one-shot/reusable class. Recheck edited/resumed request; reject replay.
7. Validate model-produced structured output against versioned bounded schema before authorization/publication. Preserve original + repair attempts; never repair authority fields.
8. Use signed manifest + runtime handshake for connector/extension discovery. Declare capabilities, permissions, resource limits, isolation, health, versions, egress, cancellation, idempotency, update, rollback, & deterministic disable.
9. Resolve per-tool policy against canonical arguments & effects before execution. Output never self-authorizes follow-up work.
10. Declare every HTTP/model/Redis/other transport pool independently from DB ownership; authenticated connections cannot cross incompatible scope/tenant.
11. Give every operation one result envelope separating transport/invocation status, domain outcome, & claim boundary. Generate closed output schemas, typed errors with remediation, opaque handle/cursor rules, side-effect class, cancellation/idempotency semantics, & long-running completion state from operation registry. Publish `membrane.context-candidate-set.v1` from canonical Rust protocol; validate Cortex golden input & reject unsupported future versions.
12. Require every public tool to state when to use/avoid it, validate live schema examples, reject secret-bearing inputs/outputs by policy, & declare destructive or external effects before authorization.
13. Enforce M50 exclusions in tests & configuration validation.

### Exit

- equivalent request yields equivalent packet/receipt across supported transports/languages;
- revoke-during-publication emits zero obsolete-policy bytes;
- concurrent Hub, stale owner, lease loss, parent kill, crash loop, update handoff, off, quit, uninstall, & rogue port tests pass on both hosts;
- zero Membrane watcher/pidfile ownership, direct Cortex private-store reads, or shipping standalone Hub paths remain;
- approval crash/replay/argument-edit tests pass;
- hostile/incompatible extension cannot mint grant, broaden root, escape sandbox, or survive disable;
- every partial/fallback state has typed cause, impact, completeness, & recovery action.

## 8. P2 — bounded I/O & resources

### Work

1. Replace full-memory `run_capped` capture with streaming bounded head/tail. Start spill only after breach; bind full bytes by digest; preserve stdout/stderr order plus exit/signal.
2. Add stable opaque cursor to every unbounded list/search/audit/export. Bind filter, order, scope, policy epoch, generation, & last key.
3. Use bounded channels, propagated disconnect cancellation, finite child settle/kill/reap, terminal partial result, & durable-mutation completion/replay semantics.
4. Centralize operation item/byte/token/CPU/wall/model-call, memory, disk, descriptor, spill, queue, cache, & derived-growth budgets. Validate at startup; reserve before work; preserve diagnostics lane.
5. Keep existing lane semaphores & fair admission. Never create unbounded tasks, threads, queues, output buffers, or retry loops.
6. Add default-on Push only for preregistered cohorts: start with PostToolUse Git/test/command output; pre-scan secrets; stream classify -> reduce -> validate -> deliver; fail open to bounded raw recovery when reduction cannot prove safety.
7. Use one canonical context-recovery marker for `run_capped`, skeletonization, & compression. Bind source digest, transform/version, kept head/tail/protected spans, dropped-byte digest, recovery handle, expiry, & exact restore verification.
8. Normalize host lifecycle events into SessionStart, UserPromptSubmit, PostToolUse, PostToolUseFailure, PreCompact, TaskCompleted, & SessionEnd, then map each to assemble/admit/render/deliver/reconcile. Handlers remain bounded, idempotent, deadline-aware, & typed when host lacks an event.
9. Put committed asynchronous mutations through one bounded durable outbox with stable event ID, attempt/deadline/backoff state, idempotent consumer, acknowledgement, replay cursor, & operator-visible DLQ. Never use DLQ as silent data loss or automatic authority promotion.

### Exit

- adversarial interleaved output stays under resident cap & remains fully recoverable;
- Push cohort proves no secret leakage, silent truncation, duplicate ingestion, or unrecoverable transform; disabled cohort changes no output;
- cursor mutation/generation/scope mismatch rejects without duplication or omission;
- timeout/disconnect/fault tests classify queued, running, committed-after-timeout, cancelled, & killed states;
- outbox crash/replay/duplicate/poison-item tests prove exactly-once effect or explicit unresolved DLQ state;
- soak/load proves declared memory, CPU, disk, queue, file, spill, & descriptor ceilings; diagnostics remains responsive.

## 9. P3 — storage, cache, graph & document freshness

### Work

1. Keep explicit single owner per SQLite store. Instrument mutex wait/hold, poison recovery, busy retries, transaction age, WAL frames, checkpoint progress/starvation, queue depth, sizes, schema, journal profile, & owner lease.
2. Add bounded per-store batch mutation with item/byte/time caps, one transaction, per-item result, no cross-scope batch, & cancellation before commit.
3. Verify pragma profile after open/migrate/restore. Use passive live checkpoint; reserve truncate/seal for drained exclusive maintenance.
4. Unify quick check, offline deep integrity, corruption quarantine, startup reconciliation, backup, dry-run restore, vault export/import, wipe, migration backout, & new-file compaction with atomic adopt + rollback.
5. For threat-model-classified payloads, use versioned envelope encryption with fresh per-record/blob DEK, authenticated metadata, OS-keystore-owned KEK, Membrane-owned encryption/key-lifecycle semantics, atomic KEK rewrap rotation, fail-closed decrypt, key-loss recovery contract, backup/export key policy, & crypto-shred tombstone that invalidates every plaintext derivative. Hub may transport opaque credential handles through lifecycle but never key bytes or data authority. Never log/export key bytes or destroy sole decryptable copy outside explicit forget policy.
6. Freeze vector storage contract: f32 type, dimensions, model, normalization, endianness, checksum, tombstone, & rebuild version. Preserve bakeoff outcome.
7. Build one cache-key contract over operation, normalized input, source hash/generation, parser/model/schema, scope, grant, policy, & sensitivity epoch. Add ordered invalidation, bounded capacity/TTL/eviction, stale refusal, metrics, & recovery source.
8. Attribute each cache miss/break to exact changed key component; add bounded perf-doctor output for churn, invalidation fanout, hit quality, false reuse, rebuild cost, & cold/warm deltas.
9. For Membrane-owned memory/imported-document/policy/session sources, use exact content hash + parser/model/schema version to skip read/parse where safe. Changed imported documents include Membrane-owned dependency closure; deletions tombstone in same generation. Repository discovery/indexing remains Cortex-only.
10. Build Membrane-owned memory/document projections in staging; validate coverage & source resolution; atomically publish one opaque generation. Cortex graph state remains an external versioned reference resolved through published API, never copied or opened directly. Stale/partial build never advertises current. Cold start detects drift; manual rebuild is bounded & receipted.
11. Replace production O(N) Membrane-owned document projection scan with measured FTS5/BM25 or smaller exact index. Preserve Unicode, camelCase/snake_case identifiers, path, anchor, short substring, stable ranking, source hash, & deterministic fallback.

### Exit

- crash at each mutation/backup/restore/migration/adopt boundary preserves sole copy & produces recovery path;
- encrypt/decrypt/tamper/wrong-key/rotation/interrupted-rewrap/backup-restore/forget/crypto-shred fixtures preserve declared recovery & remove all admitted derivatives after shred;
- active readers cannot starve WAL invisibly; exclusive maintenance waits for drain;
- unchanged source does zero parse/index mutation; change/delete/dependency cases publish one complete generation;
- revoke/model/parser/schema/source change yields zero stale admission through cache/vector/graph/document lanes;
- 12k-document corpus avoids production O(N) recall scan; 100k governed-memory recall remains within release gate.

## 10. P4 — retrieval, explanation, UI & interchange

### Work

1. Run eligibility before retrieval: grant, ACL, scope, quarantine, temporal validity, supersession, generation, sensitivity, & source availability.
2. Fuse exact anchor, lexical/BM25, vector, entity/graph, temporal, active-overlay, rules, skills, audit, memory, & Cortex lanes deterministically. Live-file/Git access may only resolve or validate Cortex-issued source references at admission time; it cannot discover/index repositories, own freshness, or become parallel graph authority. Stable tie uses canonical ID; learned reranker remains shadow-only until holdout proof.
3. Apply query-kind boosts only inside each provider before cross-provider authority ordering/fusion. Receipt records winning signal, lane contribution, exclusions, known blind spots, fallback, & per-candidate rank explanation.
4. Segment on syntax/record boundaries first, token fallback second. Bind every chunk to source span/hash/parser version & cap overlap/derived growth.
5. Add Membrane packet explanation projection used by CLI/MCP plus bounded Orthic snapshot: source span/reference, Cortex generation/as-of, freshness, omission, lane/rank/rationale, grant, conflict, & resolver. It does not redefine Cortex graph schema. UI never invents unseen edge or bypasses admission.
6. Keep canonical JSON/signed pack as lossless authority. Secondary GraphML/JSON-LD/CSV/Parquet/other formats declare schema, identity mapping, provenance/ACL loss, ordering, limits, checksum, round-trip class, & collision policy.
7. Reconcile declared capability against observed release-path execution; surface missing/dead/drifted operations without promoting test-only code.

### Exit

- fixed corpus stays deterministic across completion order & supported hosts;
- graph or vector unavailable path degrades to typed deterministic fallback;
- UI output reconciles byte/identity facts with CLI/MCP receipt;
- lossy export is never accepted as backup; lossless export/import passes identity, policy, provenance, & collision fixtures.

## 11. P5 — memory & team-policy transport

### Work

1. Close Working, Episodic, Semantic, Procedural, Entity Summary, Evolving Belief, Artifact Reference, scratchpad, temporal fact, feedback, conflict, supersession, expiry, quarantine, restore, forget, & audit contracts.
2. Separate retain, recall, reflect, consolidate, correct, supersede, expire, forget, quarantine, restore, export, migrate, & audit. Derived beliefs remain reversible & never self-certify.
3. Preserve exact protected spans in compression; every transform records source hash, kept/dropped reason, budget, risk, token delta, & recovery reference.
4. Store pinned session/task context as scoped, provenance-bound, expiry-bound records in existing Working/scratchpad lanes; never create parallel memory authority.
5. On SessionEnd/TaskCompleted, produce bounded episodic candidate packet with cited outcomes, contradictions, unresolved work, & source receipts. On PreCompact, preserve protected spans plus recovery marker. On PostToolUseFailure, capture typed failure evidence without secrets.
6. Generate procedural/evolving-belief changes only as quarantined proposals. Deterministic contradiction checks run before proposal; optional offline transcript miner emits proposals only. No feedback, miner, or model may directly edit files, hooks, policy, permissions, or durable authority.
7. Wire existing opt-in team-policy admission to authenticated E2E-encrypted envelope transport with device identity, stable event ID, sequence/fence, ack/resume cursor, duplicate/out-of-order handling, key rotation, offboarding, deadline/backoff, & content-free audit.
8. Disabled sync performs zero transport/background work. Transport cannot bypass M32 admission or write live SQLite/WAL files.

### Exit

- provenance, temporal as-of, contradiction, supersession, expiry, scope, ACL, scratchpad non-promotion, derived belief, quarantine/restore, migration/backout, & protected-span suites pass;
- two-device policy duplicate/out-of-order/reconnect/key-rotation/offboard corpus passes;
- no policy plaintext/key material enters ledger, log, metric, receipt, or diagnostic bundle.

## 12. P6 — gated decisions

P6 produces decision receipts, not speculative infrastructure. A gate can change to implementation only by amending this plan with exact owner, threat model, compatibility, recovery, benchmark, file/line/minute delta, & release impact.

| Mechanics | Default | Trigger |
|---|---|---|
| M02 remote DB pool | GATED | real shared Hub/team backend plus measured concurrent transactions |
| M13 storage/wire compression | GATED | immutable large-blob/export/transport CPU-RSS-latency win with expansion bound |
| M15 mmap/zero-copy | GATED | profiling proves copy/RSS bottleneck on immutable checksummed blob |
| M33/M36 user-data sync | GATED | explicit data-class product contract, E2E threat model, conflict/deletion/recovery UX |
| M38 shared tenant store | GATED | authenticated multi-tenant deployment with row/index/cache/queue/export isolation proof |
| M54 multimodal | GATED | named product need + licensed corpus + local extraction/privacy budget |
| M58 scheduler/webhooks | GATED | named user workflow; signed delivery, SSRF/egress, replay, misfire, fencing, & zero-work-disabled contract |

Quantization, opaque codec, worker/thread/concurrency change, external vector DB, & low-level pool tuning also remain measurement-gated. External vector backend cannot be reconsidered before 100k governed memories & fixed bakeoff protocol show current resident f32 path misses a release gate.

## 13. P7 — evaluation, Orthic delivery & release truth

### Work

1. Run real LoCoMo, LongMemEval, BEAM, retrieval, graph-freshness, durability, & commit-reveal whole-task corpora against exact release. Mark each result `source-ready`, `ran`, or `qualified`.
2. Compare eligible local/self-hosted repositories through same corpus, adapter, model/provider, hardware class, budget, metric, warm/cold policy, repetitions, variance, failure archive, & raw receipt schema. Hosted/archived/incompatible exclusions stay explicit.
3. Measure task success, stale/unauthorized context, authority misses, precision/recall/MRR/nDCG, contradiction/temporal accuracy, token/cached-token use, latency, CPU/RSS, queue/lock/cache/WAL, DB/index growth, restore, & cost.
4. Emit signed per-transform savings receipts plus aggregate ledger: source/result digests, transform/version, evidence class, before/after bytes/tokens, protected-span result, recovery handle, latency/resources, quality verdict, & cohort. Content-free telemetry contains no source text.
5. Run Right AX plugin/static/conformance gates against every public tool; static & conformance block integration, while behavioral/adversarial gates block release. Require claim boundary, closed input/output schemas, effects, examples, errors, truncation/cursors, cancellation, & secret-safety proof.
6. Produce signed Membrane add-on/runtime plus opaque Hub-facing manifest/snapshot/lifecycle child adapter. Orthic consumes only released contract-conforming artifact by exact digest. Membrane retains packet/operation/provider-candidate schemas, legal sources, qualification, product truth, stores, & key lifecycle. Remove standalone Membrane installer/release ownership only after Orthic proves equivalent install/update/rollback/uninstall.
7. Use RightKit-managed Rust commands. Build/sign Membrane add-on on native hosts through `right-release`; Orthic builds/signs/seals suite installer once per platform. Keep credential-bearing work out of CI; publish only exact user-authorized build.
8. Regenerate product truth, support matrix, exact tool/prompt/resource/skill/hook counts, dependency boundaries, operation docs, schemas, SBOM/provenance, checksums, support bundle contract, & benchmark report from accepted receipts.

### Frozen release gates

| Axis | Gate |
|---|---|
| Packet/protocol equivalence | 100% exact candidates, order, packet, omissions, delivery bytes, & receipt fixtures |
| Scope/security | 0 ACL/scope leaks, denied-source reads, path escapes, unauthorized egress, stale-policy bytes, or cross-scope connection reuse |
| Freshness | 0 stale/partial graph or document generation published as current; exact source resolution before current-evidence admission |
| Warm full-provider context | p50 ≤75 ms, p95 ≤125 ms, p99 ≤250 ms |
| Ready no-op | p95 ≤25 ms |
| Cold/degraded | bounded result or typed failure ≤1.5 s |
| Local recall | p95 ≤50 ms on current corpus; ≤100 ms at 100k governed memories |
| Task quality | holdout 95% CI lower bound no worse than baseline by 1 percentage point; no authority/safety regression |
| Token economy | ≥20% reduction versus full-context baseline at noninferior quality |
| Experiment integrity | every target is preregistered per corpus/cohort; old 40%/5-point studies remain separate evidence & never rewrite release threshold post hoc |
| AX | all public tools pass blocking static/conformance plus release behavioral/adversarial gates; claim boundary required |
| Resources | idle CPU <1% per child over 10 min; <6 combined wakeups/min; RSS regression ≤10%; all declared queues/files/spill bounded |
| Storage/recovery | active backup + clean restore + old-schema backout + event/key/count/recall equivalence; 0 data loss |
| Language truth | structural claim limited to passing Rust/Python/JavaScript/TypeScript fixtures; every other transform/fallback named separately |
| Support | each advertised platform/client pair has current exact-artifact installed receipt; unavailable pairs remain unavailable |
| Delivery | source commit, release generation, signed artifact hashes, installed bytes, support receipt, nested ref, & parent pin match |
| Seam conformance | pinned `orthic.product-manifest.v2`, `orthic.lifecycle.v1`, `orthic.snapshot.v2`, & `membrane.context-candidate-set.v1` version/digest goldens pass; no sibling-source fallback |
| Ownership grep gates | zero Cortex watcher/pidfile ownership, direct Cortex `graph.db` open, shipping standalone Hub paths, shared mutable store root, or Hub-held key bytes |
| Cross-repo release | Orthic contract released → Cortex/Membrane conform → both add-ons adopted by exact digest → one installer built once → Mac/Windows installed proof → quit/off/update yields zero orphan children |

Release is complete only when every non-gated M01–M62 row is `COMPLETE`, every gated row has current decision receipt, every rejected row has enforcement proof, required gates pass on exact native artifacts, Membrane commit is pushed, parent gitlink pins that commit, & generated public claims match installed evidence. Parent pin is workspace history only; runtime compatibility derives from released contract versions/ranges & artifact digests.

## 14. Durable trail retained

Keep these as evidence/rationale, never implementation status:

- [`docs/benchmarks/vector-backend/`](docs/benchmarks/vector-backend/) plus [`engine/vector-bakeoff/README.md`](engine/vector-bakeoff/README.md): immutable cross-host vector bakeoff & decision;
- [`docs/plans/2026-08-01-vector-backend-bakeoff-harness.md`](docs/plans/2026-08-01-vector-backend-bakeoff-harness.md): bakeoff protocol;
- `evidence/`, `docs/runs/`, `docs/evaluation/`, `benchmarks/`, generated support/product truth, protocol docs, migrations, security, operations, legal, & compatibility docs;
- workspace `docs/evidence/`, `docs/baselines/`, `docs/reviews/`, `docs/runs/`, governance, rules, release runbooks, unrelated product plans, & archived Membrane lineage;
- workspace [`SEAM-CONTRACT.md`](../docs/plans/orthic/SEAM-CONTRACT.md) as active boundary authority; prior seam executable contracts remain evidence/rationale only;
- [`docs/MEMBRANE-STATE.md`](docs/MEMBRANE-STATE.md) as historical rollout ledger only; source + current receipts override its dated status.

Archived documents remain readable under `/Volumes/D/claude/docs/archive/membrane-plan-lineage-2026-08-14/`. Restore one only for evidence recovery; never reactivate its task IDs or status model.

## 15. Completion register

Update this table only after package exit receipt is verified against current commit.

| Package | Implemented commit | Verification receipt | Native receipt | Final state |
|---|---|---|---|---|
| P0 | — | — | n/a | READY_IN_FREEZE |
| P1 | — | — | — | READY_AFTER_FREEZE |
| P2 | — | — | — | READY_AFTER_FREEZE |
| P3 | — | — | — | READY_AFTER_FREEZE |
| P4 | — | — | — | READY_AFTER_FREEZE |
| P5 | — | — | — | READY_AFTER_FREEZE |
| P6 | — | — | n/a | READY_IN_FREEZE |
| P7 | — | — | — | BLOCKED_BY_REQUIRED_DISPATCH_RECEIPTS |
