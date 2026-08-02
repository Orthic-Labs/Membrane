"""Paged, resumable Membrane -> Morph event ingestion boundary (plan C14 / L2).

Membrane's crypt-store crate (``context_telemetry.rs``, commit ``4fe7804``) added the
first read path over the observable-event store:

    MemDb::query_observable_events_for_taste(&self, filter) -> ObservableEventQueryResult
    MemDb::query_observable_events_for_insights(&self, filter) -> ObservableEventQueryResult

There is **no HTTP route over these functions yet** (the orchestrator is wiring that
separately), so this module talks to a small transport-agnostic ``EventTransport``
protocol instead of a concrete HTTP client. Once the route exists, a thin adapter
implementing ``query_for_taste`` / ``query_for_insights`` against it is a drop-in —
nothing else in this module, or in ``morph_event_learning``, needs to change.

Expected request/response shape (mirrors ``ObservableEventQuery`` /
``ObservableEventQueryResult`` field-for-field so a future HTTP client is a thin
wrapper, not a redesign):

    request = {
        "since": str | None, "until": str | None, "event_type": str | None,
        "session_id": str | None, "task_id": str | None, "trace_id": str | None,
        "installation_id": str | None, "after_sequence": int | None,
        "limit": int,            # REQUIRED, 1..=1000 — the Rust side's Default
                                  # gives an invalid 0; this module always sets it.
    }
    response = {
        "rows": list[dict],      # orthic.observable-event.v1 rows
        "limit": int,
        "truncated": bool,       # True: more rows exist beyond this page
        "next_cursor": int | None,
    }

Semantics this module exists to protect:

- ``query_for_taste`` is the ONLY entry point Morph Taste may ever read from. It is
  the one Membrane function that filters to user-origin before Morph sees a row —
  Taste has no origin parameter by design, so origin filtering happens upstream of
  this boundary, not here. This module adds a defence-in-depth check on top: any
  row a taste-stream call returns with a non-"user" origin is treated as a
  transport-contract violation and raised, never silently dropped or forwarded.
- ``query_for_insights`` carries the full authorized stream (every origin) and must
  never feed rule admission — callers route its rows only into
  ``observable_events.consume_observable_events`` for Insights labelling.
- A ``truncated: true`` result always means "more data now" and must be paged to
  exhaustion before the caller's cursor is considered caught up.
"""

from __future__ import annotations

import datetime as dt
import json
import os
from pathlib import Path
from typing import Any, Callable, Protocol

DEFAULT_PAGE_LIMIT = 200
MAX_PAGE_LIMIT = 1000
CURSOR_STATE_DIR = Path.home() / ".claude" / "morph"
CURSOR_STATE_FILE = CURSOR_STATE_DIR / "event_cursors.json"

STREAMS = ("taste", "insights")


class EventIngestionError(RuntimeError):
    """Raised when the transport boundary or paging contract is violated."""


class EventTransport(Protocol):
    """Transport-agnostic boundary a real HTTP client will eventually implement."""

    def query_for_taste(self, query: dict[str, Any]) -> dict[str, Any]: ...

    def query_for_insights(self, query: dict[str, Any]) -> dict[str, Any]: ...


def _now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def _cursor_key(stream: str, installation_id: str) -> str:
    return f"{stream}:{installation_id}"


class CursorStore:
    """Durable per-(stream, installation) paging cursor.

    One small JSON map, atomically rewritten (temp file + ``os.replace``) so a
    crash mid-write can never corrupt a previously committed cursor. Late-binds
    the default path via the instance attribute (not a module-level constant read
    at call time) so tests can point it at a tmp_path without monkeypatching.
    """

    def __init__(self, path: Path | None = None) -> None:
        self.path = path if path is not None else CURSOR_STATE_FILE

    def _load_all(self) -> dict[str, Any]:
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return {}
        return data if isinstance(data, dict) else {}

    def load(self, stream: str, installation_id: str) -> int | None:
        row = self._load_all().get(_cursor_key(stream, installation_id))
        if isinstance(row, dict) and isinstance(row.get("after_sequence"), int):
            return row["after_sequence"]
        return None

    def save(self, stream: str, installation_id: str, after_sequence: int | None) -> None:
        if after_sequence is None:
            return
        state = self._load_all()
        state[_cursor_key(stream, installation_id)] = {
            "after_sequence": after_sequence,
            "updated_at": _now_iso(),
        }
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_name(f"{self.path.name}.tmp-{os.getpid()}")
        tmp.write_text(json.dumps(state, indent=2, sort_keys=True), encoding="utf-8")
        os.replace(tmp, self.path)


