import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import full_backfill as fb


def test_adjudication_gate_accepts_calibrated_clean_primary():
    audit = {
        "eligible_models": ["minimax-m3-direct"],
        "provider_failures": {},
        "models": {
            "minimax-m3-direct": {"precision": 0.91, "recall": 0.71}
        },
    }
    fb.validate_adjudication(audit, "minimax-m3-direct")


@pytest.mark.parametrize(
    "patch,match",
    [
        ({"eligible_models": []}, "not calibration-eligible"),
        ({"provider_failures": {"minimax-m3-direct": ["timeout"]}}, "provider failure"),
        ({"models": {"minimax-m3-direct": {"precision": 0.89, "recall": 0.8}}}, "precision"),
        ({"models": {"minimax-m3-direct": {"precision": 1.0, "recall": 0.59}}}, "recall"),
    ],
)
def test_adjudication_gate_fails_closed(patch, match):
    audit = {
        "eligible_models": ["minimax-m3-direct"],
        "provider_failures": {},
        "models": {
            "minimax-m3-direct": {"precision": 1.0, "recall": 0.8}
        },
    }
    audit.update(patch)
    with pytest.raises(fb.BackfillError, match=match):
        fb.validate_adjudication(audit, "minimax-m3-direct")


def test_failed_adjudication_artifacts_are_retried(tmp_path):
    resolved = tmp_path / "resolved.json"
    audit = tmp_path / "audit.json"
    resolved.write_text('{"records": []}', encoding="utf-8")
    audit.write_text(
        json.dumps({"provider_failures": {"minimax-m3-direct": "timeout"}}),
        encoding="utf-8",
    )
    assert fb._adjudication_needs_retry(resolved, audit)

    audit.write_text('{"provider_failures": {}}', encoding="utf-8")
    assert not fb._adjudication_needs_retry(resolved, audit)


def test_existing_run_uses_frozen_corpus_cutoff(tmp_path):
    run_dir = tmp_path / "run"
    run_dir.mkdir()
    (run_dir / "run.json").write_text(
        '{"corpus_cutoff_mtime": 123.5}', encoding="utf-8"
    )
    assert fb._corpus_cutoff(run_dir) == 123.5


def test_new_sessions_excludes_files_after_corpus_cutoff(tmp_path, monkeypatch):
    session = tmp_path / "live.jsonl"
    session.write_text("{}\n", encoding="utf-8")
    monkeypatch.setattr(
        fb.morph_sessions, "discover", lambda: [("codex", session)]
    )
    monkeypatch.setattr(
        fb.morph_sessions,
        "parse_codex_session",
        lambda _path: pytest.fail("live session should not be parsed"),
    )
    assert fb.morph_sessions.new_sessions(
        {"learned": {}}, before_mtime=session.stat().st_mtime - 1
    ) == []


def test_atomic_checkpoint_round_trip(tmp_path):
    path = tmp_path / "checkpoint.json"
    fb.write_json_atomic(path, {"phase": "mined", "chunk": 3})
    assert json.loads(path.read_text(encoding="utf-8")) == {
        "phase": "mined", "chunk": 3
    }
    assert not path.with_suffix(".json.tmp").exists()


def test_command_runner_logs_and_returns_output(tmp_path):
    log = tmp_path / "command.log"
    result = fb.run_command(
        [sys.executable, "-c", "print('real-result-line')"], log
    )
    assert result.returncode == 0
    assert "real-result-line" in result.output
    assert "real-result-line" in log.read_text(encoding="utf-8")


def test_console_safe_escapes_unencodable_text():
    assert fb._console_safe("bad \ufffd", "cp1252") == "bad \\ufffd"


def test_lock_recovers_after_dead_process(tmp_path):
    path = tmp_path / ".lock"
    path.write_text("999999999", encoding="ascii")
    fd = fb._acquire_lock(path)
    try:
        assert path.read_text(encoding="ascii") == str(fb.os.getpid())
    finally:
        fb.os.close(fd)
        path.unlink()


def test_unknown_batches_only_returns_incomplete(tmp_path):
    journal = fb.run_journal.RunJournal(tmp_path / "journal.jsonl")
    journal.record("old", "discovered", sessions=["s0"])
    journal.record("old", "committed", sessions=["s0"])
    journal.record("current", "discovered", sessions=["s1"])
    assert fb._unknown_incomplete_batches(journal, set()) == {"current"}


def test_abandoned_committed_batch_needs_no_state_advance(tmp_path, monkeypatch):
    journal = fb.run_journal.RunJournal(tmp_path / "journal.jsonl")
    journal.record("live-tail", "discovered", sessions=["s1"])
    journal.record("live-tail", "committed", applied=0, abandoned=True)
    monkeypatch.setattr(
        fb, "_session_refs_learned", lambda *_args: pytest.fail("not required")
    )
    fb._verify_committed({"batch_id": "live-tail"}, journal)


def test_deterministic_scanner_block_is_not_retried(tmp_path, monkeypatch):
    calls = []

    def blocked(command, log_path):
        calls.append(command)
        return fb.CommandResult(2, "outcome=scanner_blocked reason=scanner-positive")

    monkeypatch.setattr(fb, "run_command", blocked)
    result = fb._run_with_retries(
        ["morph"], tmp_path / "commands.log", max_retries=3
    )
    assert result.returncode == 2
    assert len(calls) == 1


def test_unattended_default_retry_budget_handles_large_chunks():
    args = fb.parse_args(["--run-dir", "run"])
    assert args.max_retries == 10
    assert args.extract_workers == 5
    assert args.adjudicate_workers == 5


def test_unattended_worker_cap_is_five():
    assert fb.parse_args([
        "--run-dir", "run", "--extract-workers", "5"
    ]).extract_workers == 5
    with pytest.raises(SystemExit):
        fb.parse_args(["--run-dir", "run", "--extract-workers", "6"])
    with pytest.raises(SystemExit):
        fb.parse_args(["--run-dir", "run", "--adjudicate-workers", "6"])
