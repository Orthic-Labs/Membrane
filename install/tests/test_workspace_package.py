"""Contract tests for source and generated workspace packages.

Run source & generated package tests with Python on Mac.
"""
from __future__ import annotations

import importlib.util
import json
import os
import sys
from pathlib import Path


PACKAGE_ROOT = Path(os.environ.get("MEMBRANE_WORKSPACE_TEST_ROOT", Path(__file__).parents[1] / "workspace")).resolve()


def _load(name: str):
    spec = importlib.util.spec_from_file_location(name, PACKAGE_ROOT / f"{name}.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    previous = sys.dont_write_bytecode
    sys.dont_write_bytecode = True
    try:
        spec.loader.exec_module(module)
    finally:
        sys.dont_write_bytecode = previous
    return module


def test_runtime_package_has_only_current_modules():
    assert (PACKAGE_ROOT / "membrane_shims.py").is_file()
    assert (PACKAGE_ROOT / "version_gate.py").is_file()
    assert not (PACKAGE_ROOT / ("crypt" + "_service.py")).exists()
    assert not (PACKAGE_ROOT / ("orthic" + "_manifest.py")).exists()
    assert not any(PACKAGE_ROOT.rglob("*.pyc"))
    assert not any(path.name.startswith("test_") for path in PACKAGE_ROOT.rglob("*.py"))


def test_membrane_push_shim_render_contract():
    module = _load("membrane_shims")
    body = module.render_membrane_shim(
        "/tools/bin/membrane", "runc",
    )
    assert 'exec "/tools/bin/membrane" cli push runc' in body
    assert "cortex" not in body


def test_cortex_shim_render_contract():
    module = _load("membrane_shims")
    body = module.render_cortex_shim(
        "/tools/cortex", "/db/cortex-engine.db", "/ort.dylib", "/hf",
    )
    assert 'MEMBRANE_CATALOG="${MEMBRANE_CATALOG:-/db/catalog.db}"' in body
    assert "cortex" in body


def test_version_gate_contract(tmp_path):
    module = _load("version_gate")
    blueprint = tmp_path / "blueprint"
    blueprint.mkdir()
    (blueprint / "package.json").write_text(json.dumps({"version": "0.2.5"}), encoding="utf-8")
    assert module.check_blueprint_version(blueprint) == (True, "ok", "0.2.5")
    assert module.check_blueprint_version(None) == (False, "blueprint_not_installed", None)
