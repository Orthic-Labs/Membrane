"""Quality-gate runner cache tests."""
from __future__ import annotations

import sys
from pathlib import Path

TESTS_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(TESTS_DIR))

import run_quality_gate as gate


def test_cached_lane_call_reuses_exact_success(tmp_path):
    calls = []

    def fake_response(system, user, **kwargs):
        calls.append((system, user, kwargs))
        return {
            "text": "[]",
            "model": "MiniMax-M3",
            "stop_reason": "end_turn",
            "stop_sequence": None,
            "usage": {"input_tokens": 10, "output_tokens": 1},
        }

    first = gate.CachedLaneCall(
        tmp_path, lane="minimax", max_provider_calls=1,
        call_response=fake_response,
    )
    assert first("system", "user") == "[]"
    assert first.provider_calls == 1

    second = gate.CachedLaneCall(
        tmp_path, lane="minimax", max_provider_calls=0,
        call_response=fake_response,
    )
    assert second("system", "user") == "[]"
    assert second.provider_calls == 0
    assert second.cache_hits == 1
    assert len(calls) == 1


def test_cached_lane_call_refuses_truncated_response(tmp_path):
    def truncated(system, user, **kwargs):
        return {
            "text": "[",
            "model": "MiniMax-M3",
            "stop_reason": "max_tokens",
            "usage": {},
        }

    caller = gate.CachedLaneCall(
        tmp_path, lane="minimax", max_provider_calls=1,
        call_response=truncated,
    )

    try:
        caller("system", "user")
    except RuntimeError as exc:
        assert "truncated" in str(exc)
    else:
        raise AssertionError("expected truncated provider response to fail")


def test_evaluate_does_not_count_provider_failure_as_true_negative():
    def failed(_system, _user):
        raise ConnectionError("provider unavailable")

    result = gate.evaluate(
        [{"id": "negative", "excerpt": "one-off task", "is_preference": False}],
        lane="local",
        run_llm=failed,
    )

    assert result["tn"] == 0
    assert result["infrastructure_failures"] == 1
