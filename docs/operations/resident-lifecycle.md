# Membrane resident lifecycle

Membrane runtime never runs without a visible tray surface (Architecture B,
decided 2026-08-27 — see
`docs/design/hub-redesign/DECISION-PROCESS-ARCHITECTURE.md`). Three
processes, two resident:

- **Native tray app** (resident, parent) — the tray icon and popover. It
  spawns and supervises the daemon and owns its lifecycle end to end.
- **`membrane-daemon`** (resident, child) — headless: no window, no icon, no
  dock/taskbar presence. It hosts the Membrane runtime (`membrane-runtime`'s
  `run_hub_runtime` service, unchanged). It cannot outlive the tray.
- **Membrane Hub dashboard** (on-demand) — a Tauri client launched from the
  tray, closed freely, costing nothing while shut. It links no
  `membrane-runtime` dependency and holds no worker thread; it proxies one
  inherited bootstrap connection and makes read-only loopback calls against
  the daemon the tray already started.

Lifetime coupling is OS-enforced, not cooperative:

- **Windows** — the tray creates a Job Object with
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and spawns the daemon into it. Tray
  exit by any means (clean quit, crash, `TerminateProcess`, Task Manager)
  makes the kernel terminate the daemon; no application code has to run for
  that to hold.
- **macOS** — the daemon is supervised by the tray's `DaemonSupervisor`,
  which tracks the daemon over a pipe/`kqueue` `NOTE_EXIT` pair; parent death
  closes the pipe and the daemon exits.

Crash-loop detection, drain, restart, and readiness now live in the native
tray's process supervision, not in an in-process Hub thread.

Stateless MCP/CLI clients (see `mcp/client.mjs`) never spawn a runtime
process; they are thin HTTP clients against the daemon's loopback service.
Tray off means no daemon means no Membrane context: typed
`membrane_unavailable` with reason `hub_inactive` and `retryable: true`.
