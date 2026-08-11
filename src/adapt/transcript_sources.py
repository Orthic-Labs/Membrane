"""Frozen, stat-backed transcript descriptors for Taste v2."""
from __future__ import annotations

import json
import os
import hashlib
import re
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable

SUPPORTED_HOSTS = frozenset({"claude_code", "codex"})
_MAX_CWD_TRANSITIONS = 50
_MAX_METADATA_ROWS = 50
PROVENANCE_KINDS = frozenset({"external_user", "assistant", "developer", "internal_context",
                              "tool_result", "subagent"})
_INTERNAL_PREFIXES = ("<codex_internal_context", "<recommended_plugins", "<system-reminder",
                      "<command-name", "<environment_context", "<permissions", "<app-context",
                      "<skills_instructions", "# agents.md instructions for ")


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


@dataclass(frozen=True)
class CanonicalizationStats:
    raw_rows: int
    canonical_messages: int
    eligible_user_turns: int
    dropped_reasons: dict[str, int]
    authority_ineligible_reasons: dict[str, int]
    deduplicated_count: int

    def as_dict(self) -> dict[str, Any]:
        return {"rawRows": self.raw_rows, "canonicalMessages": self.canonical_messages,
                "eligibleUserTurns": self.eligible_user_turns,
                "droppedReasons": dict(sorted(self.dropped_reasons.items())),
                "authorityIneligibleReasons": dict(sorted(self.authority_ineligible_reasons.items())),
                "deduplicatedCount": self.deduplicated_count}


def _internal_context(text: str) -> bool:
    return text.strip().casefold().startswith(_INTERNAL_PREFIXES)


def event_provenance(event: dict[str, Any]) -> str:
    """Classify one consumer-side event without changing frozen Orthic code."""
    thread_source = str(event.get("threadSource") or event.get("thread_source") or "").casefold()
    flags = event.get("flags") if isinstance(event.get("flags"), dict) else {}
    if thread_source in {"subagent", "sidechain"} or flags.get("isSidechain"):
        return "subagent"
    existing = event.get("provenance")
    if existing in PROVENANCE_KINDS:
        return str(existing)
    kind = str(event.get("kind") or "").casefold()
    role = str(event.get("role") or "").casefold()
    text = str(event.get("text") or "")
    if role == "developer" or kind == "developer_message": return "developer"
    if kind in {"tool_call", "tool_result"}: return "tool_result"
    if _internal_context(text) or flags.get("meta") or event.get("meta"): return "internal_context"
    if kind == "user_message" and role in {"", "user"}: return "external_user"
    if kind in {"assistant_message", "agent_message"} or role in {"assistant", "agent"}: return "assistant"
    return "internal_context"


def _row_refs(event: dict[str, Any]) -> list[dict[str, Any]]:
    existing = event.get("sourceRows")
    if isinstance(existing, list) and all(isinstance(row, dict) for row in existing):
        return [dict(row) for row in existing]
    return [{"eventId": str(event.get("eventId") or ""), "rowIndex": int(event.get("rowIndex") or 0),
             "byteStart": int(event.get("byteStart") or 0), "byteEnd": int(event.get("byteEnd") or 0),
             "projection": str(event.get("projection") or "default")}]


def _time_value(value: Any) -> float | None:
    if not isinstance(value, str) or not value: return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()
    except ValueError:
        return None


def _adjacent_mirror(left: dict[str, Any], right: dict[str, Any]) -> bool:
    provenance = event_provenance(right)
    if provenance not in {"assistant", "developer", "internal_context", "subagent"}: return False
    if event_provenance(left) != provenance: return False
    left_row, right_row = int(left.get("rowIndex") or 0), int(right.get("rowIndex") or 0)
    if left_row > 0 and right_row > 0 and right_row - left_row not in {0, 1}: return False
    left_text = re.sub(r"\s+", " ", str(left.get("text") or "").strip()).casefold()
    right_text = re.sub(r"\s+", " ", str(right.get("text") or "").strip()).casefold()
    if not left_text or hashlib.sha256(left_text.encode()).digest() != hashlib.sha256(right_text.encode()).digest(): return False
    left_time, right_time = _time_value(left.get("timestamp")), _time_value(right.get("timestamp"))
    return (left_time is None and right_time is None) or (left_time is not None and right_time is not None
                                                          and abs(right_time - left_time) <= 2)


def canonicalize_events(events: Iterable[dict[str, Any]]) -> tuple[list[dict[str, Any]], CanonicalizationStats]:
    """Attach provenance, quarantine non-evidence, & collapse mirrored projections."""
    raw = [dict(event) for event in events if isinstance(event, dict)]
    explicit_raw = max((int(event.get("rawRowCount") or 0) for event in raw), default=0)
    row_indexes = {int(event.get("rowIndex") or 0) for event in raw if int(event.get("rowIndex") or 0) > 0}
    raw_rows = explicit_raw or len(row_indexes) or len(raw)
    canonical: list[dict[str, Any]] = []
    by_stable_id: dict[str, int] = {}
    dropped: dict[str, int] = {}
    authority_ineligible: dict[str, int] = {}
    deduplicated = 0
    for event in raw:
        provenance = event_provenance(event)
        event["provenance"] = provenance
        event["authorityEligible"] = provenance == "external_user"
        event["evidenceEligible"] = event.get("evidenceEligible") is not False and (
            provenance not in {"developer", "internal_context", "subagent"}
            or (provenance == "subagent" and event.get("kind") in {"assistant_message", "agent_message"})
        )
        event["sourceRows"] = _row_refs(event)
        if not event["evidenceEligible"]: dropped[provenance] = dropped.get(provenance, 0) + 1
        if not event["authorityEligible"]: authority_ineligible[provenance] = authority_ineligible.get(provenance, 0) + 1
        event_id = str(event.get("eventId") or "")
        duplicate_index = by_stable_id.get(event_id) if event_id else None
        if duplicate_index is not None:
            prior = canonical[duplicate_index]
            if event_provenance(prior) == provenance and str(prior.get("text") or "") == str(event.get("text") or ""):
                prior["sourceRows"].extend(row for row in event["sourceRows"] if row not in prior["sourceRows"])
                deduplicated += 1
                continue
        if canonical and _adjacent_mirror(canonical[-1], event):
            canonical[-1]["sourceRows"].extend(row for row in event["sourceRows"] if row not in canonical[-1]["sourceRows"])
            deduplicated += 1
            continue
        by_stable_id[event_id] = len(canonical)
        canonical.append(event)
    messages = sum(1 for event in canonical if str(event.get("kind") or "").endswith("_message"))
    eligible = sum(1 for event in canonical if event.get("authorityEligible") is True)
    return canonical, CanonicalizationStats(raw_rows, messages, eligible, dropped, authority_ineligible, deduplicated)

def _home(home: Path | None) -> Path:
    return (home or Path.home()).resolve()


def _bounded(value: int | None, default: int) -> int:
    return max(1, min(50, default if value is None else value))


def _active_codex_ids() -> set[str]:
    names = {
        "CODEX_THREAD_ID", "ADAPT_ACTIVE_CODEX_THREAD_IDS",
        "ADAPT_ACTIVE_CODEX_THREAD_ID", "ADAPT_ACTIVE_CODEX_THREADS",
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
            from adapt import cross_machine
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
