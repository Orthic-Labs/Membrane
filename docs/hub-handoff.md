# Membrane Hub runtime authority (CU-H01)

Membrane Hub is Membrane's sole desktop runtime, installer, supervisor, update,
release, & install-cleanup authority. It launches its self-contained
`membrane supervisor-child`; `cortex` remains a durable-memory CLI, not a
second resident artifact. No external product manifest, add-on
handoff, or retired Hub installer is part of current operation.

## Resident startup

- **Hub-managed:** Membrane Hub starts `membrane supervisor-child` through its
  authenticated local supervisor & binds exact current release evidence.
- **Headless/standalone:** `membrane supervisor-child` starts the resident for servers,
  CI, or SSH-only hosts.
- **No implicit OS registration:** service startup is explicit or Hub-managed;
  no separate product scheduler or compatibility shim is installed.

All install, update, uninstall, release, & cleanup actions remain receipt-bound
to current Membrane Hub contracts. Historical migration records stay archival
evidence and do not create an active runtime path.
