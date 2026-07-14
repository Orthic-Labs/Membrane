"""Production manifest adjudication contract tests."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path

import pytest


MODULE_PATH = Path(__file__).parent / "adjudicate_manifest.py"


def _module():
    spec = importlib.util.spec_from_file_location("adjudicate_manifest", MODULE_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _record(record_id: str, *, status: str = "pending") -> dict:
    return {
        "id": record_id,
        "rule": "Always run the focused tests before reporting completion.",
        "category": "verification",
        "scope": "global",
        "status": status,
        "payload_sha256": "0" * 64,
        "evidence_excerpt": "from now on run focused tests before saying done",
        "source_ids": ["session-1"],
        "evidence_count": 1,
    }


def _manifest(records: list[dict]) -> dict:
    return {
        "schema_version": "1.2.0",
        "batch_id": "batch-1",
        "source_session_ids": ["session-1"],
        "created_at": "2026-07-14T00:00:00Z",
        "records": records,
    }


def test_build_cases_blinds_manifest_status_and_uses_bound_evidence():
    runner = _module()
    record = _record("adapt-verification-focused-tests-0123456789")
    controls = [{
        "id": "ex01", "excerpt": "Always verify tests.",
        "is_agent_preference": True,
    }]

    cases, expected = runner.build_cases(_manifest([record]), controls)

    candidate = next(item for item in cases if item["kind"] == "candidate")
    assert candidate == {
        "id": record["id"],
        "kind": "candidate",
        "rule": record["rule"],
        "category": "verification",
        "evidence": [record["evidence_excerpt"]],
        "source_session_count": 1,
    }
    assert "status" not in candidate
    assert expected == {"control-ex01": True}


def test_resolve_manifest_accepts_only_two_calibrated_admits():
    runner = _module()
    accepted = _record("adapt-verification-focused-tests-0123456789")
    disputed = _record("adapt-workflow-disputed-rule-abcdef0123")
    manifest = _manifest([accepted, disputed])
    panel = {
        "decision_policy": "dual-consensus",
        "models": {"m1": {"eligible": True}, "m2": {"eligible": True}},
        "eligible_models": ["m1", "m2"],
        "candidates": {
            accepted["id"]: {
                "status": "admitted", "reason": "two calibrated corroborators",
                "hard_flags": [], "votes": {},
            },
            disputed["id"]: {
                "status": "quarantined", "reason": "insufficient calibrated consensus",
                "hard_flags": [], "votes": {},
            },
        },
    }

    resolved, audit = runner.resolve_manifest(manifest, panel)

    by_id = {item["id"]: item for item in resolved["records"]}
    assert by_id[accepted["id"]]["status"] == "accepted"
    assert by_id[accepted["id"]]["needs_review"] is False
    assert by_id[disputed["id"]]["status"] == "rejected"
    assert by_id[disputed["id"]]["needs_review"] is True
    assert by_id[accepted["id"]]["payload_sha256"] == "0" * 64
    assert audit["counts"] == {"accepted": 1, "rejected": 1, "needs_review": 1}
    assert audit["decision_policy"] == "dual-consensus"


def test_calibrated_primary_accepts_admit_and_fails_closed_without_calibration():
    runner = _module()

    class Panel:
        HARD_FLAGS = {"permission_expanding", "authority_conflict"}

        @staticmethod
        def _calibrate(_votes, _expected):
            return {"precision": 1.0, "recall": 0.7, "eligible": True}

        @staticmethod
        def _counts_by_arm(candidates):
            return {"production-manifest": {
                status: sum(item["status"] == status for item in candidates.values())
                for status in ("admitted", "quarantined", "rejected")
            }}

    votes = {"minimax": {
        "candidate-a": {"verdict": "admit", "flags": [], "reason": "durable"},
        "candidate-b": {
            "verdict": "admit", "flags": ["permission_expanding"],
            "reason": "unsafe",
        },
    }}
    report = runner.aggregate_primary(
        panel_module=Panel, votes=votes, expected_controls={},
        candidate_ids={"candidate-a", "candidate-b"}, primary_model="minimax",
    )

    assert report["candidates"]["candidate-a"]["status"] == "admitted"
    assert report["candidates"]["candidate-b"]["status"] == "quarantined"


def test_missing_or_failed_panel_decision_rejects_closed():
    runner = _module()
    record = _record("adapt-verification-focused-tests-0123456789")
    resolved, audit = runner.resolve_manifest(
        _manifest([record]),
        {"models": {}, "eligible_models": [], "candidates": {}},
    )

    assert resolved["records"][0]["status"] == "rejected"
    assert resolved["records"][0]["needs_review"] is True
    assert audit["decisions"][0]["reason"] == "missing panel decision"


def test_write_outputs_is_atomic_and_preserves_input(tmp_path):
    runner = _module()
    pending = _manifest([_record("adapt-verification-focused-tests-0123456789")])
    input_path = tmp_path / "pending.json"
    input_path.write_text(json.dumps(pending), encoding="utf-8")
    original = input_path.read_bytes()
    output_path = tmp_path / "resolved.json"
    audit_path = tmp_path / "adjudication.json"

    resolved = _manifest([_record(
        "adapt-verification-focused-tests-0123456789", status="accepted"
    )])
    runner.write_outputs(output_path, audit_path, resolved, {"counts": {}})

    assert input_path.read_bytes() == original
    assert json.loads(output_path.read_text(encoding="utf-8"))["records"][0]["status"] == "accepted"
    assert json.loads(audit_path.read_text(encoding="utf-8"))["counts"] == {}


def test_output_paths_must_not_alias_input_or_each_other(tmp_path):
    runner = _module()
    pending = tmp_path / "pending.json"
    audit = tmp_path / "audit.json"

    with pytest.raises(ValueError, match="distinct paths"):
        runner.validate_distinct_paths(pending, pending, audit)
    with pytest.raises(ValueError, match="distinct paths"):
        runner.validate_distinct_paths(pending, audit, audit)


def test_calibration_and_candidates_are_separate_and_resumable(tmp_path):
    runner = _module()
    panel = runner._load_phase3()
    cases = [
        {
            "id": "control-positive", "kind": "control",
            "rule": "Always verify tests.", "category": "verification",
            "evidence": ["Always verify tests."], "source_session_count": 1,
        },
        {
            "id": "candidate-a", "kind": "candidate",
            "rule": "Always inspect the rendered UI.", "category": "verification",
            "evidence": ["Always inspect the rendered UI."], "source_session_count": 1,
        },
    ]
    calls = []

    def fake_call(model_id, _system, user, _max_tokens):
        calls.append(model_id)
        items = json.loads(user)["items"]
        return json.dumps([
            {"id": item["id"], "verdict": "admit", "flags": [], "reason": "durable"}
            for item in items
        ])

    first = runner.run_calibrated_panel(
        panel_module=panel, cases=cases,
        expected_controls={"control-positive": True}, model_ids=["m1"],
        calibration_dir=tmp_path / "calibration", calls_dir=tmp_path / "candidate",
        call_fn=fake_call, scanner=lambda _text: True, chunk_size=1,
        max_provider_calls=2, max_tokens=4096, max_workers=2,
        resume=False, inter_call_delay_s=0,
    )
    assert first["ledger"] == {"provider_calls": 2, "cache_hits": 0,
                               "input_chars": first["ledger"]["input_chars"]}
    assert first["calibration_models"]["m1"]["eligible"] is True
    assert first["votes"]["m1"]["candidate-a"]["verdict"] == "admit"

    def forbidden(*_args):
        raise AssertionError("shared calibration and candidate calls must resume from cache")

    second = runner.run_calibrated_panel(
        panel_module=panel, cases=cases,
        expected_controls={"control-positive": True}, model_ids=["m1"],
        calibration_dir=tmp_path / "calibration", calls_dir=tmp_path / "candidate",
        call_fn=forbidden, scanner=lambda _text: True, chunk_size=1,
        max_provider_calls=2, max_tokens=4096, max_workers=2,
        resume=True, inter_call_delay_s=0,
    )
    assert second["ledger"] == {"provider_calls": 0, "cache_hits": 2, "input_chars": 0}
    assert calls == ["m1", "m1"]
