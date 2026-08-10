# Language support

Cortex routes supported languages through the grammar catalog
(`grammars/catalog.json`): every entry carries an extension list, a fact
profile, and a base precision tier. Base precision is **AST** for parsed
languages (tree-sitter WASM grammars) and **LEXICAL** for unknown
extensions. Tier placement (`release/compatibility.template.json` `languageDepth`)
reflects grammar and fixture depth, not compiler backing.

Compiler-backed exact definitions, references, and types are available only
where the repository supplies a pre-generated, committed SCIP JSON index.
Cortex never vendors, installs, or invokes an indexer or interpreter — it
only reads the index (`graph/scip-provider.mjs`). When no index is present,
unreadable, or malformed, the graph degrades to the base AST tier with a
typed, explicit reason instead of inventing edges.

## Per-language docs

- [Python](python.md) — AST base tier plus optional scip-python compiler
  backing from a committed SCIP JSON index.

## Index discovery

For languages with optional SCIP backing, the provider looks for, in order:

1. `CORTEX_SCIP_INDEX` environment variable (explicit path),
2. `index.scip.json` at the repository root,
3. `.agent/index.scip.json`.

## Regeneration

Indexes are produced out-of-band on the contributor machine and committed.
The repository never runs the indexer or the interpreter. See each language
page for the exact workflow and pinned indexer versions.

## Degradation

- Absent index → `state: "unavailable"`, reason
  `no SCIP index found (set CORTEX_SCIP_INDEX, or place index.scip.json / .agent/index.scip.json at repo root)`.
- Unreadable JSON → `state: "unavailable"`, parse error as the reason.
- Missing `documents` array → `state: "unavailable"`, typed shape reason.
- Partial index → only occurrences literally present are joined against
  existing generation nodes; nothing is fabricated.

The result is recorded per generation in `generation.augmentation.scip`.
