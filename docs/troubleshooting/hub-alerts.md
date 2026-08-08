# Hub alert & reason runbook (MBR-1005)

Hub renders stable reason strings from its versioned protocol. Each entry below
has one smallest safe first action; links are deliberately local so the map is
usable offline.

## Installation

- `verified` — [diagnostics](diagnostics.md#read-only-diagnostics); no action; retain the snapshot as evidence.
- `evidence_unavailable` — [diagnostics](diagnostics.md#read-only-diagnostics); rerun a read-only snapshot.
- `wrong_installation` — [migrations](migrations.md#installation-identity); compare installation IDs; do not repair in place.
- `incompatible_schema` — [migrations](migrations.md#schema-migration); verify release/schema; stop before writes.
- `unexpected_data_root` — [backups](backups.md#data-root-boundary); compare root digest; do not point Hub at another root.

## Delivery

- `delivered` — [diagnostics](diagnostics.md#read-only-diagnostics); no action; retain receipt evidence.
- `not_selected` — [diagnostics](diagnostics.md#read-only-diagnostics); confirm selection inputs before changing policy.
- `selected_without_delivery` — [diagnostics](diagnostics.md#read-only-diagnostics); inspect the delivery receipt once; do not retry blindly.
- `evidence_unavailable` — [diagnostics](diagnostics.md#read-only-diagnostics); collect a fresh snapshot.

## Provider readiness

- `ready` — [diagnostics](diagnostics.md#read-only-diagnostics); no action; retain fresh observation.
- `degraded` — [diagnostics](diagnostics.md#read-only-diagnostics); inspect test-query evidence.
- `unavailable` — [diagnostics](diagnostics.md#read-only-diagnostics); verify fresh evidence before restart.
- `unknown` — [diagnostics](diagnostics.md#read-only-diagnostics); treat as no evidence.
- `readiness_missing` — [diagnostics](diagnostics.md#read-only-diagnostics); request a fresh readiness observation.
- `process_exists_without_readiness` — [diagnostics](diagnostics.md#read-only-diagnostics); do not infer liveness from process existence.
- `identity_drift` — [migrations](migrations.md#installation-identity); compare identity fields; do not merge records.
- `observation_missing` — [diagnostics](diagnostics.md#read-only-diagnostics); request a fresh observation.
- `readiness_stale` — [diagnostics](diagnostics.md#read-only-diagnostics); refresh observation before any repair.
- `test_query_failed` — [diagnostics](diagnostics.md#read-only-diagnostics); preserve failure evidence; do not promote provider.
- `schema_version_mismatch` — [migrations](migrations.md#schema-migration); stop and use matching release tooling.
- `required_identity_or_reason_missing` — [diagnostics](diagnostics.md#read-only-diagnostics); capture contract failure; do not fill fields locally.
- `observation_freshness_invalid` — [diagnostics](diagnostics.md#read-only-diagnostics); discard invalid observation and recapture.

## Hub transport & generic reasons

- `observed` — [diagnostics](diagnostics.md#read-only-diagnostics); no action; retain authoritative evidence.
- `authoritative` — [diagnostics](diagnostics.md#read-only-diagnostics); no action; retain the source receipt.
- `stream_not_configured` — [diagnostics](diagnostics.md#read-only-diagnostics); treat stream as unavailable.
- `readiness_handle_missing` — [diagnostics](diagnostics.md#read-only-diagnostics); treat provider status as degraded.
- `source_not_connected` — [diagnostics](diagnostics.md#read-only-diagnostics); collect a fresh snapshot.
- `reason_unavailable` — [diagnostics](diagnostics.md#read-only-diagnostics); preserve unknown state.
- `unknown` — [diagnostics](diagnostics.md#read-only-diagnostics); do not infer health.

## Recovery topics

Any action beyond these first steps must follow [backups](backups.md), [migrations](migrations.md),
[update rollback](update-rollback.md), and [support bundles](support-bundles.md).
