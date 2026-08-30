# The Findings Lane — cheapest-tier deterministic error signal for coding agents

**Status:** design proposal · revision 3 · **phase 0 implemented** in `blueprint/src/lib/findings/`
**Scope:** Blueprint (tier-0 producer) + Membrane planner (routing/policy) + hook host (transport)
**Problem owner:** agents that edit code without an editor, then pay a build to find out

---

## 1. The problem, stated as economics

In VS Code the TypeScript language service holds a live program model and redraws a
squiggle the instant an import stops resolving. You never ran `tsc`. The error found *you*,
in under a second, because a resident process was already holding the answer.

Terminal agents lost the resident process, not the analysis. They edit blind and pay for
discovery with an episodic, cold, whole-workspace check — `pnpm test`, `cargo build`, CI —
often to learn a symbol was renamed in one file and not its caller.

The mistake is framing this as *"cheap static analysis vs expensive compiler."* The real
distinction is:

> **Cold, whole-workspace, episodic checking is expensive. Warm, scoped, resident checking
> is nearly free — and it is the *same compiler*.**

`cargo build` on a large workspace is expensive. `cargo check -p touched-crate` against a
warm target dir, run by a daemon that has already done it before the agent asks, is not.
The lane's job is to make sure the agent always reads the cheapest tier that can answer,
and never triggers a cold expensive one inside the inner loop.

## 2. What already exists (prior art — do not rebuild)

This was researched before designing. Most *detection* is solved per-language. Almost no
one solves *delivery*.

| Category | Tools | What they give | Gap |
|---|---|---|---|
| LSP-over-MCP bridges | **Serena** (~25k★, 30+ languages), `mcp-language-server`, `mcpls`, VS Code diagnostics MCP servers | Real compiler diagnostics, references, rename — over MCP | **Pull only.** The agent must think to ask. It never asks after the edit that broke something |
| Unused/dead export analysis | **Knip** (JS/TS; `ts-prune` and `depcheck` archived 2025), Vulture (Py) | Exactly the "export nobody imports" check | Repo-wide dump, not edit-scoped; no push |
| Import graph | `eslint-plugin-import`, `madge`, `dpdm` | Unresolved specifiers, cycles | File-local or whole-repo; no agent path |
| Resident checkers | **bacon** / **bacon-ls** (Rust), `cargo watch`, `tsc --watch`, rust-analyzer flycheck | Continuous warm `cargo check`/`clippy` with JSON diagnostics | Built for humans in a second terminal; nothing routes them into an agent |
| Repo graphs / indexers | Aider tree-sitter repo map, SCIP/Sourcegraph, stack-graphs, Glean, CodeGraph-style SQLite indexers | Symbols, references, structure | Retrieval-oriented; no confidence, freshness or provenance contract |
| Hook folklore | Community Claude Code `PostToolUse` configs running `prettier → eslint → tsc --noEmit` | Push! Errors reach the agent unbidden | Naive: unscoped, undeduplicated, unbudgeted, cold-runs `tsc` on every edit — the advice in those same guides is *"save heavy tools like tsc for infrequent triggers"* |

A 2026 survey of this landscape concludes the field converged on "precompute structure,
expose as tools, let agents query" — and that **confidence models, freshness guarantees and
provenance contracts remain absent from most tools.**

**So the honest contribution here is not detection.** It is:

1. **Push, not pull.** The finding arrives at the moment of the edit, unrequested.
2. **Edit-scoped delta**, not a repo-wide dump the agent scrolls past.
3. **Cheapest-tier routing** — the ladder in §4, so nothing cold and expensive runs inline.
4. **Budget, dedup and typed degradation**, so the channel stays trusted.
5. **Freshness and provenance**, which Blueprint already has and the rest of the field does not.

That is an orchestration layer over per-language checkers, with a graph as its zeroth tier
and its cross-language glue. It is deliberately *not* a new analyzer.

## 3. What this is not

- Not a linter or formatter — ESLint/Biome/rustfmt keep style.
- Not a replacement for the type checker — it *schedules* the type checker.
- Not a test replacement — it removes wasted runs, not necessary ones.
- Not a policy enforcer — Blueprint reports, Membrane decides, the host enforces.
- Not model-driven — zero LLM calls in the loop. If judgement is needed, it is not a finding.
- Not a new subsystem — no seventh name, no second protocol authority.

## 4. The check ladder (the core of the design)

Every tier answers a strictly larger class of error at a strictly higher cost. The lane
routes each edit to the **lowest tier that can answer it**, and escalates only on demand or
at a gate.

