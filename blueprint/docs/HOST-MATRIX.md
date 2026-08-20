# Cortex host matrix

| Host | Instruction injection | Interception capability |
| --- | --- | --- |
| Claude Code | `.mcp.json` + `CLAUDE.md` | Optional `.claude/settings.json` `PreToolUse` hook for `Read`, `Grep`, `Glob` |
| Codex | `AGENTS.md` | Instruction only; no hook claimed |
| Cursor | `.cursor/rules/cortex.mdc` | Instruction only; no hook claimed |
| Generic | `CORTEX-AGENT.md` | Instruction only; no hook claimed |

Every host receives the same fenced orient-before-read block. Only Claude Code's optional redirect installs a tool interception hook; other hosts are not represented as intercepted.
