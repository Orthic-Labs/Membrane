# Membrane whole-product semantic implementation audit

**Frozen target:** `a4e0a9e02c64e5439cdec289a160b748f5e5686c` (current agreed checkout state supplied to Session A, 2026-09-07). The embedded `75c842…` packet baseline was explicitly superseded by the user's instruction to ignore prompt commits. No remote was configured in this checkout, so “fetch current main” and main-reachability could not be established; the audit does not relabel this SHA as `main`.

**Method and limits.** Independent static/source audit only. I read no Session B artifact or conclusion, ran no product test/build/runtime, changed no production source or user data, and made no runtime-execution claim. Exact line references below are in the frozen archive. All findings are source-confirmed unless labelled risk. The breadth requested is larger than can honestly be exhaustively certified in one static pass: the trace matrix explicitly marks shallow/uninspected areas.

## Executive verdict

**Not whole-product ready.** The installed native binding is coherent in selecting `membrane stdio-mcp`, but the resulting stdio process is a transport proxy to the active tray-owned daemon. That correctly refuses normal Membrane/Ledger operation when Hub is off, yet it also refuses the contractually retained explicit Blueprint one-shot exception. When Hub is on, the native MCP executor reaches real Pull, Ledger, Cortex, Push, Adapt, and diagnostics code, but composition remains materially incomplete: Pull represents two unimplemented owners as successful complete-empty providers; workspace federation executes children serially; Push recovery identity omits task/session identity; diagnostics audit failures are silently discarded; and generated discovery reports tools without qualification/readiness state. Canon itself corroborates incompleteness: 353 atomic rows were enumerated; Pull is mostly `PARTIAL`/`UNKNOWN`, Push mostly `PARTIAL`, Ledger mostly `PARTIAL`, and Membrane mostly `PARTIAL`/`UNKNOWN`.

## Production journey synthesis

1. **Installed discovery/cold call.** Root `mcp.json` binds `membrane stdio-mcp` (`mcp.json:4-10`); Claude plugin binds its packaged `membrane.exe stdio-mcp` (`.claude-plugin/plugin.json:18-22`). `run_stdio_mcp` installs `HubTransportExecutor` and serves native MCP (`engine/crates/membrane-runtime/src/serve.rs:5822-5829`). Selection checks stable-current runtime paths/activation elsewhere, while source JS installation is explicitly development-only (`mcp/install.mjs`). Discovery advertises 23 schemas from `membrane-mcp`, but executor readiness is not part of tool discovery.
2. **Hub off/on.** `install_native_mcp_transport` installs either an active Hub proxy or one executor that returns `membrane_unavailable` for every tool (`mcp_executor.rs:1895-1908`, `510-517`). Active binding reads installed identity/token, performs `/health`, and checks installation/store/release identity (`345-396`) before POSTing `/mcp` (`440-495`). This is strong transport identity binding but loses the Blueprint offline exception (SEM-001).
3. **Context acquisition.** Hub executor authorizes before dispatch (`533-655`). `membrane_context` validates task/session/task/H8 and calls `native_route_response`; native federation binds release/freshness/scope before scheduling providers (`membrane-federation/src/engine.rs:213-281`), permits one corrective stage (`300-370`), then planner/publication/Push wrapping runs. Blueprint and anchors share the same contextual Blueprint source, while Ledger is separately daemon-owned. Audit and decision lanes are fake complete-empty sources (SEM-002).
4. **Documents/code/knowledge.** Ledger MCP reaches `LedgerService::operation/read` under a bounded work budget (`mcp_executor.rs:1159-1178`); Blueprint MCP reaches daemon IPC with generation pin and two-second bound (`1181-1254`); Cortex uses the resident `MemoryStore` and signed installation-owned review trust (`cortex_lifecycle.rs:39-146`). Pull creates typed provider registrations for Blueprint, Ledger, Cortex, rules, live files, git, audit, architect and skills (`pull/native_federation.rs:67-129`).
5. **Freshness/convergence/recovery.** Blueprint JS owns generation/journal/watcher and complete-generation publication; resident service IPC enforces pending capacity/deadlines (`blueprint/src/service/server.mjs:142-293`). Static inspection found bounded fallback traversal, generation-indexed storage and complete-generation checks, but did not establish every edit interleaving. Cortex and Push use SQLite transaction boundaries; Push verifies retained bytes/digests on publish/resolve (`push/recovery.rs:284-352`).
6. **Diagnostics/review.** Production diagnostics installs real daemon Blueprint findings plus lazily discovered TypeScript and rust-analyzer adapters (`live_diagnostics_service.rs:652-762`). This is not merely a test seam. Missing providers are typed; no source route directly invokes Cargo, though rust-analyzer behavior requires runtime observation to certify. Audit persistence is best-effort and invisible (SEM-005).
7. **Adapt lifecycle.** MCP inspection is read-only over scoped store projections (`mcp_executor.rs:657-693`); feedback is forcibly advisory and persisted with `verified:false` (`1410-1459`). Full observation→proposal→admission→host-use causal effectiveness was not proven and remains incomplete in canon.
8. **Multi-repository/platform.** Authorization enumerates explicitly granted targets and shares one token ceiling, but executes target federation in a blocking serial loop (SEM-003). Current generated truth says Windows only. Static installation/update separation was inspected; actual CI-produced artifact identity/upgrade/rollback was not executed.

