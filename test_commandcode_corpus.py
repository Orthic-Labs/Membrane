"""CommandCode-compatible corpus freezer contract tests."""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
from datetime import datetime, timezone
from pathlib import Path

import pytest


ADAPT_DIR = Path(__file__).resolve().parent
MODULE_PATH = ADAPT_DIR / "eval" / "freeze_commandcode_corpus.py"


def _module():
    assert MODULE_PATH.is_file(), "CommandCode corpus freezer must exist"
    spec = importlib.util.spec_from_file_location("freeze_commandcode_corpus", MODULE_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _write_jsonl(path: Path, rows: list[dict], *, mtime: float = 1.0) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "".join(json.dumps(row, ensure_ascii=False) + "\n" for row in rows),
        encoding="utf-8",
    )
    os.utime(path, (mtime, mtime))
    return path


def test_commandcode_token_estimator_and_batch_format_are_exact():
    freezer = _module()

    assert freezer.estimate_text_tokens("alpha beta") == 3
    assert freezer.estimate_tokens("[alpha beta]") == 5
    assert freezer.format_batch(["first", "second\nline"]) == (
        "1. first\n\n2. second\nline"
    )
    assert freezer.split_prompts_into_batches(["aaaa", "bbbb", "c"], budget=2) == [
        ["aaaa"],
        ["bbbb"],
        ["c"],
    ]


def test_extractors_match_commandcode_shapes_and_historical_cutoff(tmp_path):
    freezer = _module()
    cutoff = datetime(2026, 7, 13, 13, 51, tzinfo=timezone.utc)
    claude = _write_jsonl(
        tmp_path / "claude.jsonl",
        [
            {
                "type": "user",
                "timestamp": "2026-07-13T13:50:00Z",
                "message": {"content": "  keep this  "},
            },
            {
                "type": "user",
                "timestamp": "2026-07-13T13:52:00Z",
                "message": {"content": "drop future"},
            },
            {
                "type": "user",
                "timestamp": "2026-07-13T13:49:00Z",
                "message": {"content": [{"type": "text", "text": "drop non-string"}]},
            },
        ],
    )
    codex = _write_jsonl(
        tmp_path / "codex.jsonl",
        [
            {
                "type": "session_meta",
                "timestamp": "2026-07-13T13:00:00Z",
                "payload": {"id": "x1", "cwd": "D:\\Claude"},
            },
            {
                "type": "event_msg",
                "timestamp": "2026-07-13T13:50:30Z",
                "payload": {"type": "user_message", "message": "  codex prompt  "},
            },
            {
                "type": "event_msg",
                "timestamp": "2026-07-13T13:52:00Z",
                "payload": {"type": "user_message", "message": "drop future"},
            },
        ],
    )

    assert freezer.extract_claude_prompts(claude, cutoff=cutoff) == ["keep this"]
    assert freezer.extract_codex_prompts(codex, cutoff=cutoff) == ["codex prompt"]
    assert freezer.read_codex_meta(codex) == {
        "id": "x1",
        "cwd": "D:\\Claude",
        "timestamp": "2026-07-13T13:00:00Z",
    }


def test_resolver_finds_archived_codex_and_preserves_source_order(tmp_path):
    freezer = _module()
    claude_dir = tmp_path / "claude"
    active = tmp_path / "codex" / "sessions"
    archive = tmp_path / "codex" / "archived_sessions"
    _write_jsonl(
        claude_dir / "c1.jsonl",
        [{"type": "user", "message": {"content": "claude one"}}],
        mtime=10,
    )
    _write_jsonl(
        active / "2026" / "07" / "13" / "rollout-x1.jsonl",
        [
            {"type": "session_meta", "payload": {"id": "x1", "cwd": "D:\\Claude"}},
            {"type": "event_msg", "payload": {"type": "user_message", "message": "active"}},
        ],
        mtime=20,
    )
    _write_jsonl(
        archive / "rollout-x2.jsonl",
        [
            {"type": "session_meta", "payload": {"id": "x2", "cwd": "D:\\Claude"}},
            {"type": "event_msg", "payload": {"type": "user_message", "message": "archived"}},
        ],
        mtime=30,
    )
    settings = tmp_path / "settings.json"
    settings.write_text(json.dumps({
        "tasteOnboarding": {"learnedSessions": {
            "claude-code": ["c1", "missing-c"],
            "codex": ["x1", "x2", "missing-x"],
        }}
    }), encoding="utf-8")

    corpus = freezer.resolve_corpus(
        settings_path=settings,
        claude_dir=claude_dir,
        codex_roots=[active, archive],
        cwd="D:\\Claude",
    )

    assert [item.session_id for item in corpus.sessions] == ["x2", "x1", "c1"]
    assert [item.source for item in corpus.sessions] == ["codex", "codex", "claude-code"]
    assert corpus.missing == {
        "claude-code": ["missing-c"],
        "codex": ["missing-x"],
    }
    assert [record.text for record in corpus.prompts] == ["archived", "active", "claude one"]


