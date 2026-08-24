"""Bounded, attributed persistence for multi-installation Adapt manifests."""

from __future__ import annotations

import hashlib
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Sequence


try:
    from adapt import workspace_runtime  # noqa: E402
    WORKSPACE_ROOT = workspace_runtime.workspace_root()
except Exception:
    _HERE = Path(__file__).resolve()
    WORKSPACE_ROOT = next(
        (p for p in _HERE.parents if (p / "tools" / "lib").is_dir()),
        _HERE.parent.parent,
    )
TOOLS_LIB = WORKSPACE_ROOT / "tools" / "lib"
if str(TOOLS_LIB) not in sys.path:
    sys.path.insert(0, str(TOOLS_LIB))

from adapt import workspace_runtime  # noqa: E402
from adapt import preference_record  # noqa: E402
from adapt import manifest  # noqa: E402


class AdaptPersistenceError(RuntimeError):
    """Raised when a reviewed Adapt batch was not durably committed completely."""


MAX_CORTEX_BATCH_ITEMS = 64
MAX_CORTEX_REQUEST_BYTES = 900 * 1024


def _token_file() -> Path:
    configured = os.environ.get("MEMBRANE_API_TOKEN_FILE", "").strip()
    if configured:
        return Path(configured)
    db = Path(os.environ.get(
        "CORTEX_DB", str(WORKSPACE_ROOT / "tools/.cache/memory/cortex-engine.db")
    ))
    return db.parent / "api-token"


def _base_url() -> str:
    return f"http://127.0.0.1:{workspace_runtime.membrane_port(os.environ)}"


def _normalize_scope(scope: str) -> str:
    """Mirror Cortex's leading Windows drive-token normalization."""
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
    semantic_validation: dict,
    record_payload_sha256s: dict[str, str],
) -> dict:
    batch_digest = hashlib.sha256(manifest_batch_id.encode("utf-8")).hexdigest()
    batch_id = f"adapt-{installation_id}-{batch_digest[:32]}"
    session_id = f"adapt-{installation_id}-{batch_digest[:24]}"
    trace_id = f"adapt-trace-{installation_id}-{batch_digest[:24]}"
    items = []
    receipt_sha256 = semantic_validation["receipt_sha256"]
    for ordinal, record in enumerate(records):
        payload_sha256 = record_payload_sha256s[record.id]
        content = (
            preference_record.to_cortex_content(record)
            + f"**Semantic validation:** receipt_sha256={receipt_sha256}, "
            f"payload_sha256={payload_sha256}\n"
        )
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
            "confidence": record.confidence,
            "confidenceBasis": (
                f"adapt semantic verification count={record.verification_count}; "
                f"last_verified_at={record.last_verified_at or 'never'}; "
                f"receipt_sha256={receipt_sha256}; payload_sha256={payload_sha256}"
            ),
        })
    return {"batch_id": batch_id, "items": items}


def _body_bytes(body: dict) -> bytes:
    return json.dumps(body, ensure_ascii=False).encode("utf-8")


def _partition_records(
    records: Sequence[preference_record.PreferenceRecord],
    *, manifest_batch_id: str, installation_id: str,
    semantic_validation: dict, record_payload_sha256s: dict[str, str],
) -> list[list[preference_record.PreferenceRecord]]:
    chunks: list[list[preference_record.PreferenceRecord]] = []
    current: list[preference_record.PreferenceRecord] = []
    for record in records:
        candidate = current + [record]
        probe_id = f"{manifest_batch_id}:chunk:{len(chunks):04d}"
        body = _request_body(
            candidate, manifest_batch_id=probe_id, installation_id=installation_id,
            semantic_validation=semantic_validation,
            record_payload_sha256s=record_payload_sha256s,
        )
        if (
            current
            and (
                len(candidate) > MAX_CORTEX_BATCH_ITEMS
                or len(_body_bytes(body)) > MAX_CORTEX_REQUEST_BYTES
            )
        ):
            chunks.append(current)
            current = [record]
            probe_id = f"{manifest_batch_id}:chunk:{len(chunks):04d}"
            body = _request_body(
                current, manifest_batch_id=probe_id, installation_id=installation_id,
                semantic_validation=semantic_validation,
                record_payload_sha256s=record_payload_sha256s,
            )
        if len(_body_bytes(body)) > MAX_CORTEX_REQUEST_BYTES:
            raise AdaptPersistenceError(
                f"Cortex content for {record.id} exceeds bounded request size"
            )
        current.append(record) if current != [record] else None
    if current:
        chunks.append(current)
    return chunks


