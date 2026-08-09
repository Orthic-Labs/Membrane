"""Frozen, stat-backed transcript descriptors for Taste v2."""
from __future__ import annotations

import json
import os
import hashlib
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

SUPPORTED_HOSTS = frozenset({"claude_code", "codex"})
_MAX_CWD_TRANSITIONS = 50
_MAX_METADATA_ROWS = 50


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
    """Discovery-time descriptor: all fields derive from paths & one ``stat`` call."""

    spec: SourceSpec
    path: Path
    path_rel: str
    size: int
    mtime_ns: int
    local_source_key: str
    session_id: str = ""
    metadata: SourceMetadata | None = None

    @property
    def cwd(self) -> str:
        metadata = self.metadata
        return metadata.cwd_by_row[-1][1] if metadata and metadata.cwd_by_row else ""


def _home(home: Path | None) -> Path:
    return (home or Path.home()).resolve()


def _bounded(value: int | None, default: int) -> int:
    return max(1, min(50, default if value is None else value))


def _active_codex_ids() -> set[str]:
    names = {
        "CODEX_THREAD_ID", "MORPH_ACTIVE_CODEX_THREAD_IDS",
        "MORPH_ACTIVE_CODEX_THREAD_ID", "MORPH_ACTIVE_CODEX_THREADS",
    }
    return {
        item
        for key, value in os.environ.items()
        if key in names
        for item in value.replace(",", " ").split()
        if item
    }


def _active_path(path: Path, active_ids: set[str]) -> bool:
    rendered = str(path)
    return any(identifier in rendered for identifier in active_ids)


