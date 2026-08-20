"""Membrane-owned workspace shim renderers.

Push reduction commands run through Membrane's signed CLI. Durable Cortex
commands keep their explicit local storage environment and binary boundary.
"""
from __future__ import annotations

from pathlib import Path


def log(msg: str) -> None:
    print(f"[membrane-workspace] {msg}")


def render_membrane_shim(exe: str, verb: str) -> str:
    """Render a Push shim backed by ``membrane cli push <verb>``."""
    if verb not in {"runc", "skel", "compress"}:
        raise ValueError(f"unsupported Push shim: {verb}")
    is_runc = verb == "runc"
    runc_env = 'export MEMBRANE_PUSH_RUNC_SHELL="${MEMBRANE_PUSH_RUNC_SHELL:-bash -c}"\n' if is_runc else ""
    spill = ' --spill-dir "$PWD/.cache/runc"' if is_runc else ""
    return f'{runc_env}exec "{exe}" cli push {verb}{spill} "$@"'


def render_cortex_shim(
    exe: str, db: str, ort: str, hf: str,
) -> str:
    """Render a durable Cortex shim with explicit storage/runtime paths."""
    catalog = str(Path(db).parent / "catalog.db")
    command = f'"{exe}"'

    return (
        f'export CORTEX_DB="${{CORTEX_DB:-{db}}}"\n'
        f'export MEMBRANE_CATALOG="${{MEMBRANE_CATALOG:-{catalog}}}"\n'
        f'export ORT_DYLIB_PATH="${{ORT_DYLIB_PATH:-{ort}}}"\n'
        f'export HF_HOME="${{HF_HOME:-{hf}}}"\n'
        'export HF_HUB_OFFLINE="${HF_HUB_OFFLINE:-1}"\n'
        f'exec {command} "$@"'
    )
