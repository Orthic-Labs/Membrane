"""Blueprint provider — spawns `blueprint.mjs graph candidates` and parses output.

Reads a real ContextCandidateSet v1 produced by the existing static
provider (Blueprint graph is the live machine-local generation). Output
candidates carry provider="blueprint" and sourceGeneration = the
manifest.generationId.
"""
from __future__ import annotations

import json
import math
import os
import sqlite3
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from . import workspace_tools_path

CORTEX_CLI_DEFAULT = Path(__file__).resolve().parents[4] / "cortex/scripts/blueprint.mjs"
BLUEPRINT_CLI_DEFAULT = workspace_tools_path(
    "skills", "blueprint", "scripts", "blueprint.mjs"
)
BLUEPRINT_CANDIDATE_CAP_DEFAULT = 64


def candidate_cap(max_tokens: int, raw_override: str | None = None) -> int:
    """Bound repo-code candidate generation independently of the total token budget."""
    raw = raw_override if raw_override is not None else os.environ.get("RIGHTCONTEXT_BLUEPRINT_CAP")
    try:
        configured = int(raw) if raw is not None else BLUEPRINT_CANDIDATE_CAP_DEFAULT
    except ValueError:
        configured = BLUEPRINT_CANDIDATE_CAP_DEFAULT
    return min(max(1, configured), 256, max(1, max_tokens))


def _resolve_blueprint_cli() -> str:
    """Locate the Blueprint CLI on this host.

    Honour an explicit BLUEPRINT_CLI override (CI / dev override) and
    otherwise locate `blueprint.mjs` relative to this file. Return the
    resolved path; raise FileNotFoundError if missing.
    """
    explicit = os.environ.get("BLUEPRINT_CLI")
    if explicit:
        return explicit
    for candidate in (CORTEX_CLI_DEFAULT, BLUEPRINT_CLI_DEFAULT):
        if candidate.exists():
            return str(candidate)
    return str(CORTEX_CLI_DEFAULT)


def _resolve_node() -> str:
    """Locate a Node.js binary that can run `blueprint.mjs`.

    Honour NODE_BIN, then PATH lookup, then a Windows `node.exe` and
    POSIX `node` fallback. Raise FileNotFoundError if missing.
    """
    env_override = os.environ.get("NODE_BIN")
    if env_override and Path(env_override).exists():
        return env_override
    for name in ("node.exe", "node"):
        found = shutil.which(name)
        if found:
            return found
    # Last-resort fallback for hosts where the Node binary is installed
    # but not on PATH (Windows installer often drops it under Program Files).
    for guess in (
        r"C:\Program Files\nodejs\node.exe",
        r"C:\Program Files (x86)\nodejs\node.exe",
        "/usr/bin/node",
        "/usr/local/bin/node",
        "/opt/homebrew/bin/node",
    ):
        if Path(guess).exists():
            return guess
    raise FileNotFoundError(
        "node.js not found on PATH; install node and retry, or set NODE_BIN"
    )


def _connect_graph_db(graph_db: Path) -> sqlite3.Connection:
    """Open the Blueprint store for reading, never writing to it.

    A WAL database opened `mode=ro` needs an existing `-shm`, which a
    read-only connection may not create. `blueprint graph build` checkpoints
    those sidecars away, so a freshly built store is unopenable read-only and
    the lane degraded exactly when the graph was most current. Retry with
    `immutable=1`, which needs no shared-memory file. Read-only is tried first
    so an existing WAL is still honoured.
    """
    uri = graph_db.as_uri()
    try:
        connection = sqlite3.connect(f"{uri}?mode=ro", uri=True, timeout=0.05)
        # sqlite3.connect is lazy: a WAL store missing its -shm only fails on
        # first use, so force the open here or the fallback never fires.
        connection.execute("SELECT 1").fetchone()
        return connection
    except sqlite3.Error:
        return sqlite3.connect(f"{uri}?immutable=1", uri=True, timeout=0.05)


