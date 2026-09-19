# COLD-START HANDOFF: Membrane implementation, qualification & ownership

## 0. Handoff Control

- **Handoff ID:** membrane-ownership-2026-09-20
- **Created:** 2026-09-20, Asia/Calcutta; live state checked during authoring.
- **Source task / chat:** Current Codex Membrane reconciliation conversation; exact runtime task ID not exposed in this packet.
- **Target:** Adrian's fresh Codex session in D:/Claude/membrane.
- **Author:** Codex lead taking ownership from Devin.
- **Receiver role:** Continuation & implementation owner, with evidence-based review responsibilities.
- **Proceed mode:** IMMEDIATE
- **Readiness:** READY_WITH_GAPS
- **Handoff reason:** User requested full context for a fresh session, then transferred implementation ownership & requested commit/push.
- **Source evidence mode:** LIVE_CONTEXT
- **Transcript evidence path:** NOT_APPLICABLE: direct full-summary request; authored from live conversation, supplied reports & local state checks.
- **Source prefix receipt:** NOT_APPLICABLE: no transcript ingest or frozen JSONL prefix claimed. Boundary includes user instruction “no, devin aint doing shit. it's yours now” & ensuing commit/push.
- **Packet path:** D:/Claude/membrane/tasks/handoffs/2026-09-20/membrane-ownership.md
- **Receipt path:** D:/Claude/membrane/tasks/handoffs/2026-09-20/membrane-ownership.receipt.json

## 1. Intent & Mission

- **Original user intent verbatim:** “fix every atom in canons based on my decisions so it doesn't regress.” “not just recommendations, I want you to amend and this and that for the canon to reflect final shape”. “work is either required or not. I didn't ask you to defer anything”. Latest transfer: “give me a full summary, i'm starting a new session. full context. decisions. progress, what's next”; “why not commit and push to origin?”; “no, devin aint doing shit. it's yours now”.
- **Underlying goal:** Implement all required Membrane capabilities in accepted final shape, qualify actual production consumers, accurately close atoms & deliver through main/origin. No shrinking scope to declare success.
- **Current objective:** Codex now owns implementation, integration, evidence & delivery. Preserve completed work; finish behavioral gaps & remaining required qualification.
- **Definition of success:** Every applicable COMMITTED atom meets its own focused verification, actual-host/platform/installed/released acceptance & delivery boundary. Final host flow installs/enables one correct projection, adopts one engine, receives useful bounded context, resolves exact evidence, pushes immutable memory & drains after final holder loss.
- **Out of scope:** EXCLUDED atoms, retired peer Push, speculative new subsystem work, unrelated CodeRight shell fixes, Arcane infrastructure repairs unless necessary for requested outcome. No deferred scope.
- **First responsibility:** Verify main/origin state & read implementation contract, then inspect remaining production failures before choosing repair.
- **Must not do first:** Start another installer build blindly, recreate working mechanisms, reopen accepted decisions, or interpret 0 lifecycle closure as 0 implementation.

## 2. Current State

- **Phase:** Implementation is substantial; qualification incomplete. Canon index says 299 committed, 54 excluded, 353 identities, 0 lifecycle-closed, 53 competitive current-best. Competitive count is not lifecycle closure.
- **Completed:** Source decisions/canon reconciliation; Ledger FTS & skill composition have reported installed evidence; actual Codex hook insertion has reported evidence. This author reviewed eight-file diff, ran onboarding 8/8 & managed test:mcp successfully, committed inherited fix as 45bbf19f, pushed it plus 44 earlier commits.
- **In progress:** Full host/lifecycle reliability, canonical installer/cache reconciliation, qualification of all committed capabilities. Latest Codex implementation is now committed, not an active Devin dirty lane.
- **Blocked:** Devin reported macOS lane unavailable & some engine unit commands denied. These are reported environment limitations, not verified universal blockers: this author successfully ran managed membrane-mcp tests. Use actual policy/tool evidence per command.
- **Not started:** Exact remaining atom-by-atom implementation distribution was not recomputed; inspect current canons rather than inventing a percentage or asserting every open atom needs new code.
- **Last action:** git push origin main, then # Verify current checkout state
git status --short --branch & git rev-parse HEAD.
- **Last observed result:** Push ff0e9752..45bbf19f succeeded; main equals origin/main; working tree clean before this handoff artifact.
- **Active goal / plan:** Continue required acceptance after delivery of summary. No create_goal tool objective was established.
- **Current hypothesis:** Codex hook compatibility repairs are useful but installer/drain & latency reliability remain incomplete. Shared-engine adoption & auth registration need evidence against accepted boundaries.

