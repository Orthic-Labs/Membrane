# Monorepo Merge + Subsystem Rename — Canonical Migration Plan

**Status:** canonical migration plan · not yet executed  
**Date:** 2026-08-19  
**Target repository:** `Orthic-Labs/Membrane`  
**Scope:** merge the former `Orthic-Labs/Cortex` repository into Membrane; rename **Cortex → Blueprint**; rename **Crypt → Cortex**.  
**Architecture authority:** `BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md` and `MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md`.  
**Rule:** this plan may execute already-canonical seam hardening required to make the migration safe, but it does not change subsystem semantic ownership.

**Canonical naming after completion**

| Before | After | Meaning |
|---|---|---|
| Cortex | **Blueprint** | repository truth/evidence engine; Node; own SQLite; own daemon + watcher |
| Crypt | **Cortex** | durable-knowledge engine; Rust; own SQLite; Membrane data plane |
| Membrane | Membrane | context control plane |

This document supersedes every earlier monorepo/rename plan for these systems.

---

# 0. Governing decisions

## 0.1 Repository boundary is not architecture boundary

> **Physical co-location does not imply semantic ownership.** Blueprint and Membrane share a repository so their seam can evolve atomically; they retain separate package, process, protocol, storage, testing, and responsibility boundaries.

The merge creates:

```text
one source tree
one atomic change boundary
one integration authority
```

At the product/system level, Membrane is the parent system. Blueprint and Cortex are named Membrane subsystems with independent runtime/storage/protocol boundaries. Guide, Adapt, and Push are also named Membrane subsystems but are outside this merge/rename plan's physical scope.

It does **not** create:

```text
one process
one database
one planner
one protocol owner
one semantic subsystem
```

## 0.2 The name reassignment is a hard semantic migration

`Cortex` is not merely retired. The name is reassigned from the repository-truth engine to the durable-knowledge engine.

Therefore the migration must never allow a bare `cortex` runtime/config identifier to mean two systems simultaneously.

This changes the compatibility strategy from the earlier draft:

- no old-Blueprint `cortex` binary shim survives into the new Cortex era;
- no old-Blueprint `CORTEX_*` runtime environment fallback survives into the new Cortex era;
- no old-Blueprint `membrane_cortex` alias remains enabled by default once the name is reassigned;
- old Blueprint IPC names are not reused by the new Cortex subsystem;
- frozen historical wire tokens remain readable only through explicitly versioned/ledgered compatibility code.

## 0.3 Existing V1 wire contracts do not get renamed for branding

The public Membrane V1 shapes remain stable.

A product rename alone does not justify breaking:

- field names;
- enum/reason values;
- historical receipt provider values;
- serialized compatibility aliases.

If a V1 token contains `cortex` or `crypt`, it keeps its historical semantic meaning until a separately justified protocol version replaces it.

The rename gate therefore means **zero unclassified active old-product tokens**, not destructive rewriting of immutable history or frozen V1 protocol data.

## 0.4 New runtime namespaces

To avoid old/new `CORTEX_*` collisions:

### Blueprint

```text
package:   @orthic-labs/blueprint
bins:      blueprint
           orthic-blueprint
           blueprint-watch
           blueprint-mcp
           blueprint-install
env:       BLUEPRINT_*
IPC:       orthic-blueprint-<identity>
provider:  blueprint on new internal/current surfaces
tool:      membrane_blueprint
```

### Cortex durable-knowledge subsystem

Cortex is a Membrane subsystem, not a second standalone repository-truth CLI.

```text
Rust crates:  cortex
              cortex-core
              cortex-store
              cortex-format

bins:         membrane-cortex
              membrane-cortex-service

env:          MEMBRANE_CORTEX_*

store:        cortex-engine.db
```

The new durable subsystem does **not** claim the bare global `cortex` executable and does not use the generic `CORTEX_*` environment namespace. This prevents stale installs/configuration from being interpreted as the wrong system.

There is no raw public `membrane_cortex` memory CRUD tool. Durable Cortex is accessed through Membrane's governed context/knowledge APIs.

---

