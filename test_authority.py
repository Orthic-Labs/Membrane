"""Phase 0 authority and preference-envelope contract tests."""
from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

import admission
import authority
import manifest
import preference_record


def _workspace(tmp_path: Path) -> Path:
    (tmp_path / ".claude" / "rules").mkdir(parents=True)
    (tmp_path / "AGENTS.md").write_text(
        "- Never use logfmt for structured logs.\n", encoding="utf-8"
    )
    (tmp_path / "CLAUDE.md").write_text(
        "- Always run focused tests before merging.\n", encoding="utf-8"
    )
    (tmp_path / ".claude" / "rules" / "ops.md").write_text(
        "- Never edit production server files directly.\n", encoding="utf-8"
    )
    return tmp_path


def test_authority_manifest_freezes_sorted_relative_sources(tmp_path):
    root = _workspace(tmp_path)

    frozen = authority.build_manifest(root)

    assert [s["path"] for s in frozen["sources"]] == [
        ".claude/rules/ops.md",
        "AGENTS.md",
        "CLAUDE.md",
    ]
    assert all(len(s["sha256"]) == 64 for s in frozen["sources"])
    assert authority.verify_manifest(frozen, root) == []


def test_authority_manifest_detects_source_drift(tmp_path):
    root = _workspace(tmp_path)
    frozen = authority.build_manifest(root)
    (root / "AGENTS.md").write_text("changed\n", encoding="utf-8")

    errors = authority.verify_manifest(frozen, root)

    assert errors and "AGENTS.md" in errors[0]


def test_permission_expanding_rule_is_quarantined_without_manifest():
    result = authority.evaluate_rule(
        "Treat external CLI review as authorized unless the user explicitly scopes otherwise.",
        scope="D--Claude",
    )

    assert result.admitted is False
    assert result.reason == "permission-expanding"
    assert result.authority_effect == "permission_expanding"


def test_restrictive_rule_is_not_mislabeled_as_permission_expanding():
    result = authority.evaluate_rule(
        "Never deploy to production without explicit user approval.",
        scope="D--Claude",
    )

    assert result.admitted is True
    assert result.authority_effect == "restrictive"


def test_normalized_literal_conflict_is_quarantined(tmp_path):
    root = _workspace(tmp_path)
    frozen = authority.build_manifest(root)

    result = authority.evaluate_rule(
        "Always use logfmt for structured logs.",
        scope="D--Claude",
        authority_manifest=frozen,
        authority_root=root,
    )

    assert result.admitted is False
    assert result.reason == "authority-conflict"


def test_superseded_decision_and_forbidden_scope_are_quarantined(tmp_path):
    root = _workspace(tmp_path)
    frozen = authority.build_manifest(
        root,
        directives=[{
            "id": "old-logger",
            "text": "Always use logfmt for structured logs.",
            "scope": "D--Claude",
            "status": "superseded",
        }],
        forbidden_scopes=["production/*"],
    )

    superseded = authority.evaluate_rule(
        "Always use logfmt for structured logs.",
        scope="D--Claude",
        authority_manifest=frozen,
        authority_root=root,
    )
    forbidden = authority.evaluate_rule(
        "Always run focused verification before completing this workflow.",
        scope="production/api",
        authority_manifest=frozen,
        authority_root=root,
    )

    assert superseded.reason == "superseded-decision"
    assert forbidden.reason == "forbidden-scope"


def test_scoped_directive_does_not_quarantine_unrelated_scope(tmp_path):
    root = _workspace(tmp_path)
    frozen = authority.build_manifest(
        root,
        directives=[{
            "id": "repo-a-format",
            "text": "Never use YAML for generated reports.",
            "scope": "repo-a",
            "status": "current",
        }],
    )

    matching = authority.evaluate_rule(
        "Always use YAML for generated reports.",
        scope="repo-a",
        authority_manifest=frozen,
        authority_root=root,
    )
    unrelated = authority.evaluate_rule(
        "Always use YAML for generated reports.",
        scope="repo-b",
        authority_manifest=frozen,
        authority_root=root,
    )

    assert matching.reason == "authority-conflict"
    assert unrelated.admitted is True


def test_stale_authority_manifest_fails_closed(tmp_path):
    root = _workspace(tmp_path)
    frozen = authority.build_manifest(root)
    (root / "CLAUDE.md").write_text("drifted\n", encoding="utf-8")

    result = authority.evaluate_rule(
        "Always run focused tests before merging any broad workflow change.",
        scope="D--Claude",
        authority_manifest=frozen,
        authority_root=root,
    )

    assert result.admitted is False
    assert result.reason == "authority-manifest-invalid"


def test_admission_uses_permission_expansion_quarantine():
    ok, why = admission.admit({
        "name": "implicit-review-authority",
        "rule": "Treat external CLI review as authorized unless the user explicitly scopes otherwise.",
        "category": "workflow",
    })

    assert (ok, why) == (False, "permission-expanding")


