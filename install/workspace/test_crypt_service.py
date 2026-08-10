"""Tests for the Membrane-owned Crypt workspace-installation primitives.

Runs with the workspace's tools venv: `.venv-tools/bin/pytest membrane/install/workspace/`.
Never invokes real launchctl — every subprocess.run call is a fixture.
"""
from __future__ import annotations

import sqlite3
import subprocess

import crypt_service as cs


def _fake_runner(returncode: int = 0):
    calls = []

    def run(args, **_kwargs):
        calls.append(args)
        return subprocess.CompletedProcess(args, returncode, "501\n", "")

    run.calls = calls
    return run


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
    with sqlite3.connect(legacy) as connection:
        connection.execute("CREATE TABLE marker (value TEXT)")
        connection.execute("INSERT INTO marker VALUES ('kept')")

    canonical = cs.migrate_legacy_crypt_database(tmp_path)

    assert canonical == tmp_path / "crypt-engine.db"
    assert not legacy.exists()
    with sqlite3.connect(canonical) as connection:
        assert connection.execute("SELECT value FROM marker").fetchone() == ("kept",)


def test_migrate_macos_label_writes_new_plist_and_bootstraps(tmp_path):
    home = tmp_path / "home"
    (home / "Library/LaunchAgents").mkdir(parents=True)
    new_plist = home / "Library/LaunchAgents/com.membrane.workspace.crypt-serve.plist"
    runner = _fake_runner()

    cs.migrate_macos_label(
        "com.example.legacy-crypt-serve", "com.membrane.workspace.crypt-serve",
        new_plist, "<plist>new</plist>", home=home, uid="501", runner=runner,
    )

    assert new_plist.read_text(encoding="utf-8") == "<plist>new</plist>"
    assert ["launchctl", "bootout", "gui/501/com.example.legacy-crypt-serve"] in runner.calls
    assert ["launchctl", "bootstrap", "gui/501", str(new_plist)] in runner.calls


def test_migrate_macos_label_deletes_old_plist_file(tmp_path):
    """A stale plist on disk after bootout is the double-ownership bug this exists to kill."""
    home = tmp_path / "home"
    agents = home / "Library/LaunchAgents"
    agents.mkdir(parents=True)
    old_plist = agents / "com.example.legacy-crypt-serve.plist"
    old_plist.write_text("<plist>old</plist>", encoding="utf-8")
    new_plist = agents / "com.membrane.workspace.crypt-serve.plist"

    cs.migrate_macos_label(
        "com.example.legacy-crypt-serve", "com.membrane.workspace.crypt-serve",
        new_plist, "<plist>new</plist>", home=home, uid="501", runner=_fake_runner(),
    )

    assert not old_plist.exists()
    assert new_plist.exists()


def test_migrate_macos_label_is_idempotent_once_old_label_is_gone(tmp_path):
    """Second run with the old label already absent must be a clean no-op, not an error."""
    home = tmp_path / "home"
    (home / "Library/LaunchAgents").mkdir(parents=True)
    new_plist = home / "Library/LaunchAgents/com.membrane.workspace.crypt-serve.plist"
    runner = _fake_runner()

    cs.migrate_macos_label(
        "com.example.legacy-crypt-serve", "com.membrane.workspace.crypt-serve",
        new_plist, "<plist>v1</plist>", home=home, uid="501", runner=runner,
    )
    cs.migrate_macos_label(
        "com.example.legacy-crypt-serve", "com.membrane.workspace.crypt-serve",
        new_plist, "<plist>v2</plist>", home=home, uid="501", runner=runner,
    )

    assert new_plist.read_text(encoding="utf-8") == "<plist>v2</plist>"


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


def test_render_crypt_shim_posix_format_on_posix_host_omits_runc_shell():
    body = cs.render_crypt_shim(
        "/tools/crypt", "/db/crypt-engine.db", "/ort.so", "/hf", "runc",
        compat_prefix=None, crypt_port=47851, cmd_format=False, win=False,
    )
    assert "CRYPT_RUNC_SHELL" not in body


def test_setup_crypt_serve_autostart_tolerates_foreign_port_owner(tmp_path):
    """Neither label owned by launchd (e.g. Membrane Hub holds the port): write the plist,
    skip the socket wait + bootstrap, never touch real launchctl."""
    repo = tmp_path / "workspace"
    home = tmp_path / "home"
    runner = _fake_runner(returncode=1)  # bootout fails: launchd doesn't own either label

    cs.setup_crypt_serve_autostart(repo, home, 47851, win=False, mac=True, runner=runner)

    assert all(call[:2] != ["launchctl", "bootstrap"] for call in runner.calls)
    plist = home / "Library/LaunchAgents/com.membrane.workspace.crypt-serve.plist"
    assert plist.exists()
    assert "com.membrane.workspace.crypt-serve" in plist.read_text(encoding="utf-8")
