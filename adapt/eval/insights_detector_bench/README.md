# Adapt Insights detector benchmark (honest, unrigged)

Status: corpus and scoring harness complete; native scorecard refresh is
pending for current guard behavior.

This benchmark gives all 33 native Insights detector families emitted by
`membrane_adapt::insights::detectors::run_all_detectors` a portable,
checked-in, labelled corpus and reports precision and recall per detector.
It implements the portable-conformance requirements in
`docs/current/architecture/subsystems/adapt.md` sections 6 and
11.2. It is synthetic conformance evidence, not a real held-out corpus and
must not be cited as production precision.

## Corpus

`cases.jsonl` contains 50 deterministic cases:

- 33 true-positive cases, covering every native family;
- 17 adversarial negatives expected to produce no firing, covering negation,
  user/assistant quotation and hypothetical text, tool-result-carried and
  relayed text, contraction handling, and cross-session duplicate boundaries;

`build_cases.py` is the source generator. Regeneration must be byte-identical:

```sh
python3 adapt/eval/insights_detector_bench/build_cases.py
git diff --exit-code -- adapt/eval/insights_detector_bench/cases.jsonl
```

The current checked-in corpus SHA-256 is
`0331c7c7ac8745592c1e08fcdbfe30c4404113e35d0e2b60f7bb73c4526b6e53`.

## Honest truth model

Every case has two deliberately separate fields:

- `should_fire` is canonical ground truth: what a correct detector run should
  emit. It is the only label used for precision/recall scoring.
- `documented_actual_fire` is retained as an empty compatibility field; no
  current cases are known gaps.

All cases must match canonical ground truth exactly. Guard fixes converted the
former known-gap constructions into adversarial negatives:

| Former construction | Correct behavior |
|---|---|
| Quoted assistant verification claim | no firing |
| Quoted assistant claim followed by correction | no firing |
| Assistant relaying eligible tool verification | no unsupported-claim firing |
| Hypothetical assistant verification claim | no firing |
| Hypothetical assistant claim followed by correction | no firing |
| Contraction-heavy user correction | both genuine claim detectors fire |

## Run and record the scorecard

From the repository root, run the exact native harness through RightKit:

```sh
rightkit cargo test \
  --manifest-path engine/Cargo.toml \
  -p membrane-adapt \
  --test insights_detector_benchmark \
  -- --nocapture
```

The test output is the reproducible scorecard. It prints `TP`, `FP`, `FN`,
precision, and recall for each of the 33 families, scored against
`should_fire`. The separate known-gap drift test remains an empty compatibility
check because current corpus has no known gaps.

Do not derive a claimed measured score from the JSON labels alone. A result is
measured only when this harness executes `run_all_detectors` from the tested
revision.

## Measured result

The checked-in scorecard records the previous native run and must be refreshed
with the current revision using the harness command above. No new precision,
recall, or F1 claim is made until that native run executes.

## Relationship to the sealed corpus

This corpus is intentionally separate from
`adapt/eval/insights_bench/v1`, exercised by
`portable_insights_benchmark.rs`. During construction, some of that corpus's
negative traps were found to pass through incidental escape-hatch phrases
already special-cased elsewhere (for example `is_historical_or_negated`
matching `reviewing my earlier message` or `will not`), or because the event
shape could never invoke the detector. This benchmark instead keeps minimal,
faithful adversarial constructions and records real gaps rather than tuning
their text until they pass.
