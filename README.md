# Adapt

> **TL;DR:** Adapt learns durable working preferences from past AI coding sessions so future agents need fewer repeated corrections.

AI assistants usually forget how you like work done once a session ends. Adapt reviews local Claude
Code & Codex transcripts, finds repeated corrections or stable decisions, then turns only durable
ones into reusable rules.

Its pipeline:

1. parses local session history;
2. redacts sensitive text & drops unsafe material;
3. extracts candidate preferences, decisions & operational lessons;
4. deduplicates, scores & adjudicates candidates;
5. stores approved rules in MemRight for relevance-based recall.

Adapt does not retrain a model or save private chain-of-thought. It builds a small, auditable
preference layer around existing models. Rules remain typed, source-attributed, reversible &
quarantined when they would weaken security or expand permissions.

## Main entry point

```sh
python3 adapt.py --help
```

Workspace installations use `run_incremental_multiwriter.py` for conformance-gated incremental
mining.
