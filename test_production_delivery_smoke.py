from __future__ import annotations

import importlib.util
import json
from pathlib import Path


SCRIPT = Path(__file__).resolve().parent / "eval" / "smoke_production_delivery.py"


def _load():
    spec = importlib.util.spec_from_file_location("production_delivery_smoke", SCRIPT)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_build_smoke_items_uses_exactly_five_non_quarantined_supported_records(tmp_path):
    module = _load()
    treatment = {
        "records": [
            {"id": f"morph-workflow-rule-{i:010x}", "category": "workflow",
             "rule": f"Apply durable workflow rule number {i} when relevant.",
             "scope": "D--Claude", "record_type": "standing_preference",
             "confidence": 0.8, "support": [{"evidence": f"my source phrase {i}"}]}
            for i in range(6)
        ],
        "exclusions": [{"id": "morph-workflow-rule-0000000000"}],
    }
    records, cases = module.build_smoke_items(treatment, limit=5)
    assert len(records) == len(cases) == 5
    assert all(row["retrieval_aliases"] for row in records)
    assert all(row["expected_id"] == records[i]["id"] for i, row in enumerate(cases))


def test_enrich_delivery_records_keeps_full_primary_treatment():
    module = _load()
    treatment = {
        "records": [
            {"id": f"morph-workflow-rule-{i:010x}", "category": "workflow",
             "rule": f"Apply durable workflow rule number {i} when relevant.",
             "scope": "D--Claude", "record_type": "standing_preference",
             "confidence": 0.8,
             "support": [] if i == 0 else [{"evidence": f"my source phrase {i}"}]}
            for i in range(6)
        ],
        "exclusions": [{"id": "morph-workflow-rule-0000000001"}],
    }
    records = module.enrich_delivery_records(treatment)
    assert len(records) == 6
    assert records[0]["retrieval_aliases"] == []
    assert records[1]["retrieval_aliases"] == ["my source phrase 1"]
