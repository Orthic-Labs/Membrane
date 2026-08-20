from __future__ import annotations

import pytest

from adapt import rule_key as rk


def test_rule_key_formats_scoped_identity() -> None:
    key = rk.RuleKey(scope="D--Claude", record_id="adapt-tooling-jsonl-abc")
    assert key.formatted() == "D--Claude/adapt-tooling-jsonl-abc"


def test_rule_index_resolves_bare_name_when_unique() -> None:
    index = rk.RuleIndex.from_mapping({
        "adapt-tooling-jsonl-abc": {
            "id": "adapt-tooling-jsonl-abc",
            "scope": "D--Claude",
            "rule": "Use JSONL.",
        }
    })
    resolved, row = index.resolve("adapt-tooling-jsonl-abc")
    assert resolved == rk.RuleKey("D--Claude", "adapt-tooling-jsonl-abc")
    assert row["rule"] == "Use JSONL."


def test_rule_index_flags_ambiguous_bare_id_across_scopes() -> None:
    rules = {
        "D--Claude/adapt-tooling-jsonl-abc": {
            "id": "adapt-tooling-jsonl-abc", "scope": "D--Claude", "rule": "one",
        },
        "Volumes-D-claude/adapt-tooling-jsonl-abc": {
            "id": "adapt-tooling-jsonl-abc", "scope": "Volumes-D-claude", "rule": "two",
        },
    }
    index = rk.RuleIndex.from_mapping(rules)
    resolved, row = index.resolve("adapt-tooling-jsonl-abc")
    assert resolved is None and row is None
    assert len(index.keys_for_id("adapt-tooling-jsonl-abc")) == 2
