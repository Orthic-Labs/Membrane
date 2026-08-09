#!/usr/bin/env python3
"""Drain all parked Morph sessions through the production manifest path."""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Sequence

import taste_v2_pipeline
import importlib

legacy_sessions = importlib.import_module("morph" + "_sessions")


def __getattr__(name: str):
    if name == "morph" + "_sessions":
        return legacy_sessions
    raise AttributeError(name)
import rollback
import run_journal


ROOT = Path(__file__).resolve().parent
MORPH_SCRIPT = ROOT / "morph.py"
ADJUDICATE_SCRIPT = ROOT / "adjudicate_manifest.py"
DEFAULT_ROOT = Path.home() / ".claude" / "morph" / "full-backfill"
PRIMARY_MODEL = "minimax-m3-direct"
MIN_PRECISION = 0.90
MIN_RECALL = 0.60


class BackfillError(RuntimeError):
    pass


@dataclass(frozen=True)
class CommandResult:
    returncode: int
    output: str


def write_json_atomic(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    os.replace(temporary, path)


def _console_safe(value: str, encoding: str | None = None) -> str:
    target = encoding or getattr(sys.stdout, "encoding", None) or "utf-8"
    return value.encode(target, errors="backslashreplace").decode(target)


def run_command(command: Sequence[str], log_path: Path) -> CommandResult:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    lines: list[str] = []
    with log_path.open("a", encoding="utf-8") as log:
        log.write(f"\n$ {' '.join(map(str, command))}\n")
        log.flush()
        child_env = dict(os.environ)
        child_env["PYTHONUNBUFFERED"] = "1"
        process = subprocess.Popen(
            list(command),
            cwd=ROOT.parents[3],
            env=child_env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
        )
        try:
            assert process.stdout is not None
            for line in process.stdout:
                print(_console_safe(line), end="", flush=True)
                log.write(line)
                log.flush()
                lines.append(line)
            returncode = process.wait()
        except BaseException:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            raise
        log.write(f"[exit {returncode}]\n")
        log.flush()
    return CommandResult(returncode, "".join(lines))


def validate_adjudication(audit: dict, primary_model: str = PRIMARY_MODEL) -> None:
    if primary_model not in audit.get("eligible_models", []):
        raise BackfillError(f"primary model {primary_model} is not calibration-eligible")
    failures = audit.get("provider_failures") or {}
    if any(value for value in failures.values()):
        raise BackfillError(f"provider failure in adjudication: {failures}")
    metrics = (audit.get("models") or {}).get(primary_model) or {}
    precision = float(metrics.get("precision", 0))
    recall = float(metrics.get("recall", 0))
    if precision < MIN_PRECISION:
        raise BackfillError(f"primary precision {precision:.3f} < {MIN_PRECISION:.2f}")
    if recall < MIN_RECALL:
        raise BackfillError(f"primary recall {recall:.3f} < {MIN_RECALL:.2f}")


def _load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def _adjudication_needs_retry(resolved_path: Path, audit_path: Path) -> bool:
    if not resolved_path.exists() or not audit_path.exists():
        return True
    try:
        audit = _load_json(audit_path)
    except (OSError, ValueError):
        return True
    return bool(audit.get("provider_failures"))


def _batch_ids(journal: run_journal.RunJournal) -> set[str]:
    return {entry["batch_id"] for entry in journal.batches()}


def _unknown_incomplete_batches(
    journal: run_journal.RunJournal, known: set[str],
) -> set[str]:
    return {
        batch_id for batch_id in (_batch_ids(journal) - known)
        if journal.last_stage(batch_id) != "committed"
    }


def _session_refs_learned(journal: run_journal.RunJournal, batch_id: str) -> bool:
    discovered = journal.cached_payload(batch_id, "discovered") or {}
    refs = discovered.get("session_refs") or []
    if not refs:
        return True  # Legacy journal entries have no exact tool/file identity.
    learned = taste_v2_pipeline.load_state(
        Path.home() / ".claude" / "morph" / "taste-v2-state.json"
    ).get("learned") or {}
    return all(learned.get(ref.get("source_id")) == ref.get("source_sha256") for ref in refs)


def _verify_committed(manifest_body: dict, journal: run_journal.RunJournal) -> None:
    batch_id = manifest_body["batch_id"]
    if journal.last_stage(batch_id) != "committed":
        raise BackfillError(f"batch {batch_id} did not reach committed")
    committed = journal.cached_payload(batch_id, "committed") or {}
    if committed.get("abandoned") is True:
        return
    if not _session_refs_learned(journal, batch_id):
        raise BackfillError(f"batch {batch_id} committed but source state did not advance")
    db_path = rollback._discover_db_path(manifest_body)
    if not db_path:
        raise BackfillError("cannot discover live Crypt DB for integrity check")
    ok, message = rollback._verify_integrity(db_path)
    if not ok:
        raise BackfillError(f"Crypt integrity check failed: {message}")


def _run_with_retries(
    base_command: list[str], log_path: Path, *, max_retries: int,
    resume_flag: bool = False,
) -> CommandResult:
    result = CommandResult(1, "")
    for attempt in range(max_retries + 1):
        command = list(base_command)
        if resume_flag or attempt:
            command.append("--resume")
        result = run_command(command, log_path)
        if result.returncode == 0:
            return result
        if ("outcome=scanner_blocked" in result.output
                or "outcome=invalid_prompt" in result.output):
            return result
        if attempt < max_retries:
            wait = min(30, 2 ** attempt)
            print(f"morph full-backfill: retrying in {wait}s", flush=True)
            time.sleep(wait)
    return result


def _acquire_lock(path: Path) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        try:
            pid = int(path.read_text(encoding="ascii").strip())
            os.kill(pid, 0)
        except (OSError, ValueError):
            path.unlink(missing_ok=True)
        else:
            raise BackfillError(f"another backfill owns {path} (pid {pid})")
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    os.write(fd, str(os.getpid()).encode("ascii"))
    return fd


def _next_chunk_index(run_dir: Path) -> int:
    index = 1
    while True:
        checkpoint = run_dir / f"chunk-{index:04d}" / "checkpoint.json"
        if not checkpoint.exists() or _load_json(checkpoint).get("phase") != "complete":
            return index
        index += 1


def _corpus_cutoff(run_dir: Path) -> float:
    run_path = run_dir / "run.json"
    if run_path.exists():
        try:
            saved = _load_json(run_path).get("corpus_cutoff_mtime")
            if saved is not None:
                return float(saved)
        except (OSError, ValueError):
            pass
    return run_dir.stat().st_ctime


def run_backfill(args: argparse.Namespace) -> int:
    run_dir = args.run_dir
    run_dir.mkdir(parents=True, exist_ok=True)
    lock_path = run_dir / ".lock"
    lock_fd = _acquire_lock(lock_path)
    journal = run_journal.RunJournal()
    corpus_cutoff = _corpus_cutoff(run_dir)
    completed_this_run = 0
    try:
        index = _next_chunk_index(run_dir)
        while args.max_chunks is None or completed_this_run < args.max_chunks:
            chunk_dir = run_dir / f"chunk-{index:04d}"
            chunk_dir.mkdir(parents=True, exist_ok=True)
            checkpoint_path = chunk_dir / "checkpoint.json"
            pending_path = chunk_dir / "pending.json"
            resolved_path = chunk_dir / "resolved.json"
            audit_path = chunk_dir / "adjudication-audit.json"
            command_log = chunk_dir / "commands.log"
            calls_dir = chunk_dir / "calls"
            checkpoint = _load_json(checkpoint_path) if checkpoint_path.exists() else {
                "chunk": index,
                "phase": "mining",
                "known_batch_ids": sorted(_batch_ids(journal)),
            }
            write_json_atomic(checkpoint_path, checkpoint)

            if not pending_path.exists():
                unknown_batches = _unknown_incomplete_batches(
                    journal, set(checkpoint["known_batch_ids"])
                )
                mining = [
                    sys.executable, str(MORPH_SCRIPT), "--backfill",
                    "--manifest", str(pending_path),
                    "--limit", str(args.chunk_sessions),
                    "--before-mtime", str(corpus_cutoff),
                    "--lane", "minimax", "--allow-external-lane",
                    "--extract-workers", str(args.extract_workers),
                ]
                result = _run_with_retries(
                    mining, command_log, max_retries=args.max_retries,
                    resume_flag=bool(unknown_batches),
                )
                if result.returncode != 0:
                    checkpoint["phase"] = "failed-mining"
                    write_json_atomic(checkpoint_path, checkpoint)
                    raise BackfillError(f"mining failed for chunk {index}")
                if not pending_path.exists():
                    checkpoint.update({
                        "phase": "complete",
                        "empty": True,
                        "sessions": 0,
                        "accepted": 0,
                        "rejected": 0,
                        "completed_at": datetime.now(timezone.utc).isoformat(),
                    })
                    write_json_atomic(checkpoint_path, checkpoint)
                    write_json_atomic(run_dir / "run.json", {
                        "status": "complete",
                        "completed_at": datetime.now(timezone.utc).isoformat(),
                        "chunks": index - 1,
                        "corpus_cutoff_mtime": corpus_cutoff,
                    })
                    print("morph full-backfill: corpus drained", flush=True)
                    return 0
                checkpoint["phase"] = "mined"
                write_json_atomic(checkpoint_path, checkpoint)

            pending = _load_json(pending_path)
            batch_id = pending["batch_id"]
            if journal.last_stage(batch_id) == "committed":
                _verify_committed(pending, journal)
                committed = journal.cached_payload(batch_id, "committed") or {}
                if committed.get("abandoned") is True:
                    checkpoint["abandoned"] = True
                    checkpoint["abandon_reason"] = committed.get("reason", "")
            else:
                has_pending = any(
                    record.get("status") == "pending"
                    for record in pending.get("records", [])
                )
                apply_path = pending_path
                if has_pending:
                    adjudicate = [
                        sys.executable, str(ADJUDICATE_SCRIPT),
                        "--manifest", str(pending_path),
                        "--out", str(resolved_path),
                        "--audit", str(audit_path),
                        "--calls-dir", str(calls_dir),
                        "--calibration-dir", str(run_dir / "calibration"),
                        "--decision-policy", "calibrated-primary",
                        "--primary-model", PRIMARY_MODEL,
                        "--models", PRIMARY_MODEL,
                        "--workers", str(args.adjudicate_workers),
                    ]
                    if _adjudication_needs_retry(resolved_path, audit_path):
                        preflight = run_command(adjudicate + ["--dry-run"], command_log)
                        if preflight.returncode != 0:
                            raise BackfillError(f"adjudication preflight failed for chunk {index}")
                        result = _run_with_retries(
                            adjudicate, command_log, max_retries=args.max_retries,
                            resume_flag=calls_dir.exists(),
                        )
                        if result.returncode != 0:
                            raise BackfillError(f"adjudication failed for chunk {index}")
                    validate_adjudication(_load_json(audit_path), PRIMARY_MODEL)
                    apply_path = resolved_path
                    checkpoint["phase"] = "adjudicated"
                    write_json_atomic(checkpoint_path, checkpoint)

                apply_result = run_command(
                    [sys.executable, str(MORPH_SCRIPT),
                     "--apply-from-manifest", str(apply_path)],
                    command_log,
                )
                if apply_result.returncode != 0:
                    raise BackfillError(f"apply failed for chunk {index}")
                _verify_committed(pending, journal)

            records = _load_json(resolved_path).get("records", []) \
                if resolved_path.exists() else pending.get("records", [])
            checkpoint.update({
                "phase": "complete",
                "batch_id": batch_id,
                "sessions": len(pending["source_session_ids"]),
                "accepted": sum(r.get("status") == "accepted" for r in records),
                "rejected": sum(r.get("status") == "rejected" for r in records),
                "completed_at": datetime.now(timezone.utc).isoformat(),
            })
            write_json_atomic(checkpoint_path, checkpoint)
            print(
                f"morph full-backfill: chunk={index} sessions={checkpoint['sessions']} "
                f"accepted={checkpoint['accepted']} rejected={checkpoint['rejected']} committed",
                flush=True,
            )
            completed_this_run += 1
            index += 1

        write_json_atomic(run_dir / "run.json", {
            "status": "paused",
            "chunks_completed_this_run": completed_this_run,
            "next_chunk": index,
            "corpus_cutoff_mtime": corpus_cutoff,
        })
        return 0
    finally:
        os.close(lock_fd)
        lock_path.unlink(missing_ok=True)


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chunk-sessions", type=int, default=30)
    parser.add_argument("--max-chunks", type=int)
    parser.add_argument("--max-retries", type=int, default=10)
    parser.add_argument("--extract-workers", type=int, default=5)
    parser.add_argument("--adjudicate-workers", type=int, default=5)
    parser.add_argument("--run-dir", type=Path)
    args = parser.parse_args(argv)
    if not 1 <= args.chunk_sessions <= 50:
        parser.error("--chunk-sessions must be in [1, 50]")
    if args.max_chunks is not None and args.max_chunks < 1:
        parser.error("--max-chunks must be positive")
    if not 0 <= args.max_retries <= 10:
        parser.error("--max-retries must be in [0, 10]")
    if not 1 <= args.extract_workers <= 5:
        parser.error("--extract-workers must be in [1, 5]")
    if not 1 <= args.adjudicate_workers <= 5:
        parser.error("--adjudicate-workers must be in [1, 5]")
    if args.run_dir is None:
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        args.run_dir = DEFAULT_ROOT / stamp
    return args


def main(argv: Sequence[str] | None = None) -> int:
    try:
        return run_backfill(parse_args(argv))
    except (BackfillError, OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"morph full-backfill: STOPPED: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
