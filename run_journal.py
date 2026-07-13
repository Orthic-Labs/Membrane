"""Per-stage run journal for the adapt pipeline with payload persistence.

Design (Codex review 2026-07-12):
- Each batch's stage progress is recorded with the actual outputs (observations,
  actions), not just stage markers. Retries consume the cached payload from
  the last completed stage instead of re-running the LLM call.
- A provider failure mid-stream becomes a recoverable checkpoint, not silent
  data loss.
- `mark_learned` does NOT fire on entry (Codex: that converts provider
  failures into silently skipped sessions).
- Each journal line carries the batch_id, stage, ts, and (where relevant) the
  LLM payloads the orchestrator would otherwise re-fetch on retry.

Stage sequence per batch:
  discovered -> extracted -> synthesized -> applied -> committed

State layout (machine-local; lives next to state.json):
  state.json           canonical learned-session map; persistence gate
  run_journal.jsonl    append-only per-stage entries with payloads
"""
from __future__ import annotations

import datetime as dt
import json
import uuid
from pathlib import Path
from typing import Iterable, Optional

STATE_DIR = Path.home() / ".claude" / "adapt"
JOURNAL_FILE = STATE_DIR / "run_journal.jsonl"

VALID_STAGES = ("discovered", "extracted", "synthesized", "applied", "committed")


class RunJournal:
    """Append-only stage log with payload persistence, queryable by batch_id."""

    def __init__(self, path: Path | None = None):
        # Late-bind JOURNAL_FILE so monkeypatching in tests works.
        self.path = path if path is not None else JOURNAL_FILE
        self.path.parent.mkdir(parents=True, exist_ok=True)

    def record(self, batch_id: str, stage: str, **details) -> None:
        if stage not in VALID_STAGES:
            raise ValueError(f"unknown stage {stage!r}; valid: {VALID_STAGES}")
        entry = {
            "ts": dt.datetime.now(dt.timezone.utc).isoformat(),
            "batch_id": batch_id,
            "stage": stage,
            **details,
        }
        # Strip payload to None so the file isn't peppered with empty keys.
        entry = {k: v for k, v in entry.items() if v not in (None, {}, [])}
        with open(self.path, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(entry, ensure_ascii=False) + "\n")

    def batches(self) -> list[dict]:
        if not self.path.exists():
            return []
        out = []
        with open(self.path, encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                out.append(json.loads(line))
        return out

    def last_stage(self, batch_id: str) -> Optional[str]:
        latest = None
        for entry in self.batches():
            if entry.get("batch_id") == batch_id:
                latest = entry["stage"]
        return latest

    def incomplete_batches(self) -> list[str]:
        completed = set()
        latest: dict[str, str] = {}
        for entry in self.batches():
            bid = entry["batch_id"]
            latest[bid] = entry["stage"]
            if entry["stage"] == "committed":
                completed.add(bid)
        return [bid for bid, stage in latest.items() if bid not in completed]

    # ----- payload retrieval: lets a retry consume cached LLM output -----

    def cached_payload(self, batch_id: str, stage: str) -> Optional[dict]:
        """Most recent record matching (batch_id, stage) including payload keys."""
        latest = None
        for entry in self.batches():
            if entry.get("batch_id") == batch_id and entry.get("stage") == stage:
                latest = entry
        return latest

    def pending_batch(self) -> Optional[dict]:
        """Return the most recent incomplete batch (any non-committed stage with
        payload). Used by the orchestrator on startup to decide whether to
        resume or start fresh.
        """
        entries = self.batches()
        if not entries:
            return None
        latest_by_batch: dict[str, dict] = {}
        for entry in entries:
            latest_by_batch[entry["batch_id"]] = entry
        for batch_id, last in reversed(list(latest_by_batch.items())):
            if last["stage"] != "committed":
                # Resume from the next stage.
                return last
        return None


def new_batch_id() -> str:
    return dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%S") + "-" + uuid.uuid4().hex[:6]
