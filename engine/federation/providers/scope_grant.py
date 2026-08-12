"""ScopeGrant lookup — reads from the central context catalog.

The catalog is a separate SQLite file at `<context-home>/catalog.db`. The
Rust `crypt` service owns the authoritative grant store; this module
reads from the same store via a small SQL helper.

The gateway treats cross-root evidence as conditional on an active grant;
ungranted cross-root candidates are filtered out before they reach the
planner (which would reject them anyway with `cross_root`).
"""
from __future__ import annotations

import os
import sqlite3
from pathlib import Path


class CatalogPathError(ValueError):
    """Typed pre-I/O rejection for ambiguous catalog bindings."""


def _absolute(binding: str, value: str | None) -> Path | None:
    if not value:
        return None
    path = Path(value)
    if not path.is_absolute():
        raise CatalogPathError(
            f"catalog path binding {binding} must be absolute: {value}"
        )
    return path


def _catalog_path() -> Path:
    explicit = _absolute(
        "RIGHTCONTEXT_CATALOG", os.environ.get("RIGHTCONTEXT_CATALOG")
    )
    if explicit:
        return explicit
    context_home = _absolute("CONTEXT_HOME", os.environ.get("CONTEXT_HOME"))
    if context_home:
        return context_home / "catalog.db"
    crypt_db = _absolute("CRYPT_DB", os.environ.get("CRYPT_DB"))
    if crypt_db:
        return crypt_db.parent / "catalog.db"
    workspace = _absolute("WORKSPACE_ROOT", os.environ.get("WORKSPACE_ROOT"))
    if workspace:
        return workspace / "tools" / ".cache" / "memory" / "catalog.db"
    raise CatalogPathError(
        "catalog path is unbound: set RIGHTCONTEXT_CATALOG, CONTEXT_HOME, "
        "CRYPT_DB, or WORKSPACE_ROOT"
    )


def lookup(repo_root: Path, grant_id: str) -> dict | None:
    """Return the grant row if active; None if missing, expired, or revoked.

    The grant row carries ALL the binding fields needed for client-side
    validation: client, task_id, session_id, manifest_digest,
    repository_ids. Callers must call `validate(grant, request_context)`
    before trusting the grant to authorise cross-root evidence.
    """
    path = _catalog_path()
    if not path.exists():
        return None
    conn = None
    try:
        conn = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)
        row = conn.execute(
            "SELECT id, client, status, expires_at_unix, repository_ids, "
            "task_id, session_id, manifest_digest, nonce "
            "FROM scope_grants WHERE id = ?",
            (grant_id,),
        ).fetchone()
    except sqlite3.Error:
        return None
    finally:
        if conn is not None:
            conn.close()
    if not row:
        return None
    (grant_id_v, client, status, expires_at_unix, repository_ids_csv,
     task_id, session_id, manifest_digest, nonce) = row
    import time
    if status != "active" or expires_at_unix < int(time.time()):
        return None
    return {
        "id": grant_id_v,
        "client": client,
        "status": status,
        "expiresAtUnix": expires_at_unix,
        "repositoryIds": [s for s in (repository_ids_csv or "").split(",") if s],
        "taskId": task_id,
        "sessionId": session_id,
        "manifestDigest": manifest_digest,
        "nonce": nonce,
    }


def validate(grant: dict, *, request_client: str, request_repo_root: Path,
            request_task: str, request_session: str,
            request_manifest_digest: str | None = None) -> None:
    """Fail-closed validation: throw PermissionError if the grant does
    not bind to the exact request context.

    Per dispatch §G1 trust contract:
    - `client` must match the requesting client identity.
    - `task_id` must match the requesting task string.
    - `session_id` must match the requesting session string.
    - At least one entry in `repositoryIds` must equal the resolved
      repo-root path (after canonicalisation).
    - When `manifestDigest` is supplied by the request, the grant's
      stored `manifestDigest` must match.
    - `nonce` must be present (defence-in-depth).
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
    repo_root_canon = str(request_repo_root.resolve())
    bound_repos = grant.get("repositoryIds") or []
    if not any(str(Path(p).resolve()) == repo_root_canon for p in bound_repos):
        raise PermissionError(
            f"scope_grant_invalid: repository_root={repo_root_canon!r} not in {bound_repos!r}"
        )
    if request_manifest_digest is not None:
        if grant.get("manifestDigest") != request_manifest_digest:
            raise PermissionError(
                f"scope_grant_invalid: manifestDigest={grant.get('manifestDigest')!r} != request={request_manifest_digest!r}"
            )
