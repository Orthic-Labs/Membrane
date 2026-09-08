# Membrane unified implementation & closure plan

Date: 2026-09-08. Owner: **membrane**, Sol/medium, task `01a07e9f-8f38-7463-b35e-f6b6013bb23e`. Coordinator: task `01a07b53-236f-73b0-a1c6-23679b5968e5`. This is one execution reference for the full Membrane lane, not a new competing canon or a completion receipt.

## 1. Intended outcome

Finish Membrane as one installed native context system: close its committed product/capability requirements across **Membrane integration, Pull, Push, Cortex, Blueprint, Ledger & Adapt**, finish inherited audit remediation, preserve existing semantics, qualify the real installed composition, & remove obsolete backend implementations. Blueprint migration is a major workstream inside this outcome; it does not replace Cortex closure or the other subsystem handoffs.

The final product must provide all six subsystems through canonical installed owners, with no Node/Python/shell-script dependency for Membrane backend behavior. Bounded Hub WebView presentation, supported external client bindings, declarative assets & useful development/build tooling are separate from backend implementation. Do not erase useful tooling or supported behavior merely because a file is interpreted. Do remove all proven dead/obsolete backend code, retired tests, launchers, packaging & completed migration scaffolding.

CodeRight remains a separate repository/executor. Membrane owns public infrastructure contracts; CodeRight owns its execution & production consumption. Both executors report only to coordinator. Neither may invent the other's missing facts or implement a second context/planning/storage authority.

## 2. Authority & source reconciliation

Latest explicit user requirements govern. Existing semantic canons define product behavior. Current source & exact-state runtime receipts establish implementation/delivery truth. Historical web handoffs supply claims, residuals & acceptance cases to reconcile; their embedded merge/build instructions are not commands to replay blindly.

All seven existing capability registers are included in full, not just IDs singled out below. Preserve IDs, lineage, qualification targets & exclusions. `docs/pending/README.md` remains the canonical pending index. This plan sequences work against those registers; do not create a second freely editable capability ledger.

Read in canonical Membrane checkout **`D:/Claude/membrane`**. Resolve every repository-relative path below against that root, never an inherited worker CWD or historical `D:/Claude/_lanes/*` checkout. Older lanes have different Cortex inventories & cannot override current main.

- `docs/agent-rules.md`, `AGENTS.md`, `CLAUDE.md` if present.
- `docs/architecture/membrane.md`, `docs/architecture/subsystems/{pull,push,cortex,blueprint,ledger,adapt}.md` where present.
- `docs/architecture/cross-subsystem-evidence.md`, `docs/architecture/execution-lifecycle-boundary.md`, `docs/architecture/integrations/coderight.md`.
- `docs/canon/{membrane,pull,push,cortex,blueprint,ledger,adapt}.md`, `docs/canon/README.md`, `docs/pending/README.md`.
- Relevant qualification/provenance registers, `audit/remediation/README.md`, generated runtime truth, release manifests & native-runtime policies.

Source inputs retained as traceable references:

| Input | Meaning & evidence limit |
|---|---|
| [Blueprint native Rust brief](MEMBRANE-BLUEPRINT-NATIVE-RUST-CLOSURE.md) | Full migration/cleanup requirements; proposed crate layout is optional, not architecture authority. All acceptance classes remain in scope. |
| [Ledger handoff](Codex%20handoff%20%E2%80%94%20Membrane%20Ledger%20post-merge.md) | PR #15 / merge `34a3c0ab2018bd5f95a28eeec8818466355828d8`; reported source/type checks, explicitly no Rust test binaries or installed qualification. |
| [Pull handoff](Codex%20handoff%20%E2%80%94%20Membrane%20Pull%20post-merge.md) | PR #16 / merge `5e95595f1ffcfc992620b63838a2d0d766a6e9ba`; focused composition checks, not installed/model delivery proof. |
| [Push handoff](Codex%20handoff%20%E2%80%94%20Membrane%20Push%20hardening.md) | PR #11 merge `cb0cbbcc308f345f8d6c063eb458040f1e37f8c8`; PR #14 was still open in document. Verify current integration rather than replay its historical merge procedure. |
| [Adapt handoff](ADAPT-MEMBRANE-CODEX-HANDOFF-2026-09-05.md) | PR #17 base `f612cdee804922cf59cd5b288624674492252c0a`; PR #18/#19 reconciliation described before final merge. Branch checks do not prove current main. |
| Retired task `01a07d36-99bd-7d32-804d-910dfd68929f` | Inherited audit, installer, lifecycle & qualification history; archive preserves retrieval. Source receipts outrank recollected status. |

