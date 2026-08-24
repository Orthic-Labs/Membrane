from __future__ import annotations

import io
import hashlib
import json
import urllib.error
from pathlib import Path

import pytest

from adapt import adapt_persistence as persistence
from adapt import manifest
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
    evidence_text = "Always preserve the reviewed preference identity during apply."
    evidence_context = {
        "sourceEventId": "event-user-1",
        "evidenceText": evidence_text,
        "contextEvents": [{
            "eventId": "event-user-1",
            "kind": "user_message",
            "role": "user",
            "provenance": "external_user",
            "authorityEligible": True,
            "isSource": True,
            "text": evidence_text,
        }],
    }
    return preference_record.from_manifest_candidate(
        {
            "id": record_id,
            "rule": evidence_text,
            "category": "tooling",
            "scope": scope,
            "record_type": "standing_preference",
            "authority_effect": "neutral",
            "confidence": 0.8,
            "evidence_count": 2,
            "source_ids": [SOURCE],
            "verification_count": 1,
            "last_verified_at": "2026-08-24T00:00:00Z",
            "evidenceContexts": [evidence_context],
        },
        now="2026-07-20T10:00:00Z",
    )


def _semantic_binding(records) -> dict:
    payloads = {
        record.id: hashlib.sha256(record.id.encode("utf-8")).hexdigest()
        for record in records
    }
    receipt = {
        "contract": manifest.SEMANTIC_VALIDATION_CONTRACT,
        "complete": True,
        "independent": True,
        "validator_run_id": "test-held-out-validator",
        "validator": "test-model",
        "validated_at": "2026-08-24T00:00:00Z",
        "canonical_pool_sha256": "b" * 64,
        "record_results": [{
            "id": record.id,
            "payload_sha256": payloads[record.id],
            "status": "accepted",
            "verdict": "valid",
            "reason": "fixture direct evidence supports exact rule",
        } for record in records],
    }
    receipt["receipt_sha256"] = manifest.semantic_validation_receipt_sha256(receipt)
    return {
        "semantic_validation": receipt,
        "record_payload_sha256s": payloads,
    }


def test_small_batch_apply_is_one_authenticated_attributed_request(
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
    records = [_record()]
    receipt = persistence.persist_manifest_batch(
        records,
        manifest_batch_id="20260720T100000-abcdef",
        installation_id=INSTALLATION,
        token_file=token,
        base_url="http://127.0.0.1:8765",
        **_semantic_binding(records),
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
    assert item["confidence"] == 0.8
    assert "semantic verification count=1" in item["confidenceBasis"]
    assert INSTALLATION in item["session_id"]


def test_large_batch_is_partitioned_into_replayable_bounded_requests(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    token = tmp_path / "api-token"
    token.write_text("test-secret\n", encoding="utf-8")
    bodies = []

    def fake_urlopen(request, timeout):
        body = json.loads(request.data.decode("utf-8"))
        bodies.append(body)
        return _Response({
            "batch_id": body["batch_id"],
            "inserted": len(body["items"]),
            "duplicates": 0,
            "complete": True,
            "receipts": [{
                "item_id": item["item_id"],
                "memory_id": f"{item['scope']}/{item['name']}",
                "status": "inserted",
            } for item in body["items"]],
        })

    monkeypatch.setattr(persistence.urllib.request, "urlopen", fake_urlopen)
    records = [
        _record(f"adapt-tooling-reviewed-{index:010d}")
        for index in range(65)
    ]
    receipt = persistence.persist_manifest_batch(
        records, manifest_batch_id="large-batch", installation_id=INSTALLATION,
        token_file=token, base_url="http://127.0.0.1:8765",
        **_semantic_binding(records),
    )

    assert len(bodies) == 2
    assert all(len(body["items"]) <= 64 for body in bodies)
    assert all(len(json.dumps(body).encode()) <= persistence.MAX_CORTEX_REQUEST_BYTES for body in bodies)
    assert receipt["complete"] is True
    assert len(receipt["receipts"]) == 65


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
        **_semantic_binding([_record()]),
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
    records = [_record(scope="d-claude-coderight")]
    receipt = persistence.persist_manifest_batch(
        records,
        manifest_batch_id="20260720T100000-normalized",
        installation_id=INSTALLATION,
        token_file=token,
        base_url="http://127.0.0.1:8765",
        **_semantic_binding(records),
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
        records = [_record()]
        persistence.persist_manifest_batch(
            records,
            manifest_batch_id="20260720T100000-abcdef",
            installation_id=INSTALLATION,
            token_file=token,
            base_url="http://127.0.0.1:8765",
            **_semantic_binding(records),
        )


def test_batch_apply_surfaces_bounded_http_error_detail(tmp_path: Path, monkeypatch) -> None:
    token = tmp_path / "api-token"
    token.write_text("secret", encoding="utf-8")

    def rejected(_request, timeout):
        assert timeout == 150.0
        raise urllib.error.HTTPError(
            "http://127.0.0.1/v1/memories:batch", 400, "bad request", {},
            io.BytesIO(b'{"error":"invalid memory batch"}'),
        )

    monkeypatch.setattr(persistence.urllib.request, "urlopen", rejected)
    with pytest.raises(persistence.AdaptPersistenceError, match="invalid memory batch"):
        records = [_record()]
        persistence.persist_manifest_batch(
            records, manifest_batch_id="batch-http-error",
            installation_id=INSTALLATION, token_file=token,
            base_url="http://127.0.0.1:8765",
            **_semantic_binding(records),
        )


def test_reviewed_manifest_id_is_not_rederived() -> None:
    reviewed_id = "adapt-tooling-reviewed-1111111111"
    record = _record(reviewed_id)

    assert record.id == reviewed_id


def test_persistence_refuses_unverified_record() -> None:
    record = preference_record.from_manifest_candidate(
        {
            "id": "adapt-tooling-unverified-2222222222",
            "rule": "Always retain semantic verification before persistence.",
            "category": "tooling",
            "scope": "D--Claude",
            "record_type": "standing_preference",
            "authority_effect": "neutral",
            "confidence": 0.8,
            "evidence_count": 1,
            "source_ids": [SOURCE],
        },
        now="2026-08-24T00:00:00Z",
    )
    with pytest.raises(persistence.AdaptPersistenceError, match="unverified"):
        persistence.persist_manifest_batch(
            [record], manifest_batch_id="unverified", installation_id=INSTALLATION
        )


def test_persistence_refuses_missing_or_mismatched_semantic_binding() -> None:
    record = _record()
    with pytest.raises(persistence.AdaptPersistenceError, match="receipt is required"):
        persistence.persist_manifest_batch(
            [record], manifest_batch_id="missing-binding", installation_id=INSTALLATION
        )

    binding = _semantic_binding([record])
    binding["record_payload_sha256s"][record.id] = "c" * 64
    with pytest.raises(persistence.AdaptPersistenceError, match="exact held-out"):
        persistence.persist_manifest_batch(
            [record], manifest_batch_id="mismatch-binding", installation_id=INSTALLATION,
            **binding,
        )


def test_persistence_refuses_receipt_bound_record_without_direct_user_evidence() -> None:
    record = preference_record.from_manifest_candidate(
        {
            **_record().to_dict(),
            "evidenceContexts": [],
        },
        now="2026-08-24T00:00:00Z",
    )
    with pytest.raises(persistence.AdaptPersistenceError, match="direct user evidence"):
        persistence.persist_manifest_batch(
            [record], manifest_batch_id="missing-evidence", installation_id=INSTALLATION,
            **_semantic_binding([record]),
        )
