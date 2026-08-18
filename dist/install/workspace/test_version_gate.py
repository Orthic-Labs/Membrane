import tempfile
import json
from pathlib import Path
from version_gate import check_cortex_version, parse_version

def make_cortex(tmp: Path, version: str | None):
    if version is None:
        return None
    d = tmp / f"cortex_{version or 'absent'}"
    d.mkdir(parents=True, exist_ok=True)
    (d / "package.json").write_text(json.dumps({"version": version}))
    return d

def test_below():
    with tempfile.TemporaryDirectory() as td:
        td = Path(td)
        r = make_cortex(td, "0.1.9")
        ok, code, ver = check_cortex_version(r)
        assert not ok and code == "cortex_version_incompatible", f"0.1.9 should fail, got {code} {ver}"

def test_in_range_low():
    with tempfile.TemporaryDirectory() as td:
        r = make_cortex(Path(td), "0.2.0")
        ok, code, ver = check_cortex_version(r)
        assert ok and code == "ok" and ver == "0.2.0"

def test_in_range_mid():
    with tempfile.TemporaryDirectory() as td:
        r = make_cortex(Path(td), "0.2.5")
        ok, code, _ = check_cortex_version(r)
        assert ok

def test_at_ceiling():
    with tempfile.TemporaryDirectory() as td:
        r = make_cortex(Path(td), "0.3.0")
        ok, code, _ = check_cortex_version(r)
        assert not ok and code == "cortex_version_incompatible"

def test_above():
    with tempfile.TemporaryDirectory() as td:
        r = make_cortex(Path(td), "0.3.1")
        ok, code, _ = check_cortex_version(r)
        assert not ok

def test_absent():
    ok, code, _ = check_cortex_version(None)
    assert not ok and code == "cortex_not_installed"
    with tempfile.TemporaryDirectory() as td:
        ok, code, _ = check_cortex_version(Path(td) / "nope")
        assert code == "cortex_not_installed"

if __name__ == "__main__":
    test_below(); test_in_range_low(); test_in_range_mid(); test_at_ceiling(); test_above(); test_absent()
    print("all version_gate tests passed")

