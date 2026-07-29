from __future__ import annotations

from pathlib import Path, PureWindowsPath

from federation.providers import anchors


def _assert_raw_anchor(candidates: list[dict], anchor: str) -> None:
    assert len(candidates) == 1
    candidate = candidates[0]
    assert candidate["id"] == f"anchor:raw:{anchor}"
    assert candidate["sourceRef"] == anchor
    assert candidate["text"] == anchor


def _reject_resolution(*_args, **_kwargs):
    raise AssertionError("rejected path must not invoke Blueprint resolution")


def test_anchor_provider_reads_absolute_file_within_repo_root(tmp_path: Path):
    source = tmp_path / "docs" / "anchor.md"
    source.parent.mkdir()
    source.write_text("inside repository\n", encoding="utf-8")

    candidates = anchors.produce(tmp_path, [str(source)], "test")

    assert len(candidates) == 1
    assert candidates[0]["id"] == "anchor:file:docs/anchor.md"
    assert candidates[0]["sourceRef"] == "docs/anchor.md"
    assert candidates[0]["text"] == "inside repository\n"


def test_anchor_provider_serializes_windows_relative_path_with_forward_slashes(
    monkeypatch, tmp_path: Path
):
    class WindowsRelativeFile:
        def exists(self):
            return True

        def is_file(self):
            return True

        def read_text(self, **_kwargs):
            return "inside repository\n"

        def relative_to(self, _root):
            return PureWindowsPath("docs", "anchor.md")

    monkeypatch.setattr(
        anchors,
        "_repo_anchor_path",
        lambda *_args: (tmp_path, WindowsRelativeFile()),
    )

    candidates = anchors.produce(tmp_path, ["docs/anchor.md"], "test")

    assert candidates[0]["id"] == "anchor:file:docs/anchor.md"
    assert candidates[0]["sourceRef"] == "docs/anchor.md"


def test_anchor_provider_rejects_absolute_path_outside_repo_before_resolution(
    monkeypatch, tmp_path: Path
):
    repo_root = tmp_path / "repo"
    repo_root.mkdir()
    outside = tmp_path / "outside.txt"
    outside.write_text("outside secret", encoding="utf-8")
    monkeypatch.setattr(anchors.subprocess, "run", _reject_resolution)

    candidates = anchors.produce(repo_root, [str(outside)], "test")

    _assert_raw_anchor(candidates, str(outside))


def test_anchor_provider_rejects_traversal_and_symlink_escapes_before_resolution(
    monkeypatch, tmp_path: Path
):
    repo_root = tmp_path / "repo"
    repo_root.mkdir()
    outside = tmp_path / "outside.txt"
    outside.write_text("outside secret", encoding="utf-8")
    link = repo_root / "outside-link.txt"
    link.symlink_to(outside)
    monkeypatch.setattr(anchors.subprocess, "run", _reject_resolution)

    traversal = "../outside.txt"
    _assert_raw_anchor(anchors.produce(repo_root, [traversal], "test"), traversal)
    _assert_raw_anchor(anchors.produce(repo_root, [str(link)], "test"), str(link))
