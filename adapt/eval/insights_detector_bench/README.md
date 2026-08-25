# Adapt Insights detector benchmark (honest, unrigged)

Status: IN PROGRESS (skeleton commit). See this file's later revisions for
the finished methodology and the measured per-detector precision/recall
table.

Goal (canon `ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md` sections 6 and
11.2): give the 33 native Insights detector families (`membrane_adapt::
insights::detectors::run_all_detectors`) a portable, checked-in, labelled
corpus plus a scoring harness that reports **per-detector** precision and
recall — not one aggregate pass/fail number — and that does not hide the
canon-required adversarial classes (negation, quoted/context-carried text,
tool-result-carried text, hypothetical narration, cross-session duplicates,
severity calibration).

This corpus is intentionally separate from the existing
`adapt/eval/insights_bench/v1` "sealed P0.5" corpus. That corpus's own
adversarial ("negative trap") cases were found, during construction of this
one, to pass only because of incidental escape-hatch phrases already
special-cased elsewhere in the detector code (e.g. `is_historical_or_negated`
matching "reviewing my earlier message" or "will not"), or because the case
was structurally incapable of ever firing (wrong event kind / missing a
required second event) — not because the guard the case claims to exercise
actually held. See below for the reproduction and the honest numbers.
