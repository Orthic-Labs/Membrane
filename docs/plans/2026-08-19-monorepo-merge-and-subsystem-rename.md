# Monorepo Merge + Subsystem Rename

**Status:** plan · not yet executed
**Date:** 2026-08-19
**Scope:** merge `Orthic-Labs/Cortex` into `Orthic-Labs/Membrane`; rename `Cortex → Blueprint`; rename `Crypt → Cortex`
**Authority:** subordinate to `CORTEX_CANONICAL_SOURCE_OF_TRUTH.md` (→ Blueprint) and `MEMBRANE_CANONICAL_ARCHITECTURE_AND_IMPLEMENTATION_DOCTRINE.md`. This plan changes physical layout and names only. It changes no architecture.

---

## 0. The governing sentence

Add to both canonical doctrines:

> **Physical co-location does not imply semantic ownership.** Blueprint and Membrane share a repository so their seam can evolve atomically; they retain separate package, process, protocol, storage, testing, and responsibility boundaries.

---

## 1. Naming decision

| Was | Becomes | What it is |
|---|---|---|
| Cortex | **Blueprint** | repository truth/evidence engine (Node, own SQLite, own daemon+watcher) |
| Crypt | **Cortex** | durable-knowledge engine (Rust, own SQLite, Membrane data plane) |
| Membrane | Membrane | context control plane (unchanged) |

**The hazard:** `Cortex` is not being retired, it is being *reassigned*. For a window, the token `cortex` in git history, docs, memories, fixtures, receipts, and env vars is ambiguous.

**The rule that removes the hazard:** the two renames are separate, ordered, gated commits. Rename A (Cortex→Blueprint) must reach **zero occurrences of `cortex`/`CORTEX_`/`Cortex` in the tree** before Rename B (Crypt→Cortex) introduces the token again. A CI gate enforces this between the two commits. They are never in flight together, never in one branch, never partially applied.

---

## 2. Blast radius (measured, not estimated)

Measured at Blueprint `bd46965` and Membrane `662d4e3`.

| Rename | Occurrences | Files | Path renames |
|---|---|---|---|
| A: Cortex → Blueprint (in Cortex repo) | 2,765 | 360 | 42 |
| A: Cortex → Blueprint (in Membrane repo) | 1,302 | 116 | ~10 fixtures |
| B: Crypt → Cortex (in Membrane repo) | 2,583 | 347 | 87 |

Most of this is mechanical. The list in §6 is the part that is *not*.

---

## 3. Target layout

```
membrane/                          # repo root (keeps the Membrane name)
│
├── blueprint/                     # ex-Cortex, whole tree, own package.json
│   ├── src/  scripts/  schemas/  evals/  tests/  release/
│   └── package.json               # @orthic-labs/blueprint, publishable, versioned independently
│
├── engine/
│   ├── Cargo.toml                 # workspace
│   ├── crates/
│   │   ├── cortex/  cortex-core/  cortex-store/  cortex-format/   # ex-crypt-*
│   │   └── membrane-*/
│   └── federation/
│       └── providers/blueprint.py # ex-cortex.py
│
├── mcp/
│
├── packages/
│   ├── membrane-protocol/         # ScopeGrant, ContextCandidateSet, ContextPacket,
│   │                              # ContextReceipt, KnowledgeEmission
│   └── blueprint-protocol/        # RecallCircuit, resolution states, generation/freshness,
│                                  # truth findings — GENERATED from blueprint/schemas
├── schemas/                       # Membrane-owned schemas
├── tests/integration/             # real daemon + real stack
├── docs/
└── package.json                   # private root, workspaces
```

### 3.1 Protocol ownership is directional

```
        Membrane
           │ depends on
           ▼
   blueprint-protocol      (Blueprint-owned contract)
           │ generated from
           ▼
       Blueprint
```

Blueprint does **not** depend on `membrane-protocol`. There is no shared "contracts" bucket.

**`blueprint-protocol` is generated, never hand-maintained.** It is built from `blueprint/schemas/**` + `blueprint/src/sdk/types.d.ts` by a Blueprint build script, committed, and verified in CI (`generate && git diff --exit-code`). Without this rule the package becomes a third copy of the contract — the exact drift the merge exists to kill.

---

## 4. Invariants preserved through the merge

Locked; a CI rule enforces each.

- Blueprint keeps its own SQLite database (`.agent/graph/graph.db`).
- Cortex (ex-Crypt) keeps its own SQLite database.
- Membrane never opens Blueprint's database. Blueprint never opens Cortex's.
- Blueprint keeps its resident service and its watcher.
- Membrane reaches Blueprint only through `blueprint-protocol` + the Blueprint service/CLI — exactly as an external consumer would.
- **`engine/**` and `mcp/**` may not import `blueprint/src/**`.** Banned by lint, not by convention.
- `blueprint/**` may not import `engine/**` or `mcp/**`.
- Repository text stays `data_only` across the seam.
- Blueprint stays independently testable, independently packageable, independently publishable.

