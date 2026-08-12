from pathlib import Path

import pytest

from federation.providers import scope_grant


VARS = (
    "RIGHTCONTEXT_CATALOG",
    "CONTEXT_HOME",
    "CRYPT_DB",
    "WORKSPACE_ROOT",
)


def bind(monkeypatch: pytest.MonkeyPatch, **values: str) -> None:
    for name in VARS:
        monkeypatch.delenv(name, raising=False)
    for name, value in values.items():
        monkeypatch.setenv(name, value)


def test_catalog_path_uses_canonical_precedence(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    explicit = tmp_path / "explicit" / "catalog.db"
    context = tmp_path / "context"
    memory = tmp_path / "memory" / "crypt-engine.db"
    workspace = tmp_path / "workspace"
    bind(
        monkeypatch,
        RIGHTCONTEXT_CATALOG=str(explicit),
        CONTEXT_HOME=str(context),
        CRYPT_DB=str(memory),
        WORKSPACE_ROOT=str(workspace),
    )
    assert scope_grant._catalog_path() == explicit

    bind(monkeypatch, CONTEXT_HOME=str(context))
    assert scope_grant._catalog_path() == context / "catalog.db"
    bind(monkeypatch, CRYPT_DB=str(memory))
    assert scope_grant._catalog_path() == memory.parent / "catalog.db"
    bind(monkeypatch, WORKSPACE_ROOT=str(workspace))
    assert scope_grant._catalog_path() == workspace / "tools/.cache/memory/catalog.db"


def test_catalog_path_rejects_relative_and_unbound_before_io(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    bind(monkeypatch, RIGHTCONTEXT_CATALOG="catalog.db")
    with pytest.raises(scope_grant.CatalogPathError, match="must be absolute"):
        scope_grant._catalog_path()

    bind(monkeypatch)
    with pytest.raises(scope_grant.CatalogPathError, match="catalog path is unbound"):
        scope_grant._catalog_path()
