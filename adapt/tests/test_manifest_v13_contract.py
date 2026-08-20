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
