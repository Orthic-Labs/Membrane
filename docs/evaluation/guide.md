# Guide qualification entrypoint

Guide qualification rebuilds document projections from hash-bound sources, then
checks stable anchors, stale-reference rejection, incremental sync, & recall
determinism.

Implementation surface: `membrane-runtime::guide::{db,outline,identifier,doc_projection,doc_spine}`.
Guide output remains a source pointer; Cortex durable-memory writes are out of scope.
