# Membrane Full-Rust Federation Port Architecture

**Decision ID:** `MEM-ADR-RUST-FEDERATION-001`

**Status:** Superseded for execution by `migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md`; accepted decision partially realized

**Date:** 2026-08-22

**Decision authority:** Adrian D'souza

**System:** Membrane Pull federation path

**Objective:** Replace every Python-executed federation responsibility with Rust while preserving public protocol, provider ownership, safety, degradation, deadline, receipt, and installed-product behavior.

**Closure ownership:** Production native routing landed in `5a9175b...`; N7 and N10 of the
canonical migration own remaining deletion, configuration, qualification, and final-seal closure.

## 1. Executive decision

Membrane federation will become an in-process Rust capability owned by
`membrane-runtime`.

The completed product will:

- execute `POST /federate` without starting Python;
- contain no Python gateway or Python provider adapter in its federation runtime;
- require no Python executable, Python environment variable, workspace source checkout, or federation script in an installed product;
- retain one Membrane planner and the five public V1 protocol shapes;
- retain Blueprint, Cortex, Audit, architecture-decision, rules, skills, anchors, and Git/live evidence ownership;
- preserve one absolute request deadline, deterministic merge order, typed local degradation, content-free receipts, and scope-grant fail-closed behavior;
- ship one Rust path with no production compatibility flag or shadow Python worker.

This is a runtime and packaging simplification, not a protocol redesign.

## 2. Why this change exists

Current federation is split across a Rust resident and a Python persistent worker:

```text
MCP / host
  -> resident POST /federate
  -> Rust ResidentGateway supervisor
  -> Python gateway.py --serve-stdio
  -> Python provider fan-out
  -> JSON ContextCandidateSetV1
  -> Rust planner
  -> ContextPacket + receipts
```

That split produced an installed-product failure:

- Hub resolved configured workspace Python 3.14 but discarded it before resident launch.
- Federation fell back to `/usr/bin/python3` 3.9.6.
- Python 3.9 failed while importing a provider type annotation.
- Worker stderr was discarded.
- Resident retried, opened its circuit breaker, and left `/health` green while federation was unusable.
- Installed resources did not contain `engine/federation/gateway.py`; current execution succeeded in locating source only because a development workspace was mounted.

Current Python federation comprises 4,976 lines across 28 Python files, including
the gateway, 10 provider/support modules, and 15 test modules. It is large enough
to require a controlled port, but small enough to replace without a new service or
protocol version.

## 3. Scope

### In scope

- `engine/federation/gateway.py` orchestration, readiness, fan-out, merge, warning, freshness, and worker-request behavior.
- Python adapters for Blueprint, Cortex, Audit, architecture decisions, Git, live worktree evidence, rules, anchors, skills, and scope grants.
- Rust `ResidentGateway`, Python interpreter discovery, script discovery, child supervision, circuit breaker, stdio protocol, and source fingerprinting.
- Hub workspace configuration fields used only to locate Python.
- Rust route and CLI paths that currently launch or depend on Python federation.
- Federation equivalence, failure, freshness, merge-order, warm-path, fault-injection, packaging, and installed-product tests.
- Canonical documentation and generated truth affected by ownership-path changes.

### Out of scope

- Porting Blueprint itself to Rust.
- Replacing Git or Blueprint subprocess/service boundaries that those owners expose.
- Changing MCP tool names or request shapes.
- Creating `ScopeGrantV2`, `ContextCandidateSetV2`, `ContextPacketV2`, `ContextReceiptV2`, or `KnowledgeEmissionV2`.
- Rewriting the final admission planner.
- Changing provider authority, freshness, trust, ranking, or budget policy.
- Removing Python from unrelated workspace tooling, tests, fixtures, or products.
- Adding a general plugin runtime, dynamic provider marketplace, WASM host, or new Pull crate.

### Definition of “full Rust port”

“Full Rust port” means no Python code executes on any Membrane federation request,
no Python process is part of resident lifecycle, and no installed federation feature
depends on Python or a source checkout. Non-executable `.py` files owned by Blueprint
fixtures or unrelated developer tooling are outside this definition.

