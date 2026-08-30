# Experiment — Local Semantic Worker

> **Status:** Fable review draft — not canonical.
> **Date:** 2026-08-29
> Existing canonical subsystem documents win on conflict. Re-derive implementation state before execution.

## Question

Can a very small local model perform semantic extraction over Ledger nodes cheaply and safely enough to justify deployment inside Membrane?

**Plausible, but benchmark before training and benchmark local separately from resident.**

## Suitable workload

Input:

```text
heading path
bounded source-bound node
closed extraction schema
LedgerEvidenceRefV1
```

Output:

```text
no_candidate
```

or typed candidates supported by that exact node.

Suitable tasks:

- detect explicit durable semantic material;
- normalize decisions/invariants;
- extract procedures;
- classify temporal state;
- extract explicit relations;
- abstain when ambiguous.

Unsuitable tasks:

- repository-wide reasoning;
- source-vs-code truth adjudication;
- global conflict resolution without retrieved evidence;
- final context planning;
- autonomous search;
- root-cause analysis over long trajectories.

## Why sub-1B may work

Ledger has already localized the source. The worker is not solving “find and understand the corpus”; it is solving “transform this coherent region conservatively or abstain.”

## Model policy

Architecture is model-agnostic.

Benchmark several size points/families, for example:

- ~300–400M control;
- Qwen3-0.6B-class candidate;
- ~1B candidate;
- ~1.5–2B upper control;
- strong larger/cloud teacher reference.

Do not fine-tune first.

## Benchmark

Freeze a labelled real-node corpus before model selection.

Measure:

- precision/recall by semantic kind;
- unsupported-claim rate;
- negation/temporal errors;
- evidence-binding accuracy;
- abstention calibration;
- latency/throughput;
- cold-load time;
- resident/peak RSS;
- CPU/GPU/NPU contention;
- energy/thermal effect;
- foreground CodeRight interference.

Choose the **smallest deployment clearing the semantic safety gates**, not the smartest model.

## Train only if justified

```text
zero/few-shot
   |
   +-- clears gates -> stop
   |
   +-- narrow repeatable misses
          -> labelled data
          -> SFT/distillation
          -> frozen held-out qualification
```

## Resident vs lazy

Compare:

A. always resident;
B. lazy-loaded on Ledger change batches;
C. scheduled/background batch;
D. no local model.

Semantic compilation is revision-triggered, so permanent residency may not be economically optimal even if the model is small.

## Runtime constraints

- tray-owned lifecycle;
- bounded inference queue;
- foreground workload priority;
- cancellable/background-yielding work;
- strict resource budget;
- model/prompt/schema version in derivation receipts;
- deterministic schema/provenance validation;
- failure -> `no_candidate`, never fabricated fallback.
