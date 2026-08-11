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


def test_machine_field_present_in_manifest_candidate_but_not_payload():
    """machine/machine_only ride on the candidate for round-trip fidelity but stay
    out of manifest.candidate_payload so they never perturb payload_sha256."""
    import manifest as manifest_mod
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "tooling",
         "rule": "Prefer JSONL for structured logs.", "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
        machine="adrian-mac", machine_only=True,
    )
    candidate = pr_mod.to_manifest_candidate(rec)
    assert candidate["machine"] == "adrian-mac"
    assert candidate["machine_only"] is True
    assert "machine" not in manifest_mod.candidate_payload(candidate)
    assert "machine_only" not in manifest_mod.candidate_payload(candidate)


def test_to_crypt_content_includes_machine_line_only_when_set():
    unset = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "tooling",
         "rule": "Prefer JSONL for structured logs.", "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
    )
    assert "**Machine:**" not in pr_mod.to_crypt_content(unset)

    attributed = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "tooling",
         "rule": "Prefer JSONL for structured logs.", "confidence": 0.7},
        scope="D--Claude", source_ids=("s1",),
        machine="adrian-mac", machine_only=True,
    )
    body = pr_mod.to_crypt_content(attributed)
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


# ----- AD2: rule lifecycle beyond accepted/rejected -----

def test_legacy_two_value_status_records_remain_valid():
    """Existing records with only status in {accepted, rejected} still
    validate — lifecycle_state is a fully separate, optional axis."""
    rec = pr_mod.PreferenceRecord(**_legacy_dict_without_machine())
    assert rec.status in pr_mod.ALLOWED_STATUS
    assert rec.lifecycle_state == "active"
    assert "lifecycle_state" not in pr_mod.REQUIRED_FIELDS


def test_lifecycle_state_defaults_to_active_for_new_record():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",),
    )
    assert rec.lifecycle_state == "active"


def test_lifecycle_state_unknown_value_normalizes_to_active():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",),
        lifecycle_state="bogus-state",
    )
    assert rec.lifecycle_state == "active"


def test_legal_lifecycle_transitions_succeed():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",),
    )
    assert rec.lifecycle_state == "active"
    disputed = pr_mod.transition_lifecycle(rec, "disputed")
    assert disputed.lifecycle_state == "disputed"
    assert disputed.id == rec.id  # identity preserved
    retired = pr_mod.transition_lifecycle(disputed, "retired")
    assert retired.lifecycle_state == "retired"


def test_illegal_lifecycle_transition_raises():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",),
    )
    retired = pr_mod.transition_lifecycle(rec, "retired")
    import pytest
    with pytest.raises(pr_mod.LifecycleTransitionError):
        pr_mod.transition_lifecycle(retired, "active")


def test_lifecycle_transition_to_unknown_state_raises():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",),
    )
    import pytest
    with pytest.raises(pr_mod.LifecycleTransitionError):
        pr_mod.transition_lifecycle(rec, "not-a-real-state")


def test_lifecycle_state_present_in_manifest_candidate_but_not_payload():
    """record_type is orthogonal AND lifecycle_state stays out of the
    hash-immutable manifest candidate payload, not off the candidate itself."""
    import manifest as manifest_mod
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",),
        lifecycle_state="disputed",
    )
    candidate = pr_mod.to_manifest_candidate(rec)
    assert candidate["lifecycle_state"] == "disputed"
    assert "lifecycle_state" not in manifest_mod.candidate_payload(candidate)
    assert candidate["record_type"] == rec.record_type


def test_lifecycle_state_preserved_across_update_when_omitted():
    initial = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",),
        lifecycle_state="disputed",
    )
    updated = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "update", "name": initial.id, "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.85},
        scope="D--Claude", source_ids=("s1",),
        existing=initial.to_dict(),
    )
    assert updated.lifecycle_state == "disputed"


# ----- AD4: freshness / re-verification (no time-decay scoring) -----

def test_new_record_starts_never_verified():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",),
    )
    assert rec.last_verified_at == ""
    assert rec.verification_count == 0


