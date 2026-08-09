from __future__ import annotations

import json
import sys
import copy
from pathlib import Path

import pytest

import cli
import manifest
import preference_record
import run_journal
import taste_v2
import taste_v2_pipeline
import transcript_sources
from tools.lib.orthic_transcripts import parse_source_events


def _write_correction(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({
        "type": "user", "uuid": "u1", "sessionId": "s1",
        "cwd": "/Volumes/D/claude/morph", "timestamp": "2026-01-01T00:00:00Z",
        "message": {"role": "user", "content":
                    "No, that's wrong. Always run focused tests before reporting completion."},
    }) + "\n", encoding="utf-8")


def test_real_parser_false_flags_admit_and_truthy_unsafe_flags_reject(tmp_path: Path) -> None:
    transcript = tmp_path / "session.jsonl"
    _write_correction(transcript)
    events = parse_source_events(transcript, host="claude_code")
    assert all(value is False for value in events[0]["flags"].values())
    candidate = taste_v2.extract_candidates(events)[0]
    assert taste_v2.admit_candidate(candidate).lifecycleState == "active"
    for unsafe in ("synthetic", "meta", "privateReasoningOmitted", "redacted"):
        flagged = [dict(events[0], flags={**events[0]["flags"], unsafe: True})]
        assert taste_v2.extract_candidates(flagged) == []


def test_cli_manifest_writer_is_v13_with_hashed_evidence_contexts(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    home = tmp_path / "home"
    _write_correction(home / ".claude" / "projects" / "project" / "session.jsonl")
    output = tmp_path / "manifest.json"
    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setattr(run_journal, "JOURNAL_FILE", tmp_path / "journal.jsonl")
    monkeypatch.setattr(sys, "argv", ["morph.py", "--incremental", "--manifest", str(output)])
    assert cli.main() == 0
    body = json.loads(output.read_text(encoding="utf-8"))
    assert body["schema_version"] == "1.3.0"
    assert body["installation_id"]
    assert len(body["canonical_pool_sha256"]) == 64
    assert all(value.startswith("install:") for value in body["source_session_ids"])
    assert body["records"] and body["records"][0]["evidenceContexts"]
    assert body["records"][0]["payload_sha256"] == manifest.payload_sha256(body["records"][0])
    assert manifest.validate_schema(output)["source_session_ids"] == body["source_session_ids"]


def test_pipeline_source_hash_delegates_streaming_helper(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    path = tmp_path / "source.jsonl"
    path.write_bytes(b"transcript")
    calls = []
    monkeypatch.setattr(
        transcript_sources, "source_hash",
        lambda value: calls.append(value) or "sha256:" + "a" * 64,
    )
    assert taste_v2_pipeline.source_hash(path) == "sha256:" + "a" * 64
    assert calls == [path]


def test_cli_manifest_validation_failure_abandons_journal_without_artifact(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    home = tmp_path / "home"
    _write_correction(home / ".claude" / "projects" / "project" / "session.jsonl")
    output = tmp_path / "manifest.json"
    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setattr(run_journal, "JOURNAL_FILE", tmp_path / "journal.jsonl")
    monkeypatch.setattr(
        cli.taste_runtime, "multiwriter_context", lambda **_kwargs: ("08c7ef55-8f6b-4ef1-b234-22232b8ea832", {}),
    )
    monkeypatch.setattr(
        cli.manifest, "validate_schema",
        lambda _path: (_ for _ in ()).throw(manifest.ManifestError("invalid test manifest")),
    )
    monkeypatch.setattr(sys, "argv", ["morph.py", "--incremental", "--manifest", str(output)])
    assert cli.main() == 2
    assert not output.exists()
    entries = [json.loads(line) for line in (tmp_path / "journal.jsonl").read_text().splitlines()]
    assert entries[-1]["stage"] == "abandoned"


def test_real_extracted_candidate_schema_round_trip(tmp_path: Path) -> None:
    transcript = tmp_path / "session.jsonl"
    _write_correction(transcript)
    candidate = taste_v2.extract_candidates(parse_source_events(transcript, host="claude_code"))[0]
    context = __import__("taste_v2_pipeline").evidence_context(candidate)
    source_id = "claude_code:s1:session.jsonl"
    record = {
        "id": preference_record.derive_id("workspace", candidate.category, candidate.rule),
        "rule": candidate.rule, "category": candidate.category, "scope": "workspace",
        "status": "pending", "source_ids": [source_id], "evidence_count": 1,
        "evidence_excerpt": candidate.evidenceText, "evidenceContexts": [context],
        "record_type": candidate.recordType, "authority_effect": candidate.authorityEffect,
    }
    record["payload_sha256"] = manifest.payload_sha256(record)
    body = {
        "schema_version": "1.3.0", "batch_id": "batch-1", "created_at": "2026-01-01T00:00:00Z",
        "source_session_ids": [source_id],
        "source_refs": [{"source_id": source_id, "tool": "claude-code", "host": "claude_code",
                         "path": str(transcript), "mtime_ns": 0,
                         "source_sha256": "sha256:" + "0" * 64}],
        "records": [record],
    }
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(body), encoding="utf-8")
    assert manifest.validate_schema(path)["records"][0]["evidenceContexts"] == [context]
    malformed = copy.deepcopy(body)
    duplicate = copy.deepcopy(malformed["records"][0]["evidenceContexts"][0]["contextEvents"][0])
    malformed["records"][0]["evidenceContexts"][0]["contextEvents"].append(duplicate)
    path.write_text(json.dumps(malformed), encoding="utf-8")
    with pytest.raises(manifest.ManifestError, match="exactly one source event"):
        manifest.validate_schema(path)
    malformed = copy.deepcopy(body)
    source = next(event for event in malformed["records"][0]["evidenceContexts"][0]["contextEvents"] if event["isSource"])
    source["text"] = "forged"
    path.write_text(json.dumps(malformed), encoding="utf-8")
    with pytest.raises(manifest.ManifestError, match="source mismatch: text"):
        manifest.validate_schema(path)

def test_cli_manifest_writer_quarantines_unsupported_via_select_sources(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    """Integration: cli must partition discovered sources through select_sources so unsupported /
    active / metadata-incomplete descriptors never reach extract_source."""
    home = tmp_path / "home"
    _write_correction(home / ".claude" / "projects" / "project" / "session.jsonl")
    unsupported = home / ".commandcode" / "projects" / "other" / "noise.jsonl"
    unsupported.parent.mkdir(parents=True, exist_ok=True)
    unsupported.write_text(json.dumps({"payload": {"session_id": "noise"}}) + "\n", encoding="utf-8")
    output = tmp_path / "manifest.json"
    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setattr(run_journal, "JOURNAL_FILE", tmp_path / "journal.jsonl")
    monkeypatch.setattr(sys, "argv", ["morph.py", "--incremental", "--manifest", str(output)])
    assert cli.main() == 0
    journal_path = tmp_path / "journal.jsonl"
    entries = [json.loads(line) for line in journal_path.read_text(encoding="utf-8").splitlines()]
    discovered = next(row for row in entries if row.get("stage") == "discovered")
    admitted = next(row for row in entries if row.get("stage") == "admitted")
    assert any(row["reason"] == "unsupported-host" for row in discovered.get("quarantined_sources", [])), discovered
    assert admitted.get("candidates") == 1
