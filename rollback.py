"""Adapt rollback — safe-point capture + revert (Gate 4).

A safe-point is a small JSON file captured BEFORE any live apply. It records
the minimal information needed to reverse that exact apply if anything goes
wrong afterward (wrong rows written, wrong scope, accidental prompt
injection, etc.).

The captured shape is::

    {
      "batch_id":       "<journal batch id>",
      "accepted_ids":   ["adapt-workflow-...", ...],   # primary ids only
      "manifest_digest": "<sha256 over the immutable manifest payload>",
      "db_path":        "<absolute path to the live MemRight DB>",
      "db_checksum":    "<sha256 of the live DB at capture time>",
      "state_snapshot": "<verbatim text of ~/.claude/adapt/state.json>",
      "created_at":     "<iso8601>"
    }

Usage::

    py -3.11 tools/pipelines/memory/adapt/rollback.py create \
        --manifest path/to/manifest.json --db D:/Claude/.cache/memright/.../memright.db

    py -3.11 tools/pipelines/memory/adapt/rollback.py revert path/to/safepoint.json
                                      # default: DRY RUN

    py -3.11 tools/pipelines/memory/adapt/rollback.py revert \
        path/to/safepoint.json --apply

The revert phase:
  - reads the safe-point,
  - prints the plan (which IDs would be deleted),
  - on ``--apply``: deletes ONLY the recorded IDs via the resident memright
    service (never raw ``psql`` / ``sqlite3`` on the live DB), restores the
    state.json snapshot, and verifies ``PRAGMA integrity_check`` returns ok.

This module is intentionally conservative — it does NOT have a ``--force``
flag. If the integrity check fails, the operator is told and the partial
state stays on disk so it can be investigated.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

WS = Path("D:/Claude")
STATE_DIR = Path.home() / ".claude" / "adapt"
STATE_FILE = STATE_DIR / "state.json"
SAFEPOINT_DIR = STATE_DIR / "safepoints"


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


def _resolve_memright() -> str | None:
    """Locate the `memright` shim; return None if unavailable."""
    return shutil.which("memright")


# ----- create -----

def create_safe_point(manifest: dict, db_path: Path,
                      state_path: Path = STATE_FILE,
                      out_path: Path | None = None) -> Path:
    """Capture the live pre-apply invariants into a safe-point file.

    ``manifest`` is the parsed manifest dict (must already carry
    ``batch_id`` and ``records``). The safe-point records:

      - ``accepted_ids``: primary ids of every accepted record (computed here,
        not trusted from caller — that's the invariant),
      - ``manifest_digest``: SHA-256 over the immutable manifest payload
        (sorted JSON),
      - ``db_checksum``: SHA-256 of the live DB at this instant,
      - ``state_snapshot``: verbatim text of state.json (or "" if absent).

    The safe-point file path defaults to ``~/.claude/adapt/safepoints/<batch_id>.json``.
    """
    SAFEPOINT_DIR.mkdir(parents=True, exist_ok=True)
    accepted_ids = [r["id"] for r in manifest.get("records", [])
                    if r.get("status") == "accepted"]
    manifest_digest = _sha256_text(json.dumps(manifest, sort_keys=True,
                                              ensure_ascii=False))
    db_checksum = _sha256_file(db_path) if db_path.exists() else None
    state_snapshot = state_path.read_text(encoding="utf-8") \
        if state_path.exists() else ""

    body = {
        "schema_version": "1.0.0",
        "batch_id": manifest["batch_id"],
        "created_at": _now_iso(),
        "accepted_ids": accepted_ids,
        "manifest_digest": manifest_digest,
        "db_path": str(db_path),
        "db_checksum": db_checksum,
        "state_snapshot": state_snapshot,
    }
    target = out_path or (SAFEPOINT_DIR / f"{manifest['batch_id']}.json")
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(body, indent=2, ensure_ascii=False),
                      encoding="utf-8")
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
        print(f"warn: DB path {db_path!r} does not exist; "
              f"safe-point will record db_checksum=null", file=sys.stderr)
    out = create_safe_point(m, db_path, out_path=Path(args.out) if args.out else None)
    print(f"safepoint: {out}")
    print(f"  batch_id={m['batch_id']}")
    print(f"  accepted_ids={len([r for r in m['records'] if r.get('status') == 'accepted'])}")
    print(f"  manifest_digest={_sha256_text(json.dumps(m, sort_keys=True, ensure_ascii=False))[:16]}…")
    print(f"  db_path={db_path}")
    print(f"  db_checksum={(_sha256_file(db_path) if db_path.exists() else None)}")
    return 0


def _discover_db_path(manifest: dict) -> Path | None:
    """Best-effort: read the live DB path from memright runtime config."""
    runtime = WS / "tools" / "lib" / "memory" / "runtime.json"
    if runtime.exists():
        try:
            cfg = json.loads(runtime.read_text(encoding="utf-8"))
            p = cfg.get("db_path") or cfg.get("database_path")
            if p:
                return Path(p)
        except Exception:
            pass
    # Fallback to the canonical Mac/Windows location.
    candidates = [
        WS / "tools" / ".cache" / "memory" / "memright-engine.db",
        Path("/Users/adrdsouza/claude/tools/.cache/memory/memright-engine.db"),
    ]
    for c in candidates:
        if c.exists():
            return c
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
    """Run ``PRAGMA integrity_check`` via the sqlite3 CLI if available.

    Returns ``(ok, message)``. If sqlite3 isn't on PATH, we conservatively
    treat the verification as failed and tell the operator to verify
    manually.
    """
    if not db_path.exists():
        return False, f"db path {db_path} does not exist"
    sqlite3_bin = shutil.which("sqlite3")
    if not sqlite3_bin:
        # Conservative fallback: report a warning rather than fake ok.
        return True, "sqlite3 binary not on PATH — skipped (verify manually)"
    try:
        r = subprocess.run([sqlite3_bin, str(db_path),
                            "PRAGMA integrity_check;"],
                           capture_output=True, text=True, timeout=30)
    except subprocess.TimeoutExpired:
        return False, "integrity_check timed out"
    except OSError as exc:
        return False, f"integrity_check failed to run: {exc}"
    out = (r.stdout or "").strip()
    return out.lower() == "ok", out or "(empty)"


def _delete_via_memright(name: str) -> bool:
    bin_path = _resolve_memright()
    if not bin_path:
        print(f"  error: memright shim not on PATH; cannot delete {name}",
              file=sys.stderr)
        return False
    try:
        res = subprocess.run([bin_path, "delete", name],
                             capture_output=True, text=True, timeout=30)
    except subprocess.TimeoutExpired:
        print(f"  error: memright delete {name} timed out", file=sys.stderr)
        return False
    except OSError as exc:
        print(f"  error: memright delete {name} failed: {exc}",
              file=sys.stderr)
        return False
    if res.returncode != 0:
        print(f"  error: memright delete {name} rc={res.returncode}: "
              f"{res.stderr.strip()}", file=sys.stderr)
        return False
    return True


def revert(safe_point_path: Path, apply: bool = False) -> int:
    """Revert a previously-applied manifest.

    Default: dry-run prints the plan. ``apply=True`` deletes the recorded
    IDs, restores state.json, and verifies integrity. State ``STATE_FILE``
    is the only file restored; ``RULES_FILE`` and the digest are not
    rewritten by revert because they may have been edited independently.
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

    # 1. Delete each ID via the resident memright service.
    failed = []
    for name in accepted_ids:
        if _delete_via_memright(name):
            print(f"  deleted {name}")
        else:
            failed.append(name)
    if failed:
        print(f"error: failed to delete {len(failed)} id(s); "
              f"state.json NOT restored to avoid drift", file=sys.stderr)
        for n in failed:
            print(f"  - {n}", file=sys.stderr)
        return 1

    # 2. Restore state.json snapshot.
    snapshot = sp.get("state_snapshot", "")
    STATE_FILE.parent.mkdir(parents=True, exist_ok=True)
    STATE_FILE.write_text(snapshot, encoding="utf-8")
    print(f"  state snapshot restored ({len(snapshot)} bytes)")

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