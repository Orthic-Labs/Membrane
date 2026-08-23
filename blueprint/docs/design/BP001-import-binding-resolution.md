# BP001 — import binding not exported

**Status:** implemented (phase 0) · `src/lib/findings/`
**Rule class:** block-eligible · **Precision floor:** AST · **Severity:** error
**Companions:** BP002 (module not found), BP003 (re-export binding not exported)

---

## 1. What the rule claims

> Module `A` contains `import { x } from "S"`. `S` resolves to repository file
> `B`. `B` does not export `x`. Therefore `A` is broken, provably, without
> running a compiler, a test or a build.

The claim is deliberately narrow. BP001 says nothing about types, nullability,
arity or runtime behaviour. It says one thing that a type checker would also say,
about 3–4 orders of magnitude more cheaply, and cross-language by construction.

## 2. Why the existing resolution code cannot answer it

Two resolvers already exist in Blueprint. Neither is name-aware.

### 2.1 `src/providers/modules/javascript.mjs`

`resolveModuleSpecifier({ specifier, fromFile, isTypeScript })` resolves a
specifier against the **filesystem**: relative paths through an extension ladder
(`.ts .tsx .mts .cts .js .jsx .mjs .cjs .json`), then `index.*`, then bare
specifiers through the nearest `node_modules` and its `package.json`
`module`/`main`. It returns `{ resolved, reason }` where `reason` is one of
`missing_input | absolute | relative | missing | no_node_modules | package | bare`.

It answers **"which file"**. It never opens the target, so it cannot answer
**"which names"**.

### 2.2 `src/graph/language-extractors.mjs`

`extractJavaScriptImports(file, files)` resolves the same specifiers against the
**scanned file set** rather than the filesystem, and returns *file paths only* —
the names in the import clause are discarded before the graph ever sees them.
`extractUnresolvedImportSpecifiers(file, files)` is its honest companion,
reporting relative specifiers that matched no file. That companion is already
BP002 in everything but name.

Both are regex-driven. A regex cannot distinguish `export default function f`
from `export function f` (identical but for one token), cannot see through
`export *`, and cannot tell an import statement from the same text inside a
comment or a template literal.

**Conclusion:** BP001 needs a new, name-level, AST-tier extraction on both sides
of the edge. That is `src/graph/module-surface.mjs`.

## 3. The soundness rule

> A consumer may claim "name `N` is not exported by module `B`" **only when
> `B`'s export surface is CLOSED** — every export-shaped construct in `B` was
> recognised, and nothing in `B` can add names the extractor cannot see.

An open surface yields **no finding**, only a named omission. This is not
conservatism for its own sake: a missing squiggle costs one build; a false
squiggle teaches an agent to distrust the channel or, worse, to "fix" working
code. The asymmetry is total, so the policy is total.

The rule inherits Blueprint's locked invariants directly — *ambiguity fails
closed*, *unsupported is distinct from unresolved*, *uncertainty is output*.

### 3.1 Constructs that OPEN a surface

`OPEN_SURFACE_REASONS` in `src/graph/module-surface.mjs` is a closed vocabulary:

| Reason | Trigger | Why it opens |
|---|---|---|
| `parse_error` | any `ERROR`/missing node in the tree | a file the grammar could not read can hide anything |
| `export_assignment` | `export = thing` (TS) | the surface is one anonymous value, not a name set |
| `commonjs_exports` | `module.exports = …`, `exports.x = …`, `exports[k] = …` | names are assigned at runtime |
| `ambient_module` | `declare module "x" { … }` | augments another module's surface |
| `nested_export` | `export` inside a namespace/module block | not a module-level export |
| `destructured_export` | `export const { a } = …`, `export const [a] = …` | the pattern is not a plain identifier |
| `unfollowable_star_reexport` | `export * from` a target that cannot be enumerated | the surface includes names from elsewhere |

Anything the extractor does not recognise leaves the surface open by default,
because unrecognised constructs fall through to `destructured_export` or
`parse_error` rather than being silently dropped.

### 3.2 `export *` semantics

Repository-local star chains **are** followed — transitively, cycle-detected,
depth-bounded at 8. This matters because barrel files are where agents break
things most often, and refusing to follow them would make the rule useless on
exactly the codebases that need it.

Three star cases open the surface instead:

- the target is a bare specifier (`export * from "some-package"`) — outside repository scope;
- the target does not resolve;
- the target's own surface is open.

One ESM subtlety is implemented and tested: **`export *` does not re-export
`default`.** A barrel that stars an implementation module does not give
consumers that module's default export, and BP001 reports it when they try.

### 3.3 Import forms and what each requests

