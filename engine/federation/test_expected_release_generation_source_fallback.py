import subprocess
from pathlib import Path

from federation import gateway


def _git(repo_root: Path, args: list[str]) -> None:
    subprocess.run(["git", "-C", str(repo_root), *args], check=True, capture_output=True)


def _init_fake_repo(repo_root: Path) -> None:
    repo_root.mkdir(parents=True, exist_ok=True)
    _git(repo_root, ["init"])
    _git(repo_root, ["config", "user.email", "test@example.com"])
    _git(repo_root, ["config", "user.name", "test"])
    engine_dir = repo_root / "engine"
    engine_dir.mkdir()
    (engine_dir / "committed.txt").write_text("committed content\n", encoding="utf-8")
    _git(repo_root, ["add", "-A"])
    _git(repo_root, ["commit", "-m", "initial"])


def test_from_source_returns_valid_sha256_prefixed_generation(tmp_path: Path):
    repo_root = tmp_path / "fake-repo"
    _init_fake_repo(repo_root)

    generation = gateway._expected_release_generation_from_source(repo_root)

    assert generation is not None
    assert generation.startswith("sha256:")
    digest = generation.removeprefix("sha256:")
    assert len(digest) == 64
    assert all(char in "0123456789abcdef" for char in digest)


def test_from_source_is_deterministic_on_clean_tree(tmp_path: Path):
    repo_root = tmp_path / "fake-repo"
    _init_fake_repo(repo_root)

    first = gateway._expected_release_generation_from_source(repo_root)
    second = gateway._expected_release_generation_from_source(repo_root)

    assert first == second


def test_from_source_changes_when_tree_becomes_dirty(tmp_path: Path):
    repo_root = tmp_path / "fake-repo"
    _init_fake_repo(repo_root)

    clean = gateway._expected_release_generation_from_source(repo_root)

    (repo_root / "engine" / "untracked.txt").write_text("new file\n", encoding="utf-8")
    dirty = gateway._expected_release_generation_from_source(repo_root)

    assert dirty is not None
    assert dirty != clean


def test_resolve_source_repo_root_on_real_repo_has_engine_subdir():
    repo_root = gateway._resolve_source_repo_root()

    assert repo_root is not None
    assert (repo_root / "engine").is_dir()
