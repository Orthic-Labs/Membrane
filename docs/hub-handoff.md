# Membrane Hub runtime authority (CU-H01)

Membrane Hub is Membrane's sole desktop runtime, installer, supervisor, update,
release, & install-cleanup authority. It launches `membrane` & `cortex-service`
from its own self-contained package; no external product manifest, add-on
handoff, or retired Hub installer is part of current operation.

## Cortex-service startup

- **Hub-managed:** Membrane Hub starts `cortex-service` through its authenticated
  local supervisor & binds the exact current release evidence.
- **Headless/standalone:** `membrane service run` starts the service for servers,
  CI, or SSH-only hosts.
- **No implicit OS registration:** service startup is explicit or Hub-managed;
  no separate product scheduler or compatibility shim is installed.

All install, update, uninstall, release, & cleanup actions remain receipt-bound
to current Membrane Hub contracts. Historical migration records stay archival
evidence and do not create an active runtime path.