def test_resolver_reads_only_codex_files_named_for_learned_ids(tmp_path, monkeypatch):
    freezer = _module()
    codex_root = tmp_path / "codex"
    for index in range(20):
        _write_jsonl(
            codex_root / f"rollout-unrelated-{index}.jsonl",
            [{"type": "session_meta", "payload": {
                "id": f"other-{index}", "cwd": "D:\\Claude"
            }}],
        )
    _write_jsonl(
        codex_root / "rollout-x1.jsonl",
        [
            {"type": "session_meta", "payload": {"id": "x1", "cwd": "D:\\Claude"}},
            {"type": "event_msg", "payload": {"type": "user_message", "message": "wanted"}},
        ],
    )
    settings = tmp_path / "settings.json"
    settings.write_text(json.dumps({
        "tasteOnboarding": {"learnedSessions": {"codex": ["x1"]}}
    }), encoding="utf-8")
    original = freezer.read_codex_meta
    calls: list[Path] = []

    def counted(path):
        calls.append(path)
        return original(path)

    monkeypatch.setattr(freezer, "read_codex_meta", counted)
    corpus = freezer.resolve_corpus(
        settings_path=settings,
        claude_dir=tmp_path / "none",
        codex_roots=[codex_root],
        cwd="D:\\Claude",
    )

    assert [record.text for record in corpus.prompts] == ["wanted"]
    assert [path.name for path in calls] == ["rollout-x1.jsonl"]


def test_cutoff_uses_last_pre_cutoff_event_for_historical_codex_order(tmp_path):
    freezer = _module()
    codex_root = tmp_path / "codex"
    _write_jsonl(
        codex_root / "rollout-x1.jsonl",
        [
            {"timestamp": "2026-07-13T12:00:00Z", "type": "session_meta",
             "payload": {"id": "x1", "cwd": "D:\\Claude"}},
            {"timestamp": "2026-07-13T12:30:00Z", "type": "event_msg",
             "payload": {"type": "user_message", "message": "newer at cutoff"}},
        ],
        mtime=2_000_000_100,
    )
    _write_jsonl(
        codex_root / "rollout-x2.jsonl",
        [
            {"timestamp": "2026-07-13T11:00:00Z", "type": "session_meta",
             "payload": {"id": "x2", "cwd": "D:\\Claude"}},
            {"timestamp": "2026-07-13T11:30:00Z", "type": "event_msg",
             "payload": {"type": "user_message", "message": "older at cutoff"}},
        ],
        mtime=2_000_000_200,
    )
    settings = tmp_path / "settings.json"
    settings.write_text(json.dumps({
        "tasteOnboarding": {"learnedSessions": {"codex": ["x1", "x2"]}}
    }), encoding="utf-8")

    corpus = freezer.resolve_corpus(
        settings_path=settings,
        claude_dir=tmp_path / "none",
        codex_roots=[codex_root],
        cwd="D:\\Claude",
        cutoff=datetime(2026, 7, 13, 13, tzinfo=timezone.utc),
    )

    assert [session.session_id for session in corpus.sessions] == ["x1", "x2"]


