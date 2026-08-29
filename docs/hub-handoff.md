# Membrane Hub runtime authority (CU-H01)

Under Architecture B (decided 2026-08-27 — see
`docs/design/hub-redesign/DECISION-PROCESS-ARCHITECTURE.md`), the resident
native tray app owns Membrane's runtime lifetime, not the Membrane Hub
dashboard. The tray spawns and supervises a headless `membrane-daemon` child
process (built from `engine/crates/membrane-runtime`'s `membrane-daemon`
binary) under OS-enforced lifetime coupling — a Windows Job Object with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, or the macOS tray's
`DaemonSupervisor` (pipe/`kqueue` `NOTE_EXIT`). `cortex` remains a
durable-memory CLI, not a second resident artifact. No external product
manifest, add-on handoff, or retired Hub installer is part of current
operation.

## Resident startup

- **Tray-managed:** the resident native tray app spawns and supervises the
  `membrane-daemon` child; the daemon hosts the Membrane runtime and cannot
  outlive the tray.
- **Hub dashboard is an on-demand client:** `apps/membrane-hub` (Tauri)
  links no `membrane-runtime` dependency and owns no worker thread. It is
  launched from the tray, proxies one inherited bootstrap connection, and
  makes read-only loopback calls against the daemon; closing it costs
  nothing.
- **No child compatibility path:** there is no `supervisor-child` process or
  adoption fallback distinct from the tray-owned daemon.
- **Tray inactive:** no daemon exists, so no runtime exists; stateless
  clients (including `mcp/client.mjs`, which never spawns a runtime) return
  typed, retryable `membrane_unavailable { reason: hub_inactive }`.
- **No implicit OS registration:** daemon startup is explicit and
  tray-managed; no separate product scheduler, standalone runtime, or
  compatibility shim is installed.

All install, update, uninstall, release, & cleanup actions remain receipt-bound
to current Membrane Hub contracts. Historical migration records stay archival
evidence and do not create an active runtime path.