## Prioritized findings

### SEM-001 — explicit Blueprint Hub-off operation is unreachable through installed MCP
- **Journey/atoms:** A/B; BPT-042, MEM-008, MEM-D012. **Severity:** High.
- **Trigger:** installed client starts `membrane stdio-mcp` while tray daemon is inactive, then invokes `membrane_blueprint` explicitly.
- **Expected:** bounded Blueprint one-shot read, no watcher/daemon; normal context/Ledger remain typed unavailable.
- **Actual path:** `run_stdio_mcp` → `install_native_mcp_transport` selects a single `UnavailableHubTransportExecutor`; its `execute` returns Hub-inactive for *all* names (`mcp_executor.rs:510-517,1895-1908`). There is no name-sensitive Blueprint adapter in this path.
- **Consequence:** advertised Blueprint independence is not usable by the actual installed MCP binding with Hub off.
- **Evidence/confidence:** definitive static call path, high.
- **Smallest owner-correct repair:** in stateless transport composition only, route explicit Blueprint operations to Blueprint’s bounded one-shot application adapter when Hub is inactive; keep every other tool on typed refusal and prohibit watcher/store writer startup.
- **Acceptance:** installed-boundary test observes explicit Blueprint result with generation/freshness, no spawned daemon/watcher, while `membrane_context` and `membrane_ledger` remain `hub_inactive`.

### SEM-002 — absent audit/decision owners are reported as complete empty evidence
- **Journey/atoms:** C/D/I; PUL provider accounting and MEM final publication. **Severity:** High.
- **Trigger:** any Hub-on context request that could require audit findings or decision records.
- **Expected:** unimplemented/unbound provider remains a visible unavailable/omitted lane; absence cannot become negative evidence.
- **Actual:** `NativeSourceBindings::with_store` installs `EmptyAuditSource` and `EmptyDecisionSource` (`federation_sources.rs:96-115`); both return empty values with `complete:true` and no warning (`184-225`). Native federation registers both as real providers (`native_federation.rs:93-104`).
- **Consequence:** receipts can mean “searched completely, found none” when no production owner was queried, corrupting sufficiency and negative-evidence semantics.
- **Evidence/confidence:** definitive, high.
- **Repair:** omit registrations or return typed `ProviderUnavailable`/incomplete source responses until real owner handles exist.
- **Acceptance:** a contract requiring either lane ends insufficient with an explicit omission; it never reports complete-empty.

### SEM-003 — workspace fan-out is serial and does not enforce one absolute deadline across children
- **Journey/atoms:** C/L/resource; MEM workspace aggregation, Pull deadline. **Severity:** High (deadline correctness/performance).
- **Trigger:** `scope=workspace` with multiple authorized targets.
- **Expected:** independent targets run in parallel behind one ingress deadline/cancellation; late children become typed omissions.
- **Actual:** executor computes shares then loops targets and synchronously calls `native_route_response` one at a time (`mcp_executor.rs:766-833`). Each child receives the original `deadlineMs` as a fresh `maxWaitMs` (`800-802`), so elapsed work in earlier children is not subtracted.
- **Consequence:** wall time scales with target count and can exceed caller deadline by approximately the serial sum; cancellation/deadline semantics reset per child.
- **Evidence/confidence:** definitive source behavior, high; latency magnitude unmeasured.
- **Repair:** compute one absolute deadline at ingress, dispatch bounded-concurrency child work, pass remaining duration, and stop/omit after expiry.
- **Acceptance:** deterministic slow-child harness proves total work bounded by one deadline and healthy siblings preserved.

