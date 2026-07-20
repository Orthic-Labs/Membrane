from __future__ import annotations

import json
import sqlite3
from pathlib import Path

import pytest

import cross_machine as cm


INSTALLATION = "08c7ef55-8f6b-4ef1-b234-22232b8ea832"
OTHER_INSTALLATION = "cbf1f308-4ea9-46db-8db1-89df616a41b2"


def _db(path: Path) -> Path:
    conn = sqlite3.connect(path)
    conn.execute(
        "CREATE TABLE memories ("
        "id TEXT PRIMARY KEY, scope_id TEXT NOT NULL, content TEXT NOT NULL, "
        "source_ids TEXT NOT NULL DEFAULT '[]', created_at TEXT NOT NULL, "
        "updated_at TEXT NOT NULL, record_type TEXT NOT NULL DEFAULT 'memory')"
    )
    conn.execute(
        "INSERT INTO memories VALUES (?, ?, ?, ?, ?, ?, ?)",
        (
            "D--Claude/adapt-tooling-jsonl-1111111111",
            "D--Claude",
            "**[adapt/tooling]** — Always use JSONL for structured logs in shared pipelines. "
            "Confidence: 0.80 (observations: 3, needs_review: false, updated 2026-07-20)\n"
            "**Why:** preference record; id=adapt-tooling-jsonl-1111111111, evidence_count=3\n"
            "**Record:** type=standing_preference, authority_effect=neutral\n",
            json.dumps(["install:old:codex:source"]),
            "2026-07-13T00:00:00Z",
            "2026-07-20T00:00:00Z",
            "standing_preference",
        ),
    )
    conn.execute(
        "INSERT INTO memories VALUES (?, ?, ?, ?, ?, ?, ?)",
        (
            "D--Claude/unrelated-memory",
            "D--Claude",
            "not an Adapt preference",
            "[]",
            "2026-07-13T00:00:00Z",
            "2026-07-20T00:00:00Z",
            "memory",
        ),
    )
    conn.commit()
    conn.close()
    return path


def test_session_sources_are_installation_qualified_and_content_free() -> None:
    first = cm.qualify_source_session(INSTALLATION, "codex", "local-session-123")
    same = cm.qualify_source_session(INSTALLATION, "codex", "local-session-123")
    other = cm.qualify_source_session(OTHER_INSTALLATION, "codex", "local-session-123")

    assert first == same
    assert first != other
    assert first.startswith(f"install:{INSTALLATION}:codex:")
    assert "local-session-123" not in first

    for tool in ("cline", "commandcode"):
        source = cm.qualify_source_session(INSTALLATION, tool, "local-session-123")
        assert f":{tool}:" in source
        assert "local-session-123" not in source


def test_canonical_rule_pool_is_rebuilt_from_engine_not_local_cache(tmp_path: Path) -> None:
    rules = cm.load_canonical_rules(_db(tmp_path / "engine.db"))

    assert set(rules) == {"adapt-tooling-jsonl-1111111111"}
    rule = rules["adapt-tooling-jsonl-1111111111"]
    assert rule["scope"] == "D--Claude"
    assert rule["category"] == "tooling"
    assert rule["rule"] == "Always use JSONL for structured logs in shared pipelines."
    assert rule["source_ids"] == ["install:old:codex:source"]
    assert rule["record_type"] == "standing_preference"
    assert rule["authority_effect"] == "neutral"
    assert rule["retrieval_aliases"] == []


def test_canonical_rule_pool_reads_pre_record_type_database(tmp_path: Path) -> None:
    path = tmp_path / "legacy-engine.db"
    conn = sqlite3.connect(path)
    conn.execute(
        "CREATE TABLE memories ("
        "id TEXT PRIMARY KEY, scope_id TEXT NOT NULL, content TEXT NOT NULL, "
        "source_ids TEXT NOT NULL DEFAULT '[]', created_at TEXT NOT NULL, "
        "updated_at TEXT NOT NULL)"
    )
    conn.execute(
        "INSERT INTO memories VALUES (?, ?, ?, ?, ?, ?)",
        (
            "D--Claude/adapt-workflow-legacy-1111111111",
            "D--Claude",
            "**[adapt/workflow]** — Preserve legacy Adapt rules during candidate upgrades. "
            "Confidence: 0.80 (observations: 2, needs_review: false, updated 2026-07-20)\n"
            "**Record:** type=standing_preference, authority_effect=neutral\n",
            "[]",
            "2026-07-13T00:00:00Z",
            "2026-07-20T00:00:00Z",
        ),
    )
    conn.commit()
    conn.close()

    rules = cm.load_canonical_rules(path)

    assert rules["adapt-workflow-legacy-1111111111"]["record_type"] == (
        "standing_preference"
    )


def test_canonical_rule_pool_excludes_non_pipeline_adapt_prefixed_rows(
    tmp_path: Path,
) -> None:
    path = _db(tmp_path / "engine.db")
    conn = sqlite3.connect(path)
    conn.execute(
        "INSERT INTO memories VALUES (?, ?, ?, ?, ?, ?, ?)",
        (
            "D--Claude/adapt-manual-feedback",
            "D--Claude",
            "A manually authored memory outside the Adapt pipeline.",
            "[]",
            "2026-07-13T00:00:00Z",
            "2026-07-20T00:00:00Z",
            "memory",
        ),
    )
    conn.commit()
    conn.close()

    rules = cm.load_canonical_rules(path)

    assert "adapt-manual-feedback" not in rules


def test_pool_digest_is_order_independent_and_changes_with_canonical_content() -> None:
    first = {
        "a": {"name": "a", "scope": "s", "category": "workflow", "rule": "one"},
        "b": {"name": "b", "scope": "s", "category": "tooling", "rule": "two"},
    }
    reordered = {"b": first["b"], "a": first["a"]}
    changed = {**first, "b": {**first["b"], "rule": "changed"}}
    changed_confidence = {**first, "b": {**first["b"], "confidence": 0.9}}

    assert cm.canonical_pool_sha256(first) == cm.canonical_pool_sha256(reordered)
    assert cm.canonical_pool_sha256(first) != cm.canonical_pool_sha256(changed)
    assert cm.canonical_pool_sha256(first) != cm.canonical_pool_sha256(changed_confidence)


def test_multiwriter_apply_refuses_wrong_installation_or_stale_pool() -> None:
    rules = {
        "a": {"name": "a", "scope": "s", "category": "workflow", "rule": "one"}
    }
    source = cm.qualify_source_session(INSTALLATION, "claude-code", "session-1")
    manifest = {
        "installation_id": INSTALLATION,
        "canonical_pool_sha256": cm.canonical_pool_sha256(rules),
        "source_session_ids": [source],
        "records": [{"source_ids": [source]}],
    }

    cm.validate_multiwriter_binding(
        manifest, installation_id=INSTALLATION, canonical_rules=rules
    )
    with pytest.raises(cm.CrossMachineAdaptError, match="installation"):
        cm.validate_multiwriter_binding(
            manifest, installation_id=OTHER_INSTALLATION, canonical_rules=rules
        )
    with pytest.raises(cm.CrossMachineAdaptError, match="pool changed"):
        cm.validate_multiwriter_binding(
            manifest,
            installation_id=INSTALLATION,
            canonical_rules={**rules, "b": {"name": "b", "rule": "two"}},
        )