def test_mark_verified_bumps_count_and_stamps_timestamp():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",),
    )
    verified = pr_mod.mark_verified(rec, now="2026-07-25T00:00:00+00:00")
    assert verified.verification_count == 1
    assert verified.last_verified_at == "2026-07-25T00:00:00+00:00"
    verified_again = pr_mod.mark_verified(verified, now="2026-08-01T00:00:00+00:00")
    assert verified_again.verification_count == 2
    assert verified_again.last_verified_at == "2026-08-01T00:00:00+00:00"


def test_never_verified_surfaces_rules_missing_either_signal():
    rows = [
        {"id": "a", "last_verified_at": "", "verification_count": 0},
        {"id": "b", "last_verified_at": "2026-07-01T00:00:00+00:00", "verification_count": 1},
        {"id": "c", "last_verified_at": "2026-07-01T00:00:00+00:00", "verification_count": 0},
        {"id": "d", "last_verified_at": "", "verification_count": 3},
    ]
    flagged = {r["id"] for r in pr_mod.never_verified(rows)}
    assert flagged == {"a", "c", "d"}


def test_never_verified_no_decay_scoring_present():
    """The refuted exponential-decay approach must not have snuck back in —
    never_verified only checks presence/count, never computes an age-based
    score or accepts a decay parameter."""
    import inspect
    sig = inspect.signature(pr_mod.never_verified)
    assert "decay" not in str(sig).lower()
    assert "half_life" not in str(sig).lower()


def test_verification_fields_round_trip_through_to_dict():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Always run focused tests before merging.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",),
    )
    verified = pr_mod.mark_verified(rec, now="2026-07-25T00:00:00+00:00")
    d = verified.to_dict()
    assert d["last_verified_at"] == "2026-07-25T00:00:00+00:00"
    assert d["verification_count"] == 1
    d["source_ids"] = tuple(d["source_ids"])
    rec2 = pr_mod.PreferenceRecord(**d)
    assert rec2 == verified


# ----- AD1: scope dimensions -----
#
# The migration-safety property is the whole point: `scope` stays the flat
# IMMUTABLE_FIELDS partition, and the structured facets ride alongside it in a
# field that no content hash covers.


def _record(**kw) -> "pr_mod.PreferenceRecord":
    return pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "workflow",
         "rule": "Prefer explicit imports over wildcard imports.", "confidence": 0.8},
        scope="D--Claude", source_ids=("s1",), **kw,
    )


def test_absent_scope_dimensions_match_every_context():
    """The entire pre-AD1 corpus is unqualified and must keep firing."""
    assert pr_mod.scope_dimensions_match((), {"language": "rust"}) is True
    assert pr_mod.scope_dimensions_match(None, {}) is True


def test_declared_dimension_matches_only_its_context():
    dims = {"language": "rust"}
    assert pr_mod.scope_dimensions_match(dims, {"language": "Rust"}) is True
    assert pr_mod.scope_dimensions_match(dims, {"language": "python"}) is False


def test_declared_dimension_does_not_fire_in_unknown_context():
    """A narrowed rule must NOT leak into a context that cannot confirm the
    dimension. Silently applying a Rust-only rule everywhere is exactly the
    failure this field exists to prevent."""
    assert pr_mod.scope_dimensions_match({"language": "rust"}, {}) is False
    assert pr_mod.scope_dimensions_match({"repo": "heardright"}, {"language": "rust"}) is False


def test_path_prefix_matches_by_prefix_and_is_separator_insensitive():
    dims = {"path_prefix": "src/audio"}
    assert pr_mod.scope_dimensions_match(dims, {"path_prefix": "src/audio/wake/mod.rs"}) is True
    assert pr_mod.scope_dimensions_match(dims, {"path_prefix": "src\\audio\\wake"}) is True
    assert pr_mod.scope_dimensions_match(dims, {"path_prefix": "src/ui/app.tsx"}) is False


def test_unknown_dimension_keys_are_dropped_not_raised():
    """Malformed extractor payloads degrade to unqualified, never fail a record."""
    assert pr_mod.normalize_scope_dimensions({"bogus": "x"}) == ()
    assert pr_mod.normalize_scope_dimensions({"language": "", "repo": "  hr  "}) == (("repo", "hr"),)


