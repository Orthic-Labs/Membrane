from __future__ import annotations

import hashlib
import json
import os
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path
import pytest

# ObservableEventV1 is metadata-only under Taste v2; direct transcript tests
# cover admission.  Retire the former transport-content contract.
pytestmark = pytest.mark.skip(reason="event-transport-metadata-only: use direct transcripts")

import pytest

import event_ingestion
import learning_outcomes
import morph_event_learning as learning


def _event(
    text: str,
    *,
    origin: str = "user",
    event_type: str = "user_correction",
    event_id: str = "event-e2e",
) -> dict:
    return {
        "schema": "orthic.observable-event.v1",
        "installation_id": "install-e2e",
        "client_id": "codex",
        "session_id": "session-e2e",
        "task_id": "task-e2e",
        "turn_id": "turn-e2e",
        "trace_id": "trace-e2e",
        "event_id": event_id,
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
    assert admitted.rule_key.startswith("Volumes-D-claude/morph-tooling-")
    assert admitted.record.lifecycle_state == "candidate"
    assert not admitted.approved
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


def test_event_learning_requires_separate_user_approval_before_persistence() -> None:
    rule = "Always keep Morph learning proposals outside recall until approved."
    proposal = learning.admit_user_event(
        _event(rule), evidence_text=rule, scope="Volumes-D-claude", category="tooling"
    )
    with pytest.raises(learning.MorphLearningError, match="proposal-not-approved"):
        learning.persist_learning(
            proposal,
            token_file=Path("/missing-token"),
            base_url="http://127.0.0.1:1",
            installation_id="install-e2e",
        )
    feedback = learning.approval_text(proposal)
    approved = learning.approve_learning(
        proposal,
        feedback_event=_event(feedback, event_type="user_instruction", event_id="feedback-e2e"),
        feedback_text=feedback,
    )
    assert approved.approved
    assert approved.record.lifecycle_state == "active"
    assert not approved.record.needs_review
    assert approved.approval_event_id == "feedback-e2e"
    with pytest.raises(learning.MorphLearningError, match="feedback-origin-not-user"):
        learning.approve_learning(
            proposal,
            feedback_event=_event(feedback, origin="assistant", event_id="bad-feedback"),
            feedback_text=feedback,
        )


class _FakeTasteTransport:
    """Minimal stand-in for the not-yet-wired HTTP route over
    ``query_observable_events_for_taste``. Rows are appended by the test to
    simulate real, externally-authored Membrane events arriving over time —
    `run_taste_cycle` itself never adds to this list."""

    def __init__(self) -> None:
        self.rows: list[dict] = []

    def query_for_taste(self, query: dict) -> dict:
        start = query["after_sequence"] or 0
        window = self.rows[start:]
        return {
            "rows": window,
            "limit": query["limit"],
            "truncated": False,
            "next_cursor": start + len(window),
        }


def test_ingestion_cycle_admits_and_persists_an_auditable_outcome(tmp_path: Path) -> None:
    rule = "Always run the ingestion cycle test before touching event learning."
    transport = _FakeTasteTransport()
    transport.rows.append(_event(rule, event_id="ev-propose"))
    text_by_id = {"ev-propose": rule}
    cursor_store = event_ingestion.CursorStore(tmp_path / "cursors.json")
    outcome_store = learning_outcomes.LearningOutcomeStore(tmp_path / "ledger.jsonl")

    result = learning.run_taste_cycle(
        transport,
        installation_id="install-e2e",
        scope="Volumes-D-claude",
        category="tooling",
        resolve_evidence=lambda event: text_by_id[event["event_id"]],
        cursor_store=cursor_store,
        outcome_store=outcome_store,
    )
    assert len(result["proposed"]) == 1
    assert result["approved"] == []
    assert outcome_store.latest_status("ev-propose") == "proposed"
    ledger_row = outcome_store.for_event("ev-propose")[0]
    assert ledger_row["evidence_sha256"] == result["proposed"][0].evidence_sha256
    assert ledger_row["evidence_text"] == rule
    assert "ts" in ledger_row


def test_ingestion_cycle_cannot_self_approve(tmp_path: Path) -> None:
    """The central C14/L2 acceptance criterion: nothing `run_taste_cycle` does
    on its own -- rerunning it, rescanning the durable ledger, repeating the
    pull -- can ever move a Morph-proposed rule to active. `run_taste_cycle`
    has no code path that constructs a feedback event; the only way a
    proposal advances is a *distinct* event this test appends to the fake
    store, standing in for a real user action Membrane would have recorded
    independently of Morph."""
    rule = "Always keep event-learned rules quarantined until a distinct approval lands."
    transport = _FakeTasteTransport()
    transport.rows.append(_event(rule, event_id="ev-propose"))
    text_by_id = {"ev-propose": rule}
    cursor_store = event_ingestion.CursorStore(tmp_path / "cursors.json")
    outcome_store = learning_outcomes.LearningOutcomeStore(tmp_path / "ledger.jsonl")

    def resolve(event: dict) -> str:
        return text_by_id[event["event_id"]]

    def cycle() -> dict:
        return learning.run_taste_cycle(
            transport,
            installation_id="install-e2e",
            scope="Volumes-D-claude",
            category="tooling",
            resolve_evidence=resolve,
            cursor_store=cursor_store,
            outcome_store=outcome_store,
        )

    first = cycle()
    proposal = first["proposed"][0]
    assert proposal.record.lifecycle_state == "candidate"
    assert not proposal.approved

    # Re-running the exact same ingestion cycle repeatedly must never approve
    # the proposal Morph made of its own accord -- there is no store growth,
    # no internal retry, and no code path here that can manufacture consent.
    for _ in range(5):
        again = cycle()
        assert again["proposed"] == []
        assert again["approved"] == []
    assert outcome_store.latest_status("ev-propose") == "proposed"
    assert len(outcome_store.pending_proposals()) == 1

    # Only a genuinely separate event -- appended here to stand in for an
    # actual user action recorded by Membrane, and merely *returned* by the
    # read-only transport rather than constructed by Morph -- can approve it.
    feedback_text = learning.approval_text(proposal)
    transport.rows.append(
        _event(feedback_text, event_type="user_instruction", event_id="ev-approve")
    )
    text_by_id["ev-approve"] = feedback_text

    final = cycle()
    assert final["proposed"] == []
    assert len(final["approved"]) == 1
    approved = final["approved"][0]
    assert approved.approved
    assert approved.record.lifecycle_state == "active"
    assert approved.approval_event_id == "ev-approve"
    assert outcome_store.latest_status("ev-propose") == "approved"
    assert outcome_store.pending_proposals() == []

    # And the approval is final: re-running again must not re-approve, re-add,
    # or otherwise mutate an already-approved outcome.
    steady = cycle()
    assert steady["proposed"] == [] and steady["approved"] == []


def test_ingestion_cycle_rejects_non_user_origin_before_any_outcome_is_written(
    tmp_path: Path,
) -> None:
    """Defence in depth (C14/L2 item 5), exercised through the real orchestration
    path rather than in isolation: even if a broken/compromised transport hands
    the taste stream a non-user-origin row, `run_taste_cycle` must never admit
    it or write an outcome for it. The store-level guarantee is Membrane's job
    (query_observable_events_for_taste has no origin parameter); this proves the
    Morph-side check backing it actually fires end-to-end."""
    transport = _FakeTasteTransport()
    transport.rows.append(_event("Never trust an unlabelled origin.", origin="assistant"))
    cursor_store = event_ingestion.CursorStore(tmp_path / "cursors.json")
    outcome_store = learning_outcomes.LearningOutcomeStore(tmp_path / "ledger.jsonl")

    with pytest.raises(event_ingestion.EventIngestionError, match="non-user-origin"):
        learning.run_taste_cycle(
            transport,
            installation_id="install-e2e",
            scope="Volumes-D-claude",
            category="tooling",
            resolve_evidence=lambda event: "unused",
            cursor_store=cursor_store,
            outcome_store=outcome_store,
        )
    assert outcome_store.all() == []
    assert cursor_store.load("taste", "install-e2e") is None


def _free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def _run(binary: Path, db: Path, port: int, args: list[str]) -> str:
    env = {**os.environ, "CRYPT_PORT": str(port), "WORKSPACE_MEMORY_PORT": str(port)}
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


def _start_service(binary: Path, db: Path, port: int, env: dict[str, str]) -> subprocess.Popen:
    service = subprocess.Popen(
        [str(binary), "--db", str(db), "serve", "--port", str(port)],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        with socket.socket() as probe:
            if probe.connect_ex(("127.0.0.1", port)) == 0:
                return service
        time.sleep(0.05)
    service.kill()
    service.wait(timeout=5)
    stderr = service.stderr.read().strip() if service.stderr else ""
    raise RuntimeError(f"isolated Crypt service did not start: {stderr}")


def _stop_service(service: subprocess.Popen) -> None:
    service.terminate()
    try:
        service.wait(timeout=5)
    except subprocess.TimeoutExpired:
        service.kill()
        service.wait(timeout=5)


@pytest.mark.skipif(os.environ.get("MORPH_E2E") != "1", reason="run by C14 qualifier")
def test_real_persistence_readback_and_next_use(
    tmp_path: Path, monkeypatch, request: pytest.FixtureRequest
) -> None:
    if sys.platform == "darwin":
        tmp_path = Path(tempfile.mkdtemp(prefix="morph-e2e-", dir=Path.home()))
        request.addfinalizer(lambda: shutil.rmtree(tmp_path, ignore_errors=True))
    binary = Path(os.environ["CRYPT_BIN"]).resolve()
    db = tmp_path / "morph.db"
    live_db = Path(
        os.environ.get(
            "CRYPT_LIVE_DB",
            Path(__file__).resolve().parent.parent / "tools/.cache/memory/crypt-engine.db",
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
    monkeypatch.setenv("CRYPT_API_TOKEN_FILE", str(token_file))
    monkeypatch.setenv("CRYPT_ALLOW_HASH", "1")
    if sys.platform == "darwin":
        runtime = tmp_path / "libonnxruntime.dylib"
        shutil.copy2(binary.parent / "libonnxruntime.dylib", runtime)
        monkeypatch.setenv("ORT_DYLIB_PATH", str(runtime))
    monkeypatch.setenv("CRYPT_PORT", str(port))
    monkeypatch.setenv("WORKSPACE_MEMORY_PORT", str(port))
    env = {
        **os.environ,
    }
    service = _start_service(binary, db, port, env)
    try:
        rule = "Always use JSONL for Morph parity event-e2e logs."
        proposal = learning.admit_user_event(
            _event(rule), evidence_text=rule, scope="Volumes-D-claude", category="tooling"
        )
        feedback = learning.approval_text(proposal)
        admitted = learning.approve_learning(
            proposal,
            feedback_event=_event(feedback, event_type="user_instruction", event_id="feedback-e2e"),
            feedback_text=feedback,
        )
        receipt = learning.persist_learning(
            admitted,
            token_file=token_file,
            base_url=f"http://127.0.0.1:{port}",
            installation_id="install-e2e",
        )
        assert receipt["inserted"] == 1
        _stop_service(service)
        service = _start_service(binary, db, port, env)
        proof = learning.verify_next_use(
            admitted,
            get_memory=lambda memory_id: _run(binary, db, port, ["get", memory_id]),
            recall=lambda query, scope: _recall(port, token, query, scope),
        )
        assert proof["delivered"] is True
        assert proof["event_id"] == "event-e2e"
    finally:
        _stop_service(service)