| Tier | Mechanism | Typical cost | Catches |
|---|---|---|---|
| **0** | Blueprint graph delta | 100–400ms, always warm | unresolved import, symbol not exported, dangling reference, removed export with live consumers, ambiguity, cycles, orphan exports |
| **1** | Resident scoped check daemon — `cargo check -p <crate>` via bacon-style watcher; `tsc --noEmit` incremental; language equivalents | already computed; agent read is ~free | full type errors in the touched crate/project |
| **2** | Scoped check, cold | seconds → tens of seconds | same, when the daemon is down or the scope moved |
| **3** | Workspace check / clippy | tens of seconds | everything type-level, repo-wide |
| **4** | Tests | minutes | behaviour |
| **5** | `cargo build` / release build | minutes+ | codegen, linking, artifacts |

**Tier 5 does not belong in a correctness loop at all.** `cargo check` skips codegen and
linking entirely; nothing about "did I break this" needs a binary. Answering your question
directly: yes — `cargo build` can be *excluded* from the inner loop, not merely delayed. It
belongs at exactly two moments: when you need to run the thing, and at release.

The lane's routing rule:

```text
edit → tier 0 always (inline, in the tool result)
     → tier 0 finding at EXACT precision?  stop. that IS the error.
     → tier 1 daemon has a fresh verdict?  attach it. free.
     → agent reaches for tier 3/4/5?       gate: is tier 0 or 1 already red?
                                            if red → block, show the finding
                                            if green → allow, with the narrowed scope
```

Tier 0 is not a weaker substitute for tier 1. It is a **filter** that stops the agent from
ever paying tiers 3–5 to discover something tiers 0–1 already knew.

## 5. Rust economics specifically

You named `cargo build` as the pain. The levers, in order of payoff:

1. **Never `build` to verify — `check`.** Skips codegen and linking. Reported ~1.5x in one
   benchmark (≈55s vs ≈90s), but that ratio understates it badly for link-heavy and
   generic-heavy workspaces where LLVM and the linker dominate.
2. **Make the check resident, not episodic.** `bacon` already runs `cargo check`/`clippy` in
   the background on file change and `bacon-ls` publishes the JSON diagnostics as a language
   server. The agent's marginal cost of "am I broken" becomes a file read. This is the single
   biggest win and it is off-the-shelf.
3. **Scope by dependency graph.** `cargo check -p <crate>` for the touched crate plus its
   reverse dependencies only. Blueprint's impact query already computes that set — this is
   what rule `BP010` is for.
4. **Separate `CARGO_TARGET_DIR` for the check lane.** Otherwise the background checker and
   the agent's own `cargo test` take turns on the same cargo file lock and each blocks the
   other. This is a real, commonly-hit failure — plan for it up front.
5. **Cheap profile knobs for dev only:** Cranelift backend for debug codegen, `mold`/`lld`/`wild`
   linker, `debug = 0` or split debuginfo, `CARGO_INCREMENTAL=1`, `sccache` for shared deps.
   Parallel rustc frontend reportedly gives another 15–25% on larger codebases.

For TypeScript the premise has moved under you: **TypeScript 7 (Go-native `tsc`) went GA on
8 July 2026 at roughly 8–12x**. A full check of the VS Code codebase went from 125.7s to
10.6s. `tsc --noEmit` is now plausibly a *tier-1* operation, not a tier-3 one. Caveat: TS 7.0
has no stable programmatic API yet, so typescript-eslint, ts-morph, ts-jest and the
Vue/Svelte/Astro template checkers cannot run on it until ~7.1.

## 6. Ownership

| Concern | Owner |
|---|---|
| Tier-0 facts, spans, precision tiers, baseline delta | Blueprint |
| Rule registry, stable IDs, suppressions | Blueprint |
| Tier routing, budget, advisory vs blocking | Membrane planner |
| Transport into agent context | Hook host (`mcp/hooks/**`, RightKit dispatch) |
| Reduction of an oversized payload | Push |
| Memory of recurring findings and their fixes | Cortex |
| Proposals to add/retire/retune rules | Adapt |
| Receipts, omissions, typed degradation | Membrane receipt |

Tier 1+ adapters are *external processes the planner schedules*, not Blueprint internals —
which keeps Blueprint doctrine §2.2 #9/#10 (no live compiler/LSP as a required dependency,
no generic LSP runtime inside Blueprint) intact.

## 7. Tier-0 rule catalogue

Derived only from graph facts, each with a **precision floor**, each either *block-eligible*
(provable) or *advisory* (probable).

