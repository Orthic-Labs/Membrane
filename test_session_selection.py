"""Session selection ordering contracts."""
from __future__ import annotations

import os
from pathlib import Path

import adapt_sessions as sessions


def test_new_sessions_applies_limit_after_chronological_sort(tmp_path, monkeypatch):
    paths = []
    for index, mtime in enumerate((30, 10, 20)):
        path = tmp_path / f"session-{index}.jsonl"
        path.write_text("{}\n", encoding="utf-8")
        os.utime(path, (mtime, mtime))
        paths.append(path)

    monkeypatch.setattr(
        sessions, "discover", lambda: [("claude-code", path) for path in paths]
    )
    monkeypatch.setattr(
        sessions,
        "parse_claude_session",
        lambda path: sessions.Session(
            "claude-code", path.stem, path, "D--Claude", path.stat().st_mtime,
            [sessions.Turn("always verify the focused test", "D--Claude")],
        ),
    )

    selected = sessions.new_sessions({"learned": {}}, limit=2)

    assert [item.mtime for item in selected] == [10, 20]


def test_newest_selection_parses_only_requested_sessions(tmp_path, monkeypatch):
    paths = []
    for index, mtime in enumerate((10, 20, 30, 40)):
        path = tmp_path / f"session-{index}.jsonl"
        path.write_text("{}\n", encoding="utf-8")
        os.utime(path, (mtime, mtime))
        paths.append(path)
    monkeypatch.setattr(
        sessions, "discover", lambda: [("claude-code", path) for path in paths]
    )
    parsed = []

    def fake_parse(path):
        parsed.append(path)
        return sessions.Session(
            "claude-code", path.stem, path, "D--Claude", path.stat().st_mtime,
            [sessions.Turn("always verify the focused test", "D--Claude")],
        )

    monkeypatch.setattr(sessions, "parse_claude_session", fake_parse)

    selected = sessions.new_sessions({"learned": {}}, limit=2, newest=True)

    assert [item.mtime for item in selected] == [30, 40]
    assert len(parsed) == 2
