from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

import cli
import manifest
import run_journal
import taste_v2
from tools.lib.orthic_transcripts import parse_source_events


def _write_correction(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({
        "type": "user", "uuid": "u1", "sessionId": "s1",
        "cwd": "/Volumes/D/claude/morph", "timestamp": "2026-01-01T00:00:00Z",
        "message": {"role": "user", "content":
                    "No, that's wrong. Always run focused tests before reporting completion."},
    }) + "\n", encoding="utf-8")


def test_real_parser_false_flags_admit_and_truthy_unsafe_flags_reject(tmp_path: Path) -> None:
    transcript = tmp_path / "session.jsonl"
    _write_correction(transcript)
    events = parse_source_events(transcript, host="claude_code")
    assert all(value is False for value in events[0]["flags"].values())
    candidate = taste_v2.extract_candidates(events)[0]
    assert taste_v2.admit_candidate(candidate).lifecycleState == "active"
    for unsafe in ("synthetic", "meta", "privateReasoningOmitted", "redacted"):
        flagged = [dict(events[0], flags={**events[0]["flags"], unsafe: True})]
        assert taste_v2.extract_candidates(flagged) == []


def test_cli_manifest_writer_is_v13_with_hashed_evidence_contexts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    home = tmp_path / "home"
    _write_correction(home / ".claude" / "projects" / "project" / "session.jsonl")
    output = tmp_path / "manifest.json"
    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setattr(run_journal, "JOURNAL_FILE", tmp_path / "journal.jsonl")
    monkeypatch.setattr(sys, "argv", ["morph.py", "--incremental", "--manifest", str(output)])
    assert cli.main() == 0
    body = json.loads(output.read_text(encoding="utf-8"))
    assert body["schema_version"] == "1.3.0"
    assert body["records"] and body["records"][0]["evidenceContexts"]
    assert body["records"][0]["payload_sha256"] == manifest.payload_sha256(body["records"][0])
