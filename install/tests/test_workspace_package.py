"""Contract tests for source and generated workspace packages.

Run source package tests with ``PYTHONDONTWRITEBYTECODE=1 python3 -m pytest membrane/install/tests``.
Run generated-package tests with
``PYTHONDONTWRITEBYTECODE=1 MEMBRANE_WORKSPACE_TEST_ROOT=membrane/dist/install/workspace python3 -m pytest membrane/install/tests``.
"""
from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path


PACKAGE_ROOT = Path(os.environ.get("MEMBRANE_WORKSPACE_TEST_ROOT", Path(__file__).parents[1] / "workspace")).resolve()


def _load(name: str):
    spec = importlib.util.spec_from_file_location(name, PACKAGE_ROOT / f"{name}.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_runtime_package_has_only_current_modules():
    assert (PACKAGE_ROOT / "cortex_service.py").is_file()
    assert (PACKAGE_ROOT / "version_gate.py").is_file()
    assert not (PACKAGE_ROOT / ("crypt" + "_service.py")).exists()
    assert not (PACKAGE_ROOT / ("orthic" + "_manifest.py")).exists()
    assert not any(PACKAGE_ROOT.rglob("*.pyc"))
    assert not any(path.name.startswith("test_") for path in PACKAGE_ROOT.rglob("*.py"))


def test_cortex_service_render_contract():
    module = _load("cortex_service")
    body = module.render_cortex_shim(
        "C:/tools/cortex.exe", "C:/db/cortex-engine.db", "C:/ort.dll", "C:/hf", "runc",
        cortex_port=47851, cmd_format=True, win=True,
    )
    assert "MEMBRANE_CATALOG=C:/db/catalog.db" in body
    assert "cortex.exe" in body


def test_version_gate_contract(tmp_path):
    module = _load("version_gate")
    blueprint = tmp_path / "blueprint"
    blueprint.mkdir()
    (blueprint / "package.json").write_text(json.dumps({"version": "0.2.5"}), encoding="utf-8")
    assert module.check_blueprint_version(blueprint) == (True, "ok", "0.2.5")
    assert module.check_blueprint_version(None) == (False, "blueprint_not_installed", None)
