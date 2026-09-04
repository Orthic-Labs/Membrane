# Blueprint v2 — Implementation Plan

**Status:** Execution-ready plan  
**Date:** 2026-09-04  
**Baseline:** current `Orthic-Labs/Membrane/blueprint` implementation plus Blueprint Architecture Canon v2

---

## 1. Objective

Implement Blueprint v2 without destabilizing the strong runtime primitives already present.

This plan deliberately does **not** rewrite the watcher or replace SQLite. It converts the existing system into a closed, verifiable product loop and then adds semantic precision/intelligence in bounded stages.

Execution order:

```text
Stage 0 — reconcile existing canon and schema contracts
Stage 1 — operational closure: init/watcher/MCP/freshness
Stage 2 — semantic precision: producers/resolution/frontiers
Stage 3 — structural intelligence: tests/entry points/process/contracts/frameworks
Stage 4 — retrieval expansion: BM25/AST/signatures; benchmark-gated dense search
Stage 5 — optional advanced projections
```

The first release gate is at the end of Stage 1, not after the entire destination canon is implemented.

For donor-specific implementation references, use `04_BLUEPRINT_DONOR_REFERENCE_V2.md`. It identifies the strongest project to inspect for each new BPT atom while preserving Blueprint-specific boundaries and license rules.

---

# 2. Preserve these existing components

Do not replace:

- `node:sqlite` + WAL;
- immutable/transactional generation model;
- `@parcel/watcher` native watcher;
- `watchman/adapter.mjs`;
- `watchman/repo-actor.mjs`;
- `watchman/reconcile.mjs`;
- `watchman/supervisor.mjs`;
- source/applied clocks;
- persistent event journal;
- Merkle convergence checks;
- current/stale/degraded/unwatched freshness semantics;
- bounded dependent repair;
- existing graph query primitives;
- current six MCP tools;
- observe-only query principle for unbounded reconciliation.

Treat changes to these areas as augmentation/hardening unless a failing invariant proves a rewrite is necessary.

---

# 3. Stage 0 — Canon and schema reconciliation

## BPC-001 — Close the 18 partial and 2 missing existing atoms

Existing partial atoms identified by the canon reconciliation:

`BPT-003, 010, 011, 012, 013, 014, 017, 020, 026, 027, 033, 034, 038, 039, 041, 042, 044, 057`

Missing implementations:

`BPT-051, BPT-052`

### Required work

For each atom:

1. identify its observable contract;
2. map current implementation paths/tests;
3. split “partial because implementation missing” from “partial because acceptance proof missing”;
4. implement or narrow the contract;
5. add atom-specific acceptance test;
6. mark delivered only when observable behavior is proven.

### Gate

No new atom may be marked delivered merely because a supporting internal type exists.

---

## BPC-002 — Formalize canonical fact vs projection schema

### Goal

Make the Architecture Canon's semantic distinction explicit in code/database metadata without requiring a wholesale storage migration.

### Add/normalize logical classes

Canonical:

- Repo/Revision/Worktree identity;
- Artifact;
- Entity;
- Occurrence;
- Relation;
- Evidence;
- Provider/Analyzer;
- Generation/Freshness.

Derived:

- architecture component/flow;
- process/step;
- contract bridge;
- search indexes;
- communities/metrics;
- export artifacts.

### Required metadata for persisted projections

- projection kind/version;
- source graph generation;
- producer/analyzer version;
- rebuild status;
- last-known-good generation where relevant;
- failure/degraded reason.

### Acceptance criteria

- dropping/rebuilding a projection does not alter canonical identity/facts;
- projection rows cannot be mistaken for canonical facts by application APIs;
- doctor/status can report projection readiness independently.

---

## BPC-003 — Correct provenance/confidence semantics

### Rule

Authoritative/deterministic facts use provenance, not probabilistic confidence.

### Migration behavior

Where existing code stores confidence for deterministic facts:

- preserve compatibility on read if necessary;
- stop emitting new fake confidence values;
- normalize application-level output to `confidence: null` for authoritative facts;
- retain confidence for inferred/heuristic facts only.

### Test

A compiler/SCIP relation must serialize with authoritative provenance and no confidence value. A heuristic bridge may serialize confidence and evidence.

---

## BPC-004 — Add source-state-aware authority evaluator

Implement a central evaluator rather than scattering producer precedence through queries.

Conceptual API:

```ts
evaluateEvidence({
  targetSourceState,
  candidates,
  requestedRelation,
}) -> admitted | unresolved_frontier
```

Ordering:

1. admissibility/scope;
2. source-state coherence/freshness;
3. semantic authority;
4. resolution specificity;
5. inferential confidence.

### Required regression test

Dirty workspace source + SCIP generated for older HEAD: Blueprint repairs/reparses the dirty source before admission. An optional LSP cross-check may agree or emit `resolution_conflict`; it never silently overrides the canonical result.

## BPC-005 — Add schema/indexer conformance verification

### Goal

Make the semantic contract executable before broadening provider coverage.

### Required work

Add a verifier harness that can consume small source fixtures plus human-readable assertions for:

- definitions/references;
- imports/calls;
- inheritance/override/type facts;
- expected unresolved/ambiguous relationships;
- evidence/provenance/source spans;
- negative assertions that specific false edges must not exist.

Provider/indexer versions and schema versions are part of the fixture result. A provider cannot be advertised as fully supported if its conformance suite is failing.

Do **not** add a Datalog runtime merely to mimic Kythe. Use the smallest implementation that gives deterministic, reviewable assertions; a richer rule engine is justified only if the fixture model outgrows simpler checks.

