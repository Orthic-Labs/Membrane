"""Membrane install-time Blueprint version gate."""
from __future__ import annotations

import json
from pathlib import Path

REQUIRED_MIN = (0, 2, 0)
REQUIRED_MAX = (0, 3, 0)


def parse_version(value: str):
    try:
        parts = value.strip().lstrip("v").split(".")
        return tuple(int(part) for part in parts[:3])
    except Exception:
        return None


def check_blueprint_version(blueprint_root: Path | None) -> tuple[bool, str, str | None]:
    """Return ``(ok, code, detected_version)`` with typed absence/version errors."""
    if blueprint_root is None or not blueprint_root.exists():
        return False, "blueprint_not_installed", None
    package = blueprint_root / "package.json"
    if not package.is_file():
        return False, "blueprint_not_installed", None
    try:
        value = json.loads(package.read_text(encoding="utf-8")).get("version")
        if not isinstance(value, str):
            return False, "blueprint_version_incompatible", None
        parsed = parse_version(value)
        if parsed is None:
            return False, "blueprint_version_incompatible", value
        if REQUIRED_MIN <= parsed < REQUIRED_MAX:
            return True, "ok", value
        return False, "blueprint_version_incompatible", value
    except Exception:
        return False, "blueprint_version_incompatible", None


def ensure_blueprint_compatible(blueprint_root: Path | None) -> None:
    ok, code, version = check_blueprint_version(blueprint_root)
    if ok:
        return
    if code == "blueprint_not_installed":
        raise RuntimeError(
            f"blueprint_not_installed: blueprint not found at {blueprint_root} — required range >=0.2.0 <0.3.0"
        )
    raise RuntimeError(
        f"blueprint_version_incompatible: detected {version} — required range >=0.2.0 <0.3.0"
    )


def resolve_blueprint_root(membrane_root: Path) -> Path | None:
    candidate = membrane_root / "blueprint"
    return candidate if candidate.exists() else None