Before each implementation wave, record actual main HEAD, dirty inventory, relevant installed generation & changed acceptance surfaces. Do not recreate deleted web branches or overwrite current unrelated changes. Verify handoff changes are ancestors or present in current source; repair only demonstrated residuals. Historical PR status here is not freshly verified remote status.

## 3. Current baseline: inventory is not completion

Coordinator inspected local main `2e8e57bbb1e368332b37466080f1e925f2f4726e`. Membrane executor reported the same main/origin-main & older installed source `13951e0917c10b147cd009a43703c06acd8da863`; revalidate at execution. Pre-existing dirty `apps/membrane-hub/src-tauri/Cargo.toml` belongs to another owner.

The current generated canon index records:

| Register | Committed capabilities | Exploratory | Competitive current-best | Lifecycle closed |
|---|---:|---:|---:|---:|
| Membrane | 66 | 0 | 12 | 0 |
| Pull | 40 | 1 | 0 | 0 |
| Push | 29 | 0 | 3 | 0 |
| Cortex | 39 | 3 | 10 | 0 |
| Blueprint | 68 | 1 | 20 | 0 |
| Ledger | 30 | 1 | 3 | 0 |
| Adapt | 68 | 7 | 10 | 0 |
| Total | 340 | 13 | 58 | 0 |

These are a snapshot of recorded canon state, not a fresh audit of 340 production behaviors. Historical handoff totals differ because later atoms landed. Competitive closure, implementation, verification, qualification & delivery are independent. Zero recorded lifecycle closure does not mean no code works; it means release-bound closure is not established by these registers. The lane must reconcile stale rows & complete actual residuals rather than blindly rewrite working systems or inflate status.

All 68 committed Blueprint rows currently say implementation `DELIVERED`, largely against JavaScript mechanisms; this does not satisfy native Rust migration. Cortex records 32 delivered & seven partial committed mechanisms; all 39 still require lifecycle qualification. Pending competitive decisions also require reconciliation against their existing evidence, not an unsolicited new donor-shopping exercise.

## 4. Non-negotiable product boundaries

| Owner | Owns | Must not acquire |
|---|---|---|
| Membrane/Pull | Final grant/eligibility/authority/freshness/sufficiency/fusion/attention/publication policy & receipts | A second competing planner in any subsystem or host |
| Blueprint | Repository/source identity, graph, generations, freshness, claims, traversal, re-anchoring & repository evidence | Cortex durable-memory authority or host execution |
| Cortex | Durable knowledge admission/storage, provenance, conflict/supersession, temporal/lifecycle & memory retrieval | A separate resident service identity or final cross-provider planner |
| Ledger | Document registration/navigation/search/link projections & exact registered-source resolution | Document truth, durable facts or final admission |
| Push | Faithful reversible representations, exact originals/recovery & local measurement | Evidence admission/ranking, a second final budget owner |
| Adapt | Governed learning/proposals, evidence-bound diagnostics & effectiveness | Direct durable-truth writes, invented host telemetry or blocking authority |
| CodeRight | Execution, host observations/acknowledgements & consumption of public Membrane contracts | Membrane implementation, duplicate memory/context/ranking |

Shared native address space does not dissolve ownership: callers use Blueprint APIs, never open Blueprint SQLite themselves; Blueprint never opens Cortex durable storage. Reuse existing libraries/services; do not create crates, daemons or frameworks merely for symmetry.

