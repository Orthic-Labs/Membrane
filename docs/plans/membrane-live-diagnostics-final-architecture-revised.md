# Membrane Live Diagnostics — Final Architecture

**Status:** Reference architecture; runtime/process migration and native-cutover portions superseded by `migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md`

**Scope note:** Capability semantics remain reference input until adopted into canonical subsystem
doctrine. Canonical migration owns runtime/process topology, packaging, deletion, sequencing, and
native-only acceptance.

**Date:** 2026-08-23

**Repository:** `Orthic-Labs/Membrane`

**Capability:** Membrane Live Diagnostics

**Enforcement:** Semantic Edit Fence

This document consolidates:

- [The Findings Lane](agent-findings-lane.md);
- [BP001 import-binding resolution](../../blueprint/docs/design/BP001-import-binding-resolution.md);
- *Membrane Blueprint — Deterministic Diagnostics for LLM Coding Agents*;
- *Membrane Live Diagnostics and the Semantic Edit Fence*;
- *Membrane Live Diagnostics — Canonical Architecture* external synthesis;
- independent model reviews of those proposals.

Canonical Membrane and Blueprint doctrines remain authoritative where this design has not yet
been adopted.


## Coordination amendment — Hub status repair is upstream

This architecture is being implemented in parallel with a separate bounded Hub-status repair.
That repair is the canonical owner of Membrane parent-health presentation and subsystem-status
composition. Live Diagnostics MUST consume the landed Hub status contract; it MUST NOT create a
second overall-health model or independently redesign the Hub header.

### Canonical operational status model

The Hub status repair freezes these distinctions:

```text
Membrane parent service:
  Running | Degraded | Offline

Subsystem capability:
  Available | Degraded | Unavailable | Not configured
```

The Membrane parent state is derived only from resident service health and snapshot availability.
A child subsystem/provider failure remains local unless that dependency is explicitly declared a
hard prerequisite for resident service operation.

Therefore:

- Blueprint `Unavailable` does not by itself make Membrane `Offline` or globally unavailable.
- A stale provider may be `Degraded` without changing parent service health.
- An uninstrumented or intentionally absent capability is `Not configured`, not `Degraded`.
- Presentation code MUST NOT compute parent health from the worst child state.
- Live Diagnostics provider/engine health is reported as capability/subsystem evidence and never
  becomes an alternate parent-health authority.

### Parallel-work ownership fence

Until the Hub-status repair lands, the Live Diagnostics worktree MUST NOT modify or redesign the
Hub-status composition/presentation surfaces owned by that repair, including at minimum:

```text
apps/membrane-hub/src/popover.mjs
engine/crates/membrane-runtime/src/hub_inputs.rs
docs/hub/overview.md
```

If the Hub-status repair also changes shared snapshot/status types or adjacent Hub composition
files, those paths are reserved to that repair until merge. The Live Diagnostics executor must
continue on disjoint files and emit a `SOURCE_DRIFT` / integration note rather than independently
patching those shared files.

After the Hub-status repair merges, Live Diagnostics must rebase and integrate against the landed
typed status contract. It may add diagnostics-specific health inputs, but it may not reintroduce
worst-child aggregation, hardcoded Blueprint placeholders, or a competing overall-state enum.

### Exact-tree qualification rule

A completion claim is valid only against an exact committed tree. Any test, build, qualification,
or Oracle evidence used for sign-off MUST record the tested commit/tree identity and be reproducible
from a clean checkout. Dirty-worktree-only composition, untracked test files, or uncommitted manifest
wiring cannot satisfy acceptance.

---

## 0. Executive decision

Build one Hub-hosted **Membrane Live Diagnostics** runtime that combines exact Blueprint
findings with qualified resident language services, produces mutation-bound diagnostic
evidence, and supports a host-enforced **Semantic Edit Fence**.

Do not create a seventh Membrane subsystem. Live Diagnostics is a Membrane runtime/module under
Hub lifecycle. Blueprint remains an independently bounded Membrane subsystem and retains
repository truth, source identity, graph, generation, freshness, findings, and impact authority.

Do not put LSP or compiler process supervision inside Blueprint. Do not put enforcement inside
Blueprint or the diagnostics runtime. Membrane's single planner owns shared policy. Live
Diagnostics may host its pure deterministic evaluator; coding hosts apply the resulting decision.
Operational evidence and decisions reach the host directly. Context admission, truncation, or
rendering may change what the model sees but can never change the operational decision.

