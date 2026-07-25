"""Fail-closed multi-installation contracts for Adapt transcript mining."""

from __future__ import annotations

import hashlib
import json
import re
import sqlite3
import uuid
from pathlib import Path
from typing import Any


class CrossMachineAdaptError(RuntimeError):
    """Raised when an Adapt manifest cannot safely join the shared rule pool."""


_RULE_LINE = re.compile(
    r"^\*\*\[adapt/(?P<category>[a-z-]+)\]\*\* — (?P<rule>.+?) "
    r"Confidence: (?P<confidence>(?:0(?:\.\d+)?|1(?:\.0+)?)) "
    r"\(observations: (?P<observations>\d+), needs_review: "
    r"(?P<needs_review>true|false), updated \d{4}-\d{2}-\d{2}\)$"
)
_SOURCE_ID = re.compile(
    r"^install:(?P<installation>[0-9a-f-]{36}):"
    r"(?P<tool>[a-z0-9][a-z0-9._-]{0,79}):(?P<digest>[a-f0-9]{32})$"
)
_TOOL = re.compile(r"^[a-z0-9][a-z0-9._-]{0,79}$")
_RECORD_LINE = re.compile(
    r"^\*\*Record:\*\* type=(?P<record_type>[a-z_]+), "
    r"authority_effect=(?P<authority_effect>[a-z_]+)$"
)


def _uuid4(value: object) -> str:
    try:
        parsed = uuid.UUID(str(value))
    except (TypeError, ValueError) as exc:
        raise CrossMachineAdaptError("invalid installation identity") from exc
    if parsed.version != 4 or str(parsed) != str(value):
        raise CrossMachineAdaptError("invalid installation identity")
    return str(parsed)


def load_installation_id(path: Path) -> str:
    try:
        payload = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise CrossMachineAdaptError("installation identity is unavailable") from exc
    if not isinstance(payload, dict) or payload.get("schema_version") != 2:
        raise CrossMachineAdaptError("installation identity is not schema v2")
    return _uuid4(payload.get("installation_id"))


def qualify_source_session(installation_id: str, tool: str, session_id: str) -> str:
    installation = _uuid4(installation_id)
    if not isinstance(tool, str) or not _TOOL.fullmatch(tool) or not str(session_id).strip():
        raise CrossMachineAdaptError("invalid source session identity")
    digest = hashlib.sha256(
        f"{tool}\0{session_id}".encode("utf-8")
    ).hexdigest()[:32]
    return f"install:{installation}:{tool}:{digest}"


def _parse_rule_row(
    memory_id: str,
    scope: str,
    content: str,
    source_ids_raw: str,
    created_at: str,
    updated_at: str,
    record_type: str,
) -> tuple[str, dict[str, Any]] | None:
    name = memory_id[len(scope) + 1 :] if memory_id.startswith(f"{scope}/") else memory_id
    if not name.startswith("adapt-"):
        return None
    first_line = content.splitlines()[0] if content else ""
    if not first_line.startswith("**[adapt/"):
        return None
    match = _RULE_LINE.fullmatch(first_line)
    try:
        source_ids = json.loads(source_ids_raw)
    except json.JSONDecodeError as exc:
        raise CrossMachineAdaptError(f"canonical Adapt row has invalid source IDs: {name}") from exc
    if not isinstance(source_ids, list) or not all(isinstance(value, str) for value in source_ids):
        raise CrossMachineAdaptError(f"canonical Adapt row has invalid source IDs: {name}")
    if match is None:
        raise CrossMachineAdaptError(f"canonical Adapt row has invalid envelope: {name}")
    retrieval_aliases: list[str] = []
    authority_effect = "neutral"
    envelope_record_type = record_type
    for line in content.splitlines()[1:]:
        if line.startswith("**Trigger phrases:** "):
            retrieval_aliases = [
                value.strip()
                for value in line.removeprefix("**Trigger phrases:** ").split(" | ")
                if value.strip()
            ]
        record_match = _RECORD_LINE.fullmatch(line)
        if record_match is not None:
            envelope_record_type = record_match.group("record_type")
            authority_effect = record_match.group("authority_effect")
    return name, {
        "name": name,
        "id": name,
        "scope": scope,
        "category": match.group("category"),
        "rule": match.group("rule"),
        "confidence": float(match.group("confidence")),
        "observations": int(match.group("observations")),
        "needs_review": match.group("needs_review") == "true",
        "source_ids": source_ids,
        "created_at": created_at,
        "updated_at": updated_at,
        "record_type": envelope_record_type,
        "authority_effect": authority_effect,
        "retrieval_aliases": retrieval_aliases,
    }