| ID | Finding | Floor | Class |
|---|---|---|---|
| `BP001` | Imported symbol is not exported by the module it resolves to | EXACT | block |
| `BP002` | Import specifier resolves to no repository file and no package | EXACT | block |
| `BP003` | Re-export names a binding the target module does not export (barrel break) | AST | block |
| `BP004` | Export removed or renamed while exact inbound consumers remain, incl. dangling references to a deleted entity | EXACT | block |
| `BP005` | Same-tier ambiguous export — two resolutions, neither dominates | EXACT | block |
| `BP006` | Arity/signature drift against exact call sites | SCIP/tier-1 | block |
| `BP007` | Import cycle introduced among changed files | AST | advisory |
| `BP008` | Exported symbol with zero inbound edges (orphan export) | AST | advisory |
| `BP009` | Doc claim now contradicts changed source (Phase 2 truth drift) | existing | advisory |
| `BP010` | Impact set for this edit — narrowest crate/test/project scope to check | EXACT | routing |

`BP008` is the rule you named — "an export nobody imports". Advisory only, and for JS/TS
**delegate to Knip rather than reimplementing it**; Knip already handles entrypoints,
`package.json` `exports`/`bin`, and the long tail of legitimate orphans. Emitting it as an
error would be the fastest way to destroy trust in the channel.

`BP010` is not an error at all and is arguably the most valuable row: it is what turns a
whole-workspace tier-3 check into a scoped tier-1 one.

Revision 3 note: `BP003` was originally specified as "reference to an entity that no longer
exists". Building phase 0 showed that finding and `BP004` are the same fact seen from two
sides, and both need a baseline generation. `BP003` now carries the barrel-re-export break —
provable from a single snapshot, and the failure mode agents hit most often when moving code
behind an index file.

## 8. The two invariants that decide whether this works

### 8.1 A finding may only exist at a tier that can prove it

Unresolved is not broken. Lexical extraction that cannot follow a re-export barrel emits
**unsupported**, never an error. Dynamic `import()`, DI containers, macro/codegen output,
monkey-patching and framework magic must each be a *declared unsupported semantic that
suppresses the finding*. Blueprint's locked invariants already require this — ambiguity
fails closed, unsupported is distinct from unresolved, uncertainty is output.

One false squiggle is worse than a hundred missing ones: the agent either learns to skim the
channel or, far worse, "fixes" working code.

### 8.2 A finding must be attributable to the agent's own edit

`18 unresolved imports in this repository` is noise. `your last edit made 1 import
unresolvable` is a fix. Findings are a **delta against a session baseline generation**, using
Blueprint's named-generation semantic diff and the existing rules baseline. Inherited repo
debt stays out of the inline channel and remains retrievable on request.

This is the single thing every hook-folklore `tsc --noEmit` config gets wrong.

## 9. Delivery

### 9.1 `PostToolUse` on `Write | Edit | MultiEdit | apply_patch` — the squiggle

The hook table in `mcp/hooks/membrane-hook-runtime.mjs` already dispatches these. The block
lands in the same tool result, same turn, zero extra tool calls:

```text
membrane:findings — 2 new · generation g-8123 · tier0 EXACT · 118ms · tier1 fresh (bacon, 4s ago) clean
  ERROR BP001 src/pull/admit.mjs:14
        imports { admitCandidate } from "./fuse.mjs" — that module exports
        { fuseCandidates, scoreBatch } only
  WARN  BP004 src/pull/fuse.mjs:88
        removed export `scoreCandidate`; 3 exact consumers remain
        → mcp/server.mjs:41, mcp/adapters.mjs:120, tests/fuse.test.mjs:9
  next  BP010 narrowest verification: pnpm test tests/fuse.test.mjs  (not the full suite)
```

### 9.2 `PreToolUse` on tier-3/4/5 `Bash` — the gate

Where the money is saved. The agent reaches for `cargo build --workspace`; the lane answers
before the process starts:

> Blocked. `BP001` at `engine/crates/cortex/src/store.rs:88` is exact-resolved and will fail
> this build. Also: `cargo check -p cortex` covers your edit — the workspace build is not
> needed here.

Membrane owns this policy. An advisory mode is mandatory — sometimes running the failing
thing *is* the intent.

### 9.3 `Stop` — the completion check

No successful "done" while block-eligible findings are open on files the session touched.

### 9.4 Pull surface

`blueprint findings [--path] [--since-baseline] [--all] [--sarif]` over CLI, service and MCP.
SARIF conversion already exists at `blueprint/src/lib/sarif.mjs` — stable rule IDs,
fingerprints, regions, evidence properties. Reuse it; do not invent a second wire shape.

## 10. Noise budget

The channel dies from volume as surely as from false positives.

- **Cap the injection.** Ten findings inline, ranked errors-first, then edited-files-first,
  then new-before-inherited; `+K more` behind the pull tool. Push owns the reduction, reversibly.
- **Report a fingerprint once.** If the agent saw `BP008` and chose not to act, do not
  re-inject it every turn. Re-surface only on change, plus once at the §9.2 gate.
- **Degrade typed, never silently.** Cold graph, watcher down, daemon dead, provider crash or
  deadline breach emits `findings: unavailable (reason)` — never an empty list reading as clean.

