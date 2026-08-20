# Installing Blueprint

`blueprint init` enrolls Blueprint into a repository and, optionally, into the
agents that work in it. Everything it does is reversible with
`blueprint uninstall`; the plan is visible before anything is written via
`--dry-run --json`.

## Flags

| Flag | Values | Default | Meaning |
|---|---|---|---|
| `--host` | `auto\|claude-code\|codex\|cursor\|generic` | `auto` | Which agent host to install instructions/MCP for. |
| `--scope` | `project\|user` | `project` | Project-scoped (this repo) or user-scoped. |
| `--mcp` | `auto\|on\|off` | `auto` | Install the MCP server entry. `auto` enables for Claude Code. |
| `--watch` | `auto\|on\|off` | `auto` | Enroll the resident watcher. `auto` enables for project scope. |
| `--hooks` | `none\|git\|host\|all` | `none` | Install git/host hooks. |
| `--policy` | `advisory\|recall-before-read\|task-grants` | `advisory` | Hook policy mode. |
| `--dry-run` | flag | off | Print the plan without writing. |
| `--yes` | flag | off | Skip confirmation. |
| `--json` | flag | off | Machine-readable output. |

## Deterministic host detection

1. Explicit `--host` wins.
2. Otherwise, existing host config files (`.claude/settings.json`,
   `CLAUDE.md`, `.mcp.json`, `AGENTS.md`, `.cursor/rules/blueprint.mdc`, …).
3. Otherwise, installed command probes (`claude`, `codex`, `cursor`).
4. Otherwise, `generic`.

Never more than the selected host set is installed.

## What apply does

- Writes a managed, reversible instruction block into each host's
  instruction file (`CLAUDE.md`, `AGENTS.md`, `.cursor/rules/blueprint.mdc`,
  or `BLUEPRINT-AGENT.md`).
- Optionally merges a `blueprint` entry into `.mcp.json`.
- Optionally enrolls the resident watcher.
- Optionally installs hooks.
- Builds the generation (`.agent/`), runs `status`, runs one recall query,
  and prints `blueprint uninstall` plus `blueprint doctor --repair-plan`.

## Reversibility

`blueprint uninstall` restores every touched host file byte-for-byte using the
installer state captured at apply time. Repository graph data in `.agent/`
is preserved unless `--purge-data` is explicit.
