# Adapt Insights detector benchmark (honest, unrigged)

Status: corpus and scoring harness complete; measured scorecard pending an
execution of the native Rust detector path.

This benchmark gives all 33 native Insights detector families emitted by
`membrane_adapt::insights::detectors::run_all_detectors` a portable,
checked-in, labelled corpus and reports precision and recall per detector.
It implements the portable-conformance requirements in
`docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md` sections 6 and
11.2. It is synthetic conformance evidence, not a real held-out corpus and
must not be cited as production precision.

## Corpus

`cases.jsonl` contains 50 deterministic cases:

- 33 true-positive cases, covering every native family;
- 11 adversarial negatives expected to produce no firing, covering negation,
  user-authored quotation/hypothetical text, tool-result-carried text, and
  cross-session duplicate boundaries;
- 6 known-gap cases that preserve canonical truth separately from the
  detector behavior documented when the corpus was authored.

`build_cases.py` is the source generator. Regeneration must be byte-identical:

```sh
python3 adapt/eval/insights_detector_bench/build_cases.py
git diff --exit-code -- adapt/eval/insights_detector_bench/cases.jsonl
```

The current checked-in corpus SHA-256 is
`ed58f58e96eed35fc282b1c6d846f0869acf94085ba3c04afa5efcd672c2decf`.

## Honest truth model

Every case has two deliberately separate fields:

- `should_fire` is canonical ground truth: what a correct detector run should
  emit. It is the only label used for precision/recall scoring.
- `documented_actual_fire` exists only on `known_gap` cases. It records the
  measured buggy firing set solely as a drift assertion. It never replaces
  canonical truth and therefore cannot turn a known false positive or false
  negative into a passing score.

Non-gap cases must match canonical ground truth exactly. Known-gap cases must
reproduce the documented runtime result exactly until a deliberate detector
fix updates both the implementation and benchmark. A silent fix is welcome,
but it intentionally fails the drift assertion so the corpus and scorecard
cannot become stale without review.

The six documented gaps are:

| Case | Canonical truth | Documented detector behavior | Gap |
|---|---|---|---|
| `quoted_assistant_verification_claim_gap` | no firing | `verification_claim_without_tool_evidence` | false positive |
| `quoted_assistant_claimed_then_corrected_gap` | no firing | `claimed_verified_then_corrected`, `verification_claim_without_tool_evidence` | two false positives |
| `tool_carried_relayed_by_assistant_gap` | no firing | `verification_claim_without_tool_evidence` | false positive |
| `hypothetical_assistant_verification_claim_gap` | no firing | `verification_claim_without_tool_evidence` | false positive |
| `hypothetical_assistant_claimed_then_corrected_gap` | no firing | `claimed_verified_then_corrected`, `verification_claim_without_tool_evidence` | two false positives |
| `apostrophe_contraction_falsely_suppresses_correction` | `claimed_verified_then_corrected`, `verification_claim_without_tool_evidence` | `verification_claim_without_tool_evidence` | one false negative |

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
`should_fire`. The separate known-gap drift test reports whether current
runtime behavior still equals `documented_actual_fire`.

Do not derive a claimed measured score from the JSON labels alone. A result is
measured only when this harness executes `run_all_detectors` from the tested
revision.

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
