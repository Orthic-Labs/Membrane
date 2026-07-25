"""RightContext provider adapters — federation gateway inputs.

Each module exposes a `produce(repo_root, task, ...) -> ([candidate, ...],
generation_id)` function. Candidates are ContentCandidateSet v1 records
already shaped with provider="" + sourceGeneration metadata. Failures are
surfaced as exceptions; the gateway translates those to providerWarnings
and continues.
"""
from __future__ import annotations

from pathlib import Path

__all__ = ["canonical_repository_id"]


def canonical_repository_id(repo_root: Path | str) -> str:
    """Return the workspace scope slug for a repo root — never a machine path.

    `D:\\Claude` -> `D--Claude`; `D:\\Claude\\heardright` -> `D--Claude-heardright`.

    This is the one identity space the whole system already shares: the engine's
    `memories.scope_id`, `memright/src/scope.rs::canonical_scope_chain`, and
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
