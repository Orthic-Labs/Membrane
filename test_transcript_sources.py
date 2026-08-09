from __future__ import annotations

import hashlib
import time
import tracemalloc
from pathlib import Path

import pytest

import transcript_sources as sources


def _write(path: Path, rows: list[dict]) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(__import__("json").dumps(row) for row in rows) + "\n", encoding="utf-8")
    return path


def _spec(host: str) -> sources.SourceSpec:
    return sources.SourceSpec(host, host, ".", ("*.jsonl",), True)


def test_discover_is_stat_only(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    path = _write(tmp_path / ".codex/sessions/2026/08/10/thread.jsonl", [{"payload": {"session_id": "s"}}])
    opened = []
    original_open = Path.open

    def spy(self: Path, *args, **kwargs):
        opened.append(self)
        return original_open(self, *args, **kwargs)

    monkeypatch.setattr(Path, "open", spy)
    found = sources.discover(tmp_path)
    assert len(found) == 1
    assert found[0].path == path.resolve()
    assert found[0].size == path.stat().st_size
    assert found[0].path_rel == ".codex/sessions/2026/08/10/thread.jsonl"
    assert found[0].metadata is None
    assert not opened


@pytest.mark.parametrize("spawn", ["parent-thread", {"parent_thread_id": "parent-thread"}])
def test_codex_subagent_parent_forms(spawn: object, tmp_path: Path) -> None:
    path = _write(tmp_path / "thread.jsonl", [
        {"payload": {"session_id": "child", "cwd": "/repo"}},
        {"type": "response_item", "payload": {"source": {"subagent": {"thread_spawn": spawn}}}},
    ])
    metadata = sources.inspect_metadata(_spec("codex"), path)
    assert metadata.thread_source == "subagent"
    assert metadata.exclusion_reason == "structured-subagent-parent"


def test_codex_exec_root_and_active_precedes_open(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    path = _write(tmp_path / ".codex/sessions/2026/08/10/active-thread.jsonl", [
        {"payload": {"session_id": "root"}},
        {"type": "response_item", "payload": {"kind": "codex_exec"}},
    ])
    metadata = sources.inspect_metadata(_spec("codex"), path)
    assert (metadata.thread_source, metadata.exclusion_reason) == ("root", "codex-exec")
    descriptor = sources.discover(tmp_path)[0]
    monkeypatch.setenv("MORPH_ACTIVE_CODEX_THREAD_IDS", "other-thread,active-thread")
    original_open = Path.open
    monkeypatch.setattr(Path, "open", lambda *_args, **_kwargs: (_ for _ in ()).throw(AssertionError("opened active source")))
    selected, quarantined = sources.select_sources([descriptor])
    assert not selected and quarantined[0].metadata.exclusion_reason == "active-session"
    monkeypatch.setattr(Path, "open", original_open)


def test_codex_root_parser_stops_after_first_conversational_row(tmp_path: Path) -> None:
    path = _write(tmp_path / "root.jsonl", [
        {"payload": {"session_id": "root", "cwd": "/repo"}},
        {"type": "response_item", "payload": {"source": "root"}},
        {"payload": {"session_id": "must-not-be-read"}},
    ])
    metadata = sources.inspect_metadata(_spec("codex"), path)
    assert (metadata.session_id, metadata.thread_source, metadata.exclusion_reason) == ("root", "root", "")


def test_metadata_incomplete_fails_closed_and_unsupported_stays_stat_only(tmp_path: Path) -> None:
    path = _write(tmp_path / "unknown.jsonl", [{"type": "response_item", "payload": {"cwd": "/repo"}}])
    assert sources.inspect_metadata(_spec("codex"), path).exclusion_reason == "metadata-incomplete"
    unsupported = sources.TranscriptSource(sources.SourceSpec("x", None, ".", (), False), path, "unknown.jsonl", 1, 1, "x:unknown")
    selected, quarantined = sources.select_sources([unsupported])
    assert not selected and quarantined[0].metadata.exclusion_reason == "unsupported-host"


def test_claude_cwd_transitions_are_bounded(tmp_path: Path) -> None:
    path = _write(tmp_path / "claude.jsonl", [
        {"sessionId": "s", "cwd": f"/repo/{index}"} for index in range(60)
    ])
    metadata = sources.inspect_metadata(_spec("claude_code"), path)
    assert len(metadata.cwd_by_row) == 50
    assert metadata.cwd_by_row[-1] == (50, "/repo/49")


def test_select_skips_learned_and_exclusions_without_using_limit(tmp_path: Path) -> None:
    active = _write(tmp_path / "active.jsonl", [{"payload": {"session_id": "active"}}])
    learned = _write(tmp_path / "learned.jsonl", [{"payload": {"session_id": "learned"}}])
    fresh = _write(tmp_path / "fresh.jsonl", [{"payload": {"session_id": "fresh"}}, {"type": "response_item"}])
    spec = _spec("codex")
    descriptors = [sources.TranscriptSource(spec, item, item.name, 1, 1, f"codex:{item.name}") for item in (active, learned, fresh)]
    selected, quarantined = sources.select_sources(descriptors, learned={"codex:learned.jsonl": "h"}, active_ids={"active"}, limit=1)
    assert [item.session_id for item in selected] == ["fresh"]
    assert quarantined[0].metadata.exclusion_reason == "active-session"


def test_source_hash_streams_one_megabyte_chunks(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    path = tmp_path / "large.jsonl"
    path.write_bytes(b"x" * (2 * 1024 * 1024 + 3))
    expected = "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()
    reads = []
    original_open = Path.open

    class Handle:
        def __init__(self, handle): self.handle = handle
        def __enter__(self): return self
        def __exit__(self, *args): self.handle.close()
        def read(self, size=-1):
            reads.append(size)
            return self.handle.read(size)

    monkeypatch.setattr(Path, "open", lambda self, *args, **kwargs: Handle(original_open(self, *args, **kwargs)))
    assert sources.source_hash(path) == expected
    assert reads and set(reads) == {1024 * 1024}


def test_sparse_200mb_discovery_and_header_inspection_stay_bounded(tmp_path: Path) -> None:
    path = _write(tmp_path / ".codex/sessions/2026/08/10/large.jsonl", [
        {"payload": {"session_id": "large", "cwd": "/repo"}},
        {"type": "response_item", "payload": {"source": "root"}},
    ])
    with path.open("r+b") as handle:
        handle.truncate(200 * 1024 * 1024)

    tracemalloc.start()
    started = time.monotonic()
    found = sources.discover(tmp_path)
    discover_elapsed = time.monotonic() - started
    _, discover_peak = tracemalloc.get_traced_memory()
    tracemalloc.reset_peak()
    started = time.monotonic()
    metadata = sources.inspect_metadata(_spec("codex"), found[0].path)
    inspect_elapsed = time.monotonic() - started
    _, inspect_peak = tracemalloc.get_traced_memory()
    tracemalloc.stop()

    assert found[0].size == 200 * 1024 * 1024
    assert metadata.session_id == "large"
    assert discover_elapsed <= 2 and discover_peak <= 32 * 1024 * 1024
    assert inspect_elapsed <= 1 and inspect_peak <= 16 * 1024 * 1024