Installed-only discovery must adopt compatible installer-owned stable `current`, install only on proven absence, & update/repair known incompatible same installation through canonical installer transaction. Offline/denied/timeout/corrupt-or-rotation is not absence. Preserve installation identity, serialize transitions, refresh startup generation & rediscover after cutover. Never select development checkout, CWD, PATH guess, staging or copied developer binary.

| Hub holder | CodeRight daemon holder | Required installed behavior |
|---|---|---|
| Off | Off | All six subsystems support bounded explicit operations; no automatic resident workers left behind |
| On | Off | Full background services/watchers |
| Off | On | Same full background services/watchers; Hub UI optional |
| On | On | Same controller & storage owners; losing one holder preserves the other |

Final holder release/crash/expiry drains residents. Manual Blueprint refresh/build/query remains callable at any time. Watchers refresh actual source edits/additions/deletions automatically & reconcile event loss; watcher silence never implies freshness. Inline CodeRight hosting is not automatically a CodeRight daemon holder.

Older handoff phrases such as “daemon-only CLI” or “tray-owned only” describe previous topology. Current execution-lifecycle canon supersedes that restriction while preserving one canonical subsystem owner. Likewise old Node SDK/server references require native replacement of backend semantics, not retention of an interpreter runtime.

## 5. Workstreams & exact residuals

### A. Membrane integration canon & inherited audit

Reconcile every committed `MEM-*` row, including unknown/partial production consumption & host contracts. Finish inherited audit findings from their exact acceptance records: SEM-003 deadline propagation through synchronous Ledger, skill snapshot & owner preflight; SEM-004 Push task/session binding; SEM-005 diagnostics persistence failure visibility; SEM-006 readiness; SEM-007 warm-owner reuse; SEM-008 budget claims; EVID-001/002 provenance & real oracle assertions. These are inherited reported residuals to verify, not new invented feature IDs.

Preserve checkpoint `2e8e57bb` deadline/fanout repairs. Requalify its final-source late-target regression where prior test compiled earlier source. Implement canonical provisioning/update-repair & additive holder lifecycle as public Membrane capabilities. SDK adoption, explicit Hub-off operation, generation/identity fencing & shared residency need production consumers, not callback-only test abstractions.

### B. Cortex: all 39 committed capabilities

First reconcile newer Cortex receipts/production callsites against stale canon rows. Seven currently partial rows identify priority implementation seams:

| IDs | Required residual investigation & closure |
|---|---|
| CTX-001 | One canonical SQLite authority, migrations/WAL & actual ownership through callers. Prior withdrawn promotion must not be repeated. |
| CTX-004 | Full identity/provenance/authority/sensitivity/time/lifecycle/supersession/derivation, with unknown fields explicit. |
| CTX-010 | Versioned archive-first lifecycle; time triggers review, never upgrades authority. |
| CTX-017 | Only resolved evidence-bound supports/contradicts/supersedes/derived-from relations become traversable. |
| CTX-023 | Proposal-first semantic Dream review, recoverable parents & governed admission. |
| CTX-024 | Bounded episodic proposals only for cursor ranges not already covered by authoritative foreground memory. |
| CTX-031 | Immutable experiments & promotion receipts bound to trusted controlled evidence; real host evidence remains cross-owner. |

Qualify all already-delivered mechanisms through native installed consumers: admission/DLP/utility, atomic batch/idempotency, duplicate/conflict/quarantine/supersession, temporal recall, lexical/vector/fusion, resolver previews, feedback/unknown outcomes, checkpoints/proposals/review/crash recovery, erasure, backup/restore, exports/imports, projection rebuild, diagnostics, events/skills, completeness, CTX-040 versioned recipes & CTX-041 reversible recall suppression. Preserve evidence across restart, replay & restore. A failed index is not an empty successful recall; database content remains authority.

Reuse `docs/provenance/foundation/2026-09-06-cortex-remaining/` & current later receipts. Verify freshness before reuse. Do not reimplement a mechanism merely because its row is stale. CTX-033, CTX-039 & CTX-042 remain exploratory; ordinary Ledger/Pull integration does not promote them.

### C. Blueprint: all 68 committed capabilities plus native migration