| Form | Requests |
|---|---|
| `import { x } from "S"` | `x` |
| `import { x as y } from "S"` | `x` — the *exported* name, never the local alias |
| `import type { T } from "S"` / `import { type T } from "S"` | `T` — a type export is an export |
| `import d from "S"` | `default` |
| `import * as ns from "S"` | nothing — a namespace import cannot be name-wrong |
| `import "S"` | nothing — side effect only |
| `export { x } from "S"` | `x` (as **BP003**, not BP001 — see §5) |
| `import()`, `require()` | nothing — dynamic, out of scope for phase 0 |

## 4. Resolution contract

`src/lib/findings/specifier.mjs` resolves against the **scanned file set**, not
the filesystem, so a findings verdict can never disagree with the graph's own
file-level import edges. Candidate order:

1. the exact path as written (keeps `.json`, `.css` intact);
2. the TypeScript `.js → source` rewrite plus extension completion,
   in order `ts tsx mts cts js jsx mjs cjs json vue astro`;
3. `…/index.{ts,tsx,mts,cts,js,jsx,mjs,cjs}`.

First match wins — deterministic, and `alternatives` counts the rest as evidence.

**The scanned-set guard.** If a specifier resolves to nothing in the set but a
candidate path exists on disk (an ignored prefix, a submodule, a generated
tree), the result is the omission `outside_scanned_set`, never BP002. Coverage
gaps must not masquerade as defects.

## 5. Rule boundaries

| Situation | Verdict |
|---|---|
| Named import of a name the target does not export | **BP001** |
| Relative specifier resolving to no repository file | **BP002** |
| `export { x } from "./m"` where `m` lacks `x` | **BP003** — damage is to *this barrel's consumers*, not to this file |
| Bare/package specifier | omission `package_specifier` |
| Target is `.json`, `.vue`, `.astro`, or any non-JS/TS file | omission `unsupported_language` |
| Target's surface is open | omission `open_export_surface` |

BP003 exists separately because the two findings need different agent responses:
BP001 is "your file is broken"; BP003 is "you just broke everyone downstream".

## 6. Finding identity

Fingerprint = `sha256(ruleId + path + name + specifier)[0..16]`.

Line numbers are deliberately **excluded**. A finding must keep its identity when
an unrelated edit shifts it down two lines, or a baseline becomes noise and the
agent gets re-shown findings it already declined to act on. Tested in
`tests/findings-detect.test.mjs` ("a fingerprint survives an unrelated line shift").

## 7. Output

Findings carry `ruleId, ruleName, severity, class, confidenceTier,
precisionTier, path, startLine, endLine, message, name, specifier, evidencePath,
generationId, fingerprint, remediation` — the field set
`src/lib/sarif.mjs#toSarif` already consumes, so `blueprint findings --sarif`
needs no second wire shape.

Every run also returns `coverage { filesScanned, filesParsed, surfacesClosed,
omissionCount }`. "0 findings" and "nothing could be checked" must never be the
same answer.

## 8. Measured behaviour

Phase-0 gate: **zero false positives on real repositories.**

| Repository | Files | Parsed | Closed surfaces | Duration | Findings |
|---|---|---|---|---|---|
| `blueprint/` | 619 | 354 | 354 | 1.6s | 0 |
| Membrane (whole) | 1845 | 597 | 590 | 2.2s | 2 (both true) |

The two findings on Membrane are genuine pre-existing defects — a specifier that
walks up two directories where it needs three:

```text
BP002 tests/benchmarks/memory/runner.mjs:3
      "../../scripts/qualification/verify-memory-benchmark.mjs" resolves to no file
BP002 tests/benchmarks/memory/memory-benchmark.test.mjs:4
      same specifier, same miss
```

Independently confirmed against Node's own resolver:
`ERR_MODULE_NOT_FOUND: Cannot find module '/…/Membrane/tests/scripts/qualification/verify-memory-benchmark.mjs'`.

Whole-repository scan cost is ~2s cold with no warm daemon and no incremental
delta. The lane's edit-scoped path re-checks one file and its dependents, so the
sub-400ms target in the Findings Lane design is met with headroom.

## 9. Test coverage

`tests/findings-detect.test.mjs` — 30 tests. The ones that matter:

- a correct repository produces **zero** findings;
- aliased imports judge the exported name, never the local alias;
- `export *` chains are followed transitively; cycles are reported, not crashed on;
- `export *` does not carry `default`;
- CommonJS, `export =`, destructured exports, parse failures and unsupported
  languages all suppress rather than report;
- a commented-out import and an import inside a template literal are not findings;
- fingerprints survive line shifts; output order is deterministic.

## 10. Not in phase 0

- `require()` / dynamic `import()` name tracking.
- Non-JS/TS languages. Python and Rust have tier-1 grammars loaded already; the
  surface extractor is the only per-language part, so each is additive.
- `tsconfig` `paths` aliases and `package.json` `imports` (`#internal`) — both
  currently fall out as `package_specifier` omissions.
- BP004 (export removed with live consumers) and the baseline-delta scoping that
  turns a repository scan into an edit-scoped one. That is phase 1.
