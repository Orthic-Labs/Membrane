from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest


MODULE_PATH = Path(__file__).parent / "multiwriter_conformance.py"


def _module():
    spec = importlib.util.spec_from_file_location("multiwriter_conformance", MODULE_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


NOW = datetime(2026, 7, 20, 8, 0, tzinfo=timezone.utc)
INSTALLATION_ID = "12345678-1234-4234-9234-123456789abc"


def _sha(value: bytes = b"x") -> str:
    return hashlib.sha256(value).hexdigest()


def _evidence() -> dict:
    implementation_files = [
        {"path": "morph/cross_machine.py", "sha256": _sha(b"cross")},
        {"path": "morph/morph.py", "sha256": _sha(b"morph")},
    ]
    aggregate = _sha(json.dumps(
        implementation_files,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode("utf-8"))
    return {
        "installation_id": INSTALLATION_ID,
        "canonical_pool": {"sha256": _sha(b"pool"), "rules": 70},
        "implementation": {
            "files": implementation_files,
            "aggregate_sha256": aggregate,
        },
        "installed_service": {
            "binary_role": "service",
            "binary_sha256": _sha(b"binary"),
            "release_manifest_sha256": _sha(b"release"),
            "release_generation": "sha256:" + _sha(b"tree"),
            "service_generation": "sha256:" + _sha(b"boot"),
            "batch_route_status": 400,
            "batch_route_capable": True,
        },
        "discovery": {
            "claude": {"discovered": 300, "parsed": 297, "unreadable": 1, "malformed": 1, "skipped": 1, "pending": 12},
            "claudemm": {"discovered": 121, "parsed": 120, "unreadable": 0, "malformed": 1, "skipped": 0, "pending": 7},
            "codex": {"discovered": 512, "parsed": 510, "unreadable": 1, "malformed": 0, "skipped": 1, "pending": 31},
            "ext.future": {"discovered": 4, "parsed": 4, "unreadable": 0, "malformed": 0, "skipped": 0, "pending": 2},
        },
        "source_clients": sorted([
            {"source_sha256": _sha(b"claude-source"), "client": "claude"},
            {"source_sha256": _sha(b"claudemm-source"), "client": "claudemm"},
            {"source_sha256": _sha(b"codex-source"), "client": "codex"},
            {"source_sha256": _sha(b"future-source"), "client": "ext.future"},
        ], key=lambda row: (row["source_sha256"], row["client"])),
        "mirror": {
            "append_only": True,
            "head": "a" * 40,
            "tree_oid": "b" * 40,
            "events": 1880,
        },
        "scheduler": {
            "name": "crypt-daily",
            "disabled": True,
            "state": "disabled",
        },
        "focused_tests": {
            "passed": True,
            "passed_count": 17,
            "output_sha256": _sha(b"17 passed"),
            "files": [
                {"path": "morph/test_multiwriter_conformance.py", "sha256": _sha(b"t1")},
                {"path": "morph/test_run_incremental_multiwriter.py", "sha256": _sha(b"t2")},
            ],
        },
    }


def test_receipt_is_content_free_canonical_and_fresh():
    conformance = _module()
    evidence = _evidence()

    receipt = conformance.issue_receipt(evidence, now=NOW, ttl_seconds=900)

    assert receipt["schema_version"] == 1
    assert receipt["kind"] == "morph_multiwriter_conformance"
    assert receipt["issued_at"] == "2026-07-20T08:00:00Z"
    assert receipt["expires_at"] == "2026-07-20T08:15:00Z"
    assert receipt["receipt_sha256"] == conformance.receipt_sha256(receipt)
    encoded = json.dumps(receipt, sort_keys=True)
    assert "transcript" not in encoded.lower()
    assert "Always run" not in encoded
    conformance.validate_receipt_payload(
        receipt, current_evidence=evidence, now=NOW + timedelta(minutes=5)
    )


def test_discovery_counts_use_collision_safe_morpher_state_keys(tmp_path: Path):
    conformance = _module()
    first = tmp_path / "one" / "chat_history.jsonl"
    second = tmp_path / "two" / "chat_history.jsonl"
    first.parent.mkdir()
    second.parent.mkdir()
    first.write_text("{}\n", encoding="utf-8")
    second.write_text("{}\n", encoding="utf-8")
    rows = [
        {"client": "grok-build", "tool": "grok-build", "path": first, "outcome": "parsed", "state_key": "one"},
        {"client": "grok-build", "tool": "grok-build", "path": second, "outcome": "parsed", "state_key": "two"},
    ]
    state = {"learned": {"grok-build": {"one": first.stat().st_mtime}}}

    counts = conformance.discovery_counts(rows, state, registered_clients=["grok-build"])

    assert counts["grok-build"]["pending"] == 1


def test_discovery_counts_exclude_active_codex_task_from_pending(
        tmp_path: Path, monkeypatch):
    conformance = _module()
    active = tmp_path / "rollout-active-task.jsonl"
    active.write_text("{}\n", encoding="utf-8")
    monkeypatch.setenv("CODEX_THREAD_ID", "active-task")
    rows = [{
        "client": "codex",
        "tool": "codex",
        "path": active,
        "outcome": "parsed",
        "state_key": active.stem,
    }]

    counts = conformance.discovery_counts(
        rows, {"learned": {}}, registered_clients=["codex"]
    )

    assert counts["codex"]["parsed"] == 1
    assert counts["codex"]["pending"] == 0


def test_defaults_accept_private_candidate_binding(tmp_path: Path, monkeypatch):
    conformance = _module()
    binary = tmp_path / "crypt-service.exe"
    release = tmp_path / "candidate-release.json"
    monkeypatch.setenv("CRYPT_CONFORMANCE_BINARY", str(binary))
    monkeypatch.setenv("CRYPT_CONFORMANCE_RELEASE_MANIFEST", str(release))

    defaults = conformance._defaults(tmp_path)

    assert defaults["binary_path"] == binary
    assert defaults["release_manifest_path"] == release


@pytest.mark.parametrize(
    ("mutation", "match"),
    [
        (lambda value: value["canonical_pool"].update({"sha256": "f" * 64}), "receipt hash"),
        (lambda value: value["implementation"]["files"][0].update({"sha256": "e" * 64}), "receipt hash"),
        (lambda value: value["installed_service"].update({"batch_route_capable": False}), "receipt hash"),
        (lambda value: value["scheduler"].update({"disabled": False}), "receipt hash"),
    ],
)
def test_receipt_tamper_is_rejected(mutation, match):
    conformance = _module()
    receipt = conformance.issue_receipt(_evidence(), now=NOW, ttl_seconds=900)
    mutation(receipt)

    with pytest.raises(conformance.ConformanceError, match=match):
        conformance.validate_receipt_payload(
            receipt, current_evidence=_evidence(), now=NOW + timedelta(minutes=1)
        )


def test_rehashed_tamper_and_current_environment_mismatch_are_rejected():
    conformance = _module()
    receipt = conformance.issue_receipt(_evidence(), now=NOW, ttl_seconds=900)
    receipt["canonical_pool"]["sha256"] = "d" * 64
    receipt["receipt_sha256"] = conformance.receipt_sha256(receipt)

    with pytest.raises(conformance.ConformanceError, match="current conformance evidence"):
        conformance.validate_receipt_payload(
            receipt, current_evidence=_evidence(), now=NOW + timedelta(minutes=1)
        )


def test_pre_apply_validation_allows_only_session_discovery_drift():
    conformance = _module()
    evidence = _evidence()
    receipt = conformance.issue_receipt(evidence, now=NOW, ttl_seconds=900)
    current = copy.deepcopy(evidence)
    current["discovery"]["claude"]["discovered"] += 1
    current["discovery"]["claude"]["parsed"] += 1
    current["discovery"]["claude"]["pending"] += 1
    current["source_clients"].append(
        {"source_sha256": "e" * 64, "client": "claude"}
    )
    current["source_clients"].sort(
        key=lambda row: (row["source_sha256"], row["client"])
    )

    conformance.validate_receipt_payload(
        receipt,
        current_evidence=current,
        now=NOW + timedelta(minutes=1),
        allow_session_discovery_drift=True,
    )

    current["canonical_pool"]["sha256"] = "f" * 64
    with pytest.raises(conformance.ConformanceError, match="current conformance evidence"):
        conformance.validate_receipt_payload(
            receipt,
            current_evidence=current,
            now=NOW + timedelta(minutes=1),
            allow_session_discovery_drift=True,
        )


def test_nested_unknown_field_cannot_put_content_in_receipt():
    conformance = _module()
    evidence = _evidence()
    evidence["canonical_pool"]["transcript"] = "private prompt text"

    with pytest.raises(conformance.ConformanceError, match="canonical Morph pool fields"):
        conformance.issue_receipt(evidence, now=NOW, ttl_seconds=900)


def test_incorrect_implementation_aggregate_is_rejected():
    conformance = _module()
    evidence = _evidence()
    evidence["implementation"]["aggregate_sha256"] = "f" * 64

    with pytest.raises(conformance.ConformanceError, match="aggregate hash mismatch"):
        conformance.issue_receipt(evidence, now=NOW, ttl_seconds=900)


def test_stale_or_overlong_receipt_is_rejected():
    conformance = _module()
    receipt = conformance.issue_receipt(_evidence(), now=NOW, ttl_seconds=900)
    with pytest.raises(conformance.ConformanceError, match="expired"):
        conformance.validate_receipt_payload(
            receipt, current_evidence=_evidence(), now=NOW + timedelta(minutes=16)
        )

    overlong = copy.deepcopy(receipt)
    overlong["expires_at"] = "2026-07-20T10:00:00Z"
    overlong["receipt_sha256"] = conformance.receipt_sha256(overlong)
    with pytest.raises(conformance.ConformanceError, match="freshness window"):
        conformance.validate_receipt_payload(overlong, current_evidence=_evidence(), now=NOW)


@pytest.mark.parametrize(
    ("field", "match"),
    [
        ("scheduler", "crypt-daily"),
        ("mirror", "append-only"),
        ("installed_service", "batch route"),
        ("focused_tests", "focused tests"),
    ],
)
def test_issue_fails_closed_when_required_gate_is_not_green(field, match):
    conformance = _module()
    evidence = _evidence()
    if field == "scheduler":
        evidence[field]["disabled"] = False
    elif field == "mirror":
        evidence[field]["append_only"] = False
    elif field == "installed_service":
        evidence[field]["batch_route_capable"] = False
    else:
        evidence[field]["passed"] = False

    with pytest.raises(conformance.ConformanceError, match=match):
        conformance.issue_receipt(evidence, now=NOW, ttl_seconds=900)


def test_discovery_counts_are_registry_client_split_without_session_identifiers(tmp_path):
    conformance = _module()
    claude_a = tmp_path / "claude-a.jsonl"
    claude_b = tmp_path / "claude-b.jsonl"
    codex = tmp_path / "codex.jsonl"
    for path in (claude_a, claude_b, codex):
        path.write_text("{}\n", encoding="utf-8")
    state = {
        "learned": {
            "claude-code": {claude_a.stem: claude_a.stat().st_mtime},
            "codex": {},
        }
    }

    rows = [
        {"client": "claude", "path": claude_a, "tool": "claude-code", "outcome": "parsed"},
        {"client": "claudemm", "path": claude_b, "tool": "claude-code", "outcome": "malformed"},
        {"client": "codex", "path": codex, "tool": "codex", "outcome": "parsed"},
        {"client": "ext.future", "path": codex, "tool": "future", "outcome": "skipped"},
    ]
    counts = conformance.discovery_counts(rows, state, registered_clients={"claude", "claudemm", "codex", "ext.future"})

    assert counts == {
        "claude": {"discovered": 1, "parsed": 1, "unreadable": 0, "malformed": 0, "skipped": 0, "pending": 0},
        "claudemm": {"discovered": 1, "parsed": 0, "unreadable": 0, "malformed": 1, "skipped": 0, "pending": 0},
        "codex": {"discovered": 1, "parsed": 1, "unreadable": 0, "malformed": 0, "skipped": 0, "pending": 1},
        "ext.future": {"discovered": 1, "parsed": 0, "unreadable": 0, "malformed": 0, "skipped": 1, "pending": 0},
    }
    assert claude_a.stem not in json.dumps(counts)


def test_source_hashes_are_repo_relative_and_deterministic(tmp_path):
    conformance = _module()
    source = tmp_path / "a.py"
    source.write_text("print('a')\n", encoding="utf-8")

    evidence = conformance.hash_sources(tmp_path, [source])

    assert evidence["files"] == [{"path": "a.py", "sha256": _sha(source.read_bytes())}]
    assert evidence["aggregate_sha256"] == conformance.aggregate_file_sha256(evidence["files"])


def test_service_probe_binds_release_asset_hash_and_nonmutating_batch_route(tmp_path):
    conformance = _module()
    binary = tmp_path / "crypt-service.exe"
    binary.write_bytes(b"resident")
    release_path = tmp_path / "release.json"
    release = {
        "release_generation": "sha256:" + _sha(b"source-tree"),
        "assets": [{
            "os": "windows",
            "arch": "x86_64",
            "name": binary.name,
            "role": "service",
            "sha256": _sha(binary.read_bytes()),
        }],
    }
    release_path.write_text(json.dumps(release), encoding="utf-8")

    evidence = conformance.service_evidence(
        binary_path=binary,
        release_manifest_path=release_path,
        os_name="windows",
        arch="x86_64",
        probe=lambda: {
            "release_generation": release["release_generation"],
            "service_generation": "sha256:" + _sha(b"boot"),
            "batch_route_status": 400,
            "batch_route_error": "invalid memory batch",
        },
    )

    assert evidence["binary_sha256"] == _sha(b"resident")
    assert evidence["batch_route_capable"] is True
    bad_release = copy.deepcopy(release)
    bad_release["assets"][0]["sha256"] = "0" * 64
    release_path.write_text(json.dumps(bad_release), encoding="utf-8")
    with pytest.raises(conformance.ConformanceError, match="release asset"):
        conformance.service_evidence(
            binary_path=binary,
            release_manifest_path=release_path,
            os_name="windows",
            arch="x86_64",
            probe=lambda: {},
        )

    release_path.write_text("[]", encoding="utf-8")
    with pytest.raises(conformance.ConformanceError, match="release manifest"):
        conformance.service_evidence(
            binary_path=binary,
            release_manifest_path=release_path,
            os_name="windows",
            arch="x86_64",
            probe=lambda: {},
        )