### Progress chronology — distinguish reports from independent verification

1. Initial inventory was 341 committed/0 closed; intermediate atom reconciliation at 3715589b reported 339. Final accepted scope reconciliation at d3a61693 yielded 299 COMMITTED + 54 EXCLUDED. Never use 341 or 339 as current denominator.
2. User sent a fresh Devin SWE2 agent to finish all pending atoms & MCP work. This Codex session mostly reviewed its reports & directed corrections. Devin no longer owns work after explicit user transfer.
3. Windows qualification exposed real issues: Blueprint deep cloning/index reconstruction, oversized result retry storms, supervisor failure handling, degraded watcher cadence, tray drain ordering, missing membrane_ledger registration, CLI stdout capture. Repairs produced real progress but did not close all atoms.
4. f4b01cbc introduced 5-second provider preflight into a 600ms SessionStart request. This permanently skipped Ledger in that request. We rejected treating a Cortex-only successful packet as final integration. Time budgets are request deadlines, separate from token budgets; neither 600ms nor 5s is an architectural requirement.
5. Retrieval initially synchronized all Ledger documents before searching; Blueprint freshness performed live worktree fingerprinting. Blueprint status did not build the graph, despite stale comments. Authorized resident startup uses Build dispatch, which takes incremental refresh for valid graphs, construction only missing/proven unrecoverable.
6. Ledger maintenance moved to its own thread after starving Blueprint supervisor for minutes. A remaining global operation mutex still starved reads, while legacy_scan recall exceeded 29s. Moving work to another thread alone was insufficient.
7. Ledger follow-up: reported run12 passed 8 gates, FTS MRR 0.607 vs legacy 0.051, recall@5 74.5% vs 8.5%, max latency 1922ms. Receipt a796a687 pinned; activation auto-reconciles qualified FTS. Installed recall ~2.2s, ~1.86s with foreign writer. Those timings/metrics are supplied reports, not independently rerun here.
8. 782633a6 fixed null resolver optional fields; verbatim emitted membrane_source_read arguments replayed successfully. Reported LDG-011/012/017/018/019/032 native installed probes passed. Skill-targeted Pull admitted two skill blocks & deduplicated same-source document blocks. ff0e9752 records lane evidence; 2d60fd3d repairs missing cli prefix in qualification dispatch.
9. Latest report (2026-09-20) proved real codex exec SessionStart/UserPromptSubmit/Stop completion & orientation in rollout on installed generation e43471dd. Fixed hook discovery/schema parsing, Windows shell invocation, strict hook response fields, held-open stdin, cached-client installed activation/token lookup & Codex bearer binding.
10. Same report also recorded intermittent SessionStart failure, installer daemon drain timeout, manual cache binary/script copying, 2 install-release test failures & unrelated Arcane hook failures. A subsequent green Codex run does not erase these gaps.
11. This author pushed 44 existing commits to ff0e9752, then reviewed/committed eight inherited files as 45bbf19ffc0d545c8cafdb661400e52819508d76 & pushed main. No atom closure was added. No installed acceptance rerun was claimed.

## 3. Environment & Active Work

