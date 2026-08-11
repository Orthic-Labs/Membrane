from __future__ import annotations

import json
from pathlib import Path

import pytest

from adapt import adapt_persistence as persistence
from adapt import preference_record


INSTALLATION = "08c7ef55-8f6b-4ef1-b234-22232b8ea832"
SOURCE = f"install:{INSTALLATION}:codex:{'a' * 32}"


class _Response:
    def __init__(self, payload: dict, status: int = 201) -> None:
        self.status = status
        self._payload = json.dumps(payload).encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def read(self) -> bytes:
        return self._payload


def _record(
    record_id: str = "adapt-tooling-reviewed-1111111111",
    scope: str = "D--Claude",
):
    return preference_record.from_manifest_candidate(
        {
            "id": record_id,
            "rule": "Always preserve the reviewed preference identity during apply.",
            "category": "tooling",
            "scope": scope,
            "record_type": "standing_preference",
            "authority_effect": "neutral",
            "confidence": 0.8,
            "evidence_count": 2,
            "source_ids": [SOURCE],
        },
        now="2026-07-20T10:00:00Z",
    )


def test_batch_apply_is_one_authenticated_attributed_request(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    token = tmp_path / "api-token"
    token.write_text("test-secret\n", encoding="utf-8")
    captured = {}

    def fake_urlopen(request, timeout):
        captured["request"] = request
        captured["timeout"] = timeout
        body = json.loads(request.data.decode("utf-8"))
        captured["body"] = body
        return _Response(
            {
                "batch_id": body["batch_id"],
                "inserted": 1,
                "duplicates": 0,
                "complete": True,
                "receipts": [
                    {
                        "item_id": body["items"][0]["item_id"],
                        "memory_id": "D--Claude/adapt-tooling-reviewed-1111111111",
                        "status": "inserted",
                    }
                ],
            }
        )

    monkeypatch.setattr(persistence.urllib.request, "urlopen", fake_urlopen)
    receipt = persistence.persist_manifest_batch(
        [_record()],
        manifest_batch_id="20260720T100000-abcdef",
        installation_id=INSTALLATION,
        token_file=token,
        base_url="http://127.0.0.1:8765",
    )

    assert receipt["complete"] is True
    assert captured["request"].full_url.endswith("/v1/memories:batch")
    assert captured["request"].get_header("Authorization") == "Bearer test-secret"
    item = captured["body"]["items"][0]
    assert item["name"] == "adapt-tooling-reviewed-1111111111"
    assert item["source_ids"] == [SOURCE]
    assert item["artifact_family"] == "adapt"
    assert item["producer"] == "adapt"
    assert item["record_type"] == "standing_preference"
    assert item["client"] == "codex"
    assert INSTALLATION in item["session_id"]


@pytest.mark.parametrize("tool", ["command-code", "cline", "gemini", "grok-build", "roo-cline"])
def test_single_external_source_preserves_client_attribution(tool: str) -> None:
    source = f"install:{INSTALLATION}:{tool}:{'b' * 32}"

    assert persistence._source_client([source]) == tool


def test_batch_request_is_stable_for_identical_manifest(monkeypatch, tmp_path: Path) -> None:
    token = tmp_path / "api-token"
    token.write_text("secret", encoding="utf-8")
    bodies = []

    def fake_urlopen(request, timeout):
        body = json.loads(request.data.decode("utf-8"))
        bodies.append(body)
        return _Response(
            {
                "batch_id": body["batch_id"],
                "inserted": 1 if len(bodies) == 1 else 0,
                "duplicates": 0 if len(bodies) == 1 else 1,
                "complete": True,
                "receipts": [
                    {
                        "item_id": body["items"][0]["item_id"],
                        "memory_id": f"D--Claude/{body['items'][0]['name']}",
                        "status": "inserted" if len(bodies) == 1 else "duplicate",
                    }
                ],
            },
            status=201 if len(bodies) == 1 else 200,
        )

    monkeypatch.setattr(persistence.urllib.request, "urlopen", fake_urlopen)
    kwargs = {
        "manifest_batch_id": "20260720T100000-abcdef",
        "installation_id": INSTALLATION,
        "token_file": token,
        "base_url": "http://127.0.0.1:8765",
    }
    persistence.persist_manifest_batch([_record()], **kwargs)
    persistence.persist_manifest_batch([_record()], **kwargs)

    assert bodies[0] == bodies[1]


def test_batch_receipt_compares_service_normalized_drive_scope(monkeypatch, tmp_path: Path) -> None:
    token = tmp_path / "api-token"
    token.write_text("secret", encoding="utf-8")

    def fake_urlopen(request, timeout):
        body = json.loads(request.data.decode("utf-8"))
        return _Response({
            "batch_id": body["batch_id"],
            "inserted": 1,
            "duplicates": 0,
            "complete": True,
            "receipts": [{
                "item_id": body["items"][0]["item_id"],
                "memory_id": f"D-claude-coderight/{body['items'][0]['name']}",
                "status": "updated",
            }],
        })

    monkeypatch.setattr(persistence.urllib.request, "urlopen", fake_urlopen)
    receipt = persistence.persist_manifest_batch(
        [_record(scope="d-claude-coderight")],
        manifest_batch_id="20260720T100000-normalized",
        installation_id=INSTALLATION,
        token_file=token,
        base_url="http://127.0.0.1:8765",
    )

    assert receipt["complete"] is True


def test_batch_apply_refuses_incomplete_receipt(tmp_path: Path, monkeypatch) -> None:
    token = tmp_path / "api-token"
    token.write_text("secret", encoding="utf-8")
    monkeypatch.setattr(
        persistence.urllib.request,
        "urlopen",
        lambda request, timeout: _Response(
            {
                "batch_id": json.loads(request.data)["batch_id"],
                "inserted": 0,
                "duplicates": 0,
                "complete": False,
                "receipts": [],
            }
        ),
    )

    with pytest.raises(persistence.AdaptPersistenceError, match="receipt"):
        persistence.persist_manifest_batch(
            [_record()],
            manifest_batch_id="20260720T100000-abcdef",
            installation_id=INSTALLATION,
            token_file=token,
            base_url="http://127.0.0.1:8765",
        )


def test_reviewed_manifest_id_is_not_rederived() -> None:
    reviewed_id = "adapt-tooling-reviewed-1111111111"
    record = _record(reviewed_id)

    assert record.id == reviewed_id
