"""Membrane install-time version gate for blueprint >=0.2.0 <0.3.0 (D-M03, CU-H07).

Reads blueprint package.json version via the same install-root resolution Membrane already uses for IPC socket,
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

def check_blueprint_version(blueprint_root: Path | None) -> tuple[bool, str, str | None]:
    """Return (ok, code, detected_version). code is one of ok, blueprint_not_installed, blueprint_version_incompatible."""
    if blueprint_root is None or not blueprint_root.exists():
        return False, "blueprint_not_installed", None
    pkg = blueprint_root / "package.json"
    if not pkg.is_file():
        return False, "blueprint_not_installed", None
    try:
        data = json.loads(pkg.read_text())
        ver = data.get("version")
        if not isinstance(ver, str):
            return False, "blueprint_version_incompatible", None
        parsed = parse_version(ver)
        if parsed is None:
            return False, "blueprint_version_incompatible", ver
        if REQUIRED_MIN <= parsed < REQUIRED_MAX:
            return True, "ok", ver
        return False, "blueprint_version_incompatible", ver
    except Exception:
        return False, "blueprint_version_incompatible", None

def ensure_blueprint_compatible(blueprint_root: Path | None) -> None:
    ok, code, ver = check_blueprint_version(blueprint_root)
    if not ok:
        if code == "blueprint_not_installed":
            raise RuntimeError(f"blueprint_not_installed: blueprint not found at {blueprint_root} — required range >=0.2.0 <0.3.0")
        raise RuntimeError(f"blueprint_version_incompatible: detected {ver} — required range >=0.2.0 <0.3.0")

def resolve_blueprint_root(membrane_root: Path) -> Path | None:
    """Resolve only Blueprint absorbed into this Membrane checkout."""
    candidate = membrane_root / "blueprint"
    return candidate if candidate.exists() else None
