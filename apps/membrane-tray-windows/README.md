# Membrane Windows tray

Native Slint tray surface for Architecture B. It owns no webview and keeps
daemon lifetime under a Windows Job Object.

Implemented in this bounded lane:

- 340px Slint popover with system UI typography, literal reasons, verdict shapes, restart,
  dashboard, first-launch copy, and live admission rows (`admitted`, `withheld`, `budget`,
  `observed`). Missing snapshot data stays explicitly `Unknown`.
- Pure supervisor state machine with 3-exits-in-60-seconds crash-loop behavior.
- Work-area-aware placement for top, bottom, left, and right taskbars plus 500ms blur/gesture guard.
- Typed daemon event decoder using `membrane-protocol` path dependency.
- Windows `CreateProcessW` launch with inherited pipes, `STARTUPINFOEXW`, atomic Job Object
  membership, kill-on-job-close, and no token in command line/environment.
- Per-user HKCU Run quoting helper.

`--demo=healthy`, `--demo=offline`, and `--demo=crash-loop` are explicit QA-only fixture modes;
normal launch always owns and supervises the daemon.

Build with `rightkit cargo check --manifest-path apps/membrane-tray-windows/Cargo.toml`.
