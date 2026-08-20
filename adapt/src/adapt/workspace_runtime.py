"""Parent-workspace interface for Membrane Adapt.

Adapt lives under ``membrane/adapt/`` but still needs a few workspace services.
This module is the **only** import boundary for those deps.

Required parent capabilities
----------------------------
1. Membrane runtime config
   - symbol: ``membrane_port(env=None) -> int``
   - source: ``tools/lib/memory/runtime_config.py``
   - used by: ``adapt_persistence``, ``multiwriter_conformance``

2. Session inventory / adapters
   - symbols: ``context_session_inventory``, ``context_session_adapters``
   - source: ``tools/pipelines/memory/``
   - used by: ``multiwriter_conformance`` discovery counts

3. Append-only mirror boundary
   - module: ``mirror_append_only``
   - source: ``tools/pipelines/memory/mirror_append_only.py``
   - used by: ``multiwriter_conformance`` receipt evidence

See ``docs/workspace-interface.md`` for the contract table.
"""
from __future__ import annotations

import importlib
import json
import os
import sys
from pathlib import Path
from types import ModuleType
from typing import Any


# Repository root. Package sources live under ``src/adapt``.
ADAPT_DIR = Path(__file__).resolve().parents[2]


class WorkspaceRuntimeUnavailable(RuntimeError):
    """Required workspace capability is unavailable."""


def workspace_root() -> Path:
    """Resolve the Damned Designs workspace that owns ``tools/``.

    Adapt is nested at ``<workspace>/membrane/adapt``. Walk ancestors so a
    source checkout and installed workspace resolve identically.
    """
    if os.environ.get("WORKSPACE_ROOT"):
        return Path(os.environ["WORKSPACE_ROOT"]).resolve()
    for candidate in ADAPT_DIR.parents:
        if (candidate / "tools" / "lib").is_dir():
            return candidate
    return ADAPT_DIR.parent


def _ensure_parent_paths(root: Path) -> None:
    for directory in (
        root,
        root / "tools" / "lib",
        root / "tools" / "pipelines" / "memory",
    ):
        text = str(directory)
        if text not in sys.path:
            sys.path.insert(0, text)


def _load_module(name: str) -> ModuleType:
    root = workspace_root()
    _ensure_parent_paths(root)
    try:
        return importlib.import_module(name)
    except ImportError as exc:
        raise WorkspaceRuntimeUnavailable(
            f"workspace module {name!r} unavailable from {root}"
        ) from exc


def _valid_port(raw: object, source: str) -> int:
    try:
        port = int(raw)
    except (TypeError, ValueError) as exc:
        raise ValueError(f"{source} must be an integer TCP port") from exc
    if not 1024 <= port <= 65535:
        raise ValueError(f"{source} must be between 1024 and 65535")
    return port


def membrane_port(env: Any = None, config_path: Any = None) -> int:
    values = os.environ if env is None else env
    if values.get("MEMBRANE_PORT"):
        return _valid_port(values["MEMBRANE_PORT"], "MEMBRANE_PORT")
    path = Path(config_path) if config_path is not None else workspace_root() / "tools/lib/memory/runtime.json"
    config = json.loads(path.read_text(encoding="utf-8"))
    if config.get("schemaVersion") != 1 or config.get("serviceId") != "membrane-local-v1":
        raise ValueError(f"invalid Membrane runtime config identity: {path}")
    if config.get("host") != "127.0.0.1":
        raise ValueError("Membrane runtime host must remain loopback-only")
    return _valid_port(config.get("port"), f"{path}:port")


def mirror_append_only() -> ModuleType:
    return _load_module("mirror_append_only")


def context_session_inventory() -> ModuleType:
    return _load_module("tools.pipelines.memory.context_session_inventory")


def context_session_adapters() -> ModuleType:
    return _load_module("tools.pipelines.memory.context_session_adapters")


__all__ = [
    "WorkspaceRuntimeUnavailable",
    "workspace_root",
    "membrane_port",
    "mirror_append_only",
    "context_session_inventory",
    "context_session_adapters",
]