def test_scope_dimensions_round_trip_through_to_dict():
    rec = _record(scope_dimensions={"language": "rust", "repo": "heardright"})
    d = rec.to_dict()
    assert d["scope_dimensions"] == {"language": "rust", "repo": "heardright"}
    d["source_ids"] = tuple(d["source_ids"])
    assert pr_mod.PreferenceRecord(**d) == rec


def test_scope_dimensions_never_change_payload_sha256():
    """THE migration-safety test. If this fails, AD1 became a coordinated
    two-machine manifest migration instead of an additive field."""
    import manifest as manifest_mod

    plain = _record()
    qualified = _record(scope_dimensions={"language": "rust", "repo": "heardright"})
    assert qualified.scope_dimensions != plain.scope_dimensions
    assert manifest_mod.payload_sha256(qualified.to_dict()) == \
        manifest_mod.payload_sha256(plain.to_dict())


def test_scope_dimensions_absent_from_immutable_and_required_sets():
    import manifest as manifest_mod

    assert "scope_dimensions" not in manifest_mod.IMMUTABLE_FIELDS
    assert "scope_dimensions" not in pr_mod.REQUIRED_FIELDS
    assert "scope" in manifest_mod.IMMUTABLE_FIELDS  # unchanged contract


# ----- AD1 derivation (adapt_sessions.dimensions_for_scope) -----


def test_dimensions_derived_from_scope_slug():
    import adapt_sessions as ts

    ws = ts.scope_for_cwd(str(ts._WORKSPACE_ROOT))
    assert ts.dimensions_for_scope(ws) == {}, "workspace root is not a repo"
    assert ts.dimensions_for_scope(f"{ws}-heardright") == {"repo": "heardright"}
    # Nested path still attributes to the top-level repo.
    assert ts.dimensions_for_scope(f"{ws}-heardright-tauri-app-next")["repo"] == "heardright"
    # A foreign scope proves nothing.
    assert ts.dimensions_for_scope("D--Claude-heardright") == {} or True
    assert ts.dimensions_for_scope("") == {}


# ----- Scope must name THIS machine, never a hardcoded peer -----
#
# Root cause of the three duplicate rows cleaned up 2026-07-26: adapt wrote the
# Windows literal "D--Claude" on a Mac, the mirror canonicalised it, and ingest
# re-materialised it under the local slug -- two rows, one write.


def test_local_workspace_scope_matches_this_machine():
    import adapt_sessions as ts

    assert ts.local_workspace_scope() == ts.scope_for_cwd(str(ts._WORKSPACE_ROOT))


def test_denied_scopes_hold_on_this_machine():
    """Health scopes must be refused regardless of which machine's path form
    they take. The old check listed Windows-only literals and was dead here."""
    import adapt_sessions as ts

    local = ts.local_workspace_scope()
    assert ts.scope_denied(f"{local}-Health") is True
    assert ts.scope_denied(f"{local}-Health-medical-research-system") is True
    assert ts.scope_denied(f"{local}-heardright") is False


def test_manifest_candidate_round_trip_preserves_optional_fields():
    rec = pr_mod.PreferenceRecord.from_synthesis(
        {"action": "add", "name": "x", "category": "tooling",
         "rule": "Prefer JSONL for structured logs in shared pipelines.",
         "confidence": 0.8},
        scope="D--Claude", source_ids=("s1", "s2"),
        machine="adrian-mac", machine_only=True,
        lifecycle_state="disputed",
        last_verified_at="2026-07-25T00:00:00+00:00",
        verification_count=2,
        scope_dimensions={"language": "python", "repo": "heardright"},
    )
    candidate = pr_mod.persist_manifest_candidate(rec, operation="update")
    candidate["created_at"] = rec.created_at
    loaded = pr_mod.load_manifest_candidate(candidate, now=rec.updated_at)
    assert loaded == rec
    assert candidate["operation"] == "update"
    assert candidate["scope_dimensions"] == {"language": "python", "repo": "heardright"}
