# Membrane Native Rust Migration, Runtime Closure, and CodeRight Integration Specification

**Status:** Canonical / authoritative migration specification
**Date:** 2026-08-25
**Repository:** `Orthic-Labs/Membrane`
**Historical source-audit baseline:** `8a215ac6fab11cc24bb821507057743b7898e09f` (Sections 0.1 and 21 preserve that dated audit; they are not current implementation status)
**Current implementation status reviewed:** integration tree at `51f98189da769ed005c516e2a2ea93e61678e0da` (2026-08-26; see Section 0.1.1)
**Baseline federation cutover commit:** `5a9175b9518ca6d36dca3c7c176bddeca070f5e3`
**Audience:** Membrane, Adapt, transcript, Cortex, Blueprint, Hub, CodeRight, host-adapter, build, CI, and release-engineering implementers
**Supersedes:** `MEMBRANE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md` dated 2026-08-22, `MEMBRANE-LEGION-ABSORPTION-BRIEF.md` for Membrane work, `docs/plans/2026-08-22-full-rust-federation-port-architecture.md` for federation execution and closure, the runtime/process/cutover portions of `docs/design/membrane-live-diagnostics-final-architecture.md` and `docs/plans/membrane-live-diagnostics-final-architecture-revised.md`, conflicting Membrane portions of bounded dispatch plans, and any documentation that treats a Python/Node Membrane-owned runtime path as a valid final state. Live Diagnostics capability semantics remain reference input unless canonical subsystem doctrine says otherwise.
**Companion semantic authority:** `docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md`


## Scope boundary

This document exclusively owns native runtime/process topology, packaging and deletion closure, Blueprint and CodeRight integration seams, migration sequencing, and native-only qualification. It does not define Adapt product semantics, Taste/Insights authority, Cortex internals, or Blueprint repository semantics. Those stay with their subsystem canonical documents.

---

## 0. Executive decision

Membrane's production implementation is **native Rust end to end**.

That decision applies to the **whole Membrane product**, not only to federation.
The six canonical axes are Pull, Push, Cortex, Blueprint, Ledger, and Adapt.
Generated runtime truth may continue to expose the historical `Guide` name until
the verified rename cutover lands; that is implementation status, not a second
canonical axis. A migration cannot be called complete merely because federation
moved to Rust while another canonical subsystem still executes Python or Node.

The final product therefore obeys all of the following:

1. **Membrane-owned runtime authority, policy, state transitions, storage, and effects execute as native Rust or are declarative data.** Installed Hub presentation may use only the bounded OS-WebView exception in §1.5.
2. **No installed Membrane-owned runtime implementation requires Python, npm, npx, pip, a virtual environment, or dynamically interpreted Membrane-owned source.** Blueprint's pinned installed runtime is a separately inventoried subsystem component; only Hub may keep its Node service/watcher resident, and direct Hub-off access is one bounded operation. Bounded WebView assets execute in the OS-provided WebView.
3. **CodeRight consumes Membrane through exactly one compatible active-Hub binding.** It does not embed a second Membrane backend or open a local fallback store.
4. **External products and target-project tools are reached only through explicit typed boundaries.** They do not justify an internal language worker.
5. **Development-only Python/JavaScript is permitted only when it is provably absent from installed execution paths and release artifacts.** Installed JavaScript is limited to §1.5's presentation sandbox plus Blueprint's exact hash-bound runtime tree under its lifecycle contract.
6. **A Rust implementation existing beside an executable Python/Node fallback is not completion.** Final production is `native-only`, not `native-with-legacy-available`.

### 0.1 Historical source-audit baseline verdict

At the historical repository baseline `8a215ac6...`, Membrane was **not yet native-only**.
This subsection preserves the source audit that motivated the migration. It must not be used as
the current implementation ledger; current package status is in Section 0.1.1.

The audit established this historical state:

| Surface | State at historical baseline | Canonical disposition |
|---|---|---|
| Federation request/fan-out/merge production route | Native Rust, same-process, authoritative | Keep Rust; finish legacy deletion |
| Legacy federation Python gateway/providers | Still present for shadow/qualification | Transfer fixtures, then delete executable Python |
| Adapt | Python CLI + Python runtime package; installed workflow points at Python source | Port product behavior to Rust; delete Python runtime |
| Transcript normalization | Current Python package `continuity.transcript` | Port to shared native Rust transcript crate/module; delete Python package after consumer cutover |
| Cortex | Rust | Keep Rust |
| Hub lifecycle/service authority | Native Rust/Tauri with installed OS-WebView JavaScript presentation | Keep lifecycle, state, authority, effects, and action validation in Rust; classify UI scripts under bounded-presentation policy and prove they cannot own runtime semantics/effects |
| Live Diagnostics resident core | Rust contracts, evaluator, supervisor, providers, reconciliation, and Hub/runtime routes | Keep native; preserve exact epoch/hash/root binding, no-false-clean, planner/host separation, and parent-health independence |
| Live Diagnostics host enforcement | Installed Node hook entrypoint, command classifier, current-worktree manifest, reconciliation client, and completion/test/build fence | Port Membrane-owned enforcement helpers to native Rust; installed host configuration invokes a native adapter; retain JS only as release-excluded differential coverage |
| MCP server | Generated architecture names `mcp/server.mjs` as source of truth for 17 tools, including seven diagnostic tools; native `membrane-mcp` advertises none because native execution is not implemented | Cut the complete registry-defined production MCP surface to `membrane-mcp` Rust; advertise a tool only when its native executor exists |
| Context renderer/budget implementation | Generated architecture still names `mcp/context-renderer-lib.cjs` as source of truth, with a Rust mirror | Make Rust authoritative; demote/delete production CJS path |
| Blueprint | Owns generation/hash-bound D0 findings and remains one of six product axes | Hub-hosted, not independently resident: watcher runs only under Hub, Hub-off access is a bounded one-shot operation; Membrane uses one typed native client, never imports Blueprint internals, and never bootstraps Node |
| Python/JS benchmark, migration, and repository-maintenance scripts | Mixed | May remain dev-only with machine-verifiable exclusion from release/runtime |
| Root CI | Node-oriented; does not itself prove interpreter-free installed operation | Add independent native-only installed-artifact gate |

**Historical uncommitted worktree candidates observed during that revision:**

- `engine/crates/membrane-transcript/` contains a substantial candidate native transcript crate;
- `engine/crates/membrane-adapt/` contains a substantial candidate native Adapt crate.
- `migration/native-rust/runtime-policy.json` and `runtime-language-manifest.json` are uncommitted
  candidate policy/ledger artifacts; the tracked `executable-ledger.json` contains contradictory
  legacy classifications and `invocation-graph.json` is incomplete for current runtime paths.

They are not part of committed baseline `8a215ac6...` and are not completion evidence. N0/N2/N3
must disposition them before implementation: adopt and integrate them, explicitly supersede them
with preservation/deletion receipts, or stop on ownership conflict. No executor may create a
second native owner beside either candidate.

The previous migration was therefore **a successful federation port but an incomplete product-runtime migration**.

### 0.1.1 Current implementation status

Status below is current through integration tree at
`51f98189da769ed005c516e2a2ea93e61678e0da` (2026-08-26). Source-built/copied-binary
test is not exact released-package qualification.

| Package | Status | Integrated evidence / remaining gate |
|---|---|---|
| N0 | **DONE** | Canonical invocation graph, runtime-language manifest/policy, & legacy-ledger reconciliation are checked in; 29 runtime-language blockers are closed, with zero production interpreter rows. Graph/manifest gates pass with no errors or warnings; installed receipts remain separate release evidence. |
| N1 | **DONE** | Six hashed language-neutral internal contracts, examples, policy, and recurring contract gates are checked in and pass. |
| N2 | **PARTIAL** | Native `membrane-transcript` owner, raw host discovery/parsing, exact provenance, receipts, & conformance tests pass; all consumers use native seam & Python `continuity` is excluded from production graph. Final deletion/exclusion receipt & installed qualification remain open. |
| N3 | **DONE** | Native deterministic Adapt core, authority/admission, scope/lifecycle, semantic sealing, manifests, multiwriter behavior, Insights report-only controls, and fail-closed tests pass. |
| N4 | **DONE** | Native proposal/review/adjudication/apply path is receipt-bound. The explicit user-selected transcript workflow requires exact source hash/rebinding and required review; automatic implicit host-signal evaluation remains a separate optional lane. |
| N5 | **PARTIAL** | Native Adapt source, CLI surfaces, persistence/delivery, & copied source-built-binary qualification have landed. Exact installed qualification & replacement or explicit dev-only demotion of `scripts/run-adapt-installed-current.mjs` remain pending receipts. |
| N6 | **PARTIAL** | Native MCP/renderer/host-fence implementation has landed; installed conformance, Node-absent, & host-configuration receipts remain pending. |
| N7 | **PARTIAL** | Federation Python/shadow deletion & configuration cutover implementation have landed; installed deletion & upgrade/rollback receipts remain pending. |
| N8 | **PARTIAL** | Blueprint packaging/runtime-boundary implementation has landed; Hub-coupled watcher and bounded one-shot availability are defined, while installed receipts remain pending. |
| N9 | **PARTIAL** | Membrane's strict active-Hub health, identity fence, memory routes, diagnostics pipe, and typed unavailable seams have landed; Membrane-side integration tests & installed receipts remain pending. No CodeRight repository change is in scope. |
| N10 | **BLOCKED / NOT SEALED** | Native-only seal has not been issued. It still waits for N2 receipt evidence, N5-N9 installed receipts, & remaining Section 17 package/SBOM/process-tree/native-only gates. |

### 0.2 Why the previous migration missed Adapt and transcript normalization

The 2026-08-22 specification stated the correct product-wide rule — removal of Python and Node from Membrane's production dependency graph — but the implementation plan narrowed the concrete deletion packets around federation. `MEM-029` and `MEM-030` targeted the Rust↔Python federation bridge, gateway, providers, and implementation tests. There was no equivalent native migration/deletion track for Adapt or the current transcript package.

That was the control failure:

- the architectural invariant was repo-wide;
- the executable migration ledger and dispatch plan became federation-centric;
- Adapt was treated operationally as a Python tool/package even though product truth lists it as a canonical Membrane subsystem;
- the `continuity` Python package was treated as a supporting library even though Adapt depends on its authoritative transcript normalization;
- native-only qualification was not made an unavoidable release gate before subsequent Adapt feature work continued.

After the Rust decision, Adapt received substantial new Python production work (`f602fbba...`, then `7c05b49...`). That must not happen again. This document adds structural gates that make the product/runtime boundary mechanically enforceable.

### 0.3 Non-negotiable completion rule

The migration is complete only when **every** item below is true:

- [ ] No shipped Membrane binary, shim, service, scheduler, host adapter, or installed configuration launches Python or Node to execute Membrane-owned logic.
- [ ] No installed Membrane runtime/authority path imports or depends on executable `.py`, `.js`, `.mjs`, `.cjs`, or `.ts` Membrane-owned source; §1.5 bounded presentation assets are the only installed exception.
- [ ] Federation production behavior is Rust and the executable Python federation implementation has been deleted.
- [x] Adapt production behavior is Rust; its CLI, mining, authority/admission, manifest, application, and scheduling paths require no Python.
- [ ] Transcript parsing/normalization required by Membrane is Rust and the production Python package is deleted.
- [ ] The canonical MCP server and context renderer are Rust.
- [ ] The complete registry-defined MCP tool set, currently 17 tools, has native executors before native advertisement; unsupported native tools remain unadvertised during migration.
- [ ] Live Diagnostics resident behavior, operational contracts, provider supervision, reconciliation, and deterministic gate evaluation remain native Rust.
- [ ] Installed host enforcement invokes native code for mutation sealing/observation, worktree manifest generation, exact reconciliation, command classification, and completion/test/build blocking; no installed hook requires Node.
- [ ] Live Diagnostics remains a Membrane module under Hub, not a seventh subsystem; provider degradation cannot determine parent Membrane health.
- [ ] No runtime documentation or generated host configuration points users/hosts at an interpreter-backed Membrane path.
- [ ] A separately bounded external product may be unavailable without causing Membrane to spawn or bootstrap its language runtime; the result is a typed degraded/unavailable state.
- [ ] CodeRight binds to exactly one compatible active-Hub Membrane capability,
  opens no embedded/local fallback store, and receives typed unavailability
  when Hub is inactive.
- [ ] Exact installed artifacts pass qualification with `python`, `python3`, `node`, `npm`, `npx`, and `pip` absent from `PATH` and without development checkout paths.
- [ ] Package/SBOM inspection proves no undeclared interpreter/runtime payload is shipped for Membrane-owned behavior.
- [ ] Every installed WebView script is declared as `bounded-presentation`, ships without Node,
  and passes the §1.5 capability/import/effect restrictions.
- [ ] The process tree proves no Membrane-owned interpreter child appears during install, first launch, MCP, federation, Adapt learning, memory, upgrade, rollback, or uninstall.
- [ ] Every legacy executable path has either a deletion receipt or a dev-only exclusion proof.

A release MUST NOT carry a `native-only` seal while any item above is false.

---

## 1. Normative language and classification rules

The terms **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are normative.

### 1.1 Production behavior

An executable/source path is **production behavior** if any of the following is true:

- it implements a subsystem named in canonical product truth;
- an installed CLI, service, Hub action, MCP server, host adapter, scheduler, daemon, or shim invokes it;
- packaging or release manifests include it because an installed feature needs it;
- generated user/host configuration points at it;
- runtime documentation instructs users to execute it as the normal product path;
- it performs normal authoritative reads/writes, admission, policy, retrieval, parsing, scheduling, or mutation for the installed product;
- another production component imports it or launches it;
- disabling/removing it would make a documented installed feature stop working rather than merely removing developer diagnostics/evaluation.

A path does **not** become dev-only because it lives under `scripts/`, `eval/`, `tests/`, `tools/`, or a nested package. Invocation determines classification.

### 1.2 Dev-only executable

A Python/JavaScript executable may remain after native migration only when all of these are proven:

- no production caller/import/launch site reaches it;
- it is excluded from install/package/SBOM runtime payloads;
- no host config, user shim, scheduler, or service points to it;
- it is not needed for `membrane`, Hub, MCP, Adapt, Cortex, Ledger, Pull, Push, or normal Blueprint integration to operate;
- native-only installed-artifact qualification passes with the interpreter absent;
- it has a row in canonical `migration/native-rust/runtime-language-manifest.json` with
  disposition `dev-only`, graph-derived non-reachability, and machine-verifiable exclusion evidence.

Optionality is not an exemption. If an optional feature is shipped, documented, or callable as part of Membrane, it must be native, declarative, separated as another product through a typed boundary, or deleted.

### 1.3 External product/tool

An external executable is permitted only when it is **not Membrane-owned runtime implementation** and is called through an explicit boundary with:

- typed purpose and owner;
- executable identity/version policy;
- capability authorization where effects are possible;
- canonical working directory and environment filtering;
- timeout/cancellation/process-tree cleanup;
- output bounds;
- stable typed failure/coverage-gap representation;
- receipt/provenance sufficient to distinguish the external tool from Membrane-owned behavior.

Examples include a compiler, linter, scanner, VCS executable where a Rust
library is not appropriate, or a Blueprint-owned runtime package while it is
hosted under Hub lifecycle during migration. Independent Blueprint residency is
not an external-tool exception. Spawning Python to run Adapt or federation is
never an external-tool exception.

### 1.4 Self-contained

Self-contained means **one native runtime inside the active Hub process and one
signed product distribution**. There is no standalone Membrane runtime and Hub
MUST NOT supervise a Membrane child process for isolation or any other reason;
when Hub is inactive there is no Membrane runtime at all.

CodeRight binds to Membrane through an active Hub. There is no embedded
CodeRight Membrane backend and no local fallback store; when Hub is inactive,
Membrane-requiring CodeRight operations return typed unavailability.

### 1.5 Bounded installed presentation

Tauri Hub may ship HTML, CSS, and JavaScript executed only by the OS-provided WebView. This is a
`bounded-presentation` artifact class, not permission for a Node runtime or a second application
plane.

A bounded-presentation artifact MUST:

- receive typed read models and submit typed intents through generated Tauri bindings;
- leave health composition, policy, authorization, capability grants, receipt validation, storage,
  lifecycle, process launch, filesystem access, network access, and effects in Rust;
- contain no Node imports, dynamic code loading, package-manager access, hidden transport, direct
  durable writes, or source-checkout path resolution;
- ship as content-hashed static assets listed in the runtime-language manifest and package/SBOM;
- run under a restrictive Tauri capability/CSP profile;
- pass a machine check over imports, globals, capabilities, generated bindings, and package paths;
- fail as presentation unavailability without changing resident Membrane health or authority.

Any Hub script that owns business/runtime semantics, performs an effect, or requires Node is
`native-port`, not `bounded-presentation`.

---

## 2. Canonical product topology

### 2.1 Logical target

```text
                          CodeRight / hosts
                                   |
                                   | typed Hub binding
                                   v
+-----------------------------------------------------------------------+
|                    Active Membrane Hub process                         |
|                                                                       |
|                         Membrane native core                           |
| Pull / Federation   Push / Reduction   Adapt / Learning               |
| Ledger / Navigation Cortex / Durable Knowledge  Blueprint client      |
| transcript normalization | authority/admission | lifecycle | receipts |
+----------------------+---------------------+----------------------------+
                       |                     |
                       v                     v
             native Cortex store      Hub-hosted Blueprint role and
                                      external typed target tools

Host surfaces (all stateless Hub clients; none may launch or auto-start anything):
  native `membrane` CLI
  native `membrane-mcp` stdio adapter
  native Hub integration
  declarative host configuration
```

There is no Python/Node Membrane worker in this topology.

### 2.2 Native crate/module responsibilities

Exact crate names may vary, but ownership MUST be equivalent to the following:

| Native owner | Responsibility |
|---|---|
| `membrane-protocol` | Stable requests, responses, receipts, persistence/wire contracts, version negotiation, canonical serialization |
| `membrane-runtime` | In-Hub composition, CLI/API modes, lifecycle boundaries, dependency injection; never a standalone resident runtime |
| `membrane-core` | Shared planning/rendering/budget/reconciliation logic that must have one authoritative implementation |
| `membrane-provider-sdk` | Typed provider/source interfaces and testkit |
| `membrane-federation` | Request validation, deadline, scope/freshness binding, fan-out, generation coherence, merge, warnings/omissions |
| `membrane-transcript` (current candidate to adopt/reconcile) | Canonical transcript discovery/adapters, `TranscriptEventV1`, byte spans, provenance, parser receipts, typed unavailable/error semantics |
| `membrane-adapt` (current candidate to adopt/reconcile) | Taste/Insights mining, authority/admission, manifest, semantic validation, lifecycle, application receipts, native Adapt CLI/service surface |
| Cortex crates | Durable memory, embeddings, vector/lexical retrieval, lifecycle/persistence |
| Ledger native module | Rebuildable document navigation and hash-bound section projections; no document truth |
| `membrane-mcp` | Native stdio MCP adapter over the same Rust API; no duplicate implementation |
| `membrane-client` | Typed client for the active Hub binding; no embedded or alternate resident backend |
| Hub | Sole resident lifecycle/install/update authority, including Blueprint residency; no Membrane child process |

Live Diagnostics is implemented inside `membrane-protocol`, `membrane-runtime`, `membrane`, and
the native host/MCP adapters. It is not a new subsystem crate required for naming symmetry. Extract
a dedicated crate only if an independently versioned contract or lifecycle boundary later makes
that cheaper than colocation.

### 2.3 One implementation per semantic contract

During and after migration, the following MUST NOT have two independently evolving authoritative implementations:

- budget reconciliation;
- scope-grant validation;
- authority/admission;
- transcript canonicalization;
- candidate normalization/identity;
- memory backend selection;
- lifecycle resolution;
- MCP tool contract behavior;
- product-truth enumeration;
- native/runtime classification.

Legacy implementations may serve as frozen differential oracles only until their deletion gate passes.

---

## 3. Six-axis disposition

The current product truth enumerates six axes. This section makes their runtime disposition explicit.

| Axis | Native target | Runtime rule |
|---|---|---|
| **Pull** | Rust federation, admission, planner-facing evidence | No Python/Node provider worker; external sources only through typed interfaces |
| **Push** | Rust reduction/reconstruction/budget logic | Canonical renderer/reconciliation in Rust; CJS mirror cannot remain authoritative |
| **Cortex** | Existing Rust engine/store | No competing Python memory writer; remote use only through typed client when selected |
| **Blueprint** | Typed Rust client/interface; Hub-hosted native implementation | Independently usable, not independently resident; watcher only under Hub; Hub-off access is bounded one-shot; absence degrades explicitly |
| **Ledger** | Rust record/index/navigation implementation | No interpreter-backed runtime dependency |
| **Adapt** | Rust learning subsystem over native transcript + Cortex interfaces | Python package/CLI/shims/scheduler are migration scaffolding only |

The runtime ledger MUST additionally cover Hub, MCP, client adapters, installers/updaters, generated host configuration, and release payloads because those surfaces can reintroduce interpreters even though they are not one of the six semantic axes.

Live Diagnostics is a cross-cutting Membrane runtime capability, not a seventh axis. Its resident
service, provider supervision, operational contracts, host adapters, and enforcement paths are
still Membrane-owned production behavior and therefore fall under the native-only rule.

---

## 4. Federation: preserved semantics and remaining closure

### 4.1 Current status

