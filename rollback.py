"""Adapt rollback — safe-point capture + revert (Gate 4).

A safe-point is a small JSON file captured BEFORE any live apply. It records
the minimal information needed to reverse that exact apply if anything goes
wrong afterward (wrong rows written, wrong scope, accidental prompt
injection, etc.).

The captured shape is::

    {
      "batch_id":       "<journal batch id>",
      "accepted_ids":   ["D--Claude/adapt-workflow-...", ...],
      "manifest_digest": "<sha256 over the immutable manifest payload>",
      "db_path":        "<absolute path to the live Crypt DB>",
      "db_checksum":    "<sha256 of the live DB at capture time>",
      "db_backup_path": "<consistent SQLite backup>",
      "db_backup_checksum": "<sha256 of backup>",
      "state_snapshot": "<verbatim text of ~/.claude/adapt/state.json>",
      "rules_snapshot": "<verbatim text of ~/.claude/adapt/rules.json>",
      "core_snapshot":  "<verbatim text of ~/.claude/adapt/core.json>",
      "core_digest":    "<sha256 of core.json>",
      "created_at":     "<iso8601>"
    }

Usage::

    python tools/pipelines/memory/adapt/rollback.py create \
        --manifest path/to/manifest.json --db /path/to/crypt-engine.db

    py -3.11 tools/pipelines/memory/adapt/rollback.py revert path/to/safepoint.json
                                      # default: DRY RUN

    py -3.11 tools/pipelines/memory/adapt/rollback.py revert \
        path/to/safepoint.json --apply

The revert phase:
  - reads the safe-point,
  - prints the plan (which IDs would be deleted),
  - on ``--apply``: verifies the bound backup, deletes ONLY the recorded IDs
    via the resident crypt service (never raw SQL on the live DB), restores
    Adapt's state/rules/core snapshots, and verifies ``PRAGMA integrity_check``.

This module is intentionally conservative — it does NOT have a ``--force``
flag. If the integrity check fails, the operator is told and the partial
state stays on disk so it can be investigated.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import shutil
import sqlite3
import subprocess
import sys
from pathlib import Path

import preference_record
from workspace_runtime import workspace_root

WS = workspace_root()
STATE_DIR = Path.home() / ".claude" / "adapt"
STATE_FILE = STATE_DIR / "state.json"
RULES_FILE = STATE_DIR / "rules.json"
CORE_FILE = STATE_DIR / "core.json"
SAFEPOINT_DIR = STATE_DIR / "safepoints"
CRYPT_MUTATION_TIMEOUT_SECONDS = 150


# ----- small helpers -----

def _sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def _sha256_text(s: str) -> str:
    return hashlib.sha256(s.encode("utf-8")).hexdigest()


def _now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def _record_rollback(batch_id: str, deleted_count: int, integrity: str,
                     safe_point_path: Path) -> None:
    """Append a content-free rollback event without weakening rollback success."""
    audit_path = Path(os.environ.get(
        "ADAPT_AUDIT_FILE_OVERRIDE", str(STATE_DIR / "audit.jsonl")
    ))
    event = {
        "ts": _now_iso(),
        "event": "rollback",
        "batch_id": batch_id,
        "deleted_count": deleted_count,
        "integrity": integrity,
        "safe_point_sha256": _sha256_file(safe_point_path),
    }
    try:
        audit_path.parent.mkdir(parents=True, exist_ok=True)
        with open(audit_path, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(event, ensure_ascii=False) + "\n")
    except OSError as exc:
        print(f"  warning: rollback audit write failed: {exc}", file=sys.stderr)


def _resolve_crypt() -> str | None:
    """Locate the `crypt` shim; return None if unavailable."""
    return shutil.which("crypt")


def _snapshot_text(path: Path) -> tuple[bool, str]:
    return (path.exists(), path.read_text(encoding="utf-8") if path.exists() else "")


def _atomic_write_text(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    pending = path.with_suffix(path.suffix + ".tmp")
    pending.write_text(value, encoding="utf-8")
    pending.replace(path)


def _restore_snapshot(path: Path, existed: bool, value: str) -> None:
    if existed:
        _atomic_write_text(path, value)
    elif path.exists():
        path.unlink()


def _backup_sqlite(source: Path, target: Path) -> None:
    """Create a transactionally consistent SQLite backup."""
    if source.resolve() == target.resolve():
        raise ValueError("safe-point DB backup path must differ from source DB")
    target.parent.mkdir(parents=True, exist_ok=True)
    pending = target.with_suffix(target.suffix + ".tmp")
    if pending.exists():
        pending.unlink()
    src = None
    dst = None
    try:
        try:
            src = sqlite3.connect(source)
            dst = sqlite3.connect(pending)
            src.backup(dst)
            dst.commit()
        finally:
            if dst is not None:
                dst.close()
                dst = None
            if src is not None:
                src.close()
                src = None
        pending.replace(target)
    except Exception:
        if pending.exists():
            pending.unlink()
        raise


# ----- create -----

def create_safe_point(manifest: dict, db_path: Path,
                      state_path: Path | None = None,
                      rules_path: Path | None = None,
                      core_path: Path | None = None,
                      out_path: Path | None = None) -> Path:
    """Capture the live pre-apply invariants into a safe-point file.

    ``manifest`` is the parsed manifest dict (must already carry
    ``batch_id`` and ``records``). The safe-point records:

      - ``accepted_ids``: canonical scoped ids of every accepted record
        (computed through the production serializer, not trusted from caller),
      - ``manifest_digest``: SHA-256 over the immutable manifest payload
        (sorted JSON),
      - ``db_checksum``: SHA-256 of the live DB at this instant,
      - ``state_snapshot``: verbatim text of state.json (or "" if absent).

    The safe-point file path defaults to ``~/.claude/adapt/safepoints/<batch_id>.json``.
    """
    state_path = state_path or STATE_FILE
    rules_path = rules_path or RULES_FILE
    core_path = core_path or CORE_FILE
    accepted_ids = [
        f"{stored.scope}/{stored.id}"
        for stored in (
            preference_record.from_manifest_candidate(record)
            for record in manifest.get("records", [])
            if record.get("status") == "accepted"
        )
    ]
    manifest_digest = _sha256_text(json.dumps(manifest, sort_keys=True,
                                              ensure_ascii=False))
    if not db_path.exists():
        raise FileNotFoundError(f"Crypt DB does not exist: {db_path}")
    target = out_path or (SAFEPOINT_DIR / f"{manifest['batch_id']}.json")
    backup_path = target.with_suffix(".db")
    _backup_sqlite(db_path, backup_path)
    backup_ok, backup_message = _verify_integrity(backup_path)
    if not backup_ok:
        backup_path.unlink(missing_ok=True)
        raise RuntimeError(f"safe-point DB backup failed integrity: {backup_message}")
    db_checksum = _sha256_file(db_path)
    state_existed, state_snapshot = _snapshot_text(state_path)
    rules_existed, rules_snapshot = _snapshot_text(rules_path)
    core_existed, core_snapshot = _snapshot_text(core_path)

    body = {
        "schema_version": "1.1.0",
        "batch_id": manifest["batch_id"],
        "created_at": _now_iso(),
        "accepted_ids": accepted_ids,
        "manifest_digest": manifest_digest,
        "db_path": str(db_path),
        "db_checksum": db_checksum,
        "db_backup_path": str(backup_path),
        "db_backup_checksum": _sha256_file(backup_path),
        "state_existed": state_existed,
        "state_snapshot": state_snapshot,
        "rules_existed": rules_existed,
        "rules_snapshot": rules_snapshot,
        "core_existed": core_existed,
        "core_snapshot": core_snapshot,
        "core_digest": _sha256_file(core_path) if core_existed else None,
    }
    target.parent.mkdir(parents=True, exist_ok=True)
    _atomic_write_text(target, json.dumps(body, indent=2, ensure_ascii=False))
    return target


def cmd_create(args: argparse.Namespace) -> int:
    """CLI: ``rollback.py create --manifest PATH [--db PATH] [--out PATH]``."""
    try:
        import manifest  # local module
        m = manifest.load_and_validate(args.manifest)
    except Exception as exc:
        print(f"error: failed to load manifest: {exc}", file=sys.stderr)
        return 2

    db_path = Path(args.db) if args.db else _discover_db_path(m)
    if not db_path or not db_path.exists():
        print(f"error: DB path {db_path!r} does not exist", file=sys.stderr)
        return 2
    try:
        out = create_safe_point(
            m, db_path, out_path=Path(args.out) if args.out else None
        )
    except (OSError, RuntimeError, sqlite3.Error) as exc:
        print(f"error: failed to create safe-point: {exc}", file=sys.stderr)
        return 2
    print(f"safepoint: {out}")
    print(f"  batch_id={m['batch_id']}")
    print(f"  accepted_ids={len([r for r in m['records'] if r.get('status') == 'accepted'])}")
    print(f"  manifest_digest={_sha256_text(json.dumps(m, sort_keys=True, ensure_ascii=False))[:16]}…")
    print(f"  db_path={db_path}")
    print(f"  db_checksum={(_sha256_file(db_path) if db_path.exists() else None)}")
    return 0


def _discover_db_path(manifest: dict) -> Path | None:
    """Best-effort: read the live DB path from crypt runtime config."""
    runtime = WS / "tools" / "lib" / "memory" / "runtime.json"
    if runtime.exists():
        try:
            cfg = json.loads(runtime.read_text(encoding="utf-8"))
            p = cfg.get("db_path") or cfg.get("database_path")
            if p:
                return Path(p)
        except Exception:
            pass
    # The repo-relative cache location is portable to every installation.
    candidate = WS / "tools" / ".cache" / "memory" / "crypt-engine.db"
    if candidate.exists():
        return candidate
    return None


# ----- revert -----

def _print_plan(sp: dict) -> None:
    print(f"safepoint: {sp.get('batch_id', '?')}")
    print(f"  created_at:        {sp.get('created_at', '')}")
    print(f"  manifest_digest:   {sp.get('manifest_digest', '')[:32]}…")
    accepted_ids = sp.get("accepted_ids", [])
    print(f"  accepted_ids ({len(accepted_ids)}):")
    for n in accepted_ids:
        print(f"    - {n}")
    print(f"  db_path:           {sp.get('db_path', '')}")
    print(f"  db_checksum:       {sp.get('db_checksum', '')[:32] if sp.get('db_checksum') else 'null'}…")


def _verify_integrity(db_path: Path) -> tuple[bool, str]:
    """Run ``PRAGMA integrity_check`` with Python's SQLite binding."""
    if not db_path.exists():
        return False, f"db path {db_path} does not exist"
    conn = None
    try:
        conn = sqlite3.connect(db_path)
        row = conn.execute("PRAGMA integrity_check").fetchone()
    except sqlite3.Error as exc:
        return False, f"integrity_check failed: {exc}"
    finally:
        if conn is not None:
            conn.close()
    out = str(row[0]).strip() if row else ""
    return out.lower() == "ok", out or "(empty)"


