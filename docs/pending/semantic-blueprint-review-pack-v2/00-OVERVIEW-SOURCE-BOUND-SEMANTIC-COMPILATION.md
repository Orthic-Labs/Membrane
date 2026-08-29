# Source-Bound Semantic Compilation

> **Status:** Fable review draft — not canonical.
> **Date:** 2026-08-29
> Existing canonical subsystem documents win on conflict. Re-derive implementation state before execution.

## Decision

Keep the six-subsystem architecture.

Add a governed path from Ledger source evidence to Cortex durable semantic knowledge:

```text
document bytes
    |
    v
  Ledger
doc -> section -> typed block
    |
    +--> Pull exact retrieval
    |
    +--> semantic candidate extraction
              |
              v
        Cortex admission
              |
              v
      durable derived knowledge
              |
              +--> exact Ledger evidence refs
```

Do not make the generated semantic layer source authority.

## Structural retrieval

Markdown is decomposed using **existing source structure**, not fixed token windows:

```text
document
  -> heading / section
      -> paragraph
      -> list / item
      -> table
      -> code block
      -> quote / other typed AST block
```

Retrieval starts at the smallest structurally coherent strong hit. Pull may request bounded parent/sibling/link expansion if that hit is insufficient.

Example:

```text
3,000-word file
  -> 520-word section
      -> 280-word subsection
          -> 95-word paragraph
```

A query should receive the 95- or 280-word source-bound region when sufficient, not the whole file.

## Semantic candidate

A compiler consumes Ledger-resolved nodes and emits closed-schema proposals such as:

- fact / observation;
- decision;
- constraint / invariant;
- procedure;
- temporal state;
- entity assertion;
- explicit relationship.

Every proposal carries:

```text
LedgerEvidenceRefV1
  doc_id
  node_id
  source_ref
  source_revision
  span_hash
  ledger_generation
  source_range?
```

The compiler creates neither source identity nor authority.

## Ownership

- **Ledger:** document registration, structural projection, retrieval, exact resolution, change/invalidation evidence.
- **Cortex:** semantic compiler admission producer, duplicate/conflict/supersession/lifecycle, durable derived knowledge.
- **Pull:** runtime fusion and sufficiency; chooses between semantic knowledge and exact Ledger evidence.
- **Adapt:** remains behavioral learning; may learn from pipeline failures but does not compile generic documents.
- **Blueprint:** repository/code truth remains independent.
- **Push:** representation/reduction after selection; no semantic authority.

## Forbidden paths

```text
LLM claim -> Ledger truth
Ledger -> unchecked Cortex write
document prose -> Taste authority
semantic record -> repository truth
```

## Qualification question

Compare:

A. Cortex only
B. Ledger structural retrieval only
C. Cortex semantic record + optional Ledger verification
D. semantic compilation + Cortex + Ledger verification-on-demand

Measure task success, context tokens, latency, stale-evidence detection, unsupported-claim rate, source-resolution rate, and manual correction/search.

Promote only if D materially improves correctness or context economics.
