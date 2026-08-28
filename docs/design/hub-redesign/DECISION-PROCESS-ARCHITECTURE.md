# Decision: Membrane process architecture and tray ownership

**Status:** DECIDED by Adrian, 2026-08-27 · binding on all Hub/tray work
**Supersedes:** the single-process model currently implemented in `apps/membrane-hub`
**Requires doctrine edit:** yes — see §7

---

## 1. The governing rule

> **Membrane must never run as a process the user cannot see.**

Every other decision in this document exists to serve that rule. There is no state in which the
Membrane runtime is active without a visible tray surface representing it.

This is stronger than "the runtime should be visible." It is: the runtime *cannot* outlive its
visible surface, and the mechanism enforcing that is the operating system, not application code
that might not run.

## 2. Decision: Architecture B — headless daemon, parented to the tray

Three processes. Two resident.

| Process | Lifetime | Role |
|---|---|---|
| **Tray app** | resident · **parent** | Tray icon + popover. Spawns and supervises the daemon. Owns lifecycle. |
| **Daemon** | resident · **child** | Headless. No window, no icon, no dock/taskbar presence. Runs the Membrane runtime. Dies with the parent. |
| **Dashboard (Hub)** | on demand | The full Hub UI. Launched from the tray, closed freely. Costs nothing while shut. |

The daemon is *headless* in the strict sense: it has no UI of any kind and no way to be observed
except through the tray app that owns it.

### Why not the current single-process model

The single-process model (runtime as a thread inside a Tauri app) satisfies §1 structurally, but
keeps a webview resident permanently for a surface the user is not looking at. On macOS, where a
menu-bar app is never suspended, that cost is paid continuously.

### Why B does not weaken §1

Because the lifetime coupling is kernel-enforced, not cooperative:

- **Windows:** the tray app creates a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and
  spawns the daemon into it. If the tray exits by *any* means — clean quit, crash, `TerminateProcess`,
  Task Manager — the kernel terminates the daemon. Application code does not need to run for this
  to hold.
- **macOS:** the daemon holds a pipe to the parent; parent death closes the pipe and the daemon
  exits. `kqueue` `NOTE_EXIT` as a second signal.

An orphaned daemon is therefore not a state the system can reach.

## 3. Decision: tray UI toolkit

| Platform | Toolkit |
|---|---|
| **macOS** | Native — SwiftUI + AppKit |
| **Windows** | **Slint** |

Rationale for Slint on Windows: the popover is bars, text, key/value rows and action rows. It has
no morphing geometry and no continuous animation. The HeardRight pill was the case where a
non-webview toolkit genuinely struggled — moving, morphing, hover-responsive geometry — and this
is not that. If Slint proves unable to hold the approved design, that is a finding to report, not
a thing to work around silently.

Slint is Rust, so the Windows tray and the daemon share a language and can share types.

## 4. Failure semantics — the visible-failure contract

The inverse failure (daemon dies, tray survives) is **by design**, not a defect. It is the correct
expression of §1: the user must be able to see that Membrane is not working.

Required behaviour:

1. Daemon dies → **tray icon turns red immediately**.
2. User opens the popover → it states plainly that the daemon is not running, with the typed reason.
3. The popover offers **Restart** as an explicit action.
4. An automatic restart mechanism runs, with crash-loop detection (carry over the existing logic in
   `apps/membrane-hub/src-tauri/src/supervisor.rs`).
5. A crash loop is surfaced, never silently retried forever.

What must never happen: the tray showing a healthy state while the runtime is dead, or the runtime
alive with no icon.

## 5. What moves where

- `run_hub_runtime` (from `membrane_runtime::service`) becomes the daemon's `main()`. **The runtime
  code itself does not change.**
- `supervisor.rs` — crash-loop detection, drain, restart, readiness — moves from in-process thread
  supervision to child-process supervision inside the tray app.
- Tray icon, status colours, popover anchoring/dismissal move to the native tray apps.
- The Hub dashboard keeps the ported HeardRight shell and becomes an on-demand window.

## 6. What this does not change

- Membrane's six subsystems and their ownership boundaries.
- The frozen producer model: envelope parsing, `normalizeSnapshot`, `dashboardModel`,
  `lifecycleReasonLabel`, typed reason states.
- The five public V1 shapes.
- Typed degradation vocabulary (`hub_inactive`, `graph_missing`, `not_instrumented`, `budget_drop`,
  `stale`), which must continue to surface verbatim.

## 7. Doctrine edit required

`docs/agent-rules.md` currently states:

> Membrane runtime executes only inside the active Hub process; Hub off means no Membrane context
> (typed `membrane_unavailable { hub_inactive }`).

The *intent* of that invariant is preserved exactly — no runtime without a visible surface — but
the *mechanism* changes from "same process as the Hub" to "child process of the tray, with
kernel-enforced lifetime coupling." That line must be revised to describe the guarantee rather
than the implementation, e.g.:

> Membrane runtime never runs without a visible tray surface. The runtime executes as a child
> process of the resident tray app, with OS-enforced lifetime coupling; no tray means no runtime
> (typed `membrane_unavailable { hub_inactive }`).

Do not edit that file without confirming the wording.

## 8. Open items

- Slint's ability to hold the approved popover design — to be proven by prototype, not assumed.
- The IPC surface between tray and daemon (transport, schema, readiness handshake) — to be specced.
- Whether the dashboard stays Tauri or is hosted by the native tray apps — deferred; the Tauri port
  already exists and works.