def _verify_backup(sp: dict) -> tuple[bool, str]:
    backup_value = sp.get("db_backup_path")
    expected = sp.get("db_backup_checksum")
    if not backup_value or not expected:
        return False, "safe-point has no DB backup binding"
    backup = Path(backup_value)
    if not backup.exists():
        return False, f"DB backup missing: {backup}"
    actual = _sha256_file(backup)
    if actual != expected:
        return False, "DB backup checksum mismatch"
    return _verify_integrity(backup)


def _delete_via_crypt(name: str, crypt_bin: str | Path | None = None) -> bool:
    bin_path = str(crypt_bin) if crypt_bin else _resolve_crypt()
    if not bin_path:
        print(f"  error: crypt shim not on PATH; cannot delete {name}",
              file=sys.stderr)
        return False
    try:
        res = subprocess.run(
            [bin_path, "delete", name], capture_output=True, text=True,
            timeout=CRYPT_MUTATION_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired:
        print(f"  error: crypt delete {name} timed out", file=sys.stderr)
        return False
    except OSError as exc:
        print(f"  error: crypt delete {name} failed: {exc}",
              file=sys.stderr)
        return False
    if res.returncode != 0:
        response = f"{res.stdout}\n{res.stderr}".replace("\\", "").lower()
        if "404 not found" in response and '"deleted":false' in response:
            return True
        print(f"  error: crypt delete {name} rc={res.returncode}: "
              f"{res.stderr.strip()}", file=sys.stderr)
        return False
    return True


def revert(safe_point_path: Path, apply: bool = False,
           *, state_path: Path | None = None,
           rules_path: Path | None = None,
           core_path: Path | None = None,
           crypt_bin: str | Path | None = None) -> int:
    """Revert a previously-applied manifest.

    Default: dry-run prints the plan. ``apply=True`` deletes the recorded
    IDs, restores the captured Adapt state/rules/core files, and verifies
    integrity. The DB backup is retained as an emergency artifact; it is not
    copied over a running live database.
    """
    if not safe_point_path.exists():
        print(f"error: safe-point {safe_point_path} not found", file=sys.stderr)
        return 2
    sp = json.loads(safe_point_path.read_text(encoding="utf-8"))
    _print_plan(sp)
    accepted_ids = sp.get("accepted_ids", [])

    if not apply:
        print("\nDRY RUN: pass --apply to execute the rollback.")
        return 0

    backup_ok, backup_message = _verify_backup(sp)
    if not backup_ok:
        print(f"error: refusing rollback: {backup_message}", file=sys.stderr)
        return 2

    # 1. Delete each ID via the resident crypt service.
    failed = []
    for name in accepted_ids:
        if _delete_via_crypt(name, crypt_bin=crypt_bin):
            print(f"  deleted {name}")
        else:
            failed.append(name)
    if failed:
        print(f"error: failed to delete {len(failed)} id(s); "
              f"state.json NOT restored to avoid drift", file=sys.stderr)
        for n in failed:
            print(f"  - {n}", file=sys.stderr)
        return 1

    # 2. Restore Adapt-owned file snapshots atomically.
    state_path = state_path or STATE_FILE
    rules_path = rules_path or RULES_FILE
    core_path = core_path or CORE_FILE
    state_snapshot = sp.get("state_snapshot", "")
    _restore_snapshot(state_path, sp.get("state_existed", True), state_snapshot)
    _restore_snapshot(rules_path, sp.get("rules_existed", False),
                      sp.get("rules_snapshot", ""))
    _restore_snapshot(core_path, sp.get("core_existed", False),
                      sp.get("core_snapshot", ""))
    print(f"  Adapt state/rules/core snapshots restored")

    # 3. Verify integrity.
    db_path = Path(sp.get("db_path", ""))
    ok, msg = _verify_integrity(db_path)
    if ok and msg and "skipped" not in msg:
        print(f"  integrity_check: ok")
    elif ok and msg:
        print(f"  integrity_check: {msg}")
    else:
        print(f"  integrity_check FAILED: {msg}", file=sys.stderr)
        return 2

    _record_rollback(str(sp.get("batch_id", "")), len(accepted_ids), msg,
                     safe_point_path)
    print("rollback complete")
    return 0


def cmd_revert(args: argparse.Namespace) -> int:
    return revert(Path(args.safe_point), apply=bool(args.apply))


# ----- CLI -----

def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p_create = sub.add_parser("create", help="capture a safe-point from a manifest")
    p_create.add_argument("--manifest", type=Path, required=True,
                          help="the reviewed manifest to capture invariants for")
    p_create.add_argument("--db", type=str, default=None,
                          help="explicit live DB path (defaults to runtime.json or canonical cache)")
    p_create.add_argument("--out", type=Path, default=None,
                          help="explicit safe-point output path (defaults to "
                               "~/.claude/adapt/safepoints/<batch_id>.json)")
    p_create.set_defaults(func=cmd_create)

    p_revert = sub.add_parser("revert", help="revert a previously-applied batch")
    p_revert.add_argument("safe_point", type=Path)
    p_revert.add_argument("--apply", action="store_true",
                          help="execute the rollback (default: dry-run)")
    p_revert.set_defaults(func=cmd_revert)

    args = ap.parse_args(argv)
    return int(args.func(args) or 0)


if __name__ == "__main__":
    sys.exit(main())