```text
sealed/reconciled host mutation + exact source hashes
                    │
                    ▼
             WorkspaceEpochV1
                    │
          CoverageObligationV1[]
                    │
                    ▼
        MEMBRANE LIVE DIAGNOSTICS
        ├─ Blueprint D0a parse + D0b findings/impact
        ├─ resident D1 language services
        └─ optional D2 analyzers
                    │
                    ▼
       DiagnosticEvidenceSnapshotV1
                    │
                    ▼
        Membrane planner gate policy
                    │
                    ▼
        DiagnosticGateDecisionV1
        ├─ dirty_exact  → repair
        ├─ clean_exact  → continue
        ├─ unknown_*    → recover or escalate
        └─ superseded   → await newest workspace epoch
                    │
                    ▼
       host enforcement / presentation
                    │
                    ▼
 V1 targeted check → V2 tests → V3 build/release
```

---

## 1. Architectural drivers

1. Catch cheap deterministic failures while edit intent remains in context.
2. Never report clean for stale bytes, partial coverage, absent engines, or timed-out work.
3. Keep repository truth and external tool-process lifecycle under their existing owners.
4. Support CodeRight, Claude Code, Codex, MCP clients, CLI clients, and external writes.
5. Keep source authority, cost, freshness, completeness, and gate policy independently visible.
6. Attribute inline repair feedback to one coherent multi-file mutation rather than every write.
7. Preserve unrelated brownfield debt while blocking regressions caused or inherited by touched
   files under explicit policy.
8. Eliminate duplicate user-facing errors without discarding independent provider evidence.
9. Keep compiler checks, tests, builds, and releases economically distinct.
10. Reuse Blueprint's landed BP001/BP002/BP003 work rather than replacing it.

---

## 2. Ownership

| Concern | Canonical owner | Responsibility |
|---|---|---|
| Repository/worktree/source identity | Blueprint | Canonical identity, source hashes, generations, freshness, dirty-overlay evidence |
| Parser/graph findings | Blueprint | Parse issues, BP001-class findings, stable rule IDs, omissions, Tier-0 deltas |
| Project topology and impact | Blueprint | Project/config routing, affected closure, recommended verification scope |
| External engine lifecycle | Membrane Hub / Live Diagnostics | Start, supervise, reuse, isolate, restart, and retire language services/analyzers |
| Operational workspace epochs | Live Diagnostics | Correlate exact sealed/reconciled source manifests, mutations, document versions, and engine convergence without becoming source authority |
| Normalized diagnostic evidence | Live Diagnostics | Normalize observations, coverage, provenance, correlations, and aggregate deltas |
| Final diagnostic policy | Membrane planner | Own versioned gate profiles and required capability obligations without creating a second policy owner |
| Deterministic policy evaluation | Live Diagnostics | Purely evaluate planner-owned policy against exact evidence; never invent policy |
| Edit interception and decision enforcement | Coding host | Seal mutations, present repair packets, gate commands, and prevent false completion |
| Repository task admission | Blueprint | Evidence-backed task/change admission; separate from the post-mutation fence |
| Context admission and representation | Membrane planner | Decide how diagnostic evidence enters context and record degradation/omissions |
| Engineering doctrine | Legion | Require the loop; never own engines, evidence, or host enforcement |
| Behavioral/release proof | Existing check/test/build systems | Remain downstream verification authorities |

### 2.1 Boundaries

- Membrane never opens Blueprint SQLite or imports Blueprint internals.
- Blueprint never hosts a generic LSP runtime or invokes live compilers as a required query path.
- Live Diagnostics never creates a second repository graph, project discovery truth, or final
  policy path.
- `WorkspaceEpochV1` is an operational correlation envelope derived from exact host bytes and
  reconciled Blueprint evidence. It never supersedes Blueprint source identity or generation.
- Hosts enforce; providers report evidence.
- Blueprint task admission and Membrane mutation fencing remain distinct decisions.
- Operational diagnostic schemas do not mutate Membrane's frozen public V1 context shapes.
- The host consumes the full operational snapshot and decision directly. Planner context
  admission and representation cannot weaken or change that decision.

---

## 3. Runtime topology

Hub owns one `DiagnosticsSupervisor`. It owns qualified engine instances keyed by:

