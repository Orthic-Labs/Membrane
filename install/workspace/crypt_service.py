"""Membrane-owned Crypt workspace-installation primitives. Pure stdlib only.

Ported out of Adrian's private workspace script (tools/setup-workspace.py) so Membrane
carries zero external-repo dependencies. Every path this module renders into an on-disk
artifact is derived from an explicit ``repo``/``home``/``port`` argument the caller
supplies (from WORKSPACE_ROOT / MEMBRANE_WORKSPACE_ROOT or its own config) — never a
hardcoded machine path or username.

"""
from __future__ import annotations

import os
import socket
import sqlite3
import time
from pathlib import Path

from crypt_service_launchd import DEFAULT_CRYPT_SERVE_LABEL, render_crypt_launchd_plist  # noqa: F401
from crypt_service_registrars import setup_crypt_serve_autostart  # noqa: F401

def log(msg: str) -> None:
    print(f"[membrane-workspace] {msg}")


def install_workspace_crypt_service(
    repo: Path, home: Path, port: int, *, mac: bool, win: bool = False,
    registrar=setup_crypt_serve_autostart,
) -> dict[str, str]:
    """Mac workspace installer entrypoint: launchd adopts Crypt singleton ownership."""
    if win or not mac:
        raise ValueError("Crypt workspace service installation is macOS-only")
    migrate_legacy_crypt_database(repo / "tools/.cache/memory")
    registrar(repo, home, port, mac=True, win=False)
    return {"lifecycle": "launchd", "label": DEFAULT_CRYPT_SERVE_LABEL}


def migrate_legacy_crypt_database(cache_dir: Path) -> Path:
    """Move the last pre-Crypt database name without losing a live WAL."""
    cache = cache_dir
    # DO NOT rewrite this filename to the current vocabulary. It names the RETIRED
    # database this function exists to migrate FROM; commit 322cd7e89's retired-
    # vocabulary sweep renamed it to the canonical name, which made legacy ==
    # canonical, made `dual_names` always true, and turned a no-op migration into
    # an os.replace() of the LIVE database into a backup directory on every run.
    legacy = cache / "memright-engine.db"
    canonical = cache / "crypt-engine.db"
    if not legacy.exists():
        return canonical
    dual_names = canonical.exists()

    cache.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(legacy, timeout=30) as connection:
        checkpoint = connection.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
        if checkpoint and checkpoint[0] != 0:
            # A busy checkpoint means a live Crypt still holds the legacy database.
            # Skip rather than raise: POSIX would let us rename the open file out
            # from under that process (silent split-brain) and Windows would fail
            # the rename with WinError 32 mid-run. Neither is a setup-time repair,
            # so leave the database alone and tell the operator what to do.
            log(
                "crypt migration skipped — legacy database is in use "
                f"(checkpoint busy: {checkpoint}); stop Crypt and re-run to migrate"
            )
            return canonical
        integrity = connection.execute("PRAGMA integrity_check").fetchone()
        if not integrity or integrity[0] != "ok":
            raise RuntimeError(f"legacy Crypt database integrity failed: {integrity}")

        stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
        backup_dir = cache / "rename-backups" / stamp
        backup_dir.mkdir(parents=True, exist_ok=False)
        backup = backup_dir / legacy.name
        if not dual_names:
            with sqlite3.connect(backup) as destination:
                connection.backup(destination)

    if dual_names:
        os.replace(legacy, backup)
        for suffix in ("-wal", "-shm"):
            sidecar = Path(str(legacy) + suffix)
            if sidecar.exists():
                os.replace(sidecar, backup_dir / sidecar.name)
        log(f"retired dual-name Crypt database archived atomically -> {backup}")
        return canonical

    os.replace(legacy, canonical)
    for suffix in ("-wal", "-shm"):
        sidecar = Path(str(legacy) + suffix)
        if sidecar.exists():
            os.replace(sidecar, backup_dir / sidecar.name)
    log(f"crypt database renamed atomically; recovery backup -> {backup}")
    return canonical


def wait_for_tcp_port_closed(host: str, port: int, *, timeout_seconds: float = 5.0) -> None:
    """Wait for launchd's prior service process to release its listening port."""
    deadline = time.monotonic() + timeout_seconds
    while True:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
            probe.settimeout(0.2)
            if probe.connect_ex((host, port)) != 0:
                return
        if time.monotonic() >= deadline:
            raise RuntimeError(f"Crypt port {host}:{port} remained in use after launchd bootout")
        time.sleep(0.1)


