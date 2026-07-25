from __future__ import annotations

import json
import time
from pathlib import Path

import pytest

from federation import gateway


@pytest.mark.parametrize(
    ("warning_kind", "expected_failure_kind"),
    [
        ("provider_timeout", "timeout"),
        ("provider_crash", "crash"),
        ("provider_unavailable", "unavailable"),
        ("authentication_failed", "authentication"),
        ("invalid_output", "invalid_output"),
        ("stale_snapshot", "stale_snapshot"),
        ("blueprint_generation_changed", "generation_mismatch"),
        ("budget_drop", "cancellation_budget_drop"),
        ("circuit_open", "circuit_open"),
    ],
)
def test_gateway_types_every_provider_warning_as_lane_local(
    warning_kind: str,
    expected_failure_kind: str,
    tmp_path: Path,
):
    ccs = gateway._merge_candidates(
        [(
            "git",
            [],
            [{
                "provider": "git",
                "kind": warning_kind,
                "severity": "warning",
                "message": "SECRET provider detail",
            }],
        )],
        {
            "indexedAt": "2026-07-17T00:00:00Z",
            "graphState": "dirty_overlay",
            "_providerElapsedMs": {"git": 1.25},
        },
        tmp_path,
        "taxonomy fixture",
        "trace-taxonomy",
        4096,
    )

    warning = ccs["_rightcontext"]["providerWarnings"][0]
    assert warning == {
        "provider": "git",
        "kind": warning_kind,
        "severity": "warning",
        "failureKind": expected_failure_kind,
        "failureScope": "lane_local",
    }
    assert "SECRET" not in json.dumps(ccs)


def test_gateway_exception_warning_is_typed_and_content_free():
    warning = gateway._safe_emit_warning("memright", TimeoutError("SECRET bearer detail"))

    assert warning == {
        "provider": "memright",
        "kind": "provider_failure",
        "severity": "warning",
        "failureKind": "timeout",
        "failureScope": "lane_local",
    }
    assert "SECRET" not in json.dumps(warning)


def test_bounded_fanout_returns_healthy_lanes_without_waiting_for_slow_provider():
    def fast():
        return "fast", [{"id": "fast:one"}], []

    def slow():
        time.sleep(0.25)
        return "slow", [{"id": "slow:late"}], []

    started = time.monotonic()
    results = gateway._collect_tasks_bounded(
        [("fast", fast), ("slow", slow)], timeout_s=0.03
    )

    assert time.monotonic() - started < 0.1
    fast_lane = next(result for result in results if result[0] == "fast")
    slow_lane = next(result for result in results if result[0] == "slow")
    assert fast_lane[1] == [{"id": "fast:one"}]
    assert slow_lane[1] == []
    assert slow_lane[2][0]["kind"] == "provider_timeout"
