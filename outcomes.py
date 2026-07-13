"""Typed batch outcomes — replaces the `[]` ambiguity in extract/synth.

Codex + Fable review 2026-07-12: the orchestrator couldn't distinguish a
legitimate empty result from a scan/parse/provider failure that ALSO returned
`[]`. Without that distinction, runs silently marked sessions learned while
literally no work was done.

Each batch now returns one of:
    SUCCESS          — produced non-empty actions eligible for admission
    VALID_EMPTY      — model returned [] deliberately; nothing qualifies,
                       nothing else went wrong
    SCANNER_BLOCKED  — batch scanner refused the payload; retryable
    PARSE_FAILED     — non-empty response, JSON parse failed; retryable
    PROVIDER_FAILED  — provider error before any response; retryable
    INVALID_PROMPT   — extraction prompt was malformed (deterministic guard)
"""
from __future__ import annotations

from dataclasses import dataclass


class Outcome:
    SUCCESS = "success"
    VALID_EMPTY = "valid_empty"
    SCANNER_BLOCKED = "scanner_blocked"
    PARSE_FAILED = "parse_failed"
    PROVIDER_FAILED = "provider_failed"
    INVALID_PROMPT = "invalid_prompt"


# Outcomes that allow the orchestrator to advance state.
COMMITTABLE = frozenset({Outcome.SUCCESS, Outcome.VALID_EMPTY})

# Outcomes that require retry rather than commit.
RETRYABLE = frozenset({
    Outcome.SCANNER_BLOCKED,
    Outcome.PARSE_FAILED,
    Outcome.PROVIDER_FAILED,
    Outcome.INVALID_PROMPT,
})

ALL = frozenset({Outcome.SUCCESS, Outcome.VALID_EMPTY} | RETRYABLE)


@dataclass(frozen=True)
class BatchOutcome:
    """Per-batch result envelope. `actions` is non-empty only on SUCCESS."""
    outcome: str
    actions: list[dict]
    reason: str = ""        # optional detail for retry decisions

    @classmethod
    def success(cls, actions: list[dict]) -> "BatchOutcome":
        return cls(Outcome.SUCCESS, list(actions))

    @classmethod
    def valid_empty(cls, reason: str = "") -> "BatchOutcome":
        return cls(Outcome.VALID_EMPTY, [], reason)

    @classmethod
    def scanner_blocked(cls, reason: str = "") -> "BatchOutcome":
        return cls(Outcome.SCANNER_BLOCKED, [], reason)

    @classmethod
    def parse_failed(cls, reason: str = "") -> "BatchOutcome":
        return cls(Outcome.PARSE_FAILED, [], reason)

    @classmethod
    def provider_failed(cls, reason: str = "") -> "BatchOutcome":
        return cls(Outcome.PROVIDER_FAILED, [], reason)

    @classmethod
    def invalid_prompt(cls, reason: str = "") -> "BatchOutcome":
        return cls(Outcome.INVALID_PROMPT, [], reason)

    @property
    def committable(self) -> bool:
        return self.outcome in COMMITTABLE


def aggregate(outcomes: list[BatchOutcome]) -> tuple[bool, dict[str, int]]:
    """Returns (all_committable, outcome_counts)."""
    counts = {o: 0 for o in ALL}
    for b in outcomes:
        counts[b.outcome] = counts.get(b.outcome, 0) + 1
    return all(b.committable for b in outcomes), counts
