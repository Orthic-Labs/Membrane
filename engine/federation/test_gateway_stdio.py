"""Resident-worker stdio protocol: readiness handshake, N requests → N lines,
bad requests never kill the worker (plan Task 2 + amendment A4)."""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

GATEWAY = Path(__file__).resolve().parent / "gateway.py"
sys.path.insert(0, str(GATEWAY.parents[1]))
from federation import gateway  # noqa: E402


def _run_stdio(lines: list[str], timeout: float = 60.0) -> list[dict]:
    process = subprocess.Popen(
        [sys.executable, str(GATEWAY), "--serve-stdio"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    stdout, _stderr = process.communicate("\n".join(lines) + "\n", timeout=timeout)
    assert process.returncode == 0, f"worker exited {process.returncode}"
    return [json.loads(line) for line in stdout.splitlines() if line.strip()]


def test_first_line_is_readiness_handshake_with_source_hash(tmp_path):
    payloads = _run_stdio([json.dumps({"task": "t", "repo": str(tmp_path)})])
    ready = payloads[0]["workerReady"]
    assert len(ready["gatewaySha256"]) == 64
    assert ready["pid"] > 0


def test_two_requests_yield_two_ccs_lines_and_blank_lines_are_ignored(tmp_path):
    request = json.dumps({"task": "stdio probe", "repo": str(tmp_path)})
    payloads = _run_stdio([request, "", request])
    envelopes = payloads[1:]
    assert len(envelopes) == 2
    for envelope in envelopes:
        assert envelope["schemaVersion"] == 1
        assert envelope["task"] == "stdio probe"
        assert isinstance(envelope["candidates"], list)
        assert "_membrane" in envelope


def test_malformed_request_yields_abort_envelope_and_worker_survives(tmp_path):
    good = json.dumps({"task": "after crash", "repo": str(tmp_path)})
    payloads = _run_stdio(["{not json", good])
    abort = payloads[1]["_membrane"]
    assert abort["abortReason"] == "worker_request_failed"
    assert payloads[2]["task"] == "after crash"


def test_missing_repo_field_aborts_that_request_only(tmp_path):
    good = json.dumps({"task": "still alive", "repo": str(tmp_path)})
    payloads = _run_stdio([json.dumps({"task": "no repo"}), good])
    assert payloads[1]["_membrane"]["abortReason"] == "worker_request_failed"
    assert payloads[2]["task"] == "still alive"


def test_argv_one_shot_mode_is_unchanged(tmp_path):
    result = subprocess.run(
        [sys.executable, str(GATEWAY), "--task", "one shot", "--repo", str(tmp_path)],
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert result.returncode == 0, result.stderr[:300]
    envelope = json.loads(result.stdout)
    assert envelope["task"] == "one shot"
    assert isinstance(envelope["candidates"], list)


def test_argv_forwards_exact_virtual_scope_descriptor_without_mcp(monkeypatch, tmp_path, capsys):
    seen = {}

    def assemble(**kwargs):
        seen.update(kwargs)
        return {"schemaVersion": 1, "task": kwargs["task"], "candidates": [], "_membrane": {}}, 0

    monkeypatch.setattr(gateway, "assemble_candidate_set", assemble)
    descriptor = {"kind": "virtual", "id": "thread:abc-123", "tenant_id": "tenant-a", "parents": [], "inherit_global": False}
    assert gateway.main([
        "--task", "direct transport", "--repo", str(tmp_path),
        "--scope-descriptor-json", json.dumps(descriptor),
    ]) == 0
    assert seen["scope_descriptor"] == descriptor
    assert json.loads(capsys.readouterr().out)["task"] == "direct transport"
