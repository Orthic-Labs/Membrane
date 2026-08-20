# Resident watcher service

The resident watcher keeps freshness barriers warm across enrolled
repositories. `cortex service` installs it as a user-scoped operating-system
service:

| OS | Service | Target |
|---|---|---|
| macOS | LaunchAgent | `~/Library/LaunchAgents/io.orthic.cortex.plist` |
| Linux | systemd `--user` | `~/.config/systemd/user/cortex.service` |
| Windows | per-user scheduled task | `%LOCALAPPDATA%\Orthic\Cortex\cortex-task.xml` |

## Commands

```sh
cortex service install            # register + start at login
cortex service status             # registered? running?
cortex service start|stop|restart
cortex service logs               # last 40 lines of service log
cortex service uninstall          # removes registration, preserves graph data
cortex service uninstall --purge-data   # also removes .agent graph data
```

All commands support `--json`.

## Design

- The service runs `cortex-watch start` in **foreground** mode; the OS
  service manager owns restart. Cortex never double-daemonizes.
- Install is opt-in from `cortex init` (`--watch auto|on|off`).
- Uninstall preserves repository graph data unless `--purge-data` is
  explicit.
- One repository failure degrades that repo only; it never stops the other
  actors.
- No visible Windows console is left open (the task runs PowerShell hidden).

## Verification

```sh
cortex service install
cortex service status
cortex-watch status
```