### Acceptance criteria

- fixtures are readable without inspecting serialized graph internals;
- a schema/provider regression fails CI with the exact violated assertion;
- fixtures cover positive, negative and ambiguity cases;
- schema documentation examples can be executed as fixtures where practical.

## BPC-006 — Add completeness-safe publication and shrink guards

### Goal

Prevent a partial or damaged update from becoming the new trusted generation.

### Required rules

1. a truncated/failed/partial build never supersedes a known-complete generation;
2. publication records explicit convergence/completeness state;
3. changed-file replacement may delete facts owned by that file/provider, but may not delete unrelated unchanged-file facts;
4. dependent-frontier truncation yields degraded/stale state rather than a falsely complete generation;
5. crash/restart recovery preserves the last known complete generation.

### Tests

- kill extraction mid-generation and verify prior complete generation remains published;
- simulate traversal/frontier truncation and verify readiness degrades;
- repair one file and assert unrelated file/entity counts and identities remain intact;
- inject a provider failure after partial output and verify no partial semantic tier is promoted as complete.

## BPC-007 — Preserve Membrane-native truth and lifecycle contracts

The donor-derived redesign must not regress correctness machinery already present in Membrane. Reuse existing schemas/semantics where they exist instead of introducing parallel abstractions.

### Required preservation work

1. **Typed ingestion disposition:** every discovered input/provider result reaches an inspectable terminal disposition; no silent disappearance.
2. **Freshness relation:** retain behind/ahead/diverged/unknown-style source-state diagnostics beneath the compact public freshness state; unknown never means current.
3. **Typed admission outcomes:** internal admission/gating APIs return explicit `allow | continue | block | noop`-class outcomes (or the existing equivalent) plus stable reason codes rather than ambiguous booleans.
4. **Generation-bound cursors:** paginated graph/query cursors carry the generation/source-state they were created against and fail closed after incompatible generation changes.
5. **Lease incarnation identity:** writer leases include `process_start_identity` or equivalent process-incarnation metadata, not PID alone.
6. **Declared-vs-done truth binding:** doc/config/contract truth can represent grounded agreement, contradiction, stale reference and unresolved grounding.
7. **Tier dominance:** lower resolution tiers fill unresolved gaps; they never numerically outvote stronger coherent evidence.

### Acceptance tests

- force every ingestion exit path and assert a terminal disposition + reason is recorded;
- simulate source behind/ahead/divergence and assert status never reports `current`;
- replay an old cursor after generation publication and assert a typed fail-closed error;
- simulate PID reuse with distinct process-start identity and assert stale lease ownership is rejected;
- create a declaration contradicted by current code and assert `doc_truth` preserves the contradiction rather than choosing one silently;
- present conflicting lower-tier candidates against one coherent higher-tier edge and assert the lower tier cannot win through confidence/scoring.

---


## BPC-008 — Enforce relationship producer/consumer parity

### Current code-level finding

The code-only audit found a concrete producer/schema mismatch: `blueprint/src/providers/compilers/python-scip.mjs` emits `TYPES`, while `blueprint/src/graph/relationship-kinds.mjs` does not declare that kind.

Resolve the naming contract (`TYPED` is preferred in the v2 semantic vocabulary unless existing migration constraints require another canonical spelling), then make the registry executable.

### Required work

1. make `relationship-kinds.mjs` the authoritative first-party registry;
2. reject or type-fail undeclared provider output;
3. add `tests/relationship-producer-parity.test.mjs` that runs first-party provider fixtures and asserts `emitted ⊆ registered`;
4. add consumer parity tests asserting `registered = handled ∪ explicit_exemptions` for traversals, persistence, SDK/API serialization and exports;
5. require schema migration/version notes when public relation semantics change.

### Exit gate

No first-party provider can emit a relationship that any relevant consumer silently does not know exists.

---

## BPC-009 — Replace manual parse-cache version discipline with extractor fingerprints

### Current code-level finding

`parse-cache.mjs` invalidates on `PARSE_CACHE_VERSION`, but extractor semantic changes still rely on a human remembering to bump that version.

### Required cache key

```text
contentHash
+
extractorFingerprint
```

The extractor fingerprint should derive from semantic-output inputs such as:

- cache/schema contract version;
- selected provider ID/version;
- language-table version;
- Tree-sitter grammar manifest digest;
- generic AST/extractor contract version;
- provider-specific extraction contract version.

Do not hash unrelated application code merely to force broad invalidation.

### Test

Preserve source bytes, change extractor fingerprint A→B, rebuild, and assert zero affected-language cache hits plus no stale A facts in the new generation.

---

## BPC-010 — Unify SCIP ingestion behind one semantic normalizer

### Current code-level finding

The code-only audit found two materially different first-party SCIP paths:

- `blueprint/src/graph/scip-provider.mjs`;
- `blueprint/src/providers/compilers/python-scip.mjs`.

They currently normalize roles/definitions/references/identity differently.

### Required work

Create one shared normalizer, e.g. `providers/compilers/scip-normalize.mjs`, that produces a neutral internal index representation:

```text
metadata
documents
definitionsBySymbol
occurrences
symbolInformation
relationships
externalSymbols
```

Preserve, when present:

- definition/reference/read/write roles;
- symbol kind/signature/documentation;
- relationships and implementation/type-definition relations;
- external symbols/package identity;
- position encoding.

Language-specific adapters may map normalized semantic facts into Blueprint entities but may not reimplement the SCIP wire semantics independently.

### Exit gate

