"""Git provider — emits git metadata candidates (branch, HEAD, status summary)."""
from __future__ import annotations

import subprocess
from pathlib import Path


def _git(repo_root: Path, args: list[str]) -> str:
    try:
        proc = subprocess.run(
            ["git", *args],
            cwd=str(repo_root),
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except FileNotFoundError:
        raise FileNotFoundError("git binary not on PATH")
    if proc.returncode != 0:
        raise RuntimeError(f"git {args} exited {proc.returncode}: {proc.stderr.strip()[:120]}")
    return proc.stdout.strip()


def produce(repo_root: Path) -> list[dict]:
    """Emit git metadata candidates: branch, HEAD commit, status summary."""
    candidates: list[dict] = []
    try:
        branch = _git(repo_root, ["rev-parse", "--abbrev-ref", "HEAD"])
        candidates.append({
            "id": f"git:meta:branch:{branch}",
            "layer": 1,
            "sourceKind": "git_meta",
            "sourceRef": f"git:branch:{branch}",
            "sourceHash": "0" * 64,
            "trustClass": "workspace_tracked",
            "instructionPolicy": "data_only",
            "providerScore": 0.4,
            "scoreComponents": {"git_relevance": 0.5},
            "estimatedTokens": 8,
            "protected": False,
            "exact": True,
            "recoverable": True,
            "resolver": "git rev-parse --abbrev-ref HEAD",
            "text": f"current branch {branch}",
        })
    except Exception:
        pass
    try:
        head = _git(repo_root, ["log", "-1", "--format=%H"])
        candidates.append({
            "id": f"git:meta:head:{head[:12]}",
            "layer": 1,
            "sourceKind": "git_meta",
            "sourceRef": f"git:commit:{head}",
            "sourceHash": head,
            "trustClass": "workspace_tracked",
            "instructionPolicy": "data_only",
            "providerScore": 0.3,
            "scoreComponents": {"git_relevance": 0.3},
            "estimatedTokens": 8,
            "protected": False,
            "exact": True,
            "recoverable": True,
            "resolver": "git log -1 --format=%H",
            "text": f"HEAD commit {head[:12]}",
        })
    except Exception:
        pass
    return candidates
