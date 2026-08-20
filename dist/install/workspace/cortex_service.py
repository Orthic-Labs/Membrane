"""Membrane-owned Cortex workspace-installation primitives. Pure stdlib only.

Ported out of Adrian's private workspace script (tools/setup-workspace.py) so Membrane
carries zero external-repo dependencies. Every path this module renders into an on-disk
artifact is derived from an explicit ``repo``/``home``/``port`` argument the caller
supplies (from WORKSPACE_ROOT / MEMBRANE_WORKSPACE_ROOT or its own config) — never a
hardcoded machine path or username.

"""
from __future__ import annotations

from pathlib import Path


def log(msg: str) -> None:
    print(f"[membrane-workspace] {msg}")


def render_cortex_shim(
    exe: str, db: str, ort: str, hf: str, verb: str, *, cortex_port: int,
    cmd_format: bool, win: bool = False,
) -> str:
    """Render the cortex-backed verb branch of a shim (cortex/runc/skel/compress).

    ``cmd_format`` picks the output template (Windows .cmd vs POSIX /bin/sh, the latter
    written unconditionally so Git Bash on Windows can also resolve the shim). ``win``
    is the *host* OS regardless of template — the POSIX template still needs it to decide
    whether Git Bash's CORTEX_RUNC_SHELL default applies.
    """
    token = str(Path(db).parent / "api-token")
    catalog = str(Path(db).parent / "catalog.db")
    if cmd_format:
        is_runc = verb.split()[-1:] == ["runc"]
        runc_env = (
            'if not defined CORTEX_RUNC_SHELL set "CORTEX_RUNC_SHELL=C:\\Progra~1\\Git\\bin\\bash.exe -c"\r\n'
            if is_runc else ""
        )
        pre = f'{verb} --spill-dir "%CD%\\.cache\\runc" ' if is_runc else f"{verb} " if verb else ""
        lines = ["@echo off\r\n"]
        lines.extend([
            f'if not defined CORTEX_DB set "CORTEX_DB={db}"\r\n',
            f'if not defined MEMBRANE_CATALOG set "MEMBRANE_CATALOG={catalog}"\r\n',
            f'if not defined ORT_DYLIB_PATH set "ORT_DYLIB_PATH={ort}"\r\n',
            f'if not defined HF_HOME set "HF_HOME={hf}"\r\n',
            'if not defined HF_HUB_OFFLINE set "HF_HUB_OFFLINE=1"\r\n',
            f'if not defined CORTEX_PORT set "CORTEX_PORT={cortex_port}"\r\n',
            f'if not defined CORTEX_API_TOKEN_FILE set "CORTEX_API_TOKEN_FILE={token}"\r\n',
            runc_env,
            f'"{exe}" {pre}%*\r\n',
        ])
        return "".join(lines)

    is_runc = verb.split()[-1:] == ["runc"]
    runc_env = (
        'export CORTEX_RUNC_SHELL="${CORTEX_RUNC_SHELL:-C:/Progra~1/Git/bin/bash.exe -c}"\n'
        if is_runc and win else ""
    )
    pre = f'{verb} --spill-dir "$PWD/.cache/runc" ' if is_runc else f"{verb} " if verb else ""
    return (f'export CORTEX_DB="${{CORTEX_DB:-{db}}}"\n'
            f'export MEMBRANE_CATALOG="${{MEMBRANE_CATALOG:-{catalog}}}"\n'
            f'export ORT_DYLIB_PATH="${{ORT_DYLIB_PATH:-{ort}}}"\n'
            f'export HF_HOME="${{HF_HOME:-{hf}}}"\n'
            f'export HF_HUB_OFFLINE="${{HF_HUB_OFFLINE:-1}}"\n'
            f'export CORTEX_PORT="${{CORTEX_PORT:-{cortex_port}}}"\n'
            f'export CORTEX_API_TOKEN_FILE="${{CORTEX_API_TOKEN_FILE:-{token}}}"\n'
            f'{runc_env}'
            f'exec "{exe}" {pre}"$@"')