- **Work type:** MIXED
- **Workspace / repo:** D:/Claude/membrane; GitHub https://github.com/Orthic-Labs/Membrane.
- **Branch / version:** main only; origin/main only remote head checked. Installed reports use 0.1.24 with multiple generations; version string alone does not identify tested bytes.
- **Baseline revision:** 45bbf19ffc0d545c8cafdb661400e52819508d76, pushed this turn. Packet commit will follow this code baseline.
- **Dirty state:** # Verify current checkout state
git status --short --branch returned only main...origin/main before packet creation. Eight inherited files now belong to 45bbf19f.
- **OS / shell:** Windows, PowerShell 7.6; timezone Asia/Calcutta.
- **Tools / dependencies:** packageManager pnpm@11.24.0; use nearest manifest. Rust always managed RightKit. Read D:/Claude/docs/rules/rightkit.md & release-signing.md before build/release work. Local unsigned exception in .rightkit-local-development.json.
- **Services / processes:** Targeted Get-Process membrane*,rightkit*,cargo,rustc returned no processes at handoff preparation. Managed MCP test completed afterward. Recheck before runtime work; not an exhaustive service inventory.
- **Agents / tasks / threads:** User explicitly removed Devin ownership. Runtime context lists old assess_adapt/assess_blueprint/assess_knowledge/canon_guard/membrane_atoms/other_canons/plugin_activation_review/shape_adapt_blueprint names; their live activity was not verified. Do not assume workers own files.
- **Scheduled work:** No automation created by this session. External scheduled activity not inventoried; inspect only if competing runtime/Git work appears.
- **Credentials / access:** Git push auth is already provisioned; ordinary push succeeded. MEMBRANE_BEARER_TOKEN is a name only; never expose credential contents. Read github-access.md before remote writes.

### Files preserved in 45bbf19f

- apps/membrane-hub/scripts/package-portable-windows.mjs
- apps/membrane-hub/scripts/release-build-candidate-windows.mjs
- engine/crates/membrane-runtime/src/mcp_executor.rs
- engine/crates/membrane/src/activation.rs
- engine/crates/membrane/src/bin/membrane-client.rs
- hooks/codex-hooks.json
- plugin.json
- tests/onboarding/getting-started-consistency.test.mjs

## 4. Decisions, Invariants & User Corrections

All following decisions are LOCKED by accepted ADR & implementation contract; reopen only on explicit user change. Implementation choices may change to satisfy them.

