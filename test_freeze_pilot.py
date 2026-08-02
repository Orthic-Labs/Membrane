"""Phase 1 deterministic frozen-pilot contract tests."""
from __future__ import annotations

import hashlib
import json
import os
import sys
from pathlib import Path

import pytest

MORPH_DIR = Path(__file__).resolve().parent
EVAL_DIR = MORPH_DIR / "eval"
sys.path.insert(0, str(MORPH_DIR))
sys.path.insert(0, str(EVAL_DIR))

import morph_llm
import morph_sessions
import freeze_pilot


def _session(
    root: Path,
    *,
    tool: str,
    sid: str,
    scope: str,
    mtime: float,
    chars: int,
) -> morph_sessions.Session:
    path = root / f"{tool}-{sid}.jsonl"
    text = f"{sid} durable preference " + "x" * chars
    if tool == "claude-code":
        rows = [{
            "type": "user",
            "userType": "external",
            "sessionId": sid,
            "cwd": scope,
            "message": {"content": text},
        }]
    else:
        rows = [
            {"type": "session_meta", "payload": {"session_id": sid, "cwd": scope}},
            {"type": "response_item", "payload": {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": text}],
            }},
        ]
    path.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
    os.utime(path, (mtime, mtime))
    parser = (
        morph_sessions.parse_claude_session
        if tool == "claude-code"
        else morph_sessions.parse_codex_session
    )
    parsed = parser(path)
    assert parsed is not None
    return parsed


def _workspace(root: Path) -> Path:
    (root / ".claude" / "rules").mkdir(parents=True)
    (root / "AGENTS.md").write_text("Never bypass review.\n", encoding="utf-8")
    (root / "CLAUDE.md").write_text("Always preserve user changes.\n", encoding="utf-8")
    (root / ".claude" / "rules" / "ops.md").write_text(
        "Never edit production directly.\n", encoding="utf-8"
    )
    return root


def _sessions(root: Path) -> list[morph_sessions.Session]:
    return [
        _session(root, tool="claude-code", sid="c1", scope="repo-a", mtime=90, chars=100),
        _session(root, tool="claude-code", sid="c2", scope="repo-b", mtime=70, chars=3000),
        _session(root, tool="claude-code", sid="c3", scope="repo-a", mtime=10, chars=14000),
        _session(root, tool="codex", sid="x1", scope="repo-a", mtime=100, chars=200),
        _session(root, tool="codex", sid="x2", scope="repo-c", mtime=60, chars=4000),
        _session(root, tool="codex", sid="x3", scope="repo-c", mtime=20, chars=15000),
    ]


def test_extract_payload_helper_matches_provider_user_payload():
    batch = [
        ("claude-code", "repo-a", "always use JSONL"),
        ("codex", "repo-b", "run focused tests"),
    ]

    payload = morph_llm.build_extract_payload(batch)

    assert json.loads(payload) == [
        {"id": 1, "tool": "claude-code", "scope": "repo-a", "text": "always use JSONL"},
        {"id": 2, "tool": "codex", "scope": "repo-b", "text": "run focused tests"},
    ]
    assert payload == json.dumps(json.loads(payload), ensure_ascii=False)


def test_selection_is_input_order_independent_and_balances_tools(tmp_path):
    sessions = _sessions(tmp_path)

    forward = freeze_pilot.select_sessions(sessions, target=4)
    reverse = freeze_pilot.select_sessions(list(reversed(sessions)), target=4)

    assert [s.session_id for s in forward] == [s.session_id for s in reverse]
    assert {s.tool for s in forward} == {"claude-code", "codex"}
    assert len({freeze_pilot.primary_scope(s) for s in forward}) >= 2


def test_settle_cutoff_excludes_recently_modified_sessions(tmp_path):
    sessions = _sessions(tmp_path)

    eligible = freeze_pilot.filter_settled_sessions(
        sessions, eligible_before_ns=50 * 1_000_000_000
    )

    assert {session.session_id for session in eligible} == {"c3", "x3"}


