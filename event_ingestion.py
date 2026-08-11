"""Paged, resumable Membrane -> Adapt event ingestion boundary (plan C14 / L2).

The resident Crypt service exposes the two query routes used here. ``HttpEventTransport``
adapts those routes to the transport protocol, while ``query_by_id`` retrieves the
surrounding content for a taste candidate without widening Taste's origin-scoped query.

Expected request/response fields mirror the Rust query contract.
"""

from __future__ import annotations

import datetime as dt
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Callable, Protocol

WORKSPACE_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PORT = 47851
TOKEN_PATH = WORKSPACE_ROOT / "tools" / ".cache" / "memory" / "api-token"

DEFAULT_PAGE_LIMIT = 200
MAX_PAGE_LIMIT = 1000
CURSOR_STATE_DIR = Path.home() / ".claude" / "adapt"
CURSOR_STATE_FILE = CURSOR_STATE_DIR / "event_cursors.json"

STREAMS = ("taste", "insights")


class EventIngestionError(RuntimeError):
    """Raised when the transport boundary or paging contract is violated."""


class EventTransport(Protocol):
    """Transport-agnostic boundary for the Membrane read routes."""

    def query_for_taste(self, query: dict[str, Any]) -> dict[str, Any]: ...

    def query_for_insights(self, query: dict[str, Any]) -> dict[str, Any]: ...


def _crypt_base_url() -> str:
    port = os.environ.get("CRYPT_PORT") or os.environ.get("WORKSPACE_MEMORY_PORT") or str(DEFAULT_PORT)
    return f"http://127.0.0.1:{port}"


def _token_path() -> Path:
    return Path(os.environ.get("CRYPT_API_TOKEN_FILE", str(TOKEN_PATH)))


class HttpEventTransport:
    """Bearer-authenticated client for Crypt's observable-event query routes."""

    def __init__(self, *, base_url: str | None = None, token_file: Path | None = None, timeout: float = 30.0) -> None:
        self.base_url = (base_url or _crypt_base_url()).rstrip("/")
        self.token_file = token_file or _token_path()
        self.timeout = timeout

    def _post(self, path: str, payload: dict[str, Any]) -> dict[str, Any]:
        try:
            token = self.token_file.read_text(encoding="utf-8").strip()
        except OSError as exc:
            raise EventIngestionError("Crypt API token is unavailable") from exc
        if not token:
            raise EventIngestionError("Crypt API token is empty")
        request = urllib.request.Request(
            f"{self.base_url}{path}", data=json.dumps(payload).encode(),
            headers={"Content-Type": "application/json", "Authorization": f"Bearer {token}"}, method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                status = response.status
                result = json.loads(response.read().decode())
        except urllib.error.HTTPError as exc:
            raise EventIngestionError(f"Crypt event query rejected with HTTP {exc.code}") from exc
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            raise EventIngestionError("Crypt event service is unavailable") from exc
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise EventIngestionError("Crypt event response is not valid JSON") from exc
        if status != 200 or not isinstance(result, dict):
            raise EventIngestionError("Crypt event response is malformed")
        return result

    def query_for_taste(self, query: dict[str, Any]) -> dict[str, Any]:
        return self._post("/v1/telemetry/observable-events:query-taste", _wire_query(query))

    def query_for_insights(self, query: dict[str, Any]) -> dict[str, Any]:
        return self._post("/v1/telemetry/observable-events:query-insights", _wire_query(query))


def _wire_query(query: dict[str, Any]) -> dict[str, Any]:
    names = {"event_type": "eventType", "session_id": "sessionId", "task_id": "taskId", "trace_id": "traceId", "installation_id": "installationId", "after_sequence": "afterSequence"}
    return {names.get(key, key): value for key, value in query.items()}


def query_by_id(event_id: str, *, transport: EventTransport | None = None) -> dict[str, Any] | None:
    """Fetch a candidate row by immutable event ID through authorized streams."""
    if not isinstance(event_id, str) or not event_id.strip():
        raise EventIngestionError("event_id is required")
    client = transport or HttpEventTransport()
    for method in (client.query_for_taste, client.query_for_insights):
        result = method({"event_id": event_id, "limit": 1000})
        for row in result.get("rows", []):
            if row.get("event_id") == event_id:
                return row
    return None


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
    independent check on the Adapt side: if a row somehow arrives on the taste
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
    "HttpEventTransport",
    "query_by_id",
    "CursorStore",
    "pull_stream",
    "pull_and_label_insights",
    "DEFAULT_PAGE_LIMIT",
    "MAX_PAGE_LIMIT",
]
