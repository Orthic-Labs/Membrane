# Relationships, comment claims, and chunks

## Relationship vocabulary

Cortex emits typed edges from the shared vocabulary:

`INHERITS`, `IMPLEMENTS`, `MIXES_IN`, `USES`, `TESTS`, `COVERS`, `GENERATES`,
plus the existing `IMPORTS`, `CALLS`, `CONTAINS`, `REFERENCES`, `DEFINES`,
`CONFIGURES`, `READS`, `WRITES`, `PRODUCES`, `CONSUMES`, `DEPLOYS`,
`HANDLES`, `ROUTES_TO`, and provenance kinds.

Confidence is always derived from the resolution path (compiler/SCIP →
EXACT_RESOLUTION, same-file → SAME_FILE_LEXICAL, cross-file heuristic →
CROSS_FILE_HEURISTIC, else UNRESOLVED) — never hardcoded per edge kind.

## Comment claims

`NOTE` / `WHY` / `HACK` / `TODO` / deprecation / ownership comments become
relational claims bound to the smallest enclosing symbol, with span, hash,
and `lifecycle: data_only`. Repository comments are data — they can never
become instructions.

## Syntax-aware chunks

Chunks trim only at AST/token boundaries using the enclosing symbol's exact
span (falling back to the parent span). Any dropped bytes produce a
mandatory truncation receipt (`syntax_chunk_byte_cap`) so downstream can
distinguish "small" from "was truncated".
