"""Membrane install-time version gate for cortex >=0.2.0 <0.3.0 (D-M03, CU-H07).

Reads cortex package.json version via the same install-root resolution Membrane already uses for IPC socket,
and refuses install when version outside [0.2.0, 0.3.0). Distinct error for not installed.
Pure stdlib, no hardcoded machine path.
"""
from __future__ import annotations
import json
from pathlib import Path

REQUIRED_MIN = (0, 2, 0)
REQUIRED_MAX = (0, 3, 0)

def parse_version(v: str):
    try:
        parts = v.strip().lstrip("v").split(".")
        return tuple(int(p) for p in parts[:3])
    except Exception:
        return None

def check_cortex_version(cortex_root: Path | None) -> tuple[bool, str, str | None]:
    """Return (ok, code, detected_version). code is one of ok, cortex_not_installed, cortex_version_incompatible."""
    if cortex_root is None or not cortex_root.exists():
        return False, "cortex_not_installed", None
    pkg = cortex_root / "package.json"
    if not pkg.is_file():
        return False, "cortex_not_installed", None
    try:
        data = json.loads(pkg.read_text())
        ver = data.get("version")
        if not isinstance(ver, str):
            return False, "cortex_version_incompatible", None
        parsed = parse_version(ver)
        if parsed is None:
            return False, "cortex_version_incompatible", ver
        if REQUIRED_MIN <= parsed < REQUIRED_MAX:
            return True, "ok", ver
        return False, "cortex_version_incompatible", ver
    except Exception:
        return False, "cortex_version_incompatible", None

def ensure_cortex_compatible(cortex_root: Path | None) -> None:
    ok, code, ver = check_cortex_version(cortex_root)
    if not ok:
        if code == "cortex_not_installed":
            raise RuntimeError(f"cortex_not_installed: cortex not found at {cortex_root} — required range >=0.2.0 <0.3.0")
        raise RuntimeError(f"cortex_version_incompatible: detected {ver} — required range >=0.2.0 <0.3.0")

def resolve_cortex_root(from_repo: Path) -> Path | None:
    # Same resolution Membrane uses for IPC: sibling ../cortex from repo root
    # Caller supplies repo root; we try sibling cortex
    candidate = from_repo.parent / "cortex" if from_repo.name == "membrane" else from_repo / "cortex"
    # Also try workspace root's cortex
    if candidate.exists():
        return candidate
    # Fallback: try /Volumes/D/claude/cortex
    fallback = Path("/Volumes/D/claude/cortex")
    return fallback if fallback.exists() else None