Port full existing product semantics, not a reduced Rust grep tool. Map every production module, adapter, state owner & test to canonical capability IDs, native destination, parity fixture & deletion gate. Dead code gets a deletion disposition, not a port. Preserve stable IDs; BPT-022/BPT-045 historical splits are not missing capabilities to recreate. BPT-048 remains exploratory.

Native slices cover:

1. Typed contracts, root/repository/worktree identity, deterministic source accounting & grants.
2. Existing SQLite schema/storage semantics, currently reported schema v20: migrations/backups/WAL, one writer, immutable generation publication, last-known-good, rollback & real pre-port DB compatibility.
3. File/document discovery, lexical fallback, pinned native Tree-sitter registry, optional compiler/SCIP paths, provider identity/checksum/license/capability/isolation. Preserve `COMPILER > AST > LEXICAL` & confidence ladder; grammar changes invalidate fingerprints.
4. Graph construction, import/framework/SQL/Terraform/cross-language evidence, stable entities/rename aliases, source hashes & categorical versus heuristic provenance.
5. BM25/identifier ranking, seed resolution, named traversal policies, complete atomic evidence paths, bounded search/neighborhood/path/impact/liveness/test/risk/snapshot operations. Zero inbound edges cannot prove dead code.
6. Generation/source freshness, dependency invalidation, dirty/untracked overlays, full/incremental equivalence, native watcher & event-loss/overflow reconciliation. Test edits during build/query, rename/delete/add, ignore rules, branch/reset, rapid saves, crash/restart, DB contention & concurrent publication.
7. Phase2 discovery/claims/fingerprints/verification plans/seals/verdict reuse/contradictions/history, generated architecture/flow projections & findings/SARIF/explain/evidence packs.
8. CLI/MCP/native SDK/service/error-envelope parity, federation provenance, diagnostics/repair/redacted support bundles, Explorer confinement/authentication/read-only behavior, update/rollback contracts through canonical installation.

Default is Rust libraries under existing Membrane runtime; a separate native process requires a demonstrated isolation/lifecycle reason. Preserve supported external command names via native compatibility surfaces where necessary. Supported JS client SDK may remain outside installed backend; it must own no backend semantics. Keep bounded Hub presentation separate. Reconcile legacy standalone Blueprint updater/Explorer rows to integrated native delivery without deleting their supported security/rollback behavior.

Before deleting each JS authority, freeze representative outputs & DB fixtures, execute Rust differential parity, inspect deviations & obtain independent acceptance. Retain only explicitly gate-bound temporary migration oracles; delete them when their gates close. Final packaging removes bundled Node, Blueprint npm runtime/modules, interpreter launchers/handshakes, sealed interpreter exemptions, stale SBOM/policy/docs/generated paths. Backend qualification must run without Node/Python; useful frontend/build tooling is not a backend exemption.

Measure old versus native cold/incremental build, query/path/impact/neighborhood/freshness, watcher latency, DB/startup, memory & package size; record meaningful p50/p95. Investigate material regressions. tgrep is optional post-parity acceleration, never a prerequisite or replacement for graph semantics. Stale indexes cannot yield false no-match.

### D. Ledger: merged source is not automatic-delivery qualification

Reconcile all 30 committed `LDG-*` rows. Preserve PR #15 owner-native service, grant `read_paths` propagation (empty means no range), distinct source identity versus identical content, tombstone recreation, complete indexing beyond presentation paging, shared Comrak projection, stable IDs/ambiguity, exact raw/normalized/span/generation resolution & durable erasure exclusion.

Close installed path: normal harness request → canonical Ledger owner → grant-filtered exact/FTS/literal/graph candidates → Pull admission → registered-source resolver → actual delivered evidence & receipt. Test revoked/expired grants, stale revisions/hashes/spans, moves/deletes/exclusions, cursor replay, concurrent source change, cross-root attempts, cancellation/deadlines & work/byte budgets. Ignore policy never substitutes for authorization; backlink counts do not grant authority; drift does not automatically falsify facts.

