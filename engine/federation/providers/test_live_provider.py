from __future__ import annotations

import hashlib
import subprocess
from pathlib import Path

from federation.providers import live


def _git(repo: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(repo), *args],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def _sha(body: bytes) -> str:
    return "sha256:" + hashlib.sha256(body).hexdigest()


def _repo(tmp_path: Path) -> tuple[Path, str]:
    _git(tmp_path, "init", "--quiet")
    _git(tmp_path, "config", "user.email", "test@example.invalid")
    _git(tmp_path, "config", "user.name", "Test")
    source = tmp_path / "src" / "app.py"
    source.parent.mkdir()
    source.write_bytes(b"print('committed')\n")
    _git(tmp_path, "add", "src/app.py")
    _git(tmp_path, "commit", "-m", "fixture", "--quiet")
    return source, _git(tmp_path, "rev-parse", "HEAD")


def test_live_provider_emits_committed_snapshot_plus_verified_dirty_overlay(tmp_path: Path):
    source, base_commit = _repo(tmp_path)
    committed = source.read_bytes()
    source.write_bytes(b"print('working')\n")
    working = source.read_bytes()
    overlay_digest = "sha256:" + "b" * 64

    candidates, generation, warnings = live.produce(
        tmp_path,
        base_commit=base_commit,
        overlay_digest=overlay_digest,
        overlay_entries=[
            {"path": "src/app.py", "status": " M", "contentHash": _sha(working)}
        ],
    )

    assert warnings == []
    assert generation == overlay_digest
    assert [candidate["freshnessClass"] for candidate in candidates] == [
        "committed_snapshot",
        "dirty_overlay",
    ]
    snapshot, overlay = candidates
    assert snapshot["sourceHash"] == _sha(committed)
    assert overlay["sourceHash"] == _sha(working)
    assert all(candidate["baseCommit"] == base_commit for candidate in candidates)
    assert all(candidate["overlayDigest"] == overlay_digest for candidate in candidates)
    assert snapshot["resolver"].startswith(f"git show {base_commit}:")
    assert overlay["resolver"].startswith("git diff --no-ext-diff")


def test_live_provider_caps_paths_deterministically_and_warns(tmp_path: Path):
    _source, base_commit = _repo(tmp_path)
    entries = []
    for name in ("z.py", "a.py"):
        path = tmp_path / name
        path.write_text(name, encoding="utf-8")
        entries.append({"path": name, "status": "??", "contentHash": _sha(path.read_bytes())})

    candidates, _generation, warnings = live.produce(
        tmp_path,
        base_commit=base_commit,
        overlay_digest="sha256:" + "b" * 64,
        overlay_entries=entries,
        max_paths=1,
    )

    assert [candidate["sourceRef"] for candidate in candidates] == ["a.py"]
    assert warnings == [
        {
            "provider": "live",
            "kind": "dirty_overlay_truncated",
            "severity": "warning",
            "message": "dirty overlay capped at 1 of 2 paths",
        }
    ]


def test_live_provider_rejects_overlay_changed_after_freshness_verdict(tmp_path: Path):
    source, base_commit = _repo(tmp_path)
    old_hash = _sha(source.read_bytes())
    source.write_text("changed after verdict\n", encoding="utf-8")

    candidates, _generation, warnings = live.produce(
        tmp_path,
        base_commit=base_commit,
        overlay_digest="sha256:" + "b" * 64,
        overlay_entries=[
            {"path": "src/app.py", "status": " M", "contentHash": old_hash}
        ],
    )

    assert candidates == []
    assert warnings[0]["kind"] == "dirty_overlay_changed"


def test_live_provider_rejects_control_characters_in_overlay_paths(tmp_path: Path):
    _source, base_commit = _repo(tmp_path)

    candidates, _generation, warnings = live.produce(
        tmp_path,
        base_commit=base_commit,
        overlay_digest="sha256:" + "b" * 64,
        overlay_entries=[
            {"path": "src/app.py\nresolver-injection", "status": "??", "contentHash": "sha256:" + "0" * 64}
        ],
    )

    assert candidates == []
    assert warnings == [
        {
            "provider": "live",
            "kind": "dirty_overlay_path_rejected",
            "severity": "warning",
            "message": "unsafe overlay path omitted",
        }
    ]


def test_live_provider_rejects_shell_metacharacters_in_overlay_paths(tmp_path: Path):
    _source, base_commit = _repo(tmp_path)
    unsafe = tmp_path / "src" / "app.py;echo-pwned"
    unsafe.write_text("unsafe resolver path\n", encoding="utf-8")

    candidates, _generation, warnings = live.produce(
        tmp_path,
        base_commit=base_commit,
        overlay_digest="sha256:" + "b" * 64,
        overlay_entries=[
            {"path": "src/app.py;echo-pwned", "status": "??", "contentHash": _sha(unsafe.read_bytes())}
        ],
    )

    assert candidates == []
    assert warnings[0]["kind"] == "dirty_overlay_path_rejected"