### SEM-004 — Push recovery namespace omits task and host session identity
- **Journey/atoms:** H/I; Push recovery/final wire. **Severity:** High (cross-task disclosure within a granted scope).
- **Trigger:** two agent tasks/sessions share the same canonical repository root and caller `scopeId`; one obtains another’s opaque recovery handle.
- **Expected:** recovery is bound to repository/worktree, scope/store, task and session identity plus expiry.
- **Actual:** MCP Push constructs `RecoveryScope::new(root, caller.scopeId)` (`push/api.rs:17-27`); `RecoveryScope` hashes only canonical root and that one string (`push/recovery.rs:59-72`). `sessionId`/`taskId` are not included. Resolve queries by `(scope, handle_digest)` (`324-340`).
- **Consequence:** handle possession permits exact recovery across tasks/sessions sharing a scope, contrary to identity isolation.
- **Evidence/confidence:** definitive, high.
- **Repair:** version recovery scope/handle binding to include verified sessionId and taskId and validate them on resolve; preserve explicit compatibility refusal for old handles.
- **Acceptance:** same-task resolve succeeds; wrong task or session returns typed scope denial despite same root/scope and valid handle.

### SEM-005 — diagnostics audit storage failure is silently erased
- **Journey/atoms:** G/I; diagnostics evidence/accounting. **Severity:** Medium.
- **Trigger:** audit directory/file creation/open/write fails (full disk, permissions, corruption).
- **Expected:** material omission/storage degradation visible in operation/health receipt; diagnostic result must not appear fully auditable.
- **Actual:** `AuditSink::record` returns silently on serialization, mkdir, open, or write failure (`live_diagnostics_service.rs:167-206`).
- **Consequence:** a fence decision may be returned without durable audit evidence and without a caller-visible degradation.
- **Evidence/confidence:** definitive, high.
- **Repair:** return an audit disposition and merge a typed omission into the diagnostic response/health while retaining availability policy.
- **Acceptance:** injected unwritable sink yields otherwise-correct result plus stable `audit_persistence_unavailable`, never silent success.

### SEM-006 — public discovery overstates readiness and callability
- **Journey/atoms:** A; MEM discovery/status. **Severity:** Medium.
- **Trigger:** any stdio session with inactive Hub or unavailable Ledger/Blueprint/provider.
- **Expected:** explicit capabilities/readiness distinguish advertised schema from usable owner state.
- **Actual:** generated docs and MCP list advertise the same 23 tools unconditionally; the stdio executor may be globally unavailable and Ledger separately may be absent (`mcp_executor.rs:1160-1162`). Discovery has no owner readiness handshake. 
- **Consequence:** “tool exists” is indistinguishable from usable provider; first useful call can fail despite successful negotiation.
- **Evidence/confidence:** confirmed semantic mismatch, high.
- **Repair:** retain stable names but expose typed capability/readiness resource/status tied to active daemon/provider identities; clients must not interpret discovery alone as readiness.
- **Acceptance:** Hub-off discovery plus readiness reports tool schema present but operational state unavailable; Hub-on reports per-owner readiness.

### SEM-007 — per-request Pull composition repeats owner/store setup before provider work
- **Journey/atoms:** C/L/resource; Pull budgeting. **Severity:** Performance opportunity, not measured defect.
- **Trigger:** repeated warm `membrane_context` calls.
- **Actual/source cost:** each `native_route_response_with_store` builds `NativeSourceBindings`, opens the context catalog and constructs Blueprint client (`federation_sources.rs:80-118`), then constructs a new registry/engine (`native_federation.rs:43-140`) and new Tokio runtime thread (`federation.rs:305-320`). Standalone path additionally opens Cortex DB (`72-77`).
- **Consequence:** repeated setup, allocation and SQLite/IPC identity work on warm queries; exact cost unmeasured.
- **Repair:** daemon owner should retain immutable provider registry/clients and request-local state only; preserve live grant/freshness re-observation.
- **Acceptance:** production-observable counters show warm calls do not reopen immutable owners/rebuild registry while revocation remains immediate.

### SEM-008 — generated “cross-provider budget” claim is stronger than proven wire behavior
- **Journey/atoms:** H; MEM/PUL/PSH accounting. **Severity:** Documentation/product-truth defect.
- **Trigger:** relying on generated runtime truth as proof that every receipt reconciles under one budget.
- **Expected:** generated descriptive truth follows actual production evidence/qualification.
- **Actual:** generated docs state every receipt carries reconciliation, while canons list most Pull/Push behaviors partial/unknown and the native workspace path separately partitions budget before independent child planning. Static source shows reconciliation machinery, not universal qualified behavior.
- **Consequence:** discovery documentation can be mistaken for qualification.
- **Evidence/confidence:** source/document disagreement, medium-high.
- **Repair:** phrase generated output as surface/implementation inventory and link explicit qualification status; do not assert universal behavior absent acceptance evidence.
- **Acceptance:** generated truth differentiates implemented shape from qualified journey and schema check prevents status promotion.