def _query(transport: EventTransport, stream: str, request: dict[str, Any]) -> dict[str, Any]:
    method: Callable[[dict[str, Any]], dict[str, Any]] | None = getattr(
        transport, f"query_for_{stream}", None
    )
    if method is None:
        raise EventIngestionError(f"transport is missing query_for_{stream}")
    result = method(dict(request))
    if (
        not isinstance(result, dict)
        or not isinstance(result.get("rows"), list)
        or not isinstance(result.get("truncated"), bool)
    ):
        raise EventIngestionError(f"malformed {stream} query result: {result!r}")
    return result


def _assert_taste_origin(rows: list[dict[str, Any]]) -> None:
    """Defence in depth behind the store-level origin guarantee.

    The Rust ``query_observable_events_for_taste`` path is what actually enforces
    user-origin-only — Taste has no origin parameter by design. This is a second,
    independent check on the Morph side: if a row somehow arrives on the taste
    stream with a non-"user" origin (broken transport, future refactor, bug), it
    must never reach admission silently. Raise loud instead of filtering quietly.
    """
    offenders = [row.get("origin") for row in rows if row.get("origin") != "user"]
    if offenders:
        raise EventIngestionError(
            "taste stream returned non-user-origin row(s); refusing to forward to "
            f"admission (defence in depth): origins={offenders!r}"
        )


def pull_stream(
    transport: EventTransport,
    *,
    stream: str,
    installation_id: str,
    cursor_store: CursorStore | None = None,
    page_limit: int = DEFAULT_PAGE_LIMIT,
    on_page: Callable[[list[dict[str, Any]]], None] | None = None,
    max_pages: int = 10_000,
) -> list[dict[str, Any]]:
    """Page one Membrane event stream to exhaustion, never losing or duplicating a range.

    - Pages via ``next_cursor`` while ``truncated`` is true (there is more data now).
    - The durable cursor advances only AFTER ``on_page`` returns successfully for
      that page. If ``on_page`` raises, the cursor is not advanced, so the next
      call re-fetches and re-hands the same page — at-least-once delivery. Callers
      that need exactly-once *effects* (not just delivery) make ``on_page``
      idempotent per event (see ``learning_outcomes.LearningOutcomeStore``).
    - A ``truncated: true`` page with no ``next_cursor`` is a transport-contract
      violation, not "caught up" — raised rather than silently stopping, which
      would otherwise drop the remainder of the range.
    """
    if stream not in STREAMS:
        raise EventIngestionError(f"unknown stream: {stream!r}; expected one of {STREAMS}")
    if not (1 <= page_limit <= MAX_PAGE_LIMIT):
        raise EventIngestionError(f"page_limit must be 1..={MAX_PAGE_LIMIT}, got {page_limit}")

    cursor_store = cursor_store or CursorStore()
    cursor = cursor_store.load(stream, installation_id)
    collected: list[dict[str, Any]] = []
    pages = 0

    while True:
        pages += 1
        if pages > max_pages:
            raise EventIngestionError("paging did not converge; refusing to loop forever")
        request = {
            "since": None,
            "until": None,
            "event_type": None,
            "session_id": None,
            "task_id": None,
            "trace_id": None,
            "installation_id": installation_id,
            "after_sequence": cursor,
            "limit": page_limit,
        }
        result = _query(transport, stream, request)
        rows = result["rows"]
        if stream == "taste":
            _assert_taste_origin(rows)
        if on_page is not None:
            on_page(rows)
        collected.extend(rows)

        truncated = bool(result["truncated"])
        next_cursor = result.get("next_cursor")
        if truncated and next_cursor is None:
            raise EventIngestionError(
                "truncated result without next_cursor; cannot page safely without "
                "risking a dropped range"
            )
        if next_cursor is not None:
            cursor_store.save(stream, installation_id, next_cursor)
            cursor = next_cursor
        if not truncated:
            break
    return collected


def pull_and_label_insights(
    transport: EventTransport,
    *,
    installation_id: str,
    cursor_store: CursorStore | None = None,
    page_limit: int = DEFAULT_PAGE_LIMIT,
) -> dict[str, Any]:
    """Page the Insights stream to exhaustion and emit deterministic labels.

    Routes exclusively through ``query_for_insights`` and
    ``observable_events.consume_observable_events`` — the full authorized stream,
    never the Taste admission path.
    """
    import observable_events  # local import: keeps this module importable standalone

    rows = pull_stream(
        transport,
        stream="insights",
        installation_id=installation_id,
        cursor_store=cursor_store,
        page_limit=page_limit,
    )
    return observable_events.consume_observable_events(rows)


__all__ = [
    "EventIngestionError",
    "EventTransport",
    "CursorStore",
    "pull_stream",
    "pull_and_label_insights",
    "DEFAULT_PAGE_LIMIT",
    "MAX_PAGE_LIMIT",
]
