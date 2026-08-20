import tempfile
import json
from pathlib import Path
from version_gate import check_blueprint_version, parse_version, resolve_blueprint_root

def make_blueprint(tmp: Path, version: str | None):
    if version is None:
        return None
    d = tmp / f"blueprint_{version or 'absent'}"
    d.mkdir(parents=True, exist_ok=True)
    (d / "package.json").write_text(json.dumps({"version": version}))
    return d

def test_below():
    with tempfile.TemporaryDirectory() as td:
        td = Path(td)
        r = make_blueprint(td, "0.1.9")
        ok, code, ver = check_blueprint_version(r)
        assert not ok and code == "blueprint_version_incompatible", f"0.1.9 should fail, got {code} {ver}"

def test_in_range_low():
    with tempfile.TemporaryDirectory() as td:
        r = make_blueprint(Path(td), "0.2.0")
        ok, code, ver = check_blueprint_version(r)
        assert ok and code == "ok" and ver == "0.2.0"

def test_in_range_mid():
    with tempfile.TemporaryDirectory() as td:
        r = make_blueprint(Path(td), "0.2.5")
        ok, code, _ = check_blueprint_version(r)
        assert ok

def test_at_ceiling():
    with tempfile.TemporaryDirectory() as td:
        r = make_blueprint(Path(td), "0.3.0")
        ok, code, _ = check_blueprint_version(r)
        assert not ok and code == "blueprint_version_incompatible"

def test_above():
    with tempfile.TemporaryDirectory() as td:
        r = make_blueprint(Path(td), "0.3.1")
        ok, code, _ = check_blueprint_version(r)
        assert not ok

def test_absent():
    ok, code, _ = check_blueprint_version(None)
    assert not ok and code == "blueprint_not_installed"
    with tempfile.TemporaryDirectory() as td:
        ok, code, _ = check_blueprint_version(Path(td) / "nope")
        assert code == "blueprint_not_installed"

def test_resolve_blueprint_root_uses_absorbed_layout_only():
    with tempfile.TemporaryDirectory() as td:
        workspace = Path(td)
        membrane = workspace / "membrane"
        absorbed = membrane / "blueprint"
        standalone = workspace / "blueprint"
        absorbed.mkdir(parents=True)
        standalone.mkdir()
        assert resolve_blueprint_root(membrane) == absorbed
        absorbed.rmdir()
        assert resolve_blueprint_root(membrane) is None

if __name__ == "__main__":
    test_below(); test_in_range_low(); test_in_range_mid(); test_at_ceiling(); test_above(); test_absent(); test_resolve_blueprint_root_uses_absorbed_layout_only()
    print("all version_gate tests passed")
