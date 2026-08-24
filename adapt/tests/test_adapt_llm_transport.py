from __future__ import annotations

import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace


MODULE_PATH = Path(__file__).resolve().parents[1] / "src" / "adapt" / "adapt_llm.py"


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
    # The gateway routes by substring against its slot keys, so this alias is what
    # picks the provider. "sonnet" is the slot bound to minimax:MiniMax-M3. Assert
    # the routing intent too: an alias containing "opus" lands on glm and one
    # containing "fable" lands on qwen, and both failures look like MiniMax being
    # down rather than a misroute.
    assert captured["payload"]["model"] == "claude-sonnet-4-5"
    assert "opus" not in captured["payload"]["model"]
    assert "fable" not in captured["payload"]["model"]
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


def test_opencode_lane_uses_ox_alpha_json_events(monkeypatch):
    module = _module()
    captured = {}
    events = "\n".join([
        json.dumps({"type": "text", "part": {"text": "[]"}}),
        json.dumps({"type": "step_finish", "part": {
            "reason": "stop", "tokens": {"input": 12, "output": 2},
        }}),
    ])

    monkeypatch.setattr(module.shutil, "which", lambda _name: "/bin/opencode")

    def capture_opencode_run(argv, **kwargs):
        captured["argv"] = argv
        captured["kwargs"] = kwargs
        return SimpleNamespace(returncode=0, stdout=events, stderr="")

    monkeypatch.setattr(module.subprocess, "run", capture_opencode_run)
    result = module._opencode_response("system", "user", attempts=1)

    assert captured["argv"][0:2] == ["/bin/opencode", "run"]
    assert "--pure" in captured["argv"]
    assert captured["argv"][captured["argv"].index("--model") + 1] == (
        "opencode-go/ox-alpha-free"
    )
    assert "shell" not in captured["kwargs"]
    assert captured["kwargs"]["check"] is False
    assert result == {
        "text": "[]",
        "model": "opencode-go/ox-alpha-free",
        "stop_reason": "stop",
        "stop_sequence": None,
        "usage": {"input": 12, "output": 2},
    }


def test_opencode_lane_availability_requires_exact_catalog_entry(monkeypatch):
    module = _module()
    monkeypatch.setattr(module.shutil, "which", lambda _name: "/bin/opencode")
    monkeypatch.setattr(
        module.subprocess,
        "run",
        lambda *_args, **_kwargs: SimpleNamespace(
            returncode=0, stdout="opencode-go/ox-alpha-free\n", stderr=""
        ),
    )
    assert module.lane_available("opencode") is True


def test_pi_lane_uses_sessionless_tool_free_ox_alpha(monkeypatch):
    module = _module()
    captured = {}
    events = "\n".join([
        json.dumps({"type": "message_end", "message": {
            "role": "assistant", "provider": "opencode-go", "model": "ox-alpha-free",
            "stopReason": "stop", "usage": {"input": 12, "output": 2},
            "content": [
                {"type": "thinking", "thinking": "hidden"},
                {"type": "text", "text": "[]"},
            ],
        }}),
    ])
    monkeypatch.setattr(module.shutil, "which", lambda _name: "/bin/pi")

    def capture_pi_run(argv, **kwargs):
        captured["argv"] = argv
        captured["kwargs"] = kwargs
        return SimpleNamespace(returncode=0, stdout=events, stderr="")

    monkeypatch.setattr(module.subprocess, "run", capture_pi_run)
    result = module._pi_response("system", "user", attempts=1)

    assert captured["argv"][0] == "/bin/pi"
    assert "--no-session" in captured["argv"]
    assert "--no-tools" in captured["argv"]
    assert captured["argv"][captured["argv"].index("--provider") + 1] == "opencode-go"
    assert captured["argv"][captured["argv"].index("--model") + 1] == "ox-alpha-free"
    assert captured["kwargs"]["check"] is False
    assert result == {
        "text": "[]",
        "model": "opencode-go/ox-alpha-free",
        "stop_reason": "stop",
        "stop_sequence": None,
        "usage": {"input": 12, "output": 2},
    }


def test_parse_json_array_contains_python_replace_damage_to_one_value():
    module = _module()
    raw = (
        '[{"category":"workflow","observation":"Keep status concise",'
        '"durability":"cross_task_cross_explicit".replace("_cross_","_task_")},'
        '{"category":"verification","observation":"Run tests"}]'
    )

    parsed = module.parse_json_array(raw, "extract")

    assert len(parsed) == 2
    assert parsed[0]["durability"] == "cross_task_cross_explicit"
    assert parsed[1]["observation"] == "Run tests"