# 1. Preflight facts and hazards

The occurrence counts in the uploaded draft were measured snapshots. They are useful blast-radius evidence, not execution authority.

Before implementation, Phase 0 remeasures against the exact clean checkouts being merged.

## 1.1 Rename-A hazard: `Cortex → Blueprint`

Audit:

- all `Cortex`, `cortex`, `CORTEX_*`;
- package names;
- binary names;
- IPC/socket names;
- MCP names;
- provider IDs;
- schema IDs;
- persisted receipt reason/provider values;
- config filenames;
- installers;
- catalog/workspace paths;
- release manifests;
- generated docs;
- external package/repository links.

## 1.2 Hidden hazard: Blueprint was an earlier historical name

Membrane previously used `Blueprint` before the repository-truth subsystem was renamed Cortex.

Before introducing the new canonical Blueprint name, inventory all existing:

```text
Blueprint
blueprint
BLUEPRINT_
blueprintGeneration
blueprintBaseCommit
.blueprint/
provider=blueprint
```

Every occurrence must be classified as one of:

```text
DELETE_LEGACY_ALIAS
MIGRATE_TO_NEW_BLUEPRINT
FROZEN_HISTORICAL_WIRE
IMMUTABLE_RESEARCH_PROVENANCE
UNRELATED_TERM
```

A stale historical alias must not become canonical merely because the name is being reused.

## 1.3 Rename-B hazard: `Crypt → Cortex`

Audit:

- Rust package/crate names;
- binary/service names;
- `CRYPT_*` variables;
- store path and store identity;
- release-generation variables;
- install manifests;
- service launchers;
- provider IDs;
- V1 fields such as legacy `cryptStatus`;
- persisted events/receipts;
- docs/examples;
- Hub views and labels.

## 1.4 Store migration hazard

Current history shows the durable store has existed under `crypt-engine.db`; the migration plan must discover the actual canonical runtime path from the store resolver/identity rather than assume `crypt.db`.

Never rename an open SQLite file in place while a service may retain file descriptors.

The canonical migration is:

```text
drain writer/service
→ resolve authoritative old store path
→ verified backup/copy to temp
→ open + integrity/schema/identity verification
→ fsync
→ atomic adopt as cortex-engine.db
→ update store identity
→ restart/readback
→ retain rollback copy until qualification gate
```

---

# 2. Target monorepo layout

```text
membrane/
│
├── blueprint/                         # repository-truth subsystem
│   ├── package.json                   # @orthic-labs/blueprint
│   ├── src/
│   ├── scripts/
│   ├── schemas/                       # canonical Blueprint wire schemas
│   ├── evals/
│   ├── tests/
│   ├── release/
│   └── docs/
│
├── engine/
│   ├── Cargo.toml
│   ├── crates/
│   │   ├── cortex/
│   │   ├── cortex-core/
│   │   ├── cortex-store/
│   │   ├── cortex-format/
│   │   └── membrane-*/
│   └── federation/
│       └── providers/
│           ├── blueprint.py
│           └── cortex.py
│
├── mcp/
├── schemas/                           # Membrane-owned schemas
├── tests/
│   └── integration/
├── docs/
│   ├── MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md
│   └── plans/
│       └── 2026-08-19-monorepo-merge-and-subsystem-rename.md
├── package.json                       # private root
├── pnpm-workspace.yaml                # actual pnpm workspace definition
└── pnpm-lock.yaml                     # one root workspace lockfile
```

Blueprint's SSOT lives with its subsystem:

```text
blueprint/docs/BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md
```

## 2.1 No generic shared-contract bucket

Do not create a hand-maintained `packages/contracts`.

Do not create a second Membrane protocol authority beside the existing `engine/crates/membrane-protocol`.

### Blueprint contract ownership

Canonical wire schemas live under:

```text
blueprint/schemas/**
```

Blueprint SDK/type bindings are projections of those schemas.

Membrane may consume the exported Blueprint protocol/schema surface, but:

```text
engine/**
mcp/**
```

must not import:

```text
blueprint/src/**
```

