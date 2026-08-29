# Adapt real-session held-out corpus v1

This corpus closes the first **real-session** measurement gap for Adapt Insights. It is a small, single-user seed corpus, not a promotion-quality population sample.

## Contents

| Path | Purpose |
| --- | --- |
| `selections.v1.json` | Source-bound, agent-reviewed event selections. Paths use `${HOME}` and never enter the sealed cases. |
| `v1/dev.jsonl` | 8 real-session cases used for the first detector measurement. |
| `v1/heldout.jsonl` | 8 frozen real-session cases. Integrity is checked, but detectors have not been run over this split. |
| `v1/manifest.json` | Split digests, host counts, family coverage, and held-out policy. |
| `../build_real_heldout.py` | Deterministic builder/validator using the installed native `membrane adapt mine` production parser path. |

## Collection and review

- Collection/freeze date: **2026-08-29**.
- Sources: 12 completed local transcript files selected from Claude Code, Codex, and Command Code stores discovered by `membrane-transcript` conventions.
- Cases: 16 total (8 dev, 8 heldout), covering 8 distinct detector families on each split.
- Selection is event-level. No whole session is copied into the repository.
- Labels were reviewed against the Insights detector glossary and exact evidence event ids returned by the native `membrane adapt mine` path. They are **agent-reviewed**, not human-reviewed.
- The builder reparses every source and fails if the exact family/event binding no longer exists.

### Family coverage

| Family | Dev | Heldout |
| --- | ---: | ---: |
| `claimed_verified_then_corrected` | 1 | 1 |
| `degraded_provider_treated_as_success` | 1 | 1 |
| `guard_firings` | 1 | 1 |
| `ignored_tool_failure` | 1 | 1 |
| `stale_terminology_surfacing` | 1 | 1 |
| `user_swearing` | 1 | 1 |
| `verification_claim_without_tool_evidence` | 1 | 1 |
| `visible_frustration` | 1 | 1 |

Host case counts across both splits: Claude Code 5, Codex 2, Command Code 9.

## Redaction contract and second-pass review

The production parser performs its own secret redaction and event compaction first. The evaluation export then:

- replaces Windows/macOS/Linux home identities and the local username;
- replaces emails, URLs, IPv4 addresses, UUIDs, task/tool ids, bearer/key/token patterns, and private-key material;
- hashes session ids and event identities;
- caps exported event text to a short head/tail excerpt;
- scans every emitted text field for residual credential, home-path, email, network-address, and user-name shapes before writing.

Some production evidence objects expose an already-compacted prefix around a native detector match. For `guard_firings` and `degraded_provider_treated_as_success`, the builder appends a bracketed source-observed marker only after exact native family + event-id binding. This preserves the evaluated signal without copying the omitted private log bytes.

Manual second-pass review found no live credentials, full emails, user real names, unhashed session ids, or absolute home paths in the emitted cases. Project-relative paths and product/repository names remain where they are necessary to interpret the failure.

Dropped candidates included local-command caveats misread as repeated asks, historical quoted documents, huge source dumps, ambiguous tool logs, and excerpts whose truncation removed the actual signal. Aggressive exclusion was preferred over completeness.

## Split discipline

`dev.jsonl` may be measured and used to plan future detector changes. `heldout.jsonl` is frozen and **must not be executed by routine tests or corpus regeneration**. Its semantic seals and schema may be checked without running detectors. The owner should run the heldout split exactly once after any dev-driven tuning decision is frozen.

## Validation and benchmark

From the repository root:

```text
py -3.11 adapt/eval/build_real_heldout.py
py -3.11 adapt/eval/build_real_heldout.py --validate-only
```

Dev benchmark through the installed native CLI:

```text
membrane adapt benchmark --input adapt/eval/n4_heldout/v1/dev.jsonl
```

Native test (RightKit only):

```text
cd engine
rightkit cargo test -p membrane-adapt --test real_heldout_corpus -- --nocapture
```

The test parses both splits through `portable_case_from_value`, executes detectors only on dev, and prints the per-family table.

### First dev result

Installed native CLI, report digest `aa6e5ec71de33f40a12c7332a195abaad17121d6fcaca4ecee02a303d48ed750`:

| Family | TP | FP | FN | Precision | Recall |
| --- | ---: | ---: | ---: | ---: | ---: |
| all eight listed families | 1 each | 0 each | 0 each | 1.00 | 1.00 |

This is mechanically encouraging but statistically weak: one positive case per family cannot establish production precision or the canonical 0.95 promotion gate. Negative-trap coverage remains in the larger synthetic corpus; this real v1 seed does not yet contain enough independently reviewed real negatives to estimate false-positive prevalence.

## Known limitations

- single machine, single user, English-only;
- only 3 of 10 supported transcript hosts represented;
- only 8 of 33 detector families represented;
- one real positive per family per split;
- agent-reviewed labels, not independent human labels;
- source files are local and therefore rebuilding elsewhere is intentionally unavailable;
- heldout has not been detector-executed;
- real negative traps require a larger independent review round before they can support precision promotion.
