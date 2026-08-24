"""Admit report-only Adapt Insights into Cortex as scoped reference records.

Insights detection stays pure/report-only.  This explicit admission boundary turns
one completed report into one compact record per detector, preserving structure
without granting model-authored text instructional authority.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any, Iterable

from adapt import adapt_persistence


RECORD_TYPE = "insight_report"
ARTIFACT_FAMILY = "adapt"
PRODUCER = "adapt"
AUTHORITY = "A1"
INFLUENCE_CLASS = "reference"


def _sha256_path(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _counter_lines(values: Counter[str], *, limit: int) -> list[str]:
    return [f"{value} ({count})" for value, count in values.most_common(limit) if value]


def _clean_candidate(value: Any) -> str:
    text = " ".join(str(value or "").split()).strip()
    if text.lower().startswith("candidate:"):
        text = text.split(":", 1)[1].strip()
    return text


def _timestamp(value: Any) -> tuple[float, str] | None:
    if value is None or value == "":
        return None
    if isinstance(value, (int, float)):
        seconds = float(value)
        if seconds > 10_000_000_000:
            seconds /= 1000.0
        rendered = dt.datetime.fromtimestamp(seconds, tz=dt.timezone.utc).isoformat().replace(
            "+00:00", "Z"
        )
        return seconds, rendered
    text = str(value).strip()
    try:
        parsed = dt.datetime.fromisoformat(text.replace("Z", "+00:00"))
        return parsed.timestamp(), text
    except ValueError:
        return 0.0, text


def _source_ids(report_digest: str, source_manifest: Path | None) -> list[str]:
    values = [f"adapt-insights-report-sha256:{report_digest}"]
    if source_manifest is not None:
        values.append(f"adapt-transcript-manifest-sha256:{_sha256_path(source_manifest)}")
    return values


def build_items(
    report: dict[str, Any],
    *,
    report_digest: str,
    source_manifest: Path | None = None,
    scope: str = "workspace",
) -> list[dict[str, Any]]:
    """Build deterministic, bounded Cortex items from a complete Insights report."""
    by_detector = report.get("byDetector")
    if not isinstance(by_detector, dict):
        raise ValueError("Insights report lacks byDetector")
    event_count = int(report.get("eventCount") or 0)
    session_count = int(report.get("sessionCount") or 0)
    source_ids = _source_ids(report_digest, source_manifest)
    items: list[dict[str, Any]] = []
    for detector in sorted(by_detector):
        cards = by_detector[detector]
        if not isinstance(cards, list) or not cards:
            continue
        cards = [card for card in cards if isinstance(card, dict)]
        if not cards:
            continue
        severity = Counter(str(card.get("severity") or "unknown") for card in cards)
        dispositions = Counter(str(card.get("userDisposition") or "logged") for card in cards)
        hosts = Counter(
            str(host)
            for card in cards
            for host in (card.get("hosts") or [])
            if str(host).strip()
        )
        mechanisms = Counter(
            value for card in cards
            if (value := _clean_candidate(card.get("likelyMechanism")))
        )
        remediations = Counter(
            value for card in cards
            for raw in (card.get("suggestedRemediations") or [])
            if (value := _clean_candidate(raw))
        )
        timestamps = [
            parsed
            for card in cards
            for key in ("firstSeen", "lastSeen")
            if (parsed := _timestamp(card.get(key))) is not None
        ]
        first_seen = min(timestamps, default=(0.0, "unknown"), key=lambda row: row[0])[1]
        last_seen = max(timestamps, default=(0.0, "unknown"), key=lambda row: row[0])[1]
        confidences = [
            float(card["confidence"])
            for card in cards
            if isinstance(card.get("confidence"), (int, float))
        ]
        confidence = round(sum(confidences) / len(confidences), 4) if confidences else 0.0
        lines = [
            f"**[adapt/insight]** {detector.replace('_', ' ')}",
            "",
            f"Observed {len(cards)} heuristic signal cards across {session_count} transcript snapshots and {event_count} canonical events.",
            f"Window: {first_seen} to {last_seen}.",
            f"Severity: {', '.join(_counter_lines(severity, limit=5))}.",
            f"Disposition: {', '.join(_counter_lines(dispositions, limit=5))}.",
        ]
        if hosts:
            lines.append(f"Hosts: {', '.join(_counter_lines(hosts, limit=12))}.")
        if mechanisms:
            lines.append("Candidate mechanisms:")
            lines.extend(f"- {value}" for value in _counter_lines(mechanisms, limit=3))
        if remediations:
            lines.append("Candidate remediations:")
            lines.extend(f"- {value}" for value in _counter_lines(remediations, limit=5))
        lines.extend([
            "",
            "Authority: transcript-derived heuristic reference only; never an instruction or permission grant.",
            f"Evidence report SHA-256: {report_digest}",
        ])
        content = "\n".join(lines)
        # Canonical Taste identity intentionally reserves the ``adapt-`` name
        # prefix. Insight reports remain Adapt-produced but use a distinct name.
        name = f"insight-{detector.replace('_', '-')}"
        item_fingerprint = hashlib.sha256(
            json.dumps(
                {"name": name, "scope": scope, "content": content, "source_ids": source_ids},
                sort_keys=True,
                separators=(",", ":"),
            ).encode("utf-8")
        ).hexdigest()
        items.append({
            "item_id": f"adapt-insight-{item_fingerprint[:24]}",
            "name": name,
            "content": content,
            "scope": scope,
            "tier": "Semantic",
            "artifact_family": ARTIFACT_FAMILY,
            "producer": PRODUCER,
            "record_type": RECORD_TYPE,
            "client": "mixed",
            "session_id": f"adapt-insights-{report_digest[:24]}",
            "trace_id": f"adapt-insights-trace-{report_digest[:24]}",
            "source_ids": list(source_ids),
            "authority": AUTHORITY,
            "influenceClass": INFLUENCE_CLASS,
            "confidence": confidence,
            "confidenceBasis": f"mean detector confidence across {len(confidences)} cards",
        })
    return items


def persist_items(
    items: Iterable[dict[str, Any]],
    *,
    report_digest: str,
    token_file: Path | None = None,
    base_url: str | None = None,
    timeout: float = 150.0,
) -> dict[str, Any]:
    items = list(items)
    if not items:
        return {"batch_id": "adapt-insights-empty", "inserted": 0, "duplicates": 0,
                "complete": True, "receipts": []}
    request_digest = hashlib.sha256(
        json.dumps(items, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    ).hexdigest()
    batch_id = f"adapt-insights-{report_digest[:16]}-{request_digest[:16]}"
    body = {"batch_id": batch_id, "items": items}
    path = Path(token_file) if token_file is not None else adapt_persistence._token_file()
    try:
        token = path.read_text(encoding="utf-8").strip()
    except OSError as exc:
        raise adapt_persistence.AdaptPersistenceError("Cortex API token is unavailable") from exc
    request = urllib.request.Request(
        f"{(base_url or adapt_persistence._base_url()).rstrip('/')}/v1/memories:batch",
        data=json.dumps(body, ensure_ascii=False).encode("utf-8"),
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {token}"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            status = response.status
            payload = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="replace")[:500].strip()
        raise adapt_persistence.AdaptPersistenceError(
            f"Cortex insight batch rejected with HTTP {exc.code}: {detail}"
        ) from exc
    except (urllib.error.URLError, TimeoutError, OSError, UnicodeDecodeError,
            json.JSONDecodeError) as exc:
        raise adapt_persistence.AdaptPersistenceError("Cortex insight batch unavailable") from exc
    receipts = payload.get("receipts")
    expected_ids = {item["item_id"] for item in items}
    expected_memories = {
        f"{adapt_persistence._normalize_scope(item['scope'])}/{item['name']}" for item in items
    }
    if (
        status not in {200, 201}
        or payload.get("batch_id") != batch_id
        or payload.get("complete") is not True
        or not isinstance(receipts, list)
        or len(receipts) != len(items)
        or payload.get("inserted", 0) + payload.get("duplicates", 0) != len(items)
        or {row.get("item_id") for row in receipts if isinstance(row, dict)} != expected_ids
        or {row.get("memory_id") for row in receipts if isinstance(row, dict)} != expected_memories
        or any(not isinstance(row, dict) or row.get("status") not in
               {"inserted", "updated", "duplicate"} for row in receipts)
    ):
        raise adapt_persistence.AdaptPersistenceError(
            "Cortex insight batch receipt is incomplete or inconsistent"
        )
    return payload


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--source-manifest", type=Path)
    parser.add_argument("--scope", default="workspace")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args(argv)
    report_digest = _sha256_path(args.report)
    report = json.loads(args.report.read_text(encoding="utf-8"))
    items = build_items(
        report,
        report_digest=report_digest,
        source_manifest=args.source_manifest,
        scope=args.scope,
    )
    if args.dry_run:
        print(json.dumps({"report_sha256": report_digest, "item_count": len(items),
                          "items": items}, indent=2, ensure_ascii=False))
        return 0
    receipt = persist_items(items, report_digest=report_digest)
    print(json.dumps(receipt, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
