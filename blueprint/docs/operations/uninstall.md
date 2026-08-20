# Uninstall

Uninstall is data-preserving by default: repository graph data under
`.agent/` and per-repository init state are removed only where noted below.

## Init state (`blueprint init` / `scripts/blueprint-install.mjs --uninstall`)

`scripts/blueprint-install.mjs` (and `lib/init/apply.mjs` `uninstallInit`) track
every file they modified in an install-state record
(`.agent/graph/blueprint-install-state.json`). Uninstall:

- Restores each managed file to its pre-install content, or deletes it if it
  did not exist before install. Managed files are limited to:
  `CLAUDE.md`, `AGENTS.md`, `BLUEPRINT-AGENT.md`, `.mcp.json`,
  `.cursor/rules/blueprint.mdc`, `.claude/settings.json`, and the
  `post-checkout`/`post-merge`/`post-rewrite` git hooks.
- Refuses to restore (`state_conflict`) if a managed file was modified
  outside Blueprint since install, so uninstall never silently discards
  unrelated edits.
- Removes recall session markers under `.agent/graph/`.
- Removes the install-state record itself.

Repository graph data (`.agent/graph/graph.db` and related store files) is
**not** touched by init uninstall.

## Watcher service (`service/uninstall.mjs`)

| Platform | Service mechanism removed | Registration file |
|---|---|---|
| macOS | `launchctl unload` | `~/Library/LaunchAgents/io.membrane.blueprint.plist` |
| Linux | `systemctl --user disable --now blueprint.service` | `~/.config/systemd/user/blueprint.service` |

The service registration file/target is always removed. Repository data
(`.agent/`) is preserved unless `--purge-data` is passed explicitly, in
which case the data directory is also removed.

## Portable/native install removal

- **macOS** (`release/macos/uninstall.sh`): removes
  `${INSTALL_LOCATION:-/usr/local/lib/blueprint}`. User data under `~/.blueprint`
  is explicitly preserved.
## npm/pnpm and Homebrew installs

Uninstall follows the owning package manager (`npm uninstall -g`,
`brew uninstall blueprint`); Blueprint never
self-removes files outside its own package footprint for these owners.

## What is preserved by default

- Repository graph data (`.agent/graph/graph.db`, backups) unless
  `--purge-data` is passed to the service uninstall.
- Any file outside the fixed managed-file allowlist above.
- `~/.blueprint` user data on the macOS portable/native uninstall path.
