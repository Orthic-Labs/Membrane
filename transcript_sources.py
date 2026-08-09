"""Frozen, direct transcript discovery for Taste v2 production runs."""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

SUPPORTED_HOSTS = frozenset({"claude_code", "codex"})

@dataclass(frozen=True)
class SourceSpec:
    name: str
    host: str
    root: str
    patterns: tuple[str, ...]
    supported: bool

SPECS = (
    SourceSpec("claude-code", "claude_code", ".claude/projects", ("*/*.jsonl",), True),
    SourceSpec("codex", "codex", ".codex/sessions", ("*/*/*.jsonl",), True),
    SourceSpec("command-code", "unsupported", ".commandcode/projects", ("*/*.jsonl",), False),
    SourceSpec("cline", "unsupported", ".cline/data/sessions", ("*/*.messages.json",), False),
    SourceSpec("gemini", "unsupported", ".gemini/tmp", ("*/chats/session-*.json", "*/chats/session-*.jsonl"), False),
    SourceSpec("grok", "unsupported", ".grok/sessions", ("*/*/chat_history.jsonl",), False),
    SourceSpec("roo-win", "unsupported", "AppData/Roaming/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks", ("*/api_conversation_history.json",), False),
    SourceSpec("roo-mac", "unsupported", "Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks", ("*/api_conversation_history.json",), False),
)

@dataclass(frozen=True)
class TranscriptSource:
    spec: SourceSpec
    path: Path
    session_id: str
    cwd: str = ""

def _home(home: Path | None) -> Path:
    return (home or Path.home()).resolve()

def discover(home: Path | None = None) -> list[TranscriptSource]:
    """Return deterministic, deduped paths under frozen per-host roots only."""
    base = _home(home)
    seen: set[Path] = set(); result: list[TranscriptSource] = []
    for spec in SPECS:
        root = (base / spec.root).resolve()
        if not root.is_dir():
            continue
        for pattern in spec.patterns:
            for path in sorted(root.glob(pattern)):
                try: resolved = path.resolve(); resolved.relative_to(root)
                except (OSError, ValueError): continue
                if not resolved.is_file() or resolved in seen or "checkpoint" in resolved.parts:
                    continue
                seen.add(resolved)
                result.append(TranscriptSource(spec, resolved, resolved.parent.name))
    return result
