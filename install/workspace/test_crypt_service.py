"""Tests for the Membrane-owned Crypt workspace-installation primitives.

Runs with the workspace's tools venv: `.venv-tools/bin/pytest membrane/install/workspace/`.
"""
from __future__ import annotations

import sqlite3
from pathlib import Path

import crypt_service as cs


def test_migrate_legacy_crypt_database_does_not_move_canonical_when_no_legacy(tmp_path):
    """Regression: this bug moved a live 253MB database into a backup dir on every run."""
    cache = tmp_path
    canonical = cache / "crypt-engine.db"
    with sqlite3.connect(canonical) as connection:
        connection.execute("CREATE TABLE marker (value TEXT)")
        connection.execute("INSERT INTO marker VALUES ('live')")

    result = cs.migrate_legacy_crypt_database(cache)

    assert result == canonical
    assert canonical.exists()
    assert not (cache / "rename-backups").exists()
    with sqlite3.connect(canonical) as connection:
        assert connection.execute("SELECT value FROM marker").fetchone() == ("live",)


def test_migrate_legacy_crypt_database_moves_legacy_to_canonical(tmp_path):
    legacy = tmp_path / "memright-engine.db"
    connection = sqlite3.connect(legacy)
    try:
        connection.execute("CREATE TABLE marker (value TEXT)")
        connection.execute("INSERT INTO marker VALUES ('kept')")
        connection.commit()
    finally:
        connection.close()

    canonical = cs.migrate_legacy_crypt_database(tmp_path)

    assert canonical == tmp_path / "crypt-engine.db"
    assert not legacy.exists()
    with sqlite3.connect(canonical) as connection:
        assert connection.execute("SELECT value FROM marker").fetchone() == ("kept",)


def test_render_crypt_shim_windows_cmd_format():
    body = cs.render_crypt_shim(
        "C:/tools/crypt.exe", "C:/db/crypt-engine.db", "C:/ort.dll", "C:/hf", "runc",
        compat_prefix="CRYPT", crypt_port=47851, cmd_format=True, win=True,
    )
    assert body.startswith("@echo off\r\n")
    assert "CRYPT_RUNC_SHELL" in body
    assert 'crypt.exe" runc --spill-dir "%CD%\\.cache\\runc" %*' in body


def test_render_crypt_shim_posix_format_on_windows_host_keeps_runc_shell():
    body = cs.render_crypt_shim(
        "/tools/crypt", "/db/crypt-engine.db", "/ort.so", "/hf", "runc",
        compat_prefix=None, crypt_port=47851, cmd_format=False, win=True,
    )
    assert body.startswith("export CRYPT_DB=")
    assert "CRYPT_RUNC_SHELL" in body
    assert 'exec "/tools/crypt" runc --spill-dir "$PWD/.cache/runc" "$@"' in body


def test_render_membrane_facade_shim_keeps_runc_contract():
    body = cs.render_crypt_shim(
        "/tools/membrane", "/db/crypt-engine.db", "/ort.so", "/hf", "cli runc",
        compat_prefix=None, crypt_port=47851, cmd_format=False, win=False,
    )
    assert 'exec "/tools/membrane" cli runc --spill-dir "$PWD/.cache/runc" "$@"' in body


def test_render_crypt_shim_posix_format_on_posix_host_omits_runc_shell():
    body = cs.render_crypt_shim(
        "/tools/crypt", "/db/crypt-engine.db", "/ort.so", "/hf", "runc",
        compat_prefix=None, crypt_port=47851, cmd_format=False, win=False,
    )
    assert "CRYPT_RUNC_SHELL" not in body


def test_workspace_installation_has_no_os_registration_path():
    workspace = Path(__file__).parent
    source = (workspace / "crypt_service.py").read_text(encoding="utf-8").lower()
    forbidden = ("launch" + "d", "system" + "d", "scht" + "asks")
    assert not any(token in source for token in forbidden)
    assert not (workspace / ("crypt_service_" + "launch" + "d.py")).exists()
    assert not (workspace / ("crypt_service_" + "registrars.py")).exists()