Commit `5a9175b...` made the production `/federate` route native and same-process. The Python resident worker remains only as a shadow qualification adapter. Therefore:

- **native production federation cutover is complete;**
- **native-only federation cleanup is not complete** while executable `engine/federation/*.py`, Python provider modules, worker/shadow selection, or Python-specific packaging/tests remain reachable.

`MEM-ADR-RUST-FEDERATION-001` remains accepted and is **partially realized**: commit `5a9175b...`
landed the production native route, while N7 owns hard-cut deletion, configuration migration, and
qualification closure. Its original plan is historical execution detail, not a competing active
plan.

### 4.2 Required request-boundary behavior

Rust MUST preserve/version deliberately:

- request schema validation;
- canonical repository-root resolution;
- release-generation validation;
- freshness acquisition;
- ScopeGrant lookup and fail-closed request-context validation;
- client/task/session/root/nonce/manifest binding;
- anchor normalization;
- one absolute request deadline;
- typed error taxonomy and deterministic response shape.

### 4.3 Fan-out behavior

Federation includes the nine lanes:

1. anchors;
2. Blueprint;
3. rules;
4. live files;
5. Git;
6. audit;
7. architect/decisions;
8. skills;
9. Cortex memory.

Required properties:

- concurrency under one shared deadline;
- generation coherence where source generations matter;
- explicit partial/degraded/unavailable outcomes;
- deterministic warnings/omissions;
- no hidden dynamic import or interpreter fallback;
- typed source interfaces for cross-product lanes.

Audit and Architect/decision lanes MUST preserve the owner-produced projection contract and typed
`provider_capability_missing` outcome frozen in
`migration/native-rust/federation-contract-inventory.json` and
`migration/native-rust/fixtures/provider-cases.v1.json`; Membrane MUST NOT synthesize a second
owner projection.

### 4.4 Merge behavior

Preserve:

- candidate schema validation;
- stable identity;
- deterministic dedupe/order;
- trust and instruction policy;
- score components;
- token accounting;
- exact/protected/recoverable semantics;
- resolver/provenance;
- generation admission;
- no silent provider loss.

### 4.5 Mandatory federation invariants

**F-01 — Scope enforcement fail-closed.** Missing/malformed/expired/revoked/mismatched required grant prevents the affected operation.

**F-02 — One absolute deadline.** Queueing and prerequisite work consume the same request deadline.

**F-03 — Transitive cancellation.** No provider task or external call survives cancellation/final response.

**F-04 — Generation coherence.** Mismatched source generations are omitted explicitly, never opportunistically merged.

**F-05 — Release generation validation.** Mismatch is handled before candidates enter planner admission.

**F-06 — Explicit degradation.** Expected provider yields complete, partial-with-metadata, or omission; `[]` alone is not failure information.

**F-07 — Deterministic merge.** Identical inputs/snapshots/config/clock controls yield byte-stable canonical output.

**F-08 — Trust/provenance preservation.** Defaults may not raise authority.

**F-09 — Planner separation.** Federation returns typed candidates; planner/budget owns final model-visible selection.

**F-10 — No internal interpreter spawn.** Membrane-owned provider logic is not executed through Python/Node/shell source.

### 4.6 Legacy federation deletion

After current native parity/qualification is accepted:

- delete `engine/federation/gateway.py`;
- delete executable Python provider implementations under `engine/federation/providers/`;
- delete Python-worker discovery/handshake/stdio/restart code after no shadow consumer remains;
- retain language-neutral fixtures/golden data;
- delete implementation-only Python tests after equivalent Rust contract coverage exists;
- remove runtime config/packaging/env variables whose sole purpose is the Python worker;
- remove hidden `legacy`/`shadow` selectors from production builds.

A separate developer-only comparison tool may consume archived fixtures; it MUST NOT make the old implementation selectable by the installed product.

Workspace configuration cutover is part of N7, not optional cleanup:

- installed configuration MUST use schema v3 with `schemaVersion: 3` and canonical
  `workspaceRoot` only;
- installer/updater migration MUST atomically retain the canonical root, remove
  `pythonExecutable`, emit a receipt, and be idempotent under replay;
- an unmigrated or invalid legacy configuration reaching runtime MUST fail with typed
  `workspace_config_migration_required`, never silently fall back to a source checkout or
  interpreter;
- installed upgrade and rollback qualification MUST cover v2-to-v3 migration and repeated
  migration.

---

## 5. Native transcript substrate

### 5.1 Current role

`continuity.transcript` is currently the canonical Python transcript-normalization layer for Claude/Codex and emits deterministic `TranscriptEventV1` events, source byte spans, parser receipts, and typed `TranscriptUnavailable` failures. Adapt relies on this class of semantics.

That makes the current Python package part of the production dependency graph when Adapt is enabled. It cannot remain Python in the final product.

### 5.1.1 Product disposition

At baseline, `continuity/` is not an independent product or a seventh Membrane subsystem. Its Python package exports only transcript normalization contracts, so its production semantics move wholly into the native transcript owner and the Python package is deleted after consumer cutover.

N0 must inventory every real consumer before deletion. Any consumer beyond Adapt migrates to the same native contract; discovering one delays deletion only until its adapter passes conformance and does not justify retaining Python production behavior.

`mcp/host/continuity.mjs` is a separately owned host checkpoint adapter, not a consumer of the Python package and not evidence of an independent Continuity product. Its native/runtime disposition belongs to N6 with MCP/host cutover.

### 5.2 Native owner

Adopt `membrane-transcript` if the current uncommitted candidate lands and passes N0 ownership,
workspace-integration, contract, and qualification checks; otherwise create one equivalent Rust
module after an explicit supersession disposition. Exactly one native owner exists. It owns:

- host/source discovery;
- host adapters;
- canonical transcript byte loading;
- deterministic `TranscriptEventV1` construction;
- exact source byte spans;
- role/provenance classification;
- source/session/installation identity;
- parser version/digest receipts;
- redaction/synthetic/meta flags;
- typed `TranscriptUnavailable` and parse errors;
- bounded context-window extraction used by Adapt.

It MUST NOT own preference inference or authority decisions. Those belong to Adapt.

### 5.3 Transcript invariants

**T-01 — Exact-source binding.** An event used as authority/evidence must be recoverable to its exact source bytes/spans.

**T-02 — Provenance is explicit.** `external_user`, assistant, tool, repository output, synthetic, redacted, private-reasoning-omitted, and unknown origins cannot collapse into a single text stream.

**T-03 — Omission is not empty success.** Missing/inaccessible/corrupt transcript input produces typed unavailability/degradation.

**T-04 — Deterministic parsing.** Identical bytes + parser contract produce identical canonical events/IDs/spans.

**T-05 — Host adapters are conformance-tested.** A host is not advertised as supported merely because a generic parser enum exists. Discovery + parse + identity + provenance fixtures must pass.

**T-06 — Parser version is receipt-bound.** Cached extraction is invalidated when the parser contract changes.

### 5.4 Migration strategy

1. freeze language-neutral transcript fixtures from current Claude/Codex cases plus malformed/truncated/cross-machine cases;
2. implement native parser/event model;
3. differential-test Python vs Rust canonical event streams;
4. explicitly version intentional differences;
5. switch Adapt to Rust transcript API;
6. remove production imports of `continuity` Python;
7. delete the Python package after all inventoried consumers cut over; only language-neutral fixtures or independently useful dev tools may remain with dev-only exclusion proof.

---

## 6. Adapt native runtime cutover

`docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md` exclusively defines Taste, Insights, evidence authority, proposal eligibility, lifecycle, evaluation, and feature dependencies. Migration code and this plan MUST consume those contracts rather than restating them.

Native runtime ownership includes:

- transcript/evidence ingestion from the native transcript seam;
- deterministic authority, scope, conflict, sealing, lifecycle, and eligibility enforcement;
- typed model/proposal execution without authority laundering;
- one typed Cortex durable-admission client plus delivery/effect receipts;
- native CLI and Hub scheduling;
- language-neutral golden fixtures and Python-vs-Rust differential tests during migration.

No new production Python/Node Adapt behavior is allowed. Existing Python may serve only as migration input, bounded differential oracle, or release-excluded evaluation tooling with an explicit deletion/exclusion gate. N1 freezes semantic fixtures, N2 ports transcript normalization, N3-N5 port Adapt runtime, and N10 proves Python/Node absence.

---

## 7. MCP and context-renderer native cutover

### 7.1 Current issue

Generated architecture at baseline still names:

- `mcp/server.mjs` as the MCP server source of truth;
- `mcp/context-renderer-lib.cjs` plus `engine/crates/membrane-core/` as cross-provider budget implementation, while explicitly calling the CJS file the source of truth.

That contradicts the final native-runtime rule even though Rust MCP/core crates already exist.

### 7.2 Canonical target

`membrane-mcp` becomes the sole production implementation of the complete registry-defined MCP
surface over stdio. At baseline that surface contains 17 tools: ten prior context/memory/Blueprint
tools plus seven Live Diagnostics tools. The registry, generated product truth, native discovery,
and native execution table MUST agree. Native discovery MUST NOT advertise a tool until its native
executor is wired and conformance-tested.

`membrane-core` becomes the sole production implementation of:

- context block rendering semantics;
- budget-lane classification;
- selected/delivered token reconciliation;
- stable alert ordering/IDs;
- native/resolver-backed/metadata-only/rendered lane accounting;
- receipt schema emission.

JavaScript/CJS implementations may remain temporarily as differential oracles/tests, then must either be deleted or classified dev-only and made unreachable from installed Membrane.

### 7.3 MCP parity requirements

The native MCP cutover MUST preserve:

- tool names and JSON schemas;
- error/result envelopes;
- stdio framing and protocol compatibility;
- client-adapter capability levels;
- authorization/capability checks;
- context receipts and budget reconciliation;
- resolver behavior;
- dual-era compatibility only where still explicitly supported;
- deterministic outputs under fixed inputs.
- Live Diagnostics workspace/mutation/snapshot/fence/capability/baseline/provider semantics;
- exact operational schema/version behavior without mutating the five public context V1 shapes;
- toolset negotiation for `default`, `memory`, `blueprint`, and `diagnostic` groups;
- honest non-advertisement of any unsupported native tool during staged cutover.

Host configurations generated after cutover MUST point to native executables, never `node mcp/server.mjs` or equivalent.

### 7.4 Node development tooling

Root CI, documentation generators, repository-maintenance scripts, and test harnesses MAY still use Node if they are dev-only. The final release gate is about **installed Membrane-owned behavior**, not aesthetic source-language purity.

However, a Node script that generates runtime configuration must itself generate only native runtime targets.

### 7.5 Live Diagnostics host cutover

The merged resident diagnostics core is already native Rust. Native closure therefore does not
reimplement its evaluator, supervisor, providers, or service routes. It removes interpreter-backed
Membrane-owned adapters around that core.

