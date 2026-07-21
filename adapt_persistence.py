"""Atomic, attributed persistence for multi-installation Adapt manifests."""

from __future__ import annotations

import hashlib
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Sequence


WORKSPACE_ROOT = Path(__file__).resolve().parents[4]
TOOLS_LIB = WORKSPACE_ROOT / "tools" / "lib"
if str(TOOLS_LIB) not in sys.path:
    sys.path.insert(0, str(TOOLS_LIB))

from memory.runtime_config import memright_port  # noqa: E402
import preference_record  # noqa: E402


class AdaptPersistenceError(RuntimeError):
    """Raised when a reviewed Adapt batch was not durably committed as one unit."""


def _token_file() -> Path:
    configured = os.environ.get("MEMRIGHT_API_TOKEN_FILE", "").strip()
    if configured:
        return Path(configured)
    db = Path(os.environ.get(
        "MEMRIGHT_DB", str(WORKSPACE_ROOT / "tools/.cache/memory/memright-engine.db")
    ))
    return db.parent / "api-token"


def _base_url() -> str:
    return f"http://127.0.0.1:{memright_port(os.environ)}"


def _normalize_scope(scope: str) -> str:
    """Mirror MemRight's leading Windows drive-token normalization."""
    if len(scope) >= 2 and scope[0].islower() and scope[0].isascii() and scope[0].isalpha() and scope[1] == "-":
        return scope[0].upper() + scope[1:]
    return scope


def _source_client(source_ids: Sequence[str]) -> str:
    tools = set()
    for source in source_ids:
        parts = source.split(":")
        if len(parts) == 4 and parts[0] == "install":
            tools.add(parts[2])
    if len(tools) == 1:
        tool = next(iter(tools))
        return "claude" if tool == "claude-code" else tool
    return "mixed"


def _request_body(
    records: Sequence[preference_record.PreferenceRecord],
    *,
    manifest_batch_id: str,
    installation_id: str,
) -> dict:
    batch_digest = hashlib.sha256(manifest_batch_id.encode("utf-8")).hexdigest()
    batch_id = f"adapt-{installation_id}-{batch_digest[:32]}"
    session_id = f"adapt-{installation_id}-{batch_digest[:24]}"
    trace_id = f"adapt-trace-{installation_id}-{batch_digest[:24]}"
    items = []
    for ordinal, record in enumerate(records):
        content = preference_record.to_memright_content(record)
        canonical = json.dumps(
            {
                "name": record.id,
                "scope": record.scope,
                "content": content,
                "source_ids": list(record.source_ids),
            },
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=False,
        )
        item_digest = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
        items.append({
            "item_id": f"adapt-{ordinal:04d}-{item_digest[:24]}",
            "name": record.id,
            "content": content,
            "scope": record.scope,
            "tier": "Semantic",
            "artifact_family": "adapt",
            "producer": "adapt",
            "record_type": record.record_type,
            "client": _source_client(record.source_ids),
            "session_id": session_id,
            "trace_id": trace_id,
            "source_ids": list(record.source_ids),
        })
    return {"batch_id": batch_id, "items": items}


def persist_manifest_batch(
    records: Sequence[preference_record.PreferenceRecord],
    *,
    manifest_batch_id: str,
    installation_id: str,
    token_file: Path | None = None,
    base_url: str | None = None,
    timeout: float = 150.0,
) -> dict:
    """Commit all accepted records with one inference batch and one DB transaction."""
    if not records:
        return {
            "batch_id": f"adapt-empty-{manifest_batch_id}",
            "inserted": 0,
            "duplicates": 0,
            "complete": True,
            "receipts": [],
        }
    body = _request_body(
        records,
        manifest_batch_id=manifest_batch_id,
        installation_id=installation_id,
    )
    path = Path(token_file) if token_file is not None else _token_file()
    try:
        token = path.read_text(encoding="utf-8").strip()
    except OSError as exc:
        raise AdaptPersistenceError("MemRight API token is unavailable") from exc
    if not token:
        raise AdaptPersistenceError("MemRight API token is empty")
    request = urllib.request.Request(
        f"{(base_url or _base_url()).rstrip('/')}/v1/memories:batch",
        data=json.dumps(body, ensure_ascii=False).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {token}",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            status = response.status
            payload = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        raise AdaptPersistenceError(f"MemRight batch rejected with HTTP {exc.code}") from exc
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise AdaptPersistenceError("MemRight batch service is unavailable") from exc
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise AdaptPersistenceError("MemRight batch receipt is not valid JSON") from exc

    receipts = payload.get("receipts")
    expected_item_ids = {item["item_id"] for item in body["items"]}
    expected_memory_ids = {
        f"{_normalize_scope(item['scope'])}/{item['name']}" for item in body["items"]
    }
    if (
        status not in {200, 201}
        or payload.get("batch_id") != body["batch_id"]
        or payload.get("complete") is not True
        or not isinstance(receipts, list)
        or len(receipts) != len(body["items"])
        or payload.get("inserted", 0) + payload.get("duplicates", 0) != len(body["items"])
        or {row.get("item_id") for row in receipts if isinstance(row, dict)}
        != expected_item_ids
        or {row.get("memory_id") for row in receipts if isinstance(row, dict)}
        != expected_memory_ids
        or any(
            not isinstance(row, dict)
            or row.get("status") not in {"inserted", "updated", "duplicate"}
            for row in receipts
        )
    ):
        raise AdaptPersistenceError("MemRight batch receipt is incomplete or inconsistent")
    return payload
