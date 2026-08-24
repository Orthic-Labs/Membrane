#!/usr/bin/env python3
"""Run one receipt-gated, manifest-only incremental Adapt batch.

The runner creates a unique work directory, generates a pending manifest,
adjudicates only when pending candidates exist, validates conformance again,
and applies only through ``--apply-from-manifest``. It never changes the
resident service or ``cortex-daily`` scheduler.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable, Mapping, Sequence


ADAPT_DIR = Path(__file__).resolve().parent
PRODUCT_ROOT = ADAPT_DIR.parents[1]
PACKAGE_ROOT = ADAPT_DIR.parent
if str(PACKAGE_ROOT) not in sys.path:
    sys.path.insert(0, str(PACKAGE_ROOT))
if str(ADAPT_DIR) not in sys.path:
    sys.path.insert(0, str(ADAPT_DIR))

from adapt.workspace_runtime import workspace_root  # noqa: E402
from adapt import multiwriter_conformance  # noqa: E402

REPO_ROOT = workspace_root()


class RunnerError(RuntimeError):
    """Raised when a gated Adapt pipeline phase fails closed."""


def _sha(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _canonical(value: object) -> bytes:
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    ).encode("utf-8")


def _hidden_creation_flags() -> int:
    return int(getattr(subprocess, "CREATE_NO_WINDOW", 0)) if os.name == "nt" else 0


def run_command(argv: Sequence[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(argv),
        shell=False,
        capture_output=True,
        text=True,
        timeout=kwargs.pop("timeout", 1800),
        creationflags=_hidden_creation_flags(),
        **kwargs,
    )


def create_workdir(work_root: Path) -> Path:
    root = Path(work_root)
    root.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return Path(tempfile.mkdtemp(prefix=f"adapt-{stamp}-", dir=root))


def _phase_receipt(phase: str, result: Any) -> dict[str, Any]:
    combined = ((result.stdout or "") + "\n" + (result.stderr or "")).encode("utf-8")
    return {
        "phase": phase,
        "returncode": int(result.returncode),
        "output_sha256": _sha(combined),
    }


def _load_manifest(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise RunnerError("Adapt manifest is missing or invalid") from exc
    if not isinstance(value, dict) or not isinstance(value.get("records"), list):
        raise RunnerError("Adapt manifest is missing or invalid")
    return value


def _candidate_counts(manifest: Mapping[str, Any]) -> dict[str, int]:
    counts = {"accepted": 0, "pending": 0, "rejected": 0, "total": 0}
    for record in manifest.get("records", []):
        status = record.get("status") if isinstance(record, Mapping) else None
        if status not in {"accepted", "pending", "rejected"}:
            raise RunnerError("Adapt manifest contains an invalid candidate status")
        counts[status] += 1
        counts["total"] += 1
    return counts


CLIENT_ACCOUNTING_FIELDS = (
    "discovered", "parsed", "unreadable", "malformed", "skipped", "pending",
    "processed", "failed", "accepted", "rejected", "persisted", "persistence_failed",
)


def _source_client(source_id: object, source_clients: Mapping[str, str]) -> str:
    value = str(source_id or "")
    mapped = source_clients.get(value) or source_clients.get(_sha(value.encode("utf-8")))
    if isinstance(mapped, str) and mapped:
        return mapped
    parts = value.split(":", 4)
    if len(parts) >= 4 and parts[0] == "install":
        if parts[2] == "codex":
            return "codex"
        if parts[2] == "claude-code":
            return "claude"
    return "unknown"


def _client_accounting(
    discovery: Mapping[str, Any],
    manifest: Mapping[str, Any] | None,
    source_clients: Mapping[str, str],
    *,
    persistence_outcome: str | None,
    failed: bool = False,
) -> list[dict[str, Any]]:
    if persistence_outcome not in {None, "success", "failed"}:
        raise RunnerError("Adapt persistence accounting is invalid")
    rows: dict[str, dict[str, Any]] = {}

    def row(client: str) -> dict[str, Any]:
        return rows.setdefault(
            client,
            {"client": client, **{field: 0 for field in CLIENT_ACCOUNTING_FIELDS}},
        )

    for client, values in discovery.items():
        target = row(str(client))
        if isinstance(values, Mapping):
            for field in CLIENT_ACCOUNTING_FIELDS[:6]:
                value = values.get(field, 0)
                if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                    raise RunnerError("Adapt client discovery accounting is invalid")
                target[field] = value
    if manifest is None:
        if failed:
            for target in rows.values():
                target["failed"] = target["pending"]
        return [rows[client] for client in sorted(rows)]

    session_ids = manifest.get("source_session_ids", [])
    if not isinstance(session_ids, list):
        raise RunnerError("Adapt manifest source sessions are invalid")
    for source_id in session_ids:
        target = row(_source_client(source_id, source_clients))
        target["processed"] += 1
        if failed:
            target["failed"] += 1

    for record in manifest.get("records", []):
        if not isinstance(record, Mapping):
            raise RunnerError("Adapt manifest contains an invalid candidate")
        status = record.get("status")
        if status not in {"accepted", "rejected"}:
            continue
        record_sources = record.get("source_ids") or session_ids
        if not isinstance(record_sources, list):
            raise RunnerError("Adapt manifest candidate sources are invalid")
        clients = sorted({_source_client(source_id, source_clients) for source_id in record_sources})
        owner = clients[0] if len(clients) == 1 else ("mixed" if clients else "unknown")
        target = row(owner)
        target[str(status)] += 1
        if status == "accepted" and persistence_outcome is not None:
            target[
                "persisted" if persistence_outcome == "success" else "persistence_failed"
            ] += 1
    return [rows[client] for client in sorted(rows)]


def summary_sha256(summary: Mapping[str, Any]) -> str:
    unsigned = {key: value for key, value in summary.items() if key != "summary_sha256"}
    return _sha(_canonical(unsigned))


def write_summary(workdir: Path, summary: Mapping[str, Any]) -> Path:
    body = dict(summary)
    body["summary_sha256"] = summary_sha256(body)
    path = Path(workdir) / "summary.json"
    path.write_text(json.dumps(body, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path


def _invoke(
    phase: str,
    argv: Sequence[str],
    *,
    command_runner: Callable[..., Any],
    phase_receipts: list[dict[str, Any]],
) -> Any:
    if not argv or not all(isinstance(value, str) and value for value in argv):
        raise RunnerError(f"{phase} command is invalid")
    result = command_runner(list(argv), timeout=1800)
    phase_receipts.append(_phase_receipt(phase, result))
    if result.returncode != 0:
        raise RunnerError(f"{phase} failed (output_sha256={phase_receipts[-1]['output_sha256']})")
    return result


def run_incremental(
    *,
    receipt_path: Path,
    work_root: Path,
    repo_root: Path = REPO_ROOT,
    limit: int | None = None,
    resume: bool = False,
    restart_stale: bool = False,
    lane: str = "local",
    allow_external_lane: bool = False,
    command_runner: Callable[..., Any] = run_command,
    receipt_validator: Callable[[Path], Any] | None = None,
    client_inventory_provider: Callable[[], Mapping[str, Any]] | None = None,
) -> dict[str, Any]:
    if limit is not None and (isinstance(limit, bool) or limit <= 0):
        raise RunnerError("limit must be a positive integer")
    if lane not in {"local", "minimax", "opencode", "pi"}:
        raise RunnerError("unsupported Adapt lane")
    if lane != "local" and not allow_external_lane:
        raise RunnerError("external Adapt lane requires explicit allowance")
    root = Path(repo_root)
    receipt = Path(receipt_path)
    validator = receipt_validator or (
        lambda path: multiwriter_conformance.validate_receipt_file(path, repo_root=root)
    )
    pre_apply_validator = receipt_validator or (
        lambda path: multiwriter_conformance.validate_receipt_file(
            path,
            repo_root=root,
            allow_session_discovery_drift=True,
        )
    )
    validated_receipt = validator(receipt)
    if client_inventory_provider is not None:
        inventory = dict(client_inventory_provider())
    elif isinstance(validated_receipt, Mapping):
        receipt_sources = validated_receipt.get("source_clients", [])
        inventory = {
            "discovery": validated_receipt.get("discovery", {}),
            "source_clients": {
                str(row.get("source_sha256")): str(row.get("client"))
                for row in receipt_sources
                if isinstance(row, Mapping)
            },
        }
    else:
        inventory = {"discovery": {}, "source_clients": {}}
    discovery = inventory.get("discovery")
    source_clients = inventory.get("source_clients")
    if not isinstance(discovery, Mapping) or not isinstance(source_clients, Mapping):
        raise RunnerError("Adapt client inventory is invalid")
    workdir = create_workdir(work_root)
    pending_path = workdir / "pending.json"
    resolved_path = workdir / "resolved.json"
    audit_path = workdir / "adjudication-audit.json"
    calls_dir = workdir / "calls"
    phase_receipts: list[dict[str, Any]] = []
    adapt_path = PRODUCT_ROOT / "adapt.py"
    adjudicate_path = ADAPT_DIR / "adjudicate_manifest.py"

    def record_failure(
        failed_phase: str,
        *,
        manifest: Mapping[str, Any] | None = None,
        counts: Mapping[str, int] | None = None,
        provider_calls: bool = False,
        persistence_outcome: str | None = None,
    ) -> None:
        summary = {
            "schema_version": 1,
            "run_id": workdir.name,
            "outcome": "failed",
            "failed_phase": failed_phase,
            "candidate_counts": dict(counts or {
                "accepted": 0, "pending": 0, "rejected": 0, "total": 0,
            }),
            "adjudication_provider_calls": provider_calls,
            "client_accounting": _client_accounting(
                discovery,
                manifest,
                source_clients,
                persistence_outcome=persistence_outcome,
                failed=True,
            ),
            "phase_receipts": phase_receipts,
        }
        write_summary(workdir, summary)

    manifest_argv = [
        sys.executable,
        str(adapt_path),
        "--incremental",
        "--manifest",
        str(pending_path),
    ]
    if limit is not None:
        manifest_argv.extend(["--limit", str(limit)])
    if resume:
        manifest_argv.append("--resume")
    if restart_stale:
        if not resume:
            raise RunnerError("restart_stale requires resume")
        manifest_argv.append("--restart-stale")
    if lane != "local":
        manifest_argv.extend(["--lane", lane, "--allow-external-lane"])
    try:
        _invoke(
            "manifest generation",
            manifest_argv,
            command_runner=command_runner,
            phase_receipts=phase_receipts,
        )
    except RunnerError:
        record_failure("manifest_generation")
        raise

    if not pending_path.is_file():
        summary = {
            "schema_version": 1,
            "run_id": workdir.name,
            "outcome": "no_new_sessions",
            "candidate_counts": {
                "accepted": 0,
                "pending": 0,
                "rejected": 0,
                "total": 0,
            },
            "adjudication_provider_calls": False,
            "client_accounting": _client_accounting(
                discovery, None, source_clients, persistence_outcome=None
            ),
            "phase_receipts": phase_receipts,
        }
        write_summary(workdir, summary)
        return summary

    pending = _load_manifest(pending_path)
    pending_counts = _candidate_counts(pending)
    if pending_counts["accepted"]:
        raise RunnerError("emit-time manifest contains pre-accepted candidates")
    provider_calls = pending_counts["pending"] > 0
    if provider_calls:
        try:
            _invoke(
                "manifest adjudication",
                [
                    sys.executable,
                    str(adjudicate_path),
                    "--manifest",
                    str(pending_path),
                    "--out",
                    str(resolved_path),
                    "--audit",
                    str(audit_path),
                    "--calls-dir",
                    str(calls_dir),
                ],
                command_runner=command_runner,
                phase_receipts=phase_receipts,
            )
        except RunnerError:
            record_failure(
                "manifest_adjudication",
                manifest=pending,
                counts=pending_counts,
                provider_calls=True,
            )
            raise
    else:
        shutil.copyfile(pending_path, resolved_path)

    resolved = _load_manifest(resolved_path)
    counts = _candidate_counts(resolved)
    if counts["pending"]:
        raise RunnerError("resolved Adapt manifest still contains pending candidates")

    # Re-check every receipt binding after model work and immediately before mutation.
    try:
        pre_apply_validator(receipt)
    except multiwriter_conformance.ConformanceError:
        record_failure(
            "pre_apply_conformance",
            manifest=resolved,
            counts=counts,
            provider_calls=provider_calls,
        )
        raise
    try:
        _invoke(
            "manifest apply",
            [
                sys.executable,
                str(adapt_path),
                "--apply-from-manifest",
                str(resolved_path),
            ],
            command_runner=command_runner,
            phase_receipts=phase_receipts,
        )
    except RunnerError:
        record_failure(
            "manifest_apply",
            manifest=resolved,
            counts=counts,
            provider_calls=provider_calls,
            persistence_outcome="failed",
        )
        raise
    summary = {
        "schema_version": 1,
        "run_id": workdir.name,
        "outcome": "applied",
        "candidate_counts": counts,
        "adjudication_provider_calls": provider_calls,
        "client_accounting": _client_accounting(
            discovery, resolved, source_clients, persistence_outcome="success"
        ),
        "phase_receipts": phase_receipts,
    }
    write_summary(workdir, summary)
    return summary


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument(
        "--work-root",
        type=Path,
        default=REPO_ROOT / "tools/.cache/memory/adapt-multiwriter-runs",
    )
    parser.add_argument("--repo-root", type=Path, default=REPO_ROOT)
    parser.add_argument("--limit", type=int)
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--restart-stale", action="store_true")
    parser.add_argument("--lane", choices=("local", "minimax"), default="local")
    parser.add_argument("--allow-external-lane", action="store_true")
    args = parser.parse_args(argv)
    try:
        summary = run_incremental(
            receipt_path=args.receipt,
            work_root=args.work_root,
            repo_root=args.repo_root,
            limit=args.limit,
            resume=args.resume,
            restart_stale=args.restart_stale,
            lane=args.lane,
            allow_external_lane=args.allow_external_lane,
        )
    except (RunnerError, multiwriter_conformance.ConformanceError) as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, sort_keys=True))
        return 1
    print(json.dumps({
        "ok": True,
        "run_id": summary["run_id"],
        "outcome": summary["outcome"],
        "candidate_counts": summary["candidate_counts"],
        "adjudication_provider_calls": summary["adjudication_provider_calls"],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
