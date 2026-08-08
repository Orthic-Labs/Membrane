# Hub overview

Hub is read-only. It consumes `hub.snapshot` operation envelopes and renders
exactly eight resources: deliveries, providers, repositories, adapters, devices,
memory, sentinel, and alerts. Each resource displays state, reason, items, source
operation, observed time, schema, evidence, and resolver. Missing values remain
`unknown`; unavailable sections never imply liveness or health.

Installation, adapter, and delivery dimensions are shown separately when present.
Alerts sort deterministically by severity, then reason, and retain evidence plus
resolver. Cached snapshots are visibly marked stale. Snapshot operation errors
remain explicit and do not fall back to an inferred status.
