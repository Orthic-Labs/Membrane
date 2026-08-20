from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from adapt import doctor
from adapt import workspace_runtime


def test_doctor_scope_declares_cortex_forge_not_yet(capsys):
    assert doctor.main(["--scope"]) == 0
    payload = json.loads(capsys.readouterr().out)
    assert payload["product"] == "Orthic Adapt Doctor"
    assert "multiwriter_conformance issue" in payload["implemented"]
    assert any("Cortex" in item for item in payload["not_yet"])
    assert any("Forge" in item for item in payload["not_yet"])


def test_doctor_without_args_prints_scope_and_usage(capsys):
    assert doctor.main([]) == 2
    out = capsys.readouterr()
    assert "Orthic Adapt Doctor" in out.out
    assert "adapt doctor" in out.err


def test_workspace_runtime_cortex_port(monkeypatch):
    monkeypatch.setenv("CORTEX_PORT", "41234")
    assert workspace_runtime.cortex_port() == 41234


def test_workspace_runtime_session_inventory():
    inv = workspace_runtime.context_session_inventory()
    assert inv.infer_candidate_client("codex", Path("x.jsonl")) == "codex"
