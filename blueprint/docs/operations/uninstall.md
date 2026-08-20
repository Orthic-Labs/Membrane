# Uninstall

Uninstall is data-preserving by default: repository graph data under
`.agent/` and per-repository init state are removed only where noted below.

## Init state (`cortex init` / `scripts/cortex-install.mjs --uninstall`)

`scripts/cortex-install.mjs` (and `lib/init/apply.mjs` `uninstallInit`) track
every file they modified in an install-state record
(`.agent/graph/cortex-install-state.json`). Uninstall:

- Restores each managed file to its pre-install content, or deletes it if it
  did not exist before install. Managed files are limited to:
  `CLAUDE.md`, `AGENTS.md`, `CORTEX-AGENT.md`, `.mcp.json`,
  `.cursor/rules/cortex.mdc`, `.claude/settings.json`, and the
  `post-checkout`/`post-merge`/`post-rewrite` git hooks (plus `.cmd`
  variants on Windows).
- Refuses to restore (`state_conflict`) if a managed file was modified
  outside Cortex since install, so uninstall never silently discards
  unrelated edits.
- Removes orientation session markers under `.agent/graph/`.
- Removes the install-state record itself.

Repository graph data (`.agent/graph/graph.db` and related store files) is
**not** touched by init uninstall.

## Watcher service (`service/uninstall.mjs`)

| Platform | Service mechanism removed | Registration file |
|---|---|---|
| macOS | `launchctl unload` | `~/Library/LaunchAgents/io.orthic.cortex.plist` |
| Linux | `systemctl --user disable --now cortex.service` | `~/.config/systemd/user/cortex.service` |
| Windows | `schtasks /Delete /F /TN OrthicCortex` | scheduled task `OrthicCortex` |

The service registration file/target is always removed. Repository data
(`.agent/`) is preserved unless `--purge-data` is passed explicitly, in
which case the data directory is also removed.

## Portable/native install removal

- **macOS** (`release/macos/uninstall.sh`): removes
  `${INSTALL_LOCATION:-/usr/local/lib/cortex}`. User data under `~/.cortex`
  is explicitly preserved.
- **Windows** (per-user installer under `%LOCALAPPDATA%\Orthic\Cortex`,
  checked by `release/windows/uninstall-check.ps1`): removes the install
  directory, the user `PATH` entry pointing at it, the `OrthicCortex`
  scheduled task (if present), and the `HKCU:\Software\Orthic\Cortex`
  registry key.

## npm/pnpm and Homebrew/WinGet installs

Uninstall follows the owning package manager (`npm uninstall -g`,
`brew uninstall cortex`, `winget uninstall OrthicLabs.Cortex`); Cortex never
self-removes files outside its own package footprint for these owners.

## What is preserved by default

- Repository graph data (`.agent/graph/graph.db`, backups) unless
  `--purge-data` is passed to the service uninstall.
- Any file outside the fixed managed-file allowlist above.
- `~/.cortex` user data on the macOS portable/native uninstall path.
