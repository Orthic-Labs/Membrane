"""Freeze every local open coding transcript into canonical event snapshots.

Snapshots are immutable JSONL projections: raw host stores remain read-only,
private reasoning is omitted, secret-like text is redacted by TranscriptEventV1,
& every snapshot binds its source bytes or OpenCode session rows by SHA-256.
"""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import sqlite3
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Iterator

from adapt import adapt_sessions
from continuity.transcript import compact_text, parse_source_events


@dataclass(frozen=True)
class NativeSource:
    host: str
    tool: str
    path: Path
    source_key: str
    session_id: str = ""
    cwd: str = ""
    selector: str = ""


def load_frozen_sources(manifest_path: Path):
    """Load hash-verified snapshots as direct-transcript source descriptors."""
    from adapt import transcript_sources

    body = json.loads(manifest_path.read_text(encoding="utf-8"))
    rows = body.get("sources", body.get("snapshots"))
    if not isinstance(rows, list):
        raise ValueError("snapshot manifest has no sources array")
    sources = []
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError("snapshot manifest contains a non-object row")
        path = Path(str(row.get("snapshot_path") or ""))
        expected = str(row.get("snapshot_sha256") or "")
        if not path.is_file() or _hash_file(path) != expected:
            raise ValueError(f"snapshot hash mismatch: {path}")
        host = str(row.get("host") or "")
        tool = str(row.get("tool") or "")
        if host not in transcript_sources.SUPPORTED_HOSTS or not tool:
            raise ValueError(f"unsupported frozen transcript host: {host or '<missing>'}")
        stat = path.stat()
        spec = transcript_sources.SourceSpec(tool, host, "", (), True)
        first = {}
        with path.open(encoding="utf-8") as handle:
            for line in handle:
                value = json.loads(line)
                if isinstance(value, dict):
                    first = value
                    break
        session_id = str(first.get("sessionId") or row.get("session_id") or "")
        if not session_id:
            raise ValueError(f"snapshot metadata invalid: {path}: metadata-incomplete")
        cwd = str(first.get("cwd") or row.get("cwd") or "")
        metadata = transcript_sources.SourceMetadata(
            session_id=session_id,
            cwd_by_row=((1, cwd),) if cwd else (),
            thread_source=str(first.get("threadSource") or "root"),
        )
        source_key = str(row.get("source_key_sha256") or expected)
        sources.append(transcript_sources.TranscriptSource(
            spec=spec,
            path=path,
            path_rel=path.name,
            size=stat.st_size,
            mtime_ns=stat.st_mtime_ns,
            local_source_key=f"frozen:{host}:{source_key.removeprefix('sha256:')}",
            session_id=metadata.session_id,
            metadata=metadata,
        ))
    return sources


