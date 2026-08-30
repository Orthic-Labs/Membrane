# Membrane tray–daemon contract

**Status:** implementation contract · Architecture B
**Decision source:** [`tray-daemon-process.md`](../adr/tray-daemon-process.md)
**Governing invariant:** Membrane runtime never runs without a visible tray surface.

## 1. Process topology & ownership

| Process | Residency | Owner | Responsibilities |
|---|---|---|---|
| Tray | resident | operating-system startup entry | visible icon/popover, daemon launch/supervision, crash-loop state, dashboard launch |
| Daemon | resident child | tray | `run_hub_runtime`, Blueprint/Adapt resident work, authenticated local snapshot service |
| Dashboard | on demand | tray launch action | existing Tauri shell & read-only Hub views; exits when window closes |

Tray is SwiftUI + AppKit on macOS & Slint + Rust on Windows. Daemon is headless Rust. Dashboard
remains Tauri. Existing `externalBin` entries for `cortex` & `membrane` remain on-demand CLI tools;
they are not resident sidecars.

`apps/membrane-hub` separates into three build outputs. No output may quietly absorb another's
ownership:

- tray owns process lifecycle & user-visible status;
- daemon owns runtime execution & local service endpoints;
- dashboard owns full-screen presentation only.

## 2. Bootstrap & IPC

Use inherited anonymous pipes for bootstrap/control & existing authenticated loopback HTTP for
runtime reads. This adds no second snapshot protocol authority.

### 2.1 Inherited channels

Tray creates three child streams before launch:

1. child stdin: tray → daemon control plus lifetime signal;
2. child stdout: daemon → tray typed events only;
3. child stderr: bounded diagnostic log only.

Frames on stdin/stdout are UTF-8 newline-delimited JSON, one frame per line, maximum 16 KiB. Invalid
UTF-8, oversize frames, unknown schema versions, unknown fields, missing sequence numbers, or a
sequence regression close the session with `daemon_protocol_invalid`. stdout never carries logs;
stderr never carries control frames.

```text
DaemonLaunchV1 {
  schemaVersion: 1,
  sequence: 1,
  kind: "launch",
  workspaceRoot: string,
  httpPort: u16,
  bearerToken: string,
  parentPid: u32
}

DaemonCommandV1 {
  schemaVersion: 1,
  sequence: u64,
  kind: "drain"
}

DaemonEventV1 {
  schemaVersion: 1,
  sequence: u64,
  kind: "ready" | "draining" | "drained" | "fatal",
  pid: u32,
  observedAtUnixMs: u64,
  endpoint?: string,
  reason?: string
}
```

Tray generates one random 256-bit bearer token per daemon generation & sends it only through
inherited stdin. Daemon does not bind until a valid `launch` frame arrives. `ready` is emitted only
after `LifecycleControl::wait_until_ready` succeeds & authenticated `GET /health` can answer. Token
is never printed, persisted, placed in process arguments/environment, or exposed to dashboard JS.

Dashboard receives endpoint + token through its own one-shot inherited bootstrap pipe. Tauri's
native backend keeps token in memory & performs existing `GET /health` / `GET /hub/snapshot` calls;
webview code receives typed results only.

### 2.2 Steady-state reads

Tray polls existing authenticated endpoints using current Hub polling/grace constants:

- `GET /health` — liveness & resident health;
- `GET /hub/snapshot` — cached/read-only dashboard & popover data.

Process-exit notification outranks polling. A terminated daemon turns icon red immediately, even if
last cached snapshot says Running. Cache may remain visible only with explicit `cached_snapshot`.

## 3. Kernel lifetime coupling

### 3.1 Windows

Use `windows-sys` with these feature groups:

- `Win32_Foundation`
- `Win32_Security`
- `Win32_Storage_FileSystem`
- `Win32_System_JobObjects`
- `Win32_System_Pipes`
- `Win32_System_Threading`

Launch sequence:

1. `CreateJobObjectW`.
2. `SetInformationJobObject(JobObjectExtendedLimitInformation)` with
   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
3. Create stdin/stdout/stderr pipes; parent ends are non-inheritable.
4. Build `STARTUPINFOEXW` with `PROC_THREAD_ATTRIBUTE_JOB_LIST` containing job handle &
   `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` containing only child stream handles.
5. `CreateProcessW` with `EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW` & handle inheritance
   enabled. Job membership is atomic at process creation; no spawn→assign race is permitted.
6. Close child-side handles in tray. Retain job handle for full tray lifetime.
7. Wait on process handle for immediate exit notification.

Closing tray's job handle kills daemon, including when tray crashes or is terminated in Task
Manager. Daemon must not use `CREATE_BREAKAWAY_FROM_JOB` or spawn a resident process outside job.

### 3.2 macOS

Swift tray launches daemon with `Process`. Set control pipe read end as daemon stdin, event pipe
write end as stdout, & diagnostic pipe write end as stderr. Tray retains control pipe write end.

Daemon treats stdin EOF as mandatory parent loss: request runtime drain immediately, then exit.
Parent clean quit sends `drain` before closing pipe. Parent crash or `SIGKILL` closes pipe in kernel,
so daemon receives EOF without parent cleanup code. Tray also registers child PID through `kqueue`
`EVFILT_PROC | NOTE_EXIT` for immediate visible failure while tray remains alive.

