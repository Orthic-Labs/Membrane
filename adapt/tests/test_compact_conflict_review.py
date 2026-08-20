from __future__ import annotations

import importlib.util
import json
from pathlib import Path

import pytest


MODULE_PATH = Path(__file__).resolve().parents[1] / "eval" / "run_compact_conflict_review.py"


def _module():
    spec = importlib.util.spec_from_file_location("run_compact_conflict_review", MODULE_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _response(names):
    return json.dumps({
        "batch_id": "batch-001",
        "reviews": [
            {"rule_name": name, "verdict": "safe", "findings": []}
            for name in names
        ],
        "reviewed_rule_count": len(names),
    })


def test_validate_response_requires_exact_coverage():
    module = _module()

    parsed = module.validate_response(
        _response(["a", "b"]), batch_id="batch-001", expected_names=["a", "b"]
    )

    assert parsed["reviewed_rule_count"] == 2
    with pytest.raises(ValueError, match="missing or extra"):
        module.validate_response(
            _response(["a"]), batch_id="batch-001", expected_names=["a", "b"]
        )


def test_validate_worker_results_preserves_provider_and_parse_failures():
    module = _module()
    packet_manifest = [{"batch_id": "batch-001", "expected_names": ["a"]}]
    workers = [
        {"id": "minimax-batch-001", "ok": True, "output": _response(["a"])},
        {"id": "gptoss-batch-001", "ok": True, "output": _response([])},
        {"id": "mistral-batch-001", "ok": False, "error": "HTTP 413"},
    ]

    result = module.validate_worker_results(workers, packet_manifest)

    assert result["job_count"] == 3
    assert result["valid_count"] == 1
    assert result["invalid_count"] == 2
    assert "HTTP 413" in result["jobs"][2]["error"]


def test_flagged_review_requires_a_finding():
    module = _module()
    raw = json.dumps({
        "batch_id": "batch-001",
        "reviews": [{"rule_name": "a", "verdict": "flagged", "findings": []}],
        "reviewed_rule_count": 1,
    })

    with pytest.raises(ValueError, match="no findings"):
        module.validate_response(raw, batch_id="batch-001", expected_names=["a"])


def test_load_worker_results_recovers_stdout_from_failed_wrapper(tmp_path):
    module = _module()
    path = tmp_path / "wrapper.json"
    expected = [{"id": "x", "ok": False, "error": "quota"}]
    path.write_text(json.dumps({
        "worker_error": "exit 1", "worker_stdout": json.dumps(expected),
    }), encoding="utf-8")

    assert module.load_worker_results(path) == expected
