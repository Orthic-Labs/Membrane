"""RightContext provider adapters — federation gateway inputs.

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

# `providers/` sits at <workspace>/membrane/engine/federation/providers.
# parents[3] used to be the `tools` directory (old tools/crypt/federation
# layout) and is now `membrane`; parents[4] is the workspace root under BOTH
# layouts, which is why skills.py/crypt.py survived the move untouched.
_WORKSPACE_ROOT = Path(__file__).resolve().parents[4]
_LEGACY_TOOLS_ROOT = Path(__file__).resolve().parents[3]


def workspace_tools_path(*parts: str) -> Path:
    """Resolve a path under the workspace `tools/` tree across both layouts.

    The membrane consolidation moved this package from `tools/crypt/` to
    `membrane/engine/`, which silently repointed every `parents[3] / "skills"`
    expression at a non-existent `membrane/skills/...`. The blueprint, audit,
    architect, and anchors lanes then failed instantly with `unavailable` on
    every prompt. Probe the real layouts; first existing hit wins, and the
    canonical location is returned when neither exists so the caller's own
    "missing at <path>" warning names the path an operator should create.
    """
    override = os.environ.get("RIGHTCONTEXT_TOOLS_ROOT", "").strip()
    roots = (
        [Path(override)]
        if override
        else [_WORKSPACE_ROOT / "tools", _LEGACY_TOOLS_ROOT]
    )
    candidates = [root.joinpath(*parts) for root in roots]
    for candidate in candidates:
        if candidate.exists():
            return candidate
    return candidates[0]


def canonical_repository_id(repo_root: Path | str) -> str:
    """Return the workspace scope slug for a repo root — never a machine path.

    `D:\\Claude` -> `D--Claude`; `D:\\Claude\\heardright` -> `D--Claude-heardright`.

    This is the one identity space the whole system already shares: the engine's
    `memories.scope_id`, `crypt/src/scope.rs::canonical_scope_chain`, and
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