If generated consumer bindings are needed for Python/Rust, they are generated artifacts from Blueprint-owned schemas and CI verifies regeneration byte-for-byte. They are not independent contract sources.

## 2.2 pnpm workspace ownership

The repository already has `pnpm-workspace.yaml`; the monorepo package set is declared there.

Do not rely on a root `package.json.workspaces` field as the pnpm workspace authority.

Final workspace configuration lists only real package directories, beginning with:

```yaml
packages:
  - blueprint
```

and adds additional package directories only if they actually contain package manifests.

The root remains `private: true`.

Use one root pnpm lockfile after workspace adoption. Do not maintain an independently drifting nested Blueprint lockfile.

---

# 3. Locked subsystem boundaries

CI enforces all of these.

## 3.1 Storage

- Blueprint owns `.agent/graph/graph.db`.
- Cortex owns the Membrane durable-knowledge database.
- Membrane never opens Blueprint's SQLite store directly.
- Blueprint never opens Cortex's durable store.
- No raw store merge occurs.

## 3.2 Process

- Blueprint keeps its resident service.
- Blueprint keeps its watcher.
- Membrane keeps its Application / Control / Data process architecture.
- Cortex remains part of Membrane's data/runtime subsystem.
- Repository co-location does not authorize in-process graph shortcuts.

## 3.3 Imports

Allowed:

```text
Membrane → Blueprint public protocol/service/client surface
Blueprint → Blueprint internals
Membrane → Cortex durable-memory crates through canonical Membrane owners
```

Forbidden:

```text
engine/** → blueprint/src/**
mcp/**    → blueprint/src/**
blueprint/** → engine/**
blueprint/** → mcp/**
Blueprint → Cortex store
Membrane → Blueprint store
```

## 3.4 Trust

Repository text remains:

```text
data_only
```

across the Blueprint → Membrane seam.

A rename never upgrades trust, authority, or instruction influence.

---

# 4. CI enforcement

Add one root boundary/rename checker.

It validates:

1. import direction;
2. store ownership;
3. generated schema/binding equivalence;
4. active-name ownership;
5. frozen legacy-wire allowlist;
6. no ambiguous old-product token.

## 4.1 Machine-readable rename ledger

Use one checked-in machine-readable ledger, for example:

```text
docs/migrations/2026-08-19-rename-ledger.json
```

Each surviving old-name occurrence records:

```text
path
token
classification
reason
protocol/version if applicable
removal_condition
```

Allowed classifications:

```text
frozen_wire
historical_provenance
external_legacy_url
migration_fixture
```

No broad path exemption such as `docs/**` or `research/**` is accepted without classifying the actual token family.

## 4.2 Semantic rename gate

The gate is not:

```bash
git grep cortex | wc -l == 0
```

because frozen wire values, immutable research, historical URLs and the new durable Cortex legitimately contain the token.

The gate is:

> every `cortex` / `crypt` / `blueprint` token is either owned by its new canonical subsystem or is explicitly classified in the rename ledger.

After Rename B:

```text
Blueprint meaning:
    Blueprint / blueprint / BLUEPRINT_*

Cortex durable meaning:
    Cortex in prose
    cortex-* Rust crates
    MEMBRANE_CORTEX_*
    cortex-engine.db

legacy old-repository Cortex:
    only frozen/ledgered historical wire, URLs, migration fixtures

legacy Crypt:
    only frozen/ledgered V1 wire, migration fixtures, compatibility read paths
```

---

# 5. Migration phases

Each phase is a separate PR and must be independently reviewable.

Do not combine the two semantic renames into one PR.

---

## Phase 0 — Freeze, inventory, adopt the three authorities

### Do

- Adopt:
  - `BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md`;
  - `MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md`;
  - this migration plan.
- Freeze exact clean SHAs of both pre-merge repositories.
- Run full available suites in both repositories.
- Record baseline test failures rather than silently treating known red as green.
- Snapshot release/package surfaces:
  - Blueprint npm tarball;
  - CLI/bin inventory;
  - MCP/plugin manifests;
  - installers;
  - Hub add-on manifests;
  - store identity/path;
  - IPC endpoint;
  - environment-variable inventory.