Python/TypeScript/Rust SCIP fixtures normalize to the same contract shape and cross-document identity behavior.

# 4. Stage 1 — Operational closure

This is the highest-priority implementation tranche because it determines whether agents can rely on Blueprint continuously.

## BPI-001 — Make `blueprint init` the only canonical setup path

### Current problem

The modern init path and shipped legacy `blueprint-install` can produce inconsistent MCP/instruction behavior. The legacy path also contains stale semantics.

### Change

`blueprint init` owns:

- host detection;
- MCP configuration;
- graph initial build/adoption;
- watcher enrollment;
- Git lifecycle hooks;
- routing guidance;
- readiness verification;
- reversible state record.

### Compatibility

1. mark `blueprint-install` deprecated;
2. make it delegate to `blueprint init` where feasible;
3. emit one deprecation warning;
4. remove the legacy bin in the next breaking release.

### Acceptance criteria

- one idempotent init path;
- running init twice creates no duplicate entries/hooks;
- uninstall can reverse Blueprint-owned changes;
- no installed guidance references obsolete MCP tool names.

---

## BPI-002 — Host adapters, not ad hoc probes

Create explicit adapters for at least:

- Claude Code;
- Codex;
- Cursor;
- OpenCode/generic MCP where supported.

Each adapter owns:

```ts
interface HostAdapter {
  id: string;
  detect(root): Detection;
  planMcp(root): PlannedChange[];
  planInstructions(root): PlannedChange[];
  verify(root): VerificationResult;
  uninstall(root): PlannedChange[];
}
```

### Immediate bug fix

Claude detection must not treat the mere presence of `codex` as evidence that Claude Code is installed.

### Acceptance criteria

- detection fixtures for each host;
- multi-host repository configures all detected hosts independently;
- no host adapter overwrites unrelated user configuration;
- verification confirms the effective MCP command includes the repository root.

---

## BPI-003 — Universal MCP setup and probe

### Requirement

Every supported host must receive the same semantic Blueprint server configuration, adapted only to host syntax.

Canonical launch semantics:

```text
node <blueprint-mcp-entry> --root <repo-root>
```

or packaged binary equivalent.

### Verification

After writing config, init must perform a direct Blueprint MCP smoke test independent of whether the host currently has a live GUI/session:

1. spawn server for root;
2. initialize protocol;
3. list tools/resources;
4. call `blueprint_status`;
5. terminate cleanly.

### Acceptance criteria

- all six canonical tools present;
- live resources enumerate;
- wrong/missing root fails typed and actionable;
- startup does not require arbitrary client-supplied repo paths.

---

## BPI-004 — Separate liveness from readiness

Implement BPT-100 explicitly.

Suggested readiness payload:

```json
{
  "service": {"live": true},
  "watcher": {"owned": true, "state": "current"},
  "graph": {"generation": 42, "state": "current"},
  "providers": {
    "structural": "ready",
    "scip": "not_configured",
    "lsp": "available_on_demand"
  },
  "projections": {
    "architecture": "ready",
    "bm25": "not_built"
  },
  "mcp": {"probe": "ok"}
}
```

### Product states

At minimum distinguish:

- `ready_current`
- `ready_catching_up`
- `installed_unwatched`
- `installed_hub_unavailable`
- `installed_mcp_unavailable`
- `degraded_provider`

### Acceptance criteria

`blueprint status`, MCP status and init completion use the same application semantics.

---

## BPW-001 — Verify watcher ownership during init

Enrollment is not equivalent to active watching.

After enrollment:

1. ask supervisor/Hub for repository ownership;
2. verify source/applied clock relationship;
3. verify actor health;
4. if catching up, report that state explicitly;
5. if Hub unavailable, use configured policy below.

### Fallback policy

Preferred resident owner remains Membrane Hub.

If Hub is unavailable but Blueprint MCP is active, allow one session-scoped watcher for the current repository only. It dies with the MCP session and uses the same store lease. Do not create a second permanent daemon.

---

## BPW-002 — Git-transition fast path (BPT-099)

### Goal

Treat checkout/merge/rebase/rewrite as a coherent source transition rather than an event storm.

### Primary trigger

Install Blueprint-owned Git lifecycle hooks for:

- `post-checkout`;
- `post-merge`;
- rewrite/rebase-completion path where safely supported.

Hooks should nudge Blueprint; they should not perform heavyweight indexing themselves.

### Algorithm

```text
receive transition old/new source state
→ compute git diff --name-status old..new
→ normalize A/M/D/R entries
→ preserve dirty working tree differences
→ enqueue one ordered journal batch
→ suppress/coalesce duplicate watcher notifications where possible
→ run normal file-delta + bounded-dependent machinery
→ publish convergence state
```

### Requirements

- rename retains identity reconciliation path;
- transition batching remains crash-safe through journal;
- no assumption that hooks always fire: watcher/reconcile remains fallback;
- nested enrolled repos still respect ownership boundaries.

### Tests

- branch switch changing 1 file;
- branch switch changing 10k files;
- rename-heavy transition;
- checkout with dirty retained file;
- missed hook followed by watcher events;
- process crash after journal batch write but before all applications.

### SLO

Work scales primarily with changed Git paths, not raw watcher notification count.

---

## BPW-003 — Bounded query-time dirty-file repair (BPT-098)

### Purpose

Close the race where source changes immediately before an agent query.

### Hard limits

Initial product budget:

- wall-clock budget: 100 ms target cap for pre-answer verification/repair;
- max directly repaired files: 3;
- tiny dependent closure only;
- zero repository tree walks;
- zero semantic model load;
- zero cold LSP startup solely for this repair path.

