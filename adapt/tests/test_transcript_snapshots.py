from __future__ import annotations

import json
import sqlite3
from pathlib import Path

import pytest

from adapt import transcript_snapshots
from continuity.transcript import parse_source_events


def _jsonl(path: Path, rows: list[dict]) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
    return path


def test_freeze_all_normalizes_claude_pi_and_opencode(tmp_path: Path) -> None:
    _jsonl(tmp_path / ".claude/projects/repo/c1.jsonl", [{
        "type": "user", "sessionId": "c1", "cwd": "/repo",
        "message": {"role": "user", "content": "Always run focused tests."},
    }])
    _jsonl(tmp_path / ".pi/agent/sessions/repo/p1.jsonl", [
        {"type": "session", "id": "p1", "cwd": "/repo"},
        {"type": "message", "message": {
            "role": "assistant", "content": [{
                "type": "toolCall", "id": "t1", "name": "read",
                "arguments": {"path": "x"},
            }],
        }},
        {"type": "message", "message": {
            "role": "toolResult", "toolCallId": "t1", "toolName": "read",
            "isError": True, "content": [{"type": "text", "text": "ENOENT"}],
        }},
    ])

    db_path = tmp_path / ".local/share/opencode/opencode.db"
    db_path.parent.mkdir(parents=True)
    with sqlite3.connect(db_path) as database:
        database.executescript("""
            CREATE TABLE session (
              id TEXT PRIMARY KEY, directory TEXT, parent_id TEXT, agent TEXT,
              model TEXT, time_created INTEGER, time_archived INTEGER
            );
            CREATE TABLE message (
              id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT
            );
            CREATE TABLE part (
              id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT,
              time_created INTEGER, data TEXT
            );
        """)
        database.execute(
            "INSERT INTO session VALUES (?, ?, ?, ?, ?, ?, ?)",
            ("o1", "/repo", None, "build", "ox", 1, None),
        )
        database.execute(
            "INSERT INTO message VALUES (?, ?, ?, ?)",
            ("m1", "o1", 2, json.dumps({"role": "user", "time": {"created": 2}})),
        )
        database.execute(
            "INSERT INTO part VALUES (?, ?, ?, ?, ?)",
            ("pt1", "m1", "o1", 3, json.dumps({"type": "text", "text": "Never skip verification."})),
        )

    manifest = transcript_snapshots.freeze_all(tmp_path / "run", home=tmp_path)
    assert manifest["snapshot_count"] == 3
    assert manifest["accounting"]["claude_code"]["snapshotted"] == 1
    assert manifest["accounting"]["pi"]["events"] == 2
    assert manifest["accounting"]["opencode"]["snapshotted"] == 1
    events = [
        event
        for source in manifest["sources"]
        for event in parse_source_events(source["snapshot_path"])
    ]
    assert {event["host"] for event in events} == {"claude_code", "pi", "opencode"}
    assert any(event["classification"] == "unresolved_failure" for event in events)


def test_snapshot_redacts_secret_like_text(tmp_path: Path) -> None:
    _jsonl(tmp_path / ".pi/agent/sessions/repo/p1.jsonl", [
        {"type": "session", "id": "p1", "cwd": "/repo"},
        {"type": "message", "message": {
            "role": "user", "content": [{"type": "text", "text": "token=abcdefghijklmnop"}],
        }},
    ])
    manifest = transcript_snapshots.freeze_all(tmp_path / "run", home=tmp_path)
    text = Path(manifest["sources"][0]["snapshot_path"]).read_text(encoding="utf-8")
    assert "abcdefghijklmnop" not in text
    assert "[REDACTED]" in text


def test_load_frozen_sources_verifies_hashes(tmp_path: Path) -> None:
    snapshot = _jsonl(tmp_path / "pi.jsonl", [{
        "type": "adapt_event_v1", "host": "pi", "sessionId": "s1",
        "cwd": str(tmp_path), "threadSource": "root",
        "event": {"kind": "user_message", "role": "user", "text": "Always be concise"},
    }])
    digest = transcript_snapshots._hash_file(snapshot)
    manifest = tmp_path / "snapshot-manifest.json"
    manifest.write_text(json.dumps({"sources": [{
        "host": "pi", "tool": "pi", "snapshot_path": str(snapshot),
        "snapshot_sha256": digest, "source_key_sha256": "sha256:" + "a" * 64,
    }]}), encoding="utf-8")

    loaded = transcript_snapshots.load_frozen_sources(manifest)
    assert len(loaded) == 1
    assert loaded[0].spec.host == "pi"
    assert loaded[0].session_id == "s1"

    snapshot.write_text("tampered\n", encoding="utf-8")
    with pytest.raises(ValueError, match="snapshot hash mismatch"):
        transcript_snapshots.load_frozen_sources(manifest)