- Remeasure rename occurrence/file/path counts.
- Inventory all pre-existing `blueprint*` legacy aliases in Membrane and the former Cortex tree.
- Build the initial rename ledger.
- Resolve phantom `SEAM-CONTRACT.md` prerequisites; the subsystem SSOTs own their seam semantics.

### Gate

- both checkouts clean;
- exact SHAs recorded;
- current failures recorded;
- package/install/store snapshots captured;
- every pre-existing `blueprint*` occurrence classified.

---

## Phase 1 — Import Blueprint history, no semantic rename

### Do

From the Membrane checkout:

```bash
git remote add blueprint-src <path-or-url-to-former-cortex-repo>
git fetch blueprint-src
git subtree add --prefix=blueprint blueprint-src main
```

Do not use `--squash`; imported history remains reachable.

At this phase:

- subsystem content remains semantically Cortex internally;
- the physical prefix is already `blueprint/`;
- no product identifiers are renamed yet.

### Workspace changes

Update `pnpm-workspace.yaml` to include the imported package.

Create one root lockfile containing both package dependency graphs.

Remove the nested lockfile only after root install/test/package equivalence is proven.

The root package stays private.

### CI

Run:

- Blueprint Node suite from the workspace;
- Membrane Node suite;
- Rust workspace;
- Python federation suite;
- existing legal/productization gates.

### Package equivalence

Compare the pre-merge Blueprint npm pack snapshot with:

```bash
pnpm --dir blueprint pack
```

Differences caused only by repository metadata/path relocation must be explicitly classified; runtime package contents must remain equivalent.

### Gate

- unified CI reaches the frozen baseline or better;
- imported history is reachable;
- Blueprint package output is equivalent;
- no semantic rename occurred.

---

## Phase 2 — Boundary enforcement + canonical seam alignment

This phase banks the monorepo benefit before names move.

### Contract ownership

- Blueprint wire schemas stay canonical in `blueprint/schemas/**`.
- Blueprint SDK/bindings are generated or equivalence-checked from the canonical schema source.
- Membrane consumer bindings may be generated from those schemas but are never independently edited.

### Boundary lint

Add the §3/§4 checks.

A deliberately planted direct import/store violation must fail CI.

### Integration tests

Add real-stack tests that:

1. start the Blueprint service;
2. build/pin a repository generation;
3. query through Membrane;
4. assert schema/generation identity;
5. assert fail-closed mismatch;
6. assert repository text remains `data_only`;
7. assert Membrane never reads Blueprint SQLite directly.

### Daemon-first seam

The final SSOTs already require resident Blueprint transport. Pull that already-canonical seam requirement forward before renaming:

```text
Membrane
→ long-lived Blueprint client
→ Blueprint local service
→ RecallCircuit / resolve / other public operations
```

Per-query Node subprocess becomes a typed fallback only.

This is seam hardening already required by the SSOTs, not a new ownership change created by the monorepo.

### Gate

- real service integration green;
- daemon is normal Membrane path;
- subprocess is typed fallback;
- import/store lints proven;
- schema/binding regeneration/equivalence green.

---

## Phase 3 — Rename A: repository-truth Cortex → Blueprint

One PR. Prefer one mechanically focused rename commit plus narrowly separated generated-artifact updates if tooling requires them; do not mix feature work.

### Rename active surfaces

```text
Cortex       → Blueprint
cortex       → blueprint
CORTEX_*     → BLUEPRINT_*
```

where the token semantically names the repository-truth product.

Rename:

- package → `@orthic-labs/blueprint`;
- bins → Blueprint bin names;
- service/socket names → Blueprint namespace;
- provider module → `providers/blueprint.py`;
- current provider IDs → `blueprint` where not frozen V1 history;
- MCP tool → `membrane_blueprint`;
- installer/product labels;
- config filenames;
- release manifests;
- schema IDs for new schema versions;
- current docs and generated docs.

### Do not mechanically rename

- immutable git history;
- frozen historical receipts;
- V1 field/reason names whose mutation would break the protocol;
- research quotations/provenance;
- external old repository URLs;
- unrelated uses of the word cortex.

