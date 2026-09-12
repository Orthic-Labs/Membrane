# Push qualification entrypoint

> Legacy compatibility qualification: Pull owns reduction, recovery, & publication in current architecture. Public `push` writes durable memory through Cortex.

Push qualification checks Pull-selected headroom handoff, head/tail capture,
skeleton fidelity, protected-span retention, compression accounting, prep
routing, content-addressed restore, & host telemetry isolation from
Cortex/Persist. It covers the `runc` → `skel` → `compress` → `truncate`
lineage without moving selection into Push.

Implementation entrypoints: `membrane cli push runc|skel|compress|prep|restore`.
