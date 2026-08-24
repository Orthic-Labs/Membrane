from __future__ import annotations

from adapt import mine_snapshot_manifest
from adapt import taste_apply


def test_mining_indices_cover_deterministic_and_recall_turns(monkeypatch) -> None:
    events = [{}, {}, {}]
    monkeypatch.setattr(
        mine_snapshot_manifest.taste_v2, "iter_candidate_indices", lambda _events: iter((1,))
    )
    monkeypatch.setattr(
        mine_snapshot_manifest.taste_v2, "iter_proposer_indices", lambda _events: iter((0, 2))
    )
    assert mine_snapshot_manifest._mining_indices(events) == [0, 1, 2]


def test_contextual_turn_separates_context_from_authoritative_source(monkeypatch) -> None:
    events = [
        {"text": "I will skip verification."},
        {"text": "That is not acceptable; verify before claiming completion."},
    ]
    monkeypatch.setattr(
        mine_snapshot_manifest.transcript_sources,
        "event_provenance",
        lambda event: "assistant" if event is events[0] else "external_user",
    )
    payload = mine_snapshot_manifest._contextual_turn(events, 1)
    assert "[NON-AUTHORITATIVE PRIOR CONTEXT]" in payload
    assert "assistant: I will skip verification." in payload
    assert "[AUTHORITATIVE SOURCE USER TURN]" in payload
    assert payload.endswith("That is not acceptable; verify before claiming completion.")


def test_frozen_v2_coverage_refuses_partial_state_advance() -> None:
    body = {
        "generator": "adapt-frozen-open-transcripts-v2:" + "a" * 64,
        "extraction_coverage": {
            "complete": True,
            "source_count": 2,
            "corpus_source_count": 2,
            "shard_index": 0,
            "shard_count": 1,
            "canonical_user_turns": 10,
            "mined_user_turns": 8,
            "policy_excluded_user_turns": 1,
            "llm_batches": 2,
            "committable_batches": 2,
            "failed_batches": 0,
            "batch_char_budget": 120000,
            "checkpointed_batches": 2,
        },
    }
    assert "incomplete" in taste_apply._extraction_coverage_error(body, 2)
