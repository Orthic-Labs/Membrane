from __future__ import annotations

import json
import sqlite3
from pathlib import Path

import pytest

from adapt import cross_machine as cm


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


def _legacy_db(path: Path) -> Path:
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
            "D--Claude/adapt-tooling-jsonl-legacy",
            "D--Claude",
            "**[adapt/tooling]** — Keep structured logs in JSONL. "
            "Confidence: 0.80 (observations: 2, needs_review: false, updated 2026-07-20)\n"
            "**Why:** preference record; id=adapt-tooling-jsonl-legacy, evidence_count=2\n"
            "**Record:** type=standing_preference, authority_effect=neutral\n",
            "[]",
            "2026-07-13T00:00:00Z",
            "2026-07-20T00:00:00Z",
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


@pytest.mark.parametrize("tool", ["command-code", "cline", "gemini", "grok-build", "roo-cline"])
def test_external_harness_sources_use_the_same_qualified_contract(tool: str) -> None:
    source = cm.qualify_source_session(INSTALLATION, tool, "private-session")
    assert source.startswith(f"install:{INSTALLATION}:{tool}:")
    assert "private-session" not in source


def test_canonical_rule_pool_is_rebuilt_from_engine_not_local_cache(tmp_path: Path) -> None:
    rules = cm.load_canonical_rules(_db(tmp_path / "engine.db"))

    # Keys are scope-qualified: one rule name can legitimately exist under two
    # scope spellings for the same workspace across machines.
    assert set(rules) == {"D--Claude/adapt-tooling-jsonl-1111111111"}
    rule = rules["D--Claude/adapt-tooling-jsonl-1111111111"]
    assert rule["scope"] == "D--Claude"
    assert rule["category"] == "tooling"
    assert rule["rule"] == "Always use JSONL for structured logs in shared pipelines."
    assert rule["source_ids"] == ["install:old:codex:source"]
    assert rule["record_type"] == "standing_preference"
    assert rule["authority_effect"] == "neutral"
    assert rule["retrieval_aliases"] == []
    assert rule["lifecycle_state"] == "active"
    assert rule["verification_count"] == 0


def test_canonical_rule_pool_supports_live_schema_without_record_type(tmp_path: Path) -> None:
    rules = cm.load_canonical_rules(_legacy_db(tmp_path / "legacy-engine.db"))

    assert rules["D--Claude/adapt-tooling-jsonl-legacy"]["record_type"] == "standing_preference"


def test_canonical_rule_pool_excludes_legacy_plain_adapt_row(tmp_path: Path) -> None:
    path = tmp_path / "plain-engine.db"
    conn = sqlite3.connect(path)
    conn.execute(
        "CREATE TABLE memories (id TEXT PRIMARY KEY, scope_id TEXT, content TEXT, "
        "source_ids TEXT, created_at TEXT, updated_at TEXT)"
    )
    conn.execute(
        "INSERT INTO memories VALUES (?, ?, ?, ?, ?, ?)",
        (
            "global/adapt-review-external-explicit-opt-in",
            "global",
            "External review is explicit opt-in.",
            "[]",
            "2026-07-13T00:00:00Z",
            "2026-07-13T00:00:00Z",
        ),
    )
    conn.commit()
    conn.close()

    assert cm.load_canonical_rules(path) == {}


def test_canonical_rule_pool_excludes_adapt_insight_reports(tmp_path: Path) -> None:
    path = _db(tmp_path / "insights-engine.db")
    conn = sqlite3.connect(path)
    conn.execute(
        "INSERT INTO memories VALUES (?, ?, ?, ?, ?, ?, ?)",
        (
            "workspace/adapt-insight-ignored-tool-failure",
            "workspace",
            "**[adapt/insight]** ignored tool failure\nObserved heuristic cards.",
            "[]",
            "2026-08-24T00:00:00Z",
            "2026-08-24T00:00:00Z",
            "insight_report",
        ),
    )
    conn.commit()
    conn.close()

    rules = cm.load_canonical_rules(path)

    assert set(rules) == {"D--Claude/adapt-tooling-jsonl-1111111111"}


def test_pool_digest_is_order_independent_and_changes_with_canonical_content() -> None:
    first = {
        "a": {"name": "a", "scope": "s", "category": "workflow", "rule": "one"},
        "b": {"name": "b", "scope": "s", "category": "tooling", "rule": "two"},
    }
    reordered = {"b": first["b"], "a": first["a"]}
    changed = {**first, "b": {**first["b"], "rule": "changed"}}
    changed_confidence = {**first, "b": {**first["b"], "confidence": 0.9}}
    changed_lifecycle = {**first, "b": {**first["b"], "lifecycle_state": "superseded"}}

    assert cm.canonical_pool_sha256(first) == cm.canonical_pool_sha256(reordered)
    assert cm.canonical_pool_sha256(first) != cm.canonical_pool_sha256(changed)
    assert cm.canonical_pool_sha256(first) != cm.canonical_pool_sha256(changed_confidence)
    assert cm.canonical_pool_sha256(first) != cm.canonical_pool_sha256(changed_lifecycle)


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


def test_same_rule_under_two_machine_scopes_is_not_a_duplicate(tmp_path: Path) -> None:
    """The live failure this guards: one workspace is scoped 'D--Claude' on Windows
    and 'Volumes-D-claude' on macOS. Syncing one rule across both machines produced
    two rows sharing a name, which raised and disabled canonical context entirely."""
    path = _db(tmp_path / "engine.db")
    import sqlite3
    with sqlite3.connect(path) as conn:
        row = conn.execute(
            "SELECT id, scope_id, content, source_ids, created_at, updated_at, record_type "
            "FROM memories WHERE id LIKE '%adapt-tooling-jsonl-1111111111'"
        ).fetchone()
        mac = ("Volumes-D-claude/" + row[0].split("/", 1)[1], "Volumes-D-claude") + tuple(row[2:])
        conn.execute(
            "INSERT INTO memories (id, scope_id, content, source_ids, created_at, "
            "updated_at, record_type) VALUES (?,?,?,?,?,?,?)", mac
        )

    rules = cm.load_canonical_rules(path)
    assert set(rules) == {
        "D--Claude/adapt-tooling-jsonl-1111111111",
        "Volumes-D-claude/adapt-tooling-jsonl-1111111111",
    }

    # A real duplicate -- same name in the SAME scope -- must still raise, even
    # though the id (the SQLite primary key) differs, because `_parse_rule_row`
    # derives the rule's identity by stripping the `scope/` id-prefix, not from
    # the raw id string. A bare-name row with no scope prefix at all still
    # produces the same derived name under the "D--Claude" scope column.
    with sqlite3.connect(path) as conn:
        conn.execute(
            "INSERT INTO memories (id, scope_id, content, source_ids, created_at, "
            "updated_at, record_type) VALUES (?,?,?,?,?,?,?)",
            ("adapt-tooling-jsonl-1111111111", "D--Claude") + tuple(row[2:]),
        )
    with pytest.raises(cm.CrossMachineAdaptError, match="duplicate canonical Adapt identity"):
        cm.load_canonical_rules(path)