def _post_body(
    body: dict, *, token: str, base_url: str, timeout: float,
) -> dict:
    request = urllib.request.Request(
        f"{base_url.rstrip('/')}/v1/memories:batch",
        data=_body_bytes(body),
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
        try:
            detail = exc.read().decode("utf-8", errors="replace")[:500].strip()
        except OSError:
            detail = ""
        suffix = f": {detail}" if detail else ""
        raise AdaptPersistenceError(
            f"Cortex batch rejected with HTTP {exc.code}{suffix}"
        ) from exc
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise AdaptPersistenceError("Cortex batch service is unavailable") from exc
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise AdaptPersistenceError("Cortex batch receipt is not valid JSON") from exc

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
        raise AdaptPersistenceError("Cortex batch receipt is incomplete or inconsistent")
    return payload


def _validate_semantic_binding(
    records: Sequence[preference_record.PreferenceRecord],
    *,
    semantic_validation: dict | None,
    record_payload_sha256s: dict[str, str] | None,
) -> None:
    """Require held-out receipt binding for every Cortex-bound record."""
    if not isinstance(semantic_validation, dict):
        raise AdaptPersistenceError("semantic validation receipt is required")
    if not isinstance(record_payload_sha256s, dict):
        raise AdaptPersistenceError("semantic payload bindings are required")
    if (
        semantic_validation.get("contract") != manifest.SEMANTIC_VALIDATION_CONTRACT
        or semantic_validation.get("complete") is not True
        or semantic_validation.get("independent") is not True
        or semantic_validation.get("receipt_sha256")
        != manifest.semantic_validation_receipt_sha256(semantic_validation)
    ):
        raise AdaptPersistenceError("semantic validation receipt is invalid")

    record_ids = [record.id for record in records]
    if len(record_ids) != len(set(record_ids)) or set(record_payload_sha256s) != set(record_ids):
        raise AdaptPersistenceError("semantic payload bindings do not exactly match records")
    results: dict[str, dict] = {}
    for result in semantic_validation.get("record_results") or []:
        record_id = str(result.get("id") or "")
        if record_id in results:
            raise AdaptPersistenceError("semantic validation receipt contains duplicate ids")
        results[record_id] = result
    validated_at = str(semantic_validation.get("validated_at") or "").strip()
    for record in records:
        payload_sha256 = record_payload_sha256s[record.id]
        result = results.get(record.id)
        if (
            not isinstance(payload_sha256, str)
            or len(payload_sha256) != 64
            or any(char not in "0123456789abcdef" for char in payload_sha256)
            or not result
            or result.get("payload_sha256") != payload_sha256
            or result.get("status") != "accepted"
            or result.get("verdict") != "valid"
            or record.last_verified_at != validated_at
        ):
            raise AdaptPersistenceError(
                f"record lacks exact held-out semantic binding: {record.id}"
            )
        contexts = record.evidence_contexts
        if not contexts:
            raise AdaptPersistenceError(f"record lacks direct user evidence: {record.id}")
        for context in contexts:
            source_events = [
                event for event in context.get("contextEvents", ())
                if event.get("isSource") is True
            ]
            if len(source_events) != 1:
                raise AdaptPersistenceError(f"record evidence is ambiguous: {record.id}")
            source = source_events[0]
            if (
                source.get("eventId") != context.get("sourceEventId")
                or source.get("kind") != "user_message"
                or source.get("role") != "user"
                or source.get("provenance") != "external_user"
                or source.get("authorityEligible") is not True
                or source.get("text") != context.get("evidenceText")
            ):
                raise AdaptPersistenceError(
                    f"record lacks authority-eligible external user evidence: {record.id}"
                )


def persist_manifest_batch(
    records: Sequence[preference_record.PreferenceRecord],
    *,
    manifest_batch_id: str,
    installation_id: str,
    semantic_validation: dict | None = None,
    record_payload_sha256s: dict[str, str] | None = None,
    token_file: Path | None = None,
    base_url: str | None = None,
    timeout: float = 150.0,
) -> dict:
    """Commit accepted records through deterministic, replayable Cortex transactions."""
    unverified = [
        record.id for record in records
        if record.verification_count < 1 or not record.last_verified_at
    ]
    if unverified:
        raise AdaptPersistenceError(
            f"refusing semantically unverified Adapt records: {unverified}"
        )
    if not records:
        return {
            "batch_id": f"adapt-empty-{manifest_batch_id}",
            "inserted": 0,
            "duplicates": 0,
            "complete": True,
            "receipts": [],
        }
    _validate_semantic_binding(
        records,
        semantic_validation=semantic_validation,
        record_payload_sha256s=record_payload_sha256s,
    )
    path = Path(token_file) if token_file is not None else _token_file()
    try:
        token = path.read_text(encoding="utf-8").strip()
    except OSError as exc:
        raise AdaptPersistenceError("Cortex API token is unavailable") from exc
    if not token:
        raise AdaptPersistenceError("Cortex API token is empty")
    chunks = _partition_records(
        records, manifest_batch_id=manifest_batch_id, installation_id=installation_id,
        semantic_validation=semantic_validation,
        record_payload_sha256s=record_payload_sha256s,
    )
    chunk_receipts = []
    for index, chunk in enumerate(chunks):
        chunk_manifest_id = (
            manifest_batch_id if len(chunks) == 1
            else f"{manifest_batch_id}:chunk:{index:04d}"
        )
        body = _request_body(
            chunk, manifest_batch_id=chunk_manifest_id, installation_id=installation_id,
            semantic_validation=semantic_validation,
            record_payload_sha256s=record_payload_sha256s,
        )
        try:
            chunk_receipts.append(_post_body(
                body, token=token, base_url=base_url or _base_url(), timeout=timeout,
            ))
        except AdaptPersistenceError as exc:
            raise AdaptPersistenceError(
                f"Cortex chunk {index + 1}/{len(chunks)} failed; "
                f"{index} completed chunk(s) remain replayable: {exc}"
            ) from exc
    return {
        "batch_id": f"adapt-manifest-{hashlib.sha256(manifest_batch_id.encode()).hexdigest()[:32]}",
        "inserted": sum(int(item.get("inserted", 0)) for item in chunk_receipts),
        "duplicates": sum(int(item.get("duplicates", 0)) for item in chunk_receipts),
        "complete": True,
        "semantic_validation_receipt_sha256": semantic_validation["receipt_sha256"],
        "receipts": [row for item in chunk_receipts for row in item["receipts"]],
        "chunks": [{
            "batch_id": item["batch_id"],
            "inserted": item.get("inserted", 0),
            "duplicates": item.get("duplicates", 0),
        } for item in chunk_receipts],
    }
