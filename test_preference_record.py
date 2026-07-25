"""Tests for the optional machine-attribution fields on PreferenceRecord.

Adrian's decision: "adapt has to be by machine and should be recorded with
machine." `scope` (workspace-recall partition, IMMUTABLE_FIELDS in
manifest.py / part of payload_sha256) is left untouched. `machine` and
`machine_only` are new, optional, backward-compatible attribution fields —
NOT in REQUIRED_FIELDS, NOT part of the manifest candidate payload.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import preference_record as pr_mod


def _legacy_dict_without_machine() -> dict:
    """Shape of a pre-existing on-disk record — no machine/machine_only keys
    at all, matching every record written before this change."""
    return {
        "schema_version": "1.2.0",
        "id": "adapt-tooling-legacy-abc1234567",
        "kind": "preference",
        "rule": "Prefer JSONL for structured logs.",
        "category": "tooling",
        "scope": "D--Claude",
        "confidence": 0.7,
        "needs_review": False,
        "evidence_count": 1,
        "source_ids": ["s1"],
        "created_at": "2026-01-01T00:00:00+00:00",
        "updated_at": "2026-01-01T00:00:00+00:00",
    }


def test_legacy_record_without_machine_field_still_validates():
    """A pre-existing record dict with no machine/machine_only keys
    constructs fine — the field is optional and backward compatible."""
    rec = pr_mod.PreferenceRecord(**_legacy_dict_without_machine())
    assert rec.machine == ""
    assert rec.machine_only is False
    assert "machine" not in pr_mod.REQUIRED_FIELDS
    assert "machine_only" not in pr_mod.REQUIRED_FIELDS


def test_machine_absent_from_synthesis_still_validates():
    """The normal construction path (from_synthesis with no machine kwargs
    at all) never raises and yields the unqualified default."""
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "tooling",
         "rule": "Prefer JSONL for structured logs.", "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
    )
    assert rec.machine == ""
    assert rec.machine_only is False
    assert rec.scope == "D--Claude"  # scope contract untouched


def test_machine_field_present_round_trips_through_to_dict():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "tooling",
         "rule": "Prefer JSONL for structured logs.", "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
        machine="adrian-mac", machine_only=True,
    )
    assert rec.machine == "adrian-mac"
    assert rec.machine_only is True

    d = rec.to_dict()
    assert d["machine"] == "adrian-mac"
    assert d["machine_only"] is True
    d["source_ids"] = tuple(d["source_ids"])
    rec2 = pr_mod.PreferenceRecord(**d)
    assert rec2 == rec


def test_machine_field_absent_from_manifest_candidate_payload():
    """machine/machine_only must never leak into the manifest candidate: the
    manifest schema has additionalProperties: false and manifest.py's
    IMMUTABLE_FIELDS does not know about them."""
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "tooling",
         "rule": "Prefer JSONL for structured logs.", "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
        machine="adrian-mac", machine_only=True,
    )
    candidate = pr_mod.to_manifest_candidate(rec)
    assert "machine" not in candidate
    assert "machine_only" not in candidate


def test_to_memright_content_includes_machine_line_only_when_set():
    unset = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "tooling",
         "rule": "Prefer JSONL for structured logs.", "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
    )
    assert "**Machine:**" not in pr_mod.to_memright_content(unset)

    attributed = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "tooling",
         "rule": "Prefer JSONL for structured logs.", "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
        machine="adrian-mac", machine_only=True,
    )
    body = pr_mod.to_memright_content(attributed)
    assert "**Machine:** adrian-mac (machine-only)" in body


def test_machine_and_machine_only_preserved_across_update_when_omitted():
    """Updating a rule (existing= prior dict) without repeating machine
    kwargs preserves the original attribution and narrowing — matches the
    existing created_at-preservation pattern."""
    initial = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "tooling",
         "rule": "Prefer JSONL for structured logs.", "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
        machine="adrian-mac", machine_only=True,
    )
    updated = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "update", "name": initial.id, "category": "tooling",
         "rule": "Prefer JSONL for structured logs.", "confidence": 0.85},
        scope="D--Claude", source_ids=("s1",),
        existing=initial.to_dict(),
    )
    assert updated.id == initial.id
    assert updated.machine == "adrian-mac"
    assert updated.machine_only is True


def test_default_behavior_unchanged_applies_everywhere_not_machine_narrowed():
    """A caller that never mentions machine at all — the entire population of
    call sites before this change — gets exactly today's behavior: no
    narrowing, no attribution, workspace-wide recall."""
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",),
    )
    assert rec.machine_only is False
    assert rec.machine == ""


def test_default_machine_id_is_stable_across_two_calls(tmp_path):
    missing = tmp_path / "installation.json"
    first = pr_mod.default_machine_id(installation_file=missing)
    second = pr_mod.default_machine_id(installation_file=missing)
    assert first == second
    assert first  # never empty — falls back to platform+hostname


def test_default_machine_id_reuses_existing_installation_identity(tmp_path):
    """Reuses the SAME identity file/shape already loaded by
    cross_machine.load_installation_id for multiwriter conformance —
    legacy_labels, if present, wins as the human-readable machine name."""
    identity = tmp_path / "installation.json"
    identity.write_text(json.dumps({
        "schema_version": 2,
        "installation_id": "be8b2353-c0f5-4250-867c-22c5629bd4e8",
        "legacy_labels": ["adrian-mac"],
    }), encoding="utf-8")
    assert pr_mod.default_machine_id(installation_file=identity) == "adrian-mac"


def test_default_machine_id_falls_back_to_installation_id_without_label(tmp_path):
    identity = tmp_path / "installation.json"
    identity.write_text(json.dumps({
        "schema_version": 2,
        "installation_id": "be8b2353-c0f5-4250-867c-22c5629bd4e8",
        "legacy_labels": [],
    }), encoding="utf-8")
    assert pr_mod.default_machine_id(installation_file=identity) == \
        "be8b2353-c0f5-4250-867c-22c5629bd4e8"


def test_default_machine_id_falls_back_to_platform_hostname_when_no_file(tmp_path):
    """Never raises on a missing identity file; always returns a non-empty,
    already-trimmed label (platform-hostname, or 'unknown-machine')."""
    missing = tmp_path / "does-not-exist.json"
    result = pr_mod.default_machine_id(installation_file=missing)
    assert isinstance(result, str) and result != "" and result.strip() == result