Native host integration MUST preserve:

- canonical project-root and worktree identity binding;
- mutation begin/seal/register-observed semantics;
- deterministic current-worktree manifest generation, including deleted/unreadable paths;
- exact `/diagnostics/reconcile` behavior before enforced verification/completion boundaries;
- quote-aware verification-command classification without compound-command bypass;
- fail-closed behavior for missing status, root mismatch, Git inspection failure, transport
  failure, stale/superseded evidence, unknown outcomes, or uncleared fence state;
- enforcement-disabled non-interference;
- transactional CodeRight integration and honest observed/reconciliation-only host modes.
- every diagnostic observation and its exact schema/identity while grouping only observations
  confidently correlated to the same issue; uncertain correlations remain separate;
- a hard `maxCost` ceiling: providers MUST NOT run above it, and required coverage that cannot fit
  becomes typed unknown/invalid rather than an implicit cost escalation.

Installed host configuration MUST invoke a native executable or linked API. JavaScript hook files
may remain only as release-excluded conformance fixtures/oracles after native cutover.

---

## 8. Blueprint boundary

### 8.1 Settled ownership

Blueprint owns durable repository truth, graph identity/generations, structural indexing, and drift/change observation. Membrane consumes that truth through a typed interface.

Membrane MUST NOT duplicate Blueprint's repository graph merely to remove a process boundary.

### 8.2 Installed lifecycle

Membrane packages one pinned Blueprint runtime. Hub owns its resident service &
watcher lifecycle, publishes its typed endpoint, & drains both on shutdown.
Hub-off Blueprint access is one bounded operation from installed bytes. No
external provisioning, workspace discovery, independently resident daemon, or
hidden fallback is part of installed topology.

### 8.3 Final packaging decision

Blueprint retains an independently versioned package and protocol, but not an
independently resident service. The signed Membrane installer includes one
pinned, digest-recorded Blueprint runtime component so no external provisioning
is needed for qualification; that component remains reusable by later standalone
package channels. While Hub is active, Hub owns Blueprint residency and hosts
its watcher/query role; Membrane reaches it through one typed native client and
never opens Blueprint SQLite directly. When Hub is inactive, Membrane returns
typed `membrane_unavailable { hub_inactive }` and does not fall back. An explicit
direct Blueprint request may use a bounded one-shot operation that publishes
transactionally and exits.

---

## 9. Runtime, process, and effect policy

### 9.1 Allowed process boundaries

Allowed:

1. Hub -> a Blueprint-owned runtime role whose residency ends with Hub and that never registers an independent OS service;
2. an explicit direct Blueprint client -> a bounded Hub-off one-shot operation that publishes transactionally and exits;
3. a governed Rust effect executor -> approved external target/project tools.

Forbidden:

- Rust -> Python/Node worker for Membrane-owned logic;
- provider-local ad hoc `subprocess`/`child_process` calls;
- dynamic imports from workspace paths;
- same-process capability reached through loopback self-calls when a direct API exists;
- shell interpolation to execute product logic;
- detached children without cancellation/process-tree ownership;
- hidden CLI re-entry to cross an internal module boundary.

### 9.2 Central external-process rule

Any permitted external process launch must flow through one governed native boundary with:

- typed purpose/owner;
- executable allowlist/identity;
- cwd normalization;
- explicit environment allowlist;
- no shell interpolation;
- absolute deadline/cancellation;
- stdout/stderr byte caps;
- process-group/job-object cleanup;
- immutable receipt;
- authorization before effectful spawn.

Qualified language services and compilers are `external-target-tool` dependencies, not
Membrane-owned interpreter workers. A TypeScript engine may itself use an interpreter only when
its external identity, version, executable path, capabilities, side-effect class, and process tree
are explicit. Membrane MUST NOT bundle, install, or bootstrap those external tool interpreters.
The sole bundled interpreter exception is Blueprint's pinned Node component, which is executable
only from Blueprint's installed root under Hub-owned service/watcher or an explicit bounded
one-shot request. Tool absence yields typed `Unavailable`/`Not configured` evidence and can never
produce `clean_exact` where its capability is required. Native-only qualification covers both
absent-tool degradation and a separately provisioned available external-tool case.

### 9.3 Runtime-language freeze

From adoption of this document until native-only seal:

- no new Python/Node production path may be added to Membrane;
- additions under `adapt/`, `continuity/`, `mcp/`, runtime crates, Hub, installers, or generated host configs that introduce an interpreter dependency are architectural violations unless explicitly marked temporary migration code with a deletion owner/gate;
- CI MUST detect new interpreter launch/import sites and require ledger classification.

---

## 10. CodeRight integration architecture

The valid ownership decisions from the prior migration specification remain authoritative unless superseded below.

### 10.1 Durable vs transactional ownership

**Cortex owns semantically/lexically retrievable durable records:**

- memories/temporal facts;
- admitted document records;
- admitted sessions and transcript chunks/events intended for later retrieval;
- tasks/artifacts admitted for later recall;
- admitted Taste preferences, Insight issues, and other governed knowledge;
- retrieval indexes and recall/delivery receipts.

Native transcript code owns normalization, stable event identity, provenance, and typed parse failures. Source hosts retain raw transcripts until explicit Cortex durable admission. Ledger owns rebuildable navigation/section projections only.

**CodeRight owns live transactional execution state:**

- workflow/run/node/lease/attempt/claim/wait/approval state;
- tool invocation/cancellation/driver state;
- live inference connection and delivery state;
- execution-only budgets/queues/outboxes;
- state-machine data not intended for later semantic/lexical retrieval.

Test: if an admitted record will later be retrieved by meaning/keyword, Cortex is its default durable owner. Raw unadmitted transcript material stays with its source host; navigation projections belong to Ledger; hot state-machine data stays with CodeRight.

### 10.2 One memory universe

CodeRight uses exactly one typed Membrane capability binding served by an
active Hub. There is no embedded CodeRight Membrane backend and no alternate
local Cortex store.

Startup:

1. probe Hub and perform the versioned Membrane capability handshake;
2. verify protocol, capability set, installation identity, and Cortex store identity;
3. if Hub is inactive or incompatible, block Membrane-required agent execution with typed unavailability;
4. bind the compatible Hub/store identity for the session;
5. binding loss remains explicit and MUST NOT create a local divergent universe.

After an active-Hub binding is selected, backend loss behaves as follows:

1. never open, create, or write an embedded fallback database;
2. memory reads return typed `BackendUnavailable`; optional context lanes record an attributable omission, while memory-required operations stop;
3. new durable writes fail closed with retryable `BackendUnavailable` before acceptance and retain a caller-supplied idempotency key for explicit retry;
4. an in-flight write with unknown commit outcome returns typed `CommitUnknown` plus request/idempotency identity; recovery queries the original backend receipt before any retry;
5. CodeRight may continue unrelated hot transactional work, but it cannot report a memory-dependent transition as complete;
6. recovery rebinds only to the same compatible Hub/store identity. A different store identity requires explicit migration or process restart with a rebind receipt.

No volatile queue may report a durable write as accepted. These rules prefer visible unavailability over split-brain or silent loss.

### 10.3 Event/session invariants

Absorbed session/event storage preserves:

- `(session_id, seq)` stable monotonicity;
- idempotent append identity;
- interactive-latency writes;
- range queries;
- durable restart resume;
- no sequence reuse;
- import gap/duplicate/reorder detection.

### 10.4 Transcript routing

- admitted structured transcript chunks/events -> Cortex/native event store with lexical+semantic retrieval;
- human-readable per-session summary/handoff -> Ledger navigation projection;
- summaries never replace the raw authoritative event history.

### 10.5 Retrieval

Cortex remains embedding/vector owner. Adopt FTS5 or equivalently measured native lexical indexing for hybrid retrieval; benchmark fusion quality before replacement.

### 10.6 Dependency policy

CodeRight uses workspace/path dependencies during coordinated development or semantically versioned stable crates. Scattered SHA pins and retired `memright` package identities are not the final contract.

---

## 11. Migration work packages — remaining canonical plan

The earlier federation work packages are historical evidence. The following work packages are the **remaining canonical closure plan** from baseline `8a215ac6...`. They may be split into executor packets, but their ownership and exit gates must remain intact.

### N0 — Refresh the complete executable/runtime ledger

**Status: DONE — current implementation status is recorded in Section 0.1.1.**

**Goal:** make the product-wide language/runtime graph exhaustive.

Canonical classification authority is `runtime-language-manifest.json`. Reachability authority is
`invocation-graph.json`; manifest reachability fields are generated/validated projections of that
graph, never caller assertions. Existing `executable-ledger.json` is a stale legacy migration input:
N0 reconciles any useful evidence into the canonical artifacts, records supersession, then removes
it from gates. No completion check may consult both ledgers or choose the more permissive row.

**Inventory at current HEAD:**

- every `.py`, `.js`, `.mjs`, `.cjs`, `.ts` executable/source module;
- shebangs;
- `subprocess`, `child_process`, `Command`, shell/process launch sites;
- dynamic imports/module loaders;
- generated executable commands/configs;
- installed shims;
- schedulers/launch agents;
- MCP/host configs;
- Hub HTML/CSS/WebView scripts, Tauri commands/capabilities/CSP, generated bindings, package assets,
  and every UI-to-Rust action/read-model edge;
- Live Diagnostics operational routes, host hooks, command classifiers, reconciliation clients,
  provider adapters, and external-engine launch sites;
- Blueprint findings/service calls, generation/hash bindings, and any CLI/subprocess fallback;
- loopback self-calls;
- package/release/SBOM inclusions;
- CI/release tooling;
- source-checkout-only scripts;
- every Rust mode/entrypoint that can reach them.

**Disposition enum:**

`native-port | declarative-data | bounded-presentation | external-typed-service | external-target-tool | dev-only | migration-oracle | delete`

**Required fields:** see Section 19.

N0 MUST inspect untracked/in-flight native candidates before dispatch. A candidate is adopted only
after its ownership, contract, workspace membership, dependencies, consumers, and tests are
recorded; otherwise it receives an explicit supersession/preservation disposition. Parallel
native owners are forbidden.

**Exit gate:** 100% of executable files and launch sites classified; no production `unknown`;
every production row has native owner, parity fixture, cutover gate, and deletion/proof owner;
every `production_reachable` value is derived from the canonical invocation graph; all conflicting
legacy ledger rows are reconciled; native transcript/Adapt candidates have one recorded owner and
integration/supersession disposition.

### N1 — Freeze language-neutral contracts and native-only policy

**Status: DONE — current implementation status is recorded in Section 0.1.1.**

Freeze fixtures/contracts for:

- federation request/provider/merge behavior;
- `TranscriptEventV1` and host adapter semantics;
- Adapt candidates/records/manifests/authority/admission/apply/core compilation;
- complete registry-defined MCP schemas/results/errors, currently 17 tools;
- `WorkspaceEpochV1`, `CoverageObligationV1`, `DiagnosticEvidenceSnapshotV1`, and
  `DiagnosticGateDecisionV1` operational contracts;