### 4.1 CI boundary rules

```
engine/**, mcp/**   →  MAY import packages/blueprint-protocol
                       MUST NOT import blueprint/src/**
blueprint/**        →  MUST NOT import engine/**, mcp/**, packages/membrane-protocol
store access        →  only blueprint/** opens the Blueprint store
                       only engine/crates/cortex-store/** opens the Cortex store
```

Implement as a repo-root check script in CI (path-prefix grep over import/`use` statements) plus a workspace dependency-graph assertion.

---

## 5. Phases

Each phase is one PR, independently revertable. **No phase may be combined with another.**

### Phase 0 — Freeze and baseline

- Land the pending `docs/CORTEX_SOURCE_OF_TRUTH_REVISED.md` rename/adoption decision in the Cortex repo (do not carry an uncommitted rename into a subtree import).
- Resolve the phantom `docs/plans/orthic/SEAM-CONTRACT.md` rule in Cortex `docs/agent-rules.md` (delete the line; §7.8/§17.6 of the canonical doc is the seam authority).
- Green `pnpm test:all` in Cortex; green Rust/Python suites in Membrane. Record both SHAs.
- Snapshot current release artifacts: npm tarball contents, Homebrew/scoop/winget manifests, Hub add-on manifest.

**Gate:** both trees green, both SHAs recorded, no uncommitted work in either repo.

### Phase 1 — Merge, no renames

```bash
cd /Volumes/D/claude/membrane
git remote add blueprint-src /Volumes/D/claude/cortex
git fetch blueprint-src
git subtree add --prefix=blueprint blueprint-src main
```

- History preserved. Zero identifier changes. Zero file-content changes inside `blueprint/`.
- Root `package.json`: add `workspaces: ["blueprint", "packages/*", "mcp"]`; stays `private: true`.
- CI matrix gains the Blueprint Node suite alongside Rust + Python.
- `blueprint/CLAUDE.md` (currently `cortex/CLAUDE.md`) keeps working via the existing `@docs/agent-rules.md` import.

**Gate:** all three suites green in one CI run; `blueprint/` still publishes an identical npm tarball to the Phase-0 snapshot.

### Phase 2 — Boundary enforcement

- Add `packages/blueprint-protocol` with its generator + `git diff --exit-code` CI check.
- Add the §4.1 import/store lint.
- Add `tests/integration/`: boot the real Blueprint daemon, run Membrane's federation against it, assert generation pinning and fail-closed on mismatch.
- Membrane's `providers/cortex.py` switches from "find an external Blueprint install" to the in-tree daemon-first client; subprocess path stays as the documented fallback.

**Gate:** integration test green; import lint fails on a deliberately-planted violation; `blueprint-protocol` regenerates byte-identically.

> After this phase the merge's value is already banked: RecallCircuit v1→v2 becomes one atomic commit (protocol + producer + consumer + integration test).

### Phase 3 — Rename A: Cortex → Blueprint

One commit, whole tree, both former repos at once. This is the payoff of merging first.

Mechanical sweep (`cortex`→`blueprint`, `Cortex`→`Blueprint`, `CORTEX_`→`BLUEPRINT_`) plus the §6 hand-checked surfaces.

**Gate (hard, blocking Phase 5):**

```bash
git grep -in 'cortex' -- . ':!docs/plans/*rename*' ':!RENAME-LEDGER.md' | wc -l   # must be 0
```

Documented exceptions are enumerated in `RENAME-LEDGER.md`, not waved through.

### Phase 4 — Compatibility window for Rename A

- npm: publish `@orthic-labs/blueprint`; `@orthic-labs/cortex` gets a final release that re-exports it and prints a deprecation notice; `npm deprecate` the old name.
- Binaries: ship `blueprint`, `orthic-blueprint`, `blueprint-watch`, `blueprint-mcp`, `blueprint-install`; keep `cortex*` shims that exec the new bin and warn, for one minor version.
- Env vars: read `BLUEPRINT_*` first, fall back to `CORTEX_*` with a warning, for one minor version.
- IPC: new socket/pipe `orthic-blueprint-<hash>`; the client tries the old endpoint once on miss, for one minor version.
- MCP tool: `membrane_cortex` → `membrane_blueprint`, with `membrane_cortex` retained as a deprecated alias in the frozen tool catalog. **This is a public protocol change — it needs an explicit operations-registry version bump, not a silent rename.**

**Gate:** clean-machine install of the new package works; a machine with the old package installed upgrades without manual intervention.

### Phase 5 — Rename B: Crypt → Cortex

