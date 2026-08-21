"""Static contract checks for isolated Membrane evaluation workspaces."""

import json
import importlib.util
import sys
from pathlib import Path

_SPEC = importlib.util.spec_from_file_location(
    "run_value_ab", Path(__file__).parents[1] / "eval/run_value_ab.py"
)
run_value_ab = importlib.util.module_from_spec(_SPEC)
assert _SPEC.loader is not None
sys.modules["run_value_ab"] = run_value_ab
_SPEC.loader.exec_module(run_value_ab)


class _FakeProcess:
    def __init__(self):
        self.returncode = 0
        self.kwargs = None

    def terminate(self):
        self.returncode = 0

    def wait(self, timeout=None):
        return 0

    def kill(self):
        self.returncode = -9


def test_eval_resident_binds_copy_and_requested_port(tmp_path, monkeypatch):
    source = tmp_path / "source.db"
    source.write_bytes(b"snapshot")
    binary = tmp_path / "membrane"
    process = _FakeProcess()

    def fake_popen(argv, **kwargs):
        process.argv = argv
        process.kwargs = kwargs
        return process

    monkeypatch.setattr(run_value_ab.subprocess, "Popen", fake_popen)
    started = run_value_ab._start_service(binary, source, 49321)
    workspace = Path(started._membrane_eval_workspace)
    runtime = json.loads((workspace / "tools/lib/memory/runtime.json").read_text())
    assert started is process
    assert process.argv == [str(workspace / "tools/bin" / binary.name), "supervisor-child"]
    assert process.kwargs["env"]["WORKSPACE_ROOT"] == str(workspace)
    assert runtime["port"] == 49321
    assert (workspace / "tools/.cache/memory/cortex-engine.db").read_bytes() == b"snapshot"
    run_value_ab._stop_service(started)
    assert not workspace.exists()
