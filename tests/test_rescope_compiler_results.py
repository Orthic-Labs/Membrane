from __future__ import annotations

import importlib.util
from pathlib import Path


MODULE_PATH = Path(__file__).resolve().parents[1] / "eval" / "rescope_compiler_results.py"


def _module():
    spec = importlib.util.spec_from_file_location("rescope_compiler_results", MODULE_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_rescope_report_preserves_baseline_and_records_narrowing():
    module = _module()
    report = {
        "content_sha256": "before",
        "rules": [
            {
                "name": "baseline", "status": "baseline", "scope": "D--Claude",
                "scope_reason": "seeded-baseline", "rule": "Keep JSONL.",
            },
            {
                "name": "heardright", "status": "candidate", "scope": "D--Claude",
                "scope_reason": "evidence-scope", "authority_reason": "ok",
                "record_type": "operational_playbook", "rule": "Test HeardRight hashes.",
                "support": [{"session_id": "s1", "evidence": "HeardRight hashes"}],
            },
            {
                "name": "sellright", "status": "quarantined", "scope": "SellRight",
                "scope_reason": "scope-outside-workspace", "authority_reason": "ok",
                "record_type": "locked_decision", "rule": "Keep SellRight modular.",
                "support": [{"session_id": "s2", "evidence": "SellRight modular"}],
            },
        ],
    }
    rows = [
        {
            "session_id": "s1", "scope": "D--Claude",
            "text": "HeardRight hashes are required. SellRight is discussed elsewhere.",
        },
        {"session_id": "s2", "scope": "D--Claude", "text": "Keep SellRight modular."},
    ]
    catalog = {
        "D--Claude-heardright": "heardright",
        "D--Claude-sellright": "sellright",
    }

    output = module.rescope_report(
        report, rows=rows, scope_catalog=catalog, workspace_scope="D--Claude"
    )

    assert output["rules"][0] == report["rules"][0]
    assert output["rules"][1]["scope"] == "D--Claude-heardright"
    assert output["rules"][2]["scope"] == "D--Claude-sellright"
    assert output["rules"][2]["status"] == "candidate"
    assert output["rescope"]["changed_rule_count"] == 2
    assert output["rules_sha256"]
    assert output["content_sha256"] != "before"


def test_rescope_report_fails_closed_when_support_cannot_be_grounded():
    module = _module()
    report = {"rules": [{
        "name": "bad", "status": "candidate", "scope": "D--Claude",
        "scope_reason": "evidence-scope", "authority_reason": "ok",
        "record_type": "standing_preference", "rule": "Use JSONL.",
        "support": [{"session_id": "missing", "evidence": "Use JSONL"}],
    }]}

    output = module.rescope_report(
        report, rows=[], scope_catalog={}, workspace_scope="D--Claude"
    )

    assert output["rules"][0]["status"] == "quarantined"
    assert output["rules"][0]["scope_reason"] == "unsupported-evidence-rescope"
