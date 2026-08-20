# Membrane Adapt — parent workspace interface

Adapt lives at `membrane/adapt/` and consumes a small set of shared workspace
services through one explicit boundary.

## Import boundary

| Capability | Adapter | Parent source | Consumers |
|---|---|---|---|
| Membrane loopback port | `workspace_runtime.membrane_port` | `tools/lib/memory/runtime_config.py` | `adapt_persistence`, Doctor/conformance |
| Session inventory | `workspace_runtime.context_session_inventory` | `tools/pipelines/memory/context_session_inventory.py` | `multiwriter_conformance` |
| Session adapters | `workspace_runtime.context_session_adapters` | `tools/pipelines/memory/context_session_adapters.py` | `multiwriter_conformance` |
| Append-only mirror | `workspace_runtime.mirror_append_only` | `tools/pipelines/memory/mirror_append_only.py` | `multiwriter_conformance` |

All Adapt code that needs these services should import **`workspace_runtime`**,
not reach into `tools/` directly. Missing services fail closed.

## What is intentionally out of scope

- Duplicating shared `tools/pipelines/memory` services inside Adapt
- Pretending Cortex/Forge Doctor checks exist (see `doctor.py --scope`)
