import json
import socket

import pytest

from federation.providers import blueprint


@pytest.fixture(autouse=True)
def _isolate_candidate_cache():
    blueprint._candidate_cache.clear()
    yield
    blueprint._candidate_cache.clear()


def test_repo_code_candidate_cap_is_bounded():
    assert blueprint.candidate_cap(2048, None) == 64
    assert blueprint.candidate_cap(4096, "32") == 32
    assert blueprint.candidate_cap(4096, "999") == 256
    assert blueprint.candidate_cap(4096, "invalid") == 64


def test_provider_uses_resident_daemon_and_generation_pin(monkeypatch, tmp_path):
    expected = "sha256:" + "1" * 64
    observed = {}
    candidate = {
        "id": "symbol:bounded",
        "sourceRef": "src/bounded_runtime.rs:10-20",
        "sourceHash": "xxh128:" + "2" * 32,
        "text": "bounded_runtime",
    }

    def daemon(repo_root, task, cap, generation):
        observed.update(repo_root=repo_root, task=task, cap=cap, generation=generation)
        return {
            "requestId": "ignored-by-mock",
            "ok": True,
            "generation": expected,
            "result": {
                "generationId": expected,
                "candidateSet": {"candidates": [candidate]},
                "recallCircuit": {"state": "complete"},
            },
        }

    monkeypatch.setattr(blueprint, "_read_daemon_recall", daemon)
    candidates, generation, warnings, observability = blueprint.produce_with_observability(
        tmp_path, "bounded runtime", 4096, expected_generation=expected
    )

    assert generation == expected
    assert warnings == []
    assert observed == {"repo_root": tmp_path, "task": "bounded runtime", "cap": 64, "generation": expected}
    assert candidates[0]["sourceHash"] == candidate["sourceHash"]
    assert observability["stageElapsedMs"]["blueprint_daemon"] >= 0


def test_daemon_abstention_is_typed(monkeypatch, tmp_path):
    expected = "sha256:" + "3" * 64
    monkeypatch.setattr(blueprint, "_read_daemon_recall", lambda *_args: {
        "ok": True,
        "generation": expected,
        "result": {
            "generationId": expected,
            "candidateSet": {"candidates": []},
            "recallCircuit": {"state": "abstained"},
        },
    })

    candidates, generation, warnings = blueprint.produce(
        tmp_path, "termthatdoesnotexist", 64, expected_generation=expected
    )

    assert candidates == []
    assert generation == expected
    assert [warning["kind"] for warning in warnings] == ["blueprint_abstained_no_relevant_seed"]


def test_timeout_is_typed_without_process_fallback(monkeypatch, tmp_path):
    def timeout(*_args):
        raise socket.timeout("deadline")

    monkeypatch.setattr(blueprint, "_read_daemon_recall", timeout)
    candidates, generation, warnings = blueprint.produce(tmp_path, "task", 4096)

    assert candidates == []
    assert generation == "blueprint-timeout"
    assert warnings[0]["kind"] == "provider_timeout"


def test_unavailable_daemon_is_typed(monkeypatch, tmp_path):
    monkeypatch.setattr(blueprint, "_read_daemon_recall", lambda *_args: (_ for _ in ()).throw(FileNotFoundError("socket missing")))
    candidates, generation, warnings = blueprint.produce(tmp_path, "task", 4096)

    assert candidates == []
    assert generation == "blueprint-unavailable"
    assert warnings[0]["kind"] == "blueprint_daemon_unavailable"


def test_exact_generation_cache_avoids_second_daemon_call(monkeypatch, tmp_path):
    expected = "sha256:" + "4" * 64
    calls = {"count": 0}

    def daemon(*_args):
        calls["count"] += 1
        return {
            "ok": True,
            "generation": expected,
            "result": {
                "generationId": expected,
                "candidateSet": {"candidates": [{"id": "x", "sourceHash": "sha256:" + "5" * 64}]},
                "recallCircuit": {"state": "complete"},
            },
        }

    monkeypatch.setattr(blueprint, "_read_daemon_recall", daemon)
    first = blueprint.produce(tmp_path, "task", 64, expected_generation=expected)
    second = blueprint.produce(tmp_path, "task", 64, expected_generation=expected)

    assert first == second
    assert calls["count"] == 1


def test_request_frame_binds_root_task_and_generation(monkeypatch, tmp_path):
    class FakeSocket:
        def __init__(self, *_args):
            self.sent = b""
        def __enter__(self): return self
        def __exit__(self, *_args): return False
        def settimeout(self, _value): pass
        def connect(self, endpoint): self.endpoint = endpoint
        def sendall(self, wire):
            self.sent = wire
            request = json.loads(wire)
            self.response = (json.dumps({"protocolVersion": 1, "requestId": request["requestId"], "ok": True, "generation": request["generation"], "result": {}}) + "\n").encode()
        def recv(self, _size):
            response, self.response = self.response, b""
            return response

    fake = FakeSocket()
    monkeypatch.setattr(blueprint.socket, "socket", lambda *_args: fake)
    monkeypatch.setattr(blueprint, "_daemon_endpoint", lambda: str(tmp_path / "blueprint.sock"))
    generation = "sha256:" + "6" * 64
    blueprint._read_daemon_recall(tmp_path, "task text", 12, generation)
    request = json.loads(fake.sent)

    assert request["method"] == "recall"
    assert request["generation"] == generation
    assert request["input"]["repoRoot"] == str(tmp_path.resolve())
    assert request["input"]["task"] == "task text"
    assert request["input"]["limit"] == 12


def test_manifest_digest_comes_from_resident_status(monkeypatch, tmp_path):
    digest = "sha256:" + "7" * 64
    observed = {}

    def daemon(repo_root, method, payload, generation, **_kwargs):
        observed.update(repo_root=repo_root, method=method, payload=payload, generation=generation)
        return {
            "ok": True,
            "result": {"manifest": {"generationId": "g", "manifestDigest": digest}},
        }

    monkeypatch.setattr(blueprint, "_read_daemon_request", daemon)
    assert blueprint.manifest_digest(tmp_path) == digest
    assert observed == {"repo_root": tmp_path, "method": "status", "payload": {}, "generation": None}
