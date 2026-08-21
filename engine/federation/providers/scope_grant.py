"""ScopeGrant boundary for the Membrane federation gateway.

Blueprint owns repository truth, Cortex owns durable knowledge, and the
Membrane resident service owns grant storage, lifecycle, and transport. This
adapter therefore accepts a service-owned lookup callback and only performs
request-context validation; it never opens either subsystem's storage.
"""
from __future__ import annotations

from collections.abc import Callable, Mapping
import json
import os
from pathlib import Path
from typing import Any
import urllib.request


GrantLookup = Callable[[Path, str], Mapping[str, Any] | None]


def _resident_lookup(repo_root: Path, grant_id: str) -> Mapping[str, Any] | None:
    port = os.environ.get("MEMBRANE_PORT", "47851").strip() or "47851"
    token = os.environ.get("MEMBRANE_API_TOKEN", "").strip()
    token_file = os.environ.get("MEMBRANE_API_TOKEN_FILE", "").strip()
    candidates = [Path(token_file)] if token_file else []
    candidates.extend(
        parent / "tools" / ".cache" / "memory" / "api-token"
        for parent in (repo_root, *repo_root.parents)
    )
    if not token:
        for candidate in candidates:
            try:
                token = candidate.read_text(encoding="utf-8").strip()
            except OSError:
                continue
            if token:
                break
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/scope_grants",
        data=json.dumps({"operation": "lookup", "id": grant_id}).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {token}",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=0.35) as response:
            if response.status != 200:
                return None
            value = json.loads(response.read().decode("utf-8"))
    except (OSError, ValueError, json.JSONDecodeError):
        return None
    return value if isinstance(value, Mapping) else None


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
    grant = (transport or _resident_lookup)(repo_root, grant_id)
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