- Live Diagnostics project-root/worktree identity, source-manifest framing, mutation,
  reconciliation, coverage, exactness, gate-precedence, and parent-health invariants;
- transactional, observed-hook, and reconciliation-only host conformance;
- renderer/budget reconciliation;
- installed CLI/Hub/scheduler behavior;
- CodeRight backend/session/event behavior;
- package/runtime-language inventory.
- bounded Hub presentation assets, capability/CSP restrictions, generated binding surface, and
  machine-checkable separation from Rust-owned authority/effects.

The five public Membrane V1 shapes remain unchanged. `TranscriptEventV1`, `FailureEpisodeV1`, and `InsightIssueV1` are internal domain contracts; record their schema/version/digest in the N1 fixture manifest without adding them to the public protocol registry.

Add a checked-in policy manifest that defines allowed production runtimes (`rust`, declarative
formats, and §1.5 `bounded-presentation`) plus permitted external/dev-only exceptions.
`bounded-presentation` is the sole installed JavaScript class and validates its exact Hub
asset/capability restrictions.

**Exit gate:** fixtures are language-neutral, hashed, versioned, and not dependent on executing the legacy implementation to exist.

### N2 — Port transcript normalization to native Rust

**Status: PARTIAL — native owner/cutover/conformance done; 29 runtime-language blockers are closed, while final deletion/exclusion & installed receipts remain open.**

Implement Section 5. First audit the current `engine/crates/membrane-transcript/` worktree
candidate. Adopt/integrate it when contract-correct; otherwise explicitly supersede it before
writing replacement code.

**Exit gate:** one committed native transcript owner is an engine-workspace member; transcript
differential corpus passes; every N0-inventoried consumer uses its native API; no production
import of Python `continuity` remains; Python package is deleted except release-excluded
language-neutral fixtures/dev tools.

### N3 — Port Adapt deterministic core

**Status: DONE — current implementation status is recorded in Section 0.1.1.**

First audit the current `engine/crates/membrane-adapt/` worktree candidate. Adopt/integrate it
when contract-correct; otherwise explicitly supersede it before writing replacement code.

Port without LLM dependency first:

- record/ID canonicalization;
- deterministic extraction;
- authority/admission;
- scope/lifecycle;
- evidence/source binding;
- manifest hash/validation;
- contradiction/direct conflict checks;
- transactional apply contracts;
- multiwriter/cross-machine receipts;
- Insights deterministic/report-only portions;
- CLI schemas.

Apply Adapt canon's correctness contracts—especially §5.4 scope, §5.8 delivery, and §7.3 semantic sealing—rather than reproducing known Python defects.

**Exit gate:** one committed native Adapt owner is an engine-workspace member; deterministic-only
fixtures pass; malformed scope fails closed; semantic manifest mutation is detected;
retired/narrow records cannot enter always-on core.

### N4 — Native Adapt proposal/review/adjudication/apply boundary

**Status: DONE — native proposal/review/adjudication/apply boundary and explicit transcript
source-binding contract are landed. Automatic implicit host signals remain an optional,
separately evaluated lane and are not a release gate for selected-transcript use.**

Implement native proposal source, required review, adjudication, and semantic-validation orchestration. Preserve exact source hashing/rebinding and authority separation.

The explicit caller-selected transcript path uses `adapt.user-taste-review.v1` for
local human review. The decision payload must bind exactly to the pending manifest,
installation identity, and canonical-pool digest; cover every pending record exactly
once with `valid` or `invalid` plus a non-empty reason; and include non-empty receipt
and validation timestamp fields. `issuer_id`, `key_id`, and `signature_hex` are empty,
and no login/authentication is required. Signed `adapt.semantic-adjudication.v1`
remains optional for enterprise/import use.

Current synthetic evidence is the committed 44-case `adapt.taste-benchmark-scorecard.v1`:
extraction precision `1.0` and recall `1.0`; admission precision `1.0` and recall `1.0`;
semantic-projection precision `1.0`; authority-negative false-positive rate `0.0` (`0/11`). The
product-fact modal false positive is closed by requiring modal words to occupy directive position
or an explicit preference/correction context. The held-out report records `23` true positives and
`97` false negatives (`recall 0.1917`), below its predeclared threshold; this optional automatic
extraction evidence does not gate the explicit selected-transcript workflow. Synthetic conformance
evidence is not a passing held-out or interval estimate.

**Exit gate:** selected transcript source identity/hash rebinding and required review are enforced;
no installed Adapt operation needs `python`, Pi CLI, OpenCode CLI, or other interpreter-backed
Membrane worker. Optional implicit host-signal quality remains separately evaluated.

### N5 — Native Adapt persistence, delivery receipts, CLI, and Hub scheduling

**Status: PARTIAL — native implementation & copied source-built-binary qualification landed; exact
installed receipts & legacy authority-runner replacement remain open.**

- native Cortex batch apply;
- current-policy/lifecycle projection;
- native recall metadata/delivery receipts;
- native `membrane adapt` CLI;
- native scheduler/lifecycle binding through Hub;
- remove source-workspace path assumptions;
- replace installed Python shim;
- replace `scripts/run-adapt-installed-current.mjs` as the authority test with native installed-artifact qualification (the JS script may remain only as dev-only test orchestration if desired).

**Exit gate:** exact installed candidate performs mine/review/apply/recall with Python absent and no development checkout.

### N6 — Cut MCP/server/renderer production path to Rust

**Status: PARTIAL — native implementation landed; installed conformance & host-fence receipts remain open.**

- make `membrane-mcp` canonical;
- implement the complete registry-defined tool set, currently 17 tools, and advertise only tools
  with executable native handlers;
- make Rust renderer/reconciliation canonical;
- port authorization/adapter behavior required at runtime;
- port Live Diagnostics MCP operations and installed host fence/reconciliation behavior to native
  Rust while preserving planner-decides/host-enforces separation;
- replace Node hook entrypoints with native executable/API bindings; retain JS only as
  release-excluded fixtures/oracles;
- prove observation-correlation conformance, including one issue with multiple preserved
  observations and uncertain matches remaining separate;
- prove `maxCost` conformance, including required coverage that cannot fit the ceiling;
- ensure generated host configs invoke native binary;
- retain JS tests only as dev-only differential coverage where useful.

**Exit gate:** complete registry-defined host conformance, currently 17 tools, passes with Node
absent; transactional/observed/reconciliation-only fence conformance passes; generated architecture
and installed host configs name native Rust sources as authoritative; native discovery advertises
no unexecutable tool; observation correlation and hard `maxCost` behavior pass contract fixtures.

### N7 — Delete federation Python and shadow runtime

**Status: PARTIAL — federation deletion/configuration implementation landed; installed deletion & upgrade/rollback receipts remain open.**

After current Rust federation parity evidence is accepted:

- delete gateway/providers;
- delete Python worker bridge/shadow selector;
- delete Python-specific runtime config/packaging;
- atomically migrate installed workspace configuration from v2 to v3, remove
  `pythonExecutable`, and return typed `workspace_config_migration_required` for invalid or
  unmigrated legacy input;
- preserve data fixtures only.

**Exit gate:** federation tests and installed artifact pass with no Python code present/selectable;
installed upgrade/rollback tests prove receipt-bound idempotent v2-to-v3 migration, and no shipped
configuration contains a legacy interpreter key.

### N8 — Close Blueprint packaging/runtime boundary

**Status: PARTIAL — Blueprint packaging/runtime-boundary implementation landed; its watcher is
Hub-coupled and installed residency receipts remain open.**

Implement Section 8.3's Hub-hosted residency and bounded one-shot contract.
Include the merged D0a/D0b findings surface, named-generation baseline/delta,
exact content hashes, affected-closure evidence, typed omissions, store lease,
and freshness/coherence separation. Membrane consumes these only through
Blueprint's public protocol and never duplicates its graph/resolution owner.

**Exit gate:** typed native Blueprint client passes Hub-hosted watcher residency, explicit
Hub-off bounded one-shot availability, Membrane Hub-off typed unavailability/no-fallback,
active-writer exclusion, generation/hash mismatch,
findings omission, and dirty-overlay conformance with honest degradation. The
`path`, `flows`, and complete inventory/audit obligations from Blueprint canon
§17.3 are satisfied before residency cutover.

### N9 — Finish Membrane's active-Hub consumer seam

**Status: PARTIAL — Membrane-side strict handshake, identity fence, native memory routes, diagnostics pipe, and typed Hub-loss behavior landed; integration and installed receipts remain open. No CodeRight repository mutation is required or permitted by this lane.**

Expose the Membrane side of the consumer integration canon:

- one versioned active-Hub Membrane binding;
- no embedded Membrane backend or local fallback store in Membrane-owned clients;
- strict health identity and per-request optional identity fence;
- complete native memory/retrieval routes;
- Hub-loss conformance for `membrane_unavailable { hub_inactive }` and same-store recovery;
- typed Hub-served Live Diagnostics workspace/mutation APIs;
- exact mutation sealing, operational gate consumption, and host enforcement without MCP,
  loopback HTTP, or a second Membrane runtime.

**Exit gate:** Membrane's client, memory, MCP, and transactional diagnostics conformance passes
through one Hub binding, with no embedded/local fallback, no split memory universe, and no
stale-byte fence clearance. External consumer implementation remains outside this repository.

### N10 — Native-only release seal

**Status: BLOCKED / NOT SEALED — N2/N5-N9 installed receipts & remaining Section 17 gates are
pending. Optional implicit host-signal evaluation is not an N4 or release gate for the explicit
selected-transcript workflow.**

Run the full Section 15–16 native package qualification against one exact package candidate
digest, including bounded Hub presentation. Record declared external-integration evidence
separately when available; it is supplemental and does not gate explicit selected-transcript
Adapt use.

**Exit gate:** every Section 17 checkbox passes and machine-readable `native-only-seal.json` is issued. Otherwise status is failed, never “mostly complete.”

---

## 12. Migration sequencing and dependency graph

Required order:

```text
W0 docs/ontology may run beside N0 executable inventory.

N0 ledger
  |
  v
N1 contract + fixture freeze
  |
  +----------------------+----------------------+----------------------+
  |                      |                      |                      |
  v                      v                      v                      v
N2 transcript          N6 MCP + host fence  N7 federation delete   N8 Blueprint findings seam
  |                                             (after parity)          |
  v                                                                    |
N3 Adapt core                                                        N9 contract tests
  |                                                                    (interface work)
  v
N4 proposal/adjudication
  |
  v
N5 installed Adapt ---------------------------> N9 final integration

Language-neutral Adapt work runs after N1 beside N2: labelled Insights benchmark
(W5), semantic-seal/group fixtures (W1/W2), evaluation corpora, and docs. It may
produce contracts, fixtures, and measurements, never new Python production behavior.

N5 + N6 + N7 + N8 + N9 -> N10 native-only seal
```

