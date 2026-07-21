from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path

import pytest


MODULE_PATH = Path(__file__).parent / "run_incremental_multiwriter.py"


def _module():
    spec = importlib.util.spec_from_file_location("run_incremental_multiwriter", MODULE_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _record(status: str) -> dict:
    return {
        "id": "adapt-verification-focused-tests-0123456789",
        "rule": "Run focused tests before completion.",
        "category": "verification",
        "scope": "global",
        "status": status,
        "payload_sha256": "0" * 64,
    }


def _manifest(records: list[dict], source_session_ids: list[str] | None = None) -> dict:
    return {
        "schema_version": "1.2.0",
        "batch_id": "batch-1",
        "source_session_ids": source_session_ids or ["install:12345678-1234-4234-9234-123456789abc:codex:" + "a" * 32],
        "created_at": "2026-07-20T08:00:00Z",
        "records": records,
    }


class Result:
    def __init__(self, returncode: int = 0, stdout: str = "ok", stderr: str = ""):
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr


def _client_inventory() -> dict:
    codex = "install:12345678-1234-4234-9234-123456789abc:codex:" + "a" * 32
    claudemm = "install:12345678-1234-4234-9234-123456789abc:claude-code:" + "b" * 32
    return {
        "discovery": {
            "claude": {"discovered": 2, "parsed": 2, "unreadable": 0, "malformed": 0, "skipped": 0, "pending": 0},
            "claudemm": {"discovered": 2, "parsed": 2, "unreadable": 0, "malformed": 0, "skipped": 0, "pending": 1},
            "codex": {"discovered": 3, "parsed": 3, "unreadable": 0, "malformed": 0, "skipped": 0, "pending": 1},
            "ext.future": {"discovered": 1, "parsed": 0, "unreadable": 0, "malformed": 0, "skipped": 1, "pending": 1},
        },
        "source_clients": {codex: "codex", claudemm: "claudemm"},
    }


def test_runner_sequences_manifest_adjudication_and_manifest_apply(tmp_path):
    runner = _module()
    calls: list[list[str]] = []
    validations: list[Path] = []

    def command(argv, **_kwargs):
        calls.append(list(argv))
        if "--manifest" in argv and Path(argv[1]).name == "adapt.py":
            path = Path(argv[argv.index("--manifest") + 1])
            path.write_text(json.dumps(_manifest([_record("pending")])), encoding="utf-8")
        elif Path(argv[1]).name == "adjudicate_manifest.py":
            path = Path(argv[argv.index("--out") + 1])
            path.write_text(json.dumps(_manifest([_record("accepted")])), encoding="utf-8")
        return Result()

    summary = runner.run_incremental(
        receipt_path=tmp_path / "receipt.json",
        work_root=tmp_path / "runs",
        repo_root=Path("D:/Claude"),
        limit=40,
        resume=True,
        restart_stale=True,
        lane="minimax",
        allow_external_lane=True,
        command_runner=command,
        receipt_validator=lambda path: validations.append(path),
        client_inventory_provider=_client_inventory,
    )

    assert len(calls) == 3
    assert calls[0][1].endswith("adapt.py")
    assert calls[0][2:4] == ["--incremental", "--manifest"]
    assert "--limit" in calls[0]
    assert "--resume" in calls[0]
    assert "--restart-stale" in calls[0]
    assert calls[0][calls[0].index("--extract-workers") + 1] == "5"
    assert calls[0][calls[0].index("--lane") + 1] == "minimax"
    assert "--allow-external-lane" in calls[0]
    assert calls[1][1].endswith("adjudicate_manifest.py")
    assert calls[2][1].endswith("adapt.py")
    assert calls[2][2] == "--apply-from-manifest"
    assert "--apply" not in calls[2]
    assert validations == [tmp_path / "receipt.json", tmp_path / "receipt.json"]
    assert summary["outcome"] == "applied"
    assert summary["candidate_counts"] == {"accepted": 1, "pending": 0, "rejected": 0, "total": 1}
    assert summary["adjudication_provider_calls"] is True
    by_client = {row["client"]: row for row in summary["client_accounting"]}
    assert by_client["codex"]["processed"] == 1
    assert by_client["codex"]["accepted"] == 1
    assert by_client["codex"]["persisted"] == 1
    assert by_client["codex"]["failed"] == 0
    assert by_client["claudemm"]["processed"] == 0
    assert by_client["ext.future"]["skipped"] == 1


@pytest.mark.parametrize("records", [[], [_record("rejected")]])
def test_zero_or_all_rejected_skips_adjudication_provider_and_still_applies(tmp_path, records):
    runner = _module()
    calls: list[list[str]] = []

    def command(argv, **_kwargs):
        calls.append(list(argv))
        if "--manifest" in argv:
            Path(argv[argv.index("--manifest") + 1]).write_text(
                json.dumps(_manifest(records)), encoding="utf-8"
            )
        return Result()

    summary = runner.run_incremental(
        receipt_path=tmp_path / "receipt.json",
        work_root=tmp_path / "runs",
        repo_root=Path("D:/Claude"),
        command_runner=command,
        receipt_validator=lambda _path: None,
    )

    assert len(calls) == 2
    assert not any(Path(call[1]).name == "adjudicate_manifest.py" for call in calls)
    assert calls[-1][2] == "--apply-from-manifest"
    assert summary["adjudication_provider_calls"] is False
    assert summary["outcome"] == "applied"


def test_emit_time_preaccepted_record_is_rejected_before_apply(tmp_path):
    runner = _module()
    calls: list[list[str]] = []

    def command(argv, **_kwargs):
        calls.append(list(argv))
        if "--manifest" in argv:
            Path(argv[argv.index("--manifest") + 1]).write_text(
                json.dumps(_manifest([_record("accepted")])), encoding="utf-8"
            )
        return Result()

    with pytest.raises(runner.RunnerError, match="pre-accepted"):
        runner.run_incremental(
            receipt_path=tmp_path / "receipt.json",
            work_root=tmp_path / "runs",
            repo_root=Path("D:/Claude"),
            command_runner=command,
            receipt_validator=lambda _path: None,
        )

    assert len(calls) == 1


def test_no_new_sessions_is_a_content_free_noop(tmp_path):
    runner = _module()
    calls: list[list[str]] = []

    def command(argv, **_kwargs):
        calls.append(list(argv))
        return Result(stdout="adapt: no new sessions")

    summary = runner.run_incremental(
        receipt_path=tmp_path / "receipt.json",
        work_root=tmp_path / "runs",
        repo_root=Path("D:/Claude"),
        command_runner=command,
        receipt_validator=lambda _path: None,
    )

    assert len(calls) == 1
    assert summary["outcome"] == "no_new_sessions"
    assert summary["candidate_counts"] == {"accepted": 0, "pending": 0, "rejected": 0, "total": 0}
    encoded = json.dumps(summary)
    assert "adapt: no new sessions" not in encoded
    assert "D:/Claude" not in encoded


def test_failed_stage_stops_before_apply_and_records_only_output_hashes(tmp_path):
    runner = _module()

    def command(_argv, **_kwargs):
        return Result(returncode=2, stdout="sensitive stdout", stderr="sensitive stderr")

    with pytest.raises(runner.RunnerError, match="manifest generation failed") as exc:
        runner.run_incremental(
            receipt_path=tmp_path / "receipt.json",
            work_root=tmp_path / "runs",
            repo_root=Path("D:/Claude"),
            command_runner=command,
            receipt_validator=lambda _path: None,
            client_inventory_provider=_client_inventory,
        )
    assert "sensitive" not in str(exc.value)
    summary_path = next((tmp_path / "runs").glob("adapt-*/summary.json"))
    summary = json.loads(summary_path.read_text(encoding="utf-8"))
    assert summary["outcome"] == "failed"
    assert summary["failed_phase"] == "manifest_generation"
    assert summary["summary_sha256"] == runner.summary_sha256(summary)
    by_client = {row["client"]: row for row in summary["client_accounting"]}
    assert by_client["claudemm"]["failed"] == 1
    assert by_client["codex"]["failed"] == 1
    assert by_client["ext.future"]["failed"] == 1


def test_each_run_gets_a_unique_exclusive_workdir(tmp_path):
    runner = _module()
    first = runner.create_workdir(tmp_path)
    second = runner.create_workdir(tmp_path)

    assert first != second
    assert first.is_dir() and second.is_dir()
    assert first.parent == tmp_path and second.parent == tmp_path


def test_default_command_runner_uses_argv_without_shell(monkeypatch):
    runner = _module()
    captured = {}

    def fake_run(argv, **kwargs):
        captured["argv"] = argv
        captured["kwargs"] = kwargs
        return Result()

    monkeypatch.setattr(runner.subprocess, "run", fake_run)
    runner.run_command(["python", "adapt.py", "--incremental"])

    assert captured["argv"] == ["python", "adapt.py", "--incremental"]
    assert captured["kwargs"]["shell"] is False
    assert captured["kwargs"]["capture_output"] is True
    assert captured["kwargs"]["text"] is True


def test_summary_file_is_canonical_and_content_free(tmp_path):
    runner = _module()
    workdir = runner.create_workdir(tmp_path)
    summary = {
        "schema_version": 1,
        "run_id": workdir.name,
        "outcome": "applied",
        "candidate_counts": {"accepted": 1, "pending": 0, "rejected": 2, "total": 3},
        "adjudication_provider_calls": True,
        "phase_receipts": [{"phase": "apply", "returncode": 0, "output_sha256": hashlib.sha256(b"ok").hexdigest()}],
    }

    path = runner.write_summary(workdir, summary)

    loaded = json.loads(path.read_text(encoding="utf-8"))
    assert loaded["summary_sha256"] == runner.summary_sha256(loaded)
    assert str(tmp_path) not in path.read_text(encoding="utf-8")