Automatic Ledger publication has an intentionally empty delivery-qualification allowlist in the handoff. Inspect current gate; add only an immutable exact-release qualification receipt after full composition passes. Never enable it simply because compilation succeeds. LDG-028 conversion qualification is per format: original bytes → converter/version/config → normalized projection → index/search → exact resolver/provenance/loss. Unsupported formats remain unadvertised. LDG-029 backlinks, LDG-030 literal source spans & LDG-031 drift need their own acceptance, not only old FTS benchmarks. Keep LDG-023 exploratory.

Remove/generalize stale branch-only Ledger workflow & operational branch wording if still present; preserve immutable historical receipts. Do not redesign FTS5 or add a second document authority absent measured need.

### E. Pull: final admission & actual host delivery

Reconcile all 40 committed `PUL-*` rows. Prioritize PUL-037 suppression, PUL-039 previous-packet reuse & PUL-040 placement, whose historical rows may still say missing despite merged mechanisms; inspect before repairing. PUL-041 workspace & PUL-042 resolver negotiation need current production & installed evidence.

Preserve distinct task text/task ID/session ID/request ID/repository/scope, real host H8 capacity, independently authorized workspace children, one aggregate attention ceiling, target-specific omissions & deterministic allocation. Resolver eligibility is intersection of host-negotiated & runtime-owned callable capabilities; unknown names confer nothing. Use faithful inline fallback or typed refusal when resolution unavailable. Preserve Blueprint atomic paths & generation provenance. Revalidate grants/policy/source/generation/resolvers/capacity immediately before publication.

Qualify real harness acquisition → eligibility/fusion/coverage → bounded corrective retrieval → Push-selected sole body → resolver follow-up → final model attachment & receipt. Exercise all grant/policy/generation/resolver/deadline/cancellation/capacity changes, unauthorized siblings/outside roots, mixed availability, aggregate pressure, empty/insufficient evidence & false resolver claims. Suppression requires explicit current host retention, restores eligibility on change/expiry/unknown/refresh & cannot suppress protected evidence. Prefix reuse changes neither membership nor authority. Placement only orders admitted evidence.

Keep execution health, evidence sufficiency & delivery outcome distinct; coverage includes partial/missing/contradictory/stale/unsafe/unavailable/not-evaluated with reasons. Measure token/cache/latency effectiveness without false suppression. Preserve one Ledger provider & exact selected-body accounting. Keep PUL-034 exploratory; parent/summary experiments do not become new committed scope. Fix stale branch/provenance wording without rebinding evidence to unreachable history.

### F. Push: recovery hardening & measured final wire

Reconcile all 29 committed `PSH-*` rows, especially PSH-002/005/025/028/029. Verify PR #11/#14 hardening is present on actual main; do not recreate old branches. Preserve scoped SQLite exact-original store, opaque handle generation separate from content digest, additive old-store migration, bounded tombstones, 4096-row expiry reclamation, resolver-proof reuse/rotation, explicit expiry/invalidation/CAS renewal & no renewal on reads/duplicate publication.

Preserve exact byte/line/JSON selectors & duplicate-key refusal, exact/exempt monotonicity, immutable-original protected-span validation, AST code projection with exact fallback, positive net savings after final serialization, H8 sizing-basis honesty & content-free telemetry. Large prepare ingress must honor schema after worst-case escaping: handoff specifies one-megachar text & Push-specific 8 MiB serialized ceiling, without widening normal routes. Port remaining JS transport semantics natively before removing their tests.

Qualify real installed tool/log/code/document delivery, passthrough/reduction, model-facing single-body accounting, visible recovery handle, actual resolver follow-up & receipt join. Negative cases: missing negotiation, wrong scope/store/root, expired/invalidated/republished old handle, malformed handle, corruption, over-budget prepare/restore/envelope, shrinking host capacity, deadline & cancellation. Verify Windows/macOS migration, permissions, compaction, concurrency, restart & disk/resource failure independently. Governed command execution remains argv-safe; no hidden shell expansion or recoverability claim without exact original.

Measure fidelity/protected errors/negation/numbers, tokens, CPU/memory/latency, recovery rate & actual outcomes where observable. Query-aware reduction needs planner-bound identity/scope/authority/freshness & held-out evidence; task prose alone is insufficient. Remove stale Push branch workflows if present. Never rewrite immutable PSH-003 historical locator receipts to silence checker: use evidence-preserving supersession, & never waive unrelated failures.