## 11. Latency budget

| Path | p50 | p95 |
|---|---|---|
| Tier 0, single-file edit, warm daemon | 150ms | 400ms |
| Tier 0, multi-file patch, warm | 400ms | 1.2s |
| Tier 1 attach (read of daemon's last verdict) | 20ms | 100ms |
| Cold graph or cold daemon | degrade immediately — never block the edit |

## 12. Build order

0. ~~**Registry + `BP001`/`BP002`/`BP003`**, CLI only.~~ **Done.**
   `blueprint/src/graph/module-surface.mjs` (AST export/import surface),
   `blueprint/src/lib/findings/` (registry, specifier resolution, detection),
   `blueprint findings` CLI with `--json`/`--sarif`/`--baseline`, 30 tests.
   *Result:* `blueprint/` — 619 files, 354 parsed, 1.6s, **0 findings**.
   Membrane whole repo — 1845 files, 597 parsed, 2.2s, **2 findings, both true positives**
   (a real broken specifier in `tests/benchmarks/memory/`, confirmed against Node's own
   resolver). Zero false positives on both. Spec:
   [`blueprint/docs/design/BP001-import-binding-resolution.md`](../../../../blueprint/docs/design/BP001-import-binding-resolution.md).
1. **Baseline delta + overlay incrementality** — findings become edit-scoped.
2. **`PostToolUse` injection**, dumbest possible payload, through the existing hook table.
   *Proof:* the agent fixes it in the same turn without running tests.
3. **`BP010` impact scoping + tier-1 adapter for one language** (Rust via bacon-ls is the
   highest-value first target, since Rust is where the build cost actually hurts).
4. **`PreToolUse` gate** on tier-3/4/5 commands with `block | continue`.
   *Proof:* measured drop in cold `cargo build`/full-suite invocations per session.
5. **`BP004`/`BP005`**; delegate `BP008` to Knip for JS/TS.
6. **Cortex recall of recurring findings + Adapt rule proposals.**

Stop after phase 2 if phase 2 does not change agent behaviour. That is the real gate.

## 13. Success metric

Not findings emitted. Not rules shipped.

- **Pre-expensive-tier catch rate** — of all tier-3/4/5 failures in a session, the share an
  open tier-0/1 finding had already predicted at the moment of the edit.
- **False-positive rate** — findings the agent correctly ignored. Target **< 2%**. This is the
  number that decides whether the channel is trusted at all, and an untrusted channel has no
  value regardless of catch rate.
- **Cold expensive invocations per session** — count of cold tier-3/4/5 runs. Should fall.
- **Median seconds from breaking edit to agent awareness** — today it is the length of a
  build. It should be the length of a hook.

---

## Sources

Prior art and cost claims above were checked against:

- [Serena — LSP-over-MCP toolkit](https://github.com/oraios/serena) · [mcpls MCP↔LSP bridge](https://github.com/bug-ops/mcpls) · [mcp-language-server](https://mcpservers.org/servers/isaacphi/mcp-language-server)
- [Code Intelligence & Code-Graph Indexing for AI Agents (2026 survey)](https://anthonywest.co.uk/research/code-intelligence-indexing-2026-openai)
- [Knip — unused files, exports, dependencies](https://knip.dev/typescript/unused-exports) · [dead-code tool comparison, 2026](https://www.pistack.xyz/posts/2026-06-19-dead-code-detection-tools-knip-ts-prune-vulture-unimported/)
- [bacon — background Rust code checker](https://dystroy.org/bacon/analyzers/) · [bacon-ls — bacon diagnostics as a language server](https://github.com/crisidev/bacon-ls)
- [cargo check — The Cargo Book](https://doc.rust-lang.org/cargo/commands/cargo-check.html) · [Optimizing Build Performance](https://doc.rust-lang.org/cargo/guide/build-performance.html) · [Tips For Faster Rust Compile Times, corrode](https://corrode.dev/blog/tips-for-faster-rust-compile-times/)
- [TypeScript 7.0 released with Go-native compiler, InfoQ](https://www.infoq.com/news/2026/08/typescript-7-released/) · [TypeScript 7.0 RC, Visual Studio Magazine](https://visualstudiomagazine.com/articles/2026/06/22/typescript-7-0-rc-moves-microsofts-go-rewrite-into-the-mainline-compiler.aspx)
- [VS Code Language Server Extension Guide](https://code.visualstudio.com/api/language-extensions/language-server-extension-guide) · [typescriptServiceClient.ts](https://github.com/microsoft/vscode/blob/main/extensions/typescript-language-features/src/typescriptServiceClient.ts)
- [Claude Code lint-on-edit PostToolUse hook pattern](https://www.claudedirectory.org/hooks/lint-on-edit)
