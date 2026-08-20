# Resident watcher service

The resident watcher keeps freshness barriers warm across enrolled
repositories. `blueprint service` installs it as a user-scoped operating-system
service:

| OS | Service | Target |
|---|---|---|
| macOS | LaunchAgent | `~/Library/LaunchAgents/io.membrane.blueprint.plist` |
| Linux | systemd `--user` | `~/.config/systemd/user/blueprint.service` |
| Windows | per-user scheduled task | `%LOCALAPPDATA%\Membrane\Blueprint\blueprint-task.xml` |

## Commands

```sh
blueprint service install            # register + start at login
blueprint service status             # registered? running?
blueprint service start|stop|restart
blueprint service logs               # last 40 lines of service log
blueprint service uninstall          # removes registration, preserves graph data
blueprint service uninstall --purge-data   # also removes .agent graph data
```

All commands support `--json`.

## Design

- The service runs `blueprint-watch start` in **foreground** mode; the OS
  service manager owns restart. Blueprint never double-daemonizes.
- Install is opt-in from `blueprint init` (`--watch auto|on|off`).
- Uninstall preserves repository graph data unless `--purge-data` is
  explicit.
- One repository failure degrades that repo only; it never stops the other
  actors.
- No visible Windows console is left open (the task runs PowerShell hidden).

## Verification

```sh
blueprint service install
blueprint service status
blueprint-watch status
```