def _read_manifest(repo_root: Path) -> dict | None:
    """Read the sealed Blueprint generation, preferring the graph.db envelope.

    Blueprint's current store is SQLite. JSON manifests remain a compatibility
    fallback for older repositories. The provider uses only generation
    identity; the central Rust ``/freshness`` verdict owns freshness
    classification.
    """
    graph_db = repo_root / ".agent" / "graph" / "graph.db"
    if graph_db.exists():
        try:
            with _connect_graph_db(graph_db) as connection:
                row = connection.execute(
                    "SELECT value FROM generation WHERE key = 'manifest'"
                ).fetchone()
                if row is not None:
                    manifest = json.loads(row[0])
                    if isinstance(manifest, dict) and manifest.get("generationId"):
                        source = connection.execute(
                            "SELECT value FROM generation WHERE key = 'sourceObservation'"
                        ).fetchone()
                        if source is not None:
                            observation = json.loads(source[0])
                            if isinstance(observation, dict):
                                manifest.setdefault("baseCommit", observation.get("head"))
                                manifest.setdefault("sourceState", "dirty" if observation.get("dirty") else "clean")
                        return manifest
        except (OSError, sqlite3.Error, TypeError, ValueError):
            # A present but unreadable DB must not make the provider invent a
            # generation. Legacy JSON is retained for older Blueprint repos.
            pass

    candidates = [
        repo_root / ".agent" / "graph" / "manifest.json",
        repo_root / ".blueprint" / "manifest.json",
    ]
    for path in candidates:
        if path.exists():
            try:
                return json.loads(path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                return None
    return None


def _blueprint_warning(reason_kind: str, message: str) -> dict:
    return {
        "provider": "blueprint",
        "kind": reason_kind,
        "severity": "warning",
        "message": message[:400],
    }


def _produce(
    repo_root: Path,
    task: str,
    max_tokens: int,
    observability: dict[str, Any],
    expected_generation: str | None = None,
) -> tuple[list[dict], str, list[dict]]:
    warnings: list[dict] = []
    cli = _resolve_blueprint_cli()
    if not Path(cli).exists():
        warnings.append(_blueprint_warning("blueprint_cli_missing", f"blueprint CLI missing at {cli}"))
        return [], "blueprint-missing", warnings
    node_bin = _resolve_node()
    manifest = _read_manifest(repo_root)
    cmd = [
        node_bin,
        cli,
        "graph", "candidates",
        "--task", task,
        "--out", ".agent",
        "--limit", str(candidate_cap(max_tokens)),
    ]
    if expected_generation:
        cmd.extend(["--expected-generation", expected_generation])
    subprocess_started = time.monotonic()
    proc = subprocess.run(
        cmd,
        cwd=str(repo_root),
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    subprocess_elapsed_ms = max(0.0, (time.monotonic() - subprocess_started) * 1000.0)
    observability["stageElapsedMs"] = {
        "blueprint_node_spawn": round(subprocess_elapsed_ms, 6),
    }
    if proc.returncode != 0:
        warnings.append(_blueprint_warning("blueprint_nonzero_exit", f"exit={proc.returncode}; stderr={proc.stderr.strip()[:300]}"))
        return [], "blueprint-failed", warnings
    out = proc.stdout.strip()
    if not out:
        warnings.append(_blueprint_warning("blueprint_empty_output", "empty stdout"))
        return [], "blueprint-failed", warnings
    try:
        payload = json.loads(out)
    except json.JSONDecodeError as exc:
        warnings.append(_blueprint_warning("blueprint_json_decode", str(exc)[:300]))
        return [], "blueprint-failed", warnings
    raw_rightcontext = payload.get("_rightcontext") if isinstance(payload, dict) else None
    raw_stages = raw_rightcontext.get("stageElapsedMs") if isinstance(raw_rightcontext, dict) else None
    raw_repo_scan = raw_stages.get("repo_code_scan") if isinstance(raw_stages, dict) else None
    if (
        isinstance(raw_repo_scan, (int, float))
        and not isinstance(raw_repo_scan, bool)
        and math.isfinite(float(raw_repo_scan))
        and float(raw_repo_scan) >= 0
    ):
        repo_scan_ms = min(float(raw_repo_scan), subprocess_elapsed_ms)
        observability["stageElapsedMs"] = {
            "blueprint_node_spawn": round(subprocess_elapsed_ms - repo_scan_ms, 6),
            "repo_code_scan": round(repo_scan_ms, 6),
        }
    # `graph candidates` returns a full ContextCandidateSet v1 object
    # (schemaVersion + candidates[...]). Normalise to the candidate list.
    if isinstance(payload, dict) and isinstance(payload.get("candidates"), list):
        results = payload["candidates"]
    elif isinstance(payload, list):
        results = payload
    else:
        warnings.append(_blueprint_warning("blueprint_unexpected_shape", f"unexpected top-level shape; first 200 bytes: {out[:200]}"))
        return [], "blueprint-failed", warnings

    candidates: list[dict[str, Any]] = []
    for entry in results:
        # Blueprint CLI may emit candidates in two shapes:
        # 1. Top-level (modern): sourceRef/sourceHash at root, no `evidence[]`
        # 2. Evidence-wrapped (legacy): each candidate has `evidence[]` with
        #    path/startLine/endLine/contentHash.
        # We support BOTH. When evidence is present, prefer its path lines;
        # otherwise build sourceRef from top-level sourceRef + layer info.
        evidence_list = entry.get("evidence") or []
        entry_id = entry.get("id") or entry.get("qualifiedName") or ""
        if evidence_list:
            first_ev = evidence_list[0]
            ev_path = first_ev.get("path") or ""
            ev_start = int(first_ev.get("startLine", 1))
            ev_end = int(first_ev.get("endLine", ev_start))
            source_ref = f"{ev_path}:{ev_start}-{ev_end}"
            source_hash = first_ev.get("contentHash") or entry.get("sourceHash") or ("0" * 64)
        else:
            source_ref = entry.get("sourceRef") or entry_id
            source_hash = entry.get("sourceHash") or ("0" * 64)
        if not source_ref:
            # Skip if neither shape carries a usable reference; surface
            # a ProviderWarning upstream instead of an empty candidate.
            continue
        declared_trust = entry.get("trustClass")
        candidates.append({
            "id": f"blueprint:{entry_id}",
            "layer": int(entry.get("layer", 3)),
            "sourceKind": entry.get("sourceKind", "repo_code"),
            "sourceRef": source_ref,
            "sourceHash": source_hash,
            "trustClass": declared_trust or "workspace_tracked",
            "instructionPolicy": entry.get("instructionPolicy", "data_only"),
            "providerScore": float(entry.get("providerScore") or entry.get("confidence") or 0.0),
            "scoreComponents": dict(entry.get("scoreComponents") or {"lexical": 0.0}),
            "estimatedTokens": int(entry.get("estimatedTokens") or 100),
            "protected": bool(entry.get("protected", False)),
            "exact": bool(entry.get("exact", False)),
            "recoverable": bool(entry.get("recoverable", True)),
            "resolver": entry.get("resolver") or f"blueprint resolve {entry_id}",
            "text": entry.get("qualifiedName") or entry.get("name") or entry_id,
        })
    generation_id = (manifest or {}).get("generationId") or "blueprint-federation-stub"
    return candidates, generation_id, warnings


def produce_with_observability(
    repo_root: Path,
    task: str,
    max_tokens: int,
    *,
    expected_generation: str | None = None,
) -> tuple[list[dict], str, list[dict], dict[str, Any]]:
    """Produce candidates plus bounded, content-free provider stage timings."""
    observability: dict[str, Any] = {"stageElapsedMs": {}}
    candidates, generation, warnings = _produce(
        repo_root, task, max_tokens, observability, expected_generation
    )
    return candidates, generation, warnings, observability


def produce(
    repo_root: Path,
    task: str,
    max_tokens: int,
    *,
    expected_generation: str | None = None,
) -> tuple[list[dict], str, list[dict]]:
    """Compatibility wrapper retaining the established three-tuple API."""
    observability: dict[str, Any] = {"stageElapsedMs": {}}
    return _produce(repo_root, task, max_tokens, observability, expected_generation)
