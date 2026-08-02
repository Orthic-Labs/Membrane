from __future__ import annotations

import hashlib
import json
import os
import socket
import sqlite3
import subprocess
import time
import urllib.request
from pathlib import Path

import pytest

import morph_event_learning as learning


def _event(text: str, *, origin: str = "user", event_type: str = "user_correction") -> dict:
    return {
        "schema": "orthic.observable-event.v1",
        "installation_id": "install-e2e",
        "client_id": "codex",
        "session_id": "session-e2e",
        "task_id": "task-e2e",
        "turn_id": "turn-e2e",
        "trace_id": "trace-e2e",
        "event_id": "event-e2e",
        "event_type": event_type,
        "origin": origin,
        "content_ref_or_digest": "sha256:" + hashlib.sha256(text.encode()).hexdigest(),
        "timestamp": "2026-08-02T00:00:00Z",
        "completeness": {"input": True},
        "policy_snapshot_digest": "sha256:" + "b" * 64,
    }


def test_only_hash_bound_user_events_can_enter_admission() -> None:
    rule = "Always use JSONL for Morph pipeline logs."
    admitted = learning.admit_user_event(
        _event(rule), evidence_text=rule, scope="Volumes-D-claude", category="tooling"
    )
    assert admitted.rule_key.startswith("Volumes-D-claude/adapt-tooling-")
    assert admitted.record.source_ids == ("install:install-e2e:codex:event-e2e",)
    with pytest.raises(learning.MorphLearningError, match="origin-not-user"):
        learning.admit_user_event(
            _event(rule, origin="assistant"),
            evidence_text=rule,
            scope="Volumes-D-claude",
            category="tooling",
        )
    with pytest.raises(learning.MorphLearningError, match="digest-mismatch"):
        learning.admit_user_event(
            _event(rule),
            evidence_text="Always use YAML for Morph pipeline logs.",
            scope="Volumes-D-claude",
            category="tooling",
        )


def _free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def _run(binary: Path, db: Path, port: int, args: list[str]) -> str:
    env = {**os.environ, "MEMRIGHT_PORT": str(port), "WORKSPACE_MEMORY_PORT": str(port)}
    result = subprocess.run(
        [str(binary), "--db", str(db), *args],
        env=env,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip())
    return result.stdout


def _recall(port: int, token: str, query: str, scope: str) -> str:
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/recall",
        data=json.dumps({
            "query": query,
            "k": 6,
            "scope": scope,
            "client": "morph-e2e",
            "session": "next-task-e2e",
        }).encode("utf-8"),
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {token}",
        },
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=120) as response:
        return response.read().decode("utf-8")


@pytest.mark.skipif(os.environ.get("MORPH_E2E") != "1", reason="run by C14 qualifier")
def test_real_persistence_readback_and_next_use(tmp_path: Path, monkeypatch) -> None:
    binary = Path(os.environ["MEMRIGHT_BIN"]).resolve()
    db = tmp_path / "morph.db"
    live_db = Path(
        os.environ.get(
            "MEMRIGHT_LIVE_DB",
            Path(__file__).resolve().parent.parent / "tools/.cache/memory/memright-engine.db",
        )
    ).resolve()
    with sqlite3.connect(f"file:{live_db.as_posix()}?mode=ro", uri=True) as source:
        with sqlite3.connect(db) as destination:
            source.backup(destination)
    token_file = tmp_path / "api-token"
    token = "morph-e2e-token-0123456789abcdef"
    token_file.write_text(token, encoding="utf-8")
    port = _free_port()
    workspace_root = Path(__file__).resolve().parent.parent
    monkeypatch.setenv("WORKSPACE_ROOT", str(workspace_root))
    monkeypatch.setenv("CONTEXT_HOME", str(tmp_path))
    monkeypatch.setenv("MEMRIGHT_API_TOKEN_FILE", str(token_file))
    monkeypatch.setenv("MEMRIGHT_ALLOW_HASH", "1")
    monkeypatch.setenv("MEMRIGHT_PORT", str(port))
    monkeypatch.setenv("WORKSPACE_MEMORY_PORT", str(port))
    env = {
        **os.environ,
    }
    service = subprocess.Popen(
        [str(binary), "--db", str(db), "serve", "--port", str(port)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            with socket.socket() as probe:
                if probe.connect_ex(("127.0.0.1", port)) == 0:
                    break
            time.sleep(0.05)
        else:
            stderr = service.stderr.read().strip() if service.stderr else ""
            raise RuntimeError(f"isolated MemRight service did not start: {stderr}")
        rule = "Always use JSONL for Morph parity event-e2e logs."
        admitted = learning.admit_user_event(
            _event(rule), evidence_text=rule, scope="Volumes-D-claude", category="tooling"
        )
        receipt = learning.persist_learning(
            admitted,
            token_file=token_file,
            base_url=f"http://127.0.0.1:{port}",
            installation_id="install-e2e",
        )
        assert receipt["inserted"] == 1
        proof = learning.verify_next_use(
            admitted,
            get_memory=lambda memory_id: _run(binary, db, port, ["get", memory_id]),
            recall=lambda query, scope: _recall(port, token, query, scope),
        )
        assert proof["delivered"] is True
        assert proof["event_id"] == "event-e2e"
    finally:
        service.terminate()
        try:
            service.wait(timeout=5)
        except subprocess.TimeoutExpired:
            service.kill()
            service.wait(timeout=5)