def load_canonical_rules(db_path: Path) -> dict[str, dict[str, Any]]:
    """Read the canonical Adapt pool from SQLite without mutating or embedding."""
    path = Path(db_path).resolve()
    if not path.is_file():
        raise CrossMachineAdaptError(f"canonical MemRight DB is unavailable: {path}")
    uri = f"file:{path.as_posix()}?mode=ro"
    try:
        conn = sqlite3.connect(uri, uri=True)
        columns = {str(row[1]) for row in conn.execute("PRAGMA table_info(memories)")}
        record_type = (
            "record_type"
            if "record_type" in columns
            else "'operational_playbook' AS record_type"
        )
        rows = conn.execute(
            f"SELECT id, scope_id, content, source_ids, created_at, updated_at, {record_type} "
            "FROM memories WHERE id LIKE '%/adapt-%' OR id LIKE 'adapt-%' ORDER BY id"
        ).fetchall()
    except sqlite3.Error as exc:
        raise CrossMachineAdaptError("canonical Adapt pool query failed") from exc
    finally:
        try:
            conn.close()
        except UnboundLocalError:
            pass
    rules: dict[str, dict[str, Any]] = {}
    for row in rows:
        parsed = _parse_rule_row(*row)
        if parsed is None:
            continue
        name, rule = parsed
        # Identity is (scope, name), not name. The same workspace is scoped
        # "D--Claude" on one machine and "Volumes-D-claude" on the other, so one
        # rule synced across both machines produced two rows with one name and
        # raised here -- disabling canonical context entirely on a healthy pool.
        # A real duplicate is the same name in the SAME scope, and that still raises.
        scope = rule.get("scope") or ""
        key = f"{scope}/{name}" if scope else name
        if key in rules:
            raise CrossMachineAdaptError(f"duplicate canonical Adapt identity: {key}")
        rules[key] = rule
    return rules


def canonical_pool_sha256(rules: dict[str, dict[str, Any]]) -> str:
    # Sorted by the row's own (id, scope) rather than by the mapping key, so the
    # digest depends only on pool CONTENT. Sorting by key silently coupled the
    # hash to how the caller happened to key the dict; a pool keyed by name and
    # the same pool keyed by scoped id would hash differently despite being
    # identical data. For a single-scope pool the order is unchanged, so this
    # does not invalidate existing manifests.
    rows = []
    for key, rule in rules.items():
        rows.append({
            "id": rule.get("id") or rule.get("name") or key,
            "scope": rule.get("scope", ""),
            "category": rule.get("category", ""),
            "rule": rule.get("rule", ""),
            "confidence": float(rule.get("confidence", 0.6)),
            "observations": int(rule.get("observations", rule.get("evidence_count", 1))),
            "needs_review": bool(rule.get("needs_review", False)),
            "record_type": rule.get("record_type", "unclassified"),
            "authority_effect": rule.get("authority_effect", "neutral"),
            "retrieval_aliases": list(rule.get("retrieval_aliases") or []),
            "source_ids": sorted(rule.get("source_ids") or []),
        })
    rows.sort(key=lambda row: (row["id"], row["scope"]))
    canonical = json.dumps(
        rows, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    )
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def _validate_source_id(value: object) -> re.Match[str]:
    if not isinstance(value, str):
        raise CrossMachineAdaptError("invalid installation-qualified source session")
    match = _SOURCE_ID.fullmatch(value)
    if match is None:
        raise CrossMachineAdaptError("invalid installation-qualified source session")
    _uuid4(match.group("installation"))
    return match


def validate_multiwriter_binding(
    manifest: dict[str, Any],
    *,
    installation_id: str,
    canonical_rules: dict[str, dict[str, Any]],
) -> None:
    """Refuse apply when identity, evidence namespace, or canonical pool changed."""
    installation = _uuid4(installation_id)
    if manifest.get("installation_id") != installation:
        raise CrossMachineAdaptError("manifest installation does not match this installation")
    expected = canonical_pool_sha256(canonical_rules)
    if manifest.get("canonical_pool_sha256") != expected:
        raise CrossMachineAdaptError("canonical Adapt pool changed; regenerate the manifest")
    sources = manifest.get("source_session_ids")
    if not isinstance(sources, list) or not sources:
        raise CrossMachineAdaptError("manifest has no installation-qualified source sessions")
    source_set = set(sources)
    if len(source_set) != len(sources):
        raise CrossMachineAdaptError("manifest source sessions are duplicated")
    for source in sources:
        match = _validate_source_id(source)
        if match.group("installation") != installation:
            raise CrossMachineAdaptError("manifest source session belongs to another installation")
    for record in manifest.get("records", []):
        for source in record.get("source_ids") or []:
            _validate_source_id(source)
            if source not in source_set:
                raise CrossMachineAdaptError("record source is outside the bound manifest sessions")
