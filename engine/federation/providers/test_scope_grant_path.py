from pathlib import Path

import pytest

from federation.providers import scope_grant


def test_lookup_uses_resident_transport_by_default(tmp_path: Path, monkeypatch) -> None:
    observed = {}

    def resident(repo_root: Path, grant_id: str) -> dict:
        observed.update(repo_root=repo_root, grant_id=grant_id)
        return {"id": grant_id, "repositoryRoot": str(repo_root)}

    monkeypatch.setattr(scope_grant, "_resident_lookup", resident)
    assert scope_grant.lookup(tmp_path, "sg-resident") == {
        "id": "sg-resident",
        "repositoryRoot": str(tmp_path),
    }
    assert observed == {"repo_root": tmp_path, "grant_id": "sg-resident"}


def test_lookup_delegates_to_service_transport_without_aliases(tmp_path: Path) -> None:
    observed = {}

    def transport(repo_root: Path, grant_id: str) -> dict:
        observed.update(repo_root=repo_root, grant_id=grant_id)
        return {"id": grant_id, "repositoryRoot": str(repo_root)}

    grant = scope_grant.lookup(tmp_path, "sg-service", transport=transport)
    assert grant == {"id": "sg-service", "repositoryRoot": str(tmp_path)}
    assert observed == {"repo_root": tmp_path, "grant_id": "sg-service"}


def test_validate_binds_canonical_repository_root_not_repository_id(tmp_path: Path) -> None:
    grant = {
        "id": "sg-service",
        "client": "mcp",
        "taskId": "inspect handler",
        "sessionId": "session-1",
        "repositoryRoot": str(tmp_path),
        "repositoryIds": ["repo-stable-id"],
        "manifestDigest": "sha256:" + "a" * 64,
        "nonce": "nonce-12345678",
    }
    scope_grant.validate(
        grant,
        request_client="mcp",
        request_repo_root=tmp_path,
        request_task="inspect handler",
        request_session="session-1",
        request_manifest_digest=grant["manifestDigest"],
    )

    with pytest.raises(PermissionError, match="repository_root"):
        scope_grant.validate(
            {**grant, "repositoryRoot": str(tmp_path / "other")},
            request_client="mcp",
            request_repo_root=tmp_path,
            request_task="inspect handler",
            request_session="session-1",
        )