def test_freeze_writes_reproducible_local_package(tmp_path):
    workspace = _workspace(tmp_path / "workspace")
    session_root = tmp_path / "sessions"
    session_root.mkdir()
    sessions = _sessions(session_root)
    taste = tmp_path / "taste"
    (taste / "nested").mkdir(parents=True)
    (taste / "taste.md").write_text("# Taste\n- use JSONL\n", encoding="utf-8")
    (taste / "nested" / "taste.md").write_text(
        "# Nested\n- preserve tests\n", encoding="utf-8"
    )

    first = freeze_pilot.freeze_pilot(
        sessions,
        tmp_path / "out-a",
        target=4,
        workspace_root=workspace,
        artifact_roots=[taste],
        max_raw_bytes=1_000_000,
    )
    second = freeze_pilot.freeze_pilot(
        list(reversed(sessions)),
        tmp_path / "out-b",
        target=4,
        workspace_root=workspace,
        artifact_roots=[taste],
        max_raw_bytes=1_000_000,
    )

    assert first == second
    assert freeze_pilot.validate_manifest(tmp_path / "out-a" / "manifest.json") == first
    assert first["selection"]["selected_sessions"] == 4
    assert first["selection"]["shortfall"] == 0
    assert first["privacy"]["raw_local_only"] is True
    assert len(first["sessions"]) == 4
    assert all((tmp_path / "out-a" / s["raw_copy"]).is_file() for s in first["sessions"])
    assert len(first["initial_artifacts"]["files"]) == 2
    roots = {item["source_root"] for item in first["initial_artifacts"]["files"]}
    assert len(roots) == 1
    assert next(iter(roots)).endswith("/taste")
    corpus = (tmp_path / "out-a" / first["corpus"]["path"]).read_bytes()
    assert hashlib.sha256(corpus).hexdigest() == first["corpus"]["sha256"]
    assert first["extraction"]["batch_count"] >= 1
    assert all(len(item["payload_sha256"]) == 64 for item in first["extraction"]["batches"])
    assert first["authority"]["manifest"]["sources"]
    assert first["quality"]["kept_turns"] == first["corpus"]["turn_count"]
    assert first["quality"]["truncated_turns"] >= 0
    assert all(
        (tmp_path / "out-a" / item["copy"]).is_file()
        for item in first["versions"]["files"]
    )
    assert first["pilot_id"] == f"morph-pilot-{first['content_sha256'][:12]}"
    assert first["manifest_sha256"] == freeze_pilot.manifest_sha256(first)
    assert freeze_pilot.main([
        "--validate-only", str(tmp_path / "out-a" / "manifest.json")
    ]) == 0


def test_freeze_refuses_raw_copy_over_ceiling_before_writing(tmp_path):
    workspace = _workspace(tmp_path / "workspace")
    session_root = tmp_path / "sessions"
    session_root.mkdir()
    sessions = _sessions(session_root)
    out = tmp_path / "too-large"

    with pytest.raises(ValueError, match="raw transcript byte ceiling"):
        freeze_pilot.freeze_pilot(
            sessions,
            out,
            target=4,
            workspace_root=workspace,
            artifact_roots=[],
            max_raw_bytes=1,
        )

    assert not out.exists()


def test_freeze_reports_shortfall_instead_of_duplicating(tmp_path):
    sessions = _sessions(tmp_path)

    selected = freeze_pilot.select_sessions(sessions, target=50)

    assert len(selected) == len(sessions)
    assert len({s.session_id for s in selected}) == len(sessions)


def test_freeze_refuses_session_that_changed_after_discovery(tmp_path):
    workspace = _workspace(tmp_path / "workspace")
    session_root = tmp_path / "sessions"
    session_root.mkdir()
    session = _session(
        session_root,
        tool="claude-code",
        sid="changing",
        scope="repo-a",
        mtime=10,
        chars=100,
    )
    with session.path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps({
            "type": "user",
            "userType": "external",
            "sessionId": "changing",
            "cwd": "repo-a",
            "message": {"content": "always preserve this newly appended preference"},
        }) + "\n")

    with pytest.raises(ValueError, match="changed after discovery"):
        freeze_pilot.freeze_pilot(
            [session],
            tmp_path / "out",
            target=1,
            workspace_root=workspace,
            artifact_roots=[],
            max_raw_bytes=1_000_000,
        )
