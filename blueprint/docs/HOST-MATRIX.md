# Blueprint host matrix

| Host | Instruction injection | Interception capability |
| --- | --- | --- |
| Claude Code | `.mcp.json` + `CLAUDE.md` | Optional `.claude/settings.json` `PreToolUse` hook for `Read`, `Grep`, `Glob` |
| Codex | `AGENTS.md` | Instruction only; no hook claimed |
| Cursor | `.cursor/rules/blueprint.mdc` | Instruction only; no hook claimed |
| Generic | `BLUEPRINT-AGENT.md` | Instruction only; no hook claimed |

Every host receives the same fenced recall-before-read block. Only Claude Code's optional redirect installs a tool interception hook; other hosts are not represented as intercepted.