```text
WorkspaceEngineKey = hash(
  repo_id,
  worktree_id,
  canonical_worktree_root,
  project_root,
  engine_id,
  engine_version,
  binary_digest,
  toolchain_digest,
  project_config_digest,
  sandbox_policy_digest
)
```

Each worktree has independent mutable engine state. Read-only clients may share a qualified
engine. Concurrent mutation of one worktree requires either a worktree lease or source-hash
conflict handling. Separate Git worktrees naturally form separate lanes.

Supervisor duties:

- lazy start and warm reuse;
- absolute request deadlines;
- bounded concurrency, CPU, memory, process count, and output;
- full-content document synchronization when incremental history is uncertain;
- source-hash conflict detection;
- crash recovery and full resynchronization;
- deterministic selection of the cheapest qualified provider set that satisfies supplied
  capability obligations within `maxCost`;
- idle eviction and process-tree shutdown;
- typed health, coverage, omissions, and degradation.

Every provider adapter exposes one lifecycle contract:

```text
initialize(capabilities)
synchronize(workspaceEpoch)
acquire(workspaceEpoch, absoluteDeadline)
cancel(requestId)
proveConvergence(workspaceEpoch)
shutdown()
```

Instant providers run immediately. Interactive providers stay resident. Verification providers
run only when `maxCost` and policy allow them, are impact-scoped, debounced, coalesced, and
cancelled when superseded. Tests and builds remain explicit downstream requests.

---

## 4. Identity and time model

Four identities must agree without being collapsed:

1. **Blueprint generation** — repository/graph evidence snapshot.
2. **Mutation ID** — optional causal identity for one coherent edit batch.
3. **`WorkspaceEpochV1`** — monotonic operational identity for exact current worktree bytes in
   one diagnostics session.
4. **Engine document version** — adapter-local convergence identity.

`WorkspaceEpochV1` contains:

```text
repoId / worktreeId / epoch / parentEpoch
mutationId?
sourceManifestDigest / changedPaths / exact changed-file hashes
projectConfigDigest / toolchainDigest / sandboxPolicyDigest
origin: transactional | observed_hook | reconciliation
```

Blueprint remains canonical owner of repository/worktree/source identity. Live Diagnostics owns
only this correlation envelope and must reconcile it against exact host bytes and Blueprint's
public generation/hash evidence.

```text
Blueprint generation G + source identity
       +
mutation M? / workspace epoch E / manifest H
       +
provider versions V
       ↓
diagnostic evidence bound to G + E + H + V
```

A stale Blueprint generation broadens impact scope or produces an omission. It never narrows
coverage into a clean claim. A late engine result becomes `superseded`. A current-worktree hash
mismatch becomes `unknown_conflict`.

### 4.1 Mutation boundary

A mutation is one coherent edit batch:

```text
mutation.begin
→ one or more create/modify/delete/rename operations
→ mutation.seal with before/after hashes
→ diagnostics acquisition
```

Transactional mode requires this boundary. Observed hooks may register exact resulting bytes
with weaker causal attribution. Reconciliation-only hosts must compare the current worktree
manifest with the latest cleared epoch before test, build, release, or completion. Any mismatch
creates a new workspace epoch and invalidates old clearance.

---

## 5. Contracts

Use two contracts so evidence remains harness-agnostic while policy remains explicit.

### 5.1 `DiagnosticEvidenceSnapshotV1`

Raw evidence contains:

```text
repoId / worktreeId
blueprintGeneration / blueprintFreshness
workspaceEpoch / sourceManifestDigest
mutationId? / exact changed-file hashes
engine/toolchain/config/sandbox identities
request maxCost / absoluteDeadline
coverageObligations[]
observations[]
issues[]
coverage[]
blueprintDelta
aggregateDelta
omissions[]
timing/provenance
```

Every observation includes orthogonal classifications:

```text
sourceClass:
  parser | repository_finding | native_language_service |
  static_analyzer | compiler_check

costClass:
  instant | interactive | verification | build | test
```

Internal Rust types are implementation representation. Canonical JSON schema is wire
representation. They are projections of one semantic contract, not competing authorities.

### 5.2 `DiagnosticGateDecisionV1`

Decision contains:

```text
snapshotId
policyProfile / policyVersion / policyDigest
outcome
blockingIssueIds[]
requiredObligations[]
reasonCodes[]
omissions[]
```

Closed outcome vocabulary:

```text
clean_exact
dirty_exact
unknown_incomplete
unknown_unavailable
unknown_timed_out
unknown_conflict
superseded
```

