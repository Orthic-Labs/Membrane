# The Findings Lane — deterministic pre-test error signal for coding agents

**Status:** design proposal · not yet adopted doctrine
**Scope:** Blueprint (producer) + Membrane planner (publisher) + hook host (transport)
**Problem owner:** agents that edit code without an editor

---

## 1. What this is

In VS Code, the TypeScript language service holds a live program model and redraws a
squiggle the instant an import stops resolving. You never ran `tsc`. You never ran the
tests. The error found *you*, in the file, in under a second.

Agents lost that. Claude Code, Codex and every other terminal agent edit blind and then
pay for discovery: a 90-second `pnpm test`, a 4-minute build, a CI round trip — often to
learn that a symbol was renamed in one file and not the other.

The Findings Lane restores the squiggle for agents. The claim in one line:

> **The graph is the program model. The tool hook is the editor. The packet is the squiggle.**

Blueprint already holds a generation-bound, dirty-worktree-aware, exact-first resolution
graph of the repository. If a symbol stops resolving, that fact already exists in the
store the moment the file is re-extracted. Nothing needs to be invented to *know* it. What
is missing is a rule catalogue, an edit-scoped delta, and a delivery path into the agent's
context at the moment of the edit.

## 2. What it is not

- Not a linter or formatter — style belongs to ESLint/Biome/rustfmt.
- Not a type checker — `tsc` still owns type-level errors (see §8).
- Not a test replacement — it shrinks the number of *wasted* test runs, not the necessary ones.
- Not a policy enforcer — Blueprint reports, Membrane decides, the host enforces.
- Not model-driven — zero LLM calls inside the loop. If judgement is required, it is not a finding.
- Not a new subsystem — no seventh name, no second protocol authority, no shared-contract bucket.

## 3. Ownership (final shape)

| Concern | Owner |
|---|---|
| Finding facts, spans, precision tiers, baseline delta | Blueprint |
| Rule registry, stable rule IDs, suppressions | Blueprint |
| Whether to inject, budget, advisory vs blocking | Membrane planner |
| Transport into agent context | Hook host (`mcp/hooks/**`, RightKit dispatch) |
| Reduction of an oversized finding payload | Push |
| Memory of recurring findings and their fixes | Cortex |
| Proposals to add/retire/retune rules from outcomes | Adapt |
| Receipts, omissions, typed degradation | Membrane receipt |

