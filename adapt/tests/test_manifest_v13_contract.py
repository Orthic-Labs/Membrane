"""Membrane Adapt hard-cut manifest contract."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from adapt import manifest


def _body(version: str) -> dict:
    source = {
        "source_id": "install:08c7ef55-8f6b-4ef1-b234-22232b8ea832:codex:" + "a" * 32,
        "tool": "codex",
        "host": "codex",
        "path": "/tmp/session.jsonl",
        "mtime_ns": 1,
        "source_sha256": "sha256:" + "a" * 64,
    }
    return {
        "schema_version": version,
        "batch_id": "batch-v13",
        "created_at": "2026-08-20T00:00:00Z",
        "installation_id": "08c7ef55-8f6b-4ef1-b234-22232b8ea832",
        "canonical_pool_sha256": "b" * 64,
        "source_session_ids": [source["source_id"]],
        "source_refs": [source],
        "records": [],
    }


def test_only_manifest_v13_is_accepted(tmp_path: Path) -> None:
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(_body("1.3.0")), encoding="utf-8")
    assert manifest.apply_time_validate(path)["schema_version"] == "1.3.0"

    for version in ("1.0.0", "1.1.0", "1.2.0", "2.0.0"):
        body = _body(version)
        if version != "1.3.0":
            body.pop("source_refs", None)
        path.write_text(json.dumps(body), encoding="utf-8")
        with pytest.raises(manifest.ManifestError):
            manifest.validate_schema(path)


def test_retired_manifest_alias_is_absent() -> None:
    assert not hasattr(manifest, "load_and_validate")


def test_frozen_v2_manifest_requires_complete_extraction_coverage(tmp_path: Path) -> None:
    body = _body("1.3.0")
    body["generator"] = "adapt-frozen-open-transcripts-v2:" + "c" * 64
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(body), encoding="utf-8")
    with pytest.raises(manifest.ManifestError):
        manifest.validate_schema(path)

    body["extraction_coverage"] = {
        "complete": True,
        "source_count": 1,
        "corpus_source_count": 1,
        "shard_index": 0,
        "shard_count": 1,
        "sources_with_mined_turns": 1,
        "canonical_user_turns": 2,
        "mined_user_turns": 2,
        "policy_excluded_user_turns": 0,
        "llm_batches": 1,
        "committable_batches": 1,
        "failed_batches": 0,
        "batch_char_budget": 120000,
        "checkpointed_batches": 1,
        "selection_contract": "all-safe-external-user-turns-v1",
        "context_contract": "authoritative-source-with-prior-nonauthoritative-context-v1",
    }
    path.write_text(json.dumps(body), encoding="utf-8")
    assert manifest.validate_schema(path)["extraction_coverage"]["complete"] is True


def _semantic_body() -> dict:
    record = {
        "id": "adapt-workflow-focused-tests-0123456789",
        "rule": "Always run focused tests before reporting completion.",
        "category": "workflow",
        "scope": "workspace",
        "status": "accepted",
        "payload_sha256": "a" * 64,
        "verification_count": 1,
        "last_verified_at": "2026-08-24T00:00:00Z",
    }
    body = {
        "canonical_pool_sha256": "b" * 64,
        "records": [record],
    }
    receipt = {
        "contract": manifest.SEMANTIC_VALIDATION_CONTRACT,
        "complete": True,
        "independent": True,
        "validator_run_id": "fresh-validator-1",
        "validator": "held-out-model",
        "validated_at": "2026-08-24T00:00:00Z",
        "canonical_pool_sha256": "b" * 64,
        "record_results": [{
            "id": record["id"],
            "payload_sha256": record["payload_sha256"],
            "status": "accepted",
            "verdict": "valid",
            "reason": "direct evidence supports exact rule",
        }],
    }
    receipt["receipt_sha256"] = manifest.semantic_validation_receipt_sha256(receipt)
    body["semantic_validation"] = receipt
    return body


def test_semantic_validation_requires_exact_hashed_coverage() -> None:
    body = _semantic_body()
    assert manifest.semantic_validation_errors(body) == []

    body["semantic_validation"]["record_results"][0]["payload_sha256"] = "c" * 64
    assert any("payload mismatch" in error for error in manifest.semantic_validation_errors(body))
    assert any("receipt hash mismatch" in error for error in manifest.semantic_validation_errors(body))


def test_semantic_validation_rejects_unverified_accepted_record() -> None:
    body = _semantic_body()
    body["records"][0]["verification_count"] = 0
    assert any("verification stamp" in error for error in manifest.semantic_validation_errors(body))


def test_semantic_validation_is_required_for_nonempty_manifest() -> None:
    body = _semantic_body()
    body.pop("semantic_validation")
    assert manifest.semantic_validation_errors(body) == ["semantic validation receipt is missing"]
