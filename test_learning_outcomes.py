from __future__ import annotations

from pathlib import Path

import pytest

import learning_outcomes as outcomes


def test_record_rejects_unknown_status(tmp_path: Path) -> None:
    store = outcomes.LearningOutcomeStore(tmp_path / "ledger.jsonl")
    with pytest.raises(ValueError, match="unknown outcome status"):
        store.record(
            event_id="e0", trace_id="t0", rule_key="", evidence_sha256="sha256:x",
            status="bogus",
        )


def test_proposed_then_approved_transition_is_auditable(tmp_path: Path) -> None:
    store = outcomes.LearningOutcomeStore(tmp_path / "ledger.jsonl")
    assert store.already_processed("e0") is False
    assert store.latest_status("e0") is None

    store.record(
        event_id="e0", trace_id="t0", rule_key="scope/rule-0",
        evidence_sha256="sha256:" + "a" * 64, status="proposed", digest="sha256:" + "b" * 64,
        event={"event_id": "e0"}, evidence_text="Always do X.", scope="scope", category="tooling",
    )
    assert store.already_processed("e0") is True
    assert store.latest_status("e0") == "proposed"
    pending = store.pending_proposals()
    assert len(pending) == 1
    assert pending[0]["evidence_text"] == "Always do X."
    assert pending[0]["event"] == {"event_id": "e0"}

    store.record(
        event_id="e0", trace_id="t0", rule_key="scope/rule-0",
        evidence_sha256="sha256:" + "a" * 64, status="approved",
        approval_event_id="feedback-1",
    )
    assert store.latest_status("e0") == "approved"
    assert store.pending_proposals() == []  # no longer pending once approved

    history = store.for_event("e0")
    assert [row["status"] for row in history] == ["proposed", "approved"]
    # Evidence-of-what-produced-it survives on the original row even after
    # a later status is appended (append-only — history is never mutated).
    assert history[0]["evidence_text"] == "Always do X."


def test_rejected_outcome_is_not_pending(tmp_path: Path) -> None:
    store = outcomes.LearningOutcomeStore(tmp_path / "ledger.jsonl")
    store.record(
        event_id="e1", trace_id="t1", rule_key="", evidence_sha256="sha256:" + "c" * 64,
        status="rejected", reason="origin-not-user:assistant",
    )
    assert store.already_processed("e1") is True
    assert store.pending_proposals() == []
    assert store.for_event("e1")[0]["reason"] == "origin-not-user:assistant"


def test_ledger_persists_across_instances(tmp_path: Path) -> None:
    path = tmp_path / "ledger.jsonl"
    outcomes.LearningOutcomeStore(path).record(
        event_id="e2", trace_id="t2", rule_key="scope/rule-2",
        evidence_sha256="sha256:" + "d" * 64, status="proposed",
        event={"event_id": "e2"}, evidence_text="Never do Y.", scope="scope", category="workflow",
    )
    reloaded = outcomes.LearningOutcomeStore(path)
    assert reloaded.already_processed("e2") is True
    assert len(reloaded.pending_proposals()) == 1


def test_empty_ledger_has_no_pending_or_processed_events(tmp_path: Path) -> None:
    store = outcomes.LearningOutcomeStore(tmp_path / "does-not-exist.jsonl")
    assert store.all() == []
    assert store.pending_proposals() == []
    assert store.already_processed("anything") is False
