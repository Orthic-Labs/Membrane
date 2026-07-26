"""canonical_repository_id — the typed stores derive record IDs from this value,
so a fork in the slug silently breaks retrieval and supersession.

Regression guard for the 2026-07-17 defect: architect.py and audit.py sent
str(repo_root.resolve()) as repositoryId while stored records carry the scope
slug, and decision_provider filters by exact equality — so both lanes returned
zero candidates on every prompt while still fanning out.
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from providers import canonical_repository_id  # noqa: E402

# A bare `D:/Claude` is an ABSOLUTE path only on Windows. On POSIX it is a
# relative path whose `.resolve()` is cwd-dependent, so the drive-letter slug
# assertions below can only hold on Windows. They stay as the Windows contract;
# `test_posix_*` covers the same nesting/stability contract on this platform.
windows_only = pytest.mark.skipif(
    os.name != "nt", reason="drive-letter paths are absolute only on Windows"
)


@windows_only
def test_workspace_root_matches_stored_record_identity():
    # .audit/architect/decisions.jsonl rows carry "repositoryId": "D--Claude".
    assert canonical_repository_id(Path("D:/Claude")) == "D--Claude"


@windows_only
def test_nested_repo_matches_scope_rs_convention():
    # scope.rs: `D:\Claude\myproject` -> `D--Claude-myproject`.
    assert canonical_repository_id(Path("D:/Claude/heardright")) == "D--Claude-heardright"


@windows_only
def test_drive_letter_case_never_forks_identity():
    # scope.rs uppercases the drive token so `d--Claude` and `D--Claude` are one id.
    assert canonical_repository_id("d:/Claude") == canonical_repository_id("D:/Claude")


def test_posix_workspace_root_matches_stored_record_identity(tmp_path):
    # The live Mac workspace slug is `Volumes-D-claude` (see recall heartbeat rows).
    expected = str(tmp_path.resolve()).replace("/", "-").strip("-")
    assert canonical_repository_id(tmp_path) == expected


def test_posix_nested_repo_appends_one_segment(tmp_path):
    nested = tmp_path / "heardright"
    assert canonical_repository_id(nested) == (
        canonical_repository_id(tmp_path) + "-heardright"
    )


def test_backslash_and_forward_slash_agree():
    assert canonical_repository_id("D:\\Claude") == canonical_repository_id("D:/Claude")


def test_never_returns_a_machine_path():
    # audit_store.derive_finding_id contract: "never a machine path".
    for probe in ("D:/Claude", "D:/Claude/heardright", "/home/user/repo"):
        got = canonical_repository_id(probe)
        assert ":" not in got
        assert "/" not in got
        assert "\\" not in got


def test_is_stable_across_calls():
    assert canonical_repository_id("D:/Claude") == canonical_repository_id("D:/Claude")