## 4. Locked constraints and invariants

1. Membrane remains parent system; Pull owns semantic evidence retrieval, admission,
   fusion, and publication.
2. One Membrane planner owns final eligibility, sufficiency, fusion, attention,
   representation, publication, omissions, and receipts.
3. Public V1 serialized shapes remain byte-compatible for equivalent inputs.
4. Blueprint owns repository truth, graph traversal, generation identity, and
   re-anchoring. Membrane must not read Blueprint SQLite or implement a second graph.
5. Cortex owns durable knowledge. Federation consumes typed Cortex APIs and does not
   open Cortex storage through a new path.
6. Hub remains sole resident service and process-lifecycle authority.
7. Provider failure remains local degradation unless the provider is a declared hard
   prerequisite.
8. Scope-grant validation remains fail-closed and binds exact client, task, session,
   canonical repository root, manifest digest, status, and expiry.
9. One absolute request deadline flows from ingress through freshness, provider fan-out,
   merge, planning, and publication. Nested operations cannot reset it.
10. Merge order and duplicate resolution remain deterministic.
11. Every material timeout, invalid output, unavailable provider, generation mismatch,
    omission, cap, and fallback remains typed and attributable.
12. Repository/model text remains data and cannot self-authorize.
13. No production runtime can select Python after cutover.
14. No new process plane or resident service is introduced.
15. Generated docs remain generated.

## 5. Quality scenarios

| ID | Stimulus | Required response | Proof |
|---|---|---|---|
| QS-1 | Installed Hub receives a valid `/federate` request without Python installed or a source checkout present. | Rust federation returns a valid packet or typed provider degradation; no script/interpreter error occurs. | Installed-artifact smoke test on a clean environment. |
| QS-2 | One provider times out, crashes, or returns invalid data. | Other eligible providers complete inside the original deadline; failed lane produces a typed warning/omission. | Fault-injection fixtures for every terminal class. |
| QS-3 | Blueprint generation changes after freshness observation. | Blueprint candidates are rejected with the existing generation-change reason; no mixed-generation packet publishes. | Frozen generation-race fixture. |
| QS-4 | Scope grant is absent for same-root access. | Existing same-root policy is preserved. | Scope-isolation fixture. |
| QS-5 | Scope grant is invalid, expired, inactive, cross-root, or request-mismatched. | Request fails closed before provider publication. | Frozen scope-grant negative corpus. |
| QS-6 | Identical request and frozen provider observations are given to legacy and Rust implementations. | Canonical packet, receipt, omission, provider order, and reason tokens match except explicitly allowlisted timing/process metadata. | Differential equivalence harness. |
| QS-7 | Resident receives normal warm traffic. | No child process is spawned; latency and resident memory do not regress against a frozen legacy baseline. | Before/after benchmark using same workload and machine. |
| QS-8 | Hub starts, stops, crashes, or upgrades. | Federation lifecycle is identical to resident lifecycle; no orphan worker or independent circuit state exists. | Hub lifecycle and process-tree tests. |
| QS-9 | Provider content contains paths, control characters, symlink escapes, or instruction-like text. | Path confinement, hashing, trust class, instruction policy, and typed rejection remain unchanged. | Existing provider security fixtures ported to Rust. |
| QS-10 | Release artifact is inspected. | Federation runtime contains no Python gateway, provider Python package, interpreter configuration, worker supervisor, or script override. | Bundle inventory plus source absence checks. |

No new numeric performance target is invented here. Wave 0 freezes current warm/cold
latency, memory, candidate, omission, and receipt baselines; cutover requires
non-regression on the same representative workload.

## 6. Current responsibility map

