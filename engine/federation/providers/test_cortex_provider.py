"""Cortex provider: the binary defaults to the resolvable workspace exe, not bare `cortex`
(bare is unresolvable in the gateway's Python subprocess on Windows -> 0 memory candidates)."""
import importlib.util
import json
import os
from pathlib import Path

import pytest

_HERE = Path(__file__).resolve().parent


def _load():
    spec = importlib.util.spec_from_file_location("cortex_provider_mod", _HERE / "cortex.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_default_bin_resolves_workspace_binary_when_present():
    mod = _load()
    os.environ.pop("CORTEX_BIN", None)
    workspace = Path(__file__).resolve().parents[4]
    exe = workspace / "tools" / "bin" / ("cortex.exe" if os.name == "nt" else "cortex")
    resolved = mod._default_bin()
    if exe.exists():
        assert resolved == str(exe), "must resolve the real workspace binary, not bare 'cortex'"
    else:
        assert resolved == "cortex"  # graceful fallback in a bare CI checkout


def test_explicit_env_wins():
    mod = _load()
    os.environ["CORTEX_BIN"] = "/custom/cortex"
    try:
        assert mod._default_bin() == "/custom/cortex"
    finally:
        os.environ.pop("CORTEX_BIN", None)


def test_produce_with_observability_allowlists_content_free_stage_timing(monkeypatch, tmp_path):
    mod = _load()
    secret_task = "sensitive provider task"
    monkeypatch.setattr(
        mod,
        "_ccs_from_serve",
        lambda *_args: {
            "task": secret_task,
            "candidates": [],
            "_membrane": {
                "stageElapsedMs": {
                    "request_parse": 1.25,
                    "embed": 2,
                    "recall": 3.5,
                    "rank": 4,
                    "query": secret_task,
                    "negative": -1,
                    "not_numeric": "5",
                }
            },
        },
    )

    candidates, source, observability = mod.produce_with_observability(
        tmp_path, secret_task, None
    )

    assert candidates == []
    assert source == "cortex-serve"
    assert observability == {
        "stageElapsedMs": {
            "request_parse": 1.25,
            "embed": 2.0,
            "recall": 3.5,
            "rank": 4.0,
        }
    }
    assert secret_task not in str(observability)


def test_replay_uses_observer_timeout_and_preserves_serve_timing(monkeypatch, tmp_path):
    mod = _load()
    seen = []

    class Response:
        def __enter__(self):
            return self

        def __exit__(self, *_args):
            return False

        def read(self):
            return json.dumps({
                "candidates": [],
                "_membrane": {
                    "stageElapsedMs": {
                        "request_parse": 0.1,
                        "embed": 12.5,
                        "recall": 3.0,
                        "rank": 0.4,
                    }
                },
            }).encode("utf-8")

    def fake_urlopen(_request, *, timeout):
        seen.append(timeout)
        return Response()

    def forbid_legacy(*_args, **_kwargs):
        raise AssertionError("replay must not invoke the legacy CLI fallback")

    monkeypatch.setenv("MEMBRANE_SAMPLE_SOURCE", "replay")
    monkeypatch.setenv("REPLAY_NO_LEGACY", "1")
    monkeypatch.setattr(mod.urllib.request, "urlopen", fake_urlopen)
    monkeypatch.setattr(mod.subprocess, "run", forbid_legacy)

    candidates, source, observability = mod.produce_with_observability(
        tmp_path, "observer replay", None
    )

    assert seen == [45.0]
    assert candidates == []
    assert source == "cortex-serve"
    assert observability == {
        "stageElapsedMs": {
            "request_parse": 0.1,
            "embed": 12.5,
            "recall": 3.0,
            "rank": 0.4,
        }
    }

    monkeypatch.setenv("MEMBRANE_SAMPLE_SOURCE", "real")
    assert mod._ccs_from_serve("production", str(tmp_path), 64) is not None
    assert seen == [45.0, 0.35]


def test_serve_forwards_exact_virtual_scope_descriptor(monkeypatch, tmp_path):
    mod = _load()
    seen = {}

    class Response:
        def __enter__(self):
            return self

        def __exit__(self, *_args):
            return False

        def read(self):
            return b'{"candidates": []}'

    def fake_urlopen(request, *, timeout):
        seen["payload"] = json.loads(request.data.decode("utf-8"))
        seen["timeout"] = timeout
        return Response()

    monkeypatch.setattr(mod.urllib.request, "urlopen", fake_urlopen)
    descriptor = {"kind": "virtual", "id": "thread:abc-123", "tenant_id": "tenant-a", "parents": [], "inherit_global": False}
    assert mod._ccs_from_serve("typed request", str(tmp_path), 64, descriptor) == {"candidates": []}
    assert seen["payload"]["scopeDescriptor"] == descriptor


def test_replay_no_legacy_rejects_serve_failure_without_cli_fallback(
    monkeypatch, tmp_path
):
    mod = _load()

    def forbid_legacy(*_args, **_kwargs):
        raise AssertionError("replay must not invoke the legacy CLI fallback")

    monkeypatch.setenv("MEMBRANE_SAMPLE_SOURCE", "replay")
    monkeypatch.setenv("REPLAY_NO_LEGACY", "1")
    monkeypatch.setattr(mod, "_ccs_from_serve", lambda *_args, **_kwargs: None)
    monkeypatch.setattr(mod.subprocess, "run", forbid_legacy)

    with pytest.raises(
        RuntimeError,
        match="replay cortex serve unavailable; CLI fallback disabled",
    ):
        mod.produce_with_observability(tmp_path, "observer replay", None)


def test_real_serve_failure_rejects_without_prompt_path_cli_fallback(monkeypatch, tmp_path):
    mod = _load()
    def forbid_cli(*_args, **_kwargs):
        raise AssertionError("prompt provider must not cold-load the CLI")

    monkeypatch.setenv("MEMBRANE_SAMPLE_SOURCE", "real")
    monkeypatch.setenv("REPLAY_NO_LEGACY", "1")
    monkeypatch.setattr(mod, "_ccs_from_serve", lambda *_args, **_kwargs: None)
    monkeypatch.setattr(mod.subprocess, "run", forbid_cli)

    with pytest.raises(RuntimeError, match="prompt-path CLI fallback disabled"):
        mod.produce_with_observability(tmp_path, "production request", None)
