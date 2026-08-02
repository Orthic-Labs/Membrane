"""Semantic corroboration panel contract tests."""
from __future__ import annotations

import importlib.util
import json
import threading
from pathlib import Path

import pytest


MODULE_PATH = Path(__file__).parent / "eval" / "run_phase3_corroboration.py"


def _module():
    assert MODULE_PATH.is_file(), "Phase 3 corroboration runner must exist"
    spec = importlib.util.spec_from_file_location("run_phase3_corroboration", MODULE_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_build_cases_blinds_arm_and_attaches_grounded_evidence():
    runner = _module()
    results = {"arms": {"current_morph": {"candidates": [{
        "id": "candidate-1",
        "rule": "Always rerun audit until clean.",
        "category": "workflow",
        "supporting_observation_ids": ["obs-1"],
    }]}}}
    observations = {"observations": [{
        "observation_id": "obs-1",
        "evidence": "audit fix means rerun until clean",
        "source_session_id": "session-1",
    }]}
    labels = [{
        "id": "ex01", "excerpt": "Use Fable for architecture.",
        "is_agent_preference": True,
    }]

    cases, ownership, expected = runner.build_cases(results, observations, labels)

    candidate = next(item for item in cases if item["id"] == "candidate-1")
    assert candidate == {
        "id": "candidate-1",
        "kind": "candidate",
        "rule": "Always rerun audit until clean.",
        "category": "workflow",
        "evidence": ["audit fix means rerun until clean"],
        "source_session_count": 1,
    }
    assert "arm" not in candidate
    assert ownership == {"candidate-1": "current_morph"}
    assert expected == {"control-ex01": True}


def test_parse_verdicts_requires_exact_ids_and_known_schema():
    runner = _module()
    raw = json.dumps([{
        "id": "a",
        "verdict": "admit",
        "flags": [],
        "reason": "explicit standing agent behavior",
    }])

    parsed = runner.parse_verdicts(raw, {"a"})
    assert parsed["a"]["verdict"] == "admit"

    unknown = runner.parse_verdicts(
        '[{"id":"a","verdict":"admit","flags":["episodic"],"reason":"x"}]',
        {"a"},
    )
    assert unknown["a"]["verdict"] == "quarantine"
    assert unknown["a"]["flags"] == ["ambiguous"]

    single_flag = runner.parse_verdicts(
        '[{"id":"a","verdict":"reject","flags":"task_specific","reason":"x"}]',
        {"a"},
    )
    assert single_flag["a"]["flags"] == ["task_specific"]

    with pytest.raises(ValueError, match="missing or extra"):
        runner.parse_verdicts(raw, {"a", "b"})
    with pytest.raises(ValueError, match="unknown verdict"):
        runner.parse_verdicts(
            '[{"id":"a","verdict":"maybe","flags":[],"reason":"x"}]', {"a"}
        )

    duplicate_same_decision = json.dumps([
        {"id": "a", "verdict": "reject", "flags": ["task_specific"], "reason": "one"},
        {"id": "a", "verdict": "reject", "flags": ["task_specific"], "reason": "two"},
    ])
    assert runner.parse_verdicts(duplicate_same_decision, {"a"})["a"]["verdict"] == "reject"
    with pytest.raises(ValueError, match="conflicting duplicate"):
        runner.parse_verdicts(json.dumps([
            {"id": "a", "verdict": "reject", "flags": [], "reason": "one"},
            {"id": "a", "verdict": "admit", "flags": [], "reason": "two"},
        ]), {"a"})


def test_parse_verdicts_accepts_exact_jsonl_objects():
    runner = _module()
    raw = "\n".join([
        json.dumps({
            "id": "a", "verdict": "admit", "flags": [], "reason": "durable"
        }),
        json.dumps({
            "id": "b", "verdict": "reject", "flags": ["task_specific"],
            "reason": "one-off",
        }),
    ])

    parsed = runner.parse_verdicts(raw, {"a", "b"})

    assert parsed["a"]["verdict"] == "admit"
    assert parsed["b"]["flags"] == ["task_specific"]


def test_parse_verdicts_accepts_single_items_wrapper():
    runner = _module()
    raw = json.dumps({"items": [{
        "id": "a",
        "verdict": "reject",
        "flags": ["task_specific"],
        "reason": "one-off",
    }]})

    parsed = runner.parse_verdicts(raw, {"a"})

    assert parsed["a"]["verdict"] == "reject"


def test_parse_verdicts_repairs_only_bare_allowed_flag_tokens():
    runner = _module()
    raw = '[{"id":"a","verdict":"reject","flags":[task_specific],"reason":"one-off"}]'

    parsed = runner.parse_verdicts(raw, {"a"})

    assert parsed["a"]["flags"] == ["task_specific"]
    with pytest.raises(ValueError):
        runner.parse_verdicts(
            '[{"id":"a","verdict":"reject","flags":[invented_flag],"reason":"x"}]',
            {"a"},
        )


def test_aggregate_requires_two_calibrated_votes_and_hard_flags_quarantine():
    runner = _module()
    expected = {"control-positive": True, "control-negative": False}
    votes = {
        "m1": {
            "control-positive": {"verdict": "admit", "flags": [], "reason": ""},
            "control-negative": {"verdict": "reject", "flags": [], "reason": ""},
            "safe": {"verdict": "admit", "flags": [], "reason": ""},
            "risky": {"verdict": "admit", "flags": [], "reason": ""},
        },
        "m2": {
            "control-positive": {"verdict": "admit", "flags": [], "reason": ""},
            "control-negative": {"verdict": "reject", "flags": [], "reason": ""},
            "safe": {"verdict": "admit", "flags": [], "reason": ""},
            "risky": {"verdict": "admit", "flags": ["permission_expanding"], "reason": ""},
        },
    }

    report = runner.aggregate_votes(
        votes=votes,
        expected_controls=expected,
        candidate_ids={"safe", "risky"},
        ownership={"safe": "current_morph", "risky": "bounded_full_set"},
    )

    assert report["models"]["m1"]["eligible"] is True
    assert report["candidates"]["safe"]["status"] == "admitted"
    assert report["candidates"]["risky"]["status"] == "quarantined"
    assert report["candidates"]["risky"]["hard_flags"] == ["permission_expanding"]


def test_panel_calls_are_content_addressed_and_resume_without_provider(tmp_path):
    runner = _module()
    cases = [{
        "id": f"item-{index}", "kind": "control", "rule": "rule",
        "category": "unknown", "evidence": ["evidence"],
        "source_session_count": 1,
    } for index in range(3)]
    calls = []

    def fake_call(model_id, _system, user, _max_tokens):
        calls.append(model_id)
        items = json.loads(user)["items"]
        return json.dumps([{
            "id": item["id"], "verdict": "reject", "flags": [], "reason": "test"
        } for item in items])

    first = runner.run_panel(
        cases=cases, model_ids=["m1"], output_dir=tmp_path,
        call_fn=fake_call, scanner=lambda _text: True, chunk_size=2,
        max_provider_calls=2, resume=False,
    )
    assert first["ledger"]["provider_calls"] == 2
    assert calls == ["m1", "m1"]

    def forbidden(*_args):
        raise AssertionError("resume must not call provider")

    second = runner.run_panel(
        cases=cases, model_ids=["m1"], output_dir=tmp_path,
        call_fn=forbidden, scanner=lambda _text: True, chunk_size=2,
        max_provider_calls=2, resume=True,
    )
    assert second["ledger"] == {"provider_calls": 0, "cache_hits": 2, "input_chars": 0}
    assert second["votes"] == first["votes"]


def test_panel_retries_invalid_json_within_provider_call_ceiling(tmp_path):
    runner = _module()
    cases = [{
        "id": "case-1", "kind": "control", "rule": "rule",
        "category": "unknown", "evidence": ["evidence"],
        "source_session_count": 1,
    }]
    calls = []

    def flaky_call(_model_id, _system, user, _max_tokens):
        calls.append(1)
        if len(calls) == 1:
            return '[{"id":"item-001","verdict":"reject"'
        return json.dumps([{
            "id": "item-001", "verdict": "reject", "flags": [], "reason": "valid"
        }])

    result = runner.run_panel(
        cases=cases, model_ids=["m1"], output_dir=tmp_path,
        call_fn=flaky_call, scanner=lambda _text: True, chunk_size=1,
        max_provider_calls=2, max_workers=1, resume=False,
    )

    assert len(calls) == 2
    assert result["ledger"]["provider_calls"] == 2
    assert result["votes"]["m1"]["case-1"]["verdict"] == "reject"


def test_panel_blinds_case_identity_and_remaps_votes(tmp_path):
    runner = _module()
    cases = [
        {
            "id": "control-ex49", "kind": "control",
            "rule": "Always verify the actual interface.",
            "category": "verification", "evidence": ["Always verify the actual interface."],
            "source_session_count": 1,
        },
        {
            "id": "morph-workflow-focused-tests-0123456789", "kind": "candidate",
            "rule": "Always run focused tests.", "category": "verification",
            "evidence": ["from now on run focused tests"], "source_session_count": 1,
        },
    ]

    def fake_call(_model_id, system, user, _max_tokens):
        assert "control" not in system.lower()
        payload = json.loads(user)["items"]
        assert all("kind" not in item for item in payload)
        assert all(item["id"].startswith("item-") for item in payload)
        assert "control-ex49" not in user
        assert "morph-workflow-focused-tests" not in user
        return json.dumps([
            {"id": item["id"], "verdict": "admit", "flags": [], "reason": "durable"}
            for item in payload
        ])

    result = runner.run_panel(
        cases=cases, model_ids=["m1"], output_dir=tmp_path,
        call_fn=fake_call, scanner=lambda _text: True, chunk_size=2,
        max_provider_calls=1, max_workers=1, resume=False,
    )

    assert set(result["votes"]["m1"]) == {
        "control-ex49", "morph-workflow-focused-tests-0123456789",
    }


def test_panel_overlaps_independent_provider_calls(tmp_path):
    runner = _module()
    cases = [{
        "id": f"case-{index}", "kind": "control", "rule": "rule",
        "category": "unknown", "evidence": ["evidence"],
        "source_session_count": 1,
    } for index in range(4)]
    barrier = threading.Barrier(4)

    def fake_call(_model_id, _system, user, _max_tokens):
        barrier.wait(timeout=2)
        item = json.loads(user)["items"][0]
        return json.dumps([{
            "id": item["id"], "verdict": "reject", "flags": [], "reason": "test",
        }])

    result = runner.run_panel(
        cases=cases, model_ids=["m1"], output_dir=tmp_path,
        call_fn=fake_call, scanner=lambda _text: True, chunk_size=1,
        max_provider_calls=4, max_workers=4, resume=False,
    )

    assert result["ledger"]["provider_calls"] == 4
    assert len(result["votes"]["m1"]) == 4


def test_cli_dry_run_reports_bounded_calls_without_writing(tmp_path, capsys):
    runner = _module()
    phase2 = tmp_path / "phase2"
    phase2.mkdir()
    (phase2 / "results.json").write_text(json.dumps({
        "arms": {"current_morph": {"candidates": []}}
    }), encoding="utf-8")
    (phase2 / "observations.json").write_text(
        json.dumps({"observations": []}), encoding="utf-8"
    )
    labels = tmp_path / "labels.jsonl"
    labels.write_text(json.dumps({
        "id": "ex01", "excerpt": "Always verify tests.",
        "is_agent_preference": True,
    }) + "\n", encoding="utf-8")
    out = tmp_path / "out"

    assert runner.main([
        "--phase2-dir", str(phase2), "--labels", str(labels),
        "--out", str(out), "--models", "minimax-m3-direct", "--dry-run",
    ]) == 0

    report = json.loads(capsys.readouterr().out)
    assert report["planned_provider_calls"] == 1
    assert report["candidate_count"] == 0
    assert report["control_count"] == 1
    assert not out.exists()