There is no `clean_partial`, `clean_stale`, or `probably_clean`.

### 5.3 Decision precedence

This final contract intentionally amends the earlier Live Diagnostics proposal, which required
all required providers to complete before either exact outcome. Exactness is asymmetric: one
current, exact blocking observation is sufficient to prove dirty, while proving clean requires
complete exact capability coverage. `dirty_exact` therefore means **dirty state proven for the bound
bytes**, not **all possible diagnostics enumerated**. Incomplete lanes remain explicit in
coverage, omissions, and reason codes.

1. If hashes or mutation ancestry do not match, return `unknown_conflict` or `superseded`.
2. If any policy-blocking observation exactly matches current bytes, return `dirty_exact` even
   when another required capability is unavailable. One exact blocker is sufficient proof of
   dirty.
3. If no blocker exists and every required capability obligation is exactly satisfied, return
   `clean_exact`.
4. Otherwise return the applicable `unknown_*` outcome.

This preserves no-false-clean while ensuring a known error is not hidden behind an unrelated
provider failure. After that error is repaired, incomplete required coverage still prevents a
clean claim.

---

## 6. Diagnostic and verification ladder

| Tier | Owner/mechanism | Cost | Role |
|---|---|---:|---|
| **D0a** | Blueprint parser | instant | Exact positive syntax/parse evidence |
| **D0b** | Blueprint exact repository findings | instant | Missing modules/bindings, re-export breaks, impact |
| **D1** | Resident native language service | interactive | Names, types, signatures, references, native semantic diagnostics |
| **D2** | Qualified deterministic analyzer/lint lane | interactive/configured | Selected low-noise static rules |
| **V1** | Targeted compiler/checker | verification | Build-system-aware static proof |
| **V2** | Targeted tests | test | Behavioral proof |
| **V3** | Build/package/release | build | Codegen, linking, artifacts, release proof |

Warmth changes latency, not authority, side effects, or cost class. Therefore:

- rust-analyzer native diagnostics are D1;
- rust-analyzer flycheck backed by `cargo check` is V1;
- `cargo check` remains V1 even when warm;
- `cargo build` is never routine inner-loop correctness feedback;
- TypeScript native language-service diagnostics are D1;
- `tsc --noEmit` remains compiler/check evidence unless its exact qualified mode is explicitly
  classified otherwise.

D0a/D0b are sublanes of one instant cost tier. They run without external process, package
installation, network, or repository-executed code.
D1/D2 engines must pass adapter qualification. V1 is explicit, debounced, cancellable, and
Blueprint-scoped. V2/V3 remain outside Live Diagnostics acquisition by default.

---

## 7. Blueprint D0a/D0b lane

Blueprint D0a contains exact Tree-sitter parse/missing-node findings with source ranges.

Blueprint D0b contains:

- BP001 imported binding not exported;
- BP002 unresolved repository-relative module specifier;
- BP003 invalid named re-export;
- future exact provider-qualified findings;
- BP010 impact and verification-scope evidence.

The BP001 closed-surface rule is the D0b soundness pattern:

> A negative finding exists only when the relevant semantic surface is closed. Unsupported,
> ambiguous, partially parsed, dynamic, generated, or external semantics produce typed omissions.

BP001/BP002/BP003 are currently JS/TS provider capabilities. They are not cross-language by
construction. Other languages require their own qualified provider semantics.

### 7.1 Phase-0 integration correction

Keep landed Phase-0 implementation, tests, CLI, SARIF, stable rule IDs, fingerprints, and
coverage. Before treating it as final architecture:

1. feed module surfaces into Blueprint's canonical provider capability system;
2. converge specifier/name resolution on Blueprint's one exact-first resolution owner;
3. avoid a permanent parallel scanned-file resolver;
4. add named-generation baseline delta and dirty-overlay incrementality;
5. bind daemon results to exact generation and content hashes;
6. expose findings through Blueprint's resident public service;
7. retain CLI/SARIF as adapters over that behavior.

Only after exact generation/hash binding is proven may the Membrane adapter classify Blueprint
D0a/D0b as `snapshot_checker_exact`.

---

## 8. Coverage, convergence, and baselines

The fence is defined over semantic capabilities, not provider completion. Membrane's planner
derives required obligations from repository configuration, language/dialect, touched scope,
Blueprint topology/impact, gate profile, and cost policy:

