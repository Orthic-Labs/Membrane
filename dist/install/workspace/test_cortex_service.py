"""Tests for the Membrane-owned Cortex workspace-installation primitives.

Runs with the workspace's tools venv: `.venv-tools/bin/pytest membrane/install/workspace/`.
"""
from __future__ import annotations

from pathlib import Path

import cortex_service as cs


def test_render_cortex_shim_windows_cmd_format():
    body = cs.render_cortex_shim(
        "C:/tools/cortex.exe", "C:/db/cortex-engine.db", "C:/ort.dll", "C:/hf", "runc",
        cortex_port=47851, cmd_format=True, win=True,
    )
    assert body.startswith("@echo off\r\n")
    assert 'MEMBRANE_CATALOG=C:/db/catalog.db' in body
    assert "CORTEX_RUNC_SHELL" in body
    assert 'cortex.exe" runc --spill-dir "%CD%\\.cache\\runc" %*' in body


def test_render_cortex_shim_posix_format_on_windows_host_keeps_runc_shell():
    body = cs.render_cortex_shim(
        "/tools/cortex", "/db/cortex-engine.db", "/ort.so", "/hf", "runc",
        cortex_port=47851, cmd_format=False, win=True,
    )
    assert body.startswith("export CORTEX_DB=")
    assert 'MEMBRANE_CATALOG="${MEMBRANE_CATALOG:-/db/catalog.db}"' in body
    assert "CORTEX_RUNC_SHELL" in body
    assert 'exec "/tools/cortex" runc --spill-dir "$PWD/.cache/runc" "$@"' in body


def test_render_membrane_facade_shim_keeps_runc_contract():
    body = cs.render_cortex_shim(
        "/tools/membrane", "/db/cortex-engine.db", "/ort.so", "/hf", "cli runc",
        cortex_port=47851, cmd_format=False, win=False,
    )
    assert 'exec "/tools/membrane" cli runc --spill-dir "$PWD/.cache/runc" "$@"' in body


def test_render_cortex_shim_posix_format_on_posix_host_omits_runc_shell():
    body = cs.render_cortex_shim(
        "/tools/cortex", "/db/cortex-engine.db", "/ort.so", "/hf", "runc",
        cortex_port=47851, cmd_format=False, win=False,
    )
    assert "CORTEX_RUNC_SHELL" not in body


def test_workspace_installation_has_no_os_registration_path():
    workspace = Path(__file__).parent
    source = (workspace / "cortex_service.py").read_text(encoding="utf-8").lower()
    forbidden = ("launch" + "d", "system" + "d", "scht" + "asks")
    assert not any(token in source for token in forbidden)
    assert not (workspace / ("cortex_service_" + "launch" + "d.py")).exists()
    assert not (workspace / ("cortex_service_" + "registrars.py")).exists()