| ID | Decision / invariant / correction | Source / why | Status | Reopen only when |
|---|---|---|---|---|
| DECISION-1 | Membrane is parent with five subsystems: Pull, Blueprint, Cortex, Ledger, Adapt. Former Push survives only history/compatibility; public push is memory write. | Accepted ADR decisions 17–19 | LOCKED | User explicitly changes scope |
| DECISION-2 | One planner owns grants, eligibility, authority/freshness, sufficiency, fusion, admission, reduction, publication, recovery & receipts. Provider scores/caps are proposals. | Implementation contract | LOCKED | User explicitly changes ownership |
| DECISION-3 | Blueprint full construction only absent/proven corrupt unrecoverable graph. Valid updates incremental; incompatible/newer graphs preserved/migrated/typed-rejected. All query/status/freshness/recall reads nonmutating. | ADR 1–6; BPT-021/056/072 | LOCKED | User explicitly changes decision |
| DECISION-4 | Auto Blueprint maintenance only active eligible Hub/CodeRight holder; explicit user build/refresh works Hub-off. Enrollment is not authorization. | Canon BPT-043 | LOCKED | User explicitly changes authority |
| DECISION-5 | One resident engine per OS user/canonical installed state. Hub or accessing harness starts/adopts/holds it. Final holder loss drains/stops; other owners survive partial release. No alternate runtime/store/port, one-shot direct-store fallback or ownerless restart. | ADR 20–24 | LOCKED | User explicitly changes lifetime |
| DECISION-6 | SessionStart invokes thin installed client, acquires holder & requests bounded orientation through shared Pull; host inserts response. Never builds graph/dumps stores/bypasses grants; deduplicate event identity. | MEM-067; pluginmcp section 9 | LOCKED | User explicitly changes contract |
| DECISION-7 | Ledger owns source-bound document & portable skill projections, index/invalidation & exact hash-bound resolution. Source files authoritative. Direct Pull provider; no ordinary body replication into Cortex. | LDG-022/032; ADR 11–12 | LOCKED | User explicitly changes ownership |
| DECISION-8 | Cortex owns durable admission/conflicts/lifecycle/recall. Public push preserves submitted body byte-for-byte in immutable source/admission record; embeddings/derived forms cannot replace it. | CTX-043; MEM-068 | LOCKED | User explicitly changes write semantics |
| DECISION-9 | Adapt produces evidence-bound proposals/reviewed Taste & Insights through Cortex, never direct Pull injection or durable truth writes. Keep useful mining/review/lineage/status, not speculative detector expansion. | ADR 13–14; ADP-029/035 | LOCKED | User explicitly changes scope |
| DECISION-10 | Bounded-response needs explicit bounded response budget, reports host capacity unknown when absent. Host-fit needs trusted fresh exact identity-matched H8; invalid evidence refuses without silent downgrade. Account full rendered envelope, protected spans & recovery. | PUL-027/028/031/050/051 | LOCKED | User explicitly changes budget contract |
| DECISION-11 | One qualified fusion default; retire reserved memory/skills lanes from normal path after acceptance. Do not force every provider into every packet, but prove each relevant eligible provider in suitable fixtures. | PUL-022/026; final contract | LOCKED | User explicitly changes policy |
| DECISION-12 | Codex/Claude native plugin & authenticated HTTP; generic/other host projections use thin stable client where required. Each committed host independently proves discovery/invocation/context. Devin proof cannot be inferred from Windsurf. | MEM-045–050/067/069 | LOCKED | Explicit decision or supported compatibility projection satisfying all boundaries |
| DECISION-13 | Canonical installer owns payload/trust/current/projections/update/rollback. Blueprint owns graph compatibility, not self-update. No manual cache surgery as delivered install mechanism; no duplicate MCP registration. | MEM-057–060; pluginmcp | LOCKED | User explicitly changes installer contract |
| DECISION-14 | Scope binary COMMITTED/EXCLUDED, no deferred queue. LDG-023 excluded; CTX-034 historical alias of LDG-032; all 29 PSH IDs preserved as historical migration aliases. | 2026-09-16 amendment | LOCKED | Explicit user scope change |
| DECISION-15 | Use main only; remove other branches/worktrees without losing unique work. User request is approval for in-scope actions; no repetitive permission questions. Minimal prose, use &, ETA only agentic critical-path wall clock, no ranges/human days. | Live user instructions | LOCKED | User explicitly changes instructions |

## 5. Artifacts & Evidence