| Current owner | Responsibility | Problem |
|---|---|---|
| `pull/federation.rs` | Locate Python, run one-shot gateway, parse CCS, invoke planner. | Runtime interpreter and source-tree dependency. |
| `pull/federation_worker.rs` | Supervise persistent Python worker, handshake, restart, fingerprint, circuit breaker. | Duplicate lifecycle/failure domain under Hub. |
| `serve.rs` | Validate `/federate`, locate script, serialize worker access, map worker failures. | Synchronous bridge to external worker; health omits worker readiness. |
| `engine/federation/gateway.py` | Freshness gate, scope validation, bounded fan-out, deterministic merge, warnings, telemetry. | Core Pull behavior outside shipping Rust binary. |
| `engine/federation/providers/*.py` | Convert provider-owned evidence into candidate records. | Python and workspace-tool imports leak into product runtime. |
| Hub workspace config | Resolve `workspaceRoot` and `pythonExecutable`. | Python field is discarded and should not exist after port. |

## 7. Target architecture

```text
MCP / host
  -> authenticated resident POST /federate
  -> FederationRequestV1 validation
  -> Rust FederationCoordinator
       -> ScopeGrantGate
       -> existing Rust FreshnessService
       -> fixed ProviderKind plan
       -> one absolute Deadline
       -> bounded concurrent provider collection
       -> deterministic CandidateSetAssembler
  -> existing Rust planner
  -> existing packet / receipt publication
```

### 7.1 Runtime placement

Target code remains inside `membrane-runtime`:

```text
engine/crates/membrane-runtime/src/pull/federation/
  mod.rs                 public CLI/runtime entry points
  request.rs             strict request parsing and binding
  coordinator.rs         freshness, provider plan, deadline, fan-out
  outcome.rs             typed provider success/degradation terminals
  merge.rs               deterministic CCS assembly and warnings
  scope_grant.rs         request-context validation over catalog-owned grant
  providers/
    mod.rs               fixed ProviderKind registry and shared context
    anchors.rs
    architecture.rs
    audit.rs
    blueprint.rs
    cortex.rs
    git.rs
    live.rs
    rules.rs
    skills.rs
```

This is a module refactor of existing Pull ownership, not a new crate or subsystem.

### 7.2 Internal provider abstraction

Providers are a fixed internal enum, not a dynamic plugin trait:

```rust
enum ProviderKind {
    Blueprint,
    Audit,
    Architecture,
    Cortex,
    Git,
    Live,
    Rules,
    Anchors,
    Skills,
}

struct ProviderContext<'a> {
    request: &'a FederationRequestV1,
    freshness: &'a FreshnessVerdict,
    deadline: Deadline,
    services: &'a FederationServices,
}

struct ProviderOutcome {
    provider: ProviderKind,
    generation: Option<String>,
    candidates: Vec<CandidateV1>,
    warnings: Vec<ProviderWarning>,
    timing: ProviderTiming,
}
```

Fixed enum dispatch is selected because current provider set is closed, known at compile
time, and product-owned. A dynamic trait object, plugin registry, ABI, or WASM runtime adds
distribution and compatibility cost without a declared requirement.

`membrane-provider-sdk::Provider` is not reused. That trait represents external MCP
operation adapters, not internal evidence-candidate producers.

### 7.3 Concurrency and deadline

- Route ingress creates one monotonic absolute deadline from existing request policy.
- Coordinator evaluates freshness and hard prerequisites first.
- Eligible providers start concurrently using Tokio tasks.
- Blocking filesystem and Git operations run in bounded `spawn_blocking` tasks or
  process-aware async wrappers.
- Every task receives remaining time; no task creates an independent full timeout.
- Deadline expiry cancels unfinished task handles and emits one terminal per lane.
- Results are sorted by canonical provider order before merge, independent of completion
  order.
- No provider lock may serialize unrelated lanes.
- No provider may call `/federate` recursively.

### 7.4 Service access

`FederationServices` exposes typed owner APIs already hosted by resident:

- Cortex candidate retrieval through the existing Rust memory-provider/application API.
- Scope-grant lookup through catalog-owned typed functions.
- Freshness through existing Rust freshness logic.
- Skills through one shared engine snapshot function used by both route and provider.
- Blueprint through its owned daemon protocol and generation-bound request.
- Git through explicit argv execution with no shell.
- Audit and architecture evidence through owner-produced, versioned candidate projections.