def test_freeze_writes_content_addressed_batches_and_validates(tmp_path):
    freezer = _module()
    claude_dir = tmp_path / "claude"
    _write_jsonl(
        claude_dir / "c1.jsonl",
        [
            {"type": "user", "message": {"content": "first preference"}},
            {"type": "user", "message": {"content": "second preference"}},
        ],
    )
    settings = tmp_path / "settings.json"
    settings.write_text(json.dumps({
        "tasteOnboarding": {"learnedSessions": {"claude-code": ["c1"]}}
    }), encoding="utf-8")
    out = tmp_path / "frozen"

    manifest = freezer.freeze_corpus(
        settings_path=settings,
        claude_dir=claude_dir,
        codex_roots=[],
        cwd="D:\\Claude",
        output_dir=out,
        budget=150_000,
    )

    assert manifest["prompt_count"] == 2
    assert manifest["batch_count"] == 1
    batch = manifest["batches"][0]
    payload = (out / batch["path"]).read_bytes()
    assert hashlib.sha256(payload).hexdigest() == batch["sha256"]
    assert payload.decode("utf-8") == "1. first preference\n\n2. second preference"
    assert freezer.validate_manifest(out / "manifest.json") == manifest


def test_freeze_removes_staging_output_when_a_write_fails(tmp_path, monkeypatch):
    freezer = _module()
    claude_dir = tmp_path / "claude"
    _write_jsonl(
        claude_dir / "c1.jsonl",
        [{"type": "user", "message": {"content": "first preference"}}],
    )
    settings = tmp_path / "settings.json"
    settings.write_text(json.dumps({
        "tasteOnboarding": {"learnedSessions": {"claude-code": ["c1"]}}
    }), encoding="utf-8")
    out = tmp_path / "frozen"
    original = Path.write_bytes

    def fail_batch_write(path, data):
        if path.name == "batch-001.txt":
            raise OSError("injected write failure")
        return original(path, data)

    monkeypatch.setattr(Path, "write_bytes", fail_batch_write)

    with pytest.raises(OSError, match="injected write failure"):
        freezer.freeze_corpus(
            settings_path=settings,
            claude_dir=claude_dir,
            codex_roots=[],
            cwd="D:\\Claude",
            output_dir=out,
        )

    assert not out.exists()
    assert not list(tmp_path.glob(".frozen.tmp-*"))


def test_capture_log_comparison_reports_exact_and_mismatched_batches(tmp_path):
    freezer = _module()
    log = tmp_path / "command.log"
    log.write_text(
        "12:00 < [Import] batch 1/2: 2 prompts "
        "sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa "
        "bytes=10\n"
        "12:01 < [Import] batch 2/2: 1 prompts "
        "sha256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb "
        "bytes=5\n",
        encoding="utf-8",
    )
    manifest = {"batches": [
        {
            "index": 1,
            "prompt_count": 2,
            "sha256": "a" * 64,
            "bytes": 10,
        },
        {
            "index": 2,
            "prompt_count": 2,
            "sha256": "c" * 64,
            "bytes": 6,
        },
    ]}

    captured = freezer.parse_capture_log(log)
    report = freezer.compare_batches(manifest, captured)

    assert [item["index"] for item in captured] == [1, 2]
    assert report["exact_batches"] == [1]
    assert report["exact_count"] == 1
    assert report["captured_count"] == 2
    assert report["mismatches"] == [{
        "index": 2,
        "fields": ["prompt_count", "bytes", "sha256"],
    }]


def test_cli_smoke_freezes_fixture_and_prints_summary(tmp_path, capsys):
    freezer = _module()
    claude_dir = tmp_path / "claude"
    _write_jsonl(
        claude_dir / "c1.jsonl",
        [{"type": "user", "message": {"content": "fixture preference"}}],
    )
    settings = tmp_path / "settings.json"
    settings.write_text(json.dumps({
        "tasteOnboarding": {"learnedSessions": {"claude-code": ["c1"]}}
    }), encoding="utf-8")

    assert freezer.main([
        "--settings", str(settings),
        "--claude-dir", str(claude_dir),
        "--cwd", "D:\\Claude",
        "--output", str(tmp_path / "out"),
    ]) == 0

    summary = json.loads(capsys.readouterr().out)
    assert summary == {
        "batch_count": 1,
        "missing": {"claude-code": [], "codex": []},
        "prompt_count": 1,
        "session_count": 1,
    }