| Artifact | Path / URL | Role | State | Version / hash | Validation + last checked |
|---|---|---|---|---|---|
| Final scope | D:/Claude/membrane/docs/canon/implementation-contract.md | Read first; supersedes conflicting older plans | Accepted | 2026-09-16 amendment | Read this turn |
| Accepted decisions | D:/Claude/membrane/docs/architecture/adr/2026-09-12-context-system-decisions.md | Decisions 1–25 & scope amendment | Accepted | Contains final-shape amendment | Read during source conversation |
| Plugin target | D:/Claude/membrane/pluginmcp.md | Host/MCP/lifetime acceptance | Target, not runtime proof | Historical known-defect/scope lists can be stale | Apply final contract precedence |
| Atom state | D:/Claude/membrane/docs/canon/ | Six active canons & retired Push history | Required evidence ledger | 299 committed / 54 excluded | Current pending index checked |
| Pending index | D:/Claude/membrane/docs/pending/README.md | Sole generated pending index | 0 lifecycle closed | 353 identities | Read this turn |
| Ledger lane | D:/Claude/membrane/docs/provenance/foundation/2026-09-18-ledger-composition-qual/ | Installed source/resolver/skill evidence | Reported native passes, not lifecycle closure | ff0e9752 source; installed 782633a6 | Inspect exact receipts before claiming qualification |
| First report | C:/Users/adrds/.codex/attachments/1489e072-833e-4632-b318-777f260b1215/pasted-text.txt | Maintenance starvation & initial hook progress | Supplied transcript | Older report | Read in source conversation |
| Ledger report | C:/Users/adrds/.codex/attachments/9006c069-d928-49e4-95ed-81bb4d3a79d9/Pasted text.txt | FTS/skill/replay progress | Supplied transcript | 782633a6 installed / ff0e9752 evidence | Read in source conversation |
| Latest report | C:/Users/adrds/.codex/attachments/72c462f3-9e79-4756-91d0-35966637d3ff/Pasted text.txt | Actual Codex hook progress & unresolved failures | Supplied transcript | e43471dd installed | Read in source conversation |
| Delivered fix | https://github.com/Orthic-Labs/Membrane/commit/45bbf19ffc0d545c8cafdb661400e52819508d76 | Eight inherited files committed & pushed | Delivered to origin, not released | 45bbf19f | Onboarding 8/8, managed MCP suite exit 0, git diff --check clean |

CodeRight report attachment 9af2b0a1-7cb8-43b6-a1e6-c04aeccd9e70 was explicitly withdrawn as wrong conversation. Ignore it entirely for Membrane status.

## 6. Failures, Dead Ends & Attempts

| ID | Attempt / command | Exact symptom / result | Cause / diagnosis | Evidence | DO_NOT_RETRY_UNLESS | Replacement / next diagnostic |
|---|---|---|---|---|---|---|
| FAILURE-1 | Five-second provider floor in 600ms hook | provider_unavailable / freshness_budget_insufficient | Permanent provider exclusion masks slow reads | f4b01cbc history | Testing actual deadline refusal | Query published indexes with bounded validation |
| FAILURE-2 | Ledger sync on Blueprint supervisor | Blueprint stale until multi-minute Ledger pass ends | Supervisor starvation | First report | Thread ownership changes under test | Dedicated cancellable maintenance; verify foreground concurrency |
| FAILURE-3 | Dedicated maintenance with global operation mutex | recall deadline_exhausted at 29s; minutes of contention | Lock ownership plus legacy scanning | First/Ledger reports | Inputs or synchronization changed | FTS/indexed reads; prove real maintenance concurrent retrieval |
| FAILURE-4 | Synthetic hook payloads | Hook output existed without actual host insertion proof | Wrong acceptance boundary | Initial reports | Testing adapter only | Real host invocation & rollout evidence |
| FAILURE-5 | Multiple binding/explicit calls | corrupt_or_rotation; startupGeneration changes | Final release between calls or duplicate engine, diagnosis not settled | Ledger report | Pin actual owner identity & inspect lifetimes | Prove one engine adoption, not assume transient duplicate |
| FAILURE-6 | Codex hook read_to_end | Host kills after 20s with held-open stdin | Blocking until EOF | Latest report | Framing/deadline repaired | Test held-open/chunked input, strict wire & complete payload preservation |
| FAILURE-7 | Latest installer & load | daemon drain timeout; intermittent SessionStart failure | Still unresolved after one subsequent green run | Latest report | Targeted cause/fix or decisive instrumentation | Bound total hook latency; prove normal drain & loaded startup |
| FAILURE-8 | Manual cache sync at same 0.1.24 | Old binary persists; orphan hooks hold files | Host cache/version reconciliation incomplete | Latest report | Diagnosing only | Canonical installer/activation refresh path, exact identity receipts |

## 7. Learnings, Gotchas & Landmines