Loopback HTTP remains an external surface. In-process federation does not call its own HTTP
server for Cortex, skills, scope grants, or freshness; route handlers and federation share
the same typed application function instead.

## 8. Provider port map

| Provider | Rust source/transport | Required parity | Ownership rule |
|---|---|---|---|
| Blueprint | Blueprint daemon client using current framed protocol and generation pin. | Candidate cap, cache key, timeout, abstention, generation change, source hashes, observability. | No Blueprint SQLite, parser, traversal, or re-anchor implementation in Membrane. |
| Cortex | Existing Rust `memory_provider` and store-facing application API. | Scope descriptor, ranking output, stage timing, no legacy prompt-path fallback. | Cortex owns durable-memory semantics. |
| Git | Explicit `git -C <root>` argv commands with bounded output and time. | HEAD/status/branch candidate identity and typed unavailable behavior. | Git remains source owner. |
| Live | Rust overlay verifier over central freshness verdict plus bounded Git blob reads. | Dirty/new/deleted/renamed paths, snapshot hashes, race rejection, caps, control-character/path safety. | Do not recreate central freshness policy. |
| Rules | Rust confined file reader plus existing session-delivery ledger semantics. | Native/inline/reference modes, full-policy first delivery, hash-triggered redelivery, deterministic order. | Host-loaded policy remains native; text never self-authorizes. |
| Anchors | Confined path handling plus Blueprint-owned resolution for semantic anchors. | Exact source hash, traversal/symlink rejection, unresolved/raw fallback, deterministic candidates. | Blueprint owns semantic resolution. |
| Skills | Shared Rust skills snapshot/read application API. | Enablement, generation seal, ranking, reference-only bodies, typed unavailable state. | Skills owner produces snapshot; federation only adapts it. |
| Audit | Versioned Audit-owned `ContextCandidateSetV1` projection from a completed frozen report. | Finding IDs, trust overrides, source refs/hashes, exact/recoverable fields, local degradation. | Membrane does not interpret Audit internals or select providers. |
| Architecture | Versioned Architect-owned `ContextCandidateSetV1` projection from decision records. | Repository identity, decision IDs, source refs/hashes, trust/instruction policy, local degradation. | Membrane does not own decision lifecycle. |
| Scope grant | Catalog lookup plus Rust request-binding validator. | Status/expiry, nonce, client/task/session/root/manifest bindings, fail-closed errors. | Catalog owns stored grant; federation owns exact request validation. |

Audit and architecture projections are explicit integration dependencies. Until owner-produced
projections exist, those lanes return typed `provider_capability_missing`; Rust must not import
owner-private Python libraries or reverse-engineer owner stores.

## 9. Candidate assembly contract

Rust `CandidateSetAssembler` preserves existing behavior:

1. Canonical provider order is `blueprint`, `audit`, `architect`, `cortex`, `git`,
   `live`, `rules`, `anchors`, `skills`.
2. Candidate IDs deduplicate first-wins after canonical ordering.
3. Provider name is stamped by coordinator, not trusted from provider payload.
4. Missing freshness class receives only the existing provider-specific default.
5. Blueprint and skills generation must match central freshness verdict.
6. Release-generation mismatch retains existing typed stale/degradation behavior until a
   separately approved policy change.
7. Provider warnings become attributable omissions/receipt terminals using existing reason
   tokens.
8. Candidate and provider ceilings remain bounded by request budget and current caps.
9. Internal timing and warning metadata never leaks into public closed shapes except through
   existing envelope/receipt fields.
10. Planner receives `ContextCandidateSetV1`; no second admission or ranking policy is added.

## 10. Error and health model

Removing the Python worker removes worker-only failure states:

- `spawn resident gateway`;
- readiness handshake timeout;
- stdio framing failure;
- stale Python source fingerprint;
- orphan worker tree;
- Python circuit open;
- interpreter/script unavailable.

Remaining federation states are provider and request states:

