"""Read-only Adapt consumer for Membrane ObservableEventV1 streams.

This module never promotes assistant/tool events into Taste. It keeps event order and lineage
opaque, then emits deterministic Insights labels for operator review.
"""

from __future__ import annotations

import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Iterable

ORIGINS = {"host", "user", "assistant", "tool", "repository", "service"}
REQUIRED = {
    "schema", "installation_id", "client_id", "session_id", "task_id", "turn_id",
    "trace_id", "event_id", "event_type", "origin", "content_ref_or_digest",
    "timestamp", "completeness", "policy_snapshot_digest",
}


def _validate(event: dict) -> None:
    if not isinstance(event, dict) or event.get("schema") != "membrane.observable-event.v1":
        raise ValueError("observable event schema is invalid")
    if set(event) - REQUIRED:
        raise ValueError("observable event contains unknown fields")
    missing = [key for key in REQUIRED if key not in event or event[key] in (None, "")]
    if missing or event["origin"] not in ORIGINS:
        raise ValueError("observable event required fields are invalid")


def consume_observable_events(events: Iterable[dict]) -> dict:
    """Return metadata-only lineage & Insights labels.

    Taste authority is sourced directly from transcripts; ObservableEventV1
    deliberately contains no recoverable content and cannot mint candidates.
    """
    materialized = list(events)
    for event in materialized:
        _validate(event)
    lineage = defaultdict(list)
    for index, event in enumerate(materialized):
        key = (event["installation_id"], event["session_id"], event["task_id"], event["turn_id"], event["trace_id"])
        lineage["|".join(key)].append({"index": index, "event_id": event["event_id"], "event_type": event["event_type"], "origin": event["origin"]})
    event_types = Counter(event["event_type"] for event in materialized)
    by_lineage = defaultdict(list)
    for event in materialized:
        by_lineage[(event["installation_id"], event["session_id"], event["task_id"], event["turn_id"], event["trace_id"])].append(event["event_type"])
    insights = []
    if any("context_requested" in types and "packet_delivered" not in types for types in by_lineage.values()):
        insights.append("missing_context_delivery")
    if any(event["event_type"] == "packet_delivered" and not event["completeness"].get("packet", False) for event in materialized):
        insights.append("degraded_context_delivery")
    if any(event["event_type"] in {"tool_receipt", "tool_receipt_failed"} and not event["completeness"].get("receipt", False) for event in materialized):
        insights.append("incomplete_tool_receipt")
    failed_tools = sum(event["event_type"] == "tool_receipt_failed" for event in materialized)
    if failed_tools >= 3:
        insights.append("repeated_tool_failure")
    return {
        "events": materialized,
        "lineage": dict(lineage),
        "taste_candidates": [],
        "insights": insights,
        "coverage": {"event_count": len(materialized), "origins": sorted({event["origin"] for event in materialized})},
    }


def read_observable_jsonl(path: str | Path) -> dict:
    records = []
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        wrapper = json.loads(line)
        records.extend(wrapper.get("observable_events", []))
    return consume_observable_events(records)