N0 and N1 are the only global prerequisites. After N1, independent native ports, boundary work, deletion-qualified federation work, and language-neutral evaluation proceed in parallel. Do not serialize work merely because final N10 consumes every lane.

Do not delete a legacy implementation before its language-neutral fixtures and native replacement gate exist. Do not keep legacy code after the deletion gate passes.

---

## 13. Implementation governance and anti-loop rules

### 13.1 Settled decisions are not open design prompts

The following are settled:

- Membrane production runtime is native Rust/declarative data;
- this applies to all six axes and installed surfaces, not only federation;
- Adapt is a product subsystem and must be native;
- transcript normalization required by Adapt must be native;
- MCP/runtime rendering must be native;
- Cortex remains durable-memory authority;
- Adapt proposes/applies through Cortex rather than owning a competing DB;
- CodeRight binds to Membrane only through active Hub;
- no embedded CodeRight Membrane backend or local fallback store exists;
- production does not spawn Python/Node to execute Membrane-owned source;
- external product/tool boundaries are explicit and typed;
- dev-only scripts are allowed only with exclusion proof;
- final release has no hidden runtime fallback.

Do not reopen these because an executor prefers another language or because porting is inconvenient.

Valid revisit triggers require concrete evidence of an unimplementable platform/API/licensing/security constraint and an ADR accepted by the architecture owner. A revisit is not permission to silently weaken the native-only completion rule.

### 13.2 Preflight

Before each work package:

- refresh relevant paths at current HEAD;
- verify fixture/schema hashes;
- confirm native target owner;
- detect user/unrelated work in owned files;
- verify predecessor receipts;
- stop on source drift that invalidates the packet assumptions.

### 13.3 Semantic blockers

A semantic blocker is a required behavior whose contract cannot be recovered from source, tests, schema, or an explicit prior decision. It stops only the affected unit; it does not justify broad architecture churn.

### 13.4 Acceptance independence

The implementation author should not be the sole acceptor for:

- authority/scope enforcement;
- manifest integrity;
- backend binding/migration;
- process/runtime-language isolation;
- package/SBOM qualification;
- native-only seal.

### 13.5 No bug-for-bug parity where the contract is known wrong

Differential parity is a migration tool, not an obligation to preserve defects. Known issues documented in this specification — especially Adapt manifest hashing, fail-open scope normalization, lossy mirrors, retired-rule core compilation, and magic root scope — are corrected intentionally and represented as versioned expected deltas.

---

## 14. Data, compatibility, and migration rules

### 14.1 Serialized data may survive; executable legacy code may not

Compatibility readers MAY preserve old:

- JSON/JSONL record shapes;
- manifests;
- migration receipts;
- transcript fixture formats;
- database schema versions;
- IDs/hash algorithms where stability is required.

They MUST NOT preserve an executable Python/Node implementation as a hidden fallback.

### 14.2 ID/hash stability

Where IDs are externally persisted/referenced, Rust must reproduce the canonical algorithm or perform an explicit versioned migration with old->new mapping and receipts.

No model may assign authoritative record IDs when deterministic code can derive them.

### 14.3 Atomicity

Adapt/Cortex apply and CodeRight migration operations must be transactional or receipt-backed so partial failure cannot create a state reported as success.

### 14.4 Multiwriter

Concurrent/multi-machine learning must preserve installation/session identity, deterministic conflict handling, and no duplicate authority. Merge policy must be explicit; last-write-wins by incidental timestamp is not a sufficient authority rule.

### 14.5 Rollback

Rollback is between signed product artifacts. A native release does not contain an interpreter fallback for rollback. Data rollback must not re-enable dual writers or divergent stores.

---

## 15. Verification strategy

### 15.1 Contract/golden corpora

#### Federation corpus

Include all nine providers, scope/freshness/generation states, timeout/cancel/panic/malformed cases,
candidate conflicts, path/symlink/case behavior, and deterministic merge permutations. Include
valid v2-to-v3 workspace configuration migration, repeated/idempotent migration, invalid or
unmigrated legacy input returning `workspace_config_migration_required`, distinct resident and
federation-capability health, and Hub shutdown/crash/restart/upgrade leaving no orphan worker or
process.

#### Transcript corpus

Include:

- Claude/Codex canonical sessions;
- truncated/corrupt files;
- source byte-span edge cases;
- cross-machine/session identity;
- assistant/tool/user/provenance distinctions;
- synthetic/redacted/private-reasoning-omitted flags;
- unknown host/source behavior;
- missing/inaccessible transcripts.

#### Adapt corpus

Include:

- explicit durable directives;
- corrections with/without reasons;
- current-task-only instructions;
- product/repo facts vs personal taste;
- permission/security expansion attempts;
- assistant/tool/repo injection attempts;
- duplicates;
- direct contradictions;
- semantic conflict cases;
- scope dimensions valid/malformed/unknown;
- lifecycle states;
- machine-only applicability;
- manifest post-review mutation attempts;
- evidence hash/span mismatch;
- multiwriter convergence;
- core compiler eligibility;
- negative-control retrieval/delivery;
- Insights report-only isolation.

#### MCP/renderer/Live Diagnostics corpus

Include every registry-defined tool (17 at baseline), schemas, authorization outcomes, toolset
negotiation, host capability modes, budget lane classification, resolver-backed blocks, and
deterministic alert ordering. Include Live Diagnostics workspace identity, mutation sealing,
current-byte reconciliation, source-manifest framing, exact coverage, gate precedence, baseline
deltas, provider lifecycle, and transactional/observed/reconciliation-only host modes.
Include BP001 plus TS2305 as one issue with two preserved observations, uncertain matches remaining
separate, and `maxCost` ceilings that produce typed unknown/invalid coverage without provider
escalation.

### 15.2 Property tests

At minimum:

- canonical serialization stable;
- federation merge independent of provider completion order;
- unvalidated scope cannot reach output;
- one deadline does not extend per provider;
- cancellation leaves no child/task behind;
- transcript parse deterministic;
- exact evidence span round-trips;
- malformed present scope cannot broaden applicability;
- semantically active manifest mutation changes/rejects digest;
- retired/disputed/superseded record cannot compile into active core;
- machine-only record cannot deliver cross-machine;
- apply is atomic/idempotent under replay;
- multiwriter ordering/convergence rules hold;
- active-Hub CodeRight binding opens no local DB;
- native MCP outputs remain deterministic for fixed inputs.
- deleted/unreadable changed paths remain represented in the source-manifest digest;
- an old cleared epoch cannot clear modified, added, deleted, symlink-escaped, or differently rooted bytes;
- malformed or compound verification commands cannot bypass an enabled host fence;
- provider degradation cannot change parent Membrane health;
- unsupported native MCP tools are never advertised.

### 15.3 Differential testing

Compare contracts, not incidental implementation formatting.

For each port:

- legacy frozen oracle vs native output;
- normalize only named nondeterministic fields;
- record expected intentional deltas;
- reject unexplained security/authority/provenance/scope/lifecycle differences.

Once deletion gate passes, store golden fixtures/results so future tests do not need the legacy executable.

### 15.4 Fault injection

Inject failures at:

- transcript discovery/read/parse;
- model proposal timeout/malformed output;
- semantic adjudication disagreement;
- scope/authority manifest load;
- Cortex batch begin/write/commit;
- multiwriter receipt replay;
- Blueprint/service read and generation switch;
- MCP stdin/stdout framing;
- diagnostics workspace/reconciliation transport and current-worktree Git inspection;
- language-service crash, timeout, stale convergence, executable replacement, and process-tree cleanup;
- host pre-verification and completion fence boundaries;
- Hub schedule/crash/restart;
- external process timeout/kill where any allowed external tool is used;
- CodeRight active-Hub binding loss;
- installer/updater/rollback transitions.

### 15.5 Behavioral Adapt evaluation

Use versioned language-neutral fixtures and outcome gates defined by Adapt canonical authority. Migration acceptance proves native results preserve those semantics, records intentional corrections explicitly, and keeps current scorecards reproducible without invoking legacy Python. This plan does not define a second Adapt evaluation taxonomy.

### 15.6 Performance gates

Record hardware/OS/corpus and versioned thresholds. Measure:

- federation p50/p95;
- transcript parsing throughput/memory;
- Adapt mine/review/apply latency and model-call budget separately;
- Cortex batch/retrieval latency;
- MCP request latency;
- mutation-to-diagnostic and reconciliation-to-gate latency;
- startup/resident memory;
- cancellation completion;
- package size changes.

The Rust port should remove interpreter/serialization overhead but performance claims require measured evidence.

Before N7 hard cut and legacy deletion, freeze a committed same-machine federation baseline at
`migration/native-rust/federation-benchmark-baseline.json`. It MUST bind workload and artifact
identity and record warm/cold latency, resident memory, candidate counts, omission counts, and
receipt counts. Replay the same workload on the same machine against the native candidate and apply
an explicitly approved non-regression threshold. N7 cannot exit without this baseline and result;
this specification does not invent the numeric threshold.

---

## 16. CI, packaging, and native-only enforcement

### 16.1 Root CI is not the native-only proof

Current root CI installs Node/pnpm and runs JavaScript-centric checks. That is not itself a product violation because CI tooling may be dev-only. It is also **not sufficient evidence** that installed Membrane is native-only.

Add independent gates.

### 16.2 Source/runtime-language gate

CI generates/validates:

`migration/native-rust/runtime-language-manifest.json`

It is the one canonical classification artifact. `invocation-graph.json` is the reachability
authority from which its `production_reachable` fields are derived and validated.
`executable-ledger.json` is not a parallel gate after N0. CI fails when:

- a new executable path is unclassified;
- a production row uses Python/Node as `runtime` without an approved migration-only expiry/deletion owner;
- an installed shim/config/scheduler points at a dev-only path;
- a deleted legacy selector/path reappears;
- generated product truth names an interpreter implementation as authoritative after its native cutover.
- registry/product-truth/native-discovery tool sets differ or native discovery advertises an
  unexecutable tool;
- an installed Live Diagnostics hook, host config, or reconciliation path invokes Node.
- a manifest reachability value lacks or contradicts an invocation-graph path;
- legacy and canonical ledger rows remain unresolved or a gate still consumes the legacy ledger;
- a bounded-presentation row violates §1.5 or is absent from package/SBOM evidence.

### 16.3 Native unit/integration gates

Root CI MUST gate the Rust suites that own production behavior, including native
Adapt/Transcript/MCP and Live Diagnostics. Python/JavaScript tests are migration-oracle coverage
only and cannot be the sole branch gate for a production subsystem or installed host fence.

### 16.4 Installed-artifact qualification

Test one exact package digest, not a source checkout. This is native-only release evidence for N10,
not an N4 gate and not a prerequisite for the explicit selected-transcript workflow.

