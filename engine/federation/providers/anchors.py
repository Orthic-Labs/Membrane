"""Anchors provider — resolves explicit task anchors to file or symbol candidates."""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

from . import workspace_tools_path


def produce(repo_root: Path, anchors: list[str], task: str) -> list[dict]:
    """Each anchor is treated as either a file path or a symbol."""
    if not anchors:
        return []
    candidates: list[dict] = []
    for anchor in anchors:
        if not anchor:
            continue
        # File path resolution
        path = (repo_root / anchor).resolve() if not Path(anchor).is_absolute() else Path(anchor)
        if path.exists() and path.is_file():
            try:
                text = path.read_text(encoding="utf-8", errors="replace")[:1500]
            except OSError:
                text = anchor
            rel = str(path.relative_to(repo_root)) if str(path).startswith(str(repo_root)) else anchor
            candidates.append({
                "id": f"anchor:file:{rel}",
                "layer": 3,
                "sourceKind": "anchor",
                "sourceRef": rel,
                "sourceHash": "0" * 64,
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
                    "sourceHash": ev.get("contentHash", "0" * 64),
                    "trustClass": "user_direct",
                    "instructionPolicy": "data_only",
                    "providerScore": 0.95,
                    "scoreComponents": {"anchor_relevance": 1.0},
                    "estimatedTokens": 100,
                    "protected": True,
                    "exact": True,
                    "recoverable": True,
                    "resolver": f"blueprint resolve {anchor}",
                    "text": anchor,
                })
        except Exception:
            pass
        # Fallback: emit as an anchor-only candidate so the planner sees it.
        candidates.append({
            "id": f"anchor:raw:{anchor}",
            "layer": 1,
            "sourceKind": "anchor",
            "sourceRef": anchor,
            "sourceHash": "0" * 64,
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
        })
    return candidates
