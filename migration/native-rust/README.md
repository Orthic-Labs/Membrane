# Membrane native-runtime migration ledger

**Canonical migration plan:** [`MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md`](MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md)

**Adapt semantic authority:** [`../../docs/architecture/subsystems/adapt.md`](../../docs/architecture/subsystems/adapt.md)

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

- N0-N1: canonical invocation graph, runtime-language policy & frozen internal contracts land with recurring gates; 29 runtime-language blockers are closed, with zero production interpreter rows in sealed manifest.
- N2 is PARTIAL: `membrane-transcript` is native production owner, consumers use its native seam, & Python `continuity` is excluded from production graph. Final deletion/exclusion receipt & installed qualification remain open.
- N3 is DONE: the native deterministic Adapt core, authority/admission, semantic sealing, and fail-closed contracts have landed.
- N4 is DONE: native proposal/review/adjudication/apply supports explicit user-selected transcripts with exact source hash/rebinding; automatic implicit host signals remain optional and separately evaluated.
- N5-N8 are PARTIAL: native implementations have landed; exact installed qualification/deletion/packaging receipts remain pending.
- N9 is PARTIAL: native Membrane seam/storage binding has landed; CodeRight mutation is outside this migration lane. Independent verification, commit, & installed receipts remain pending.
- N10 is NOT SEALED: native-only release seal remains withheld pending N2/N5-N9 evidence & Section 17 qualification.

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
watcher runs only inside active tray-owned daemon; tray-off access is an explicit bounded
one-shot, while resident requests receive typed unavailability. Cortex owns
durable memory and remains application-API-only.
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