This is an extension of existing ownership, not a redraw. Blueprint's canonical doctrine
already lists diagnostics (§3.1), typed contradictions and ambiguity (IN #15), impact and
liveness and recommended test selection (IN #16), and doctor findings over vanity counts
(IN #27). The lane is the consumer path those capabilities never got.

## 4. Layer stack

```text
 4  Delivery      PostToolUse squiggle · PreToolUse gate · Stop check · pull tool
 3  Policy        Membrane planner: severity → advisory | block, budget, dedup
 2  Findings      rule registry over graph facts, baseline-delta scoped, SARIF-shaped
 1  Model         Blueprint generation + dirty overlay + exact-first resolution
```

Layer 1 exists. Layer 2 is the new work. Layer 3 is planner policy. Layer 4 reuses the
hook table already wired for `Write | Edit | MultiEdit | apply_patch`.

## 5. The rule catalogue

Every rule is derived **only** from graph facts, carries a **precision floor**, and is
either *block-eligible* (provable) or *advisory* (probable).

| ID | Finding | Floor | Class |
|---|---|---|---|
| `BP001` | Imported symbol is not exported by the module it resolves to | EXACT | block |
| `BP002` | Import specifier resolves to no repository file and no package | EXACT | block |
| `BP003` | Reference/call to an entity that no longer exists in this generation | EXACT | block |
| `BP004` | Export removed while exact inbound consumers remain (half-done rename) | EXACT | block |
| `BP005` | Same-tier ambiguous export — two resolutions, neither dominates | EXACT | block |
| `BP006` | Arity/signature drift against exact call sites | SCIP/compiler | block |
| `BP007` | Import cycle introduced among changed files | AST | advisory |
| `BP008` | Exported symbol with zero inbound edges (orphan export) | AST | advisory |
| `BP009` | Documentation claim now contradicts changed source (Phase 2 truth drift) | existing | advisory |
| `BP010` | Changed entity has exact-resolved tests — narrowest check to run | EXACT | next-action |

`BP008` is the rule you asked for by name — "an export nobody imports". It is deliberately
advisory-only and suppression-heavy: package `exports`/`bin` entrypoints, public API
surfaces, test-only consumers, and dynamic imports all make an orphan legitimate. Emitting
it as an error would be the fastest way to destroy trust in the whole channel.

`BP010` is not an error at all. It is the highest-leverage item in the table: when the
agent *does* need to verify, the lane names the three files to check instead of the whole
suite.

## 6. The two rules that decide whether this works

Everything else is engineering. These two are the design.

### 6.1 A finding may only exist at a tier that can prove it

Unresolved is not broken. A lexical-tier extraction that cannot see a re-export barrel
must emit **unsupported**, not an error. Blueprint's locked invariants already say this —
*ambiguity fails closed*, *unsupported is distinct from unresolved*, *uncertainty is
output* — and the lane inherits them without exception.

The practical consequence: dynamic `import()`, barrel re-exports the provider cannot
follow, DI containers, framework magic, monkey-patching and codegen must each be a
declared **unsupported semantic** that *suppresses* the finding, never a silent
UNRESOLVED that becomes an error.

One wrong squiggle is worse than a hundred missing ones. An agent that gets a false error
either learns to skim the channel, or — much worse — "fixes" working code.

### 6.2 A finding must be attributable to the agent's own edit

`18 unresolved imports in this repository` is noise the agent will scroll past. `your last
edit made 1 import unresolvable` is a fix.

So findings are computed as a **delta against a session baseline generation** — the clean
generation at session start (or the last green commit), using Blueprint's named-generation
semantic diff (IN #17) and the existing rules baseline. Pre-existing findings are held out
of the inline channel and remain retrievable by explicit query. Inherited repo debt is not
this turn's problem.

## 7. Delivery — the actual squiggle

Three touch points, in descending order of value.

### 7.1 PostToolUse on `Write | Edit | MultiEdit | apply_patch` — the squiggle

The hook table already dispatches these events. The finding block lands in the same tool
result, in the same turn, at zero extra tool calls:

```text
membrane:findings — 2 new · generation g-8123 · tier EXACT · 118ms
  ERROR BP001 src/pull/admit.mjs:14
        imports { admitCandidate } from "./fuse.mjs" — that module exports
        { fuseCandidates, scoreBatch } only
  WARN  BP004 src/pull/fuse.mjs:88
        removed export `scoreCandidate`; 3 exact consumers remain
        → mcp/server.mjs:41, mcp/adapters.mjs:120, tests/fuse.test.mjs:9
```

This is the whole product. Everything else is supporting structure.

### 7.2 PreToolUse on test/build `Bash` — the gate

Where the money is actually saved. When a block-eligible finding is open and the agent
reaches for `pnpm test`, the admission decision returns `block` with the finding list:

> 1 exact unresolved import will fail this run. Fix `src/pull/admit.mjs:14` first.

Membrane owns this policy, not Blueprint, and an advisory mode is mandatory — sometimes
running the failing test *is* the intent.

### 7.3 Stop / pre-delivery — the completion check

No successful "done" while block-eligible findings are open on files the session touched.
This slots into the existing `Stop` hook and the completion-gate discipline.

### 7.4 Pull surface

`blueprint findings [--path <p>] [--since-baseline] [--all] [--sarif]`, exposed over CLI,
service and MCP. SARIF conversion already exists in `blueprint/src/lib/sarif.mjs` — stable
rule IDs, fingerprints, regions, evidence properties. Reuse it; do not invent a second
finding wire shape.

## 8. On LSPs specifically

You asked whether to use LSPs. **Wrong substrate, right upgrade tier.**

Wrong substrate, because a language server answers *"what is in this file"* for *one
language* from a process with a multi-second-to-multi-minute cold start, non-deterministic
partial-index results, heavy memory, and no evidence provenance. The lane needs *"what did
this edit break across the repository"*, cross-language, deterministically, in under
200ms, with a citation. Blueprint doctrine also puts a generic LSP runtime explicitly OUT
(§2.2 #9, #10) — for exactly these reasons, not arbitrarily.

Right upgrade tier, because compiler/LSP/SCIP providers are already sanctioned as **opt-in**
(D28) and they raise precision where a repository supplies them:

```text
LEXICAL  <  AST (tree-sitter)  <  SCIP  <  compiler / LSP (opt-in)
```

The binding rule: **no finding's existence may depend on a live process.** A live process
may only *promote* a finding's tier (advisory → block-eligible, e.g. `BP006` arity drift)
or *retire* a finding the cheaper tier could not disprove. The loop must run, and run
fast, with nothing but the graph.

And the honest limit: the graph reproduces the *resolution and reference* half of what
your TypeScript squiggle showed you, not the *type* half. Passing a `string` where a
`number` is expected is still `tsc`'s job. Two things make that acceptable — resolution
breakage is the majority of agent-caused build failures, and `BP010` turns the remaining
type check into `tsc` over three files rather than a full suite.

## 9. Noise budget

The channel dies from volume as surely as from false positives.

- **Cap the injection.** Ten findings inline, ranked errors-first, then edited-files-first,
  then new-before-inherited; `+K more` behind the pull tool. Push owns the reduction and it
  stays reversible.
- **Report a fingerprint once.** If the agent saw `BP008` and chose not to act, do not
  re-inject it every turn. Re-surface only on change, plus once at the `7.2` gate.
- **Degrade typed, never silently.** Cold graph, watcher down, provider crash or deadline
  breach emits `findings: unavailable (reason)` — never an empty list that reads as clean.

## 10. Latency budget

The lane is worthless if it is slower than the thing it replaces.

| Path | p50 | p95 |
|---|---|---|
| Single-file edit, warm resident daemon | 150ms | 400ms |
| Multi-file patch, warm | 400ms | 1.2s |
| Cold graph | degrade immediately — never block the edit |

The resident `blueprint-watch` daemon and the freshness barrier already exist to hold this.
The hook takes a hard deadline and reports timeout as degradation.

## 11. Build order

Each phase ships and proves something on its own.

0. **Registry + BP001/BP002/BP003**, CLI only, over existing exact resolution.
   *Proof:* a fixture repo with one deliberately broken import yields exactly one finding;
   a clean fixture yields zero. Zero false positives on the Membrane repo itself.
1. **Baseline delta + overlay incrementality** — findings become edit-scoped, not repo-scoped.
2. **PostToolUse injection**, dumbest possible payload, through the existing hook table.
   *Proof:* the agent fixes the finding in the same turn, without running tests.
3. **PreToolUse test/build gate** + admission `block | continue`.
   *Proof:* measured drop in failed test invocations per session.
4. **BP004/BP005 impact findings + BP010 test selection.**
5. **Opt-in SCIP/compiler tier promotion**, `BP006`.
6. **Cortex recall of recurring findings + Adapt rule proposals.**

Stop after phase 2 if phase 2 does not change agent behaviour. That is the real gate.

## 12. Success metric

Not findings emitted. Not rules shipped. Two numbers:

- **Pre-test catch rate** — of all build/test failures in a session, the share that an open
  finding had already predicted at the moment the edit was made.
- **False-positive rate** — findings the agent correctly ignored. Target **< 2%**, because
  this is the number that decides whether the channel is trusted at all, and a channel that
  is not trusted has no value regardless of its catch rate.

A third, softer: median seconds from breaking edit to agent awareness. Today it is the
length of a test run. It should be the length of a hook.