| Class | HTTP/product behavior |
|---|---|
| Invalid request | `400` with existing typed validation error. |
| Invalid scope grant | fail-closed authorization response before publication. |
| Resident overload | existing bounded `503` overload response. |
| Hard prerequisite unavailable | typed `503`/abort matching existing policy. |
| Optional provider unavailable/timeout/invalid output | successful partial packet with attributable degradation. |
| Planner or serialization failure | typed `5xx`, no partial unvalidated publication. |

Detailed health adds federation readiness derived from coordinator dependencies and recent
lane terminals. Liveness remains independent: optional provider degradation cannot take resident
liveness down. Hub snapshot must distinguish resident health from federation capability health.

## 11. Configuration and packaging

### Target configuration

Workspace configuration advances from schema v2 to v3:

```json
{
  "schemaVersion": 3,
  "workspaceRoot": "/absolute/workspace/root"
}
```

`pythonExecutable` is removed. Updater/installer performs an atomic v2-to-v3 migration by
retaining canonical `workspaceRoot` and dropping Python configuration. Runtime reads only v3
after migration and emits a typed migration-required error for unmigrated invalid input.

### Bundle rules

Shipping artifacts must not include:

- `engine/federation/gateway.py`;
- `engine/federation/providers/*.py`;
- a Python interpreter for federation;
- Python package metadata for federation;
- `MEMBRANE_FEDERATION_SCRIPT` behavior;
- `PYTHON` selection behavior;
- `ResidentGateway` Python worker code;
- tests or fixtures registered as runtime resources solely for Python federation.

Installed Hub starts only bundled Rust binaries and separately owned packaged services such as
Blueprint. It must work with source volume unavailable.

## 12. Migration strategy

### Mode: `HARD_CUT`

There will be no released dual runtime and no production traffic split.

During development, legacy Python runs only in the differential test harness. It is never selected
by Hub, `/federate`, CLI, environment variable, or feature flag in a shipping build. Final cutover
deletes legacy runtime code and test registration in the same integration change that makes Rust
canonical.

Rollback is artifact-level: reinstall the last known-good signed release. Rollback is not a runtime
switch to Python.

### Absence checks at cutover

| Surface | Required absence evidence |
|---|---|
| Imports | No Rust source references Python federation modules or interpreter resolution. |
| Routes | `/federate` has one Rust coordinator route. |
| Runtime registration | No worker singleton, child supervisor, stdio handshake, fingerprint, or circuit breaker. |
| Configuration | No emitted config or active runtime lookup for `pythonExecutable`, `PYTHON`, or `MEMBRANE_FEDERATION_SCRIPT`. |
| Dependencies | No federation Python package/interpreter dependency. |
| Tests | No production-path test requires Python; equivalence fixtures are Rust-owned. |
| Documentation | Canonical ownership map names Rust modules, not Python paths. |
| Protocol | One V1 output path; no Python/Rust output variant marker. |
| Bundle | Installed artifact contains no executable federation Python source/runtime. |
| Process tree | Installed request creates no Python process. |

## 13. Implementation waves

### Wave 0 — freeze behavior and baseline

- Capture canonical outputs from all existing federation fixtures.
- Add representative real-workspace requests for every provider lane.
- Freeze provider order, warning taxonomy, omission reasons, generation behavior, scope behavior,
  source hashes, delivery modes, timing-field allowlist, warm/cold latency, and resident memory.
- Record current unsupported/degraded Audit and architecture lanes instead of treating empty output
  as healthy parity.
- Freeze installed-product failure reproduction.

Exit: legacy behavior is replayable without relying on prose or current process state.

### Wave 1 — Rust request, outcome, merge, and deadline core

- Add strict request type and canonical repository binding.
- Add `Deadline`, `ProviderKind`, `ProviderOutcome`, warning taxonomy, and deterministic assembler.
- Port pure merge/freshness-metadata logic first.
- Run golden merge, failure, freshness, scope, and receipt fixtures against Rust.

Exit: Rust converts frozen provider outcomes into byte-equivalent planner input and envelope output.

### Wave 2 — local providers

