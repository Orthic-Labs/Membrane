from __future__ import annotations

from pathlib import Path

import pytest

from adapt import event_ingestion as ing


def _row(event_id: str, origin: str = "user") -> dict:
    return {
        "schema": "membrane.observable-event.v1",
        "installation_id": "install-1",
        "client_id": "codex",
        "session_id": "session-1",
        "task_id": "task-1",
        "turn_id": "turn-1",
        "trace_id": "trace-1",
        "event_id": event_id,
        "event_type": "user_correction",
        "origin": origin,
        "content_ref_or_digest": "sha256:" + "a" * 64,
        "timestamp": "2026-08-02T00:00:00Z",
        "completeness": {"input": True},
        "policy_snapshot_digest": "sha256:" + "b" * 64,
    }


class FakeTransport:
    """Simulates the eventual HTTP client: pages a fixed list of rows by
    after_sequence, honouring `limit` and reporting truncated/next_cursor
    exactly like ObservableEventQueryResult would."""

    def __init__(self, rows: list[dict], *, page_size_cap: int | None = None) -> None:
        self._rows = rows  # index == sequence number
        self._page_size_cap = page_size_cap
        self.calls: list[dict] = []

    def _page(self, query: dict) -> dict:
        self.calls.append(dict(query))
        start = (query["after_sequence"] or 0)
        limit = query["limit"]
        if self._page_size_cap is not None:
            limit = min(limit, self._page_size_cap)
        window = self._rows[start : start + limit]
        next_index = start + len(window)
        truncated = next_index < len(self._rows)
        return {
            "rows": window,
            "limit": query["limit"],
            "truncated": truncated,
            "next_cursor": next_index if window else query["after_sequence"],
        }

    def query_for_taste(self, query: dict) -> dict:
        return self._page(query)

    def query_for_insights(self, query: dict) -> dict:
        return self._page(query)


def test_pull_stream_pages_to_exhaustion_and_persists_cursor(tmp_path: Path) -> None:
    rows = [_row(f"e{i}") for i in range(25)]
    transport = FakeTransport(rows, page_size_cap=7)
    cursor_store = ing.CursorStore(tmp_path / "cursors.json")

    collected = ing.pull_stream(
        transport,
        stream="taste",
        installation_id="install-1",
        cursor_store=cursor_store,
        page_limit=10,
    )

    assert [r["event_id"] for r in collected] == [f"e{i}" for i in range(25)]
    assert len(transport.calls) == 4  # 7+7+7+4
    assert cursor_store.load("taste", "install-1") == 25

    # A second call with no new rows must not re-deliver anything already seen.
    more = ing.pull_stream(
        transport,
        stream="taste",
        installation_id="install-1",
        cursor_store=cursor_store,
        page_limit=10,
    )
    assert more == []
    assert transport.calls[-1]["after_sequence"] == 25


def test_pull_stream_resumes_from_persisted_cursor_after_new_rows_land(tmp_path: Path) -> None:
    rows = [_row(f"e{i}") for i in range(5)]
    transport = FakeTransport(rows)
    cursor_store = ing.CursorStore(tmp_path / "cursors.json")

    first = ing.pull_stream(
        transport, stream="taste", installation_id="install-1", cursor_store=cursor_store,
    )
    assert [r["event_id"] for r in first] == [f"e{i}" for i in range(5)]

    transport._rows.extend([_row("e5"), _row("e6")])
    second = ing.pull_stream(
        transport, stream="taste", installation_id="install-1", cursor_store=cursor_store,
    )
    assert [r["event_id"] for r in second] == ["e5", "e6"]


def test_pull_stream_does_not_advance_cursor_when_on_page_raises(tmp_path: Path) -> None:
    rows = [_row(f"e{i}") for i in range(3)]
    transport = FakeTransport(rows)
    cursor_store = ing.CursorStore(tmp_path / "cursors.json")

    def boom(_rows: list[dict]) -> None:
        raise RuntimeError("simulated durable-write failure")

    with pytest.raises(RuntimeError, match="simulated"):
        ing.pull_stream(
            transport,
            stream="taste",
            installation_id="install-1",
            cursor_store=cursor_store,
            on_page=boom,
        )
    assert cursor_store.load("taste", "install-1") is None

    # Retry without the failure must re-deliver the same range, not skip it.
    seen: list[str] = []
    ing.pull_stream(
        transport,
        stream="taste",
        installation_id="install-1",
        cursor_store=cursor_store,
        on_page=lambda page: seen.extend(r["event_id"] for r in page),
    )
    assert seen == ["e0", "e1", "e2"]


