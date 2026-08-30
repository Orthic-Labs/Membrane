# Adapt Insights portable labelled benchmark (P0.5)

Frozen, language-neutral fixtures for per-detector precision/recall scoring of
Adapt Insights detectors. Canonical authority:
`docs/current/architecture/subsystems/adapt.md` sections 6.4–6.6,
7.3–7.4, 11.2, and plan item P0.5.

## Contents

| File | Purpose |
| --- | --- |
| `v1/cases.jsonl` | 70 labelled cases; one JSON object per line |
| `v1/manifest.json` | hashed manifest: digests, coverage matrix, glossary |

Identity:

- `bench_id`: `adapt-insights-bench-3329d6699a06`
- corpus sha256: `8ddd0e6dc9c9036ee68dcf7e05eb72ecd306fbd718df827861acb35ede8ba2ed`

## Regenerate / validate

```sh
python3 eval/build_insights_bench.py      # deterministic; byte-identical reruns
python3 eval/validate_insights_bench.py   # schema + digests + coverage
```

Validation uses `jsonschema` when available (workspace tools venv) and falls
back to an equivalent built-in structural checker otherwise. Both modes must
pass.

## Case shape

Each case conforms to `../../insights_bench_case.schema.json`:

- immutable semantic payload sealed by `payload_sha256` (canonical JSON,
  sorted keys, compact separators); `case_id = "ibc_" + payload_sha256`;
- mutable state envelope carries only lifecycle status + freeze receipts;
- `expected.detected` / `family_match` are the ground-truth labels;
  `min_severity` and `confidence_ceiling` support calibration checks.

### Language neutrality

Cases are pure data. Event roles ride in the `event_id` role tag so no
implementation-specific structures are needed:

- `…u` user message, `…a` assistant message, `…tc` tool call, `…tr` tool result.

Byte spans are contiguous UTF-8 offsets reconstructing the excerpt stream;
`source_digests[0]` is the sha256 of that concatenated stream (synthetic
fixtures are their own source).

Cross-session cases use composite session ids `xsession:<tag>|<tag>` with
per-session tags in `event_id`.

## Coverage

- **All 19 current detector families** (`adapt/src/adapt/insights.py`,
  canonical §6.4) — one positive case each.
- **All 14 missing priority families** (canonical §6.5): overengineering,
  architecture churn, repeated redesign, planning-instead-of-executing,
  unnecessary abstraction/dependency, scope expansion (single + repeated),
  verification theatre, false completion claim, instruction noncompliance,
  repeated same-theme correction, model-specific gotcha,
  client/tool-specific gotcha.
- **Required P0.5 case classes**: positive (`real_failure`, 32), negative
  traps — negated (25), quoted/context-carried (5), tool-carried (3),
  hypothetical narration (3), cross-session duplicate (2, one positive one
  negative). Total: 33 positive / 37 negative.

Negative traps encode the known false-positive classes from canonical §11.2:
negation, quoted/attributed text, signal text inside tool output, assistant
narration of counterfactual failures, and cross-session near-duplicates.

## Honesty

All text is synthetic; no human-sourced transcript content is included.
Labels prove only what each case's `honesty_limit` states — exhibiting a
family's operational pattern here does not establish root cause, recurrence,
or remediation justification (canonical §6.6).
