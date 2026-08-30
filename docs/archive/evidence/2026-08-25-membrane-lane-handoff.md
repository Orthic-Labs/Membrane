# Membrane lane handoff

> **Superseded:** This point-in-time handoff is retained as historical evidence. Its
> implementation percentages, open-lane table, and “not started” list do not describe
> current `main`; use the subsystem canons and native-runtime migration ledger for
> current status.

**Date:** 2026-08-25
**Base:** `main` @ `de2f95c`
**Status:** architecture complete; implementation ~10-15%, largest items not started

## Landed on `main`

**Architecture/docs — complete.** Four canons adopted (Adapt, Ledger, cross-subsystem
evidence gates, CodeRight integration), each patched for the Hub-owned lifecycle
decisions. Membrane doctrine amended (runtime only inside active Hub; Ledger replaces
Guide; Blueprint independently usable but not independently resident). Blueprint canon
rewritten: resident daemon removed as primary path, `BlueprintStoreLeaseV1` and freshness
contracts added, selector mapping recorded, Legion's three surface obligations accepted,
hosting transparency resolved. Migration plan, agent-rules and subsystem docs reconciled.
`guide.md` -> `ledger.md`. 10-row lifecycle test matrix added.

**Code:**
- `b206b0a` Ledger held-out eval corpus. 152 cases, train/dev/heldout 51/51/50. Verified:
  all 154 doc refs and 141 headings resolve to real files. Paired-bootstrap methodology
  predeclared. No harness consumes it yet.
- `de2f95c` Blueprint store lease + freshness. 24/24 tests. Crash and 6-way race proofs
  use real spawned processes and real SIGKILL. **Not wired into `openStore`'s 43 call
  sites or Hub routing — by our own invariant this is merged, not landed.**

## Open lanes (all pushed, all unverified)

| Branch | Head | State | Next |
|---|---|---|---|
| `lane/ledger-rename` | `49a5572` | Rename complete incl. Hub surface + protocol layer. Compiled clean at `8ce521c`; `49a5572` unverified. | Build check, then ONE scoped test pass (`-p membrane-runtime`, `-p cortex` + touched integration tests). Prove legacy `guide-index.sqlite3` handling. Verify CLI surface. |
| `lane/insights-benchmark` | `7f16f4c` | 49-case corpus + per-detector precision/recall harness. Ground truth separates canonical intent from documented gap reality. | Run the harness. Report measured per-detector numbers. Do NOT tune detectors to make it pass. |
| `lane/blueprint-lease-freshness` | `9f1158c` | Merged to main. | — |
| `lane/ledger-eval-corpus` | `d58390a` | Merged to main. | — |

## Not started

1. **Hub/runtime topology** — the trunk. Queued behind the rename; both touch
   `membrane-runtime`. The rename revealed Guide naming reaches into
   `membrane-protocol` (`hub.rs`, `membrane_status.rs`, `operations.rs`) and the Hub app,
   so this lane is more invasive than "move a module".
2. **Ledger indexing** — AST projections, FTS5, query processing, activation gate.
   Largest item in the plan. Eval corpus is ready for it.
3. **ASCII tokenizer fix** — `doc_spine.rs:186` splits on `!is_ascii_alphanumeric()`, so
   CJK-only queries return zero hits. Live bug, one function, blocked behind rename only.
4. Blueprint: wire lease into call sites + Hub/one-shot routing.
5. Blueprint: expose `path` as protocol method; define `flows`/`architecture` semantics;
   preserve `audit-projection` or define exact paginated mapping. Required before
   Hub-residency cutover (Legion contract).
6. Adapt: semantic sealing, duplicate-group determinism (grouping is model-decided at
   `consolidate_manifest.py:15`), user-act evidence, native port completion, distribution.
7. Token attribution: `apparently_unused_always_on_context` and siblings.
8. Ontology CI check; Adapt tests into root CI; regenerate generated docs.

## Operational notes for whoever picks this up

- **Four external kills happened** (2 session limits, 2 container restarts). Commit and
  push every file edit immediately regardless of build state. Lane branches gate nothing;
  a broken intermediate commit costs nothing, lost work costs everything.
- **Agent worktrees can be in detached HEAD.** `git push -u origin <branch>` then reports
  "Everything up-to-date" while pushing a stale ref. Use `git push origin HEAD:<branch>`.
  This silently stranded the Hub/protocol rename work once.
- **Scope test runs.** `--workspace` builds cost minutes here for no benefit when two
  crates changed.
- **`blueprint/` suite shows ~146 pre-existing failures** in this sandbox: Node v22.22.2
  against a `>=22.22.3` engine floor. Verify against `origin/main` before attributing any
  failure to your change.
- Generated docs (`docs/product.md`, `product-truth.md`, `architecture.md`) are
  regenerated from source, never hand-edited.
