# Membrane macOS tray

Native SwiftUI + AppKit menu-bar owner for Architecture B. It starts `membrane-daemon` through inherited stdin/stdout/stderr pipes, closes control stdin on shutdown (daemon EOF lifetime enforcement), watches child exit through `kqueue` `NOTE_EXIT`, & launches dashboard on demand.

Build on macOS:

```sh
swift test
swift build -c release
```

Set `MEMBRANE_DAEMON_PATH`, `MEMBRANE_DASHBOARD_PATH`, & `MEMBRANE_WORKSPACE_ROOT` when running an unpackaged build. A signed app bundle should place daemon beside tray executable & provide `LSUIElement=true` from `Sources/MembraneTrayMacOS/Info.plist`.
