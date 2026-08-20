# Python language support

## Base tier: AST

Python routes through the grammar catalog as an **AST** language
(`precisionTier: "AST"`, `tree-sitter-python.wasm` in `grammars/catalog.json`).
At this tier, structural facts — files, symbols, containment, definitions —
are exact. Cross-reference resolution (imports, calls, attribute lookups)
is name-match heuristic, the same ceiling the lexical tier has for those
edges. The base tier never claims compiler precision.

## Optional tier: compiler-backed (SCIP)

Python additionally supports compiler-backed exact definitions, references,
and types — but **only** when the repository supplies a pre-generated,
committed SCIP JSON index. Blueprint never produces that index itself; it only
reads a portable SCIP JSON export
(`{ documents: [{ relativePath, occurrences: [{ symbol, roles, range }] }] }`)
via `graph/scip-provider.mjs`.

The index is located, in order:

1. `BLUEPRINT_SCIP_INDEX` environment variable (explicit path),
2. `index.scip.json` at the repository root,
3. `.agent/index.scip.json`.

## Regeneration

The index is generated **out-of-band** on the contributor machine with the
scip-python indexer (version **0.6.6**, as recorded in
`evals/scip-answer-keys.json`) and then exported to the portable JSON shape
Blueprint reads. The conversion command is the same one the eval harness uses:

```sh
scip print --json <index.scip> > index.scip.json
```

Commit the resulting JSON at the repository root (`index.scip.json`) or in
`.agent/` (`.agent/index.scip.json`), or point `BLUEPRINT_SCIP_INDEX` at it.
Eval answer keys are regenerated with
`evals/generate-scip-answer-keys.mjs` (`--tasks --out --scip-cli --index repo=path`).

The repository never invokes the scip-python indexer and never executes the
Python interpreter — not during graph generation and not during evals.

## Degradation (typed, honest)

- **No index** — `probeScip` reports `state: "unavailable"` with reason
  `no SCIP index found (set BLUEPRINT_SCIP_INDEX, or place index.scip.json / .agent/index.scip.json at repo root)`;
  the graph degrades to the AST tier.
- **Unreadable JSON** — `state: "unavailable"` with the parse error as the
  reason; same AST fallback.
- **Wrong shape** — a JSON file without a `documents` array is
  `state: "unavailable"` with a typed shape reason.
- **Partial index** — only occurrences literally present in the index are
  joined against nodes already in the generation; nothing is fabricated to
  compensate for gaps.

Every degradation is recorded explicitly per generation in
`generation.augmentation.scip` and never silently invents edges.