def source_hash(path: Path) -> str:
    """Hash transcript bytes without materialising a source in memory."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def _value_at(mapping: Any, *keys: str) -> str:
    if not isinstance(mapping, dict):
        return ""
    for key in keys:
        value = mapping.get(key)
        if isinstance(value, (str, int)) and str(value):
            return str(value)
    return ""


def _has_codex_exec(value: Any) -> bool:
    if isinstance(value, str):
        return value == "codex_exec"
    if isinstance(value, dict):
        return any(key == "codex_exec" or _has_codex_exec(item) for key, item in value.items())
    if isinstance(value, list):
        return any(_has_codex_exec(item) for item in value)
    return False


def _subagent_parent(value: Any) -> bool:
    """Recognise canonical ``source.subagent.thread_spawn`` parent forms."""
    if isinstance(value, (str, int)):
        return bool(str(value))
    if not isinstance(value, dict):
        return False
    return bool(_value_at(value, "parent_id", "parent_thread_id", "parent", "thread_id", "id"))


def _codex_thread(payload: dict[str, Any], obj: dict[str, Any]) -> tuple[str, str]:
    source = payload.get("source", obj.get("source"))
    subagent = source.get("subagent") if isinstance(source, dict) else None
    if isinstance(subagent, dict) and _subagent_parent(subagent.get("thread_spawn")):
        return "subagent", "structured-subagent-parent"
    if _has_codex_exec(payload) or _has_codex_exec(obj):
        return "root", "codex-exec"
    return "root", ""


def inspect_metadata(spec: SourceSpec, path: Path, *, max_cwd_transitions: int | None = None,
                     max_rows: int | None = None) -> SourceMetadata:
    """Read only bounded binary JSONL metadata; malformed or incomplete input is excluded."""
    cwd_limit = _bounded(max_cwd_transitions, _MAX_CWD_TRANSITIONS)
    row_limit = _bounded(max_rows, _MAX_METADATA_ROWS)
    session_id, cwd_rows, thread, reason = "", [], "root", ""
    conversational_row = False
    try:
        with path.open("rb") as handle:
            for row, raw in enumerate(handle, 1):
                try:
                    obj = json.loads(raw)
                except (UnicodeDecodeError, json.JSONDecodeError):
                    continue
                if not isinstance(obj, dict):
                    continue
                payload = obj.get("payload") if isinstance(obj.get("payload"), dict) else {}
                if spec.host == "claude_code":
                    session_id = session_id or _value_at(obj, "sessionId", "session_id")
                    cwd = _value_at(obj, "cwd")
                    if cwd and (not cwd_rows or cwd_rows[-1][1] != cwd) and len(cwd_rows) < cwd_limit:
                        cwd_rows.append((row, cwd))
                    if obj.get("isSidechain") or obj.get("is_sidechain"):
                        thread = "sidechain"
                    if len(cwd_rows) >= cwd_limit or row >= row_limit:
                        break
                    continue
                session_id = session_id or _value_at(payload, "session_id", "id") or _value_at(obj, "session_id")
                cwd = _value_at(payload, "cwd") or _value_at(obj, "cwd")
                if cwd and (not cwd_rows or cwd_rows[-1][1] != cwd) and len(cwd_rows) < cwd_limit:
                    cwd_rows.append((row, cwd))
                parsed_thread, parsed_reason = _codex_thread(payload, obj)
                if parsed_reason:
                    thread, reason = parsed_thread, parsed_reason
                conversational_row = conversational_row or obj.get("type") in {"response_item", "event_msg"}
                # Codex headers carry identity/cwd; first conversation row settles thread origin.
                if session_id and conversational_row:
                    break
                if row >= row_limit:
                    break
    except OSError:
        return SourceMetadata("", exclusion_reason="metadata-unreadable")
    if not session_id:
        return SourceMetadata("", tuple(cwd_rows), thread, "metadata-incomplete")
    if spec.host == "claude_code" and thread == "sidechain":
        cwd_rows = []
    return SourceMetadata(session_id, tuple(cwd_rows), thread, reason)


def discover(home: Path | None = None) -> list[TranscriptSource]:
    """Traverse candidate paths only; never open, hash, or parse transcript content."""
    base, seen, result = _home(home), set(), []
    for spec in SPECS:
        root = (base / spec.root).resolve()
        for pattern in spec.patterns:
            for path in sorted(root.glob(pattern)):
                try:
                    resolved = path.resolve()
                    path_rel = resolved.relative_to(base).as_posix()
                    stat = resolved.stat()
                except (OSError, ValueError):
                    continue
                if not resolved.is_file() or resolved in seen or "checkpoint" in resolved.parts:
                    continue
                seen.add(resolved)
                result.append(TranscriptSource(
                    spec, resolved, path_rel, stat.st_size, stat.st_mtime_ns,
                    f"{spec.host or spec.tool}:{path_rel}",
                ))
    return result


def select_sources(sources: Iterable[TranscriptSource], *, learned: dict[str, str] | None = None,
                   active_ids: set[str] | None = None, limit: int | None = None,
                   installation_id: str | None = None) -> tuple[list[TranscriptSource], list[TranscriptSource]]:
    """Lazily enrich supported descriptors; excluded/quarantined sources never consume limit."""
    learned = learned or {}
    active_ids = _active_codex_ids() if active_ids is None else active_ids
    selected, quarantined = [], []
    for source in sources:
        if not source.spec.supported or source.spec.host not in SUPPORTED_HOSTS:
            quarantined.append(_with_metadata(source, SourceMetadata("", exclusion_reason="unsupported-host")))
            continue
        if source.spec.host == "codex" and _active_path(source.path, active_ids):
            quarantined.append(_with_metadata(source, SourceMetadata("", exclusion_reason="active-session")))
            continue
        learned_id = source.local_source_key
        if installation_id:
            import cross_machine
            learned_id = cross_machine.qualify_source_session(
                installation_id, source.spec.tool, source.local_source_key,
            )
        if learned_id in learned or source.local_source_key in learned:
            continue
        if len(selected) >= _bounded(limit, 50):
            continue
        metadata = inspect_metadata(source.spec, source.path)
        enriched = _with_metadata(source, metadata)
        if metadata.exclusion_reason == "metadata-unreadable":
            raise RuntimeError(f"supported transcript metadata unreadable: {source.path}")
        if metadata.exclusion_reason:
            quarantined.append(enriched)
            continue
        selected.append(enriched)
    return selected, quarantined


def _with_metadata(source: TranscriptSource, metadata: SourceMetadata) -> TranscriptSource:
    return TranscriptSource(source.spec, source.path, source.path_rel, source.size, source.mtime_ns,
                            source.local_source_key, metadata.session_id, metadata)