def _hash_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def _safe_id(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()[:24]


def _glob(root: Path, pattern: str) -> list[Path]:
    return sorted(path for path in root.glob(pattern) if path.is_file()) if root.exists() else []


def discover_native_transcripts(
    home: Path | None = None,
) -> tuple[list[NativeSource], dict[str, int]]:
    base = (home or Path.home()).resolve()
    sources: list[NativeSource] = []

    for path in _glob(base / ".claude/projects", "*/*.jsonl"):
        sources.append(NativeSource("claude_code", "claude-code", path, f"claude-code:{path}"))
    for path in _glob(base / ".codex/sessions", "*/*/*/*.jsonl"):
        sources.append(NativeSource("codex", "codex", path, f"codex:{path}"))
    for path in _glob(base / ".commandcode/projects", "*/*.jsonl"):
        if not path.name.endswith(".checkpoints.jsonl"):
            sources.append(NativeSource("command_code", "command-code", path, f"command-code:{path}"))
    for path in _glob(base / ".cline/data/sessions", "*/*.messages.json"):
        if path.name == f"{path.parent.name}.messages.json":
            sources.append(NativeSource("cline", "cline", path, f"cline:{path}"))
    for path in _glob(base / ".pi/agent/sessions", "*/*.jsonl"):
        sources.append(NativeSource("pi", "pi", path, f"pi:{path}"))
    for path in _glob(base / ".gemini/tmp", "*/chats/session-*.json*"):
        if path.suffix in {".json", ".jsonl"}:
            sources.append(NativeSource("gemini", "gemini", path, f"gemini:{path}"))
    for path in _glob(base / ".grok/sessions", "*/*/chat_history.jsonl"):
        sources.append(NativeSource("grok_build", "grok-build", path, f"grok-build:{path}"))
    roo_roots = (
        base / "AppData/Roaming/Cursor/User/globalStorage/rooveterinaryinc.roo-cline/tasks",
        base / "Library/Application Support/Cursor/User/globalStorage/rooveterinaryinc.roo-cline/tasks",
        base / "Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks",
    )
    for root in roo_roots:
        for path in _glob(root, "*/api_conversation_history.json"):
            sources.append(NativeSource("roo_cline", "roo-cline", path, f"roo-cline:{path}"))

    opencode_db = base / ".local/share/opencode/opencode.db"
    open_count = 0
    if opencode_db.is_file():
        uri = f"file:{opencode_db}?mode=ro"
        with sqlite3.connect(uri, uri=True, timeout=30) as database:
            rows = database.execute(
                "SELECT id, directory FROM session WHERE time_archived IS NULL ORDER BY time_created, id"
            ).fetchall()
        for session_id, cwd in rows:
            sources.append(NativeSource(
                "opencode", "opencode", opencode_db,
                f"opencode:{session_id}", str(session_id), str(cwd or ""), str(session_id),
            ))
        open_count = len(rows)

    observed = {host: 0 for host in (
        "claude_code", "codex", "command_code", "cline", "opencode", "pi",
        "gemini", "grok_build", "roo_cline", "qwen", "cursor",
    )}
    for source in sources:
        observed[source.host] = observed.get(source.host, 0) + 1
    observed["opencode"] = open_count
    return sources, observed


def _message_events(message: dict[str, Any], timestamp: Any = None) -> list[dict[str, Any]]:
    role = str(message.get("role") or "")
    content = message.get("content")
    when = message.get("timestamp", message.get("ts", timestamp))
    if role == "toolResult":
        text = "\n".join(_text_values(content))
        return [{
            "kind": "tool_result", "role": "user", "timestamp": when,
            "tool": message.get("toolName"), "call_id": message.get("toolCallId"),
            "text": text, "is_error": bool(message.get("isError")),
        }] if text else []
    if role not in {"user", "assistant"}:
        return []
    if isinstance(content, str):
        return [{"kind": f"{role}_message", "role": role, "timestamp": when, "text": content}]
    if not isinstance(content, list):
        return []
    events: list[dict[str, Any]] = []
    for block in content:
        if not isinstance(block, dict):
            continue
        block_type = str(block.get("type") or "")
        if block_type == "text" and isinstance(block.get("text"), str):
            events.append({"kind": f"{role}_message", "role": role, "timestamp": when,
                           "text": block["text"]})
        elif block_type in {"tool_use", "toolCall", "tool_call"}:
            value = block.get("input", block.get("arguments", {}))
            events.append({
                "kind": "tool_call", "role": "assistant", "timestamp": when,
                "tool": block.get("name") or block.get("toolName") or "unknown",
                "call_id": block.get("id") or block.get("callId"), "text": value,
            })
        elif block_type in {"tool_result", "toolResult"}:
            value = block.get("content", block.get("output", ""))
            events.append({
                "kind": "tool_result", "role": "user", "timestamp": when,
                "tool": block.get("name") or block.get("toolName"),
                "call_id": block.get("tool_use_id") or block.get("toolCallId"),
                "text": value, "is_error": bool(block.get("is_error") or block.get("isError")),
            })
    return events


def _text_values(content: Any) -> list[str]:
    if isinstance(content, str):
        return [content]
    if not isinstance(content, list):
        return []
    return [str(item.get("text")) for item in content
            if isinstance(item, dict) and isinstance(item.get("text"), str)]


def _jsonl_message_events(path: Path) -> tuple[str, str, list[dict[str, Any]]]:
    session_id, cwd, events = path.stem, "", []
    with path.open(encoding="utf-8", errors="replace") as handle:
        for line in handle:
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(row, dict):
                continue
            if row.get("type") == "session":
                session_id = str(row.get("id") or session_id)
                cwd = str(row.get("cwd") or cwd)
            elif row.get("type") == "message" and isinstance(row.get("message"), dict):
                events.extend(_message_events(row["message"], row.get("timestamp")))
    return session_id, cwd, events


def _cline_events(path: Path) -> tuple[str, str, list[dict[str, Any]]]:
    body = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    if not isinstance(body, dict) or not isinstance(body.get("messages"), list):
        raise ValueError("invalid Cline transcript")
    companion = path.with_name(path.name.removesuffix(".messages.json") + ".json")
    metadata: dict[str, Any] = {}
    if companion.is_file():
        loaded = json.loads(companion.read_text(encoding="utf-8", errors="replace"))
        if isinstance(loaded, dict):
            metadata = loaded
    session_id = str(body.get("sessionId") or metadata.get("session_id") or path.parent.name)
    cwd = str(metadata.get("workspace_root") or metadata.get("cwd") or path.parent.parent.name)
    events: list[dict[str, Any]] = []
    for message in body["messages"]:
        if isinstance(message, dict):
            events.extend(_message_events(message))
    return session_id, cwd, events


def _generic_json_events(source: NativeSource) -> tuple[str, str, list[dict[str, Any]]]:
    if source.host in {"command_code", "pi"}:
        return _jsonl_message_events(source.path)
    if source.host == "cline":
        return _cline_events(source.path)
    body = json.loads(source.path.read_text(encoding="utf-8", errors="replace"))
    rows = body if isinstance(body, list) else (
        body.get("messages") or body.get("history") or [] if isinstance(body, dict) else []
    )
    session_id = source.path.parent.name
    cwd = source.cwd or source.path.parent.parent.name
    events: list[dict[str, Any]] = []
    for row in rows:
        if isinstance(row, dict):
            events.extend(_message_events(row, row.get("timestamp")))
    return session_id, cwd, events


def _opencode_events(source: NativeSource) -> tuple[str, str, list[dict[str, Any]], str]:
    uri = f"file:{source.path}?mode=ro"
    events: list[dict[str, Any]] = []
    canonical: list[dict[str, Any]] = []
    with sqlite3.connect(uri, uri=True, timeout=30) as database:
        session = database.execute(
            "SELECT directory, parent_id, agent, model, time_created FROM session WHERE id = ?",
            (source.selector,),
        ).fetchone()
        if not session:
            raise ValueError("OpenCode session disappeared during snapshot")
        cwd, parent_id, agent, model, _created = session
        rows = database.execute(
            "SELECT m.id, m.time_created, m.data, p.id, p.time_created, p.data "
            "FROM message m LEFT JOIN part p ON p.message_id = m.id "
            "WHERE m.session_id = ? ORDER BY m.time_created, m.id, p.time_created, p.id",
            (source.selector,),
        ).fetchall()
    messages: dict[str, tuple[int, dict[str, Any], list[tuple[int, dict[str, Any]]]]] = {}
    for message_id, message_time, message_data, _part_id, part_time, part_data in rows:
        message = json.loads(message_data)
        entry = messages.setdefault(str(message_id), (int(message_time), message, []))
        if part_data:
            entry[2].append((int(part_time or message_time), json.loads(part_data)))
    for _message_id, (message_time, message, parts) in messages.items():
        canonical.append({"message": message, "parts": [part for _time, part in parts]})
        role = str(message.get("role") or "")
        when = (message.get("time") or {}).get("created") or message_time
        for _part_time, part in parts:
            part_type = part.get("type")
            if part_type == "text" and isinstance(part.get("text"), str):
                events.append({"kind": f"{role}_message", "role": role, "timestamp": when,
                               "text": part["text"], "synthetic": bool(part.get("synthetic")),
                               "agentRole": agent or model})
            elif part_type == "tool":
                state = part.get("state") if isinstance(part.get("state"), dict) else {}
                call_id = part.get("callID")
                events.append({"kind": "tool_call", "role": "assistant", "timestamp": when,
                               "tool": part.get("tool") or "unknown", "call_id": call_id,
                               "text": state.get("input", {}), "agentRole": agent or model})
                if state.get("status") in {"completed", "error"}:
                    events.append({"kind": "tool_result", "role": "user", "timestamp": when,
                                   "tool": part.get("tool") or "unknown", "call_id": call_id,
                                   "text": state.get("output", state.get("error", "")),
                                   "is_error": state.get("status") == "error", "agentRole": agent or model})
    digest = "sha256:" + hashlib.sha256(json.dumps(
        canonical, sort_keys=True, separators=(",", ":"), ensure_ascii=False,
    ).encode("utf-8")).hexdigest()
    return source.selector, str(cwd or ""), events, digest


def _native_events(source: NativeSource) -> tuple[str, str, list[dict[str, Any]], str]:
    if source.host == "opencode":
        return _opencode_events(source)
    if source.host in {"claude_code", "codex"}:
        with tempfile.TemporaryDirectory(prefix="adapt-source-freeze-") as directory:
            frozen = Path(directory) / source.path.name
            shutil.copyfile(source.path, frozen)
            digest = _hash_file(frozen)
            events = parse_source_events(frozen, host=source.host)
        session_id = next((str(event.get("sessionId")) for event in events if event.get("sessionId")), source.path.stem)
        cwd = ""
        parser = adapt_sessions.parse_claude_session if source.host == "claude_code" else adapt_sessions.parse_codex_session
        parsed = parser(source.path, max_turns=1)
        if parsed is not None:
            cwd = parsed.cwd
        return session_id, cwd, events, digest
    session_id, cwd, events = _generic_json_events(source)
    return session_id, cwd, events, _hash_file(source.path)


def _event_row(source: NativeSource, session_id: str, cwd: str, event: dict[str, Any]) -> dict[str, Any] | None:
    kind = str(event.get("kind") or "")
    if kind not in {"user_message", "assistant_message", "tool_call", "tool_result", "meta"}:
        return None
    text = compact_text(event.get("text", ""))
    if not text:
        return None
    if kind in {"user_message", "assistant_message"} and adapt_sessions.text_excluded(text):
        return None
    normalized = {
        "kind": kind,
        "role": event.get("role"),
        "tool": event.get("tool"),
        "call_id": event.get("call_id"),
        "text": text,
        "timestamp": event.get("timestamp"),
        "is_error": bool(event.get("is_error") or (event.get("flags") or {}).get("isError")),
        "synthetic": bool(event.get("synthetic")),
        "meta": bool(event.get("meta")),
        "agentRole": event.get("agentRole"),
        "threadSource": event.get("threadSource") or "root",
        "parentThreadId": event.get("parentThreadId"),
        "cwd": cwd,
    }
    return {
        "type": "adapt_event_v1", "host": source.host,
        "sessionId": session_id, "cwd": cwd, "threadSource": normalized["threadSource"],
        "event": {key: value for key, value in normalized.items() if value not in (None, "")},
    }


def freeze_all(output_dir: Path, *, home: Path | None = None) -> dict[str, Any]:
    root = output_dir.resolve()
    snapshots = root / "snapshots"
    snapshots.mkdir(parents=True, exist_ok=False)
    sources, discovery = discover_native_transcripts(home)
    accounting = {host: {"discovered": count, "snapshotted": 0, "empty": 0, "excluded": 0,
                         "failed": 0, "events": 0} for host, count in discovery.items()}
    refs: list[dict[str, Any]] = []
    for source in sources:
        host_counts = accounting[source.host]
        try:
            session_id, cwd, events, source_sha = _native_events(source)
            if adapt_sessions.scope_excluded(cwd):
                host_counts["excluded"] += 1
                continue
            rows = [row for event in events if (row := _event_row(source, session_id, cwd, event))]
            if not rows:
                host_counts["empty"] += 1
                continue
            host_dir = snapshots / source.host
            host_dir.mkdir(parents=True, exist_ok=True)
            path = host_dir / f"{_safe_id(source.source_key)}.jsonl"
            temporary = path.with_suffix(".jsonl.tmp")
            with temporary.open("w", encoding="utf-8") as handle:
                for row in rows:
                    handle.write(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n")
            os.replace(temporary, path)
            snapshot_sha = _hash_file(path)
            refs.append({
                "host": source.host, "tool": source.tool, "session_id": session_id,
                "cwd": cwd, "source_key_sha256": "sha256:" + hashlib.sha256(source.source_key.encode()).hexdigest(),
                "source_sha256": source_sha, "snapshot_path": str(path),
                "snapshot_sha256": snapshot_sha, "event_count": len(rows),
            })
            host_counts["snapshotted"] += 1
            host_counts["events"] += len(rows)
        except Exception:
            host_counts["failed"] += 1
    manifest = {
        "schema": "adapt.transcript-snapshot-run.v1",
        "root": str(root), "discovery": discovery, "accounting": accounting,
        "source_count": len(sources), "snapshot_count": len(refs), "sources": refs,
    }
    manifest_path = root / "snapshot-manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    manifest["manifest_path"] = str(manifest_path)
    return manifest


__all__ = ["NativeSource", "discover", "freeze_all"]
