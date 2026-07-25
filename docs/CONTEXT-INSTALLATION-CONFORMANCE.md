# Context installation conformance

`context_conformance.py` is the same read-only conformance gate on every installation. It discovers
no machine names from source code and accepts an arbitrary number of distinct peer installation
UUIDs in its evidence. Windows and macOS are platform fixtures, not special identities.

## What it proves

- schema-v2 immutable installation identity and startup generation;
- an owner replica cursor that acknowledges a genuine local-origin delta with a positive,
  contiguous `(logical_clock, origin_seq)` pair;
- zero event mutation, deletion, causal-cursor regression, or unacceptable future-clock skew in a
  digest-bound append-only audit;
- equal winner-map digests for every installation whose workspace membership interval was active at
  the snapshot cutoff;
- zero fresh canonical lifecycle reconciliation gaps;
- complete privacy-sentinel rejection with zero leaks;
- prompt p99 computed from the supplied samples and strictly below 1,000 ms;
- `memright-daily` absent, disabled, or unloaded; and
- an ad-hoc snapshot declared `manual_unscheduled`.

The JSON and Markdown reports contain opaque installation IDs, hashes, counts, bounded platform and
release fields, and typed failures. They never contain prompts, memory bodies, paths, hostnames,
memory IDs, raw errors, or mirror payloads. A local delta event ID is exported only as a SHA-256.

## Inputs

The runtime evidence JSON is schema-versioned and fail-closed. Its required top-level keys are:

```text
schema_version, platform, release, append_only, convergence, privacy,
prompt_latency_ms, minimum_prompt_samples, snapshot
```

`convergence.peer_winner_maps` is a dynamic list of
`{installation_id, winner_map_sha256}` objects. Installation IDs must be distinct canonical UUIDv4
values. Repeating one digest without distinct installation IDs does not prove N-installation
convergence.

Membership is also dynamic. An installation is a convergence obligation only when its immutable
workspace-membership interval includes the evidence cutoff. This prevents both hard-coded
two-machine assumptions and false failures from retired or not-yet-enrolled hosts. Legacy rows that
predate causal fields remain readable through an explicit compatibility path, but they cannot satisfy
the replacement candidate's causal-closure proof.

The reconciliation input is the content-free report emitted by
`tools/pipelines/memory/context_value_reconcile.py`. The mirror and identity inputs are read directly;
the command never writes the DB or mirror, installs or restarts MemRight, starts a replay, or changes
a scheduler.

## Run

Windows:

```powershell
py -3.11 tools/pipelines/memory/context_conformance.py `
  --installation-file tools/.cache/memory/installation.json `
  --mirror-root memory-mirror `
  --runtime-evidence <runtime-evidence.json> `
  --reconciliation-report <reconciliation-report.json> `
  --output <conformance.json> `
  --markdown-output <conformance.md> `
  --pretty
```

macOS/Linux:

```bash
python3 tools/pipelines/memory/context_conformance.py \
  --installation-file tools/.cache/memory/installation.json \
  --mirror-root memory-mirror \
  --runtime-evidence <runtime-evidence.json> \
  --reconciliation-report <reconciliation-report.json> \
  --output <conformance.json> \
  --markdown-output <conformance.md> \
  --pretty
```

Exit `0` means every invariant passed, `1` means a typed conformance failure was reported, and `2`
means the evidence or command input was invalid. The command probes scheduler state but never enables,
disables, loads, unloads, registers, or triggers `memright-daily`.

P3 is a fresh installed-host proof: each active installation must independently emit a green
conformance report from the same candidate commit. P4 is a separate aggregate proof that consumes
those fresh P3 reports plus the causal convergence and strict-snapshot evidence for the full active
membership set. A source-green test run is neither P3 nor P4, and no policy/cohort activation or
fresh replay may begin until aggregate P4 is green.

The local authenticated loopback endpoint is the trusted producer boundary. Built-in producers
canonicalize session, trace, workspace, and artifact identifiers before emission. Registry/schema
validation rejects forbidden keys, types, and budgets, but it cannot semantically prove that every
arbitrary token string was not copied from user-authored content. Mirror, conformance, and snapshot
scans therefore fail closed on forbidden keys and values shaped like paths, URLs, email addresses, or
hostnames.

Strict snapshots use UUID filenames and a hash-bound envelope containing the cutoff/watermark. The
aggregate selects the freshest valid snapshot per active installation, rejects legacy label-named
files, records ad-hoc collection as `manual_unscheduled`, and preserves missing or invalid evidence as
null/red rather than converting it to zero.

## Historical evidence bridge

Legacy rows cannot be promoted to canonical ledger events when their required installation, event,
session, trace, span, or terminal identities were never recorded. Generate a separate read-only
partial manifest instead:

```powershell
py -3.11 tools/pipelines/memory/context_telemetry_backfill.py `
  --db tools/.cache/memory/memright-engine.db `
  --rightcontext-jsonl tools/.cache/metrics/rightcontext-heartbeat.jsonl `
  --output <historical-partial.json> `
  --pretty
```

Every record is marked `evidence_quality=historical_partial`; missing fields are enumerated under
`unknown_fields`. Only an explicit historical RightContext heartbeat outcome is terminal evidence.
The importer opens SQLite with `mode=ro`, emits no canonical event IDs, and strips prompt previews,
paths, scopes, memory IDs, candidate IDs, provider internals, metadata, and raw errors.

The reconciler accepts these manifests independently or beside canonical sources:

```powershell
py -3.11 tools/pipelines/memory/context_value_reconcile.py `
  --historical <historical-partial.json> `
  --output <historical-reconciliation.json> `
  --pretty
```

Historical counts appear only in the report's `historical` section. They never increase canonical
event, session, lifecycle, success, or terminal totals.