```text
CoverageObligationV1
capability
languageDialect / projectIdentity / requiredScope
exactnessRequirement
acceptableProviderAlternatives[]
maximumCost
state
omissions[]
```

Initial capability vocabulary includes:

```text
syntax
repository_module_resolution
import_export_binding
name_resolution
type_semantics
configured_static_policy
compiler_project_semantics
generated_source_awareness
```

Live Diagnostics chooses the cheapest qualified provider set that can satisfy each supplied
obligation within `maxCost`. At least one qualified exact provider must cover the required
capability and scope. An optional duplicate provider's failure remains recorded but does not
invalidate coverage already supplied by another qualified exact provider.

Each invoked producer emits a coverage lane:

```text
engine/project/scope
capabilities covered
requested and covered entities/files/packages
convergence class
bound workspace epoch
state: complete | partial | unavailable | timed_out | unsupported
omissions[]
```

Qualified convergence classes:

- `pull_exact`;
- `push_versioned_exact` with a proven completion barrier;
- `snapshot_checker_exact`;
- `push_unversioned_advisory`;
- `unsupported`.

Quiet windows and unversioned asynchronous pushes may improve presentation latency but cannot
clear the fence.

### 8.1 Two deltas

- Blueprint owns its Tier-0 finding delta against named generation/treeish/session baseline.
- Live Diagnostics carries that delta without recomputation and calculates aggregate
  observation/issue delta across exact workspace epochs.

Aggregate classifications:

```text
new | persistent | resolved | moved | changed | unknown_baseline
```

Default gate profile is `changed-files-zero`:

- touched files must contain no policy-blocking diagnostics;
- no new blocking issue may appear in Blueprint's affected closure;
- unrelated pre-existing debt outside affected scope does not hijack the task.

Baseline identity includes repository/worktree, source/config/toolchain/engine identities,
adapter/normalizer versions, capability profile, and policy digest.

---

## 9. Observation correlation and repair packets

Independent producers may report one logical problem, such as BP001 and TypeScript `TS2305`.
Do not discard either observation. They differ in authority, cost, coverage, and failure mode.

Use two identities:

```text
ObservationFingerprint = provider + version + code + path + message + semantic anchor/range
IssueCorrelationKey     = repository + path + semantic anchor + requested/exported name + target
```

Snapshot stores every observation and groups confidently equivalent observations under one
`DiagnosticIssue`. Uncertain matches remain separate. User-facing repair packets show one issue
with supporting observations:

```text
Missing export `RunPolicy`
  observed by Blueprint BP001 (instant)
  confirmed by TypeScript TS2305 (interactive)
```

This preserves evidence honesty while preventing duplicate squiggles.

Source line numbers are never stable identity. A semantic anchor is preferred; normalized range
is only a bounded fallback when no stronger anchor exists. An occurrence ID may include exact
range, workspace epoch, and content hash without changing stable observation/issue identity.

---

## 10. Semantic Edit Fence

Fence states control what may happen after a sealed mutation:

| Outcome | Allowed | Blocked by default |
|---|---|---|
| `dirty_exact` | inspect, search, explain, repair, rerun diagnostics | unrelated implementation, ordinary tests/builds, completion |
| `clean_exact` | continue or escalate proportionally | nothing additional |
| `unknown_*` | inspect, repair service/config, run approved V1 verifier | clean claim, completion, escalation that assumes semantic cleanliness |
| `superseded` | await newest epoch | decisions based on old evidence |

The fence blocks semantic acceptance, not disk persistence. It never auto-rolls back edits.

An explicit typed exception may allow a generator or V1 checker needed to make D1 exact. It must
identify repository/project, command capability, allowed effects, expiry/config digest, and
mandatory post-command mutation reconciliation.

---

## 11. Host integrations

Every session declares one host mode:

| Mode | Guarantee | Typical integration |
|---|---|---|
| `transactional` | Host begins, applies, and seals one coherent mutation with exact resulting hashes | CodeRight; transactional MCP wrapper |
| `observed_hook` | Host registers exact observed resulting bytes/hashes; causal grouping may be weaker | Claude Code; Codex with edit hooks |
| `reconciliation_only` | Host proves current worktree manifest only at mandatory gates | CLI clients; generic hosts without edit interception |

CodeRight uses exact edit transactions in its tool dispatcher. Generic MCP uses
`mutation.begin` → edits → `mutation.seal` or one transactional edit wrapper. Claude Code and
Codex use observed hooks when available plus mandatory reconciliation before test, build,
release, or completion. Raw CLI snapshot/delta APIs remain useful without enforcement.