Tune only with benchmark evidence.

### Algorithm

```text
query resolves candidate entities/artifacts
→ compare indexed digest/source state for involved files
→ if unchanged: continue
→ if changed and within budget:
     stable read
     lexical/Tree-sitter parse
     apply file delta
     bounded dependent repair
     advance applied clock
     complete every domain this delta marked pending
     reseal and advance the generation
     continue query
→ if budget/contention exceeded:
     do not block for full catch-up
     return answer only with explicit stale/degraded receipt
```

### Lock behavior

If another writer owns the repo:

- do not compete indefinitely;
- bounded wait/lease observation only;
- prefer typed stale receipt while resident watcher finishes.

### Domain completion and generation advance

Advancing the applied clock is not sufficient. In the observed failure the applied clock already equalled the
source clock (188 == 188) while `domainsPending` held `doc` and `indexed_revision` stayed at a superseded commit.
A repair that stops at the clock leaves freshness reporting `changed_since_generation` and dependent context empty.

Repair therefore also:

- clears every domain the applied delta marked pending, including phase-2 domains such as `doc`, by running that
  completion inline rather than deferring it to a manual seal;
- reseals `sourceObservation`, recomputes `manifestDigest`, and advances the generation so `indexed_revision`
  tracks the repaired state;
- leaves genuinely source-level domains (for example `compiler_python`) blocking convergence as before — the
  exception is scoped to phase-2 domains and must not flatten real source pendency.

If completion cannot finish inside the budget, the generation is not advanced and the query returns a typed
stale/degraded receipt. A partially completed domain set must never be published as a complete generation (INV-014).

### Acceptance criteria

- edit then immediate query returns repaired evidence for target file in normal case;
- large pending backlog does not turn query into multi-second reconciliation;
- micro-repair never invokes full reconcile;
- **end-to-end**: edit a Markdown file, let the watcher converge, and `membrane_context` returns evidence with no
  manual `blueprint build` or `phase2 seal` — measured against the installed binary, not a unit test;
- after that edit, `domainsPending` no longer holds `doc` and `indexed_revision` matches the repaired revision;
- a repair that cannot complete its pending domains inside budget leaves the previous complete generation
  published and returns a typed stale receipt.

---

## BPM-001 — Replace placeholder MCP resources with live projections

Current placeholder resources are a product bug because advertised architecture/claims/conflicts/rules/receipts can return empty/null payloads irrespective of repository state.

### Implement first

```text
blueprint://repos
blueprint://repo/{id}/context
blueprint://repo/{id}/architecture
blueprint://repo/{id}/flows
blueprint://repo/{id}/claims
blueprint://repo/{id}/conflicts
blueprint://repo/{id}/receipts
blueprint://repo/{id}/schema
```

### Context payload

Keep intentionally small:

```json
{
  "repoId": "...",
  "generation": 42,
  "freshness": "current",
  "languages": ["javascript", "rust"],
  "entryPoints": [],
  "components": [],
  "hubSymbols": [],
  "recentlyChanged": [],
  "flows": [],
  "docConflicts": [],
  "coverageGaps": [],
  "providerGaps": []
}
```

### Acceptance criteria

- values originate from application/graph services, not hard-coded placeholders;
- resources are repository-confined;
- generation/freshness included;
- unavailable projection is typed as unavailable/degraded, not silently empty.

---

## BPI-005 — Actually install lifecycle/routing hooks

The current init planning path can include a hooks action without applying it.

Implement the action.

### Git hooks

Prefer coexistence-safe installation. Do not overwrite user hooks.

Use one of:

- managed hook directory/dispatcher;
- append-safe wrapper with ownership markers;
- repository mechanism already established elsewhere in Membrane.

### Agent routing

Install concise host-specific guidance from one semantic source:

```text
Orient with blueprint_recall before broad repository crawling.
Use blueprint_search for symbol/concept discovery.
Use blueprint_expand for implementation context.
Use blueprint_impact before consequential edits.
Use blueprint_doc_truth for code/document disagreement.
Use blueprint_status when freshness/trust matters.
```

Do not maintain divergent prose semantics per editor.

---

## BPP-001 — Real resident watcher performance SLOs

Current budget infrastructure is insufficient if it does not time the actual incremental event-to-current path and measure the resident watcher process.

### Add benchmark harness

Measure:

- watcher process RSS while idle;
- idle CPU over a stable interval;
- one-file modify event to applied/current;
- rename/delete/add;
- bounded dependent repair;
- branch transition batch;
- no-op relevant-file freshness check;
- cold reconcile independently from steady-state update.

### Initial engineering targets

- idle CPU: effectively zero;
- base resident RSS target `<150 MB`;
- no-op relevant-file check p95 `<50 ms`;
- internal changed-file repair p95 `<500 ms` on representative medium fixture;
- one-file event → current p95 `<1.5 s` including 1 s debounce;
- ordinary edit performs zero full repo walks;
- resident test failures use modest environment-specific slack, not a blanket 4× allowance that masks regressions.

These are targets, not claims about existing measured performance.

---

## BPP-002 — Resource-aware reconcile scheduler

The supervisor already avoids unbounded parallel startup. Refine scheduling into work classes.

Suggested classes:

- `interactive_micro_repair` — highest priority, strict budget;
- `steady_incremental` — high priority;
- `branch_transition` — high priority/coalesced;
- `cold_reconcile` — heavyweight;
- `derived_projection_refresh` — lower priority;
- `semantic_optional` — demand/config driven.

