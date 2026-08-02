# Orthic Morph — parent workspace interface

Morph (`morph/`) is not a fully standalone package today. Live Taste apply and
Doctor conformance need a small set of parent-workspace services. Packaging is
**not** “add a pyproject and go”; define this contract, then optionally stub.

## Import boundary

| Capability | Adapter | Parent source | Consumers |
|---|---|---|---|
| Crypt loopback port | `workspace_runtime.crypt_port` | `tools/lib/memory/runtime_config.py` | `morph_persistence`, Doctor/conformance |
| Session inventory | `workspace_runtime.context_session_inventory` | `tools/pipelines/memory/context_session_inventory.py` | `multiwriter_conformance` |
| Session adapters | `workspace_runtime.context_session_adapters` | `tools/pipelines/memory/context_session_adapters.py` | `multiwriter_conformance` |
| Append-only mirror | `workspace_runtime.mirror_append_only` | `tools/pipelines/memory/mirror_append_only.py` | `multiwriter_conformance` |

All Morph code that needs these services should import **`workspace_runtime`**,
not reach into `tools/` directly (legacy direct imports are being migrated).

## Optional stubs

```sh
export MORPH_WORKSPACE_STUBS=1
```

Stubs allow offline import/unit tests without the parent checkout. They must
not be used for live Crypt apply or production Doctor receipts.

## What is intentionally out of scope

- Vendoring the entire parent `tools/pipelines/memory` tree into `morph/`
- Pretending Cortex/Sentinel Doctor checks exist (see `doctor.py --scope`)
- A one-liner standalone wheel until Membrane/Crypt APIs and session
  inventory are either vendored or published as real packages