## Resource/duplication analysis (source-derived, not measurement)

| Scenario | Dominant work |
|---|---|
| Cold request | stdio performs filesystem identity/token reads + TCP health; daemon context canonicalizes root, opens/binds owner sources, creates federation registry, schedules all enabled providers, plans, then Push materializes/serializes. Blueprint/Ledger/Cortex I/O is provider-dependent. |
| Warm no-change | Hub proxy repeats health on process construction only; each request still reconstructs native federation/provider registry and checks release/freshness/grant. No proof of a whole-packet cache was found. |
| One-file edit | Blueprint watcher/journal/delta/generation system is present; publication completeness policy is present. Exhaustive rapid-edit/newer-pending interleavings remain unverified statically. |
| Repeated query | Repeats provider fan-out and composition; provider-local caches may reduce parse/read work, but no shared Pull result cache was proven. |
| Multi-repository | Equal token partition plus serial full federation per child; O(number of targets × per-repository federation), with deadline reset defect SEM-003. |

Concrete bounded sites include MCP payload limits, Ledger `WorkBudget`, two-second Blueprint call bounds, scheduler deadlines, Push store/item/byte/TTL caps, and Blueprint IPC pending caps. Risks requiring execution: cancellation responsiveness of child processes, rust-analyzer build-script behavior, watcher overflow recovery equivalence, and actual allocation/read/parse counts.

## Coverage and unknowns

Inspected: all seven canon tables/register headings and status rows; required architecture/generated truth; root/plugin bindings; native tool registry; installed stdio route; Hub proxy identity; native executor dispatch/authorization; Pull composition/scheduler/corrective stage; Ledger dispatch and provider registration; Blueprint IPC/service/generation-storage slices; Cortex proposal/review/store slices; Push API/recovery/selection slices; Adapt MCP inspection/feedback; diagnostics production provider wiring; Windows tray supervisor/process slices; installation/update binding slices.

Not exhaustively inspected: every one of 353 atom acceptance/evidence links; all Blueprint language extractors and every watcher interleaving; all Ledger parser/conversion/link algorithms; Cortex backup/restore/erase projections; Adapt detector corpus and effectiveness joins; macOS tray; HTTP/SDK/CLI semantic parity; updater rollback details; schema-by-schema alias/coercion inventory; actual installed CI artifact; dynamic resource measurements. These are marked `unverified_static` or `uninspected` in the matrix, not passed.

# Repair addendum: complete atom manifest and deeper terminal traces

This addendum responds to independent Completion Oracle review. The original frozen artifacts remain byte-for-byte as `*.initial.*`; this revision does not erase them. The repaired matrix contains **one explicit row for each of 353 atomic canon rows**, retaining its exact canon status, requirement text, canon line, mapped journey(s), production locus, consumer, and an honest coverage classification. A canon `DELIVERED` row is never promoted: absent fresh execution it is `source_locus_identified_unverified`; `PARTIAL`/`UNKNOWN`/`MISSING` remain correspondingly incomplete.

## E — edit → freshness → convergence, decision-bearing path

Watcher observations enter Blueprint's durable `event_journal`/`watch_state` schema (`blueprint/src/graph/store-sqlite.mjs:576-611`). The service owns bounded pending requests and cancellations (`blueprint/src/service/server.mjs:142-293`). Phase-two planning fingerprints graph file hashes, claims, verdicts and dimensions (`blueprint/src/lib/incremental-phase2.mjs:54-249`); `completePendingDocDomain` writes completion state atomically and associates it with the current generation (`blueprint/src/lib/phase2-completion.mjs:33-120`). Publication is decided by `evaluatePublicationCandidate`/`assertPublicationCandidate`, which rejects malformed/incomplete generations and unintended fact removal (`blueprint/src/graph/publication-policy.mjs:19-89`); `saveGeneration`/`adoptRebuiltGeneration` are the persistence consumers (`store-sqlite.mjs:1509-1601`). Query consumers use generation-indexed symbol/document/edge retrieval (`1493-1508,1841-1901,2188-2306`).