Host integration is optional for diagnostic visibility but required for a strong mutation-bound
fence. External writes invalidate the latest cleared decision until reconciliation.

---

## 12. Public operational surface

Expose one operational capability through existing Hub, CLI, MCP, and client adapters:

```text
diagnostics.workspace.open/status/close
diagnostics.workspace.reconcile
diagnostics.mutation.begin/seal/registerObserved
diagnostics.snapshot.await/get/explain/delta
diagnostics.fence.evaluate
diagnostics.capabilities
diagnostics.baseline.capture/update
diagnostics.provider.list/status/restart
diagnostics.subscribe
```

Requests identify repository/worktree, workspace epoch or mutation, required capability profile,
scope, gate profile, `maxCost`, and one absolute deadline. Events support presentation and
telemetry but cannot clear the fence; `snapshot.await` plus `fence.evaluate` is the operational
enforcement path.

Human-readable output and SARIF are renderings of canonical evidence, not independent truth
paths. The operational schema remains separate from frozen Membrane context V1 shapes while
retaining one owning implementation and one schema authority.

Every acquisition request carries a hard `maxCost`. Provider scheduling may use cheaper lanes
but may never silently cross that ceiling. `maxCost` and gate profile are orthogonal: cost limits
what may run; policy defines which coverage is required. If required coverage cannot be obtained
within the ceiling, result is typed unknown or invalid configuration, never implicit escalation.

---

## 13. Security and containment

Repository text is untrusted data. Engine adapters declare side-effect class:

```text
pure_analysis
repository_plugin_load
package_manager_access
compiler_spawn
build_script_execution
network_required
```

Default policy:

- no network;
- repository-scoped filesystem access;
- read-only outside declared cache/output locations;
- allowlisted toolchain resolution;
- sanitized environment and no implicit credentials;
- bounded CPU, memory, process count, output, and absolute wall time;
- process-tree termination on timeout/shutdown;
- no automatic dependency installation;
- code actions returned as suggestions and applied only through a new mutation.

Untrusted-repository mode guarantees D0a/D0b. Richer lanes degrade explicitly according to sandbox
and toolchain availability.

---

## 14. Storage and physical boundary

Persist only audit/evaluation state: workspace epochs and source manifests, snapshots and
decisions, provider/config/toolchain/policy identities, stable fingerprints, obligations,
coverage, omissions, baselines/deltas, health, timing, and failure reason codes. Keep live
document overlays, progress events, request correlation, stdout/stderr, and superseded
intermediate results ephemeral.

Cache reuse requires every behavior-bearing source, Blueprint, provider, toolchain,
configuration, sandbox, adapter, normalization, capability-profile, and policy input to match.

Live Diagnostics remains one capability under Hub regardless of physical packaging. Start at
the existing runtime/provider boundary. Extract a dedicated crate when its public contract is
stable and independent lifecycle, sandbox, cache, or multi-engine qualification makes that
boundary cheaper than continued colocation. Directory layout never creates policy or source
authority.

---

## 15. Implementation sequence

### Phase 0 — Complete D0a/D0b contract

- preserve exact parser issue ranges;
- integrate Phase-0 findings with canonical provider/resolution ownership;
- implement Blueprint generation baseline + dirty-overlay incremental delta;
- expose generation/hash-bound findings through resident service;
- freeze only narrow source-identity, omission, and evidence-envelope seams needed for D0a/D0b;
- qualify deterministic output, omissions, and no-false-positive fixtures.

### Phase 1 — Prove delivery value

- inject D0a/D0b delta after sealed/observed edit batches;
- use advisory mode first;
- prove agents repair before tests/builds;
- stop if behavior and expensive-command frequency do not improve.

### Phase 2 — Freeze Membrane contracts + TypeScript D1

- add workspace epoch, coverage obligation, evidence, and gate schemas;
- implement one persistent TypeScript adapter;
- prove mutation correlation, stale clearing, crash/timeout behavior, normalization, correlation,
  and warm latency.

### Phase 3 — Hub lifecycle

Prerequisite: the separate Hub-status repair has landed and its typed parent/subsystem state model
is authoritative.

- add supervisor, workspace reuse, absolute deadlines, cache, eviction, health, sandbox, and
  installed-runtime qualification;