Default heavy cold reconcile concurrency: 1.

Prioritize repositories with active queries/session ownership over dormant enrolled repos.

---

## BPM-002 — Cold-start orientation projection (BPT-097)

Generate `.agent/graph/context.json` or equivalent internal projection whenever structural generation meaningfully changes.

Required fields:

- repo ID/generation/freshness;
- languages;
- namespaces/components;
- entry points where available;
- hub symbols;
- recent structural changes;
- architecture flows;
- doc conflicts;
- provider/coverage gaps.

Constraints:

- small;
- deterministic;
- regenerable;
- no durable agent memory;
- no LLM required.

Expose the same semantics through MCP context resource.

---

# 5. Stage 2 — Semantic precision

Implement after Stage 1 reliability gates pass.

## BPS-001 — Semantic indexer orchestration (BPT-073)

Create a provider orchestration layer that discovers/invokes/validates compiler-grade semantic producers.

Conceptual provider contract:

```ts
interface SemanticIndexerProvider {
  id: string;
  detect(root): Capability;
  plan(root, sourceState): IndexPlan;
  run(plan): ProducedIndex;
  validate(index): ValidationResult;
  ingest(index, generation): IngestResult;
}
```

Requirements:

- version pin/record;
- deterministic failure reasons;
- cache compatible outputs;
- never auto-download/run arbitrary executables without explicit safe policy;
- source-state identity recorded with ingested facts;
- missing provider degrades capability, not whole Blueprint.

SCIP remains preferred interchange where available.

---

## BPS-002 — On-demand LSP verification/cross-check (BPT-074)

Implement a bounded optional verifier against an available project/host LSP. It cross-checks already-resolved current-source facts; it is not a parallel canonical index.

Rules:

- off by default and never mandatory for baseline indexing;
- never kept resident merely because a repo is enrolled;
- normal dirty-file repair/reparse establishes current Blueprint truth first;
- agreement emits a verification receipt; disagreement emits typed `resolution_conflict`;
- no automatic LSP-wins override;
- editing/refactoring actions remain outside Blueprint;
- capability/readiness exposed separately.

---

## BPS-003 — Generalized scope/name resolution (BPT-075)

Extend beyond current JS/Python module resolution toward deterministic per-language lexical/scope binding.

Recommended design principles from stack-graphs-style systems:

- file-local reusable resolution artifacts;
- explicit pre/postconditions/frontiers;
- query-time stitching where useful;
- deterministic incomplete result rather than heuristic same-name binding.

Do not make the archived upstream stack-graphs implementation an unexamined hard dependency. Reimplement/own the necessary algorithmic layer or isolate a maintained fork.

### JS/TS completion requirements

The current JS resolver is not considered complete until fixtures cover:

- `tsconfig.json` / `jsconfig.json` `baseUrl` and `paths`;
- project references where applicable;
- `package.json` `exports` and `imports`;
- conditional exports (`import`, `require`, `types`, `node`, `browser`, `default`);
- package subpaths;
- monorepo/workspace package mapping;
- barrel/re-export chains;
- explicit external-package identity.

Unresolved conditional exports must remain ambiguous/unresolved/external; never collapse plausibly to `main` and emit a false exact edge.

### Resolution rules-as-data requirement

Move stable language-specific resolver behavior into versioned, diffable rule manifests where practical. Rule data may describe scope/export/import matching, tie-breaks and feature switches; core freshness/admission/tier-dominance invariants remain code/canon and cannot be overridden. Include rule-manifest digest in extractor fingerprints and verifier fixtures.


---

## BPS-004 — Type hierarchy / MRO / override facts (BPT-076)

### Required semantic symbol substrate

Before or with member/hierarchy resolution, normalize optional symbol metadata:

- signature/parameters;
- declared and raw declared type;
- return type;
- receiver/declaring type;
- parent symbol;
- visibility/export status;
- static/async/abstract/final/override modifiers;
- annotations/decorators;
- generic/type parameters;
- docstring reference.

Member resolution then follows: exact semantic target → lexical/import binding → receiver type → declaring type/member lookup → inheritance/MRO → signature/arity/type disambiguation → framework resolver → bounded candidates/frontier.

Priority:

1. compiler/SCIP;
2. live LSP;
3. deterministic language-specific fallback.

Expose explicit provenance and unresolved cases.

---

## BPS-005 — Resolution frontier reporting (BPT-077)

Every unresolved reference/dynamic call path should be able to answer:

- where resolution stopped;
- why;
- source evidence;
- dispatch category;
- bounded candidates;
- missing provider/capability if applicable.

Add frontier information to relevant search/expand/impact responses without bloating default output.

---

## BPS-006 — Explicit dynamic-dispatch seams (BPT-078)

Support callbacks, interface dispatch, emit→listener, framework registration and similar seams only when statically evidenced.

No semantic-similarity edges.

Heuristic bridges must remain inferred and carry confidence/evidence.

---


## BPS-007 — Portable semantic identity for federation/interchange

Keep existing internal IDs. Add an optional portable identity when exact evidence permits:

- exact SCIP symbol;
- package/language/descriptor identity;
- Kythe/VName-like corpus/root/path/language/signature context.

Portable identity is for federation, external symbols and interchange. It must not trigger same-name merging.

---

## BPS-008 — Deterministic semantic export

Extend the existing export surface rather than creating another storage engine.

