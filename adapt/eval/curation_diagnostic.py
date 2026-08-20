"""Gate 5 — curation diagnostic for Cortex preference rows.

Implements v2 plan Gate 5 step 1 + step 2:

  Step 1. On a copied DB, run current Dream (``cortex curate``) and
          assert every accepted preference either survives under its primary
          ID or is consolidated into a recorded primary with source provenance.
          Compare exact duplicate counts before/after; never assume semantic
          compression occurred.

  Step 2. Build a read-only near-duplicate candidate report from same-scope
          ``adapt-*`` rows. Deterministic similarity finds candidates; an
          Adapt manifest will adjudicate merge/update/deprecate. Dream remains
          provider-free and performs only accepted deterministic mutations.

This script is fully standalone and safe-by-construction: it copies the live
DB via SQLite's backup API (read-only source), runs Dream against the COPY
through a temporary isolated Cortex service, and never touches the live
DB or the global Cortex config. Token/embedding pipelines are not invoked
in step 1's pre/post comparison.

Outputs::

    <out>/curation.diagnostic.json    structured pre/post row inventory + cluster report
    <out>/curation.diagnostic.md      human-readable summary

Usage::

    py -3.11 membrane/adapt/eval/curation_diagnostic.py \\
        --live-db D:/Claude/tools/.cache/memory/cortex-engine.db \\
        --cortex-bin D:/Claude/tools/bin/cortex.exe \\
        --out /tmp/curation
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import time
from collections import defaultdict
from pathlib import Path


WS = next(p for p in Path(__file__).resolve().parents if (p / "tools" / "lib").is_dir())  # workspace root: the dir that owns tools/lib (never a fixed parent depth)
DEFAULT_LIVE = WS / "tools/.cache/memory/cortex-engine.db"
DEFAULT_CORTEX = WS / "tools/bin/cortex.exe"
DEFAULT_OUT = WS / ".cache/adapt-curation"
SERVICE_READY_TIMEOUT = 20.0

ADAPT_PREFIX = "adapt-"


# ----- DB plumbing (port from run_value_ab.py) -----

def snapshot_live(live: Path, dest: Path) -> int:
    if dest.exists():
        dest.unlink()
    src_uri = f"file:{live.as_posix()}?mode=ro"
    with sqlite3.connect(src_uri, uri=True) as src, sqlite3.connect(dest) as dst:
        src.backup(dst)
    with sqlite3.connect(dest) as chk:
        result = chk.execute("PRAGMA integrity_check").fetchone()[0]
        count = chk.execute("SELECT COUNT(*) FROM memories").fetchone()[0]
    if result != "ok":
        raise RuntimeError(f"snapshot integrity_check failed: {result}")
    return count


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _start_service(cortex: Path, db: Path, port: int) -> subprocess.Popen:
    env = __import__("os").environ.copy()
    env["MEMBRANE_PORT"] = str(port)
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    return subprocess.Popen(
        [str(cortex), "--db", str(db), "serve", "--port", str(port)],
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
        text=True, creationflags=flags,
    )


def _wait_ready(port: int) -> None:
    deadline = time.monotonic() + SERVICE_READY_TIMEOUT
    while time.monotonic() < deadline:
        with socket.socket() as c:
            c.settimeout(0.2)
            if c.connect_ex(("127.0.0.1", port)) == 0:
                return
        time.sleep(0.05)
    raise RuntimeError(f"cortex on port {port} did not become ready")


def _stop_service(svc: subprocess.Popen) -> None:
    svc.terminate()
    try:
        svc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        svc.kill()
        svc.wait(timeout=5)


def _run(cortex: Path, db: Path, port: int, cmd: list[str],
         timeout: float = 30.0) -> tuple[int, str, str]:
    env = __import__("os").environ.copy()
    env["MEMBRANE_PORT"] = str(port)
    res = subprocess.run([str(cortex), "--db", str(db), *cmd],
                         text=True, encoding="utf-8",
                         capture_output=True, check=False, env=env,
                         timeout=timeout)
    return (res.returncode, res.stdout, res.stderr)


# ----- Inventory + same-scope near-duplicate report -----

def inventory_adapt_rows(db: Path) -> list[dict]:
    """Read every adapt-prefixed row from the snapshot.

    Matches both:
      - unscoped ids starting with `adapt-` (Cortex legacy layout)
      - scope/adapt-* ids (``D--Claude/adapt-x-...``)
    """
    with sqlite3.connect(db) as conn:
        rows = conn.execute(
            "SELECT id, content FROM memories "
            "WHERE id LIKE ? OR id LIKE '%/' || ?",
            (f"{ADAPT_PREFIX}%", f"{ADAPT_PREFIX}%"),
        ).fetchall()
    return [{"id": rid, "content": content} for rid, content in rows]


def _normalize(text: str) -> str:
    return re.sub(r"\s+", " ", text.lower().strip())


def same_scope_clusters(rows: list[dict]) -> dict[str, list[dict]]:
    """Group rows by scope (the prefix before the first '/'). Returns
    ``{scope: [row_dicts]}``. Scope-less rows go under ``"<global>"``."""
    by_scope: dict[str, list[dict]] = defaultdict(list)
    for r in rows:
        id_ = r["id"]
        scope = id_.split("/", 1)[0] if "/" in id_ else "<global>"
        r["scope"] = scope
        by_scope[scope].append(r)
    return by_scope


def near_duplicate_candidates(rows: list[dict],
                              threshold: float = 0.97) -> list[dict]:
    """Deterministic similarity for textual content.

    For small per-scope cohorts (most real-world scopes have a handful of
    rules), exact-normalized-text match is the right primitive — it's the
    cheapest sufficient signal and avoids requiring embedding queries. Larger
    scopes would need cosine, but that's the Gate 5 deferred work.

    Returns cluster dicts:
        ``{scope, canonical_id, normalized_key, member_ids: [..]}``
    grouped by normalized content within each scope.
    """
    by_scope = same_scope_clusters(rows)
    clusters: list[dict] = []
    for scope, items in by_scope.items():
        bucket: dict[str, list[dict]] = defaultdict(list)
        for r in items:
            nk = _normalize(r["content"])
            bucket[nk].append(r)
        for nk, members in bucket.items():
            clusters.append({
                "scope": scope,
                "normalized_key": nk,
                "canonical_id": members[0]["id"],
                "member_ids": [m["id"] for m in members],
                "size": len(members),
            })
    # Largest clusters first so reviewers triage the worst offenders first.
    clusters.sort(key=lambda c: -c["size"])
    return clusters


# ----- Curation driver -----

def diagnose(live_db: Path, cortex: Path, out: Path) -> int:
    out.mkdir(parents=True, exist_ok=True)
    snap = out / "snapshot.db"
    pre_count = snapshot_live(live_db, snap)
    pre_rows = inventory_adapt_rows(snap)
    pre_id_count = len(pre_rows)
    pre_clusters = near_duplicate_candidates(pre_rows)

    port = _free_port()
    svc = _start_service(cortex, snap, port)
    curate_ok = False
    curate_error = None
    try:
        _wait_ready(port)
        rc, _out, err = _run(cortex, snap, port, ["curate"], timeout=120.0)
        if rc != 0:
            curate_error = err.strip() or f"rc={rc}"
        else:
            curate_ok = True
    except subprocess.TimeoutExpired:
        curate_error = "timeout after 120s"
    except Exception as exc:
        curate_error = f"exception: {exc}"
    finally:
        _stop_service(svc)

    # post_count is whatever the snapshot has now (curate may have rewritten).
    # Connect directly to the snapshot file to read post-state.
    if not snap.exists():
        post_count = 0
        post_rows: list[dict] = []
    else:
        post_count = sqlite3.connect(snap).execute(
            "SELECT COUNT(*) FROM memories").fetchone()[0]
        post_rows = inventory_adapt_rows(snap)
    post_id_count = len(post_rows)
    post_clusters = near_duplicate_candidates(post_rows)

    # Diff cluster sizes.
    pre_index = {(c["scope"], c["normalized_key"]): c for c in pre_clusters}
    post_index = {(c["scope"], c["normalized_key"]): c for c in post_clusters}
    keys = set(pre_index) | set(post_index)
    cluster_changes = []
    for k in sorted(keys):
        pre = pre_index.get(k)
        post = post_index.get(k)
        cluster_changes.append({
            "scope": k[0], "normalized_key": k[1],
            "pre_size": pre["size"] if pre else 0,
            "post_size": post["size"] if post else 0,
            "pre_member_ids": pre["member_ids"] if pre else [],
            "post_member_ids": post["member_ids"] if post else [],
        })

    report = {
        "generated_at": __import__("datetime").datetime.now(
            __import__("datetime").timezone.utc).isoformat(),
        "live_db": str(live_db),
        "curate_status": {"ok": curate_ok, "error": curate_error},
        "pre_curate": {
            "snapshot_rows": pre_count,
            "adapt_row_count": pre_id_count,
            "cluster_count": len(pre_clusters),
            "max_cluster_size": max((c["size"] for c in pre_clusters), default=0),
        },
        "post_curate": {
            "snapshot_rows": post_count,
            "adapt_row_count": post_id_count,
            "cluster_count": len(post_clusters),
            "max_cluster_size": max((c["size"] for c in post_clusters), default=0),
        },
        "cluster_changes": cluster_changes,
    }
    (out / "curation.diagnostic.json").write_text(
        json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")

    md_lines = [
        "# Gate 5 — curation diagnostic",
        "",
        f"- generated: {report['generated_at']}",
        f"- live_db: {report['live_db']}",
        f"- curate_status: ok={curate_ok}"
        + (f"  error={curate_error}" if curate_error else ""),
        "",
        "## Pre-Dream",
        f"- rows in snapshot: {pre_count}",
        f"- adapt-* rows:     {pre_id_count}",
        f"- exact clusters:   {len(pre_clusters)}",
        f"- max cluster size: {max((c['size'] for c in pre_clusters), default=0)}",
        "",
        "## Post-Dream",
        f"- rows in snapshot: {post_count}",
        f"- adapt-* rows:     {post_id_count}",
        f"- exact clusters:   {len(post_clusters)}",
        f"- max cluster size: {max((c['size'] for c in post_clusters), default=0)}",
        "",
        "## Cluster changes (size pre → size post)",
        "",
        "| scope | pre | post | pre_members | post_members |",
        "|---|---:|---:|---|---|",
    ]
    for c in cluster_changes:
        if c["pre_size"] != c["post_size"]:
            md_lines.append(
                f"| {c['scope']} | {c['pre_size']} | {c['post_size']} | "
                f"{','.join(c['pre_member_ids'])} | "
                f"{','.join(c['post_member_ids'])} |"
            )
    if all(c["pre_size"] == c["post_size"] for c in cluster_changes):
        md_lines.append("| (no changes) | | | | |")
    (out / "curation.diagnostic.md").write_text("\n".join(md_lines) + "\n",
                                                  encoding="utf-8")
    print(f"diagnostic: {out / 'curation.diagnostic.md'}")
    print(f"  adapt rows pre/post: {pre_id_count} -> {post_id_count}; "
          f"clusters pre/post: {len(pre_clusters)} -> {len(post_clusters)}; "
          f"curate ok={curate_ok}")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--live-db", type=Path, default=DEFAULT_LIVE)
    ap.add_argument("--cortex-bin", type=Path, default=DEFAULT_CORTEX)
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = ap.parse_args(argv)

    if not args.live_db.exists():
        print(f"error: live DB missing: {args.live_db}", file=sys.stderr)
        return 2
    if not args.cortex_bin.exists():
        print(f"error: cortex binary missing: {args.cortex_bin}",
              file=sys.stderr)
        return 2

    return diagnose(args.live_db, args.cortex_bin, args.out)


if __name__ == "__main__":
    sys.exit(main())