| Signal | Hidden trap / learning | Required safe behavior | Source |
|---|---|---|---|
| 0/299 closed | Includes release/host evidence requirements, not 299 missing implementations | Report implementation, installed proof & lifecycle closure separately | Current canons |
| Checker PASS | Validates evidence structure, not product outcome | Read actual acceptance & receipts before closing | User corrections |
| LDG-023 listed open by Devin | EXCLUDED in current Scope | Do not implement journal/cursor expansion | ledger.md |
| Global Codex bearer workaround | Plugin plus config may create duplicate binding or violate intended ownership | Verify exactly one resolved authenticated connection & reconcile docs only against actual accepted boundary | 45bbf19f activation.rs |
| Hook quiet interval | 400ms silence is heuristic, not payload framing; CLI 2s timeout discards unfinished input | Review complete JSON handling, bounded resource use & error propagation; avoid data loss | 45bbf19f membrane-client.rs |
| Engine-relative fallback | activation_control_binary still permits exe-relative fallback with overrides/no install | Verify this cannot revive alternate production engine/store; explicit fixture paths must stay isolated | 45bbf19f |
| Cached script only in one packager diff | Candidate packaging shares/copies earlier payload assumptions | Check both real package paths include membrane-hook.ps1 & stable client identity | Eight-file diff |
| Report says tests blocked locally | Current managed test:mcp succeeded | Consult exact admission, never bypass RightKit or repeat denied commands blindly | Tool result this turn |
| Signed release interpretation | Devin equates RELEASED with signed public release globally | Verify each atom's actual acceptance/delivery contract; do not weaken it or invent extra gates | Canons/qualification rows |
| Heavy tests/builds | Rebuilding repeatedly before diagnosis wastes critical path | Cheapest decisive checks first, one coherent candidate for installed acceptance | User frustration & task history |

## 8. Open Loops & Context Gaps

| Gap / open loop | Severity | Impact | Recovery action | Safe subset | Owner |
|---|---|---|---|---|---|
| Exact latest installed bytes/logs vs committed diff | HIGH | Cannot claim all inherited changes installed-qualified | Read release/activation receipts & saved rollout; compare source/artifact identity | Source inspection & focused tests | New Codex owner |
| Running workers/schedules beyond targeted process scan | MEDIUM | Possible concurrent changes | Inspect activity before runtime mutation; verify Git immediately before commit | Read-only triage | New Codex owner |
| Actual maintenance concurrency & loaded SessionStart | HIGH | Responsiveness not fully proven | Instrument real maintenance plus normal Pull/hook, retain output/latency evidence | Preserve code & targeted diagnostics | New Codex owner |
| Exact all-299 residual mapping | MEDIUM | Cannot give reliable global ETA/completion percent | Read canons & current receipts, derive required gaps without reimplementing delivered behavior | Named integration fixes | New Codex owner |
| macOS/other host access availability | MEDIUM | Required platform proof outstanding | Inspect configured sanctioned host/CI capabilities; no assumed blanket block | Windows/source work | New Codex owner |

## 9. Safety, Authority & Boundaries

- **May do:** User transferred ownership: inspect, repair, test, commit & ordinary push on main; continue already-authorized required implementation without asking again.
- **Do not change:** Accepted scope/ownership to manufacture closure; unrelated CodeRight/Arcane work; parent gitlink or independent repositories incidentally.
- **Do not run:** Direct Cargo/rustc, bypass flags, force push, invented alternate engine/store, manual credential extraction, unsupported background autostart.
- **Irreversible / production actions:** Ordinary push already authorized & done. Public publication requires exact release authority & canonical signing/publication rules; taking ownership does not select an arbitrary artifact for publication.
- **Spend / external effects:** Read matching access/release rules. No messaging others without explicit user instruction. Public builds use sanctioned GitHub paths except declared local unsigned exception.
- **Secrets handling:** Names & locations only; never read/print token values to discover configuration.
- **Reserved decisions:** User owns changes to accepted target/scope. Routine implementation & authorized delivery are Codex-owned.

## 10. Exact Resume Sequence

### Resume Step 1 — Verify delivered checkout

- **Owner:** New Codex integration owner.
- **Working directory / system:** D:/Claude/membrane
- **Exact action:**