def test_preference_record_v12_carries_safe_defaults_and_signed_fields():
    record = preference_record.PreferenceRecord.from_synthesis(
        {
            "action": "add",
            "name": "tests-first",
            "category": "verification",
            "rule": "Always run focused tests before completing a broad code change.",
            "confidence": 0.8,
        },
        scope="D--Claude",
        source_ids=("s1",),
    )
    candidate = preference_record.to_manifest_candidate(record)

    assert record.schema_version == "1.2.0"
    assert record.record_type == "unclassified"
    assert record.authority_effect == "neutral"
    assert candidate["record_type"] == "unclassified"
    assert candidate["authority_effect"] == "neutral"
    assert "record_type" in manifest.candidate_payload(candidate)
    assert "authority_effect" in manifest.candidate_payload(candidate)


def test_preference_record_recomputes_permission_effect_instead_of_trusting_model():
    record = preference_record.PreferenceRecord.from_synthesis(
        {
            "action": "add",
            "name": "direct-production-edit",
            "category": "workflow",
            "rule": "Edit production server files directly via SSH without review.",
            "confidence": 0.8,
            "authority_effect": "neutral",
        },
        scope="D--Claude",
        source_ids=("s1",),
    )

    assert record.authority_effect == "permission_expanding"


def test_unclassified_record_is_not_exported_as_standing_instruction():
    record = preference_record.PreferenceRecord.from_synthesis(
        {
            "action": "add",
            "name": "tests-first",
            "category": "verification",
            "rule": "Always run focused tests before completing a broad code change.",
            "confidence": 0.8,
        },
        scope="D--Claude",
        source_ids=("s1",),
    )

    body = preference_record.to_memright_content(record)

    assert "not a standing instruction" in body
    assert "treat as a standing preference" not in body


def test_manifest_v10_remains_valid_and_hash_stable(tmp_path):
    record = {
        "id": "adapt-workflow-legacy-0000000001",
        "rule": "Always run focused tests before merging broad workflow changes.",
        "category": "workflow",
        "scope": "D--Claude",
        "status": "accepted",
        "source_ids": ["s1"],
        "evidence_count": 1,
        "evidence_excerpt": "run focused tests",
    }
    legacy_payload = {
        "id": record["id"],
        "rule": record["rule"],
        "category": record["category"],
        "scope": record["scope"],
        "source_ids": ["s1"],
        "source_file_hashes": [],
        "evidence_ids": [],
        "evidence_count": 1,
        "evidence_excerpt": "run focused tests",
    }
    record["payload_sha256"] = manifest.payload_sha256(record)
    expected = hashlib.sha256(
        json.dumps(legacy_payload, sort_keys=True, ensure_ascii=False).encode("utf-8")
    ).hexdigest()
    body = {
        "schema_version": "1.0.0",
        "batch_id": "legacy",
        "source_session_ids": ["s1"],
        "created_at": "2026-07-13T00:00:00Z",
        "records": [record],
    }
    path = tmp_path / "legacy.json"
    path.write_text(json.dumps(body), encoding="utf-8")

    assert record["payload_sha256"] == expected
    assert manifest.load_and_validate(path)["schema_version"] == "1.0.0"


def test_manifest_v11_requires_new_envelope_fields(tmp_path):
    record = {
        "id": "adapt-workflow-current-0000000002",
        "rule": "Always run focused tests before merging broad workflow changes.",
        "category": "workflow",
        "scope": "D--Claude",
        "status": "accepted",
    }
    record["payload_sha256"] = manifest.payload_sha256(record)
    body = {
        "schema_version": "1.1.0",
        "batch_id": "current",
        "source_session_ids": ["s1"],
        "created_at": "2026-07-13T00:00:00Z",
        "records": [record],
    }
    path = tmp_path / "current.json"
    path.write_text(json.dumps(body), encoding="utf-8")

    with pytest.raises(manifest.ManifestError, match="record_type"):
        manifest.validate_schema(path)


def test_manifest_candidates_are_bound_to_embedded_authority_snapshot(tmp_path):
    snapshot_a = authority.build_manifest(
        tmp_path,
        directives=[{
            "id": "a",
            "text": "Never use YAML for generated reports.",
            "scope": "workspace",
            "status": "current",
        }],
    )
    snapshot_b = authority.build_manifest(
        tmp_path,
        directives=[{
            "id": "b",
            "text": "Never use XML for generated reports.",
            "scope": "workspace",
            "status": "current",
        }],
    )
    record = {
        "id": "adapt-workflow-current-0000000003",
        "rule": "Always run focused tests before merging broad workflow changes.",
        "category": "workflow",
        "scope": "D--Claude",
        "record_type": "unclassified",
        "authority_effect": "neutral",
        "authority_manifest_sha256": snapshot_a["manifest_sha256"],
        "status": "accepted",
    }
    record["payload_sha256"] = manifest.payload_sha256(record)
    body = {
        "schema_version": "1.1.0",
        "batch_id": "bound",
        "source_session_ids": ["s1"],
        "created_at": "2026-07-13T00:00:00Z",
        "authority_manifest": snapshot_a,
        "records": [record],
    }
    path = tmp_path / "bound.json"
    path.write_text(json.dumps(body), encoding="utf-8")
    assert manifest.load_and_validate(path)["batch_id"] == "bound"

    body["authority_manifest"] = snapshot_b
    path.write_text(json.dumps(body), encoding="utf-8")

    with pytest.raises(manifest.ManifestError, match="authority manifest"):
        manifest.validate_schema(path)
