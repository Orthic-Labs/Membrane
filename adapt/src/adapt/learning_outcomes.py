"""Durable, auditable outcome ledger for Membrane-event-derived Adapt rules.

``adapt_event_learning.AdmittedLearning`` is an in-memory value: build it, approve
it, persist it, and it is gone once the process exits. Plan item C14 / L2 requires
that each learned rule leave an audit trail — what evidence produced it, when, and
what happened — that survives a restart and lets a resumed ingestion cycle tell
"already proposed, still pending" apart from "never seen this event before"
without re-admitting it.

One append-only JSONL row per lifecycle transition, keyed by ``event_id``. Mirrors
the existing ``run_journal.RunJournal`` shape/conventions (append-only, JSONL,
``~/.claude/adapt``) rather than inventing a second persistence style.
"""

from __future__ import annotations

import datetime as dt
import json
from pathlib import Path
from typing import Any, Optional

STATE_DIR = Path.home() / ".claude" / "adapt"
OUTCOME_FILE = STATE_DIR / "event_learning_outcomes.jsonl"

# proposed        — admitted as a quarantined candidate (lifecycle_state=candidate)
# rejected        — admission or approval raised; not eligible for recall
# approved        — a separate, later user-origin event supplied the exact
#                    approval text; lifecycle_state=active, not yet in Cortex
# persisted       — approved AND durably written through adapt_persistence
# persist_failed  — approved but the Cortex write did not complete
VALID_STATUSES = frozenset(
    {"proposed", "rejected", "approved", "persisted", "persist_failed"}
)

# Statuses from which a proposal can still move forward (i.e. is "pending").
_PENDING_STATUSES = frozenset({"proposed"})
_TERMINAL_STATUSES = VALID_STATUSES - _PENDING_STATUSES


def _now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


class LearningOutcomeStore:
    """Append-only ledger keyed by ``event_id``; late-binds its path per instance."""

    def __init__(self, path: Path | None = None) -> None:
        self.path = path if path is not None else OUTCOME_FILE

    def record(
        self,
        *,
        event_id: str,
        trace_id: str,
        rule_key: str,
        evidence_sha256: str,
        status: str,
        digest: str = "",
        reason: str = "",
        approval_event_id: str = "",
        event: dict[str, Any] | None = None,
        evidence_text: str = "",
        scope: str = "",
        category: str = "",
        record_now: str = "",
        now: str | None = None,
    ) -> dict[str, Any]:
        """Append one outcome row. ``event``/``evidence_text``/``scope``/``category``/
        ``record_now`` are only meaningful (and only persisted) on
        ``status="proposed"`` — they are exactly what a later run needs to
        deterministically replay ``admit_user_event`` (including its original
        record timestamp, via ``record_now``) and reconstruct the pending
        proposal byte-for-byte, without Adapt ever having to remember it in
        memory across restarts.
        """
        if status not in VALID_STATUSES:
            raise ValueError(f"unknown outcome status: {status!r}; valid: {sorted(VALID_STATUSES)}")
        entry: dict[str, Any] = {
            "ts": now or _now_iso(),
            "event_id": event_id,
            "trace_id": trace_id,
            "rule_key": rule_key,
            "evidence_sha256": evidence_sha256,
            "status": status,
            "digest": digest,
            "reason": reason,
            "approval_event_id": approval_event_id,
        }
        if status == "proposed":
            entry["event"] = event
            entry["evidence_text"] = evidence_text
            entry["scope"] = scope
            entry["category"] = category
            entry["record_now"] = record_now
        entry = {k: v for k, v in entry.items() if v not in (None, "", {})}
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with open(self.path, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(entry, ensure_ascii=False, sort_keys=True) + "\n")
        return entry

    def all(self) -> list[dict[str, Any]]:
        if not self.path.exists():
            return []
        rows: list[dict[str, Any]] = []
        with open(self.path, encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if line:
                    rows.append(json.loads(line))
        return rows

    def for_event(self, event_id: str) -> list[dict[str, Any]]:
        return [row for row in self.all() if row.get("event_id") == event_id]

    def latest_status(self, event_id: str) -> Optional[str]:
        rows = self.for_event(event_id)
        return rows[-1]["status"] if rows else None

    def already_processed(self, event_id: str) -> bool:
        """Idempotency guard: any recorded outcome means this event has already
        been proposed/rejected once and must not be re-admitted on a
        resumed/retried pull (double-counting guard for at-least-once delivery).
        """
        return self.latest_status(event_id) is not None

    def pending_proposals(self) -> list[dict[str, Any]]:
        """Proposals whose *latest* status is still ``proposed`` — i.e. not yet
        approved, rejected, or persisted — with the fields needed to replay
        ``admit_user_event`` and rebuild the in-memory candidate.
        """
        latest_by_event: dict[str, dict[str, Any]] = {}
        for row in self.all():
            latest_by_event[row["event_id"]] = row
        return [
            row
            for row in latest_by_event.values()
            if row.get("status") in _PENDING_STATUSES and "event" in row
        ]


__all__ = [
    "LearningOutcomeStore",
    "VALID_STATUSES",
]