```text
# Verify current checkout state
git status --short --branch
git log -3 --oneline
git branch -a
git worktree list
Get-Content docs/pending/README.md -TotalCount 15
```
- **Expected result:** main only, code baseline 45bbf19f ancestor plus packet commit; 299 committed, 0 closed unless new evidence landed.
- **Evidence path:** D:/Claude/membrane/docs/pending/README.md & command output.
- **Timeout / retry:** 30 seconds, one read; adapt on drift.
- **If failure:** Inspect actual checkout before changes; preserve new work.
- **Depends on:** Receipt verification & workspace access.

### Resume Step 2 — Ground remaining host failures

- **Owner:** New Codex integration owner.
- **Working directory / system:** D:/Claude/membrane
- **Exact action:**

```text
# Read accepted implementation scope
Get-Content docs/canon/implementation-contract.md
Get-Content docs/architecture/adr/2026-09-12-context-system-decisions.md
Get-Content -LiteralPath 'C:/Users/adrds/.codex/attachments/72c462f3-9e79-4756-91d0-35966637d3ff/Pasted text.txt' -Tail 190
git show --stat 45bbf19f
```
- **Expected result:** Distinguish inherited Codex compatibility fix from unresolved timeout/drain/cache/auth/lifecycle evidence.
- **Evidence path:** C:/Users/adrds/.codex/attachments/72c462f3-9e79-4756-91d0-35966637d3ff/Pasted text.txt plus committed source files listed in section 3.
- **Timeout / retry:** 30 seconds, one bounded read; no builds.
- **If failure:** Use git source & canonical receipts; mark missing report evidence explicitly.
- **Depends on:** Successful Resume Step 1 checkout verification.

### Resume Step 3 — Inspect cause before next repair/build

- **Owner:** New Codex implementation owner.
- **Working directory / system:** D:/Claude/membrane
- **Exact action:**

```text
# Inspect production entry points
rg -n 'read_hook_stdin|codex_wire_response|activation_control_binary|IO_TIMEOUT|ACTIVATION_TIMEOUT' engine/crates/membrane/src/bin/membrane-client.rs
rg -n 'drain timeout|Drain|holder|plugin|cache' scripts/qualification/install-release.ps1
Get-Content docs/reference/release/local-windows-development.md
Get-Content D:/Claude/docs/rules/release-signing.md
```
- **Expected result:** Identify request deadline owner, final-holder shutdown path & installation projection boundary; choose one concrete repair with decisive tests. Parallel independent work only when coordination pays; one Git owner.
- **Evidence path:** Source locations returned by rg; existing release/qualification receipts located by referenced scripts.
- **Timeout / retry:** 30 seconds for reads; design bounded test before executing expensive lane.
- **If failure:** Resolve renamed source via rg --files; no blind full installer retry.
- **Depends on:** Step 2 & applicable runtime rules.

Then finish normal/loading SessionStart, automatic projection refresh, single binding/engine identity, Hub-off/simultaneous/partial/final release, real maintenance reads, actual Claude/other hosts, useful Adapt loop, storage/source correctness & remaining committed qualification. Keep exact per-atom receipts; do not call a lane completion product completion.

## 11. State Verification & Invalidation

- **Verification command:**

```text
# Verify current checkout state
git status --short --branch
git rev-parse HEAD
git ls-remote --heads origin
```
- **Expected state:** main/origin synchronized after packet commit; 45bbf19f reachable; no other branch heads at last remote check.
- **Invalidated by:** New source commits, manual installed/cache edits, engine generation change, grant/source changes, new release receipts or corrected atom scope.
- **Refresh action:** Re-read Git, exact installed identity & affected acceptance receipts; preserve unaffected proof.
- **Validator command:**

```text
py -3.11 C:/Users/adrds/.claude/skills/legion/skills/handoff/scripts/validate-handoff.py D:/Claude/membrane/tasks/handoffs/2026-09-20/membrane-ownership.md --write-receipt D:/Claude/membrane/tasks/handoffs/2026-09-20/membrane-ownership.receipt.json
```
- **Receiver receipt check:**