- emit standard SCIP for the semantic subset that maps losslessly;
- emit a versioned Blueprint-native format for Blueprint-only evidence such as resolution tiers, receipts, drift and projections;
- deterministic sort/order and schema/provider metadata;
- round-trip/golden tests for supported semantic records;
- never call a custom incompatible format “SCIP-like.”

## BPS-009 — Generation-history truth reads

Add bounded read-only helpers/CLI/API for historical truth over retained generations:

- `fact_at(entity, generation)` / equivalent;
- `changed_between(entity, g1, g2)` / equivalent;
- history/frontier receipts when retention prevents a complete answer.

Do not add a duplicate bitemporal store. Generation history is the source; historical reads remain distinct from current-code truth.

# 6. Stage 3 — Structural intelligence expansion

## BPIQ-001 — First-class test identity (BPT-079)

Represent tests as typed entities, not only filename/seed heuristics.

Support language/framework providers incrementally.

## BPIQ-002 — Static test reachability (BPT-080)

Build explicit evidence-backed production↔test relationships.

Use `UNKNOWN` when unproven. Never equate absence of static relation with absence of coverage.

Feed these facts into existing impact/test recommendation semantics.

---

## BPIQ-003 — Entry-point registry (BPT-081)

Extract deterministic entry points:

- HTTP routes;
- CLI commands;
- workers/jobs/schedulers;
- executables;
- RPC/MCP tools;
- UI routes/screens;
- tests/handlers.

Entry points become first-class seeds for architecture/process views.

---

### Canonical domain identity requirement

Framework/contract providers must distinguish source occurrences from shared domain entities. Examples:

```text
Destination = (brokerFamily, resolvedAddress)
Route = (repo/service scope, method, normalizedPath)
Table = (datastore?, schema?, table)
Service = (scope, canonicalName)
```

A publisher and consumer in different files should point to one canonical destination when the broker/address identity is actually proven. Unresolved placeholders such as `${TOPIC}` must not collapse merely because their text matches.

## BPIQ-004 — Process/Step projection (BPT-082)

Materialize reusable evidence-backed flows as derived projections.

Required properties:

- entry-point anchor;
- ordered/branching steps;
- source-backed relation IDs;
- projection generation/version;
- explicit uncertainty/frontier markers;
- disposable/rebuildable status.

Do not present Process as runtime trace truth.

---

## BPIQ-005 — Named federation groups (BPT-083)

Add logical repo grouping metadata without merging repo identity spaces.

---

## BPIQ-006 — Service/API contract registry (BPT-084)

Normalize provider/consumer structural facts for:

- HTTP method/path;
- RPC method;
- MCP/tool name/schema;
- event/topic;
- package/API identity where useful.

Direct provider declaration may be canonical; cross-repo matched linkage is a derived projection.

---

## BPIQ-007 — Cross-repo contract links and trace stitching (BPT-085/086)

Algorithm:

```text
repo-local consumer evidence
→ normalize contract identity
→ candidate provider contracts in named federation scope
→ exact compatible match rules
→ create evidence-backed ContractBridge projection
→ cross-repo path can traverse bridge
```

Never use global same-name matching.

---

## BPIQ-008 — Split framework semantic providers (BPT-087–091)

Provider families:

- dependency injection (`BPT-087`);
- ORM/query target (`BPT-088`);
- configuration binding (`BPT-089`);
- RPC/MCP/tool definition→handler (`BPT-090`);
- UI screen/navigation (`BPT-091`).

Each gets independent provider contract, tests, evidence rules and degradation state.

Do not further overload the existing generic framework atom as a catch-all.

---

## BPIQ-009 — Project convention facts (BPT-106)

Implement a deterministic descriptive conventions provider inspired by Sense.

Initial convention families:

- naming by scope/entity kind;
- error-handling shape;
- module/layout patterns;
- test-placement patterns.

Every result carries support/coverage, counterexamples, source generation and `WeakEvidence` authority. Conventions describe current code; they do not become policy. A drift consumer may cite them only as weak evidence unless an explicit architecture/rule source independently makes the convention normative.

Acceptance gate: fixtures must prove both the dominant pattern and preserved counterexamples; low-support patterns abstain rather than become conventions.

# 7. Stage 4 — Retrieval expansion

## BPR-001 — Identifier-aware BM25 (BPT-092)

Implement before embeddings.

Requirements:

- tokenize identifiers sensibly (snake/camel/path/qualified names);
- preserve exact symbol boosts;
- separately index symbol/signature and source/document text where useful;
- generation/version metadata;
- incremental changed-unit updates;
- benchmark against current bounded graph text search.

---

## BPR-002 — AST structural search (BPT-095)

Support syntax-shape/pattern queries over Tree-sitter structures.

Keep the query language intentionally bounded initially; prioritize exact useful patterns over general AST DSL complexity.

---

## BPR-003 — Compact symbol/signature projection (BPT-096)

Return outlines/signatures/imports/relationships without forcing agents to load full source bodies.

Integrate with search/expand and cold-start orientation.

---

## BPR-004 — Exploratory benchmark for local embeddings/hybrid fusion (BPT-093/094)

Blueprint already has vector persistence and brute-force cosine search in the SQLite store. Therefore this work is **not** a vector-database project. Implement the missing producer/planner/ranking lane first: optional embedder → independent candidate lists (FTS/Recall/vector) → fusion → bounded reranker. Reuse the existing store until measured scale proves it inadequate.

These remain exploratory Blueprint retrieval candidates. A passing benchmark is necessary but not sufficient: promotion also requires an explicit canon decision because current Blueprint doctrine excludes semantic/hybrid vector search as a correctness dependency.

### Gate experiment

