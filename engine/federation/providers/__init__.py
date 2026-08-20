"""Membrane provider adapters — federation gateway inputs.

Each module exposes a `produce(repo_root, task, ...) -> ([candidate, ...],
generation_id)` function. Candidates are ContentCandidateSet v1 records
already shaped with provider="" + sourceGeneration metadata. Failures are
surfaced as exceptions; the gateway translates those to providerWarnings
and continues.
"""
from __future__ import annotations

import os
from pathlib import Path

__all__ = ["canonical_repository_id", "workspace_tools_path"]

_WORKSPACE_ROOT = next(
    (
        candidate
        for candidate in Path(__file__).resolve().parents
        if (candidate / "tools" / "lib").is_dir()
    ),
    None,
)


def workspace_tools_path(*parts: str) -> Path:
    """Resolve a path under current workspace's `tools/` tree."""
    override = os.environ.get("MEMBRANE_TOOLS_ROOT", "").strip()
    if override:
        root = Path(override)
    elif _WORKSPACE_ROOT is not None:
        root = _WORKSPACE_ROOT / "tools"
    else:
        raise RuntimeError("workspace tools root unavailable; set MEMBRANE_TOOLS_ROOT")
    return root.joinpath(*parts)


def canonical_repository_id(repo_root: Path | str) -> str:
    """Return the workspace scope slug for a repo root — never a machine path.

    `D:\\Claude` -> `D--Claude`; `D:\\Claude\\heardright` -> `D--Claude-heardright`.

    This is the one identity space the whole system already shares: the engine's
    `memories.scope_id`, `cortex/src/scope.rs::canonical_scope_chain`, and
    `tools/hooks/ingest_memory.py::_scope_for_path` all use this slug. The typed
    stores derive record IDs from it (`audit_store.derive_finding_id` hashes it
    into every finding ID), so it must stay stable across runs and machines —
    which is why `audit_store.py` specifies "issued by the planner/manifest;
    never a machine path".

    The leading drive token is uppercased for the same reason scope.rs does it:
    so `d--Claude` and `D--Claude` can never fork into two identities.
    """
    norm = str(Path(repo_root).resolve()).replace("\\", "/")
    slug = norm.replace(":", "-").replace("/", "-").strip("-")
    head, sep, tail = slug.partition("-")
    if sep and len(head) == 1 and head.isalpha():
        slug = head.upper() + sep + tail
    return slug or "global"
