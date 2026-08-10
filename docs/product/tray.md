# Cortex tray

Status: implemented and tested on the Tauri build contract; not yet published.

Tray is a separate desktop surface. It starts and stops Cortex service,
opens authenticated loopback explorer, reports freshness, and never writes
graph storage directly.

The tray owns only its explorer child process. Quit never stops a watcher that
was already managed by the operating system. Session tokens stay in memory and
never enter logs, disk state, or child-process arguments.