Only after Phase 3's zero-occurrence gate is green and merged.

- Crates: `crypt` → `cortex`, `crypt-core` → `cortex-core`, `crypt-store` → `cortex-store`, `crypt-format` → `cortex-format`.
- Binaries: `crypt` → `cortex`, `crypt-service` → `cortex-service`.
- `CRYPT_*` → `CORTEX_*` env vars (read-both window).
- `crypt.db` → `cortex.db` with a startup migration that renames in place and records it in the store-identity record.
- README's existing "legacy name is Crypt, `crypt*` binaries remain the compatibility facade" line already establishes the precedent — extend that facade rather than inventing a new one.

**Gate:** backup/restore drill across the rename preserves logical keys, lineage, and recall equivalence; `crypt*` facade still resolves.

### Phase 6 — Consolidate packaging and docs

- One Hub add-on covering both subsystems; one installer; one doctor entry point that reports both stores.
- Regenerate `docs/architecture.md` / `docs/product.md` on both sides.
- Rename and adopt the two canonical doctrines; insert the §0 sentence into each.
- Redirect the public `Orthic-Labs/Cortex` repo to the monorepo; archive it read-only.

**Gate:** installed-path qualification (Mac + Windows) green against the merged artifacts.

---

## 6. Surfaces that need a human, not `sed`

### Rename A (Cortex → Blueprint)

| Surface | Detail |
|---|---|
| npm package | `@orthic-labs/cortex`, `publishConfig.access: public` — new name + deprecation |
| binaries (5) | `cortex`, `orthic-cortex`, `cortex-watch`, `cortex-mcp`, `cortex-install` |
| env vars (~20) | incl. `CORTEX_DAEMON_ENDPOINT`, `CORTEX_SCIP_INDEX`, `CORTEX_REPO_ROOTS`, `CORTEX_SERVICE_CHILD` |
| IPC endpoint | `\\.\pipe\orthic-cortex-<hash>` / `orthic-cortex-<hash>.sock` |
| schema `$id`s | `https://orthic.labs/schemas/cortex-*-v1.json` — URL is part of the contract; mint `blueprint-*-v1` and keep the old id resolvable |
| on-disk state | `.agent/graph/cortex-install-state.json` (the `.agent/` root is unaffected) |
| installers | Homebrew template, scoop template, winget (3 templates), Inno `Cortex.iss`, Linux archive json, launchers ×4 |
| config files | `examples/cortex.config.example.toml`, `cortex.languages.example.toml`, `cortex.rules.yml`, and the `cortex.json` fixture — **user-facing filenames**, need a read-both window |
| eval baselines | `evals/performance-baselines/cortex-*` — renaming invalidates baseline joins unless the manifest is migrated |
| Membrane wire values | provider id string `"cortex"` in fusion quotas/receipts/fixtures; `DegradationReason::CortexStale/CortexUnavailable/CortexCorrupt` → these appear in **persisted receipts**, so old values must stay readable |
| MCP | `membrane_cortex` tool; `.continue/config.json` server name `cortex` |
| workspace | `/Volumes/D/claude/cortex` path, `catalog.db` entries, Legion/workspace agent rules, Claude Code project dir + memory index |

### Rename B (Crypt → Cortex)

| Surface | Detail |
|---|---|
| crates ×4 + bins ×2 | see Phase 5 |
| env vars (~15) | `CRYPT_DB`, `CRYPT_BIN`, `CRYPT_API_TOKEN*`, `CRYPT_ANCHOR_*`, … |
| store file | `crypt.db` → `cortex.db` + in-place migration |
| peer discovery | Blueprint's `CORTEX_PEER_BIN_CRYPT` becomes `BLUEPRINT_PEER_BIN_CORTEX` — touched by **both** renames; verify explicitly after each |
| docs/examples | `docs/providers/crypt-example.md`, `docs/examples/providers/crypt_example/` |
| dist workspace | `dist/install/workspace/crypt_service.py` |

---

## 7. Rollback

| Phase | Rollback |
|---|---|
| 1 | `git revert` the subtree commit; the Cortex repo is untouched and still authoritative |
| 2 | disable the lint/gates; protocol package is additive |
| 3 | revert the single rename commit (this is why it must be one commit) |
| 4 | compat shims mean rollback is a re-release, not a user migration |
| 5 | revert; store migration is rename-in-place and reversible from the identity record |
| 6 | keep the old Hub add-on published until the new one qualifies |

The Cortex repo stays online read-only until Phase 6 gates are green.

---

## 8. What this plan explicitly does not do

- Does not merge stores, planners, ranking policy, traversal policy, or memory semantics.
- Does not merge processes or runtimes.
- Does not change either canonical architecture.
- Does not make Blueprint depend on Membrane.
- Does not create a shared "contracts" bucket.
