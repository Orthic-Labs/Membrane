from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import doctor
import workspace_runtime


def test_doctor_scope_declares_blueprint_beacon_not_yet(capsys):
    assert doctor.main(["--scope"]) == 0
    payload = json.loads(capsys.readouterr().out)
    assert payload["product"] == "Orthic Morph Doctor"
    assert "multiwriter_conformance issue" in payload["implemented"]
    assert any("Blueprint" in item for item in payload["not_yet"])
    assert any("Beacon" in item for item in payload["not_yet"])


def test_doctor_without_args_prints_scope_and_usage(capsys):
    assert doctor.main([]) == 2
    out = capsys.readouterr()
    assert "Orthic Morph Doctor" in out.out
    assert "morph doctor" in out.err


def test_workspace_runtime_stubs_memright_port(monkeypatch):
    monkeypatch.setenv("MORPH_WORKSPACE_STUBS", "1")
    monkeypatch.setenv("MEMRIGHT_PORT", "41234")
    assert workspace_runtime.memright_port() == 41234


def test_workspace_runtime_stubs_session_inventory(monkeypatch):
    monkeypatch.setenv("MORPH_WORKSPACE_STUBS", "1")
    inv = workspace_runtime.context_session_inventory()
    assert inv.infer_candidate_client("codex", Path("x.jsonl")) == "codex"