Daemon may not daemonize, call `setsid`, replace inherited stdin, or transfer runtime ownership.

## 4. Supervisor state machine

```text
stopped ──start──> starting ──ready──> running ──drain──> draining ──drained──> stopped
                       │                  │
                       └─fail/exit────────┴──> backoff ──restart──> starting
                                                   │
                                      third exit / 60 s
                                                   v
                                              crash_loop
```

| State | Tray verdict | User action | Required reason |
|---|---|---|---|
| starting | amber half-square · Starting | none | `daemon_starting` |
| running | green filled-square · Running | Open dashboard | live health reason |
| draining | amber half-square · Stopping | none | `daemon_draining` |
| stopped/unexpected exit | red hollow-square · Offline | Restart | child exit/fatal reason |
| backoff | red hollow-square · Restarting | Restart now | `daemon_restart_backoff` |
| crash_loop | red hollow-square · Crash loop | Restart | `daemon_crash_loop` |

Carry existing constants: three unexpected exits inside 60 seconds enter `crash_loop`; a run lasting
60 seconds clears prior exits. First two failures restart on next supervisor tick. Manual Restart
clears crash history, increments daemon generation, & attempts exactly one fresh start. Every state
transition includes generation, PID when known, timestamp, exit code/signal when known, & typed
reason. Never paraphrase reason away in popover.

Startup/handshake failures use stable reasons:

- `daemon_spawn_failed`
- `daemon_handshake_timeout`
- `daemon_protocol_invalid`
- `daemon_ready_failed`
- `daemon_exited`
- `daemon_drain_timeout`
- `daemon_crash_loop`

Clean Quit sends `drain`, waits current seven-second drain timeout, then closes job/pipe. Timeout is
surfaced as `daemon_drain_timeout`; tray still exits & kernel coupling terminates daemon.

## 5. Visible surfaces

- Tray icon exists before daemon launch begins.
- Daemon death changes icon to red from process-exit notification, not next HTTP poll.
- Popover always names current state, literal reason, last observation, & Restart when not Running.
- First launch automatically opens one native first-run surface from tray. It explains visible
  lifetime coupling, exposes Launch at login, & offers Open dashboard. Both windows may no longer
  start hidden with no first-run affordance.
- Windows startup uses per-user `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` with exact,
  quoted installed tray path; macOS keeps per-user LaunchAgent for tray bundle. Startup never points
  at daemon or dashboard directly.
- Windows tray assets ship at 16, 20, 24, & 32 px for DPI scaling. Status variants retain verdict
  shape in monochrome.

### Popover placement & dismissal

Compute against monitor work area containing tray icon. Center horizontally, then clamp to left &
right insets. Prefer below icon only when full height fits; otherwise place above. If neither fits,
choose larger side & clamp both top & bottom. This covers top, bottom, left, & right taskbars.

After tray click, suppress blur-dismiss for 500 ms. Pointer-down inside popover sets a gesture guard
until matching pointer-up/cancel; focus loss during guarded gesture cannot close window. Escape & an
unguarded outside click close immediately.

## 6. Migration order

1. Add daemon entrypoint around unchanged `run_hub_runtime` plus control/event framing.
2. Add process supervisor behind current Tauri tray tests; prove lifetime & crash-loop semantics.
3. Implement Windows Slint tray & macOS Swift/AppKit tray against same protocol fixtures.
4. Move Blueprint/Adapt ownership into daemon process.
5. Change dashboard to on-demand child & remove resident tray/webview ownership from Tauri.
6. Delete in-process `supervisor.rs` path only after both native trays pass acceptance.

Migration mode is bounded coexistence during steps 2–5: build-time platform selection chooses one
owner; no installed build may launch both resident paths. Cutover becomes hard when old in-process
imports, startup routes, runtime registrations, tests, & documentation are absent.

## 7. Acceptance

1. Tray forced termination kills/unblocks daemon without cooperative tray cleanup.
2. No daemon endpoint answers when tray process is absent.
3. Daemon forced termination changes icon red & surfaces exact reason before next snapshot poll.
4. First two fast crashes restart; third inside 60 seconds enters visible crash loop; manual Restart
   clears loop & performs one start.
5. Clean quit drains within seven seconds or surfaces timeout, then leaves no Membrane process.
6. Bottom-taskbar, top-taskbar, 100%, & 150% DPI placements stay within monitor work area.
7. Blur inside 500 ms after tray click or guarded pointer gesture does not dismiss; later outside
   click & Escape do.
8. Dashboard closes without stopping tray/daemon; tray closes only through explicit Quit.
9. Startup launches tray only; first-run surface is visible; daemon/dashboard have no startup entry.
10. Slint proof at 340 px width renders all healthy/degraded/offline/crash-loop rows, keyboard focus,
    long reasons, 100%/150% DPI, opaque `rgba(44,44,46,0.95)`-equivalent fallback, & any supported
    translucency. Failure of those plates reopens Windows toolkit choice.
