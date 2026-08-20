"""Parent-workspace interface for Orthic Adapt / Adapt.

Adapt lives under ``membrane/adapt/`` but still needs a few workspace services.
This module is the **only** import boundary for those deps.

Required parent capabilities
----------------------------
1. Cortex runtime config
   - symbol: ``cortex_port(env=None) -> int``
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


def cortex_port(env: Any = None, config_path: Any = None) -> int:
    mod = _load_module("memory.runtime_config")
    return mod.cortex_port(env, config_path) if config_path is not None else mod.cortex_port(env)


def mirror_append_only() -> ModuleType:
    return _load_module("mirror_append_only")


def context_session_inventory() -> ModuleType:
    return _load_module("tools.pipelines.memory.context_session_inventory")


def context_session_adapters() -> ModuleType:
    return _load_module("tools.pipelines.memory.context_session_adapters")


__all__ = [
    "WorkspaceRuntimeUnavailable",
    "workspace_root",
    "cortex_port",
    "mirror_append_only",
    "context_session_inventory",
    "context_session_adapters",
]
