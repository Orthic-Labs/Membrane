# context-outcome-baseline (BM11 minimum corpus)

Fixture corpus for BM11 ("Outcome qualification"): compares current Membrane
context delivery against a fixed baseline on an identical fixture/task/source
snapshot, using independently authored expected evidence rather than
model-graded or self-reported outcomes.

This is the **minimum baseline** required to ship with the first integrated
producer-consumer tranche (BM09/BM10 delivery + Cortex/Pull evidence). It is
not the final statistical corpus — expansion to full category coverage and
statistical significance is sequenced later, before Windows final
qualification, per the BM11 acceptance row, and is not a prerequisite to this
first functional checkpoint.

## Files

- `tasks.jsonl` — one task per line. Each task is a fixed `(taskId, category,
  prompt, repoSnapshot, sourceSnapshot)` tuple. `repoSnapshot`/`sourceSnapshot`
  are content-addressed references (git commit + path set), never inline
  repository content, so the corpus stays independent of any one working
  tree.
- `expected-evidence.jsonl` — one independently authored expected-evidence
  record per `taskId`, keyed by `taskId`. Each record states the minimum
  set of evidence IDs / sources that must be present in a correct answer and
  the false-confidence traps (a plausible but wrong answer a ungrounded model
  would produce) the task is designed to catch. Expected evidence is authored
  **before** and independently of running Membrane against the task — it is
  not derived from observing Membrane's own output.

## Schema (per `tasks.jsonl` line)

```json
{
  "taskId": "string, stable, unique",
  "category": "one of: exact_implementation | ambiguity | multifile | config | stale_docs | dirty_state | fanout | dynamic_surface | tight_budget | contradictory_memory_update | standing_preference",
  "prompt": "the task prompt exactly as given to the client host",
  "repoSnapshot": { "commit": "40-hex git sha", "paths": ["path", "..."] },
  "sourceSnapshot": { "kind": "blueprint|cortex|ledger|pull", "generation": "string or null" },
  "hostConstraints": { "budgetTokens": 0, "client": "string" }
}
```

## Schema (per `expected-evidence.jsonl` line)

```json
{
  "taskId": "must match a tasks.jsonl entry",
  "requiredEvidenceIds": ["stable evidence/source id", "..."],
  "falseConfidenceTraps": ["a specific wrong-but-plausible claim this task is designed to expose", "..."],
  "correctnessCriteria": "independently authored description of what a correct, evidence-grounded answer must state",
  "primaryMetric": "correctness | evidence_recall | false_confident_decision",
  "secondaryMetrics": ["calls", "context_tokens", "cost", "rss", "latency_ms"]
}
```

## What this baseline proves and does not prove

- Proves: for each listed `taskId`, current Membrane vs. the named baseline
  can be compared on the same snapshot against the same independently
  authored expected evidence, with host version, adapter, generation,
  capability, actual injection point, and outcome strength captured
  per BM09/BM11 (no inferred model use, no unsupported causality).
- Does not prove: statistical significance across a broad task population.
  That expansion (more categories, more tasks per category, a
  train/dev/heldout split) is explicitly deferred by the BM11 acceptance row
  and tracked separately; it is a precondition of Windows *final*
  qualification, not of this checkpoint.
- Excludes: CodeRight desktop/iOS and macOS execution. Every fixture here is
  scoped to supported Windows hosts only.

## Corpus fixture path (negative control Z20)

The corpus lives at
`engine/crates/membrane-runtime/tests/fixtures/context-outcome-baseline/`.
A missing or empty corpus at this exact path is the Z20 negative control:
`scripts/qualification/cases/mem-windows.mjs` (case `BM11`) fails closed when
this directory is absent, empty, or when `tasks.jsonl`/`expected-evidence.jsonl`
do not reference the same `taskId` set.
