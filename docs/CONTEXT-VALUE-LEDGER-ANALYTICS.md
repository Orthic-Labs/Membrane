# Context Value Ledger reconciliation and analytics

The Context Value Ledger reconciler turns canonical, content-free events into a deterministic
N-installation report. Installations, clients, providers, artifact families, sessions, phases,
policy activation receipts, and statuses are discovered from the ledger; none are hardcoded. The existing legacy
`daily-analysis.py` remains a compatibility report while the canonical ledger is in dual-write.

Before reconciliation, `context-value-daily.py` builds an independent schema-v3 turn inventory by
scanning uncapped local Claude, ClaudeMM, and Codex session sources outside the hooks being audited.
It records discovered/parsed counts and unreadable, malformed, skipped, or other exclusions per
client. Missing or unparsed sources are explicit loss/unknown coverage; they are never silently
converted into zero observed turns. Built-in producers project raw local session labels into the
same opaque session IDs used by Python and JavaScript telemetry, so the census can join without
exporting raw provider session IDs.

```text
canonical SQLite / JSON rows
          |
          v
schema validation -> lifecycle reconciliation -> per-install/session/provider cube
          |                         |
          |                         +-> typed gaps + remediation owner
          v
deterministic content-free JSON (production traffic only by default)
```

## What is reconciled

| Invariant | Typed result when incomplete |
|---|---|
| observed supported turn has a provider expectation set | `should_have_used_but_didnt` |
| every expected provider starts and has one terminal | `missing_expected_start`, `missing_expected_terminal` |
| one semantic terminal closes one attempt | `duplicate_terminal`, `conflicting_terminal` |
| started/terminal phases have their matching lifecycle edge | `missing_lifecycle_terminal`, `orphan_lifecycle_phase` |
| successful write commit has local embedding and export | `committed_write_missing_embedding`, `committed_write_missing_export` |
| every installation active in the workspace at the cutoff imports and embeds a committed artifact | `committed_write_missing_peer_apply`, `committed_write_missing_peer_embedding` |
| every successful delivery later closes as used/ignored/contradicted/unknown | `delivery_missing_value_terminal` |

Each gap includes a deterministic `gap_id`, owning component, finite reason, fixed remediation,
source installation/session/trace/provider/family identifiers, policy activation receipt, missing phase, and target
installation when replication is incomplete. `turn.observed` is the external evidence boundary for
missing-hook accounting: every supported harness turn must emit it before planner execution so a
planner omission cannot disappear from the ledger.

The report contains:

- exact source and report digests plus the ledger cutoff timestamp;
- dynamic installation and opaque-session accounting;
- an exact installation/client/family/provider/operation/phase/status/policy-activation cube;
- provider and write funnels;
- success/failure counts and quantities/durations;
- typed gap counts and individual content-free gap rows.

Prompts, memory bodies, paths, usernames, hostnames, arguments, replies, and raw errors are never
selected from SQLite and are rejected in JSON input by the shared event schema. Smoke and eval
events are excluded from the production report by default. Use their explicit traffic class only
for isolated instrumentation checks.

Sync is a bounded job, not a resident-service startup. It keeps the installation UUID stable and
must not advance or replace the resident service's `startup_generation` or
`current_service_instance_id`; each invocation uses a separate ephemeral job UUID only for its
telemetry process dimension. Local exports carry only opaque source commit, trace, workspace, and
artifact identifiers. A peer import can report success only when it links `replicated_from` that
source commit; missing provenance is recorded as `source_commit_missing`, and an engine failure has
a typed failed terminal. Cursor and health events are queued locally on every completed run. Sync
does not call a remote telemetry endpoint.

New mirror events and causal cursor marks carry the positive pair `logical_clock` + `origin_seq`.
Per-origin sequences are contiguous; event winners and cursor progress use causal order rather than
wall clock. Causal events more than five minutes in the future fail closed before apply and cannot
advance a cursor, while timestamp-only legacy history remains readable. Claim/retirement intervals scoped to the
event workspace determine which installations owe peer-apply evidence at each commit; retired or
not-yet-claimed installations are not permanent fleet obligations.

Installation conformance derives privacy evidence instead of accepting self-reported counters. The
runner semantically scans the read-only local ledger and the exact exported snapshot, exercises its
path/URL/email/hostname canaries, and reports only counts plus source digests. Forbidden field names
(`prompt`, `body`, `content`, `path`, `hostname`, and related raw-text fields) or path/URL/email/
hostname-like values fail the privacy check. UUIDs and digest-based opaque identifiers remain valid.

The registry owns providers, artifact families, phases, and cardinality/retention/byte/latency
budgets. The language-neutral schema owns operations, statuses, relations, types, structural rules,
and forbidden fields. Together they reject forbidden keys, invalid types, unregistered dimensions,
and over-budget envelopes; they cannot prove that every arbitrary syntactically valid token was not
copied from user-authored text. Built-in producers therefore canonicalize session, trace, workspace,
and artifact IDs before emit, and the authenticated loopback endpoint is the trusted local producer
boundary. Mirror/conformance/snapshot scans provide the second fail-closed boundary for forbidden
keys and path/URL/email/hostname-like values. Raw provider session IDs remain local; only their
projected opaque joins may enter a snapshot.

## Strict snapshots

Each local snapshot is named `snapshot-<installation_uuid>.json`, carries a hash-bound envelope, and
binds its cutoff to the exact source watermark/reconciliation report. Aggregation discovers any
number of files dynamically, rejects legacy label-named snapshots, and selects only the freshest
valid snapshot per installation. Every projected report table preserves the exact
`policy_activation_sha256` dimension; filters use full-digest equality, and malformed digests fail
closed. Missing or partial measures remain null/red rather than zero.
While `memright-daily` is disabled, snapshots must declare `manual_unscheduled`; no dashboard may
turn an ad-hoc cutoff into a continuous-freshness claim.

The snapshot retains opaque per-session operation/phase/status aggregates and an observer × origin
replication matrix. A deterministic origin hash is resolved only against installation UUIDs already
present in the reconciled membership set; unknown legacy origins stay null. Each peer row carries
applied and available causal sequence, lag, latest status, prior failure count/reason, and last
success/observation. A successful all-null sequence is an explicit zero-event peer. This lets the
operator identify which installation is stale or failing without exporting hostnames or raw event,
trace, artifact, prompt, path, or error data.

## Commands

Windows daily/on-demand production snapshot:

```powershell
py -3.11 tools/pipelines/memory/context-value-daily.py `
  --db tools/.cache/memory/memright-engine.db `
  --output tools/.cache/metrics/context-value-daily.json `
  --pretty --fail-on-gap
```

macOS/Linux uses the identical implementation:

```bash
python3 tools/pipelines/memory/context-value-daily.py \
  --db tools/.cache/memory/memright-engine.db \
  --output tools/.cache/metrics/context-value-daily.json \
  --pretty --fail-on-gap
```

Reconcile exported event rows, or combine any number of sources into one N-installation report:

```powershell
py -3.11 tools/pipelines/memory/context-reconcile.py `
  --json installation-a-events.json `
  --jsonl installation-b-events.jsonl `
  --db installation-c.db `
  --output context-value-all.json --pretty
```

`--fail-on-gap` writes the complete report and exits 1 when reconciliation gaps exist. Invalid or
content-bearing input exits 2. File output is flushed to a temporary file and atomically replaced,
so interruption cannot publish a partial report. The command is read-only and does not enable or
invoke `memright-daily`.
