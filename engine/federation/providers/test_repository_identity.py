"""canonical_repository_id — the typed stores derive record IDs from this value,
so a fork in the slug silently breaks retrieval and supersession.

Regression guard for the 2026-07-17 defect: architect.py and audit.py sent
str(repo_root.resolve()) as repositoryId while stored records carry the scope
slug, and decision_provider filters by exact equality — so both lanes returned
zero candidates on every prompt while still fanning out.
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from providers import canonical_repository_id  # noqa: E402


def test_workspace_root_matches_stored_record_identity():
    # .audit/architect/decisions.jsonl rows carry "repositoryId": "D--Claude".
    assert canonical_repository_id(Path("D:/Claude")) == "D--Claude"


def test_nested_repo_matches_scope_rs_convention():
    # scope.rs: `D:\Claude\myproject` -> `D--Claude-myproject`.
    assert canonical_repository_id(Path("D:/Claude/heardright")) == "D--Claude-heardright"


def test_drive_letter_case_never_forks_identity():
    # scope.rs uppercases the drive token so `d--Claude` and `D--Claude` are one id.
    assert canonical_repository_id("d:/Claude") == canonical_repository_id("D:/Claude")


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