- Port Git, live, rules, and anchors.
- Reuse central Rust freshness verdict; remove Python source-generation fallback.
- Preserve path jail, symlink, race, size, cap, ledger, and delivery-mode behavior.

Exit: local-provider Rust outputs match frozen fixtures and security negatives.

### Wave 3 — resident-owned providers

- Extract shared typed application functions for Cortex candidates, skills snapshot, freshness, and
  scope-grant lookup.
- Make HTTP handlers and federation adapters call the same functions.
- Port Cortex, skills, and scope validation.
- Prove no self-HTTP recursion, store bypass, or second policy owner.

Exit: resident-owned lanes run in Rust under one deadline with exact scope behavior.

### Wave 4 — Blueprint and advisory artifacts

- Port Blueprint daemon client and generation-bound conversion.
- Define owner-produced Audit and Architect candidate projections.
- Add Rust readers for those projections with strict schema/hash validation.
- Preserve typed degradation when an owner projection is absent or stale.

Exit: all nine provider lanes execute without Python and preserve ownership boundaries.

### Wave 5 — route integration and differential qualification

- Replace `ResidentGateway` in `/federate` and CLI paths with `FederationCoordinator`.
- Run legacy and Rust implementations side-by-side only in tests over frozen and real workloads.
- Compare canonical packet, receipts, candidates, omissions, warning reasons, hashes, ordering, and
  degradation; allow only process/timing metadata explicitly frozen in Wave 0.
- Run timeout, cancellation, overload, generation-race, provider-failure, scope-leak, and restart
  fault injection.

Exit: Rust dominates legacy on mandatory gates; no semantic delta remains unexplained.

### Wave 6 — hard cut and product migration

- Delete Python gateway/providers and Python worker supervisor.
- Delete interpreter/script discovery, overrides, circuit breaker, and stdio protocol.
- Migrate workspace config to v3.
- Update canonical doctrine, generated runtime truth, installation inventory, and troubleshooting.
- Rewrite equivalence CI to use Rust fixtures and installed-product tests.
- Build, sign, notarize, install, and test artifact without workspace source mounted.

Exit: every absence check passes and installed `/federate` works with no Python available.

## 14. Verification matrix

| Acceptance ID | Requirement | Verification |
|---|---|---|
| AC-1 | Zero Python-executed federation path. | Source search, bundle inventory, process-tree assertion during installed request. |
| AC-2 | No source-checkout dependency. | Installed app test with repository source volume unavailable. |
| AC-3 | Public V1 compatibility. | Protocol roundtrip tests plus canonical legacy/Rust fixture comparison. |
| AC-4 | Deterministic provider order and first-wins dedupe. | Merge-order golden corpus under randomized completion order. |
| AC-5 | One absolute deadline. | Virtual-time and slow-provider tests proving remaining-budget propagation. |
| AC-6 | Local provider failure. | Fault injection for timeout, crash, invalid output, unavailable, generation mismatch, and cancellation. |
| AC-7 | Scope safety. | Same-root positives plus invalid, expired, inactive, cross-root, path-escape, and request-binding negatives. |
| AC-8 | Blueprint ownership. | Static import/path checks plus daemon-only integration tests; no Blueprint SQLite reads. |
| AC-9 | Cortex ownership. | Typed application API tests; no new direct store path or self-HTTP recursion. |
| AC-10 | Installed lifecycle. | Hub launch/restart/quit/upgrade tests with no orphan process. |
| AC-11 | Health truth. | Health and Hub snapshot tests distinguish resident health from federation degradation. |
| AC-12 | Performance non-regression. | Same-machine replay against frozen Wave 0 latency and memory baseline. |
| AC-13 | Canonical documentation. | Productization docs check plus source/path ownership review. |
| AC-14 | Hard-cut completeness. | All migration absence checks and clean-clone full suite. |

Required commands at implementation completion:

```text
rightkit cargo test --manifest-path engine/Cargo.toml --workspace --locked
pnpm test
pnpm test:mcp
pnpm test:equivalence
node scripts/ci/check-generated.mjs
node scripts/tools/productization/check-docs.mjs --check
```

