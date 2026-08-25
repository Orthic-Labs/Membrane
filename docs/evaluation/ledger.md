# Ledger qualification entrypoint

Ledger qualification rebuilds document projections from hash-bound sources, then
checks stable anchors, stale-reference rejection, incremental sync, & recall
determinism.

Implementation surface: `membrane-runtime::ledger::{db,outline,identifier,doc_projection,doc_spine}`.
Ledger output remains a source pointer; Cortex durable-memory writes are out of scope.