Evaluate representative natural-language code queries with:

A. exact + graph;  
B. exact + graph + BM25;  
C. exact + graph + BM25 + dense retrieval;  
D. C + hybrid fusion.

Measure:

- relevant seed recall;
- evidence precision after Blueprint admission;
- latency;
- memory/RSS;
- index size;
- incremental update cost;
- tool-call/token savings in real agent tasks.

Implement/ship dense retrieval only if C/D materially improve useful retrieval enough to justify resource cost.

If enabled:

- model lazy-loaded;
- local by default;
- vectors disposable;
- changed semantic units only;
- model/dimension/version compatibility checks;
- candidate generation only; Recall admission unchanged.
- promotion requires an agent-task A/B against the simpler baseline when the feature adds resident cost, tool complexity, or ranking behavior; a feature that does not improve agent outcomes does not justify its complexity.

---

# 8. Stage 5 — Exploratory/advanced projections

The following remain exploratory until benchmark/use-case evidence justifies commitment to product behavior:

- code-aware reranker;
- evidence-preserving output compression beyond current bounded responses;
- subsystem/community clustering;
- hub/centrality analysis;
- cycle analysis;
- complexity/coupling metrics, including read-time hotspot/surprise projections;
- Git-derived ownership maps as weak historical evidence;
- clone detection;
- OSV vulnerability projection;
- API producer/consumer shape checking;
- route-aware API pre-change impact.

General CPG/PDG/taint remains outside Blueprint's default ownership. If ever added, use an optional provider interface and lazy execution.

---

# 9. MCP/agent work

## 9.1 Keep the six canonical tools

Do not replace them with generic mega-tools in this implementation.

Possible future `blueprint_explore` may compose `recall + search + expand` after tool-selection benchmarks.

## 9.2 Strengthen prompts/workflows

Existing prompts should become explicit task doctrine rather than one-line tool references.

Three reusable workflows are enough initially:

### Explore

1. `blueprint_status` when trust/freshness uncertain;
2. `blueprint_recall` for repository orientation;
3. `blueprint_search` for target discovery;
4. `blueprint_expand` for implementation neighborhood;
5. raw file search only for gaps/frontiers.

### Change

1. recall/search target;
2. expand surrounding implementation;
3. impact before consequential edit;
4. after edit, query/verify relevant graph state;
5. doc-truth where documentation claims are affected.

### Debug

1. identify symptom/entry point;
2. search/expand callers, handlers and flows;
3. inspect resolution frontiers;
4. use exact raw source only where Blueprint evidence stops.

Host instructions should point to these workflows without duplicating their semantics.

---

# 10. File-level implementation map

The exact repo may evolve, but current likely touchpoints are:

| Area | Current file/module | Primary changes |
|---|---|---|
| watcher events/excludes | `watchman/adapter.mjs` | Git-transition suppression/coalescing support, event metadata |
| per-repo actor | `watchman/repo-actor.mjs` | transition batch intake, micro-repair primitive reuse, work-class tagging |
| reconciliation | `watchman/reconcile.mjs` | transition/source-state reconciliation, projection readiness hooks |
| fleet scheduling | `watchman/supervisor.mjs` | resource-aware heavy-work queue, active-repo priority |
| watcher CLI | `scripts/blueprint-watch.mjs` | readiness/ownership diagnostics |
| main CLI | `scripts/blueprint.mjs` | canonical facade/status; legacy plumbing hidden/deprecated |
| facade commands | `scripts/cli/commands.mjs` | canonical init, live verification, hook apply, host adapters |
| init plan/apply | current init modules | explicit adapter model; idempotent reversible writes |
| MCP server | current MCP server module | live resources, repo discovery/context, readiness semantics |
| MCP prompts | current prompt module | explore/change/debug doctrine |
| storage/schema | current store/schema modules | canonical-vs-projection metadata; provenance/confidence cleanup |
| provider layer | parser/provider modules | semantic indexer/LSP/scope/framework contracts |
| performance tests | current performance suite/budgets | real resident process/event latency/RSS tests |

Do not mechanically create all new modules before checking existing boundaries. Prefer extending current application/domain interfaces over parallel implementations.

---

# 11. Test strategy

## 11.1 Unit tests

Required areas:

- source-state authority ordering;
- deterministic vs inferred confidence serialization;
- host detection/config planning;
- contract normalization;
- resolution frontier encoding;
- BM25 tokenization/ranking;
- projection generation compatibility.

## 11.2 Integration tests

- fresh repo `blueprint init`;
- init rerun idempotence;
- multi-host config;
- Hub alive/dead readiness;
- MCP spawn/list/status/resource probe;
- ordinary file edit;
- edit immediately followed by query;
- branch checkout/merge/rebase batch;
- watcher event gap recovery;
- nested enrolled repos;
- projection missing/degraded while canonical graph remains queryable;
- stale SCIP + dirty-source repair + LSP agreement/conflict receipt;
- cross-repo contract bridge exact-match and false-match rejection.

## 11.3 Adversarial tests

- generated `target/`, `node_modules/`, build churn does not starve useful edits;
- 30k+ irrelevant filesystem changes remain excluded;
- same symbol name in unrelated repos never creates bridge;
- same method name in unrelated types never creates call edge;
- dynamic callback unresolved returns frontier rather than guessed target;
- missing semantic indexer degrades only semantic tier;
- broken optional BM25/vector projection does not corrupt canonical facts;
- user Git hooks coexist with Blueprint hooks;
- duplicate daemon/session writers resolve through lease rather than double-apply;
- query during long cold reconcile remains bounded.