def wait_for_tcp_port_open(host: str, port: int, *, timeout_seconds: float = 60.0) -> None:
    """Wait until the newly bootstrapped resident service owns its socket."""
    deadline = time.monotonic() + timeout_seconds
    while True:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
            probe.settimeout(0.2)
            if probe.connect_ex((host, port)) == 0:
                return
        if time.monotonic() >= deadline:
            raise RuntimeError(f"Crypt port {host}:{port} did not open after launchd bootstrap")
        time.sleep(0.1)


def render_crypt_shim(
    exe: str, db: str, ort: str, hf: str, verb: str, *, compat_prefix: str | None,
    crypt_port: int, cmd_format: bool, win: bool = False,
) -> str:
    """Render the crypt-backed verb branch of a shim (crypt/runc/skel/compress).

    ``cmd_format`` picks the output template (Windows .cmd vs POSIX /bin/sh, the latter
    written unconditionally so Git Bash on Windows can also resolve the shim). ``win``
    is the *host* OS regardless of template — the POSIX template still needs it to decide
    whether Git Bash's CRYPT_RUNC_SHELL default applies.
    """
    token = str(Path(db).parent / "api-token")
    if cmd_format:
        runc_env = (
            'if not defined CRYPT_RUNC_SHELL set "CRYPT_RUNC_SHELL=C:\\Progra~1\\Git\\bin\\bash.exe -c"\r\n'
            if verb == "runc" else ""
        )
        pre = 'runc --spill-dir "%CD%\\.cache\\runc" ' if verb == "runc" else f"{verb} " if verb else ""
        lines = ["@echo off\r\n"]
        if compat_prefix:
            lines.extend([
                f'if defined {compat_prefix}_DB if not defined CRYPT_DB set "CRYPT_DB=%{compat_prefix}_DB%"\r\n',
                f'if defined {compat_prefix}_PORT if not defined CRYPT_PORT set "CRYPT_PORT=%{compat_prefix}_PORT%"\r\n',
            ])
        lines.extend([
            f'if not defined CRYPT_DB set "CRYPT_DB={db}"\r\n',
            f'if not defined ORT_DYLIB_PATH set "ORT_DYLIB_PATH={ort}"\r\n',
            f'if not defined HF_HOME set "HF_HOME={hf}"\r\n',
            'if not defined HF_HUB_OFFLINE set "HF_HUB_OFFLINE=1"\r\n',
            f'if not defined CRYPT_PORT set "CRYPT_PORT={crypt_port}"\r\n',
            'if not defined WORKSPACE_MEMORY_PORT set "WORKSPACE_MEMORY_PORT=%CRYPT_PORT%"\r\n',
            f'if not defined CRYPT_API_TOKEN_FILE set "CRYPT_API_TOKEN_FILE={token}"\r\n',
            runc_env,
            f'"{exe}" {pre}%*\r\n',
        ])
        return "".join(lines)

    runc_env = (
        'export CRYPT_RUNC_SHELL="${CRYPT_RUNC_SHELL:-C:/Progra~1/Git/bin/bash.exe -c}"\n'
        if verb == "runc" and win else ""
    )
    pre = 'runc --spill-dir "$PWD/.cache/runc" ' if verb == "runc" else f"{verb} " if verb else ""
    db_value = f'${{CRYPT_DB:-${{{compat_prefix}_DB:-{db}}}}}' if compat_prefix else f'${{CRYPT_DB:-{db}}}'
    port_value = (
        f'${{CRYPT_PORT:-${{{compat_prefix}_PORT:-{crypt_port}}}}}' if compat_prefix
        else f'${{CRYPT_PORT:-{crypt_port}}}'
    )
    return (f'export CRYPT_DB="{db_value}"\n'
            f'export ORT_DYLIB_PATH="${{ORT_DYLIB_PATH:-{ort}}}"\n'
            f'export HF_HOME="${{HF_HOME:-{hf}}}"\n'
            f'export HF_HUB_OFFLINE="${{HF_HUB_OFFLINE:-1}}"\n'
            f'export CRYPT_PORT="{port_value}"\n'
            f'export WORKSPACE_MEMORY_PORT="${{WORKSPACE_MEMORY_PORT:-$CRYPT_PORT}}"\n'
            f'export CRYPT_API_TOKEN_FILE="${{CRYPT_API_TOKEN_FILE:-{token}}}"\n'
            f'{runc_env}'
            f'exec "{exe}" {pre}"$@"')