#### Native package qualification

Environment:

- `python`, `python3`, `pip`, `npm`, and `npx` unavailable; `node` is absent from
  `PATH` and may exist only as the hash-addressed Blueprint runtime component
  under the installed package's bounded Blueprint root;
- no `.venv-tools` assumption;
- no network fetch to obtain an interpreter/runtime.

Exercise:

- install;
- first launch;
- Hub status;
- all supported federation lanes;
- MCP tools through installed native config;
- Live Diagnostics workspace open/status/close, mutation, reconciliation, snapshot, gate,
  capability, baseline, provider lifecycle, and host enforcement paths;
- transactional, observed-hook, and reconciliation-only modes, including unobserved
  modify/add/delete, root mismatch, symlink escape, stale epoch, unavailable provider/service,
  malformed command, and compound-command cases;
- Cortex memory/retrieval;
- Ledger record/navigation path;
- Adapt mine/review/apply/recall/doctor path;
- Blueprint installed-component availability, Hub-hosted service/watcher
  residency, Hub-off bounded one-shot behavior, and typed Membrane
  `hub_inactive` behavior according to Section 8;
- external diagnostic tools absent/`Not configured` without false clean;
- upgrade;
- rollback;
- uninstall.

#### Supplemental external integration evidence

External diagnostic integration evidence is supplemental and does not gate the
native Adapt selected-transcript workflow. Use the same Membrane package digest
with its pinned, bundled Blueprint runtime; no external Blueprint provisioning is
allowed. Each optional diagnostic engine is supplied on a controlled injected search path with exact executable,
  version, digest, protocol, side-effect class, and transitive-runtime identity;
- prefer a self-contained native engine such as `tsgo` for TypeScript D1 seal evidence; any
  separately tested interpreter-backed external tool must bring its own declared runtime outside
  the Membrane package and cannot establish a Membrane runtime dependency.

Exercise available external-tool conformance separately from Blueprint's
Hub-hosted, Hub-off bounded one-shot, generation-switch/hash-mismatch, and
active-writer-exclusion behavior. Evidence separates the Hub process, bounded
Blueprint one-shot processes, and declared external-tool processes.
Membrane never fetches, installs, bootstraps, or resolves an interpreter from
its own package or an uncontrolled PATH.

Inspect:

- process tree;
- open executable paths;
- package content;
- SBOM;
- generated host configuration;
- logs/diagnostics for checkout/interpreter paths;
- bounded WebView asset hashes, CSP/capabilities, imports, generated bindings, and absence of Node;
- external service/tool ownership, executable/runtime identity, and process ancestry.

### 16.5 Native-only seal artifact

Final qualification produces machine-readable evidence, e.g.:

```text
migration/native-rust/final-acceptance.json
migration/native-rust/release-content-manifest.json
migration/native-rust/sbom-verification.json
migration/native-rust/process-tree-evidence.json
migration/native-rust/bounded-presentation-evidence.json
migration/native-rust/external-integration-evidence.json
migration/native-rust/runtime-language-manifest.json
migration/native-rust/deletion-receipts/*.json
migration/native-rust/native-only-seal.json
```

`native-only-seal.json` is emitted only when every required gate passes against one immutable release-candidate digest.

The issuer is `node scripts/qualification/issue-native-only-seal.mjs` with
`--release-manifest`, `--qualification`, `--runtime-language-manifest`,
`--invocation-graph`, `--native-contract-manifest`, and `--out` arguments. It
validates all five structured inputs, requires one lowercase installer digest
across release and installed evidence, then atomically writes the seal; it never
builds, signs, installs, or runs tests.

---

## 17. Final acceptance criteria

### 17.1 Product/runtime topology

- [ ] Canonical product truth lists six axes and each has a native/declarative/typed-external disposition consistent with this document.
- [ ] All Membrane-owned runtime authority, policy, state transitions, storage, and effects are Rust.
- [ ] Installed JavaScript is limited to §1.5 content-hashed bounded presentation under OS WebView; no Node dependency exists.
- [ ] No installed Membrane runtime feature requires Python/Node/npm/pip or interpreter-managed standard libraries.
- [ ] No production dynamic import/module resolution from development workspaces exists.
- [ ] No installed shim/scheduler/host config points at source-checkout `.py/.js/.mjs/.cjs`.
- [ ] Membrane runtime exists only in the Hub process; Hub launches no
  Membrane child, and any declared external target-tool or bounded Blueprint
  one-shot process is identity-bound and exits under its governing contract.

### 17.2 Federation

- [x] Federation Python/shadow deletion and v2-to-v3 configuration cutover implementation has landed; installed deletion/upgrade receipts remain pending.
- [ ] Production federation is native and same-process.
- [ ] All nine provider semantics pass native conformance.
- [ ] Scope/deadline/cancellation/generation/trust/merge invariants pass.
- [ ] Hub snapshots distinguish resident health from federation capability health; provider
  degradation cannot make parent Membrane offline unless it is an explicit hard prerequisite.
- [ ] Hub launch/restart/crash/upgrade/shutdown leaves no orphan federation worker or process.
- [ ] Installed workspace configuration is schema v3, upgrade/rollback migration is atomic,
  receipt-bound and idempotent, and no `pythonExecutable` key remains.
- [ ] Frozen same-machine federation baseline and native replay satisfy the approved
  non-regression gate.
- [ ] Python federation gateway/providers/worker/shadow selector are deleted from production.

### 17.3 Transcript normalization

- [x] Canonical transcript normalization is Rust.
- [x] `TranscriptEventV1`, exact byte spans, provenance, parser receipts, and typed unavailability pass fixtures.
- [x] Adapt has no production dependency on Python `continuity`.
- [x] Every N0-inventoried consumer uses native transcript contract; Python package is excluded from production (final deletion/exclusion receipt remains pending).

### 17.4 Adapt

- [x] Native deterministic extraction/authority/admission/manifest/apply pipeline passes.
- [x] The committed 44-case synthetic Taste conformance scorecard passes its declared point thresholds.
- [x] Explicit selected-transcript Taste path enforces source hash/rebinding and required review before apply.
- [ ] Optional automatic implicit host-signal lane passes approved extraction/admission thresholds with interval reporting — current report: `23` TP / `97` FN, recall `0.1917`; this lane is not a release gate for selected-transcript use.
- [x] A copied source-built `membrane` binary passes isolated Adapt qualification without interpreter tools or a checkout cwd.
- [ ] The exact released package passes Adapt qualification without Python, Pi CLI, OpenCode CLI, or a source checkout.
- [ ] `scripts/run-adapt-installed-current.mjs` is replaced as an authority test or explicitly proven release-excluded and dev-only.
- [x] Manifest digest covers semantically active applicability/lifecycle fields.
- [x] Malformed declared scope cannot broaden eligibility.
- [x] Decision-making projections preserve lifecycle/scope/machine/current-policy controls.
- [x] Core compiler selects only active eligible standing preferences using canonical root semantics.
- [x] Insights remains report-only unless separately admitted through governed policy.
- [x] Live interaction-signal learning is claimed only if host delivery/outcome receipts actually ship.
- [x] Current behavioral scorecards are committed; their synthetic/held-out scope is stated before comparative “better” claims.

### 17.5 MCP / Push rendering

- [x] `membrane-mcp` native production implementation has landed; installed conformance receipt remains pending.
- [x] Rust renderer/reconciliation implementation has landed; installed receipt remains pending.
- [ ] Complete registry-defined host conformance, currently 17 tools, passes with Node absent.
- [ ] Native discovery advertises only executable tools and matches registry/product truth.
- [ ] Generated host configurations invoke native binaries.

### 17.6 Live Diagnostics

- [x] Native Live Diagnostics/host-fence implementation has landed; installed enforcement receipt remains pending.
- [ ] Live Diagnostics remains a Membrane runtime module under Hub, not a seventh subsystem.
- [ ] Resident contracts, evaluator, supervisor, reconciliation, provider adapters, and routes are native Rust.
- [ ] Exact repo/worktree/project-root/epoch/hash/manifest binding passes.
- [ ] Current-byte reconciliation blocks unobserved modify/add/delete, root mismatch, symlink escape, Git/transport failure, stale/superseded evidence, and uncleared fence state.
- [ ] One exact blocker may prove `dirty_exact`; `clean_exact` requires all required exact coverage.
- [ ] Planner owns gate policy, deterministic evaluator applies it, and hosts enforce it.
- [ ] Installed host enforcement requires no Node and disabled enforcement remains non-interfering.
- [ ] External engines are identity-bound, bounded, cancellable, and typed-unavailable without auto-install.
- [ ] Diagnostics/provider degradation cannot determine parent Membrane health; `Not configured` remains distinct.
- [ ] Correlation preserves every observation and deduplicates only issue presentation; uncertain
  observations remain separate.
- [ ] `maxCost` is never exceeded; unmet required coverage is typed unknown/invalid rather than
  silently escalated.

### 17.7 Blueprint

- [x] Blueprint packaging/runtime-boundary implementation has landed; installed Hub-hosted/one-shot receipt remains pending.
- [ ] No Node Blueprint CLI fallback exists inside Membrane providers.
- [ ] Hub-off Membrane requests return typed unavailability and never invoke Blueprint one-shot.
- [ ] Direct Hub-off Blueprint requests are bounded, publish transactionally, and exit.
- [ ] Native-only Membrane artifact does not bootstrap an undeclared Node runtime.

### 17.8 Cortex / data / CodeRight

- [ ] Cortex remains the single durable-memory authority for Adapt records.
- [x] CodeRight source implements one compatible active-Hub Membrane binding with no local fallback; independent verification, commit, and installed receipts remain pending.
- [ ] CodeRight uses Hub-served transactional diagnostics and host enforcement without a second Membrane runtime.
- [ ] CodeRight opens no embedded/local fallback Cortex DB.
- [ ] Backend failure does not silently split memory universes.
- [ ] Sessions/events/tasks/artifacts migrate with verified receipts.
- [ ] `(session_id, seq)` invariants pass.
- [ ] Hybrid lexical/semantic retrieval passes relevance/performance gates.

### 17.9 Release/operations

- [ ] Exact package passes with Python/Node absent from `PATH`.
- [ ] No development checkout or virtual-env path is required.
- [ ] Process tree contains no Membrane-owned interpreter child.
- [ ] Package/SBOM contains no undeclared interpreter runtime payload.
- [ ] Bounded WebView presentation evidence proves §1.5 restrictions for exact shipped assets.
- [ ] Native package qualification references one exact package digest; supplemental external integration evidence is recorded separately when available.
- [ ] Upgrade, rollback, crash recovery, shutdown, cancellation, and uninstall pass.
- [ ] Product truth and architecture docs describe the exact shipped topology.
- [ ] Legacy code is deleted, not merely disabled.
- [x] Runtime-language manifest closes 29 identified blockers; sealed manifest has zero production interpreter rows and exclusion proofs.
- [ ] Every runtime-language ledger row has deletion receipt or dev-only proof.
- [ ] `native-only-seal.json` references the exact package digest tested.

