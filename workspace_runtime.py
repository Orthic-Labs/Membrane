"""Parent-workspace interface for Orthic Adapt / Adapt.

Adapt lives under ``adapt/`` but still needs a few parent-workspace services.
This module is the **only** import boundary for those deps. Do not vendor the
parent tree into adapt/; wire through this adapter (or optional stubs).

Required parent capabilities
----------------------------
1. Crypt runtime config
   - symbol: ``crypt_port(env=None) -> int``
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

Optional stubs
--------------
Set ``ADAPT_WORKSPACE_STUBS=1`` (or ``ADAPT_WORKSPACE_STUBS=1``) to load the
in-tree stub implementations instead of parent modules. Stubs are for unit
tests / standalone dry imports only — they must not be used for live apply.

See ``docs/workspace-interface.md`` for the contract table.
"""
from __future__ import annotations

import importlib
import os
import sys
from pathlib import Path
from types import ModuleType
from typing import Any, Callable


ADAPT_DIR = Path(__file__).resolve().parent


class WorkspaceRuntimeUnavailable(RuntimeError):
    """Parent workspace capability is missing and stubs were not enabled."""


def _stubs_enabled() -> bool:
    return os.environ.get("ADAPT_WORKSPACE_STUBS", "").strip() in {"1", "true", "yes"} or (
        os.environ.get("ADAPT_WORKSPACE_STUBS", "").strip() in {"1", "true", "yes"}
    )


def workspace_root() -> Path:
    """Resolve the Damned Designs workspace that owns ``tools/``.

    Supports both layouts:
      - top-level submodule: ``<workspace>/adapt``
      - nested historical path: ``<workspace>/tools/pipelines/memory/adapt``
    """
    if os.environ.get("WORKSPACE_ROOT"):
        return Path(os.environ["WORKSPACE_ROOT"]).resolve()
    candidates: list[Path] = [ADAPT_DIR.parent]
    # Nested under tools/pipelines/memory/adapt → parents[4] == workspace
    if len(ADAPT_DIR.parents) >= 5:
        candidates.append(ADAPT_DIR.parents[4])
    for candidate in candidates:
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


def _load_module(name: str, stub_factory: Callable[[], ModuleType]) -> ModuleType:
    if _stubs_enabled():
        return stub_factory()
    root = workspace_root()
    _ensure_parent_paths(root)
    try:
        return importlib.import_module(name)
    except ImportError as exc:
        raise WorkspaceRuntimeUnavailable(
            f"parent module {name!r} unavailable from {root}; "
            "set ADAPT_WORKSPACE_STUBS=1 for offline stubs"
        ) from exc


def _stub_runtime_config() -> ModuleType:
    mod = ModuleType("memory.runtime_config")

    def crypt_port(env=None, config_path=None) -> int:  # noqa: ANN001
        values = os.environ if env is None else env
        for key in ("CRYPT_PORT", "WORKSPACE_MEMORY_PORT"):
            if values.get(key):
                return int(values[key])
        return 47851

    mod.crypt_port = crypt_port  # type: ignore[attr-defined]
    return mod


def _stub_mirror() -> ModuleType:
    mod = ModuleType("mirror_append_only")

    class AppendOnlyViolation(RuntimeError):
        pass

    mod.AppendOnlyViolation = AppendOnlyViolation  # type: ignore[attr-defined]
    return mod


def _stub_session_inventory() -> ModuleType:
    mod = ModuleType("tools.pipelines.memory.context_session_inventory")

    def infer_candidate_client(tool: str, path: Path) -> str:  # noqa: ARG001
        return tool or "unknown"

    def unparsed_reason(path: Path) -> str:  # noqa: ARG001
        return "stub"

    mod.infer_candidate_client = infer_candidate_client  # type: ignore[attr-defined]
    mod.unparsed_reason = unparsed_reason  # type: ignore[attr-defined]
    return mod


def _stub_session_adapters() -> ModuleType:
    return ModuleType("tools.pipelines.memory.context_session_adapters")


def crypt_port(env: Any = None, config_path: Any = None) -> int:
    mod = _load_module("memory.runtime_config", _stub_runtime_config)
    return mod.crypt_port(env, config_path) if config_path is not None else mod.crypt_port(env)


def mirror_append_only() -> ModuleType:
    return _load_module("mirror_append_only", _stub_mirror)


def context_session_inventory() -> ModuleType:
    return _load_module(
        "tools.pipelines.memory.context_session_inventory",
        _stub_session_inventory,
    )


def context_session_adapters() -> ModuleType:
    return _load_module(
        "tools.pipelines.memory.context_session_adapters",
        _stub_session_adapters,
    )


__all__ = [
    "WorkspaceRuntimeUnavailable",
    "workspace_root",
    "crypt_port",
    "mirror_append_only",
    "context_session_inventory",
    "context_session_adapters",
]
