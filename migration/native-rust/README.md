# Membrane native-runtime migration ledger

**Canonical migration plan:** [`MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md`](MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md)

**Adapt semantic authority:** [`../../docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md`](../../docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md)

This directory freezes Membrane's historical executable federation boundary at baseline
`322855c33e65dc936e3927570451c98e54fb0bd2` for MEM-000. It is migration input,
not runtime configuration or current implementation status. Current package status is summarized
below and governed by the canonical plan's Section 0.1.1 and Section 17.

## Ledger scope

`executable-ledger.json` records shipped/runtime artifacts and every discovered
production launch or loopback site. Python federation gateway/providers are
`port` rows owned by MEM-014, MEM-017–026, or MEM-027, then deleted by MEM-030.
The Python worker bridge is deleted by MEM-029. Existing Hub, MCP, Blueprint,
Cortex, and Ledger boundaries remain typed owner boundaries.

Canonical plan expands runtime closure beyond this federation-baseline ledger to transcript normalization, Adapt, MCP/renderer, Blueprint packaging, CodeRight integration, and native-only release qualification.

## Current cutover status

- N0-N1: canonical invocation graph, runtime-language policy & frozen internal contracts land with recurring gates.
- N2 is PARTIAL: `membrane-transcript` is the native production owner; deletion or proven release exclusion of Python `continuity` remains open.
- N3 is DONE: the native deterministic Adapt core, authority/admission, semantic sealing, and fail-closed contracts have landed.
- N4 is PARTIAL: the committed 44-case synthetic Taste conformance scorecard passes its declared thresholds (extraction precision `0.9667`/recall `1.0`, admission precision `0.9524`/recall `1.0`, semantic-projection precision `1.0`, and `0/11` authority-negative false positives). An independently sourced real-world held-out corpus, interval report, and released-package run remain open.
- N5 is PARTIAL: native Adapt source/CLI and an isolated copied source-built-binary test have landed. Exact released-package proof and replacement or explicit dev-only demotion of `scripts/run-adapt-installed-current.mjs` as an authority test remain open.
- N6-N9 remain separate open migration lanes. N10 remains BLOCKED on those lanes, N2 closure, N4/N5 release evidence, and the remaining package/SBOM/process-tree/native-only gates. The committed behavioral scorecards are no longer an N10 blocker.

Tests, benchmarks, evaluation runners, release helpers, and unrelated workspace tooling qualify
as development-only inputs only when the runtime manifest and release evidence prove them
unreachable from the installed product. Their parity responsibilities are named by fixture or
shadow packets; classification alone is not exclusion proof.

## Locked ordering and ownership

Legacy gateway order is `blueprint, audit, architect, cortex, git, live, rules,
anchors, skills`. Native order is `anchors, blueprint, rules, live_files, git,
audit, architect, skills, cortex`, as locked by MEM-010. Candidate IDs use
first-wins deduplication after native canonical ordering. One monotonic deadline
flows from ingress through freshness, provider work, merge, and publication.

Blueprint owns repository truth and remains protocol-only to Membrane. Its
continuous role is hosted under Hub lifecycle; explicit Hub-off access is a
bounded one-shot. Cortex owns durable memory and remains application-API-only.
Ledger owns its rebuildable document index. Membrane catalog owns grants,
generations, events, and content-free receipts. No provider opens another
owner's SQLite store.

## Nine-lane contract

`federation-contract-inventory.json` records each lane's input, output, terminal
errors, generation seal, trust class, provenance, ordering, parity fixture, and
cutover gate. Optional provider failures degrade locally with attributable
omissions; invalid request, scope, hard prerequisite, planner, or serialization
failures remain request failures. Public V1 shapes do not change.

## Validation

Arrays are lexically ordered by stable ID/path. `complete` is true only with no
unknown disposition, blocker, unowned launch site, or ownership collision.
Static packet checks must verify baseline, owned paths, diff whitespace, and
absence of changes outside these six files. MEM-001 seals behavior fixtures;
MEM-I01 independently reproduces coverage and acceptance.