Those are ledgered.

### Public tool change

`membrane_cortex` currently means repository truth.

Because `Cortex` is being reassigned, do **not** keep it as a normal alias for Blueprint.

Introduce `membrane_blueprint` with an explicit operation/tool-catalog version bump.

If a legacy toolset must exist for migration testing, it is disabled by default and clearly named as legacy; it does not coexist in the normal catalog.

### npm

Publish `@orthic-labs/blueprint`.

Deprecate existing `@orthic-labs/cortex` versions with a migration message.

Do not publish a runtime re-export package that continues to install the bare `cortex` executable.

The old package name is a historical migration pointer, not a second live identity.

### Gate

- all active repository-truth runtime/config names use Blueprint;
- no normal old-Blueprint `cortex` bin/env/socket/tool alias remains;
- all surviving old names are in the rename ledger;
- clean install of `@orthic-labs/blueprint` works;
- Membrane integration uses Blueprint names;
- V1 compatibility fixtures remain readable.

---

## Phase 4 — Namespace quarantine before reassigning Cortex

This phase exists specifically because the same token is being reused.

### Remove/disable old active namespace

Confirm:

- no old repository-truth runtime reads `CORTEX_*`;
- no old repository-truth process listens on `orthic-cortex-*`;
- no active old repository-truth binary named `cortex` is installed by the new Blueprint package;
- no default MCP tool named `membrane_cortex` still points to Blueprint;
- no active provider lookup interprets `"cortex"` as current Blueprint unless reading explicitly historical V1 data.

### Configuration migration

For user-facing files/state that previously named Cortex:

- installer/upgrade tooling performs a one-time explicit migration to Blueprint names;
- migration is idempotent;
- it emits a receipt/report;
- old file is retained only when required for rollback;
- runtime does not indefinitely read both ambiguous namespaces.

### Gate

A semantic namespace test proves that a fresh/current process cannot interpret the token `cortex` as the repository-truth subsystem.

Only then may Rename B begin.

---

## Phase 5 — Rename B: durable-knowledge Crypt → Cortex

### Rust/source names

```text
crypt            → cortex
crypt-core       → cortex-core
crypt-store      → cortex-store
crypt-format     → cortex-format
```

Update:

- Cargo workspace members;
- crate package names;
- Rust imports/modules;
- release/build scripts;
- generated docs;
- tests;
- Hub labels;
- provider adapter filename/identity where not frozen by V1.

### Runtime names

Do not claim the bare `cortex` global executable.

Use:

```text
membrane-cortex
membrane-cortex-service
```

Use:

```text
MEMBRANE_CORTEX_*
```

for new environment variables.

During the safe migration window, new Cortex may read legacy `CRYPT_*` variables because those names cannot be mistaken for old Blueprint. Emit deprecation diagnostics and remove that fallback at the stated release boundary.

Never use old repository-truth `CORTEX_*` variables as a fallback for durable Cortex.

### Store

Canonical new install name:

```text
cortex-engine.db
```

Migration:

```text
drain
→ resolve current store identity
→ verified copy/backup
→ integrity/schema/readback
→ atomic adopt
→ identity update
→ restart
→ recall-equivalence check
```

No open-file rename-in-place.

Keep the verified prior store as rollback material until the phase gate passes.

### Public Membrane V1

Do not rename V1 fields solely for branding.

For example, a frozen V1 `cryptStatus` field may remain as a historical wire name meaning durable-knowledge availability. Document that mapping in the rename ledger.

Likewise, historical receipt/provider values remain readable with their original meaning.

### Gate

- Rust workspace green under new crate names;
- current runtime labels use Cortex;
- no durable Cortex code consumes old Blueprint `CORTEX_*`;
- store migration passes integrity + backup/restore + recall-equivalence;
- V1 old-data fixtures remain readable;
- rollback to the verified old store is tested.

---

## Phase 6 — Packaging, Hub, docs, repository retirement

### Membrane bundle

The primary Membrane installation may ship one Hub add-on/bundle that manages both:

- Membrane/Cortex durable engine;
- Blueprint service/watcher.

The add-on still models them as distinct children/subsystems.

### Blueprint standalone distribution

Blueprint remains independently:

- packageable;
- publishable;
- runnable;
- testable;
- usable through CLI/MCP/service without Membrane.

Monorepo location does not remove standalone distribution.

### Doctor

One top-level Membrane doctor may aggregate health, but output keeps distinct sections for:

```text
Membrane
Cortex durable store
Blueprint evidence store/service
```

No repair command crosses ownership boundaries silently.

### Docs

Canonical normative docs after the migration are exactly:

```text
blueprint/docs/BLUEPRINT_CANONICAL_SOURCE_OF_TRUTH.md
docs/subsystems/MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md
docs/subsystems/2026-08-19-monorepo-merge-and-subsystem-rename.md
```

Earlier architecture/implementation/absorption plans are deleted or reduced to non-normative provenance only after inbound links are fixed.

Generated `docs/product.md` / `docs/architecture.md` remain generated runtime truth and are not replaced by the three planning authorities.

### Old GitHub repository

Do not claim GitHub can automatically redirect `Orthic-Labs/Cortex` to a subdirectory of the existing Membrane repository.

Before archiving the old repository:

1. update its README and description to point to the canonical monorepo Blueprint path;
2. close/transfer any live issues that must remain actionable;
3. update release/package links;
4. then archive it read-only.

Keep it as historical provenance until all install/upgrade gates pass.

### Gate

- Mac installed-path qualification green;
- Windows installed-path qualification green;
- standalone Blueprint package qualification green;
- combined Membrane bundle qualification green;
- docs/link audit green;
- old repo archived only after migration links are live.

---

# 6. Human-reviewed surfaces

Mechanical rename is insufficient for these.

## 6.1 Blueprint rename

Review:

- npm package metadata and published bins;
- plugin/MCP/server manifests;
- `BLUEPRINT_*` env namespace;
- IPC/socket/pipe names;
- schema `$id` and catalog mappings;
- source/on-disk install-state filenames;
- Homebrew/Scoop/WinGet/Inno/Linux packaging;
- user config filenames;
- eval/performance baseline identities;
- provider IDs and receipt reason codes;
- `membrane_blueprint` tool catalog/version;
- workspace catalog/path references;
- old GitHub/npm links;
- historical `blueprintGeneration` aliases from the earlier product-name cycle.

## 6.2 Cortex durable rename

Review:

- four Rust crate directories/package names;
- binary names;
- `MEMBRANE_CORTEX_*` variables;
- release-generation variables;
- actual resolved store path;
- store identity record;
- backup/restore;
- provider IDs;
- V1 `crypt*` wire names;
- historical data/receipts;
- docs/examples;
- Hub views;
- service wrappers;
- installer and upgrade state.

## 6.3 Cross-rename collision test

Add a fixture containing simultaneously:

```text
legacy old Blueprint-via-Cortex receipt
current Blueprint receipt
legacy Crypt durable record
current Cortex durable record
```

Assert every item resolves to exactly one semantic owner.

No decoder may infer ownership from the bare English product name alone when protocol/version/provenance says otherwise.

---

# 7. Rollback

| Phase | Rollback |
|---|---|
| 1 import | revert subtree/workspace integration; old repository remains authoritative |
| 2 boundaries | revert generated bindings/lints/seam client; no data migration |
| 3 Blueprint rename | revert rename before Rename B; npm deprecation can be corrected with a follow-up release |
| 4 quarantine | restore Blueprint namespace only if Rename B has not begun |
| 5 Cortex rename | code rollback plus explicit store-identity rollback to the verified retained old store; never rely on `git revert` alone |
| 6 packaging/docs | keep prior install artifacts available until new qualification is green |

## 7.1 Point of no ambiguous rollback

After Rename B begins, never restore old repository-truth `cortex` runtime aliases in the same installation.

Rollback of Blueprint after that point means restoring a **Blueprint-named** build, not reintroducing old `cortex` binaries/env/socket names.