## 11.4 Golden fixtures

Maintain small multi-language repositories with known:

- imports/references/calls;
- inheritance/overrides;
- routes;
- DI/ORM/config patterns;
- tests;
- RPC/MCP handlers;
- UI navigation;
- cross-repo HTTP/RPC/event contracts;
- intentionally unresolved dynamic dispatch.

Each provider must have positive, negative and ambiguity fixtures.

## 11.5 Frozen evaluation discipline

Maintain a small pinned evaluation manifest covering:

- repository/corpus revision;
- Blueprint schema/provider versions;
- semantic-correctness fixtures;
- retrieval tasks;
- impact/path tasks;
- agent tool-selection tasks where measured;
- latency/RSS environment metadata.

When a more complex retrieval/provider technique is proposed, compare it against the current simpler baseline on the same manifest. Preserve regressions and negative results in the evaluation record; do not promote a feature merely because it wins selected examples.

This is especially required before activating `BPT-093/094` embeddings/hybrid fusion or `BPT-101` reranking.

---

# 12. Release gates

## Gate A — Reliable resident Blueprint

Required before calling the operational loop complete:

- existing partial/missing atom repair has no known blocker for affected contracts;
- `blueprint init` is canonical/idempotent;
- watcher ownership verified;
- live readiness exposed;
- Git-transition batching works;
- query-time micro-repair works and is bounded;
- MCP resources are live;
- host routing installed;
- resident watcher SLO harness exists and passes agreed thresholds;
- schema/indexer conformance harness is active for currently supported providers;
- incomplete/partial generation cannot replace the last known complete generation;
- incremental changed-file repair passes unrelated-fact shrink guards;
- every domain the watcher can mark pending has a demonstrated automatic clear path (INV-024);
- BPT-021 and BPT-043 are closed against the installed binary.

This is the first meaningful release gate.

## Gate B — Precision semantics

- semantic indexer orchestration;
- on-demand LSP provider;
- generalized scope/name resolution;
- authority lattice proven;
- resolution frontier surfaced;
- type/MRO/override facts reliable for supported languages.

## Gate C — Structural intelligence

- first-class tests/test reachability;
- entry-point registry;
- process projection;
- contract/federation bridging;
- framework provider split.

## Gate D — Retrieval sophistication

- BM25 + structural search + compact signatures shipped;
- embeddings/hybrid only if benchmark gate passes.

---

# 13. Suggested PR sequence

Keep PRs narrow enough to review and bisect.

1. **Canon schema/provenance normalization** — canonical vs projection metadata, confidence rules, source-state authority interface.
2. **Schema/indexer verifier harness** — source-anchored positive/negative/ambiguity fixtures.
3. **Completeness-safe publication + shrink guards**.
4. **Canonical init + legacy installer deprecation**.
5. **Host adapters + MCP smoke verification**.
6. **Readiness/liveness model + init completion status**.
7. **Git lifecycle hooks + transition journal batch**.
8. **Bounded query-time repair**.
9. **Live MCP resources + cold-start context**.
10. **Resident watcher benchmark/SLO harness + frozen evaluation manifest**.
11. **Resource-aware supervisor scheduling**.
12. **Semantic indexer orchestration**.
13. **LSP cross-check + resolution-conflict tests**.
14. **Generalized resolution + frontier reporting**.
15. **Test entities/reachability + entry points**.
16. **Process/contract/federation projections**.
17. **Framework provider split**.
18. **BM25 + signatures + structural search**.
19. **Convention provider + weak-evidence fixtures**.
20. **Deterministic SCIP/native export + generation-history read tests**.
21. **Embedding/hybrid benchmark experiment; canon promotion only on pass + explicit decision**.

---

# 14. Rollback / migration doctrine

Every stage must preserve a path back to the previous stable graph behavior.

- schema changes use versioned migrations and compatibility reads where needed;
- projections are rebuildable and may be dropped/recreated;
- init changes track Blueprint-owned modifications for uninstall;
- hooks are reversible and coexistence-safe;
- semantic providers are feature/capability gated;
- a failed optional provider must not block baseline structural queries;
- new result fields should be additive until the next intentional breaking version;
- do not remove legacy installer bin until canonical init has been exercised in supported hosts.

---

# 15. Final definition of done

Blueprint is ready for continuous use when a developer can enter a repository, run one canonical init, and thereafter rely on the following without manual graph maintenance:

1. detected agents can connect to Blueprint;
2. watcher ownership is real and visible;
3. ordinary file changes update bounded graph state automatically;
4. Git branch transitions do not cause pathological churn;
5. an immediate query after an edit either repairs the relevant source quickly or explicitly reports stale evidence;
6. MCP resources provide live repository context instead of placeholders;
7. results expose evidence, generation and freshness;
8. semantic providers improve precision when available without becoming baseline dependencies;
9. unresolved semantics are reported honestly;
10. idle resource cost remains low enough that Blueprint can stay enabled continuously;
11. canonical facts are protected from projection/search/provider churn;
12. agent workflows use Blueprint as the first structural intelligence layer rather than rediscovering the repository blindly.


---

# 16. Donor-use execution rule

Before implementing a donor-influenced atom, the PR must identify the exact Blueprint atom/task, the donor(s) studied, the behavior being borrowed, and whether the implementation is copied/adapted or independently reimplemented. GitNexus source is design prior art only unless separately licensed. Current repository license files, not secondary comparison tables, control code-absorption decisions. See `04_BLUEPRINT_DONOR_REFERENCE_V2.md`.
