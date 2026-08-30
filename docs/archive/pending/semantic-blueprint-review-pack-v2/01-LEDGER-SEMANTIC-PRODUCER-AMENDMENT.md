# Ledger Amendment — Structural Evidence Producer

> **Status:** Fable review draft — not canonical.
> **Date:** 2026-08-29
> Existing canonical subsystem documents win on conflict. Re-derive implementation state before execution.

## Authority preserved

Ledger remains the rebuildable document registry/index/navigation/resolution subsystem. It does not own durable learned knowledge and never opens the Cortex store.

## New public seam

Expose changed source-bound structural nodes to a governed consumer:

```text
SemanticSourceDeltaV1
  doc_id
  source_ref
  source_revision
  content_hash
  ledger_generation
  parser_version
  projection_schema_version
  changed_nodes[]
  removed_node_ids[]

SemanticSourceNodeV1
  node_id
  parent_id?
  node_kind
  heading_path
  source_range
  span_hash
  text
  link_targets[]
```

This is a source/projection observation, not semantic truth.

## Structural granularity

Primary decomposition follows the Markdown AST/hierarchy:

1. document;
2. section/subsection;
3. typed blocks.

A large section can expose paragraph/list/table/code nodes independently. A small coherent section need not be subdivided merely to hit a token target.

Target:

> **smallest structurally coherent sufficient region**

Pull owns sufficiency/budget and bounded expansion.

## Ledger may

- enumerate changed/removed structural nodes;
- resolve node text under an active grant;
- verify source revision/hash;
- return `current`, `relocated`, `stale`, `missing`, or `denied`;
- expose enough identity for downstream revalidation.

## Ledger may not

- admit durable knowledge;
- write Cortex;
- let a model invent source identity;
- store LLM-generated semantic relations as Ledger truth.

## Evidence contract

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

Structural node identity should be canonical; human-readable slug/ordinal anchors remain aliases.

## Downstream invalidation

A changed source does not cause Ledger to mutate Cortex. It only makes prior evidence re-resolvable:

```text
resolve_evidence(ref)
 -> current
 -> relocated(new_ref)
 -> stale
 -> missing
 -> denied
```

The consumer decides lifecycle consequences.

## Qualification additions

Test:

- block vs section retrieval;
- bounded parent/sibling expansion;
- section move/rename/edit;
- evidence re-resolution;
- whole-file context avoided when a smaller node suffices;
- no semantic-worker output entering Ledger authority tables.

Measure resolved bytes/tokens, expansion hops, stale/relocation rate, and task success.
