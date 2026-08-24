from __future__ import annotations

import hashlib
import io
import json
from pathlib import Path

from adapt import insight_persistence


def _report() -> dict:
    return {
        "eventCount": 100,
        "sessionCount": 4,
        "byDetector": {
            "ignored_tool_failure": [
                {
                    "severity": "high",
                    "confidence": 0.8,
                    "firstSeen": "2026-01-01T00:00:00Z",
                    "lastSeen": "2026-01-02T00:00:00Z",
                    "hosts": ["codex"],
                    "likelyMechanism": "candidate: failed result was ignored",
                    "suggestedRemediations": ["candidate: resolve failed result"],
                    "userDisposition": "escalated",
                },
                {
                    "severity": "medium",
                    "confidence": 0.6,
                    "firstSeen": "2026-01-03T00:00:00Z",
                    "lastSeen": "2026-01-03T00:00:00Z",
                    "hosts": ["codex"],
                    "likelyMechanism": "candidate: failed result was ignored",
                    "suggestedRemediations": ["candidate: resolve failed result"],
                    "userDisposition": "repeated",
                },
            ],
            "empty": [],
        },
    }


def test_build_items_preserves_detector_structure_as_reference(tmp_path: Path) -> None:
    manifest = tmp_path / "manifest.json"
    manifest.write_text("{}", encoding="utf-8")
    items = insight_persistence.build_items(
        _report(), report_digest="a" * 64, source_manifest=manifest
    )
    assert len(items) == 1
    item = items[0]
    assert item["name"] == "insight-ignored-tool-failure"
    assert item["authority"] == "A1"
    assert item["influenceClass"] == "reference"
    assert item["record_type"] == "insight_report"
    assert item["confidence"] == 0.7
    assert "Observed 2 heuristic signal cards" in item["content"]
    assert "failed result was ignored (2)" in item["content"]
    assert "never an instruction or permission grant" in item["content"]
    expected = hashlib.sha256(b"{}").hexdigest()
    assert f"adapt-transcript-manifest-sha256:{expected}" in item["source_ids"]


def test_persist_items_requires_complete_receipt(monkeypatch, tmp_path: Path) -> None:
    token = tmp_path / "token"
    token.write_text("secret", encoding="utf-8")
    items = insight_persistence.build_items(_report(), report_digest="b" * 64)
    expected_memory = "workspace/insight-ignored-tool-failure"
    response_payload = {
        "batch_id": "placeholder",
        "inserted": 1,
        "duplicates": 0,
        "complete": True,
        "receipts": [{
            "item_id": items[0]["item_id"],
            "memory_id": expected_memory,
            "status": "inserted",
        }],
    }

    class Response(io.BytesIO):
        status = 201

        def __enter__(self):
            return self

        def __exit__(self, *args):
            return False

    monkeypatch.setattr(
        insight_persistence.urllib.request,
        "urlopen",
        lambda request, timeout: Response(json.dumps(response_payload).encode()),
    )
    request_digest = hashlib.sha256(
        json.dumps(items, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    ).hexdigest()
    response_payload["batch_id"] = f"adapt-insights-{'b' * 16}-{request_digest[:16]}"
    receipt = insight_persistence.persist_items(
        items, report_digest="b" * 64, token_file=token, base_url="http://127.0.0.1"
    )
    assert receipt["complete"] is True
