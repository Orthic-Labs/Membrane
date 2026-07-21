from __future__ import annotations

import importlib.util
import json
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("adapt_llm.py")


def _module():
    spec = importlib.util.spec_from_file_location("adapt_llm_transport_test", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class _Response:
    def __enter__(self):
        return self

    def __exit__(self, *_args):
        return False

    def read(self):
        return json.dumps({
            "model": "MiniMax-M3",
            "content": [{"type": "thinking", "thinking": "hidden"},
                        {"type": "text", "text": "[]"}],
            "stop_reason": "end_turn",
            "stop_sequence": None,
            "usage": {"input_tokens": 10, "output_tokens": 2},
        }).encode("utf-8")


def test_minimax_lane_uses_local_mm_proxy_and_preserves_metadata(monkeypatch):
    module = _module()
    captured = {}

    def urlopen(request, timeout):
        captured["url"] = request.full_url
        captured["headers"] = dict(request.header_items())
        captured["payload"] = json.loads(request.data.decode("utf-8"))
        captured["timeout"] = timeout
        return _Response()

    monkeypatch.setattr(module.urllib.request, "urlopen", urlopen)
    result = module._minimax_response("system", "user", attempts=1)

    assert captured["url"] == "http://127.0.0.1:8801/v1/messages"
    assert captured["headers"]["X-api-key"] == "router-dummy"
    assert captured["payload"]["model"] == "claude-opus-4-8"
    assert captured["payload"]["thinking"] == {"type": "adaptive"}
    assert result == {
        "text": "[]",
        "model": "MiniMax-M3",
        "stop_reason": "end_turn",
        "stop_sequence": None,
        "usage": {"input_tokens": 10, "output_tokens": 2},
    }


def test_default_minimax_extract_and_synthesis_keep_transient_retries(monkeypatch):
    module = _module()
    attempts = []

    def fake_call(_system, _user, **kwargs):
        attempts.append(kwargs["attempts"])
        return {"text": "[]"}

    monkeypatch.setattr(module, "_minimax_response", fake_call)
    assert module._default_llm("system", "user", "minimax") == "[]"
    assert module._default_synth_llm("system", "user", "minimax") == "[]"
    assert attempts == [3, 3]
