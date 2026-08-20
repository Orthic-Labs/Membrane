# Python SCIP fixture — regeneration

`index.scip.json` is a **hand-authored** portable-SCIP-JSON export for this
fixture. It is parser-compatible with `graph/scip-provider.mjs` and
`providers/compilers/python-scip.mjs`: every document carries a
`relativePath`, every occurrence carries `symbol`, `roles` (portable
role-name array form), and `range` (zero-based `[startLine, startChar,
endLine, endChar]`).

The fixture exercises the adapter contract:

- `pkg/models.py` defines `Item` (class) and `Item.total()` (method).
- `pkg/service.py` imports `Item` from `pkg.models` (relative import) and
  calls `item.total()` — a **cross-document** reference resolved by exact
  SCIP symbol identity, with no name-match heuristic.
- `main.py` imports `pkg.service` and calls `pkg.service.line_total()`.
- `scip-python python blueprint-python-adapter-fixture 1 pkg/service` (the
  module descriptor referenced from `main.py`) intentionally has **no
  definition anywhere in the index**, so the adapter must report it
  UNRESOLVED rather than speculating.

## Manual commands (out-of-band; Blueprint never runs these)

Blueprint only READS a committed index and never invokes an indexer or
interpreter. Regenerating this fixture happens on a contributor machine with
the pinned toolchain from `evals/scip-answer-keys.json` (scip-python 0.6.6,
scip CLI 0.9.0):

```sh
# 1. Index the fixture repo out-of-band (a real scip-python run would need
#    the project's actual package/version identity, e.g. a pyproject.toml).
npx @sourcegraph/scip-python index . --project-name=blueprint-python-fixture

# 2. Export the binary index to the portable JSON shape Blueprint reads.
scip print --json index.scip > index.scip.json
```

This checkout did not have `scip-python`, so no package was installed & this
fixture was hand-authored to the parser contract above. Regeneration would
replace occurrence ranges & symbols with tool output while preserving each
asserted definition/reference relationship.

Commit the resulting `index.scip.json` at the fixture root. The answer-key
harness uses the same export command
(`evals/generate-scip-answer-keys.mjs --scip-cli <cli> --index repo=path`).
Do not commit `index.scip` (binary SCIP format) or any interpreter artifacts.
