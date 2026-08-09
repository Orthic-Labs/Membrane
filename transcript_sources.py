"""Frozen direct transcript discovery & source-local metadata for Taste v2."""
from __future__ import annotations

import json
import os
from dataclasses import dataclass
from pathlib import Path

SUPPORTED_HOSTS = frozenset({"claude_code", "codex"})

@dataclass(frozen=True)
class SourceSpec:
    tool: str
    host: str | None
    root: str
    patterns: tuple[str, ...]
    supported: bool

SPECS = (
    SourceSpec("claude-code", "claude_code", ".claude/projects", ("*/*.jsonl",), True),
    SourceSpec("codex", "codex", ".codex/sessions", ("*/*/*/*.jsonl",), True),
    SourceSpec("roo", None, "AppData/Roaming/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks", ("*/api_conversation_history.json",), False),
    SourceSpec("roo", None, "Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks", ("*/api_conversation_history.json",), False),
    SourceSpec("cursor", None, "Library/Application Support/Cursor/User/workspaceStorage", ("*/*.jsonl",), False),
    SourceSpec("command-code", None, ".commandcode/projects", ("*/*.jsonl",), False),
    SourceSpec("cline", None, ".cline/data/sessions", ("*/*.messages.json",), False),
    SourceSpec("gemini", None, ".gemini/tmp", ("*/chats/session-*.json", "*/chats/session-*.jsonl"), False),
    SourceSpec("grok", None, ".grok/sessions", ("*/*/chat_history.jsonl",), False),
)

@dataclass(frozen=True)
class SourceMetadata:
    session_id: str
    cwd_by_row: tuple[tuple[int, str], ...] = ()
    thread_source: str = "root"
    exclusion_reason: str = ""

@dataclass(frozen=True)
class TranscriptSource:
    spec: SourceSpec
    path: Path
    session_id: str
    metadata: SourceMetadata
    @property
    def cwd(self) -> str:
        return self.metadata.cwd_by_row[-1][1] if self.metadata.cwd_by_row else ""

def _home(home: Path | None) -> Path: return (home or Path.home()).resolve()

def _active_codex_ids() -> set[str]:
    return {value for key, value in os.environ.items() if key in {"CODEX_THREAD_ID", "MORPH_ACTIVE_CODEX_THREAD_ID", "MORPH_ACTIVE_CODEX_THREADS"} for value in value.replace(",", " ").split()}

def _metadata(spec: SourceSpec, path: Path) -> SourceMetadata:
    session_id, cwd_rows, thread, reason = path.parent.name, [], "root", ""
    try:
        for row, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            try: obj = json.loads(raw)
            except json.JSONDecodeError: continue
            if not isinstance(obj, dict): continue
            if spec.host == "claude_code":
                session_id = str(obj.get("sessionId") or obj.get("session_id") or session_id)
                cwd = obj.get("cwd")
                if isinstance(cwd, str) and cwd: cwd_rows.append((row, cwd))
                if obj.get("isSidechain") or obj.get("is_sidechain"): thread = "sidechain"
            else:
                payload = obj.get("payload") if isinstance(obj.get("payload"), dict) else {}
                session_id = str(payload.get("session_id") or payload.get("id") or obj.get("session_id") or session_id)
                cwd = payload.get("cwd") or obj.get("cwd")
                if isinstance(cwd, str) and cwd: cwd_rows.append((row, cwd))
                text = json.dumps(payload, sort_keys=True)
                if "codex_exec" in text or '"source":"exec"' in text: reason = "codex-exec"
                if "subagent" in text and (payload.get("parent_id") or payload.get("parent_thread_id")): reason = "structured-subagent-parent"; thread = "subagent"
    except (OSError, UnicodeDecodeError): reason = "metadata-unreadable"
    if spec.host == "claude_code" and thread == "sidechain":
        # Sidechains never provide durable user authority; retain the latest root cwd only.
        cwd_rows = []
    if spec.host == "codex" and session_id in _active_codex_ids(): reason = "active-session"
    return SourceMetadata(session_id, tuple(cwd_rows), thread, reason)

def discover(home: Path | None = None) -> list[TranscriptSource]:
    base, seen, result = _home(home), set(), []
    for spec in SPECS:
        root = (base / spec.root).resolve()
        if not root.is_dir(): continue
        for pattern in spec.patterns:
            for path in sorted(root.glob(pattern)):
                try: resolved = path.resolve(); resolved.relative_to(root)
                except (OSError, ValueError): continue
                if not resolved.is_file() or resolved in seen or "checkpoint" in resolved.parts: continue
                seen.add(resolved)
                active = _active_codex_ids() if spec.host == "codex" else set()
                if active and any(value in str(resolved) for value in active):
                    metadata = SourceMetadata(resolved.parent.name, exclusion_reason="active-session")
                else:
                    metadata = _metadata(spec, resolved)
                result.append(TranscriptSource(spec, resolved, metadata.session_id, metadata))
    return result