**Negative terminals traced:** IPC duplicate/capacity/deadline/cancel/unknown-method branches are emitted at `server.mjs:225-293`; interrupted migration has an explicit repair entry at `store-sqlite.mjs:272-323`; read-only generation paths are separated (`325-345`). **Unresolved:** static source does not prove that every watcher event kind schedules every needed phase-two consumer, nor the “older completion cannot clear newer pending” interleaving. Those atoms remain `unverified_interleaving`, not pass.

## F — branch/crash/corruption/recovery, decision-bearing path

Blueprint persistence first classifies mutable paths and repairs interrupted schema migration (`store-sqlite.mjs:67-323`), then publication validates completeness before transactional generation replacement (`publication-policy.mjs:19-89`; `store-sqlite.mjs:1509-1601`). Generation reads reject missing/incomplete envelopes through application errors (`blueprint/src/lib/application/errors.mjs:26-145`). Snapshot change computation calls `git diff --name-status -z --find-renames` and records rename/copy distinctions (`blueprint/src/graph/snapshots.mjs:175-330`). Push uses an immediate SQLite transaction, compacts expired/invalidated rows, verifies stored bytes/digest before commit, and refuses missing/expired/invalidated/corrupt/oversize resolution (`push/recovery.rs:237-352`). Cortex schema/admission effects use strict tables and transaction-bound lifecycle code (`cortex_lifecycle.rs:148-193`).

**Final consumers:** Blueprint IPC callers receive typed generation/error envelopes; Pull consumes Blueprint freshness/generation through provider admission; Push resolver returns exact selected bytes; Cortex recall reads admitted memory. **Unresolved:** complete crash-point enumeration, dirty/untracked overlay convergence, and backup/restore equivalence were not statically certified, so affected atoms explicitly remain partial/unverified.

## J — Cortex durable lifecycle, decision-bearing path

`membrane_knowledge_propose` reaches `cortex_lifecycle::propose`, which validates schema/scope/producer, DLP, epistemic class and derived identity before an `INSERT OR IGNORE` pending proposal (`cortex_lifecycle.rs:258-308`). Review authority is installation-file-owned and Ed25519-bound to installation/store/repository/scope/operation/target/hash/nonce/expiry (`39-146`); request text cannot self-authorize. Review then drives admission/control tables created at `148-193`. `membrane_memory` routes get/checkpoint-promote/recall/history/lifecycle operations through `memory_operation` (`mcp_executor.rs:1483+`) and scope-bound store consumers. Temporal record is proposal-only and resets claimed authority/veracity (`cortex_lifecycle.rs:311-345`).

**Negative terminals:** missing/malformed/revoked trust, wrong binding, unauthorized operation, bad signature, expiry, replay nonce, content/CAS mismatch, storage error, and scope mismatch are typed lifecycle errors. **Unresolved:** full erase/restore and projection-rebuild non-resurrection were not exhaustively followed across every store table and remain explicit `partial` atom rows.

## K — Adapt observed effectiveness, decision-bearing path

Production MCP only exposes scoped inspection: preferences/explain → `adapt_service::inspect_preferences`, insights → `inspect_issues`, status → `status`, and proposals currently return comparison events with `pending_proposals:null` plus an explicit unavailable reason (`mcp_executor.rs:657-693`). Feedback ingress persists an advisory record with `verified:false`; no wire verdict upgrades it (`1410-1459`). Adapt library contains distinct stages: event eligibility/candidate extraction (`membrane-adapt/src/taste.rs:229-306`), eligibility/model proposal gates (`admission.rs:300-483`), versioned detector execution (`detector_contract.rs:40-105`), issue/proposal/outcome lineage (`lineage.rs:210-518`), delivery selection (`delivery.rs:174-311`), and host/closed-loop effectiveness projection (`procedural_effectiveness.rs:236-453`). Signed/adjudicated proposal functions are separate (`proposal.rs:267-339,436-836`).

**Meaning transitions:** emitted ≠ loaded ≠ used ≠ evaluated ≠ effective; the functions preserve separate records, but no inspected production MCP call closes all joins into proven host use. The terminal production consumer for pending proposals is explicitly unavailable in the current inspect result. Therefore the mapped ADP atoms covering causal improvement and host exposure are `production_consumer_missing_or_unverified`, not delivered.

## Explicit atom coverage summary

The repaired trace matrix is authoritative for atom-by-atom coverage. Counts are generated directly from the seven canon tables and include exploratory rows. Production loci are conservative owner-level call paths, not assertions that every line implements the behavior. Rows whose committed behavior lacks a verified terminal consumer are labelled missing/partial/unverified even when helper code exists.