### G. Adapt: finish owned semantics & separate host/empirical gaps

Reconcile all 68 committed `ADP-*` rows, including later ADP-074..077 absent from older handoff. Verify PR #17/#18 reconciliation against actual main; pre-merge checks are not current qualification. Preserve user-authoritative Taste evidence, failure applicability, versioned detector catalog/hard negatives, exact procedural-content separation, persisted coverage & typed unavailable when required host facts are missing.

Specific residuals: ADP-041 persisted production multiwriter convergence under competing requests/retry/crash with retained conflicts through Cortex/admission, not a new Adapt DB; ADP-072 independently authenticated human/adjudicator transport before clarification answer, not a caller-provided enum; ADP-038/040 exact detector→episode/exposure→evaluator/outcome joins; ADP-036 exact host-loaded representation before effectiveness claims. Preserve ADP-073 target/version exclusion & idempotent plan conflict behavior. Reconcile all efficiency ADP-043..064 individually; a detector name is not a working detector.

Later committed rows remain explicit acceptance targets: ADP-074 negotiated scope-bound read-only agent inspection without approval/exposure side effects; ADP-075 readiness from actual producer/consumer bindings with distinct empty/unavailable/blocked/missing-join states; ADP-076 bounded version-bound comparison from host-run baseline/variant outcomes without admission/activation authority; ADP-077 evidence-bound guard-stage eligibility without host blocking/scope-expansion authority.

CodeRight owns its observation, acknowledgement, evaluator & experiment producers. Coordinator assigns explicit host dependencies there. Membrane must report absent H4/H6/H9/H10 facts honestly, never widen contracts solely to turn unavailable into ran. Real held-out cohorts & causal effectiveness require empirical evidence. Guard eligibility is not host authorization; proposals never self-approve; collect no private reasoning. Keep ADP-065..071 exploratory/HOLD. Remove stale operational workflow/branch wording without erasing evidence.

## 6. Execution: parallelize independent work, serialize delivery

Membrane Sol retains design/integration ownership. Use Luna subagents for every genuinely independent bounded lane, with exact product target, edit allowlist, exclusions, dependencies, acceptance & durable handback. No arbitrary agent count, manufactured tasks or duplicate owners. No unnecessary architecture rewrite or process-file framework.

Approved lead roster:

| Task | Scope |
|---|---|
| `01a07e9f-8f38-7463-b35e-f6b6013bb23e` | Membrane repository integration, managed builds, delivery & exact shared-file coordination |
| `01a07ec1-8c88-77e1-8b15-6cad49875579` | Blueprint native Rust |
| `01a07ec1-9021-7d93-9e43-226a044fe9f3` | Cortex & Adapt closure |
| `01a07ec1-9496-75c3-9f4e-7b45eb39fc28` | Pull, Push & Ledger closure |
| `01a07e9f-1f88-7861-90a8-fc8ef04cb4b4` | CodeRight daemon consumption in separate repository |

Domain leads own only coordinator-approved exact files & report through coordinator task `01a07b53-236f-73b0-a1c6-23679b5968e5`. Membrane integration owner alone changes repository HEAD/index, shared manifests, managed build state, release composition & final delivery. Leads consume existing inventories instead of rerunning them.

Initial concurrent lanes can cover Cortex residual/source qualification, Blueprint native core/parity mapping, Ledger post-merge reconciliation, Pull qualification/reconciliation, Push qualification/reconciliation, Adapt residuals, & installed lifecycle/inherited audit. Split Blueprint parsing/query/fixtures from storage once native interfaces are frozen. Shared `membrane-runtime`, Cargo manifests, generated canon truth & packaging are integration-owned touchpoints; queue those edits or hand back patches instead of racing.

Dependency order is local, not a global serial lock:

1. Reconcile current evidence & freeze bounded slice contracts; preserve ongoing work.
2. Implement independent Cortex/other-axis residuals alongside Blueprint native core & public lifecycle contracts.
3. Integrate native Blueprint storage → indexing/query → freshness/watch → public adapters; each slice carries parity & deletion gates.
4. Qualify individual owner paths early; qualify cross-subsystem composition after affected contracts stabilize.
5. Perform final native cutover/package cleanup, installed upgrade/restart & full release-bound acceptance on exact artifact.

Only integration owner stages/commits/pushes. One managed native build root per host coordinated through lead; parallel source work continues during builds. Public qualification/release uses existing RightKit/GitHub capabilities; internal unsigned Windows installer is explicitly authorized through declared RightKit development lane. No ad-hoc build machinery, direct Cargo, public upload of internal test artifacts or invented signing prerequisite. Read current release/access rules before corresponding effects.

15-minute heartbeat inspects bounded deltas once, checks real Luna lane concurrency, compares evidence to full scope & stays quiet when unchanged. No polling loop, repeated rebuild after observation timeout, or status-only busywork. Preserve exact build handle. Reproduce available live failures before spending diagnostic builds. Stop/revocation overrides continuation.

## 7. Common qualification & completion gates

Each committed atom must map requirement → production owner/symbol → live consumer → acceptance → exact source/artifact receipt → declared delivery boundary. Reconcile competitive comparison separately. Neither a passing unit test, merged branch, newly authored test nor generated counter alone closes a release-bound atom.

Minimum composed qualification covers:

- Both supported installed platforms where required; clean install, upgrade from existing databases, rollback/restart, relevant uninstall behavior & no development binding.
- All four holder states, holder crash/expiry/race, one controller/store, manual operations & automatic source-refresh correctness.
- Native MCP plus native SDK/CodeRight consumers independently; one path's success does not prove another.
- Real source/grant/provider acquisition, Pull policy & coverage, Push measured selection, exact resolver invocation, model-facing attachment & receipt identity/byte/hash conservation.
- Cortex durable integrity/backup/restore/recall, Ledger exact registered resolution/conversion, Blueprint generation parity/freshness/Phase2, Adapt honest evidence/host joins.
- Negative authorization, redaction, malformed/corrupt input/DB, huge files/repos, symlink/root escape, unsupported parser, deadline/cancellation, source/policy changes at publication & resource exhaustion.
- Native workspace build/test/relevant feature/release checks through sanctioned tooling, focused tests first; no interpreter dependency for backend qualification. Backend artifact exercise with Node/npm/npx/Python/pip/pnpm absent from PATH; inspect bundled files/SBOM & full process trees so a bundled interpreter cannot evade the PATH check.
- Old-versus-native differential results, meaningful performance/resource/package-size measurements & no unreachable legacy backend entrypoint.

Do not delete migration oracles until their corresponding parity acceptance passes; do delete them when no open gate consumes them. Do not delete unrelated user work. Regenerate current docs/policy/indexes from their owners; immutable history stays immutable. Existing known provenance defect gets a superseding record, not a forged green receipt.

A Membrane-owned source milestone may be reported while real-host/empirical qualification is pending, but those rows remain open. Escalate concrete cross-owner requirements through coordinator & continue independent work. Do not declare full Membrane complete by relabeling remaining requirements external, exploratory, or optional.

## 8. Required handback & reference maintenance

Membrane executor adopts this plan as scope reference, reconciles its active goal to all seven canon registers & maps existing Luna work without restarting completed inventory. Return first bounded wave with ownership/dependency reasons; maintain progress in existing canon/pending/evidence files, not a duplicate status database.

At each delivered slice report changed files, capability IDs, behavior, exact tests/receipts, source SHA, committed/pushed/installed state & residuals. At overall closure deliver one report covering requested/evaluated/unresolved/excluded scope, all seven canon rollups, ported/deleted modules/tests, every remaining interpreted file's justified category, runtime-language/invocation/SBOM/process evidence, DB compatibility, parity, performance/package delta & exact qualification commands/artifacts.

Independent completion review must inspect actual consumers & acceptance before promotion. Coordinator checks original user scope, not merely latest packet. This document does not itself certify any implementation, release or competitive closure.