- publish Live Diagnostics/provider health into the existing subsystem/capability status contract;
- preserve parent-service health as resident-service/snapshot truth only;
- never aggregate diagnostics/provider failure into parent Membrane failure;
- represent absent/uninstrumented diagnostics capabilities as `Not configured`;
- do not restore `worstPresentation()`-style worst-child aggregation or hardcoded Blueprint status.

### Phase 4 — Blueprint routing + enforced hosts

- add project/impact routing slice;
- broaden stale graph scope conservatively;
- implement CodeRight, MCP, Claude, and Codex enforcement;
- qualify `transactional`, `observed_hook`, and `reconciliation_only` modes;
- reconcile unobserved writes before test, build, release, or completion.

### Phase 5 — Rust

- rust-analyzer native D1;
- distinct RightKit-controlled targeted V1 `cargo check`;
- feature/target/build-script/proc-macro omissions;
- isolated cache/target policy without calling verification free.

### Phase 6 — More languages and optimization

- Pyright, gopls, then additional qualified engines;
- prewarming, impact-guided opening, cache reuse, resource budgets, and richer repair packets;
- add D2 only when measured precision and latency justify it.

---

## 16. Acceptance

1. Every enforced result identifies exact repo, worktree, mutation, source hashes, providers,
   configurations, and policy.
2. Stale, partial, unavailable, conflicting, or timed-out evidence never becomes clean.
3. One exact current blocker produces `dirty_exact` even if another lane degraded.
4. `clean_exact` requires every required capability obligation to be exactly satisfied; optional
   provider failure cannot invalidate already-satisfied exact coverage.
5. Blueprint D0a/D0b work without external engine/process/toolchain execution.
6. BP001/BP002/BP003 reach agents as edit-scoped delta, not repository-wide noise.
7. BP001 plus language-service duplicates appear as one repair issue with two observations.
8. Multi-file mutations may be inconsistent internally and are fenced only when sealed.
9. Current worktree bytes cannot inherit a cleared decision for older bytes.
10. Compiler/check work is separately cost-labelled, debounced, cancellable, and impact-scoped.
11. Tests and builds remain downstream proof layers.
12. Blueprint is consumed only through public service contracts.
13. Membrane's one planner remains final policy owner; host remains enforcement owner.
14. Existing Membrane public V1 context shapes remain unchanged.
15. Context admission, truncation, or rendering cannot alter an operational gate decision.
16. Every host mode states its exactness and causal-attribution strength.
17. Frozen benchmarks establish SLOs and show fewer late bulk repairs and unnecessary
    tests/builds with zero stale-clean incidents.
18. Parent Membrane health is derived only from resident-service health and snapshot availability;
    child subsystem/provider failure cannot be promoted into parent failure without an explicit
    hard-prerequisite rule.
19. Subsystem/capability presentation preserves `Available`, `Degraded`, `Unavailable`, and
    `Not configured` as distinct states.
20. Live Diagnostics consumes the landed Hub status contract and does not define a competing
    overall-health aggregation path.
21. Every acceptance/qualification result records the exact tested commit/tree and is reproducible
    from a clean checkout with all required manifests, test registrations, and source files committed.

---

## 17. Rejected shapes

- Generic LSP runtime inside Blueprint.
- Membrane-owned duplicate repository graph or revision truth.
- Diagnostics as a seventh named subsystem.
- Broker-owned final policy or host enforcement.
- One overloaded contract mixing raw evidence and policy decision.
- One generic cross-language module resolver.
- Treating warm compiler/check work as free native diagnostics.
- Empty diagnostic arrays interpreted as clean.
- Quiet-window convergence used as exact proof.
- Per-file-write fencing instead of sealed mutation batches.
- Suppressing duplicate provider evidence rather than correlating observations.
- Full build/test execution as routine diagnostic acquisition.
- Automatic toolchain/dependency installation.
- Provider completion used as a substitute for capability coverage.
- A workspace epoch treated as competing Blueprint source truth.
- A physical crate or directory treated as architecture authority.
- Unmeasured latency targets promoted to guarantees.

---

## 18. Final rule

> After every sealed agent edit batch, obtain diagnostic evidence bound to the exact resulting
> source state. Any exact policy-blocking observation proves the mutation dirty. Only exact
> satisfaction of every required semantic capability with no blocker proves it clean. Everything
> else remains unknown. Repair first, then escalate through targeted checks, tests, and builds
> only when each higher tier adds proof unavailable below it.
