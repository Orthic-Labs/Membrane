"""ScopeGrant boundary for the Membrane federation gateway.

Blueprint owns repository truth, Cortex owns durable knowledge, and the
Membrane resident service owns grant storage, lifecycle, and transport. This
adapter therefore accepts a service-owned lookup callback and only performs
request-context validation; it never opens either subsystem's storage.
"""
from __future__ import annotations

from collections.abc import Callable, Mapping
from pathlib import Path
from typing import Any


GrantLookup = Callable[[Path, str], Mapping[str, Any] | None]


def lookup(
    repo_root: Path,
    grant_id: str,
    *,
    transport: GrantLookup | None = None,
) -> dict[str, Any] | None:
    """Ask the Membrane service adapter for one grant.

    Transport is deliberately injected by the service owner. Without an
    owner-provided transport, lookup fails closed and performs no I/O.
    """
    if transport is None:
        return None
    grant = transport(repo_root, grant_id)
    if grant is None:
        return None
    if not isinstance(grant, Mapping):
        raise TypeError("scope grant transport returned a non-object")
    return dict(grant)


def validate(
    grant: Mapping[str, Any],
    *,
    request_client: str,
    request_repo_root: Path,
    request_task: str,
    request_session: str,
    request_manifest_digest: str | None = None,
) -> None:
    """Fail-closed validation against the exact request context.

    Service transport owns grant status and expiry. This function validates
    only immutable request bindings required by federation, including the
    canonical ``repositoryRoot`` field; ``repositoryIds`` are identifiers,
    not filesystem-path aliases.
    """
    if not grant.get("nonce"):
        raise PermissionError(
            f"scope_grant_invalid: {grant.get('id')!r} has no nonce"
        )
    if grant.get("client") != request_client:
        raise PermissionError(
            f"scope_grant_invalid: client={grant.get('client')!r} != request={request_client!r}"
        )
    if grant.get("taskId") != request_task:
        raise PermissionError(
            f"scope_grant_invalid: task_id={grant.get('taskId')!r} != request={request_task!r}"
        )
    if grant.get("sessionId") != request_session:
        raise PermissionError(
            f"scope_grant_invalid: session_id={grant.get('sessionId')!r} != request={request_session!r}"
        )
    repository_root = grant.get("repositoryRoot")
    if not isinstance(repository_root, str) or not repository_root:
        raise PermissionError(
            f"scope_grant_invalid: {grant.get('id')!r} has no repositoryRoot"
        )
    if not Path(repository_root).is_absolute():
        raise PermissionError(
            f"scope_grant_invalid: repositoryRoot must be absolute: {repository_root!r}"
        )
    repo_root_canon = str(request_repo_root.resolve())
    if str(Path(repository_root).resolve()) != repo_root_canon:
        raise PermissionError(
            f"scope_grant_invalid: repository_root={repo_root_canon!r} != grant={repository_root!r}"
        )
    if request_manifest_digest is not None:
        if grant.get("manifestDigest") != request_manifest_digest:
            raise PermissionError(
                f"scope_grant_invalid: manifestDigest={grant.get('manifestDigest')!r} != request={request_manifest_digest!r}"
            )
