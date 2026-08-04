"""Anchors provider — resolves explicit task anchors to file or symbol candidates."""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

from . import workspace_tools_path


def _repo_anchor_path(repo_root: Path, anchor: str) -> tuple[Path, Path] | None:
    """Return canonical repo root and contained anchor path, if safe."""
    try:
        root = repo_root.resolve()
        requested = Path(anchor)
        path = (requested if requested.is_absolute() else root / requested).resolve()
        path.relative_to(root)
    except (OSError, ValueError):
        return None
    return root, path


def _raw_candidate(anchor: str) -> dict:
    """Preserve a rejected anchor as user intent without resolving it."""
    return {
        "id": f"anchor:raw:{anchor}",
        "layer": 1,
        "sourceKind": "anchor",
        "sourceRef": anchor,
        "sourceHash": "sha256:" + "0" * 64,
        "trustClass": "user_direct",
        "instructionPolicy": "data_only",
        "providerScore": 0.6,
        "scoreComponents": {"anchor_relevance": 0.6},
        "estimatedTokens": 8,
        "protected": True,
        "exact": False,
        "recoverable": True,
        "resolver": f"anchor {anchor}",
        "text": anchor,
    }


def produce(repo_root: Path, anchors: list[str], task: str) -> list[dict]:
    """Each anchor is treated as either a file path or a symbol."""
    if not anchors:
        return []
    candidates: list[dict] = []
    for anchor in anchors:
        if not anchor:
            continue
        # File path resolution
        resolved_path = _repo_anchor_path(repo_root, anchor)
        if resolved_path is None:
            candidates.append(_raw_candidate(anchor))
            continue
        root, path = resolved_path
        if path.exists() and path.is_file():
            try:
                text = path.read_text(encoding="utf-8", errors="replace")[:1500]
            except OSError:
                text = anchor
            rel = path.relative_to(root).as_posix()
            candidates.append({
                "id": f"anchor:file:{rel}",
                "layer": 3,
                "sourceKind": "anchor",
                "sourceRef": rel,
                "sourceHash": "sha256:" + "0" * 64,
                "trustClass": "user_direct",
                "instructionPolicy": "data_only",
                "providerScore": 0.95,
                "scoreComponents": {"anchor_relevance": 1.0},
                "estimatedTokens": max(1, len(text) // 4),
                "protected": True,
                "exact": True,
                "recoverable": True,
                "resolver": f"anchor resolve {anchor}",
                "text": text,
            })
            continue
        # Symbol resolution via blueprint resolve (best effort)
        try:
            bp = os.environ.get(
                "BLUEPRINT_CLI",
                str(workspace_tools_path("skills", "blueprint", "scripts", "blueprint.mjs")),
            )
            proc = subprocess.run(
                [sys.executable.replace("python.exe", "node.exe"), str(bp), "graph", "resolve", anchor, "--out", ".agent"],
                cwd=str(repo_root),
                capture_output=True,
                text=True,
                timeout=5,
                check=False,
            )
            if proc.returncode == 0 and proc.stdout.strip().startswith("{"):
                resolved = json.loads(proc.stdout.strip())
                ev = (resolved.get("evidence") or [{}])[0]
                candidates.append({
                    "id": f"anchor:symbol:{anchor}",
                    "layer": 3,
                    "sourceKind": "anchor",
                    "sourceRef": ev.get("path", anchor),
                    "sourceHash": ev.get("contentHash", "sha256:" + "0" * 64),
                    "trustClass": "user_direct",
                    "instructionPolicy": "data_only",
                    "providerScore": 0.95,
                    "scoreComponents": {"anchor_relevance": 1.0},
                    "estimatedTokens": 100,
                    "protected": True,
                    "exact": True,
                    "recoverable": True,
                    # Plan 3.4 resolver drift: the line above EXECUTES
                    # `blueprint graph resolve <id>` so the published
                    # resolver hint MUST match exactly the command run
                    # (`blueprint graph resolve --node <id>` for re-fetch).
                    "resolver": f"blueprint graph resolve --node {anchor}",
                    "text": anchor,
                })
        except Exception:
            pass
        # Fallback: emit as an anchor-only candidate so the planner sees it.
        candidates.append(_raw_candidate(anchor))
    return candidates