Installed-artifact proof is additional; source tests cannot substitute for it.

## 15. Risks and dispositions

| Risk | Consequence | Disposition |
|---|---|---|
| Port reproduces Python implementation details but changes semantics. | Silent context regression. | Golden differential corpus and canonical JSON comparison before deletion. |
| Async conversion blocks Tokio workers on Git/file IO. | Resident saturation and deadline misses. | Bounded blocking lane, cancellation tests, existing workload ingress limits. |
| Direct in-process Cortex access creates a second policy path. | Ownership drift. | Shared application function used by HTTP and federation; planner remains sole final owner. |
| Blueprint adapter reimplements repository semantics. | Duplicate/stale truth. | Daemon-only typed protocol; static prohibition on Blueprint store/parser imports. |
| Audit/Architect adapters depend on private stores. | Cross-repository coupling. | Owner-produced versioned CCS projections; unavailable projection degrades locally. |
| Rust and Python coexist indefinitely. | Two truths and permanent maintenance cost. | Hard-cut mode, no shipping flag, deletion gate in Wave 6. |
| Health remains green while federation is unusable. | False operational confidence. | Add federation capability state to detailed health and Hub snapshot. |
| Config migration strands existing installs. | Hub startup failure. | Atomic installer/updater v2-to-v3 migration plus installed upgrade test. |
| Error wording drifts while reason tokens appear stable. | Consumer and diagnostic mismatch. | Freeze exact public tokens and structured fields; prose may change only when not contracted. |

## 16. Rejected alternatives

### Bundle and pin Python

Rejected as final architecture. It repairs current incident but retains interpreter packaging,
child supervision, two language toolchains, hidden stderr, source inventory, and release coupling.

### Rust coordinator with Python providers

Rejected. It removes only `gateway.py`; Python remains on the request path and preserves the same
installed-product failure class.

### Python sidecar service

Rejected. It adds another resident authority and process plane, contradicting Hub ownership and
increasing lifecycle, security, health, and distribution cost.

### Dynamic Rust plugin/WASM provider system

Rejected. Provider set is fixed and product-owned; an ABI/runtime adds complexity without a current
requirement.

### Rewrite final planner during port

Rejected. It destroys the differential oracle and mixes language migration with policy change.

## 17. Decision record

```yaml
schema: architecture-decision.v1
decision_id: MEM-ADR-RUST-FEDERATION-001
decision_status: accepted
realization_status: partially_realized
superseded_by: migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md
remaining_closure: N7 and N10
date: 2026-08-22
owner: Membrane runtime
decision_authority: Adrian D'souza
decision_question: How will Membrane remove Python from its installed federation path without changing protocol or context policy?
alternatives:
  - id: C-1
    description: Full in-process Rust federation port
    disposition: selected
  - id: C-2
    description: Bundle and pin Python
    disposition: rejected
  - id: C-3
    description: Rust coordinator with Python providers
    disposition: rejected
  - id: C-4
    description: Python sidecar service
    disposition: rejected
  - id: C-5
    description: Dynamic Rust plugin or WASM provider runtime
    disposition: rejected
decision: Replace Python gateway, provider adapters, and worker lifecycle with one fixed in-process Rust FederationCoordinator in membrane-runtime.
rationale: Removes an unnecessary process and toolchain boundary while preserving existing owners, protocol, planner, deadline, and degradation semantics.
reversibility_and_exit: Before hard cut, revert source changes. After release, rollback to previous signed artifact; no production Python switch remains.
review_triggers:
  - A real external provider requires an independently deployable ABI.
  - Public V1 cannot represent required provider behavior.
  - Differential fixtures prove current Python behavior conflicts with canonical doctrine.
  - Blueprint changes its owned daemon contract.
```

## 18. Completion rule

The port is complete only when Rust federation is installed-product proven, every semantic and
safety gate passes, canonical ownership names Rust, and Python runtime absence is demonstrated.

“Rust path exists,” “unit tests pass,” “Python fallback remains,” or “developer workspace works”
are not completion states.