---

## 18. Explicit deletion and demotion list

Final native closure removes or demotes the production role of at least:

### Federation

- `engine/federation/gateway.py`;
- `engine/federation/providers/*.py`;
- Python-specific federation implementation tests after fixture transfer;
- interpreter discovery, worker handshake, stdio JSON worker framing, restart, Python-worker diagnostics;
- legacy/shadow runtime selector after final differential acceptance.

### Adapt

- `adapt/adapt.py` as an interpreter-backed installed entrypoint;
- production role of `adapt/src/adapt/*.py` after native replacement;
- `run_incremental_multiwriter.py` installed/scheduled authority path;
- Python `cli.py` installed shim target;
- `.venv-tools/bin/python` assumptions;
- Python-only scheduler/daily-sync linkage;
- production Pi/OpenCode CLI subprocess lanes;
- Python tests as sole acceptance authority after native contract transfer.

Residual Python evaluation/oracle scripts may remain only if classified dev-only and unreachable from installed product.

### Continuity Python package

- production imports of `continuity` Python package;
- installed/runtime dependence on `continuity/transcript/*.py` after native parser cutover.

Language-neutral transcript fixtures may remain.

### MCP / Node

- production authority of `mcp/server.mjs`;
- production authority of `mcp/context-renderer-lib.cjs`;
- production authority of `mcp/hooks/membrane-hook-entrypoint.mjs`,
  `mcp/hooks/membrane-hook-runtime.mjs`,
  `mcp/hooks/membrane-workspace-operations.mjs`, and
  `mcp/lib/verification-command.mjs` after native host cutover;
- production authority of `mcp/lib/diagnostics-client.mjs` after native MCP/host cutover;
- interpreter-backed runtime host configurations;
- Node runtime adapters/authorization code that remains in the installed path after native equivalents exist.

JS test/generator code may remain dev-only if exclusion proof passes.

### Cross-product/runtime

- Node Blueprint CLI invocation from anchors/providers;
- dynamic Python imports for Audit/Architect/skills or workspace-relative source loading;
- same-process loopback calls where direct native APIs exist;
- retired CodeRight `memright` package identities/pins once stable current crates are wired;
- docs that describe hybrid Python/Node Membrane-owned runtime as the intended end state.

---

## 19. Migration ledger schema

`migration/native-rust/runtime-language-manifest.json` is the canonical migration ledger.
`migration/native-rust/invocation-graph.json` is its reachability source. The prior
`executable-ledger.json` is reconciled and retired during N0; it is never a second authority.

Every executable/invocation item gets a row with at least:

| Field | Meaning |
|---|---|
| `path_or_symbol` | Source path, package entry, generated command, or launch symbol |
| `runtime` | `rust`, `python`, `node`, `shell`, `declarative`, `webview`, `external` |
| `kind` | source module, binary, shim, scheduler, service, config generator, test, benchmark, etc. |
| `invoked_by` | exact caller/entrypoint(s) |
| `product_surface` | Pull/Push/Cortex/Blueprint/Ledger/Adapt/Hub/MCP/install/CI/etc. |
| `production_reachable` | boolean proven from invocation graph |
| `packaged` | whether exact release candidate contains/requires it |
| `semantics_owned` | contracts/invariants currently implemented |
| `target_disposition` | native-port/declarative-data/bounded-presentation/external-typed-service/external-target-tool/dev-only/migration-oracle/delete |
| `target_owner` | Rust crate/module or external product owner |
| `parity_fixture` | language-neutral fixture/contract proving behavior |
| `cutover_gate` | observable native replacement gate |
| `deletion_or_exclusion_proof` | deletion receipt or dev-only proof artifact |
| `deadline_owner` | person/packet responsible for removal if temporary |
| `baseline_hash` | content/contract hash where needed |
| `notes` | explicit exception rationale; never blank for non-Rust production classifications |

No production row is complete without a native/external/declarative/bounded-presentation target
and a deletion/cutover gate. `bounded-presentation` additionally requires every §1.5 proof.

### 19.1 Invocation graph

The canonical manifest is derived from/validated against `invocation-graph.json`, which links:

```text
installed entrypoint
  -> runtime mode
  -> module/crate
  -> process/import/call boundary
  -> downstream executable/source
  -> data writes/effects
```

This is what would have exposed Adapt immediately: installed `adapt`/daily scheduler -> Python -> `adapt/src/adapt/...` -> Cortex.

Every production entrypoint must reach its imports, processes, services, storage, and effects in
the graph. A flat file inventory is not an invocation graph and cannot satisfy N0.

---

## 20. Documentation authority and generated truth

### 20.1 Source of truth order

When documents conflict, use:

1. this canonical migration/runtime-closure specification for runtime/process target, sequencing, packaging, deletion, and completion rules;
2. the Adapt canonical specification for Taste/Insights meaning, evidence authority, lifecycle, evaluation, and feature dependencies;
3. compiling typed Rust contracts for actual API/schema details;
4. canonical subsystem architecture documents for ownership;
5. generated product truth derived from current implementation;
6. migration ledger/receipts for current migration status;
7. README/user docs.

Generated product truth describes what currently ships; it does not override the target architecture merely because legacy code is still present.

### 20.2 Required doc updates during cutover

After each native cutover, regenerate/update:

- `docs/product-truth.md`;
- `docs/architecture.md`;
- `docs/subsystems/adapt.md` / Adapt README;
- transcript-contract docs;
- MCP/host docs;
- Live Diagnostics architecture, operations, provider, hook, and fence docs;
- release/install/doctor docs;
- CodeRight integration docs;
- native migration status/receipts.

No document may claim “native-only” before Section 17 passes.

---

## 21. Historical source-audit baseline and evidence

This section preserves the dated source audit through `8a215ac6...`; its uses of “current” describe
that historical baseline only. For current package status, use Section 0.1.1 and the Section 17
acceptance checklist.

Load-bearing evidence at that historical baseline included:

- `docs/product-truth.md` — six axes include Adapt and current MCP truth lists 17 tools, including seven diagnostic tools;
- `docs/architecture.md` — current generated source of truth still names `mcp/server.mjs` and `mcp/context-renderer-lib.cjs` on production surfaces;
- commit `5a9175b9518ca6d36dca3c7c176bddeca070f5e3` — production federation route made native/same-process while Python worker retained only for shadow qualification;
- `engine/crates/membrane-federation/` — native federation implementation;
- `engine/federation/` — still-present Python legacy implementation;
- `adapt/README.md` — Adapt is a Python CLI/library and product subsystem surface;
- `adapt/src/adapt/` — substantial Python runtime package;
- `scripts/run-adapt-installed-current.mjs` — installed-current check explicitly binds Python interpreter, Python runner, Python CLI shim, and scheduler source path;
- commit `f602fbbaec1d13629e6b09ca4d6d4c07277ad7ba` — substantial new Python Adapt multi-source transcript learning after the Rust migration decision;
- commit `7c05b49b6f9ea202116f6829e4f74949a4529592` — prior baseline, adding held-out semantic admission enforcement in Adapt;
- commit `8a215ac6fab11cc24bb821507057743b7898e09f` — historical audit baseline, adding qualified Live Diagnostics Rust core/providers/contracts, Blueprint D0 findings, seven JS diagnostic MCP tools, and installed Node hook enforcement;
- `docs/design/membrane-live-diagnostics-final-architecture.md` — exact mutation/evidence/fence ownership, no-false-clean contract, host enforcement, Hub lifecycle, and no-seventh-subsystem authority;
- `engine/crates/membrane-protocol/src/diagnostics.rs` and
  `engine/crates/membrane-runtime/src/{live_diagnostics.rs,live_diagnostics_service.rs,providers/}` — native operational contracts, evaluator, Hub-hosted service module, and qualified providers;
- `mcp/server.mjs`, `mcp/toolsets.mjs`, `mcp/lib/diagnostics-client.mjs`, and
  `mcp/hooks/` — current interpreter-backed diagnostic MCP/host surfaces that N6 must port or demote;
- `engine/crates/membrane-mcp/src/tools.rs` — native MCP currently advertises no tools, correctly avoiding advertisement before native execution exists;
- `apps/membrane-hub/src/*.mjs`, Tauri capabilities/config, and packaged frontend assets —
  current installed WebView presentation requiring §1.5 classification and authority/effect checks;
- uncommitted `engine/crates/membrane-transcript/` and `engine/crates/membrane-adapt/` — substantial
  candidate native owners that N0/N2/N3 must adopt/integrate or explicitly supersede; they are not
  part of committed baseline or completion evidence;
- tracked `executable-ledger.json` versus uncommitted `runtime-language-manifest.json` and
  `runtime-policy.json` — conflicting ledger candidates requiring N0 reconciliation into one
  manifest plus graph-derived reachability authority;
- `continuity/README.md`, `continuity/pyproject.toml`, `continuity/transcript/` — canonical Python transcript normalization;
- `.github/workflows/ci.yml` and `scripts/ci/run-ci.sh` — current root CI is Node/pnpm-based and does not by itself prove interpreter-free installed operation;
- Cortex/Hub/Rust runtime crates — native owners that remain part of the target architecture.

Any implementation starting from a later commit MUST refresh N0 against that HEAD. The commit above is an evidence baseline, not permission to ignore new executable paths.

---

## 22. Final statement

The final architecture is not “Rust federation plus Python Adapt plus some Node adapters.”

It is:

> **Membrane-owned runtime authority, policy, state transitions, storage, and effects are native
> Rust or declarative data; external products/tools use typed boundaries; installed JavaScript is
> limited to bounded OS-WebView presentation with no independent authority or effect path.**

Adapt and Membrane-owned transcript normalization are part of that rule. MCP, rendering, Live
Diagnostics, host enforcement, and current-byte reconciliation are part of that rule. Packaging,
shims, schedulers, generated host configs, and hidden fallbacks are part of that rule.

The native migration is complete only when the exact installed artifact proves that topology without Python or Node available, and the legacy executable paths have been deleted or proven dev-only.

That is the canonical completion definition.


---

## Appendix — Hub-owned runtime lifecycle summary

The Membrane runtime executes only inside the active Hub process. MCP and CLI
surfaces are stateless Hub clients; they never launch, auto-start, or register a
Membrane or Blueprint process.

```text
Agent -> membrane stdio adapter
            -> Hub probe
            -> Hub unavailable
            -> typed membrane_unavailable { reason: hub_inactive, retryable: true }
```

The adapter then exits, or remains a thin unavailable transport per host
protocol. It never launches anything. Blueprint's watcher exists only under Hub;
Hub-off Blueprint access is an explicit bounded one-shot operation, never a
Membrane fallback.
