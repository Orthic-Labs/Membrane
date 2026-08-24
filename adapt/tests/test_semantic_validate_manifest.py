from __future__ import annotations

import pytest

from adapt import semantic_validate_manifest as semantic


def _record(record_id: str, evidence_count: int = 1) -> dict:
    return {
        "id": record_id,
        "rule": f"Keep durable rule {record_id}.",
        "scope": "workspace",
        "category": "workflow",
        "record_type": "standing_preference",
        "authority_effect": "neutral",
        "evidence_count": evidence_count,
    }


def test_parse_response_requires_exact_coverage() -> None:
    parsed = semantic._parse_response(
        '[{"id":"a","verdict":"valid","flags":[],"related_ids":[],"reason":"supported"}]',
        {"a"},
    )
    assert parsed["a"]["verdict"] == "valid"

    with pytest.raises(semantic.SemanticValidationError, match="coverage"):
        semantic._parse_response(
            '[{"id":"b","verdict":"valid","flags":[],"related_ids":[],"reason":"supported"}]',
            {"a"},
        )


def test_existing_canonical_duplicate_wins() -> None:
    candidate = _record("candidate")
    reviews = {
        "candidate": {
            "verdict": "valid",
            "flags": ["duplicate"],
            "related_ids": ["existing"],
        }
    }
    canonical = {
        "workspace/existing": {
            "id": "existing",
            "lifecycle_state": "active",
        }
    }
    assert semantic._resolve_candidates([candidate], reviews, canonical) == set()


def test_candidate_duplicate_component_keeps_strongest_evidence() -> None:
    first = _record("first", evidence_count=3)
    second = _record("second", evidence_count=1)
    reviews = {
        "first": {"verdict": "valid", "flags": ["duplicate"], "related_ids": ["second"]},
        "second": {"verdict": "valid", "flags": ["duplicate"], "related_ids": ["first"]},
    }
    assert semantic._resolve_candidates([first, second], reviews, {}) == {"first"}


def test_nonduplicate_hard_flag_rejects_candidate() -> None:
    reviews = {
        "candidate": {
            "verdict": "valid",
            "flags": ["overgeneralized"],
            "related_ids": [],
        }
    }
    assert semantic._resolve_candidates([_record("candidate")], reviews, {}) == set()