def test_pull_stream_rejects_truncated_result_missing_next_cursor(tmp_path: Path) -> None:
    class BrokenTransport:
        def query_for_taste(self, query: dict) -> dict:
            return {"rows": [_row("e0")], "limit": query["limit"], "truncated": True, "next_cursor": None}

    with pytest.raises(ing.EventIngestionError, match="next_cursor"):
        ing.pull_stream(
            BrokenTransport(),
            stream="taste",
            installation_id="install-1",
            cursor_store=ing.CursorStore(tmp_path / "cursors.json"),
        )


def test_pull_stream_taste_defends_against_non_user_origin_rows(tmp_path: Path) -> None:
    """Store-level origin isolation is Membrane's job; this is the Adapt-side
    defence in depth required by C14/L2 item 5 — even if a row somehow arrives
    on the taste stream with a non-user origin, it must never reach admission."""

    class LeakyTransport:
        def query_for_taste(self, query: dict) -> dict:
            return {
                "rows": [_row("e0", origin="assistant")],
                "limit": query["limit"],
                "truncated": False,
                "next_cursor": None,
            }

    handled: list[dict] = []
    with pytest.raises(ing.EventIngestionError, match="non-user-origin"):
        ing.pull_stream(
            LeakyTransport(),
            stream="taste",
            installation_id="install-1",
            cursor_store=ing.CursorStore(tmp_path / "cursors.json"),
            on_page=handled.append,
        )
    assert handled == []  # never forwarded to the caller's handler


def test_pull_stream_insights_allows_non_user_origin(tmp_path: Path) -> None:
    rows = [_row("e0", origin="assistant"), _row("e1", origin="tool")]
    transport = FakeTransport(rows)
    collected = ing.pull_stream(
        transport,
        stream="insights",
        installation_id="install-1",
        cursor_store=ing.CursorStore(tmp_path / "cursors.json"),
    )
    assert [r["origin"] for r in collected] == ["assistant", "tool"]


def test_pull_and_label_insights_routes_through_observable_events(tmp_path: Path) -> None:
    rows = [_row("e0", origin="user"), _row("e1", origin="assistant")]
    transport = FakeTransport(rows)
    result = ing.pull_and_label_insights(
        transport,
        installation_id="install-1",
        cursor_store=ing.CursorStore(tmp_path / "cursors.json"),
    )
    assert result["coverage"]["event_count"] == 2
    assert result["taste_candidates"] == []


def test_pull_stream_rejects_unknown_stream(tmp_path: Path) -> None:
    with pytest.raises(ing.EventIngestionError, match="unknown stream"):
        ing.pull_stream(
            FakeTransport([]),
            stream="bogus",  # type: ignore[arg-type]
            installation_id="install-1",
            cursor_store=ing.CursorStore(tmp_path / "cursors.json"),
        )


def test_pull_stream_rejects_out_of_range_page_limit(tmp_path: Path) -> None:
    with pytest.raises(ing.EventIngestionError, match="page_limit"):
        ing.pull_stream(
            FakeTransport([]),
            stream="taste",
            installation_id="install-1",
            cursor_store=ing.CursorStore(tmp_path / "cursors.json"),
            page_limit=0,
        )


def test_pull_stream_rejects_transport_missing_method(tmp_path: Path) -> None:
    class Empty:
        pass

    with pytest.raises(ing.EventIngestionError, match="query_for_taste"):
        ing.pull_stream(
            Empty(),
            stream="taste",
            installation_id="install-1",
            cursor_store=ing.CursorStore(tmp_path / "cursors.json"),
        )


def test_cursor_store_survives_reload(tmp_path: Path) -> None:
    path = tmp_path / "cursors.json"
    ing.CursorStore(path).save("taste", "install-1", 42)
    reloaded = ing.CursorStore(path)
    assert reloaded.load("taste", "install-1") == 42
    assert reloaded.load("insights", "install-1") is None
    assert reloaded.load("taste", "install-2") is None