```text
py -3.11 C:/Users/adrds/.claude/skills/legion/skills/handoff/scripts/validate-handoff.py D:/Claude/membrane/tasks/handoffs/2026-09-20/membrane-ownership.md --verify-receipt D:/Claude/membrane/tasks/handoffs/2026-09-20/membrane-ownership.receipt.json
```

## 12. First Output & Readback Contract

```text
READBACK
MISSION: Own required Membrane implementation, qualification & main delivery.
CURRENT_STATE: 45bbf19f code pushed; 299 committed, 0 lifecycle-closed at snapshot.
LOCKED_DECISIONS: Five subsystems, one planner/engine, nonmutating Blueprint reads, exact Cortex push, direct Ledger Pull, binary scope.
SAFETY_BOUNDARIES: Main only, preserve work, managed tools, no invented closure/publication.
NEXT_ACTION: Verify live Git & acceptance evidence, inspect timeout/drain/cache path.
CRITICAL_GAPS: Installed identity, reliability & remaining actual-host proof.
ASSUMPTIONS: Supplied reports are evidence pointers, not independent validation.
FIRST_VERIFICATION: Git status/log/branches/worktrees & current pending index.
PACKET_RECEIPT: <verified sha256>
```
- **First deliverable after readback:** Concrete remaining behavioral blockers vs qualification-only gaps, then execute smallest justified repair under transferred ownership.
- **Gap report format:** GAP: severity | missing evidence | impact | recovery action | safe subset | owner.

## 13. Ready-to-Paste First Message

```text
Take ownership of Membrane; Devin is no longer working on it. Read D:/Claude/membrane/tasks/handoffs/2026-09-20/membrane-ownership.md & verify its adjacent receipt. This contains decisions, progress, failures, source reports & exact next actions. Return READBACK, follow Proceed mode IMMEDIATE, verify current main/origin state, then continue required implementation & qualification. Stay on main; no extra branches/worktrees. Do not ask again for authorized fixes/ordinary commit/push. Do not claim completion until matching acceptance evidence supports it.
```

## 14. Context Gap Report

- **Gap summary:** Two HIGH runtime-evidence gaps & three MEDIUM drift/inventory/access gaps listed in section 8; no fatal gap for source inspection/targeted repair planning.
- **Safe-to-proceed scope:** Delivered source review, bounded diagnostics & repairs after observable failure identification; do not claim installed/released completion from this packet.
- **Fatal recovery owner:** NOT_APPLICABLE: first resume actions are safe & executable.
- **Exact recovery sequence:** Verify checkout; read target & latest report; identify exact installed generation/receipts; reproduce named failure; repair/test; qualify coherent candidate; update affected atoms & deliver.

## 15. Handoff Author Gate

- [x] Original intent, latest ownership correction & pending objective preserved.
- [x] LIVE_CONTEXT mode & non-applicable transcript prefix explained.
- [x] Git state & pending counts re-read; code commit/push verified.
- [x] Reported installed progress separated from this author's focused checks.
- [x] Runtime/worker inventory limitations recorded as gaps, not guessed.
- [x] Decisions, exclusions, authority, failures & retry guards explicit.
- [x] No credential values included.
- [x] Exact resume actions & receipt verification provided.
- [x] Cold-reader mission/state/boundaries recoverable without conversation history.
- [x] Packet validation & receipt verification required before delivery.

- [x] Scope remains binary COMMITTED/EXCLUDED.
- [x] Single-engine lifetime preserved as locked decision.
- [x] Current delivery distinguished from release acceptance.
- [x] Platform gaps have named recovery owner.
- [x] Latest source revision recorded exactly.
- [x] State invalidation rules explicit.
- [x] Packet & receipt stored in durable project path.
- [x] User-facing first message supplies absolute packet path.
