# Membrane Hub runtime authority (CU-H01)

Membrane Hub is Membrane's sole desktop runtime, installer, supervisor, update,
release, & install-cleanup authority. It hosts `membrane-runtime` inside the
active Hub process; `cortex` remains a durable-memory CLI, not a second
resident artifact. No external product manifest, add-on
handoff, or retired Hub installer is part of current operation.

## Resident startup

- **Hub-managed:** Membrane Hub starts and drains its linked runtime in-process.
- **No child compatibility path:** there is no `supervisor-child` process or
  adoption fallback; shutdown retains and joins the Hub-owned runtime thread.
- **Hub inactive:** no runtime exists; stateless clients return typed,
  retryable `membrane_unavailable { reason: hub_inactive }`.
- **No implicit OS registration:** service startup is explicit or Hub-managed;
  no separate product scheduler, standalone runtime, or compatibility shim is installed.

All install, update, uninstall, release, & cleanup actions remain receipt-bound
to current Membrane Hub contracts. Historical migration records stay archival
evidence and do not create an active runtime path.