def test_live_provider_omits_oversized_historical_blob_without_buffering_it(tmp_path: Path):
    source, base_commit = _repo(tmp_path)
    source.write_bytes(b"x" * (live.MAX_HISTORICAL_BLOB_BYTES + 1))
    _git(tmp_path, "add", "src/app.py")
    _git(tmp_path, "commit", "-m", "oversized", "--quiet")
    base_commit = _git(tmp_path, "rev-parse", "HEAD")
    source.write_text("small overlay\n", encoding="utf-8")

    candidates, _generation, warnings = live.produce(
        tmp_path,
        base_commit=base_commit,
        overlay_digest="sha256:" + "b" * 64,
        overlay_entries=[
            {"path": "src/app.py", "status": " M", "contentHash": _sha(source.read_bytes())}
        ],
    )

    assert [candidate["freshnessClass"] for candidate in candidates] == ["dirty_overlay"]
    assert warnings[0]["kind"] == "historical_blob_oversized"


def test_live_provider_preserves_committed_baseline_for_deleted_path(tmp_path: Path):
    source, base_commit = _repo(tmp_path)
    committed = source.read_bytes()
    source.unlink()
    deleted_hash = _sha(b"deleted:src/app.py")

    candidates, _generation, warnings = live.produce(
        tmp_path,
        base_commit=base_commit,
        overlay_digest="sha256:" + "b" * 64,
        overlay_entries=[
            {"path": "src/app.py", "status": " D", "contentHash": deleted_hash}
        ],
    )

    assert warnings == []
    assert [candidate["freshnessClass"] for candidate in candidates] == [
        "committed_snapshot",
        "dirty_overlay",
    ]
    assert candidates[0]["sourceHash"] == _sha(committed)
    assert candidates[1]["sourceHash"] == deleted_hash


def test_live_provider_uses_old_path_for_renamed_snapshot(tmp_path: Path):
    source, base_commit = _repo(tmp_path)
    committed = source.read_bytes()
    renamed = source.with_name("renamed.py")
    source.rename(renamed)

    candidates, _generation, warnings = live.produce(
        tmp_path,
        base_commit=base_commit,
        overlay_digest="sha256:" + "b" * 64,
        overlay_entries=[{
            "path": "src/renamed.py",
            "oldPath": "src/app.py",
            "status": "R ",
            "contentHash": _sha(renamed.read_bytes()),
        }],
    )

    assert warnings == []
    assert candidates[0]["sourceRef"].startswith("src/app.py@")
    assert candidates[0]["sourceHash"] == _sha(committed)
    assert candidates[1]["sourceRef"] == "src/renamed.py"


def test_live_provider_rejects_deleted_path_recreated_after_verdict(tmp_path: Path):
    source, base_commit = _repo(tmp_path)
    source.unlink()
    verdict_hash = _sha(b"deleted")
    source.write_text("recreated after verdict\n", encoding="utf-8")

    candidates, _generation, warnings = live.produce(
        tmp_path,
        base_commit=base_commit,
        overlay_digest="sha256:" + "b" * 64,
        overlay_entries=[
            {"path": "src/app.py", "status": " D", "contentHash": verdict_hash}
        ],
    )

    assert candidates == []
    assert warnings[0]["kind"] == "dirty_overlay_changed"


def test_prompt_fast_mode_uses_trusted_snapshot_without_file_or_git_io(
    monkeypatch, tmp_path: Path
):
    monkeypatch.setattr(
        live,
        "_file_sha256",
        lambda *_args: (_ for _ in ()).throw(AssertionError("prompt path read a file")),
    )
    monkeypatch.setattr(
        live,
        "_git_blob_sha256",
        lambda *_args: (_ for _ in ()).throw(AssertionError("prompt path ran git")),
    )
    expected_hash = "sha256:" + "a" * 64

    candidates, generation, warnings = live.produce(
        tmp_path,
        base_commit="b" * 40,
        overlay_digest="sha256:" + "c" * 64,
        overlay_entries=[
            {"path": "src/app.py", "status": " M", "contentHash": expected_hash}
        ],
        prompt_fast=True,
    )

    assert warnings == []
    assert generation == "sha256:" + "c" * 64
    assert len(candidates) == 1
    assert candidates[0]["id"] == "git-overlay:src/app.py"
    assert candidates[0]["sourceHash"] == expected_hash