This prevents rollback itself from recreating the name collision.

---

# 8. Definition of Done

The monorepo/rename migration is complete only when all are true.

## Repository

- [ ] Blueprint history is reachable from the Membrane monorepo.
- [ ] One root pnpm workspace/lockfile is authoritative.
- [ ] Root package remains private.
- [ ] Blueprint remains independently packageable/publishable/testable.

## Boundaries

- [ ] Membrane does not import `blueprint/src/**`.
- [ ] Blueprint does not import Membrane engine/MCP internals.
- [ ] Membrane never opens Blueprint SQLite.
- [ ] Blueprint never opens Cortex durable SQLite.
- [ ] Real daemon integration test proves the seam.
- [ ] Repository text remains `data_only`.

## Naming

- [ ] Repository truth is called Blueprint on all current active surfaces.
- [ ] Durable knowledge is called Cortex on all current active surfaces.
- [ ] New Blueprint uses `BLUEPRINT_*`.
- [ ] New durable Cortex uses `MEMBRANE_CORTEX_*`.
- [ ] New durable Cortex does not install a bare global `cortex` binary.
- [ ] `membrane_blueprint` is the repository-truth MCP tool.
- [ ] No default `membrane_cortex` alias still points to Blueprint.
- [ ] Every surviving legacy `cortex`/`crypt` token is classified in the rename ledger.
- [ ] Every pre-existing historical `blueprint*` alias was classified before Rename A.

## Protocol

- [ ] Blueprint schemas have one canonical owner.
- [ ] Generated/consumer bindings regenerate/equivalence-check cleanly.
- [ ] Membrane V1 shapes remain compatible.
- [ ] Frozen V1 old-name tokens retain their historical meaning.
- [ ] Provider/reason decoding can distinguish historical old Cortex from current Blueprint/Cortex state.

## Store migration

- [ ] Actual pre-rename durable store path was resolved, not guessed.
- [ ] New canonical store is `cortex-engine.db`.
- [ ] Migration drains the writer and uses verified atomic adoption.
- [ ] Integrity/schema/readback green.
- [ ] Backup/restore and recall equivalence green.
- [ ] Rollback store retained until qualification passes.

## Runtime

- [ ] Blueprint service + watcher remain separate responsibilities.
- [ ] Membrane normal Blueprint path is persistent daemon IPC.
- [ ] Per-query subprocess is typed fallback only.
- [ ] Cortex remains Membrane-owned durable knowledge, not a second repository-truth service.

## Packaging

- [ ] `@orthic-labs/blueprint` clean install works.
- [ ] Old `@orthic-labs/cortex` package is deprecated as historical repository-truth name and installs no conflicting runtime shim.
- [ ] Combined Membrane bundle works.
- [ ] Standalone Blueprint works.
- [ ] Mac qualification green.
- [ ] Windows qualification green.

## Documentation

- [ ] Exactly three normative planning/architecture documents remain.
- [ ] Generated product/architecture docs regenerate from landed code.
- [ ] Earlier implementation/absorption plans are retired as authority.
- [ ] Old GitHub repository README points to the monorepo before archive.
- [ ] Old repository is archived only after migration qualification.

---

# 9. Final migration statement

The target is not a merged application.

The semantic system hierarchy is:

```text
Membrane
├── Blueprint   repository evidence + repository truth
├── Cortex      durable governed knowledge
├── Guide       document navigation + section index
├── Adapt       learning + governed proposals
└── Push        reversible reduction
```

Membrane's core planner remains the context control plane across those subsystems.

This migration changes the physical placement/names of Blueprint and Cortex only. Guide, Adapt, and Push remain Membrane subsystems but their physical placement is not decided by this plan.

Within the `Orthic-Labs/Membrane` repository, Blueprint and Cortex retain their own process/storage/protocol boundaries even though they sit under the Membrane system.

The repository merge exists so a seam change can land atomically.

The process, storage, protocol, trust and semantic boundaries remain real.

The rename is complete only when **Blueprint** can no longer be confused with historical Cortex, and **Cortex** can no longer be confused with the repository-truth product that previously carried that name.
